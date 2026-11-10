//! Thin-wire method-of-moments solver (mixed-potential EFIE, triangle current functions
//! approximated by pulses on half segments, reduced kernel). Matrix fill, LU solve,
//! far-field scans and frequency sweeps all run in parallel on the rayon thread pool.

use crate::model::*;
use num_complex::Complex64 as C;
use rayon::prelude::*;
use std::f64::consts::PI;
use std::time::Instant;

pub type V3 = [f64; 3];
pub const C0: f64 = 299_792_458.0;
pub const MU0: f64 = 4.0e-7 * PI;
pub const EPS0: f64 = 1.0 / (MU0 * C0 * C0);
pub const ETA0: f64 = MU0 * C0;

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
fn mirror(a: V3) -> V3 {
    [a[0], a[1], -a[2]]
}
#[inline]
fn mid(a: V3, b: V3) -> V3 {
    scale(add(a, b), 0.5)
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub a: V3,
    pub b: V3,
    pub r: f64,
    pub wire: usize,
}

/// One half of a triangle current function, lying on one segment.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub seg: usize,
    /// 0 = first half (a..mid), 1 = second half (mid..b)
    pub half: u8,
    /// +1 when the basis current runs along the segment's direction.
    pub dir: f64,
    /// Sign of the charge (+1 where current flows towards the node).
    pub sigma: f64,
}

#[derive(Clone, Debug)]
pub struct Basis {
    pub node: V3,
    pub parts: Vec<Part>,
}

/// A place on a wire where a source or load can sit: the current functions that cross it.
#[derive(Clone, Debug)]
pub struct Gap {
    pub frac: f64,
    pub crossing: Vec<(usize, f64)>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub segs: Vec<Seg>,
    pub bases: Vec<Basis>,
    pub wire_gaps: Vec<Vec<Gap>>,
    pub wire_segs: Vec<(usize, usize)>,
    pub warnings: Vec<String>,
    pub image: bool,
}

/// Segment lengths along one wire, following the original solver (MECHANICS.md §2):
/// seg > 0 -> FFSolver 0x405390, seg == 0 -> 0x405460, seg < 0 -> 0x405540.
pub fn wire_segment_lengths(len: f64, lambda: f64, seg: i32, r: f64, sp: &Segmentation) -> Vec<f64> {
    let r = r.abs();
    let even = |n: i64| -> usize {
        let mut n = n.max(2);
        if n % 2 == 1 {
            n += 1;
        }
        n as usize
    };
    let uniform = |n: usize| vec![len / n as f64; n];
    // fixed count: even, >= 2, no segment shorter than the wire diameter, 2 when lambda/L > 1500
    if seg > 0 {
        let mut n = seg as i64;
        if len / (n as f64) < 2.0 * r {
            n = (len / (2.0 * r)) as i64;
        }
        if lambda / len > 1500.0 {
            n = 2;
        }
        return uniform(even(n));
    }
    // zero: uniform at the longest segment length lambda/DM2
    let uniform_dm2 = |dm2: f64| -> Vec<f64> {
        let mut n = (len * dm2 / lambda) as i64;
        while n > 0 && len / (n as f64) < 2.0 * r {
            n -= 2;
        }
        uniform(even(n))
    };
    if seg == 0 {
        return uniform_dm2(sp.dm2);
    }
    // automatic: EC shortest segments (lambda/DM1) at each end, then segments growing by SC
    // until they reach lambda/DM2, and an even number of segments in the middle.
    let mut dm1 = sp.dm1.max(1.0);
    let mut dm2 = sp.dm2.max(1.0);
    if lambda / dm1 < 2.0 * r {
        dm1 = (lambda / (2.0 * r)).floor().max(1.0);
        dm2 = dm2.min(dm1);
    }
    if len < lambda / dm1 {
        return uniform(2);
    }
    let ec = sp.ec.max(1) as usize;
    let sc = if sp.sc > 1.0 { sp.sc } else { 2.0 };
    while dm2 < dm1 {
        let mut taper = vec![lambda / dm1; ec];
        let mut d = dm1 / sc;
        while d > dm2 {
            taper.push(lambda / d);
            d /= sc;
        }
        let t: f64 = taper.iter().sum();
        let mid = len - 2.0 * t;
        let lmax = lambda / dm2;
        let mut n_mid = if mid > 0.0 { ((mid / lmax) as usize) & !1 } else { 0 };
        if mid > 0.0 && n_mid > 1 {
            if n_mid < 5 && lmax < (mid / n_mid as f64) * 0.85 {
                n_mid += 2;
            }
            let mut out = taper.clone();
            out.extend(std::iter::repeat(mid / n_mid as f64).take(n_mid));
            out.extend(taper.iter().rev());
            return out;
        }
        dm2 *= 2.0;
    }
    uniform_dm2(dm1)
}

