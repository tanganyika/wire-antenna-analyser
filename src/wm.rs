//! Bridge to the wiremom engine (../wiremom): MPIE with triangle basis functions and
//! Galerkin testing, perfect or Sommerfeld real ground, solved in-process on the
//! rayon pool.  The antenna is meshed with this program's own segmentation, so the
//! segment layout matches the MININEC engine; patterns, F/B and sweeps reuse the
//! shared code in `mom`.

use crate::model::*;
use crate::mom::{self, CalcResult, FullResult, Radiator, Solution};
use num_complex::Complex64 as C;
use std::sync::LazyLock;
use std::time::Instant;
use wiremom::{Conductor, Ground as WGround, LoadKind, Model, RealGround, Segments};

/// Sommerfeld tables shared by every calculation, so a sweep or an optimizer run
/// builds each table once (tables are reused within ±1 % of their frequency).
static TABLES: LazyLock<wiremom::TableCache> = LazyLock::new(Default::default);

/// Shown when wires connect to real ground.  Neither this engine nor NEC-2 can model
/// current flowing into lossy earth (NEC-2 gives meaningless impedances for it; it
/// needs NEC-4-style buried-wire fields), so, like the MININEC engine, the currents
/// and impedance are solved over perfect ground (the connection acting as a good
/// ground system) and only the radiation pattern includes the real ground.
pub const GROUND_CONNECTED_NOTE: &str = "Wires connect to the real ground: impedance and currents are calculated over perfect ground (as for a good radial system); the pattern includes the real ground's reflection. Ground losses at the connection are not included.";

/// Real ground with some wire end at ground level (after "add height").
pub fn touches_ground(ant: &Antenna) -> bool {
    let dz = ant.ground.add_height;
    ant.ground.kind == GroundKind::Real
        && ant.wires.iter().any(|w| w.length() > 0.0 && w.r != 0.0 && ((w.p1[2] + dz).abs() < 1e-6 || (w.p2[2] + dz).abs() < 1e-6))
}

/// Far-field current elements per segment (Gauss points).
const ELEMENTS_PER_SEG: usize = 6;

/// The wiremom model of `ant`, segmented for wavelength `lambda`, with loads
/// evaluated at `freq_mhz`.
pub fn build(ant: &Antenna, lambda: f64, freq_mhz: f64) -> Result<Model, String> {
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let conductor = match ant.material.conduction(ant.user_rho, ant.user_mu) {
        Some((sigma, mu_r)) => Conductor::Lossy { sigma, mu_r },
        None => Conductor::Perfect,
    };
    let mut wires = vec![];
    // app wire index -> wiremom wire index (zero-length / zero-radius wires are skipped,
    // as the MININEC engine does)
    let mut index = vec![None; ant.wires.len()];
    for (wi, w) in ant.wires.iter().enumerate() {
        let p1 = [w.p1[0], w.p1[1], w.p1[2] + dz];
        let p2 = [w.p2[0], w.p2[1], w.p2[2] + dz];
        let len = w.length();
        if !(len > 0.0) || !(w.r.abs() > 0.0) {
            continue;
        }
        if grounded && (p1[2] < -1e-6 || p2[2] < -1e-6) {
            return Err(format!("Antenna or part of antenna is under ground. Increase height! (wire {})", wi + 1));
        }
        let lens = mom::wire_segment_lengths(len, lambda, w.seg, w.r, &ant.seg);
        index[wi] = Some(wires.len());
        wires.push(wiremom::Wire {
            start: p1,
            end: p2,
            radius: w.r.abs(),
            segments: Segments::Lengths(lens),
            conductor,
            series_l_per_m: w.insulation_l_per_m(),
        });
    }
    let ground = match ant.ground.kind {
        GroundKind::Free => WGround::Free,
        GroundKind::Perfect => WGround::Perfect,
        // wires connected to real ground: see GROUND_CONNECTED_NOTE
        GroundKind::Real if touches_ground(ant) => WGround::Perfect,
        GroundKind::Real => WGround::Real(RealGround { eps_r: ant.ground.eps_r, sigma: ant.ground.sigma_ms * 1e-3 }),
    };
    let mut m = Model::new(wires, ground);
    m.tables = TABLES.clone();
    m.ground_table_reuse = 0.01;
    let mesh = m.mesh().map_err(String::from)?;
    let wire_of = |w: usize, what: &str| index.get(w).copied().flatten().ok_or_else(|| format!("{what} is on wire {}, which has no length or radius", w + 1));
    for s in &ant.sources {
        let w = wire_of(s.wire, "a source")?;
        m.add_source(w, mesh.nearest_node(w, s.pos), C::from_polar(s.volts, s.phase_deg.to_radians()));
    }
    for l in &ant.loads {
        let w = wire_of(l.wire, "a load")?;
        m.add_load(w, mesh.nearest_node(w, l.pos), LoadKind::Impedance(l.impedance(freq_mhz * 1e6)));
    }
    Ok(m)
}

