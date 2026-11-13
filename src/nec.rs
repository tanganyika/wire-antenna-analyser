//! NEC engines: the bundled public-domain nec2c (compiled by build.rs), or an external NEC
//! program the user has a copy of (File → Preferences), run as a separate process.
//! Each calculation writes a NEC card deck, runs the program and reads its report.
//! Sweeps and optimizer trials run many processes at once on the rayon thread pool.

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

/// Whether an engine can run in this copy with the current settings.
pub fn engine_available(e: Engine) -> bool {
    match e {
        Engine::Nec2 => available(),
        Engine::External => crate::settings::get().external_ready(),
        Engine::Mininec | Engine::Wiremom => true,
    }
}

fn binary() -> Result<PathBuf, String> {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    PATH.get_or_init(|| {
        if NEC2C.is_empty() {
            return Err("The NEC-2 engine wasn't built into this copy of the program.".into());
        }
        // ~/.cache, %LOCALAPPDATA%, ~/Library/Caches
        let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        let mut h: u64 = 1469598103934665603;
        for b in NEC2C.iter().step_by(97) {
            h = (h ^ *b as u64).wrapping_mul(1099511628211);
        }
        let dir = base.join("wireant");
        let path = dir.join(format!("nec2c-{:x}-{}{}", h, NEC2C.len(), std::env::consts::EXE_SUFFIX));
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

/// The second ground medium as NEC-2 sees it: (RP mode 2 linear / 3 circular,
/// rotation in degrees).  NEC-2's linear cliff runs parallel to Y at x = CLT, so
/// for another bearing the structure is turned by -bearing (GM) and the pattern
/// back by +bearing.
fn second_medium(ant: &Antenna) -> Option<(u8, f64)> {
    let s = &ant.ground.second;
    (s.enabled && ant.ground.kind != GroundKind::Free).then(|| if s.circle { (3, 0.0) } else { (2, s.bearing_deg) })
}

/// Runs a deck with these RP cards; pattern azimuths in the antenna's own frame.
fn run_deck(ant: &Antenna, lay: &Layout, freq_mhz: f64, rps: &[String]) -> Result<Report, String> {
    let mut rep = parse(&run(&deck(ant, lay, freq_mhz, rps)?, ant.engine)?)?;
    // the cards asked for azimuths shifted by -rot, so this lands back on them
    if let Some((_, rot)) = second_medium(ant).filter(|s| s.1 != 0.0) {
        for p in rep.pattern.iter_mut() {
            p[1] += rot;
        }
    }
    Ok(rep)
}

fn deck(ant: &Antenna, lay: &Layout, freq_mhz: f64, rps: &[String]) -> Result<String, String> {
    if !ant.plates.is_empty() {
        // NEC patches only model closed surfaces (MFIE)
        return Err("Plates need the wiremom engine: NEC's patches only model closed surfaces, not thin plates.".into());
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
    let mut d = String::from("CM generated by wireant\nCE\n");
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
    let second = second_medium(ant);
    if let Some((_, rot)) = second.filter(|s| s.1 != 0.0) {
        d += &format!("GM 0 0 0 0 {} 0 0 0 0\n", num(-rot));
    }
    d += if grounded { "GE 1\n" } else { "GE 0\n" };
    match ant.ground.kind {
        GroundKind::Free => {}
        GroundKind::Perfect => d += "GN 1\n",
        GroundKind::Real => d += &format!("GN 2 0 0 0 {} {}\n", num(ant.ground.eps_r), num(ant.ground.sigma_ms * 1e-3)),
    }
    if second.is_some() {
        let s = &ant.ground.second;
        d += &format!("GD 0 0 0 0 {} {} {} {}\n", num(s.eps_r), num(s.sigma_ms * 1e-3), num(s.distance.max(0.0)), num(s.drop.max(0.0)));
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
        match second {
            // cliff mode, and the azimuths in the rotated frame
            Some((mode, rot)) => {
                let mut t: Vec<String> = rp.split_whitespace().map(String::from).collect();
                t[1] = mode.to_string();
                if rot != 0.0 {
                    let phis: f64 = t[6].parse().unwrap_or(0.0);
                    t[6] = num(phis - rot);
                }
                d += &t.join(" ");
            }
            None => d += rp,
        }
        d += "\n";
    }
    d += "EN\n";
    Ok(d)
}

fn run(deck: &str, engine: Engine) -> Result<String, String> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("wireant-nec-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let res = std::fs::write(dir.join("in.nec"), deck)
        .map_err(|e| e.to_string())
        .and_then(|_| if engine == Engine::External { run_external(&dir, deck) } else { run_nec2c(&dir) });
    let _ = std::fs::remove_dir_all(&dir);
    res
}

fn no_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        // don't flash a console window for each calculation
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd;
}

fn run_nec2c(dir: &std::path::Path) -> Result<String, String> {
    let bin = binary()?;
    // nec2c takes file names of at most 75 characters, so run it in the folder
    let mut cmd = std::process::Command::new(&bin);
    cmd.current_dir(dir).arg("-iin.nec").arg("-oout.txt").stdin(std::process::Stdio::null());
    no_window(&mut cmd);
    let res = cmd.output();
    let text = std::fs::read_to_string(dir.join("out.txt")).unwrap_or_default();
    let o = res.map_err(|e| format!("Couldn't start the NEC-2 engine: {e}"))?;
    if !o.status.success() || !text.contains(REPORT_MARK) {
        let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(format!("NEC-2 stopped: {}", if msg.is_empty() { tail(&text) } else { msg }));
    }
    Ok(text)
}

const REPORT_MARK: &str = "ANTENNA INPUT PARAMETERS";

fn tail(text: &str) -> String {
    text.lines().rev().filter(|l| !l.trim().is_empty()).take(4).collect::<Vec<_>>().into_iter().rev().map(str::trim).collect::<Vec<_>>().join(" ")
}

/// Where the last external run's input, output and messages are kept, for checking
/// what the program made of the deck.
pub fn external_last_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("wireant").join("external-nec-last")
}

/// Limits how many copies of the external program run at once.
struct Slots {
    used: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
}

struct Slot(&'static Slots);

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut n) = self.0.used.lock() {
            *n -= 1;
        }
        self.0.freed.notify_one();
    }
}