pub fn build_mesh(ant: &Antenna, lambda: f64) -> Result<Mesh, String> {
    if ant.wires.is_empty() {
        return Err("There are no wires.".into());
    }
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let mut segs = Vec::new();
    let mut bases = Vec::new();
    let mut wire_gaps: Vec<Vec<Gap>> = vec![Vec::new(); ant.wires.len()];
    let mut wire_segs = Vec::new();
    let mut warnings = Vec::new();
    let mut min_len = f64::MAX;
    for (wi, w) in ant.wires.iter().enumerate() {
        let p1 = [w.p1[0], w.p1[1], w.p1[2] + dz];
        let p2 = [w.p2[0], w.p2[1], w.p2[2] + dz];
        let len = norm(sub(p2, p1));
        if len <= 0.0 {
            // the original skips such wires (FFSolver 0x406cc0 / 0x407ab0)
            warnings.push(format!("Wire {} has zero length and was skipped.", wi + 1));
            wire_segs.push((segs.len(), 0));
            continue;
        }
        if w.r == 0.0 {
            warnings.push(format!("Wire {} has zero radius and was skipped.", wi + 1));
            wire_segs.push((segs.len(), 0));
            continue;
        }
        min_len = min_len.min(len);
        if grounded && (p1[2] < -1e-6 || p2[2] < -1e-6) {
            // message text from the original (english.mmn mainmes0)
            return Err(format!("Antenna or part of antenna is under ground. Increase height! (wire {})", wi + 1));
        }
        let lens = wire_segment_lengths(len, lambda, w.seg, w.r, &ant.seg);
        let dirv = scale(sub(p2, p1), 1.0 / len);
        let start = segs.len();
        let mut s = 0.0;
        for l in &lens {
            let a = add(p1, scale(dirv, s));
            s += l;
            let b = if (s - len).abs() < 1e-12 { p2 } else { add(p1, scale(dirv, s)) };
            segs.push(Seg { a, b, r: w.r, wire: wi });
        }
        let n = lens.len();
        wire_segs.push((start, n));
        if lens.iter().any(|l| *l < 2.0 * w.r.abs() * 0.999) {
            warnings.push(format!("Wire {}: segments are shorter than the wire diameter.", wi + 1));
        }
        // interior nodes
        let mut acc = 0.0;
        for k in 1..n {
            acc += lens[k - 1];
            let bi = bases.len();
            bases.push(Basis {
                node: segs[start + k].a,
                parts: vec![
                    Part { seg: start + k - 1, half: 1, dir: 1.0, sigma: 1.0 },
                    Part { seg: start + k, half: 0, dir: 1.0, sigma: -1.0 },
                ],
            });
            wire_gaps[wi].push(Gap { frac: acc / len, crossing: vec![(bi, 1.0)] });
        }
    }
    // Endpoints and junctions
    let tol = (min_len * 1e-4).max(1e-7);
    let mut ends: Vec<(usize, u8, V3)> = Vec::new(); // (wire, 0=begin 1=end, point)
    for (wi, &(st, n)) in wire_segs.iter().enumerate() {
        if n == 0 {
            continue;
        }
        ends.push((wi, 0, segs[st].a));
        ends.push((wi, 1, segs[st + n - 1].b));
    }
    let mut used = vec![false; ends.len()];
    let has_feed_at = |wi: usize, e: u8| -> bool {
        let f = if e == 0 { 0.0 } else { 1.0 };
        ant.sources.iter().any(|s| s.wire == wi && (s.pos - f).abs() < 1e-9)
            || ant.loads.iter().any(|l| l.wire == wi && (l.pos - f).abs() < 1e-9)
    };
    for i in 0..ends.len() {
        if used[i] {
            continue;
        }
        let mut cl = vec![i];
        used[i] = true;
        for j in i + 1..ends.len() {
            if !used[j] && norm(sub(ends[i].2, ends[j].2)) < tol {
                used[j] = true;
                cl.push(j);
            }
        }
        // segment end info: (seg index, half, dir-into-node)
        let end_seg = |k: usize| -> (usize, u8, f64) {
            let (wi, e, _) = ends[k];
            let (st, n) = wire_segs[wi];
            if e == 0 { (st, 0, -1.0) } else { (st + n - 1, 1, 1.0) }
        };
        let p = ends[i].2;
        if grounded && p[2].abs() < tol {
            for &k in &cl {
                let (s, h, d) = end_seg(k);
                let bi = bases.len();
                bases.push(Basis { node: p, parts: vec![Part { seg: s, half: h, dir: d, sigma: 1.0 }] });
                let (wi, e, _) = ends[k];
                // wire direction at the begin runs away from the node.
                let sign = if e == 0 { -1.0 } else { 1.0 };
                wire_gaps[wi].push(Gap { frac: e as f64, crossing: vec![(bi, sign)] });
            }
            continue;
        }
        if cl.len() < 2 {
            continue;
        }
        // reference end: one that carries a source/load, if any
        let ref_pos = cl.iter().position(|&k| has_feed_at(ends[k].0, ends[k].1)).unwrap_or(0);
        cl.swap(0, ref_pos);
        let k0 = cl[0];
        let (s0, h0, d0) = end_seg(k0);
        let mut ref_cross = Vec::new();
        for &kj in &cl[1..] {
            let (sj, hj, dj) = end_seg(kj);
            let bi = bases.len();
            bases.push(Basis {
                node: p,
                parts: vec![
                    Part { seg: s0, half: h0, dir: d0, sigma: 1.0 },
                    Part { seg: sj, half: hj, dir: -dj, sigma: -1.0 },
                ],
            });
            let (wj, ej, _) = ends[kj];
            let sign_j = if ej == 0 { 1.0 } else { -1.0 };
            wire_gaps[wj].push(Gap { frac: ej as f64, crossing: vec![(bi, sign_j)] });
            let (_, e0, _) = ends[k0];
            let sign0 = if e0 == 0 { -1.0 } else { 1.0 };
            ref_cross.push((bi, sign0));
        }
        let (w0, e0, _) = ends[k0];
        wire_gaps[w0].push(Gap { frac: e0 as f64, crossing: ref_cross });
    }
    for g in wire_gaps.iter_mut() {
        g.sort_by(|a, b| a.frac.partial_cmp(&b.frac).unwrap());
    }
    if bases.len() > 40000 {
        return Err(format!("The model needs {} pulses; the limit is 40000.", bases.len()));
    }
    if bases.is_empty() {
        return Err("The model has no current paths (add segments or connect wires).".into());
    }
    Ok(Mesh { segs, bases, wire_gaps, wire_segs, warnings, image: grounded })
}

