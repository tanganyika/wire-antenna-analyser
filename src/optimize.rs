//! Simple multi-threaded optimizer: pattern search where all trial steps of an
//! iteration are evaluated at once on the thread pool.

use crate::model::*;
use crate::mom;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
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
            VarKind::Length => "Length (about centre)",
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
    Done { ant: Antenna, steps: usize, evals: usize, ms: f64 },
    Failed(String),
}

pub fn apply(base: &Antenna, vars: &[OptVar], d: &[f64]) -> Antenna {
    let mut a = base.clone();
    for (v, &dv) in vars.iter().zip(d) {
        let Some(w) = a.wires.get_mut(v.wire) else { continue };
        match v.kind {
            VarKind::Length => {
                let len = w.length();
                if len > 0.0 {
                    let s = ((len + dv) / len).max(0.01);
                    let c: Vec<f64> = (0..3).map(|i| 0.5 * (w.p1[i] + w.p2[i])).collect();
                    for i in 0..3 {
                        w.p1[i] = c[i] + (w.p1[i] - c[i]) * s;
                        w.p2[i] = c[i] + (w.p2[i] - c[i]) * s;
                    }
                }
            }
            VarKind::MoveX | VarKind::MoveY | VarKind::MoveZ => {
                let i = match v.kind { VarKind::MoveX => 0, VarKind::MoveY => 1, _ => 2 };
                w.p1[i] += dv;
                w.p2[i] += dv;
            }
            VarKind::X1 => w.p1[0] += dv,
            VarKind::Y1 => w.p1[1] += dv,
            VarKind::Z1 => w.p1[2] += dv,
            VarKind::X2 => w.p2[0] += dv,
            VarKind::Y2 => w.p2[1] += dv,
            VarKind::Z2 => w.p2[2] += dv,
        }
    }
    a
}

fn evaluate(ant: &Antenna, g: &Goals) -> Option<(f64, Step)> {
    let need_far = g.w_ga > 0.0 || g.w_fb > 0.0;
    let (r, ..) = mom::quick(ant, ant.freq_mhz, ant.freq_mhz, need_far).ok()?;
    let swr = if r.swr.is_finite() { r.swr.min(100.0) } else { 100.0 };
    let mut cost = g.w_swr * (swr - 1.0) + g.w_jx * r.z.im.abs() / ant.z0;
    if need_far {
        cost -= g.w_ga * r.ga_dbi / 2.0;
        cost -= g.w_fb * r.fb_db.min(35.0) / 10.0;
    }
    Some((cost, Step { iter: 0, cost, r: r.z.re, x: r.z.im, swr: r.swr, ga: r.ga_dbi, fb: r.fb_db, evals: 0 }))
}

pub fn run(base: Antenna, vars: Vec<OptVar>, goals: Goals, stop: Arc<AtomicBool>, tx: Sender<Msg>, wake: impl Fn()) {
    let t = std::time::Instant::now();
    let lam = base.lambda();
    let mut d = vec![0.0; vars.len()];
    let mut step = vec![lam * 0.01; vars.len()];
    let min_step = lam * 2e-5;
    let Some((mut best, mut st)) = evaluate(&base, &goals) else {
        let _ = tx.send(Msg::Failed("The starting antenna can't be calculated.".into()));
        wake();
        return;
    };
    let mut evals = 1;
    let _ = tx.send(Msg::Progress(Step { evals, ..st.clone() }));
    wake();
    let mut it = 0;
    while it < goals.max_iter && !stop.load(Ordering::Relaxed) {
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
        let res: Vec<Option<(f64, Step)>> =
            trials.par_iter().map(|x| evaluate(&apply(&base, &vars, x), &goals)).collect();
        let mut improved = false;
        let mut bi = None;
        for (k, r) in res.iter().enumerate() {
            if let Some((c, _)) = r {
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
            if let Some((c2, s2)) = evaluate(&apply(&base, &vars, &x), &goals) {
                evals += 1;
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
        let _ = tx.send(Msg::Progress(Step { iter: it, evals, ..st.clone() }));
        wake();
    }
    let _ = tx.send(Msg::Done { ant: apply(&base, &vars, &d), steps: it, evals, ms: t.elapsed().as_secs_f64() * 1e3 });
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resonate_dipole() {
        let a = Antenna::example(0);
        let (tx, rx) = std::sync::mpsc::channel();
        run(a.clone(), vec![OptVar { wire: 0, kind: VarKind::Length }], Goals::default(), Arc::new(AtomicBool::new(false)), tx, || {});
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