/// Build the Sommerfeld tables a sweep over `freqs` (MHz) will need, in parallel,
/// so the per-frequency solves all find them ready.
pub fn prepare_sweep(ant: &Antenna, freqs_mhz: &[f64], seg_freq_mhz: f64) {
    if ant.ground.kind != GroundKind::Real {
        return;
    }
    if let Ok(m) = build(ant, 299.792458 / seg_freq_mhz, seg_freq_mhz) {
        let hz: Vec<f64> = freqs_mhz.iter().map(|f| f * 1e6).collect();
        // errors (e.g. wires under ground) are reported by the solves themselves
        let _ = TABLES.prepare(&m, &hz);
    }
}

/// Solve at one frequency; returns the summary, the solution in this program's form,
/// the radiator (when `need_far`) and the engine's own solution.
pub fn quick(ant: &Antenna, freq_mhz: f64, seg_freq_mhz: f64, need_far: bool) -> Result<(CalcResult, Solution, Option<Radiator>, wiremom::Solution), String> {
    let t0 = Instant::now();
    if ant.sources.is_empty() {
        return Err("The antenna has no source.".into());
    }
    let model = build(ant, 299.792458 / seg_freq_mhz, freq_mhz)?;
    let ws = model.solve(freq_mhz * 1e6).map_err(String::from)?;
    let sol = Solution {
        freq_mhz,
        currents: vec![],
        z_in: ws.source_impedances(),
        p_in: ws.input_power(),
        n_unknowns: ws.mesh.n_unknowns(),
        n_segs: ws.mesh.n_segments(),
        t_fill_ms: ws.t_fill_ms,
        t_solve_ms: ws.t_solve_ms,
    };
    let rad = need_far.then(|| Radiator::from_elements(ws.current_elements(ELEMENTS_PER_SEG), freq_mhz, &ant.ground, sol.p_in));
    let res = mom::summarize(ant, freq_mhz, t0, &sol, rad.as_ref());
    Ok((res, sol, rad, ws))
}