/// (1/L) * integral over the segment a..b of exp(-jkR)/(4 pi R), with R = sqrt(d^2 + r^2).
fn psi(obs: V3, a: V3, b: V3, r: f64, k: f64) -> C {
    let d = sub(b, a);
    let l = norm(d);
    let t = scale(d, 1.0 / l);
    let w = sub(obs, a);
    let s0 = dot(w, t);
    let rho2 = (dot(w, w) - s0 * s0).max(0.0);
    let b2 = rho2 + r * r;
    let bb = b2.sqrt();
    let analytic = ((l - s0) / bb).asinh() - (-s0 / bb).asinh();
    let cdist = norm(sub(obs, mid(a, b)));
    let (xs, ws): (&[f64], &[f64]) = if cdist < 3.0 * l { (&GX8, &GW8) } else { (&GX4, &GW4) };
    let mut re = 0.0;
    let mut im = 0.0;
    for (x, wt) in xs.iter().zip(ws.iter()) {
        let s = 0.5 * l * (1.0 + x);
        let ds = s - s0;
        let rr = (ds * ds + b2).sqrt();
        let kr = k * rr;
        re += wt * (kr.cos() - 1.0) / rr;
        im -= wt * kr.sin() / rr;
    }
    let half = 0.5 * l;
    C::new(analytic + re * half, im * half) / (4.0 * PI * l)
}

const GX4: [f64; 4] = [-0.8611363115940526, -0.3399810435848563, 0.3399810435848563, 0.8611363115940526];
const GW4: [f64; 4] = [0.3478548451374538, 0.6521451548625461, 0.6521451548625461, 0.3478548451374538];
const GX8: [f64; 8] = [
    -0.9602898564975363, -0.7966664774136267, -0.5255324099163290, -0.1834346424956498,
    0.1834346424956498, 0.5255324099163290, 0.7966664774136267, 0.9602898564975363,
];
const GW8: [f64; 8] = [
    0.1012285362903763, 0.2223810344533745, 0.3137066458778873, 0.3626837833783620,
    0.3626837833783620, 0.3137066458778873, 0.2223810344533745, 0.1012285362903763,
];

/// Geometry of one part, precomputed for the fill.
#[derive(Clone, Copy)]
struct PG {
    hc: V3,
    h0: V3,
    h1: V3,
    l: V3,
    seg: usize,
    sc: V3,
    sigma: f64,
    r: f64,
    half_len: f64,
}

fn part_geo(mesh: &Mesh, p: &Part) -> PG {
    let s = &mesh.segs[p.seg];
    let m = mid(s.a, s.b);
    let (h0, h1) = if p.half == 0 { (s.a, m) } else { (m, s.b) };
    PG {
        hc: mid(h0, h1),
        h0,
        h1,
        l: scale(sub(h1, h0), p.dir),
        seg: p.seg,
        sc: m,
        sigma: p.sigma,
        r: s.r,
        half_len: norm(sub(h1, h0)),
    }
}

#[derive(Clone, Debug)]
pub struct Solution {
    pub freq_mhz: f64,
    pub currents: Vec<C>,
    pub z_in: Vec<C>,
    pub p_in: f64,
    pub n_unknowns: usize,
    pub n_segs: usize,
    pub t_fill_ms: f64,
    pub t_solve_ms: f64,
}

