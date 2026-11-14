//! Simple multi-threaded optimizer: pattern search where all trial steps of an
//! iteration are evaluated at once on the thread pool.

use crate::model::*;
use crate::mom;
use crate::progress::Progress;
use rayon::prelude::*;
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VarKind {
    Length,
    MoveX,
    MoveY,
    MoveZ,
    X1,
    Y1,
    Z1,
    X2,
    Y2,
    Z2,
}

impl VarKind {
    pub const ALL: [VarKind; 10] = [
        VarKind::Length, VarKind::MoveX, VarKind::MoveY, VarKind::MoveZ,
        VarKind::X1, VarKind::Y1, VarKind::Z1, VarKind::X2, VarKind::Y2, VarKind::Z2,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            VarKind::Length => "Length (joined ends stay)",
            VarKind::MoveX => "Move along X",
            VarKind::MoveY => "Move along Y",
            VarKind::MoveZ => "Move along Z",
            VarKind::X1 => "X1",
            VarKind::Y1 => "Y1",
            VarKind::Z1 => "Z1",
            VarKind::X2 => "X2",
            VarKind::Y2 => "Y2",
            VarKind::Z2 => "Z2",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptVar {
    pub wire: usize,
    pub kind: VarKind,
}

#[derive(Clone, Debug)]
pub struct Goals {
    pub w_swr: f64,
    pub w_jx: f64,
    pub w_ga: f64,
    pub w_fb: f64,
    pub max_iter: usize,
}

impl Default for Goals {
    fn default() -> Self {
        Goals { w_swr: 1.0, w_jx: 1.0, w_ga: 0.0, w_fb: 0.0, max_iter: 60 }
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    pub iter: usize,
    pub cost: f64,
    pub r: f64,
    pub x: f64,
    pub swr: f64,
    pub ga: f64,
    pub fb: f64,
    pub evals: usize,
}

pub enum Msg {
    Progress(Step),
    /// `failed` trial antennas couldn't be calculated; `error` is the first reason.
    Done { ant: Antenna, steps: usize, evals: usize, ms: f64, failed: usize, error: Option<String> },
    Failed(String),
}

/// Wire ends closer than this are joined (as the wire editor treats them).
const JOIN_TOL: f64 = 1e-6;

/// Which wire ends are joined to each other: `group[w][e]` for end e (0 = p1, 1 = p2)
/// of wire w; ends with the same number are one joint.
fn joints(a: &Antenna) -> (Vec<[usize; 2]>, Vec<usize>) {
    let pts: Vec<[f64; 3]> = a.wires.iter().flat_map(|w| [w.p1, w.p2]).collect();
    let mut id = vec![usize::MAX; pts.len()];
    let mut size = Vec::new();
    for i in 0..pts.len() {
        if id[i] != usize::MAX {
            continue;
        }
        let g = size.len();
        size.push(0);
        for j in i..pts.len() {
            if id[j] == usize::MAX && (0..3).all(|k| (pts[i][k] - pts[j][k]).abs() < JOIN_TOL) {
                id[j] = g;
                size[g] += 1;
            }
        }
    }
    ((0..a.wires.len()).map(|w| [id[2 * w], id[2 * w + 1]]).collect(), size)
}

/// The antenna with the variables changed by `d` (metres).  Joined wire ends stay joined:
/// moving an end moves every end joined to it.  "Length" keeps an end that is joined to
/// another wire or stands on the ground where it is and changes the wire at its free end;
/// it changes both ends only when neither or both are held.
pub fn apply(base: &Antenna, vars: &[OptVar], d: &[f64]) -> Antenna {
    let mut a = base.clone();
    let (group, size) = joints(base);
    let grounded = |p: [f64; 3]| base.ground.kind != GroundKind::Free && (p[2] + base.ground.add_height).abs() < JOIN_TOL;
    // how firmly an end is held: joined to another wire, standing on the ground
    let held = |w: usize, e: usize| {
        let p = if e == 0 { base.wires[w].p1 } else { base.wires[w].p2 };
        (size[group[w][e]] > 1) as u8 + grounded(p) as u8
    };
    // the move of each joint
    let mut shift = vec![[0.0f64; 3]; size.len()];
    let mut add = |w: usize, e: usize, v: [f64; 3]| {
        let g = group[w][e];
        for k in 0..3 {
            shift[g][k] += v[k];
        }
    };
    for (v, &dv) in vars.iter().zip(d) {
        let Some(w) = base.wires.get(v.wire) else { continue };
        let axis = |i: usize| {
            let mut u = [0.0; 3];
            u[i] = dv;
            u
        };
        match v.kind {
            VarKind::Length => {
                let len = w.length();
                if len <= 0.0 {
                    continue;
                }
                let u: Vec<f64> = (0..3).map(|k| (w.p2[k] - w.p1[k]) / len).collect();
                let out = |s: f64, sign: f64| [0, 1, 2].map(|k| sign * s * u[k]);
                // don't shrink a wire below 1 % of its length
                let dv = dv.max(-0.99 * len);
                let (h1, h2) = (held(v.wire, 0), held(v.wire, 1));
                if h1 > h2 {
                    add(v.wire, 1, out(dv, 1.0));
                } else if h2 > h1 {
                    add(v.wire, 0, out(dv, -1.0));
                } else {
                    add(v.wire, 0, out(dv / 2.0, -1.0));
                    add(v.wire, 1, out(dv / 2.0, 1.0));
                }
            }
            VarKind::MoveX | VarKind::MoveY | VarKind::MoveZ => {
                let u = axis(match v.kind { VarKind::MoveX => 0, VarKind::MoveY => 1, _ => 2 });
                add(v.wire, 0, u);
                // a wire whose ends are joined to each other (a closed loop) moves once
                if group[v.wire][1] != group[v.wire][0] {
                    add(v.wire, 1, u);
                }
            }
            VarKind::X1 => add(v.wire, 0, axis(0)),
            VarKind::Y1 => add(v.wire, 0, axis(1)),
            VarKind::Z1 => add(v.wire, 0, axis(2)),
            VarKind::X2 => add(v.wire, 1, axis(0)),
            VarKind::Y2 => add(v.wire, 1, axis(1)),
            VarKind::Z2 => add(v.wire, 1, axis(2)),
        }
    }
    for (w, wire) in a.wires.iter_mut().enumerate() {
        for k in 0..3 {
            wire.p1[k] += shift[group[w][0]][k];
            wire.p2[k] += shift[group[w][1]][k];
        }
    }
    a
}

fn evaluate(ant: &Antenna, g: &Goals) -> Result<(f64, Step), String> {
    let need_far = g.w_ga > 0.0 || g.w_fb > 0.0;
    let (r, ..) = mom::quick(ant, ant.freq_mhz, ant.freq_mhz, need_far)?;
    let swr = if r.swr.is_finite() { r.swr.min(100.0) } else { 100.0 };
    let mut cost = g.w_swr * (swr - 1.0) + g.w_jx * r.z.im.abs() / ant.z0;
    if need_far {
        cost -= g.w_ga * r.ga_dbi / 2.0;
        cost -= g.w_fb * r.fb_db.min(35.0) / 10.0;
    }
    Ok((cost, Step { iter: 0, cost, r: r.z.re, x: r.z.im, swr: r.swr, ga: r.ga_dbi, fb: r.fb_db, evals: 0 }))
}

/// Counts the steps on `prog` (out of the most allowed: it often finishes sooner)
/// and stops when asked.
pub fn run(base: Antenna, vars: Vec<OptVar>, goals: Goals, prog: Arc<Progress>, tx: Sender<Msg>, wake: impl Fn()) {
    let t = std::time::Instant::now();
    prog.stage_at_most("Optimizer steps", goals.max_iter);
    let lam = base.lambda();
    let mut d = vec![0.0; vars.len()];
    let mut step = vec![lam * 0.01; vars.len()];
    let min_step = lam * 2e-5;
    let (mut best, mut st) = match evaluate(&base, &goals) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Msg::Failed(format!("The starting antenna can't be calculated: {e}")));
            wake();
            return;
        }
    };
    let mut failed = 0;
    let mut error: Option<String> = None;
    let mut note = |r: &Result<(f64, Step), String>| {
        if let Err(e) = r {
            failed += 1;
            error.get_or_insert_with(|| e.clone());
        }
    };
    let mut evals = 1;
    let _ = tx.send(Msg::Progress(Step { evals, ..st.clone() }));
    wake();
    let mut it = 0;
    while it < goals.max_iter && !prog.stopping() {
        it += 1;
        if step.iter().all(|s| *s < min_step) {
            break;
        }
        let trials: Vec<Vec<f64>> = (0..vars.len())
            .flat_map(|i| [1.0, -1.0].into_iter().map(move |sg| (i, sg)))
            .map(|(i, sg)| {
                let mut x = d.clone();
                x[i] += sg * step[i];
                x
            })
            .collect();
        evals += trials.len();
        let res: Vec<Result<(f64, Step), String>> =
            trials.par_iter().map(|x| evaluate(&apply(&base, &vars, x), &goals)).collect();
        res.iter().for_each(&mut note);
        let mut improved = false;
        let mut bi = None;
        for (k, r) in res.iter().enumerate() {
            if let Ok((c, _)) = r {
                if *c < best - 1e-9 && bi.map(|(_, b)| *c < b).unwrap_or(true) {
                    bi = Some((k, *c));
                }
            }
        }
        if let Some((k, c)) = bi {
            let i = k / 2;
            let prev = d.clone();
            d = trials[k].clone();
            best = c;
            st = res[k].clone().unwrap().1;
            improved = true;
            step[i] *= 1.5;
            // try continuing in the same direction (a cheap line search)
            let mut x = d.clone();
            x[i] += d[i] - prev[i];
            let r2 = evaluate(&apply(&base, &vars, &x), &goals);
            evals += 1;
            note(&r2);
            if let Ok((c2, s2)) = r2 {
                if c2 < best {
                    best = c2;
                    st = s2;
                    d = x;
                }
            }
        }
        if !improved {
            for s in step.iter_mut() {
                *s *= 0.4;
            }
        }
        prog.tick();
        let _ = tx.send(Msg::Progress(Step { iter: it, evals, ..st.clone() }));
        wake();
    }
    let _ = tx.send(Msg::Done { ant: apply(&base, &vars, &d), steps: it, evals, ms: t.elapsed().as_secs_f64() * 1e3, failed, error });
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resonate_dipole() {
        let a = Antenna::example(0);
        let (tx, rx) = std::sync::mpsc::channel();
        run(a.clone(), vec![OptVar { wire: 0, kind: VarKind::Length }], Goals::default(), Arc::new(Progress::default()), tx, || {});
        let mut last = None;
        for m in rx.try_iter() {
            if let Msg::Progress(s) = &m { println!("step {} cost {:.4} x {:.2}", s.iter, s.cost, s.x); }
            last = Some(m);
        }
        let Some(Msg::Done { ant, .. }) = last else { panic!() };
        let (r, ..) = mom::quick(&ant, ant.freq_mhz, ant.freq_mhz, false).unwrap();
        println!("len {:.4} -> {:.4}, Z={:.2}", a.wires[0].length(), ant.wires[0].length(), r.z);
        assert!(r.z.im.abs() < 3.0);
    }

