//! Progress of a long job, shared between the thread doing it and the window
//! showing it: a stage name, steps done out of a total (0 = no natural steps),
//! and a flag asking the job to stop.

use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    stop: AtomicBool,
    born: Instant,
    // name, when it started, and whether the total is only the most it might take
    stage: Mutex<(String, Instant, bool)>,
}

/// What the window shows: the stage, its fraction done (None with no steps), how
/// long the stage has run and roughly how long it has left.
pub struct Snapshot {
    pub stage: String,
    pub done: usize,
    pub total: usize,
    pub fraction: Option<f32>,
    /// seconds since the job started
    pub elapsed: f64,
    pub left: Option<f64>,
    pub at_most: bool,
}

/// The message a job returns when it was asked to stop.
pub const STOPPED: &str = "Stopped.";

impl Default for Progress {
    fn default() -> Self {
        Progress {
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            born: Instant::now(),
            stage: Mutex::new((String::new(), Instant::now(), false)),
        }
    }
}

impl Progress {
    /// Starts a stage of `total` steps (0: no steps, the window shows a spinner).
    pub fn stage(&self, name: &str, total: usize) {
        self.set_stage(name, total, false);
    }

    /// A stage that may well finish before `most` steps (the optimizer).
    pub fn stage_at_most(&self, name: &str, most: usize) {
        self.set_stage(name, most, true);
    }

    fn set_stage(&self, name: &str, total: usize, at_most: bool) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        if let Ok(mut s) = self.stage.lock() {
            *s = (name.to_string(), Instant::now(), at_most);
        }
    }

    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_done(&self, n: usize) {
        self.done.store(n, Ordering::Relaxed);
    }

    pub fn ask_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Err(STOPPED) once a stop has been asked for, for `?` in the job's loops.
    pub fn check(&self) -> Result<(), String> {
        if self.stopping() { Err(STOPPED.into()) } else { Ok(()) }
    }

    pub fn snapshot(&self) -> Snapshot {
        let (stage, t, at_most) = self.stage.lock().map(|s| s.clone()).unwrap_or_else(|_| (String::new(), Instant::now(), false));
        let total = self.total.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed).min(total);
        let in_stage = t.elapsed().as_secs_f64();
        let fraction = (total > 0).then(|| done as f32 / total as f32);
        // a guess from this stage's rate so far, once there is something to go on
        let left = (total > 0 && done > 0 && done < total && in_stage > 1.0).then(|| in_stage * (total - done) as f64 / done as f64);
        Snapshot { stage, done, total, fraction, elapsed: self.born.elapsed().as_secs_f64(), left, at_most }
    }
}

/// The bar (or a spinner when the stage has no steps), the stage, the time so far
/// and left, and a Stop button.
pub fn show(ui: &mut egui::Ui, p: &Progress) {
    let s = p.snapshot();
    // wraps rather than widening a narrow side panel
    ui.horizontal_wrapped(|ui| {
        if p.stopping() {
            ui.spinner();
            ui.label("Stopping after the calculations already under way…");
            return;
        }
        match s.fraction {
            Some(f) => {
                ui.add(egui::ProgressBar::new(f).desired_width(ui.available_width().min(240.0)).text(format!("{} of {}{}", s.done, if s.at_most { "at most " } else { "" }, s.total)));
            }
            None => {
                ui.spinner();
            }
        }
        let mut t = format!("{}: {}", s.stage, duration(s.elapsed));
        if let Some(l) = s.left {
            t += &format!(", {} {} left", if s.at_most { "at most" } else { "about" }, duration(l));
        }
        ui.label(t);
        if ui.button("■ Stop").clicked() {
            p.ask_stop();
        }
    });
}

/// "42 s" or "3 min 05 s".
pub fn duration(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    if s < 60 { format!("{s} s") } else { format!("{} min {:02} s", s / 60, s % 60) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_stops() {
        let p = Progress::default();
        assert!(p.snapshot().fraction.is_none());
        p.stage("Sweeping", 4);
        p.tick();
        p.tick();
        assert_eq!(p.snapshot().fraction, Some(0.5));
        // more ticks than steps don't run past the end
        for _ in 0..5 {
            p.tick();
        }
        assert_eq!(p.snapshot().fraction, Some(1.0));
        assert!(p.check().is_ok());
        p.ask_stop();
        assert_eq!(p.check(), Err(STOPPED.to_string()));
        assert_eq!(duration(185.0), "3 min 05 s");
    }
}