/// Fill the impedance matrix and solve for the currents.
pub fn solve(ant: &Antenna, mesh: &Mesh, freq_mhz: f64) -> Result<Solution, String> {
    let f = freq_mhz * 1e6;
    let omega = 2.0 * PI * f;
    let k = omega / C0;
    let n = mesh.bases.len();
    let jwmu = C::new(0.0, omega * MU0);
    let inv_jwe = C::new(0.0, -1.0 / (omega * EPS0));
    let geo: Vec<Vec<PG>> =
        mesh.bases.iter().map(|b| b.parts.iter().map(|p| part_geo(mesh, p)).collect()).collect();
    let t0 = Instant::now();
    // scalar-potential table: segment centre -> segment
    let ns = mesh.segs.len();
    let image = mesh.image;
    let segs = &mesh.segs;
    let mut pseg = vec![C::new(0.0, 0.0); ns * ns];
    pseg.par_chunks_mut(ns).enumerate().for_each(|(i, row)| {
        let c = mid(segs[i].a, segs[i].b);
        for (j, v) in row.iter_mut().enumerate() {
            let s = &segs[j];
            let mut x = psi(c, s.a, s.b, s.r, k);
            if image {
                x -= psi(c, mirror(s.a), mirror(s.b), s.r, k);
            }
            *v = x;
        }
    });
    let mut z = vec![C::new(0.0, 0.0); n * n];
    z.par_chunks_mut(n).enumerate().for_each(|(m, row)| {
        for (nn, v) in row.iter_mut().enumerate() {
            let mut sum = C::new(0.0, 0.0);
            for pm in &geo[m] {
                for pn in &geo[nn] {
                    let vv = dot(pm.l, pn.l);
                    if vv != 0.0 {
                        sum += jwmu * vv * psi(pm.hc, pn.h0, pn.h1, pn.r, k);
                    }
                    if image {
                        let li = scale(mirror(pn.l), -1.0);
                        let vi = dot(pm.l, li);
                        if vi != 0.0 {
                            sum += jwmu * vi * psi(pm.hc, mirror(pn.h0), mirror(pn.h1), pn.r, k);
                        }
                    }
                    sum += inv_jwe * (pm.sigma * pn.sigma) * pseg[pm.seg * ns + pn.seg];
                }
            }
            *v = sum;
        }
    });
    // conductor loss
    if let Some((sig, mu_r)) = ant.material.conduction(ant.user_rho, ant.user_mu) {
        let rs = (PI * f * MU0 * mu_r / sig).sqrt();
        for m in 0..n {
            let mut r = 0.0;
            for pm in &geo[m] {
                r += rs / (2.0 * PI * pm.r) * pm.half_len;
            }
            z[m * n + m] += r;
        }
    }
    // wire insulation: distributed series inductance jωL' along insulated wires
    if ant.wires.iter().any(|w| w.insulated()) {
        for m in 0..n {
            let mut x = 0.0;
            for pm in &geo[m] {
                let w = &ant.wires[mesh.segs[pm.seg].wire];
                x += omega * w.insulation_l_per_m() * pm.half_len;
            }
            z[m * n + m] += C::new(0.0, x);
        }
    }
    // excitation and loads
    let mut rhs = vec![C::new(0.0, 0.0); n];
    let mut src_gaps = Vec::new();
    for s in &ant.sources {
        let g = find_gap(mesh, s.wire, s.pos)
            .ok_or_else(|| format!("Source on wire {} has no place to sit (give the wire more segments).", s.wire + 1))?;
        let v = C::from_polar(s.volts, s.phase_deg.to_radians());
        for &(b, sg) in &g.crossing {
            rhs[b] += v * sg;
        }
        src_gaps.push((g.crossing.clone(), v));
    }
    if ant.sources.is_empty() {
        return Err("There is no source.".into());
    }
    for l in &ant.loads {
        let g = find_gap(mesh, l.wire, l.pos)
            .ok_or_else(|| format!("Load on wire {} has no place to sit.", l.wire + 1))?;
        let zl = l.impedance(f);
        for &(a, sa) in &g.crossing {
            for &(b, sb) in &g.crossing {
                z[a * n + b] += zl * (sa * sb);
            }
        }
    }
    let t_fill = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    lu_solve(&mut z, &mut rhs, n)?;
    let t_solve = t1.elapsed().as_secs_f64() * 1e3;
    let mut z_in = Vec::new();
    let mut p_in = 0.0;
    for (cross, v) in &src_gaps {
        let i: C = cross.iter().map(|&(b, s)| rhs[b] * s).sum();
        z_in.push(if i.norm() > 0.0 { *v / i } else { C::new(f64::INFINITY, 0.0) });
        p_in += 0.5 * (v * i.conj()).re;
    }
    Ok(Solution {
        freq_mhz,
        currents: rhs,
        z_in,
        p_in,
        n_unknowns: n,
        n_segs: ns,
        t_fill_ms: t_fill,
        t_solve_ms: t_solve,
    })
}

fn find_gap(mesh: &Mesh, wire: usize, pos: f64) -> Option<&Gap> {
    mesh.wire_gaps
        .get(wire)?
        .iter()
        .min_by(|a, b| (a.frac - pos).abs().partial_cmp(&(b.frac - pos).abs()).unwrap())
}

/// Dense LU with partial pivoting; rows are eliminated in parallel.
pub fn lu_solve(a: &mut [C], b: &mut [C], n: usize) -> Result<(), String> {
    for k in 0..n {
        let mut p = k;
        let mut best = a[k * n + k].norm_sqr();
        for i in k + 1..n {
            let v = a[i * n + k].norm_sqr();
            if v > best {
                best = v;
                p = i;
            }
        }
        if best == 0.0 || !best.is_finite() {
            return Err("The matrix is singular (check for overlapping or duplicate wires).".into());
        }
        if p != k {
            for j in 0..n {
                a.swap(k * n + j, p * n + j);
            }
            b.swap(k, p);
        }
        let (top, bottom) = a.split_at_mut((k + 1) * n);
        let rowk = &top[k * n..];
        let piv = rowk[k];
        let bk = b[k];
        let (_, b_rest) = b.split_at_mut(k + 1);
        let work = |(row, bi): (&mut [C], &mut C)| {
            let f = row[k] / piv;
            if f.re != 0.0 || f.im != 0.0 {
                row[k] = f;
                for j in k + 1..n {
                    row[j] -= f * rowk[j];
                }
                *bi -= f * bk;
            }
        };
        if n - k > 48 {
            bottom.par_chunks_mut(n).zip(b_rest.par_iter_mut()).for_each(work);
        } else {
            bottom.chunks_mut(n).zip(b_rest.iter_mut()).for_each(work);
        }
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= a[i * n + j] * b[j];
        }
        b[i] = s / a[i * n + i];
    }
    Ok(())
}

