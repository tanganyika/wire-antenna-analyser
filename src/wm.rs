//! Bridge to the wiremom engine: MPIE with triangle basis functions and Galerkin
//! testing, perfect or Sommerfeld real ground (wires may also run in or through the
//! ground), solved in-process on the rayon pool.  The antenna is meshed with this
//! program's own segmentation; patterns, F/B and sweeps reuse the shared code in
//! `mom`, with the gain from wiremom's own far field.

use crate::model::*;
use crate::mom::{self, CalcResult, FullResult, Gain, Radiator, Solution};
use num_complex::Complex64 as C;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use wiremom::{Conductor, Ground as WGround, LoadKind, Model, Port, RealGround, Segments, TwoPortKind};

/// Sommerfeld tables shared by every calculation, so a sweep or an optimizer run
/// builds each table once (tables are reused within ±1 % of their frequency).
static TABLES: LazyLock<wiremom::TableCache> = LazyLock::new(Default::default);

/// Shown when wires stop at the surface of real ground with nothing below it.  A
/// point contact into lossy earth can't be modelled (and NEC-2 gives meaningless
/// impedances for it), so, like the MININEC engine, the currents and impedance are
/// solved over perfect ground (the connection acting as a good ground system) and
/// only the radiation pattern includes the real ground.  Antennas with wires in the
/// ground (rods, buried radials) are modelled in full instead.
pub const GROUND_CONNECTED_NOTE: &str = "Wires connect to the real ground: impedance and currents are calculated over perfect ground (as for a good radial system); the pattern includes the real ground's reflection. Ground losses at the connection are not included. To model the ground system itself, add ground rods or buried radials (wires below z = 0).";

fn usable(w: &Wire) -> bool {
    w.length() > 0.0 && w.r != 0.0
}

/// Real ground with some wire end at ground level (after "add height").
pub fn touches_ground(ant: &Antenna) -> bool {
    let dz = ant.ground.add_height;
    ant.ground.kind == GroundKind::Real && ant.wires.iter().any(|w| usable(w) && ((w.p1[2] + dz).abs() < 1e-6 || (w.p2[2] + dz).abs() < 1e-6))
}

/// Real ground with wires (partly) below the surface: modelled in full by wiremom.
pub fn has_buried(ant: &Antenna) -> bool {
    let dz = ant.ground.add_height;
    ant.ground.kind == GroundKind::Real && ant.wires.iter().any(|w| usable(w) && (w.p1[2] + dz < -1e-6 || w.p2[2] + dz < -1e-6))
}

/// The perfect-ground stand-in for wires that stop on real ground (see
/// GROUND_CONNECTED_NOTE) applies.
pub fn uses_ground_stand_in(ant: &Antenna) -> bool {
    touches_ground(ant) && !has_buried(ant)
}