/// Full calculation for the Calculate tab.
pub fn calculate(ant: &Antenna) -> Result<FullResult, String> {
    let (mut res, _, rad, ws) = quick(ant, ant.freq_mhz, ant.freq_mhz, true)?;
    let patterns = mom::patterns(ant, &mut res, &rad.unwrap());
    // the MININEC mesh carries the segmentation warnings and the 3D view's layout
    let mesh = mom::build_mesh(ant, ant.lambda())?;
    let mut warnings = mesh.warnings.clone();
    if touches_ground(ant) {
        warnings.push(GROUND_CONNECTED_NOTE.to_string());
    }
    let seg_currents = ws.segment_currents().into_iter().map(|(a, b, i)| (a, b, i.norm())).collect();
    Ok(FullResult { result: res, patterns, mesh, currents: vec![], warnings, seg_currents: Some(seg_currents) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(i: usize, e: Engine) -> Antenna {
        let mut a = Antenna::example(i);
        a.engine = e;
        a
    }

    fn nec2(a: &Antenna) -> CalcResult {
        let mut n = a.clone();
        n.engine = Engine::Nec2;
        mom::calculate(&n).unwrap().result
    }

    #[test]
    fn examples_agree_with_nec2_and_mininec() {
        for i in 0..Antenna::example_names().len() {
            let w = mom::calculate(&with(i, Engine::Wiremom)).unwrap().result;
            let m = mom::calculate(&with(i, Engine::Mininec)).unwrap().result;
            let name = Antenna::example(i).name;
            print!("{name}: wiremom Z={:.1} Ga={:.2} FB={:.1} ({:.0} ms) | MININEC Z={:.1} Ga={:.2} FB={:.1}", w.z, w.ga_dbi, w.fb_db, w.t_total_ms, m.z, m.ga_dbi, m.fb_db);
            if crate::nec::available() {
                let n = nec2(&with(i, Engine::Nec2));
                println!(" | NEC-2 Z={:.1} Ga={:.2} FB={:.1}", n.z, n.ga_dbi, n.fb_db);
                assert!((w.ga_dbi - n.ga_dbi).abs() < 0.3, "{name}: gain {} vs NEC-2 {}", w.ga_dbi, n.ga_dbi);
                assert!((w.z - n.z).norm() < 0.08 * n.z.norm() + 2.0, "{name}: Z {} vs NEC-2 {}", w.z, n.z);
            } else {
                println!();
            }
            assert!((w.ga_dbi - m.ga_dbi).abs() < 1.5);
        }
    }

    #[test]
    fn real_ground_matches_nec2() {
        if !crate::nec::available() {
            return;
        }
        for i in 0..Antenna::example_names().len() {
            let mut a = with(i, Engine::Wiremom);
            if a.ground.kind == GroundKind::Free {
                continue;
            }
            a.ground.kind = GroundKind::Real;
            a.ground.eps_r = 13.0;
            a.ground.sigma_ms = 5.0;
            if touches_ground(&a) {
                continue; // NEC-2 can't model a connection into lossy ground (see GROUND_CONNECTED_NOTE)
            }
            let w = mom::calculate(&a);
            let n = nec2(&a);
            let w = w.unwrap().result;
            println!("{} over average ground: wiremom Z={:.1} Ga={:.2} el={:.0} | NEC-2 Z={:.1} Ga={:.2} el={:.0}", a.name, w.z, w.ga_dbi, w.el_max, n.z, n.ga_dbi, n.el_max);
            assert!((w.ga_dbi - n.ga_dbi).abs() < 0.3, "{}", a.name);
            assert!((w.z - n.z).norm() < 0.08 * n.z.norm() + 2.0, "{}", a.name);
        }
    }

    #[test]
    fn ground_connected_vertical_over_real_ground() {
        // impedance as over perfect ground, pattern with real-ground reflection: the
        // same model as the MININEC engine, so the two must agree closely
        let mut a = with(3, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        assert!(touches_ground(&a));
        let w = mom::calculate(&a).unwrap();
        assert!(w.warnings.iter().any(|s| s == GROUND_CONNECTED_NOTE));
        let mut p = a.clone();
        p.ground.kind = GroundKind::Perfect;
        let zp = mom::calculate(&p).unwrap().result.z;
        assert!((w.result.z - zp).norm() < 1e-9 * zp.norm());
        a.engine = Engine::Mininec;
        let m = mom::calculate(&a).unwrap().result;
        println!("vertical on average ground: wiremom Ga={:.2} el={} | MININEC Ga={:.2} el={}", w.result.ga_dbi, w.result.el_max, m.ga_dbi, m.el_max);
        assert!((w.result.ga_dbi - m.ga_dbi).abs() < 0.1);
        // real ground pulls the lobe up off the horizon and costs gain
        assert!(w.result.el_max > 5.0 && w.result.ga_dbi < 2.0);
    }

    #[test]
    fn sweep_runs_in_parallel() {
        let mut a = with(2, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        if a.ground.add_height <= 0.0 {
            a.ground.add_height = 10.0;
        }
        let r = mom::sweep(&a, 13.8, 14.5, 24, true).unwrap();
        println!("wiremom real-ground sweep, 24 freqs: {:.0} ms on {} threads", r.t_ms, r.threads);
        assert_eq!(r.points.len(), 24);
        assert!(r.points.iter().all(|p| p.z.re > 0.0 && p.ga_dbi.is_finite()));
    }
}