// ------------------------------------------------------------------ far field

/// Current elements for far-field sums.
pub struct Radiator {
    pos: Vec<V3>,
    cur: Vec<[C; 3]>,
    k: f64,
    ground: Ground,
    lambda: f64,
    gain_const: f64,
    /// gain computed by another engine instead (az, el in degrees)
    external: Option<GainFn>,
}

/// An engine's own gain function: (azimuth, elevation) in degrees -> gain.
pub type GainFn = std::sync::Arc<dyn Fn(f64, f64) -> Gain + Send + Sync>;

#[derive(Clone, Copy, Debug, Default)]
pub struct Gain {
    pub total: f64,
    pub hor: f64,
    pub ver: f64,
}

pub fn db(x: f64) -> f64 {
    // floor used by the original far-field code (FFSolver 0x4093e0)
    if x > 1e-30 { 10.0 * x.log10() } else { -999.0 }
}

impl Radiator {
    pub fn new(ant: &Antenna, mesh: &Mesh, sol: &Solution) -> Radiator {
        let mut pos = Vec::new();
        let mut cur = Vec::new();
        for (b, i) in mesh.bases.iter().zip(sol.currents.iter()) {
            for p in &b.parts {
                let g = part_geo(mesh, p);
                pos.push(g.hc);
                cur.push([*i * g.l[0], *i * g.l[1], *i * g.l[2]]);
            }
        }
        let f = sol.freq_mhz * 1e6;
        let omega = 2.0 * PI * f;
        let c = omega * MU0 / (4.0 * PI);
        // G = |r·E|² · 0.033356 / P, with P = Re(V·I*) (twice the input power), as in
        // FFSolver 0x4093e0 (DAT_00425d40); |r·E| = (ωμ0/4π)·|F⊥|.
        const GAIN_NORM: f64 = 0.033356;
        let gain_const = if sol.p_in > 0.0 { c * c * GAIN_NORM / (2.0 * sol.p_in) } else { 0.0 };
        Radiator { pos, cur, k: omega / C0, ground: ant.ground.clone(), lambda: C0 / f, gain_const, external: None }
    }

    /// Radiator whose gain comes from another engine's own far-field code.
    pub fn from_fn(freq_mhz: f64, ground: &Ground, gain: GainFn) -> Radiator {
        let f = freq_mhz * 1e6;
        Radiator { pos: vec![], cur: vec![], k: 2.0 * PI * f / C0, ground: ground.clone(), lambda: C0 / f, gain_const: 0.0, external: Some(gain) }
    }

    fn sum(&self, r: V3, image: bool) -> [C; 3] {
        let mut s = [C::new(0.0, 0.0); 3];
        for (p, c) in self.pos.iter().zip(self.cur.iter()) {
            let (pp, sg) = if image { (mirror(*p), [-1.0, -1.0, 1.0]) } else { (*p, [1.0, 1.0, 1.0]) };
            let ph = C::from_polar(1.0, self.k * dot(r, pp));
            s[0] += c[0] * ph * sg[0];
            s[1] += c[1] * ph * sg[1];
            s[2] += c[2] * ph * sg[2];
        }
        s
    }

    /// Gain toward azimuth/elevation (degrees). Azimuth 0 = +X, elevation 0 = horizon.
    pub fn gain(&self, az: f64, el: f64) -> Gain {
        if let Some(f) = &self.external {
            return f(az, el);
        }
        let (az, el) = (az.to_radians(), el.to_radians());
        let r = [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()];
        let th = [el.sin() * az.cos(), el.sin() * az.sin(), -el.cos()];
        let ph = [-az.sin(), az.cos(), 0.0];
        let proj = |v: &[C; 3], u: V3| v[0] * u[0] + v[1] * u[1] + v[2] * u[2];
        let d = self.sum(r, false);
        let (mut et, mut ep) = (proj(&d, th), proj(&d, ph));
        match self.ground.kind {
            GroundKind::Free => {}
            _ if el < 0.0 => return Gain::default(),
            GroundKind::Perfect => {
                let i = self.sum(r, true);
                et += proj(&i, th);
                ep += proj(&i, ph);
            }
            GroundKind::Real => {
                let i = self.sum(r, true);
                // ε_c = εr − jσ/(f·2π·ε0·10⁶), f in MHz, σ in S/m (FFSolver 0x4093e0, DAT_00425d48)
                const TWO_PI_EPS0_E6: f64 = 5.5606189968539335e-05;
                let f_mhz = C0 / self.lambda * 1e-6;
                let ec = C::new(self.ground.eps_r, -(self.ground.sigma_ms * 1e-3) / (f_mhz * TWO_PI_EPS0_E6));
                let s = el.sin();
                let c2 = el.cos() * el.cos();
                let root = (ec - c2).sqrt();
                let rv = (ec * s - root) / (ec * s + root);
                let rh = (s - root) / (s + root);
                et += rv * proj(&i, th);
                ep += -rh * proj(&i, ph);
            }
        }
        let gv = et.norm_sqr() * self.gain_const;
        let gh = ep.norm_sqr() * self.gain_const;
        Gain { total: gv + gh, hor: gh, ver: gv }
    }