/// The wiremom model of `ant`, segmented for wavelength `lambda`, with loads
/// evaluated at `freq_mhz`; also the app wire index of each wiremom wire.
pub fn build(ant: &Antenna, lambda: f64, freq_mhz: f64) -> Result<(Model, Vec<usize>), String> {
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
    let mut app_wire = vec![];
    for (wi, w) in ant.wires.iter().enumerate() {
        let p1 = [w.p1[0], w.p1[1], w.p1[2] + dz];
        let p2 = [w.p2[0], w.p2[1], w.p2[2] + dz];
        let len = w.length();
        if !(len > 0.0) || !(w.r.abs() > 0.0) {
            continue;
        }
        // only real ground can have wires in it
        if ant.ground.kind == GroundKind::Perfect && (p1[2] < -1e-6 || p2[2] < -1e-6) {
            return Err(format!("Antenna or part of antenna is under ground. Increase height! (wire {})", wi + 1));
        }
        let lens = mom::wire_segment_lengths(len, lambda, w.seg, w.r, &ant.seg);
        index[wi] = Some(wires.len());
        app_wire.push(wi);
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
        GroundKind::Real if uses_ground_stand_in(ant) => WGround::Perfect,
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
    for t in &ant.lines {
        let (wa, wb) = (wire_of(t.a_wire, "a transmission line")?, wire_of(t.b_wire, "a transmission line")?);
        let kind = TwoPortKind::Line {
            z0: t.z0.abs(),
            length: (t.length > 0.0).then_some(t.length),
            velocity_factor: t.vf,
            loss_db_per_100m: t.loss_db,
            crossed: t.reversed,
            shunt: [C::new(0.0, 0.0); 2],
        };
        m.add_network(Port { wire: wa, node: mesh.nearest_node(wa, t.a_pos) }, Port { wire: wb, node: mesh.nearest_node(wb, t.b_pos) }, kind);
    }
    Ok((m, app_wire))
}

/// Build the Sommerfeld tables a sweep over `freqs` (MHz) will need, in parallel,
/// so the per-frequency solves all find them ready.
pub fn prepare_sweep(ant: &Antenna, freqs_mhz: &[f64], seg_freq_mhz: f64) {
    if ant.ground.kind != GroundKind::Real {
        return;
    }
    if let Ok((m, _)) = build(ant, 299.792458 / seg_freq_mhz, seg_freq_mhz) {
        let hz: Vec<f64> = freqs_mhz.iter().map(|f| f * 1e6).collect();
        // errors (e.g. wires under ground) are reported by the solves themselves
        let _ = TABLES.prepare(&m, &hz);
    }
}

/// Solve at one frequency; returns the summary, the solution in this program's form,
/// the radiator (when `need_far`), the engine's own solution and the app wire index
/// of each of its wires.
pub fn quick(ant: &Antenna, freq_mhz: f64, seg_freq_mhz: f64, need_far: bool) -> Result<(CalcResult, Solution, Option<Radiator>, Arc<wiremom::Solution>, Vec<usize>), String> {
    let t0 = Instant::now();
    if ant.sources.is_empty() {
        return Err("The antenna has no source.".into());
    }
    let (model, app_wire) = build(ant, 299.792458 / seg_freq_mhz, freq_mhz)?;
    let mut ws = model.solve(freq_mhz * 1e6).map_err(String::from)?;
    if uses_ground_stand_in(ant) {
        // currents over perfect ground, pattern with the real ground's reflection
        ws.ground = WGround::Real(RealGround { eps_r: ant.ground.eps_r, sigma: ant.ground.sigma_ms * 1e-3 });
    }
    let ws = Arc::new(ws);
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
    let rad = need_far.then(|| {
        // wiremom's own far field: it also covers currents in the ground
        let w = ws.clone();
        let gain: mom::GainFn = Arc::new(move |az: f64, el: f64| {
            let g = w.gain((90.0 - el).to_radians(), az.to_radians());
            Gain { total: g.total, hor: g.phi, ver: g.theta }
        });
        Radiator::from_fn(freq_mhz, &ant.ground, gain)
    });
    let res = mom::summarize(ant, freq_mhz, t0, &sol, rad.as_ref());
    Ok((res, sol, rad, ws, app_wire))
}

/// Full calculation for the Calculate tab.
pub fn calculate(ant: &Antenna) -> Result<FullResult, String> {
    let (mut res, _, rad, ws, app_wire) = quick(ant, ant.freq_mhz, ant.freq_mhz, true)?;
    let patterns = mom::patterns(ant, &mut res, &rad.unwrap());
    // segmentation warnings from this program's mesher (it refuses buried wires,
    // which only this engine models)
    let mut warnings = mom::build_mesh(ant, ant.lambda()).map(|m| m.warnings).unwrap_or_default();
    warnings.extend(ws.warnings.iter().cloned());
    if uses_ground_stand_in(ant) {
        warnings.push(GROUND_CONNECTED_NOTE.to_string());
    }
    let mesh = view_mesh(ant, &ws, &app_wire);
    let seg_currents = ws.segment_currents().into_iter().map(|(a, b, i)| (a, b, i.norm())).collect();
    Ok(FullResult { result: res, patterns, mesh, currents: vec![], warnings, seg_currents: Some(seg_currents) })
}

/// The segment layout for the 3D view, from wiremom's mesh, one entry per app wire.
fn view_mesh(ant: &Antenna, ws: &wiremom::Solution, app_wire: &[usize]) -> mom::Mesh {
    let m = &ws.mesh;
    let mut segs = vec![];
    let mut wire_segs = vec![(0, 0); ant.wires.len()];
    for (wi, list) in m.wire_segs.iter().enumerate() {
        let aw = app_wire[wi];
        wire_segs[aw] = (segs.len(), list.len());
        for &s in list {
            segs.push(mom::Seg { a: m.p0[s], b: m.p1[s], r: m.radius[s], wire: aw });
        }
    }
    mom::Mesh { segs, bases: vec![], wire_gaps: vec![], wire_segs, warnings: vec![], image: false }
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
        assert!(uses_ground_stand_in(&a));
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
    fn vertical_with_rod_and_buried_radials() {
        // the quarter-wave vertical fed against a 1 m rod and 8 buried radials over
        // average ground: modelled in full (no stand-in), with ground loss
        let mut a = with(3, Engine::Wiremom);
        a.ground.kind = GroundKind::Real;
        a.ground.eps_r = 13.0;
        a.ground.sigma_ms = 5.0;
        a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 0.005));
        for i in 0..8 {
            let t = 2.0 * std::f64::consts::PI * i as f64 / 8.0;
            a.wires.push(Wire::new([0.0, 0.0, -0.1], [8.0 * t.cos(), 8.0 * t.sin(), -0.1], 0.002));
        }
        // radials join the rod 0.1 m down
        a.wires[1] = Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, -0.1], 0.005);
        a.wires.insert(2, Wire::new([0.0, 0.0, -0.1], [0.0, 0.0, -1.0], 0.005));
        assert!(has_buried(&a) && !uses_ground_stand_in(&a));
        let w = mom::calculate(&a).unwrap();
        let r = &w.result;
        println!("vertical + rod + 8 buried radials: Z={:.1} Ga={:.2} el={} warnings {:?}", r.z, r.ga_dbi, r.el_max, w.warnings);
        assert!(!w.warnings.iter().any(|s| s == GROUND_CONNECTED_NOTE));
        // ground loss: well below the perfect-ground 5.1 dBi, lobe off the horizon
        assert!(r.ga_dbi < 2.0 && r.ga_dbi > -6.0, "{}", r.ga_dbi);
        assert!(r.z.re > 30.0 && r.z.re < 120.0, "{}", r.z);
        // the 3D view gets every wire's segments
        assert_eq!(w.mesh.wire_segs.len(), a.wires.len());
        assert!(w.mesh.wire_segs.iter().all(|&(_, n)| n > 0));
    }

    #[test]
    fn dipole_fed_through_lines_matches_nec2() {
        // the half-wave dipole fed from a short feed wire through 75 ohm lines
        for (len, vf) in [(5.3, 1.0), (9.1, 0.66)] {
            let mut a = with(0, Engine::Wiremom);
            a.wires.push(Wire::new([0.0, 0.0, 20.0], [0.0, 0.0, 20.05], 0.001));
            a.wires[1].seg = 3;
            a.sources = vec![Source { wire: 1, pos: 0.5, phase_deg: 0.0, volts: 1.0 }];
            let mut t = TLine::new(1, 0.5, 0, 0.5);
            t.z0 = 75.0;
            t.length = len;
            t.vf = vf;
            a.lines.push(t);
            let w = mom::calculate(&a).unwrap().result;
            let mut d = with(0, Engine::Wiremom);
            d.material = a.material;
            let zd = mom::calculate(&d).unwrap().result.z;
            let bl = 2.0 * std::f64::consts::PI * len / (vf * a.lambda());
            let want = 75.0 * (zd + C::new(0.0, 75.0 * bl.tan())) / (75.0 + C::new(0.0, 1.0) * zd * bl.tan());
            print!("line {len} m vf {vf}: wiremom {:.2}, line formula {:.2}", w.z, want);
            assert!((w.z - want).norm() < 0.02 * want.norm());
            if crate::nec::available() {
                a.engine = Engine::Nec2;
                let n = mom::calculate(&a).unwrap().result;
                print!(", NEC-2 {:.2}", n.z);
                assert!((w.z - n.z).norm() < 0.05 * n.z.norm(), "{} vs {}", w.z, n.z);
            }
            println!();
        }
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