    fn joint_count(a: &Antenna) -> Vec<usize> {
        let mut s = joints(a).1;
        s.sort();
        s
    }

    #[test]
    fn length_keeps_joints_and_ground() {
        // inverted V: the joined top stays, the free end moves
        let a = Antenna::example(1);
        let b = apply(&a, &[OptVar { wire: 0, kind: VarKind::Length }], &[0.3]);
        assert_eq!(b.wires[0].p1, a.wires[0].p1);
        assert!((b.wires[0].length() - a.wires[0].length() - 0.3).abs() < 1e-9);
        assert_eq!(b.wires[1], a.wires[1]);
        // vertical on perfect ground: the base stays on the ground
        let a = Antenna::example(3);
        let b = apply(&a, &[OptVar { wire: 0, kind: VarKind::Length }], &[-0.2]);
        assert_eq!(b.wires[0].p1, a.wires[0].p1);
        assert!((b.wires[0].length() - a.wires[0].length() + 0.2).abs() < 1e-9);
        // quad: a side joined at both ends grows at both, and the loop stays closed
        let a = Antenna::example(5);
        let vars = [OptVar { wire: 0, kind: VarKind::Length }, OptVar { wire: 1, kind: VarKind::MoveX }, OptVar { wire: 6, kind: VarKind::Z2 }];
        let b = apply(&a, &vars, &[0.2, 0.1, -0.05]);
        assert_eq!(joint_count(&b), joint_count(&a));
        let b = apply(&a, &vars[..1], &[0.2]);
        assert_eq!(joint_count(&b), joint_count(&a));
        assert!((b.wires[0].length() - a.wires[0].length() - 0.2).abs() < 1e-9);
        // separate Yagi elements still grow about their centres
        let a = Antenna::example(2);
        let b = apply(&a, &[OptVar { wire: 0, kind: VarKind::Length }], &[0.4]);
        assert!((b.wires[0].p1[1] - (a.wires[0].p1[1] - 0.2)).abs() < 1e-9);
        assert!((b.wires[0].p2[1] - (a.wires[0].p2[1] + 0.2)).abs() < 1e-9);
    }