    pub fn has_ground(&self) -> bool {
        self.ground.kind != GroundKind::Free
    }

    /// Direction of maximum gain: (az, el, gain).
    pub fn find_max(&self) -> (f64, f64, Gain) {
        let (el0, el1) = if self.has_ground() { (1.0, 90.0) } else { (-90.0, 90.0) };
        let mut dirs = Vec::new();
        let mut el = el0;
        while el <= el1 + 1e-9 {
            let mut az = 0.0;
            while az < 360.0 {
                dirs.push((az, el));
                az += 3.0;
            }
            el += 3.0;
        }
        let best = dirs
            .par_iter()
            .map(|&(a, e)| (a, e, self.gain(a, e)))
            .reduce(|| (0.0, 0.0, Gain { total: -1.0, ..Default::default() }), |x, y| if y.2.total > x.2.total { y } else { x });
        // refine
        let mut fine = Vec::new();
        for i in -6..=6 {
            for j in -6..=6 {
                let e = (best.1 + j as f64 * 0.5).clamp(el0, el1);
                fine.push(((best.0 + i as f64 * 0.5).rem_euclid(360.0), e));
            }
        }
        fine.par_iter()
            .map(|&(a, e)| (a, e, self.gain(a, e)))
            .reduce(|| best, |x, y| if y.2.total > x.2.total { y } else { x })
    }
}

// ------------------------------------------------------------------ high level

#[derive(Clone, Debug)]
pub struct CalcResult {
    pub freq_mhz: f64,
    pub z: C,
    pub swr: f64,
    pub ga_dbi: f64,
    pub fb_db: f64,
    pub az_max: f64,
    pub el_max: f64,
    pub horizontal_pol: bool,
    pub ground: GroundKind,
    pub add_height: f64,
    pub material: Material,
    pub threads: usize,
    pub n_unknowns: usize,
    pub n_segs: usize,
    pub t_total_ms: f64,
    pub t_fill_ms: f64,
    pub t_solve_ms: f64,
    pub t_far_ms: f64,
}

#[derive(Clone, Debug)]
pub struct Patterns {
    /// azimuth cut at `el_cut`: (az deg, gain)
    pub az_cut: Vec<(f64, Gain)>,
    pub el_cut_deg: f64,
    /// elevation cut through azimuth `az_cut_deg`: (angle from front horizon, gain)
    pub el_cut: Vec<(f64, Gain)>,
    pub az_cut_deg: f64,
    pub free_space: bool,
    /// 3D pattern: total gain (linear) at `grid_els[i]` elevation, `j * grid_step` azimuth.
    pub grid: Vec<Vec<f64>>,
    pub grid_els: Vec<f64>,
    pub grid_step: f64,
}

#[derive(Clone, Debug)]
pub struct FullResult {
    pub result: CalcResult,
    pub patterns: Patterns,
    pub mesh: Mesh,
    pub currents: Vec<C>,
    pub warnings: Vec<String>,
    /// Per-segment current magnitude (start, end, |I|) when an external engine solved it.
    pub seg_currents: Option<Vec<(V3, V3, f64)>>,
}

pub fn swr(z: C, z0: f64) -> f64 {
    let g = ((z - z0) / (z + z0)).norm();
    if g >= 1.0 { f64::INFINITY } else { (1.0 + g) / (1.0 - g) }
}

/// Largest gain in the rear range used for F/B: `az_w` degrees of azimuth centred on the back
/// and `el_w` degrees of elevation (from the horizon over ground, centred on the main-lobe
/// elevation in free space). 0/0 means the single point straight behind.
/// (Rear range values: example .maa files and english.mmn "Rear range for F/B"; how the
/// original scans the range is my reading of the setting.)
pub fn rear_max(r: &Radiator, az: f64, el: f64, az_w: f64, el_w: f64) -> Gain {
    let back_az = (az + 180.0) % 360.0;
    if az_w <= 0.0 && el_w <= 0.0 {
        return r.gain(back_az, el);
    }
    let (e0, e1) = if r.has_ground() { (el.min(el_w).max(0.5), el_w.max(el).min(90.0)) } else { (el - el_w / 2.0, el + el_w / 2.0) };
    let (e0, e1) = (e0.min(e1), e0.max(e1));
    let n_az = ((az_w / 2.0).ceil() as i64).max(0);
    let n_el = (((e1 - e0) / 2.0).ceil() as i64).max(0);
    let dirs: Vec<(f64, f64)> = (-n_az..=n_az)
        .flat_map(|i| (0..=n_el).map(move |j| (i, j)))
        .map(|(i, j)| {
            let a = back_az + (i as f64) * (az_w / 2.0) / (n_az.max(1) as f64);
            let e = if n_el == 0 { e0 } else { e0 + (e1 - e0) * j as f64 / n_el as f64 };
            (a.rem_euclid(360.0), e)
        })
        .collect();
    dirs.par_iter()
        .map(|&(a, e)| r.gain(a, e))
        .reduce(Gain::default, |x, y| if y.total > x.total { y } else { x })
}

pub fn empty_mesh() -> Mesh {
    Mesh { segs: vec![], bases: vec![], wire_gaps: vec![], wire_segs: vec![], warnings: vec![], image: false }
}

