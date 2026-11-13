//! Puts a match from the Match calculator into the antenna: an L network as lumped
//! parts at the feed, a gamma match as wires, coax sections as transmission lines.
//! The source moves to the transmitter side of the match: onto a small feed wire for
//! an L network or lines, onto the gamma's feed wire for a gamma match.

use crate::model::*;

/// One match chosen in the calculator, with the values it worked out.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// An L network: a shunt reactance (infinite = none) and a series one (0 = none);
    /// `shunt_first` = the shunt part is across the antenna, the series part toward the
    /// transmitter, otherwise the series part is next to the antenna.
    LNetwork { x_shunt: f64, x_series: f64, shunt_first: bool },
    /// A gamma match: rod length from the feed to the shorting strap and the series
    /// reactance at the feed from the calculator's formula, rod diameter and centre spacing
    /// (m), and the resistance to match.  The rod length and the part are tuned with the
    /// antenna's own calculation when it can reach `r0`.
    Gamma { rod_len: f64, x_series: f64, d_rod: f64, spacing: f64, r0: f64 },
    /// Lines from the antenna toward the transmitter: (Z0, electrical degrees) each, all
    /// with this velocity factor.
    Lines { sections: Vec<(f64, f64)>, vf: f64 },
}

impl Plan {
    /// True when there is nothing to add: the antenna is already matched.
    pub fn is_nothing(&self) -> bool {
        match self {
            Plan::LNetwork { x_shunt, x_series, .. } => !x_shunt.is_finite() && x_series.abs() <= 1e-6,
            Plan::Gamma { .. } => false,
            Plan::Lines { sections, .. } => sections.iter().all(|s| s.1 <= 0.05),
        }
    }
}

/// Adds `plan` at the antenna's source, worked out at `f_mhz`; returns what was added.
pub fn apply(ant: &mut Antenna, plan: &Plan, f_mhz: f64) -> Result<String, String> {
    if ant.sources.len() != 1 {
        return Err(format!("A match can only be added to an antenna with one source; this one has {}.", ant.sources.len()));
    }
    let src = ant.sources[0].clone();
    if src.wire >= ant.wires.len() {
        return Err("The source isn't on a wire.".into());
    }
    let f_hz = f_mhz * 1e6;
    let lambda = 299.792458 / f_mhz;
    match plan {
        Plan::LNetwork { x_shunt, x_series, shunt_first } => {
            let shunt = x_shunt.is_finite().then(|| LoadKind::reactance(*x_shunt, f_hz));
            let series = (x_series.abs() > 1e-6).then(|| LoadKind::reactance(*x_series, f_hz));
            if shunt.is_none() && series.is_none() {
                return Err("This antenna is already matched: the network has no parts.".into());
            }
            let feed = add_feed_wire(ant, lambda, 0);
            let mut t = TLine::new(src.wire, src.pos, feed, 0.5);
            match &series {
                Some(_) => t.series = series.clone(),
                None => {
                    // nothing in series: a very short line of the matched impedance joins the ends
                    t.length = 1e-3;
                }
            }
            if *shunt_first {
                t.shunt_a = shunt.clone();
            } else {
                t.shunt_b = shunt.clone();
            }
            ant.lines.push(t);
            move_source(ant, feed, 0.5);
            let part = |k: &Option<LoadKind>| k.as_ref().map(part_text).unwrap_or_default();
            let (first, second) = if *shunt_first {
                (shunt.as_ref().map(|_| format!("{} across the antenna", part(&shunt))), series.as_ref().map(|_| format!("{} in series toward the transmitter", part(&series))))
            } else {
                (series.as_ref().map(|_| format!("{} in series with the antenna", part(&series))), shunt.as_ref().map(|_| format!("{} across the transmitter side", part(&shunt))))
            };
            let parts: Vec<String> = [first, second].into_iter().flatten().collect();
            Ok(format!("Added an L network ({}) as line {}; the source is now on the new feed wire {}.", parts.join(", then "), ant.lines.len(), feed + 1))
        }
        Plan::Gamma { rod_len, x_series, d_rod, spacing, r0 } => gamma(ant, &src, *rod_len, *x_series, *d_rod, *spacing, *r0, f_mhz),
        Plan::Lines { sections, vf } => {
            let sections: Vec<(f64, f64)> = sections.iter().copied().filter(|s| s.1 > 0.05).collect();
            if sections.is_empty() {
                return Err("This antenna is already matched: there is no line to add.".into());
            }
            let first_line = ant.lines.len() + 1;
            let mut from = (src.wire, src.pos);
            let mut last = 0;
            for (k, &(z0, deg)) in sections.iter().enumerate() {
                let w = add_feed_wire(ant, lambda, k);
                let mut t = TLine::new(from.0, from.1, w, 0.5);
                t.z0 = z0;
                t.vf = *vf;
                t.length = deg / 360.0 * lambda * vf;
                ant.lines.push(t);
                from = (w, 0.5);
                last = w;
            }
            move_source(ant, last, 0.5);
            let desc: Vec<String> = sections.iter().map(|&(z0, deg)| format!("{:.3} m of {} Ω", deg / 360.0 * lambda * vf, sig(z0))).collect();
            let joins = if sections.len() > 1 { format!("joined at wire {}, ", ant.wires.len() - sections.len() + 1) } else { String::new() };
            Ok(format!(
                "Added {} (lines {}–{}, VF {vf}), {joins}fed from the new feed wire {}.",
                desc.join(" then "),
                first_line,
                ant.lines.len(),
                last + 1
            ))
        }
    }
}