    #[test]
    fn resonates_joined_antennas() {
        // these didn't change at all when "Length" pulled the joints apart
        for i in [1, 3, 5] {
            let mut a = Antenna::example(i);
            a.engine = Engine::Mininec;
            let vars: Vec<OptVar> = (0..a.wires.len().min(3)).map(|w| OptVar { wire: w, kind: VarKind::Length }).collect();
            let (tx, rx) = std::sync::mpsc::channel();
            run(a.clone(), vars, Goals::default(), Arc::new(Progress::default()), tx, || {});
            let Some(Msg::Done { ant, failed, error, .. }) = rx.try_iter().last() else { panic!() };
            assert_eq!(failed, 0, "{}: {:?}", a.name, error);
            let (r, ..) = mom::quick(&ant, ant.freq_mhz, ant.freq_mhz, false).unwrap();
            assert!(r.z.im.abs() < 3.0, "{}: {:?}", a.name, r.z);
        }
    }

    #[test]
    fn thread_speedup() {
        let a = Antenna::example(2);
        for n in [1, 2, 4, 12] {
            let p = rayon::ThreadPoolBuilder::new().num_threads(n).build().unwrap();
            let r = p.install(|| mom::sweep(&a, 13.8, 14.5, 48, true)).unwrap();
            println!("threads {n}: sweep 48 freqs {:.0} ms", r.t_ms);
        }
    }
}