pub fn quick(ant: &Antenna, freq_mhz: f64, seg_freq_mhz: f64, need_far: bool) -> Result<(CalcResult, Mesh, Solution, Option<Radiator>), String> {
    match ant.engine {
        Engine::Nec2 => {
            let r = crate::nec::quick(ant, freq_mhz, seg_freq_mhz, need_far)?;
            let sol = Solution { freq_mhz, currents: vec![], z_in: vec![r.z], p_in: 0.0, n_unknowns: r.n_unknowns, n_segs: r.n_segs, t_fill_ms: 0.0, t_solve_ms: r.t_solve_ms };
            return Ok((r, empty_mesh(), sol, None));
        }
        Engine::Wiremom => {
            let (r, sol, rad, ..) = crate::wm::quick(ant, freq_mhz, seg_freq_mhz, need_far)?;
            return Ok((r, empty_mesh(), sol, rad));
        }
        Engine::Mininec => {}
    }
    if !ant.lines.is_empty() {
        return Err("Transmission lines need the wiremom or NEC-2 engine (choose it under Calculate → Engine).".into());
    }
    let t0 = Instant::now();
    let mesh = build_mesh(ant, 299.792458 / seg_freq_mhz)?;
    let sol = solve(ant, &mesh, freq_mhz)?;
    let rad = need_far.then(|| Radiator::new(ant, &mesh, &sol));
    let res = summarize(ant, freq_mhz, t0, &sol, rad.as_ref());
    Ok((res, mesh, sol, rad))
}

/// CalcResult for a solved antenna: impedance, SWR and (with a radiator) the gain
/// maximum and F/B.  `t0` is when the calculation started.
pub fn summarize(ant: &Antenna, freq_mhz: f64, t0: Instant, sol: &Solution, rad: Option<&Radiator>) -> CalcResult {
    let z = sol.z_in[0];
    let t2 = Instant::now();
    let (mut ga, mut fb, mut az, mut el, mut hp) = (f64::NAN, f64::NAN, 0.0, 0.0, true);
    if let Some(r) = rad {
        let (a, e, g) = r.find_max();
        let back = rear_max(r, a, e, ant.fb_az, ant.fb_el);
        ga = db(g.total);
        fb = db(g.total) - db(back.total);
        az = a;
        el = e;
        hp = g.hor >= g.ver;
    }
    let t_far = t2.elapsed().as_secs_f64() * 1e3;
    CalcResult {
        freq_mhz,
        z,
        swr: swr(z, ant.z0),
        ga_dbi: ga,
        fb_db: fb,
        az_max: az,
        el_max: el,
        horizontal_pol: hp,
        ground: ant.ground.kind,
        add_height: ant.ground.add_height,
        material: ant.material,
        threads: rayon::current_num_threads(),
        n_unknowns: sol.n_unknowns,
        n_segs: sol.n_segs,
        t_total_ms: t0.elapsed().as_secs_f64() * 1e3,
        t_fill_ms: sol.t_fill_ms,
        t_solve_ms: sol.t_solve_ms,
        t_far_ms: t_far,
    }
}

pub fn calculate(ant: &Antenna) -> Result<FullResult, String> {
    match ant.engine {
        Engine::Nec2 => return crate::nec::calculate(ant),
        Engine::Wiremom => return crate::wm::calculate(ant),
        Engine::Mininec => {}
    }
    let (mut res, mesh, sol, rad) = quick(ant, ant.freq_mhz, ant.freq_mhz, true)?;
    let patterns = patterns(ant, &mut res, &rad.unwrap());
    let warnings = mesh.warnings.clone();
    Ok(FullResult { result: res, patterns, currents: sol.currents, mesh, warnings, seg_currents: None })
}

/// Pattern cuts through the maximum and the 3D grid; adds the time taken to `res`.
pub fn patterns(ant: &Antenna, res: &mut CalcResult, rad: &Radiator) -> Patterns {
    let t = Instant::now();
    let el_cut = res.el_max;
    let az_cut: Vec<(f64, Gain)> = (0..=360).into_par_iter().map(|i| {
        let a = i as f64;
        (a, rad.gain(a, el_cut))
    }).collect();
    let azf = res.az_max;
    let el_pts: Vec<(f64, Gain)> = (0..=360).into_par_iter().map(|i| {
        let t = i as f64;
        // angle t from the front horizon, over the zenith to the back
        let (a, e) = if t <= 90.0 {
            (azf, t)
        } else if t <= 270.0 {
            ((azf + 180.0) % 360.0, 180.0 - t)
        } else {
            (azf, t - 360.0)
        };
        (t, rad.gain(a, e))
    }).collect();
    let grid_step = 3.0;
    let el0 = if ant.ground.kind == GroundKind::Free { -90.0 } else { 0.0 };
    let n_el = ((90.0 - el0) / grid_step) as usize + 1;
    let grid_els: Vec<f64> = (0..n_el).map(|i| el0 + i as f64 * grid_step).collect();
    let n_az = (360.0 / grid_step) as usize;
    let grid: Vec<Vec<f64>> = grid_els
        .par_iter()
        .map(|&e| (0..n_az).map(|j| rad.gain(j as f64 * grid_step, e).total).collect())
        .collect();
    res.t_far_ms += t.elapsed().as_secs_f64() * 1e3;
    res.t_total_ms += t.elapsed().as_secs_f64() * 1e3;
    Patterns {
        az_cut,
        el_cut_deg: el_cut,
        el_cut: el_pts,
        az_cut_deg: azf,
        free_space: ant.ground.kind == GroundKind::Free,
        grid,
        grid_els,
        grid_step,
    }
}