fn take_slot(limit: usize) -> Slot {
    static S: Slots = Slots { used: std::sync::Mutex::new(0), freed: std::sync::Condvar::new() };
    let mut n = S.used.lock().unwrap_or_else(|e| e.into_inner());
    while *n >= limit.max(1) {
        n = S.freed.wait(n).unwrap_or_else(|e| e.into_inner());
    }
    *n += 1;
    Slot(&S)
}

/// Runs the user's own NEC program (File → Preferences) in `dir` on in.nec.  The report is
/// taken from out.txt, any other file it wrote, or its standard output, whichever has one.
fn run_external(dir: &std::path::Path, deck: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let s = crate::settings::get();
    let prog = s.nec_path.trim();
    if prog.is_empty() {
        return Err("No external NEC program is set: choose it under File → Preferences.".into());
    }
    if !std::path::Path::new(prog).is_file() {
        return Err(format!("The external NEC program {prog} wasn't found: check File → Preferences."));
    }
    let args = if s.nec_args.trim().is_empty() && !s.nec_stdin { crate::settings::DEFAULT_NEC_ARGS } else { s.nec_args.as_str() };
    let _slot = take_slot(if s.nec_jobs == 0 { usize::MAX } else { s.nec_jobs });
    let mut cmd = std::process::Command::new(prog);
    cmd.current_dir(dir)
        .args(args.split_whitespace().map(|a| a.replace("{in}", "in.nec").replace("{out}", "out.txt")))
        .stdin(if s.nec_stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start the external NEC program {prog}: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        // programs that ask for the file names read them here; it may not ask at all
        let _ = si.write_all(b"in.nec\nout.txt\n");
    }
    let o = child.wait_with_output().map_err(|e| format!("The external NEC program failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
    let out = std::fs::read_to_string(dir.join("out.txt")).unwrap_or_default();
    let report = if out.contains(REPORT_MARK) {
        Some(out.clone())
    } else {
        // some programs choose their own output name
        let mut other: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && p.file_name().is_some_and(|n| n != "in.nec")).collect())
            .unwrap_or_default();
        other.sort();
        other
            .iter()
            .filter_map(|p| std::fs::read(p).ok())
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .find(|t| t.contains(REPORT_MARK))
            .or_else(|| stdout.contains(REPORT_MARK).then(|| stdout.clone()))
    };
    // keep the last run so its input and output can be looked at (and sent on)
    let keep = external_last_dir();
    if std::fs::create_dir_all(&keep).is_ok() {
        let _ = std::fs::write(keep.join("in.nec"), deck);
        let _ = std::fs::write(keep.join("out.txt"), report.as_deref().unwrap_or(&out));
        let _ = std::fs::write(keep.join("stdout.txt"), &stdout);
        let _ = std::fs::write(keep.join("stderr.txt"), &stderr);
    }
    report.ok_or_else(|| {
        let why = [tail(&stderr), tail(&stdout), tail(&out)].into_iter().find(|t| !t.is_empty()).unwrap_or_default();
        format!(
            "The external NEC program gave no report this program can read ({}{}). Its input, output and messages are kept in {}.",
            o.status,
            if why.is_empty() { String::new() } else { format!(": {why}") },
            keep.display()
        )
    })
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
    let rep = run_deck(ant, &lay, freq_mhz, &rps)?;
    Ok(result(ant, freq_mhz, &lay, &rep, need_far, t.elapsed().as_secs_f64() * 1e3).0)
}

/// Full calculation for the Calculate tab: a 3° grid (3D pattern, maximum, F/B), then 1° cuts.
pub fn calculate(ant: &Antenna) -> Result<FullResult, String> {
    let t = Instant::now();
    let f = ant.freq_mhz;
    let lay = layout(ant, ant.lambda());
    let grounded = ant.ground.kind != GroundKind::Free;
    let (grid_card, grid_els) = grid_rp(grounded);
    let rep = run_deck(ant, &lay, f, &[grid_card])?;
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
    let cut = run_deck(ant, &lay, f, &cards)?;
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
        warnings.push("NEC transmission lines (TL cards) are lossless: line loss is ignored by this engine (the wiremom engine includes it).".to_string());
    }
    // NEC-4 and later model wires into lossy ground; NEC-2 can't
    if ant.engine == Engine::Nec2 && crate::wm::touches_ground(ant) && !crate::wm::has_buried(ant) {
        warnings.push(NEC2_GROUND_CONNECTED.to_string());
    }
    if ant.engine == Engine::External {
        warnings.push("Calculated by the external NEC program with the same card deck as NEC-2 (experimental).".to_string());
    }
    if second_medium(ant).is_some() && ant.ground.second.fresnel {
        warnings.push("NEC takes the second ground at each ray's reflection point; the Fresnel-zone transition needs the wiremom or MININEC engine.".to_string());
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
            a.engine = Engine::Mininec;
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
