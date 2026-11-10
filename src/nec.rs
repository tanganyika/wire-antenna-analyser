//! NEC-2 engine: the bundled public-domain nec2c (compiled by build.rs) run as a separate
//! process. Each calculation writes a NEC card deck, runs nec2c and reads its report.
//! Sweeps and optimizer trials run many nec2c processes at once on the rayon thread pool.

use crate::model::*;
use crate::mom::{self, CalcResult, FullResult, Gain, Patterns};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static NEC2C: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/nec2c"));

pub fn available() -> bool {
    !NEC2C.is_empty()
}

fn binary() -> Result<PathBuf, String> {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    PATH.get_or_init(|| {
        if NEC2C.is_empty() {
            return Err("The NEC-2 engine wasn't built into this copy of the program.".into());
        }
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        let mut h: u64 = 1469598103934665603;
        for b in NEC2C.iter().step_by(97) {
            h = (h ^ *b as u64).wrapping_mul(1099511628211);
        }
        let dir = base.join("wireant");
        let path = dir.join(format!("nec2c-{:x}-{}", h, NEC2C.len()));
        if std::fs::metadata(&path).map(|m| m.len() as usize == NEC2C.len()).unwrap_or(false) {
            return Ok(path);
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("Can't prepare the NEC-2 engine: {e}"))?;
        let tmp = dir.join(format!("nec2c-{}.part", std::process::id()));
        std::fs::write(&tmp, NEC2C).map_err(|e| format!("Can't prepare the NEC-2 engine: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&tmp, &path).map_err(|e| format!("Can't prepare the NEC-2 engine: {e}"))?;
        Ok(path)
    })
    .clone()
}

/// One GW card: part of an original wire with equal segments.
#[derive(Clone, Debug)]
struct Piece {
    wire: usize,
    tag: usize,
    nseg: usize,
    a: [f64; 3],
    b: [f64; 3],
}

struct Layout {
    pieces: Vec<Piece>,
    /// per original wire: indices into `pieces`
    by_wire: Vec<Vec<usize>>,
    total_segs: usize,
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Same segmentation as the built-in solver, grouped into runs of equal segments (one GW each).
/// A centre feed or load that would sit on a segment boundary gets an odd middle run instead,
/// because NEC puts sources at segment centres.
fn layout(ant: &Antenna, lambda: f64) -> Layout {
    let dz = if ant.ground.kind != GroundKind::Free { ant.ground.add_height } else { 0.0 };
    let mut pieces = Vec::new();
    let mut by_wire = vec![Vec::new(); ant.wires.len()];
    let mut total = 0;
    for (wi, w) in ant.wires.iter().enumerate() {
        let p1 = [w.p1[0], w.p1[1], w.p1[2] + dz];
        let p2 = [w.p2[0], w.p2[1], w.p2[2] + dz];
        let len = w.length();
        if len <= 0.0 || w.r == 0.0 {
            continue;
        }
        // Automatic segmentation (seg < 0) is MMANA's taper: very short segments at the
        // wire ends.  NEC-2 is inaccurate with a feed on such a segment (the quarter-wave
        // vertical example came out 0.25 dB above what a lossless one can reach), so
        // NEC-2 gets uniform segments at the taper's long-segment density (the seg = 0
        // rule, lambda/DM2) instead.  Explicit counts are kept.
        let seg = if w.seg < 0 { 0 } else { w.seg };
        let lens = mom::wire_segment_lengths(len, lambda, seg, w.r, &ant.seg);
        // group equal lengths
        let mut runs: Vec<(usize, f64)> = Vec::new();
        for l in lens {
            match runs.last_mut() {
                Some((n, ll)) if (*ll - l).abs() <= 1e-9 * l.max(1e-12) => *n += 1,
                _ => runs.push((1, l)),
            }
        }
        let feeds: Vec<f64> = ant
            .sources
            .iter()
            .filter(|s| s.wire == wi)
            .map(|s| s.pos)
            .chain(ant.loads.iter().filter(|l| l.wire == wi).map(|l| l.pos))
            .chain(ant.lines.iter().flat_map(|t| [(t.a_wire, t.a_pos), (t.b_wire, t.b_pos)]).filter(|e| e.0 == wi).map(|e| e.1))
            .filter(|p| *p > 1e-9 && *p < 1.0 - 1e-9)
            .collect();
        let mut s0 = 0.0;
        for run in runs.iter_mut() {
            let run_len = run.0 as f64 * run.1;
            for f in &feeds {
                let d = f * len;
                if d > s0 && d < s0 + run_len {
                    let k = (d - s0) / run.1;
                    if (k - k.round()).abs() < 1e-6 {
                        run.0 += 1;
                        run.1 = run_len / run.0 as f64;
                    }
                }
            }
            s0 += run_len;
        }
        let mut s = 0.0;
        for (n, l) in runs {
            let t0 = s / len;
            s += n as f64 * l;
            let t1 = (s / len).min(1.0);
            let tag = pieces.len() + 1;
            by_wire[wi].push(pieces.len());
            pieces.push(Piece { wire: wi, tag, nseg: n, a: lerp(p1, p2, t0), b: lerp(p1, p2, t1) });
            total += n;
        }
    }
    Layout { pieces, by_wire, total_segs: total }
}

/// (tag, segment) of the segment whose centre is closest to `pos` along wire `wire`.
fn locate(ant: &Antenna, lay: &Layout, wire: usize, pos: f64) -> Option<(usize, usize)> {
    let w = ant.wires.get(wire)?;
    let target = lerp(w.p1, w.p2, pos);
    let dz = if ant.ground.kind != GroundKind::Free { ant.ground.add_height } else { 0.0 };
    let target = [target[0], target[1], target[2] + dz];
    let mut best: Option<(f64, usize, usize)> = None;
    for &pi in lay.by_wire.get(wire)? {
        let p = &lay.pieces[pi];
        for k in 0..p.nseg {
            let c = lerp(p.a, p.b, (k as f64 + 0.5) / p.nseg as f64);
            let d = (0..3).map(|i| (c[i] - target[i]).powi(2)).sum::<f64>();
            if best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, p.tag, k + 1));
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

fn num(x: f64) -> String {
    format!("{:.7E}", x)
}

fn deck(ant: &Antenna, lay: &Layout, freq_mhz: f64, rps: &[String]) -> Result<String, String> {
    if !ant.plates.is_empty() {
        // NEC-2 patches only model closed surfaces (MFIE)
        return Err("Plates need the wiremom engine: NEC-2's patches only model closed surfaces, not thin plates.".into());
    }
    if lay.pieces.is_empty() {
        return Err("There are no wires.".into());
    }
    if ant.sources.is_empty() {
        return Err("There is no source.".into());
    }
    let grounded = ant.ground.kind != GroundKind::Free;
    // over ground, ends a hair below z = 0 sit on the ground plane
    let zc = |z: f64| if grounded && z < 0.0 { 0.0 } else { z };
    let mut d = String::from("CM generated by Wire Antenna Analyser\nCE\n");
    for p in &lay.pieces {
        if grounded && (p.a[2] < -1e-6 || p.b[2] < -1e-6) {
            return Err(format!("Antenna or part of antenna is under ground. Increase height! (wire {})", p.wire + 1));
        }
        let r = ant.wires[p.wire].r.abs();
        d += &format!(
            "GW {} {} {} {} {} {} {} {} {}\n",
            p.tag, p.nseg, num(p.a[0]), num(p.a[1]), num(zc(p.a[2])), num(p.b[0]), num(p.b[1]), num(zc(p.b[2])), num(r)
        );
    }
    d += if grounded { "GE 1\n" } else { "GE 0\n" };
    match ant.ground.kind {
        GroundKind::Free => {}
        GroundKind::Perfect => d += "GN 1\n",
        GroundKind::Real => d += &format!("GN 2 0 0 0 {} {}\n", num(ant.ground.eps_r), num(ant.ground.sigma_ms * 1e-3)),
    }
    if let Some((sig, mu_r)) = ant.material.conduction(ant.user_rho, ant.user_mu) {
        // NEC has no permeability: the same skin-effect loss needs σ/μr
        d += &format!("LD 5 0 0 0 {}\n", num(sig / mu_r));
    }
    // insulated wires: distributed series inductance per metre (LD 2), same model as the built-in solver
    for p in &lay.pieces {
        let lp = ant.wires[p.wire].insulation_l_per_m();
        if lp > 0.0 {
            d += &format!("LD 2 {} 0 0 0 {} 0\n", p.tag, num(lp));
        }
    }
    for l in &ant.loads {
        let (tag, seg) = locate(ant, lay, l.wire, l.pos).ok_or_else(|| format!("Load on wire {} has no place to sit.", l.wire + 1))?;
        let z = l.impedance(freq_mhz * 1e6);
        d += &format!("LD 4 {} {} {} {} {}\n", tag, seg, seg, num(z.re), num(z.im));
    }
    // transmission lines: NEC-2 lines travel at c, so a velocity factor becomes a
    // longer electrical length; negative Z0 = reversed
    for t in &ant.lines {
        let (ta, sa) = locate(ant, lay, t.a_wire, t.a_pos).ok_or_else(|| format!("Transmission line end on wire {} has no place to sit.", t.a_wire + 1))?;
        let (tb, sb) = locate(ant, lay, t.b_wire, t.b_pos).ok_or_else(|| format!("Transmission line end on wire {} has no place to sit.", t.b_wire + 1))?;
        let at = |w: usize, f: f64| -> [f64; 3] {
            let w = &ant.wires[w];
            [0, 1, 2].map(|i| w.p1[i] + (w.p2[i] - w.p1[i]) * f)
        };
        let len = if t.length > 0.0 {
            t.length
        } else {
            let (p, q) = (at(t.a_wire, t.a_pos), at(t.b_wire, t.b_pos));
            (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f64>().sqrt()
        };
        let z0 = if t.reversed { -t.z0.abs() } else { t.z0.abs() };
        d += &format!("TL {} {} {} {} {} {} 0 0 0 0\n", ta, sa, tb, sb, num(z0), num(len / t.vf.max(1e-6)));
    }
    for s in &ant.sources {
        let (tag, seg) = locate(ant, lay, s.wire, s.pos).ok_or_else(|| format!("Source on wire {} has no place to sit.", s.wire + 1))?;
        let v = num_complex::Complex64::from_polar(s.volts, s.phase_deg.to_radians());
        d += &format!("EX 0 {} {} 0 {} {}\n", tag, seg, num(v.re), num(v.im));
    }
    d += &format!("FR 0 1 0 0 {} 0\n", num(freq_mhz));
    if rps.is_empty() {
        d += "XQ\n";
    }
    for rp in rps {
        d += rp;
        d += "\n";
    }
    d += "EN\n";
    Ok(d)
}

fn run(deck: &str) -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let bin = binary()?;
    let dir = std::env::temp_dir().join(format!("wireant-nec-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let inp = dir.join("in.nec");
    let out = dir.join("out.txt");
    std::fs::write(&inp, deck).map_err(|e| e.to_string())?;
    let res = std::process::Command::new(&bin)
        .arg(format!("-i{}", inp.display()))
        .arg(format!("-o{}", out.display()))
        .stdin(std::process::Stdio::null())
        .output();
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&inp);
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_dir(&dir);
    let o = res.map_err(|e| format!("Couldn't start the NEC-2 engine: {e}"))?;
    if !o.status.success() || !text.contains("ANTENNA INPUT PARAMETERS") {
        let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
        let tail: String = text.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        return Err(format!("NEC-2 stopped: {}", if msg.is_empty() { tail } else { msg }));
    }
    Ok(text)
}

/// All numbers on a line (handles fields that touch, like "1.0E+00-2.0E-01").
fn numbers(line: &str) -> Vec<f64> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || ((c == b'-' || c == b'+' || c == b'.') && i + 1 < b.len() && (b[i + 1].is_ascii_digit() || b[i + 1] == b'.')) {
            let st = i;
            i += 1;
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() || d == b'.' {
                    i += 1;
                } else if (d == b'E' || d == b'e') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = line[st..i].parse::<f64>() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Rows of a report table that starts `skip` lines after a heading line.
fn table<'a>(text: &'a str, heading: &str, skip: usize) -> Vec<Vec<&'a str>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains(heading) {
            let mut rows = Vec::new();
            let mut j = i + 1 + skip;
            while j < lines.len() && !lines[j].trim().is_empty() {
                rows.push(lines[j]);
                j += 1;
            }
            blocks.push(rows);
            i = j;
        } else {
            i += 1;
        }
    }
    blocks
}

struct Report {
    z: num_complex::Complex64,
    seg_cur: Vec<f64>,
    /// (theta, phi, vert dB, hor dB, total dB)
    pattern: Vec<[f64; 5]>,
}

fn parse(text: &str) -> Result<Report, String> {
    let inp = table(text, "ANTENNA INPUT PARAMETERS", 2);
    let first = inp.first().and_then(|b| b.first()).ok_or("NEC-2 gave no input impedance.")?;
    let n = numbers(first);
    if n.len() < 8 {
        return Err("NEC-2 input impedance couldn't be read.".into());
    }
    let z = num_complex::Complex64::new(n[6], n[7]);
    let seg_cur = table(text, "CURRENTS AND LOCATION", 4)
        .first()
        .map(|rows| rows.iter().map(|r| numbers(r)).filter(|v| v.len() >= 10).map(|v| v[8]).collect())
        .unwrap_or_default();
    let mut pattern = Vec::new();
    for block in table(text, "RADIATION PATTERNS", 4) {
        for r in block {
            let v = numbers(r);
            if v.len() >= 5 {
                pattern.push([v[0], v[1], v[2], v[3], v[4]]);
            }
        }
    }
    Ok(Report { z, seg_cur, pattern })
}

fn lin(db: f64) -> f64 {
    if db <= -999.0 { 0.0 } else { 10f64.powf(db / 10.0) }
}

fn gain_of(p: &[f64; 5]) -> Gain {
    Gain { total: lin(p[4]), hor: lin(p[3]), ver: lin(p[2]) }
}

/// Front gain, its direction and the rear-range F/B from a list of (az, el, gain).
fn front_back(pts: &[(f64, f64, Gain)], ant: &Antenna, grounded: bool) -> (f64, f64, Gain, f64) {
    let (az, el, g) = pts
        .iter()
        .cloned()
        .fold((0.0, 0.0, Gain { total: -1.0, ..Default::default() }), |b, x| if x.2.total > b.2.total { x } else { b });
    let back_az = (az + 180.0) % 360.0;
    let ang = |a: f64, b: f64| {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    let in_rear = |a: f64, e: f64| -> bool {
        if ant.fb_az <= 0.0 && ant.fb_el <= 0.0 {
            return ang(a, back_az) < 1.6 && (e - el).abs() < 1.6;
        }
        let el_ok = if grounded { e >= el.min(ant.fb_el) && e <= ant.fb_el.max(el) } else { (e - el).abs() <= ant.fb_el / 2.0 + 1e-9 };
        ang(a, back_az) <= ant.fb_az / 2.0 + 1e-9 && el_ok
    };
    let rear = pts.iter().filter(|p| in_rear(p.0, p.1)).map(|p| p.2.total).fold(0.0, f64::max);
    let fb = mom::db(g.total) - mom::db(rear);
    (az, el, g, fb)
}

fn grid_rp(grounded: bool) -> (String, Vec<f64>) {
    // theta from the zenith; 3° grid like the built-in 3D pattern
    let n_th = if grounded { 31 } else { 61 };
    (format!("RP 0 {} 120 1000 0 0 3 3", n_th), (0..n_th).map(|i| 90.0 - 3.0 * i as f64).collect())
}

fn result(ant: &Antenna, freq_mhz: f64, lay: &Layout, rep: &Report, need_far: bool, t_total: f64) -> (CalcResult, Vec<(f64, f64, Gain)>) {
    let grounded = ant.ground.kind != GroundKind::Free;
    let pts: Vec<(f64, f64, Gain)> = rep.pattern.iter().map(|p| (p[1].rem_euclid(360.0), 90.0 - p[0], gain_of(p))).collect();
    let (mut ga, mut fb, mut az, mut el, mut hp) = (f64::NAN, f64::NAN, 0.0, 0.0, true);
    if need_far && !pts.is_empty() {
        let (a, e, g, f) = front_back(&pts, ant, grounded);
        ga = mom::db(g.total);
        fb = f;
        az = a;
        el = e;
        hp = g.hor >= g.ver;
    }
    let r = CalcResult {
        freq_mhz,
        z: rep.z,
        swr: mom::swr(rep.z, ant.z0),
        ga_dbi: ga,
        fb_db: fb,
        az_max: az,
        el_max: el,
        horizontal_pol: hp,
        ground: ant.ground.kind,
        add_height: ant.ground.add_height,
        material: ant.material,
        threads: 1,
        n_unknowns: lay.total_segs,
        n_segs: lay.total_segs,
        t_total_ms: t_total,
        t_fill_ms: 0.0,
        t_solve_ms: t_total,
        t_far_ms: 0.0,
    };
    (r, pts)
}

/// NEC-2 cannot model current flowing from a wire into lossy ground: its impedance is
/// meaningless then (259 − j232 Ω for a quarter-wave vertical on average ground).
pub const NEC2_GROUND_CONNECTED: &str = "Wires connect to the real ground: NEC-2 cannot model a connection into lossy ground, so its impedance and gain are not reliable. Use perfect ground, raise the wires above the ground, or use the MININEC or wiremom engine.";

/// Impedance (and, with `need_far`, gain and F/B from a 3° grid) at one frequency.
pub fn quick(ant: &Antenna, freq_mhz: f64, seg_freq_mhz: f64, need_far: bool) -> Result<CalcResult, String> {
    let t = Instant::now();
    let lay = layout(ant, 299.792458 / seg_freq_mhz);
    let grounded = ant.ground.kind != GroundKind::Free;
    let rps = if need_far { vec![grid_rp(grounded).0] } else { vec![] };
    let rep = parse(&run(&deck(ant, &lay, freq_mhz, &rps)?)?)?;
    Ok(result(ant, freq_mhz, &lay, &rep, need_far, t.elapsed().as_secs_f64() * 1e3).0)
}

/// Full calculation for the Calculate tab: a 3° grid (3D pattern, maximum, F/B), then 1° cuts.
pub fn calculate(ant: &Antenna) -> Result<FullResult, String> {
    let t = Instant::now();
    let f = ant.freq_mhz;
    let lay = layout(ant, ant.lambda());
    let grounded = ant.ground.kind != GroundKind::Free;
    let (grid_card, grid_els) = grid_rp(grounded);
    let rep = parse(&run(&deck(ant, &lay, f, &[grid_card])?)?)?;
    let t1 = t.elapsed().as_secs_f64() * 1e3;
    let (mut res, pts) = result(ant, f, &lay, &rep, true, t1);
    // 1° cuts through the maximum
    let th_max = 90.0 - res.el_max;
    let az = res.az_max;
    let n_th = if grounded { 91 } else { 181 };
    let cards = vec![
        format!("RP 0 1 361 1000 {} 0 0 1", num(th_max)),
        format!("RP 0 {} 1 1000 0 {} 1 0", n_th, num(az)),
        format!("RP 0 {} 1 1000 0 {} 1 0", n_th, num((az + 180.0) % 360.0)),
    ];
    let t2 = Instant::now();
    let cut = parse(&run(&deck(ant, &lay, f, &cards)?)?)?;
    let az_cut: Vec<(f64, Gain)> = cut.pattern.iter().take(361).map(|p| (p[1], gain_of(p))).collect();
    let front: Vec<&[f64; 5]> = cut.pattern.iter().skip(361).take(n_th).collect();
    let back: Vec<&[f64; 5]> = cut.pattern.iter().skip(361 + n_th).take(n_th).collect();
    // t: 0 = front horizon, 90 = zenith, 180 = back horizon, then below (free space)
    let mut el_cut = Vec::new();
    for i in 0..=360usize {
        let t = i as f64;
        let (row, th) = if t <= 90.0 {
            (&front, 90.0 - t)
        } else if t <= 270.0 {
            (&back, t - 90.0)
        } else {
            (&front, 450.0 - t)
        };
        if grounded && th > 90.0 {
            continue;
        }
        if let Some(p) = row.iter().find(|p| (p[0] - th).abs() < 0.01) {
            el_cut.push((t, gain_of(p)));
        }
    }
    // refine the maximum from the 1° cuts
    if let Some((a, g)) = az_cut.iter().cloned().fold(None, |b: Option<(f64, Gain)>, x| match b {
        Some(bb) if bb.1.total >= x.1.total => Some(bb),
        _ => Some(x),
    }) {
        if mom::db(g.total) > res.ga_dbi {
            res.ga_dbi = mom::db(g.total);
            res.az_max = a % 360.0;
        }
    }
    let n_az = 120;
    let mut grid = vec![vec![0.0; n_az]; grid_els.len()];
    for (a, e, g) in &pts {
        let i = grid_els.iter().position(|x| (x - e).abs() < 0.01);
        let j = ((a / 3.0).round() as usize) % n_az;
        if let Some(i) = i {
            grid[i][j] = g.total;
        }
    }
    let mut els = grid_els.clone();
    let mut grid_sorted: Vec<(f64, Vec<f64>)> = els.drain(..).zip(grid).collect();
    grid_sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let (grid_els, grid): (Vec<f64>, Vec<Vec<f64>>) = grid_sorted.into_iter().unzip();
    res.t_far_ms = t2.elapsed().as_secs_f64() * 1e3;
    res.t_total_ms = t.elapsed().as_secs_f64() * 1e3;
    // segment currents for the 3D view
    let mut seg_currents = Vec::new();
    let mut k = 0;
    for p in &lay.pieces {
        for s in 0..p.nseg {
            let a = lerp(p.a, p.b, s as f64 / p.nseg as f64);
            let b = lerp(p.a, p.b, (s + 1) as f64 / p.nseg as f64);
            seg_currents.push((a, b, rep.seg_cur.get(k).cloned().unwrap_or(0.0)));
            k += 1;
        }
    }
    let mesh = mom::build_mesh(ant, ant.lambda())?;
    let mut warnings = mesh.warnings.clone();
    if ant.lines.iter().any(|t| t.loss_db > 0.0) {
        warnings.push("NEC-2 transmission lines are lossless: line loss is ignored by this engine (the wiremom engine includes it).".to_string());
    }
    if crate::wm::uses_ground_stand_in(ant) {
        warnings.push(NEC2_GROUND_CONNECTED.to_string());
    }
    Ok(FullResult {
        result: res.clone(),
        patterns: Patterns {
            az_cut,
            el_cut_deg: res.el_max,
            el_cut,
            az_cut_deg: res.az_max,
            free_space: !grounded,
            grid,
            grid_els,
            grid_step: 3.0,
        },
        mesh,
        currents: vec![],
        warnings,
        seg_currents: Some(seg_currents),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nec_matches_builtin_roughly() {
        if !available() {
            return;
        }
        for i in 0..6 {
            let mut a = Antenna::example(i);
            let m = mom::calculate(&a).unwrap().result;
            a.engine = Engine::Nec2;
            let n = calculate(&a).unwrap().result;
            println!("{}: MININEC Z={:.1} Ga={:.2} FB={:.1} | NEC-2 Z={:.1} Ga={:.2} FB={:.1} el={} ({:.0} ms)", a.name, m.z, m.ga_dbi, m.fb_db, n.z, n.ga_dbi, n.fb_db, n.el_max, n.t_total_ms);
            assert!((m.ga_dbi - n.ga_dbi).abs() < 1.5);
        }
    }
}

#[cfg(test)]
mod segmentation_test {
    use super::*;
    #[test]
    fn quarter_wave_vertical_gain_is_physical() {
        if !available() {
            return;
        }
        // lossless-limit 5.15 dBi; copper loss makes it a little less
        let mut a = Antenna::example(3);
        a.engine = Engine::Nec2;
        let r = calculate(&a).unwrap().result;
        assert!(r.ga_dbi > 5.05 && r.ga_dbi < 5.16, "{}", r.ga_dbi);
    }
}

#[cfg(test)]
mod sweep_test {
    use super::*;
    #[test]
    fn nec_sweep_parallel() {
        if !available() {
            return;
        }
        let mut a = Antenna::example(2);
        a.engine = Engine::Nec2;
        for n in [1, 12] {
            let p = rayon::ThreadPoolBuilder::new().num_threads(n).build().unwrap();
            let r = p.install(|| mom::sweep(&a, 13.8, 14.5, 24, true)).unwrap();
            println!("NEC-2 sweep 24 freqs on {n} threads: {:.0} ms, first Z {:.1}", r.t_ms, r.points[0].z);
        }
    }
}