#[derive(Clone, Debug)]
pub struct SweepPoint {
    pub freq_mhz: f64,
    pub z: C,
    pub swr: f64,
    pub ga_dbi: f64,
    pub fb_db: f64,
}

#[derive(Clone, Debug)]
pub struct SweepResult {
    pub points: Vec<SweepPoint>,
    pub t_ms: f64,
    pub threads: usize,
}

/// Frequency sweep: every frequency is solved on its own thread.
pub fn sweep(ant: &Antenna, f0: f64, f1: f64, steps: usize, with_gain: bool) -> Result<SweepResult, String> {
    let t = Instant::now();
    let steps = steps.max(2);
    let seg_f = f1.max(f0).max(ant.freq_mhz);
    if ant.engine == Engine::Wiremom {
        let freqs: Vec<f64> = (0..steps).map(|i| f0 + (f1 - f0) * i as f64 / (steps - 1) as f64).collect();
        crate::wm::prepare_sweep(ant, &freqs, seg_f);
    }
    let pts: Result<Vec<SweepPoint>, String> = (0..steps)
        .into_par_iter()
        .map(|i| {
            let f = f0 + (f1 - f0) * i as f64 / (steps - 1) as f64;
            let (r, ..) = quick(ant, f, seg_f, with_gain)?;
            Ok(SweepPoint { freq_mhz: f, z: r.z, swr: r.swr, ga_dbi: r.ga_dbi, fb_db: r.fb_db })
        })
        .collect();
    Ok(SweepResult { points: pts?, t_ms: t.elapsed().as_secs_f64() * 1e3, threads: rayon::current_num_threads() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dipole_free_space() {
        let mut a = Antenna::example(0);
        a.material = Material::Lossless;
        let r = calculate(&a).unwrap().result;
        println!("dipole Z={:.2} ga={:.2} n={}", r.z, r.ga_dbi, r.n_unknowns);
        assert!(r.z.re > 60.0 && r.z.re < 85.0, "{:?}", r.z);
        assert!((r.ga_dbi - 2.15).abs() < 0.2, "{}", r.ga_dbi);
    }

    #[test]
    fn dipole_uniform() {
        let mut a = Antenna::example(0);
        a.material = Material::Lossless;
        a.wires[0].seg = 21;
        let r = calculate(&a).unwrap().result;
        println!("dipole uniform Z={:.2} ga={:.2}", r.z, r.ga_dbi);
    }

    #[test]
    fn monopole() {
        let mut a = Antenna::example(3);
        a.material = Material::Lossless;
        let r = calculate(&a).unwrap().result;
        println!("monopole Z={:.2} ga={:.2} el={}", r.z, r.ga_dbi, r.el_max);
        assert!(r.z.re > 30.0 && r.z.re < 45.0);
        assert!((r.ga_dbi - 5.15).abs() < 0.3);
    }

    #[test]
    fn yagi_and_others() {
        for i in 0..6 {
            let a = Antenna::example(i);
            let r = calculate(&a).unwrap().result;
            println!("{}: Z={:.2} swr={:.2} ga={:.2} fb={:.2} el={:.1} az={:.1} n={} t={:.1}ms", a.name, r.z, r.swr, r.ga_dbi, r.fb_db, r.el_max, r.az_max, r.n_unknowns, r.t_total_ms);
        }
    }
}

#[cfg(test)]
mod conv {
    use super::*;
    #[test]
    fn convergence() {
        for n in [11, 21, 41, 81, 161, 321] {
            let mut a = Antenna::example(0);
            a.material = Material::Lossless;
            a.wires[0].seg = n;
            let (r, ..) = quick(&a, a.freq_mhz, a.freq_mhz, false).unwrap();
            println!("seg {n}: Z={:.2} fill {:.1}ms solve {:.1}ms", r.z, r.t_fill_ms, r.t_solve_ms);
        }
    }
}

#[cfg(test)]
mod conv2 {
    use super::*;
    #[test]
    fn halfwave_thin() {
        for (n, arel) in [(41, 1e-5), (81, 1e-5), (81, 1e-4), (81,1e-3)] {
            let mut a = Antenna::example(0);
            a.material = Material::Lossless;
            let lam = a.lambda();
            a.wires[0] = Wire { p1: [0.0, -lam / 4.0, 0.0], p2: [0.0, lam / 4.0, 0.0], r: arel * lam, seg: n, ins_t: 0.0, ins_eps: 1.0 };
            let (r, ..) = quick(&a, a.freq_mhz, a.freq_mhz, false).unwrap();
            println!("halfwave seg {n} a/l {arel}: Z={:.2}", r.z);
        }
    }
}

#[cfg(test)]
mod fb_range {
    use super::*;
    #[test]
    fn rear_range_changes_fb() {
        let mut a = Antenna::example(5);
        for (az, el) in [(0.0, 0.0), (120.0, 60.0), (180.0, 90.0)] {
            a.fb_az = az;
            a.fb_el = el;
            let (r, ..) = quick(&a, a.freq_mhz, a.freq_mhz, true).unwrap();
            println!("rear range {az}/{el}: F/B {:.2} dB", r.fb_db);
        }
    }
}