/// "120.5 pF" or "1.234 µH".
pub fn part_text(k: &LoadKind) -> String {
    match *k {
        LoadKind::Lc { l_uh, c_pf, .. } if c_pf == 0.0 => format!("{} µH", sig(l_uh)),
        LoadKind::Lc { l_uh, c_pf, .. } if l_uh == 0.0 => format!("{} pF", sig(c_pf)),
        LoadKind::Rx { r, x } => format!("{:.2} {} j{:.2} Ω", r, if x < 0.0 { "−" } else { "+" }, x.abs()),
        LoadKind::Lc { l_uh, c_pf, .. } => format!("{} µH ∥ {} pF", sig(l_uh), sig(c_pf)),
        LoadKind::Rlc { r, l_uh, c_pf, parallel } => format!("R {} Ω, L {} µH, C {} pF ({})", sig(r), sig(l_uh), sig(c_pf), if parallel { "parallel" } else { "series" }),
    }
}

/// Four significant figures.
fn sig(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let d = (3 - v.abs().log10().floor() as i32).max(0) as usize;
    format!("{v:.d$}")
}

/// A short vertical wire, electrically tiny so that it barely radiates or loads the
/// feed, beside the antenna: it carries the source, or joins two lines.  `k` keeps
/// several apart.  Returns its index.
fn add_feed_wire(ant: &mut Antenna, lambda: f64, k: usize) -> usize {
    let len = lambda / 200.0;
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for w in &ant.wires {
        for p in [w.p1, w.p2] {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    let x = lo[0] - lambda / 20.0 - k as f64 * lambda / 20.0;
    let y = 0.5 * (lo[1] + hi[1]);
    // over ground, clear of it (the ground plane is z = 0 before any added height)
    let z = if ant.ground.kind == GroundKind::Free { 0.5 * (lo[2] + hi[2]) } else { (lambda / 20.0).max(lo[2].max(0.0)) };
    let mut w = Wire::new([x, y, z], [x, y, z + len], len / 100.0);
    w.seg = 3;
    ant.wires.push(w);
    ant.wires.len() - 1
}

fn move_source(ant: &mut Antenna, wire: usize, pos: f64) {
    ant.sources[0].wire = wire;
    ant.sources[0].pos = pos;
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// The gamma match: the driven element is joined up at its feed and split there and at
/// the strap, the rod runs beside it (below a horizontal element), and the source and
/// series part sit on a short wire from the element to the rod.  The textbook formula
/// is only a start (it misses the rod's coupling to the element, and thin wire elements
/// most of all), so the rod length is then found with the antenna's own calculation:
/// where the resistance is `r0`; the series part cancels what reactance is left.
fn gamma(ant: &mut Antenna, src: &Source, l_formula: f64, x_formula: f64, d_rod: f64, s: f64, r0: f64, f_mhz: f64) -> Result<String, String> {
    let wi = src.wire;
    let len = ant.wires[wi].length();
    let at = src.pos * len;
    if !(0.2..=0.8).contains(&src.pos) {
        return Err("A gamma match goes on an element fed near its centre: this source is near a wire end.".into());
    }
    if ant.lines.iter().any(|t| t.a_wire == wi || t.b_wire == wi) {
        return Err("The driven element has a transmission line on it: remove it before adding a gamma match.".into());
    }
    let l_max = (len - at) * 0.95;
    let base = ant.clone();
    let build = |l: f64, x: Option<f64>| -> Result<(Antenna, String), String> {
        let mut a = base.clone();
        let msg = build_gamma(&mut a, src, l, x, d_rod, s, f_mhz * 1e6)?;
        Ok((a, msg))
    };
    // the resistance and reactance with no series part, for a rod of length l
    let z_at = |l: f64| -> Option<num_complex::Complex64> {
        let (a, _) = build(l, None).ok()?;
        crate::mom::quick(&a, f_mhz, f_mhz, false).ok().map(|r| r.0.z)
    };
    let mut tuned = None;
    let n = 12;
    let ls: Vec<f64> = (1..=n).map(|k| l_max * k as f64 / n as f64).collect();
    let mut prev: Option<(f64, f64)> = None;
    for &l in &ls {
        let Some(z) = z_at(l) else { break };
        if let Some((lp, rp)) = prev {
            if (rp - r0) * (z.re - r0) <= 0.0 {
                // bisect between the two rod lengths
                let (mut lo, mut hi, mut r_lo) = (lp, l, rp);
                for _ in 0..14 {
                    let mid = 0.5 * (lo + hi);
                    let Some(zm) = z_at(mid) else { break };
                    if (r_lo - r0) * (zm.re - r0) <= 0.0 {
                        hi = mid;
                    } else {
                        (lo, r_lo) = (mid, zm.re);
                    }
                }
                let l = 0.5 * (lo + hi);
                tuned = z_at(l).map(|z| (l, -z.im));
                break;
            }
        }
        prev = Some((l, z.re));
    }
    if tuned.is_none() && l_formula >= l_max {
        return Err(format!("The rod ({l_formula:.3} m) would run past the end of the element: it can be at most {l_max:.3} m here."));
    }
    let (l, x) = tuned.unwrap_or((l_formula, x_formula));
    let (a, msg) = build(l, Some(x))?;
    *ant = a;
    Ok(if tuned.is_some() {
        format!("{msg} Rod length and part found with this antenna's calculation (the formula gave {l_formula:.3} m).")
    } else {
        format!("{msg} No rod length up to {l_max:.3} m reaches {r0} Ω in this antenna's calculation, so these are the formula's values: try a thinner rod or wider spacing.")
    })
}

fn build_gamma(ant: &mut Antenna, src: &Source, l: f64, x: Option<f64>, d_rod: f64, s: f64, f_hz: f64) -> Result<String, String> {
    let wi = src.wire;
    let w = ant.wires[wi].clone();
    let len = w.length();
    let at = src.pos * len;
    let u = unit(sub(w.p2, w.p1));
    let mid = add(w.p1, u, at);
    let strap = add(mid, u, l);
    // the rod's side: below the element (down, square to it), or along +X for a vertical one
    let down = [0.0, 0.0, -1.0];
    let mut n = add(down, u, -dot(down, u));
    if dot(n, n) < 0.01 {
        let x = [1.0, 0.0, 0.0];
        n = add(x, u, -dot(x, u));
    }
    let n = unit(n);
    let feed = add(mid, n, s);
    let rod_end = add(strap, n, s);
    if ant.ground.kind != GroundKind::Free && feed[2].min(rod_end[2]) + ant.ground.add_height <= 0.0 {
        return Err("The gamma rod would be in the ground.".into());
    }

    // split the element at the feed and at the strap
    let pieces = [(0.0, at), (at, at + l), (at + l, len)];
    let segs = |a: f64, b: f64| if w.seg > 0 { ((w.seg as f64) * (b - a) / len).round().max(1.0) as i32 } else { w.seg };
    let mk = |a: f64, b: f64| Wire { p1: add(w.p1, u, a), p2: add(w.p1, u, b), seg: segs(a, b), ..w.clone() };
    ant.wires[wi] = mk(pieces[0].0, pieces[0].1);
    ant.wires.push(mk(pieces[1].0, pieces[1].1));
    let b_wire = ant.wires.len() - 1;
    ant.wires.push(mk(pieces[2].0, pieces[2].1));
    let c_wire = ant.wires.len() - 1;
    // loads on the element keep their places
    let place = |pos: f64| -> (usize, f64) {
        let d = pos * len;
        let [p0, p1, p2] = pieces;
        if d <= p0.1 {
            (wi, d / (p0.1 - p0.0))
        } else if d <= p1.1 {
            (b_wire, (d - p1.0) / (p1.1 - p1.0))
        } else {
            (c_wire, (d - p2.0) / (p2.1 - p2.0))
        }
    };
    for ld in ant.loads.iter_mut().filter(|ld| ld.wire == wi) {
        (ld.wire, ld.pos) = place(ld.pos);
    }

    let r = d_rod / 2.0;
    ant.wires.push(Wire::new(feed, rod_end, r));
    let rod = ant.wires.len() - 1;
    ant.wires.push(Wire::new(rod_end, strap, r));
    ant.wires.push(Wire::new(mid, feed, r));
    let fw = ant.wires.len() - 1;
    move_source(ant, fw, 0.5);
    let part = x.filter(|x| x.abs() > 1e-6).map(|x| LoadKind::reactance(x, f_hz));
    if let Some(k) = &part {
        ant.loads.push(Load { wire: fw, pos: 0.5, kind: k.clone() });
    }
    Ok(format!(
        "Added a gamma match: rod wire {} ({l:.3} m, {:.1} mm, {:.0} mm from the element), strap wire {}, feed wire {} with the source{}. \
         The driven element is now wires {}, {} and {}.",
        rod + 1,
        d_rod * 1e3,
        s * 1e3,
        rod + 2,
        fw + 1,
        part.as_ref().map(|k| format!(" and {} in series", part_text(k))).unwrap_or_default(),
        wi + 1,
        b_wire + 1,
        c_wire + 1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mom;
    use crate::tools;
    use num_complex::Complex64 as C;

    /// The example dipole (14 MHz), lossless, and its feed impedance.
    fn dipole(len_factor: f64) -> (Antenna, C) {
        let mut a = Antenna::example(0);
        a.engine = Engine::Wiremom;
        a.material = Material::Lossless;
        let w = &mut a.wires[0];
        for p in [&mut w.p1, &mut w.p2] {
            p[1] *= len_factor;
        }
        let z = mom::calculate(&a).unwrap().result.z;
        (a, z)
    }

    fn swr_after(a: &Antenna) -> (f64, C) {
        let r = mom::calculate(a).unwrap().result;
        (r.swr, r.z)
    }

    #[test]
    fn every_l_network_matches() {
        // a long dipole (R and X both off) so that every option has two parts
        let (a0, z) = dipole(1.15);
        let f = a0.freq_mhz;
        let sols = tools::l_networks(z, 50.0);
        assert!(sols.len() >= 2, "{sols:?}");
        for (xp, xs, at_load) in sols {
            let mut a = a0.clone();
            let msg = apply(&mut a, &Plan::LNetwork { x_shunt: xp, x_series: xs, shunt_first: at_load }, f).unwrap();
            let (swr, zm) = swr_after(&a);
            println!("{msg}\n  Z {z:.2} -> {zm:.2}, SWR {swr:.3}");
            assert!(swr < 1.05, "{msg}: SWR {swr}, Z {zm}");
            if crate::nec::available() {
                a.engine = Engine::Nec2;
                let (swr, zn) = swr_after(&a);
                println!("  NEC-2 {zn:.2}, SWR {swr:.3}");
                assert!(swr < 1.1, "NEC-2: SWR {swr}, Z {zn}");
            }
        }
    }

    #[test]
    fn parts_survive_a_nec_file() {
        let (mut a, z) = dipole(1.15);
        let (xp, xs, at_load) = tools::l_networks(z, 50.0)[0];
        let f = a.freq_mhz;
        apply(&mut a, &Plan::LNetwork { x_shunt: xp, x_series: xs, shunt_first: at_load }, f).unwrap();
        let text = crate::nec_file::write(&a);
        assert!(text.contains("\nNT "), "{text}");
        let b = crate::nec_file::parse(&text, "t").unwrap();
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert_eq!(b.ant.lines.len(), 1);
        let (ya, yb) = (a.lines[0].part_admittances(f * 1e6), b.ant.lines[0].part_admittances(f * 1e6));
        assert!((ya.0.unwrap() - yb.0.unwrap()).norm() < 1e-9 && (ya.1 - yb.1).norm() < 1e-9 && yb.2.norm() == 0.0, "{:?}", b.ant.lines);
        // without this program's comment the NT card's admittances still give the parts at
        // the file's frequency
        let bare: String = text.lines().filter(|l| !l.starts_with("CM wireant parts")).map(|l| format!("{l}\n")).collect();
        let mut c = crate::nec_file::parse(&bare, "t").unwrap().ant;
        c.engine = Engine::Wiremom;
        let (swr, zc) = swr_after(&c);
        assert!(swr < 1.05, "SWR {swr}, Z {zc}");
    }

    #[test]
    fn coax_sections_match() {
        let (a0, z) = dipole(1.04);
        let f = a0.freq_mhz;
        let mut plans = vec![];
        for (th2, th1) in tools::series_section(z, 50.0, 93.0) {
            plans.push(vec![(50.0, th2), (93.0, th1)]);
        }
        for (th, _, zq) in tools::quarter_wave(z, 50.0) {
            plans.push(vec![(50.0, th), (zq, 90.0)]);
        }
        assert!(plans.len() >= 3, "Z {z:.2}: {plans:?}");
        for sections in plans {
            let mut a = a0.clone();
            let msg = apply(&mut a, &Plan::Lines { sections, vf: 0.66 }, f).unwrap();
            let (swr, zm) = swr_after(&a);
            println!("{msg}\n  Z {z:.2} -> {zm:.2}, SWR {swr:.3}");
            assert!(swr < 1.05, "{msg}: SWR {swr}, Z {zm}");
        }
    }

    #[test]
    fn gamma_match_builds_and_improves() {
        // a driven element a little short, as in a Yagi
        let (a0, z) = dipole(0.97);
        let f = a0.freq_mhz;
        let d_el = 2.0 * a0.wires[0].r;
        let (d_rod, s) = (0.006, 0.08);
        let (l, x) = tools::gamma_match(z, 50.0, f * 1e6, d_el, d_rod, s).expect("a gamma solution");
        let mut a = a0.clone();
        let msg = apply(&mut a, &Plan::Gamma { rod_len: l, x_series: x, d_rod, spacing: s, r0: 50.0 }, f).unwrap();
        assert_eq!(a.wires.len(), 6);
        assert_eq!(a.sources[0].wire, 5);
        // the element is still the same length, now in three pieces
        let total: f64 = a.wires[..3].iter().map(|w| w.length()).sum();
        assert!((total - a0.wires[0].length()).abs() < 1e-9, "{total}");
        let (swr, zm) = swr_after(&a);
        println!("{msg}\n  Z {z:.2} -> {zm:.2}, SWR {swr:.3}");
        assert!(swr < 1.05, "SWR {swr}, Z {zm}");
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        let (mut a, _) = dipole(1.0);
        a.sources.push(a.sources[0].clone());
        assert!(apply(&mut a.clone(), &Plan::LNetwork { x_shunt: f64::INFINITY, x_series: 10.0, shunt_first: true }, 14.2).is_err());
        a.sources.pop();
        a.sources[0].pos = 0.0;
        assert!(apply(&mut a, &Plan::Gamma { rod_len: 0.5, x_series: -50.0, d_rod: 0.01, spacing: 0.1, r0: 50.0 }, 14.2).is_err());
        assert!(apply(&mut a, &Plan::LNetwork { x_shunt: f64::INFINITY, x_series: 0.0, shunt_first: false }, 14.2).is_err());
    }
}

