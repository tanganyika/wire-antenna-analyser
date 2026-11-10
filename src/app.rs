//! The main window: tabs for geometry, 3D view, calculation, patterns, sweep and optimizer.

use crate::maa;
use crate::model::*;
use crate::mom::{self, CalcResult, FullResult, SweepResult};
use crate::optimize::{self, Goals, OptVar, Step, VarKind};
use crate::plots::{self, PolarKind, PolarShow, Series, ViewState};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Geometry,
    View,
    Calculate,
    FarField,
    Pattern3D,
    Sweep,
    Optimizer,
}

impl Tab {
    const ALL: [Tab; 7] = [Tab::Geometry, Tab::View, Tab::Calculate, Tab::FarField, Tab::Pattern3D, Tab::Sweep, Tab::Optimizer];
    fn label(&self) -> &'static str {
        match self {
            Tab::Geometry => "Geometry",
            Tab::View => "View",
            Tab::Calculate => "Calculate",
            Tab::FarField => "Far field plots",
            Tab::Pattern3D => "3D pattern",
            Tab::Sweep => "Frequency sweep",
            Tab::Optimizer => "Optimizer",
        }
    }
    pub fn from_name(s: &str) -> Option<Tab> {
        let s = s.to_ascii_lowercase();
        Tab::ALL.into_iter().find(|t| t.label().to_ascii_lowercase().starts_with(&s) || format!("{:?}", t).to_ascii_lowercase() == s)
    }
}

enum Job {
    Calc(Receiver<Result<FullResult, String>>, Instant),
    Sweep(Receiver<Result<SweepResult, String>>, Instant),
    Opt(Receiver<optimize::Msg>, Instant),
}

pub struct Shot {
    pub path: PathBuf,
    pub requested: bool,
    pub done: Arc<AtomicBool>,
}

pub struct App {
    ant: Antenna,
    path: Option<PathBuf>,
    tab: Tab,
    threads: usize,
    max_threads: usize,
    calc: Option<FullResult>,
    history: Vec<CalcResult>,
    sweep: Option<SweepResult>,
    sweep_f0: f64,
    sweep_f1: f64,
    sweep_steps: usize,
    sweep_gain: bool,
    opt_vars: Vec<OptVar>,
    goals: Goals,
    opt_log: Vec<Step>,
    opt_result: Option<(Antenna, String)>,
    opt_backup: Option<Antenna>,
    opt_stop: Arc<AtomicBool>,
    new_var_wire: usize,
    new_var_kind: VarKind,
    job: Option<Job>,
    status: String,
    error: Option<String>,
    view: ViewState,
    show: PolarShow,
    shot: Option<Shot>,
    frame: u64,
    selected_wire: Option<usize>,
    /// wire open in the wire editor (right-click in the View), and which point stays
    /// put when its length is typed in (0 start, 1 centre, 2 end)
    wire_edit: Option<usize>,
    edit_anchor: u8,
    edit_move_joined: bool,
    az_cursor: Option<f64>,
    el_cursor: Option<f64>,
    view3d_pat: ViewState,
    pat_mesh: bool,
    pat_scale: plots::Scale3D,
    pat_min_db: f64,
    last_dir: Option<PathBuf>,
    ins_all_mm: f64,
    ins_all_kind: usize,
}

/// Small settings file (remembers the last antenna folder between runs).
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("wireant").join("settings.txt"))
}

fn load_last_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings_path()?).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("last_dir="))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn save_last_dir(dir: &std::path::Path) {
    if let Some(p) = settings_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, format!("last_dir={}\n", dir.display()));
    }
}

fn pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new().num_threads(n.max(1)).build().expect("thread pool")
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>, tab: Option<Tab>, shot: Option<PathBuf>) -> App {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let max_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let mut app = App {
            ant: Antenna::example(2),
            path: None,
            tab: tab.unwrap_or(Tab::Geometry),
            threads: max_threads,
            max_threads,
            calc: None,
            history: Vec::new(),
            sweep: None,
            sweep_f0: 0.0,
            sweep_f1: 0.0,
            sweep_steps: 41,
            sweep_gain: true,
            opt_vars: vec![],
            goals: Goals::default(),
            opt_log: vec![],
            opt_result: None,
            opt_backup: None,
            opt_stop: Arc::new(AtomicBool::new(false)),
            new_var_wire: 0,
            new_var_kind: VarKind::Length,
            job: None,
            status: format!("Ready. {} processor threads available.", max_threads),
            error: None,
            view: ViewState::default(),
            show: PolarShow { total: true, hor: false, ver: false },
            shot: shot.map(|p| Shot { path: p, requested: false, done: Arc::new(AtomicBool::new(false)) }),
            frame: 0,
            selected_wire: None,
            wire_edit: None,
            edit_anchor: 0,
            edit_move_joined: true,
            az_cursor: None,
            el_cursor: Some(5.0),
            view3d_pat: ViewState::default(),
            pat_mesh: true,
            pat_scale: plots::Scale3D::Arrl,
            pat_min_db: plots::MIN_DB_DEFAULT,
            last_dir: load_last_dir(),
            ins_all_mm: 0.5,
            ins_all_kind: 0,
        };
        match std::env::var("WIREANT_ENGINE").map(|v| v.to_ascii_lowercase()).as_deref() {
            Ok("nec") if crate::nec::available() => app.ant.engine = Engine::Nec2,
            Ok("wiremom") => app.ant.engine = Engine::Wiremom,
            Ok("mininec") => app.ant.engine = Engine::Mininec,
            _ => {}
        }
        if let Some(p) = open {
            app.open_path(p);
        }
        // test hook: WIREANT_ZOOM_WIRE=<wire number>[,<zoom>] starts the View zoomed onto that wire
        if let Ok(v) = std::env::var("WIREANT_ZOOM_WIRE") {
            let mut it = v.split(',');
            let wi = it.next().and_then(|x| x.trim().parse::<usize>().ok()).unwrap_or(1).max(1) - 1;
            let z = it.next().and_then(|x| x.trim().parse::<f32>().ok()).unwrap_or(10.0);
            if let Some(w) = app.ant.wires.get(wi) {
                let dz = if app.ant.ground.kind != GroundKind::Free { app.ant.ground.add_height } else { 0.0 };
                app.view.focus = Some([(w.p1[0] + w.p2[0]) / 2.0, (w.p1[1] + w.p2[1]) / 2.0, (w.p1[2] + w.p2[2]) / 2.0 + dz]);
                app.view.zoom = z;
            }
        }
        app.reset_sweep_band();
        app.default_opt_vars();
        if app.shot.is_some() {
            // screenshot mode: compute synchronously so the picture shows real results
            let p = pool(app.threads);
            let a = app.ant.clone();
            if let Ok(r) = p.install(|| mom::calculate(&a)) {
                app.history.push(r.result.clone());
                app.status = status_line(&r.result);
                app.calc = Some(r);
            }
            let (f0, f1, n) = (app.sweep_f0, app.sweep_f1, app.sweep_steps);
            app.sweep = p.install(|| mom::sweep(&a, f0, f1, n, true)).ok();
            if tab.is_none() {
                app.tab = Tab::Calculate;
            }
        }
        app
    }

    fn reset_sweep_band(&mut self) {
        let f = self.ant.freq_mhz;
        self.sweep_f0 = (f * 0.97 * 1000.0).round() / 1000.0;
        self.sweep_f1 = (f * 1.03 * 1000.0).round() / 1000.0;
    }

    fn default_opt_vars(&mut self) {
        self.opt_vars.clear();
        for i in 0..self.ant.wires.len().min(3) {
            self.opt_vars.push(OptVar { wire: i, kind: VarKind::Length });
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn set_antenna(&mut self, a: Antenna) {
        let mut engine = self.ant.engine;
        if !a.lines.is_empty() && engine == Engine::Mininec {
            // MININEC can't model transmission lines
            engine = Engine::Wiremom;
        }
        if !a.plates.is_empty() {
            // only wiremom models plates
            engine = Engine::Wiremom;
        }
        self.ant = a;
        self.ant.engine = engine;
        self.calc = None;
        self.sweep = None;
        self.history.clear();
        self.opt_log.clear();
        self.opt_result = None;
        self.reset_sweep_band();
        self.default_opt_vars();
        self.selected_wire = None;
    }

    fn open_path(&mut self, p: PathBuf) {
        match std::fs::read(&p) {
            Ok(bytes) => {
                let text = maa::decode(&bytes);
                let is_nec = p.extension().map(|e| e.eq_ignore_ascii_case("nec")).unwrap_or(false);
                let is_ez = p.extension().map(|e| e.eq_ignore_ascii_case("ez")).unwrap_or(false);
                let parsed = if is_ez {
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    crate::ez::parse(&bytes, &stem).map(|r| {
                        if !r.warnings.is_empty() {
                            self.error = Some(format!("Opened with notes:\n• {}", r.warnings.join("\n• ")));
                        }
                        r.ant
                    })
                } else if is_nec {
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    crate::nec_file::parse(&text, &stem).map(|r| {
                        if !r.warnings.is_empty() {
                            self.error = Some(format!("Opened with notes:\n• {}", r.warnings.join("\n• ")));
                        }
                        r.ant
                    })
                } else {
                    maa::parse(&text)
                };
                match parsed {
                    Ok(a) => {
                        self.set_antenna(a);
                        self.status = format!("Opened {}", p.display());
                        self.remember_dir(&p);
                        self.path = Some(p);
                    }
                    Err(e) => self.error = Some(format!("Couldn't read {}: {}", p.display(), e)),
                }
            }
            Err(e) => self.error = Some(format!("Couldn't open {}: {}", p.display(), e)),
        }
    }

    fn remember_dir(&mut self, file: &std::path::Path) {
        if let Some(dir) = file.parent().filter(|d| d.is_dir()) {
            if self.last_dir.as_deref() != Some(dir) {
                save_last_dir(dir);
            }
            self.last_dir = Some(dir.to_path_buf());
        }
    }

    fn file_dialog(&self) -> rfd::FileDialog {
        let d = rfd::FileDialog::new()
            .add_filter("Antenna files (.maa, .nec, .ez)", &["maa", "MAA", "nec", "NEC", "ez", "EZ"])
            .add_filter("MMANA files (.maa)", &["maa", "MAA"])
            .add_filter("NEC files (.nec)", &["nec", "NEC"])
            .add_filter("EZNEC files (.ez)", &["ez", "EZ"]);
        match &self.last_dir {
            Some(dir) => d.set_directory(dir),
            None => d,
        }
    }

    fn save_to(&mut self, p: PathBuf) {
        let p = if p.extension().is_none() { p.with_extension("maa") } else { p };
        let is_nec = p.extension().map(|e| e.eq_ignore_ascii_case("nec")).unwrap_or(false);
        let is_ez = p.extension().map(|e| e.eq_ignore_ascii_case("ez")).unwrap_or(false);
        let data: Vec<u8> = if is_ez {
            crate::ez::write(&self.ant)
        } else if is_nec {
            crate::nec_file::write(&self.ant).into_bytes()
        } else {
            maa::write(&self.ant).into_bytes()
        };
        match std::fs::write(&p, data) {
            Ok(()) => {
                self.status = format!("Saved {}", p.display());
                self.remember_dir(&p);
                self.path = Some(p);
            }
            Err(e) => self.error = Some(format!("Couldn't save {}: {}", p.display(), e)),
        }
    }

    fn start_calc(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        let a = self.ant.clone();
        let n = self.threads;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = pool(n).install(|| mom::calculate(&a));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.job = Some(Job::Calc(rx, Instant::now()));
        self.status = format!("Calculating on {} threads…", self.threads);
    }

    fn start_sweep(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        let a = self.ant.clone();
        let (f0, f1, st, g, n) = (self.sweep_f0, self.sweep_f1, self.sweep_steps, self.sweep_gain, self.threads);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = pool(n).install(|| mom::sweep(&a, f0, f1, st, g));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.job = Some(Job::Sweep(rx, Instant::now()));
        self.status = format!("Sweeping {} frequencies on {} threads…", self.sweep_steps, self.threads);
    }

    fn start_opt(&mut self, ctx: &egui::Context) {
        if self.busy() || self.opt_vars.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let a = self.ant.clone();
        let vars = self.opt_vars.clone();
        let goals = self.goals.clone();
        let n = self.threads;
        self.opt_stop.store(false, Ordering::Relaxed);
        let stop = self.opt_stop.clone();
        let ctx = ctx.clone();
        self.opt_log.clear();
        self.opt_result = None;
        std::thread::spawn(move || {
            let c2 = ctx.clone();
            pool(n).install(|| optimize::run(a, vars, goals, stop, tx, move || c2.request_repaint()));
            ctx.request_repaint();
        });
        self.job = Some(Job::Opt(rx, Instant::now()));
        self.status = format!("Optimizing on {} threads…", self.threads);
    }

    fn poll(&mut self) {
        let Some(job) = self.job.take() else { return };
        match job {
            Job::Calc(rx, t) => match rx.try_recv() {
                Ok(Ok(r)) => {
                    self.status = status_line(&r.result);
                    if !r.warnings.is_empty() {
                        self.status += &format!("  ⚠ {}", r.warnings.join(" "));
                    }
                    self.history.push(r.result.clone());
                    self.calc = Some(r);
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.status = "Calculation failed.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.job = Some(Job::Calc(rx, t)),
                Err(_) => self.status = "Calculation stopped.".into(),
            },
            Job::Sweep(rx, t) => match rx.try_recv() {
                Ok(Ok(r)) => {
                    self.status = format!("Sweep of {} frequencies done in {:.0} ms on {} threads.", r.points.len(), r.t_ms, r.threads);
                    self.sweep = Some(r);
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.status = "Sweep failed.".into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.job = Some(Job::Sweep(rx, t)),
                Err(_) => self.status = "Sweep stopped.".into(),
            },
            Job::Opt(rx, t) => {
                let mut finished = false;
                loop {
                    match rx.try_recv() {
                        Ok(optimize::Msg::Progress(s)) => self.opt_log.push(s),
                        Ok(optimize::Msg::Done { ant, steps, evals, ms }) => {
                            let txt = format!("{} steps, {} antenna calculations in {:.1} s on {} threads", steps, evals, ms / 1000.0, self.threads);
                            self.status = format!("Optimizer finished: {}.", txt);
                            self.opt_result = Some((ant, txt));
                            finished = true;
                            break;
                        }
                        Ok(optimize::Msg::Failed(e)) => {
                            self.error = Some(e);
                            finished = true;
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(_) => {
                            finished = true;
                            break;
                        }
                    }
                }
                if !finished {
                    self.job = Some(Job::Opt(rx, t));
                }
            }
        }
    }

    // ------------------------------------------------------------------ menus

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    let mut a = Antenna::example(0);
                    a.name = "New antenna".into();
                    self.set_antenna(a);
                    self.path = None;
                }
                if ui.button("Open…").clicked() {
                    if let Some(p) = self.file_dialog().pick_file() {
                        self.open_path(p);
                    }
                }
                if ui.button("Save").clicked() {
                    match self.path.clone() {
                        Some(p) => self.save_to(p),
                        None => self.save_as(),
                    }
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                }
                if ui.button("Save as EZNEC (.ez)…").clicked() {
                    let name = format!("{}.ez", self.ant.name.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect::<String>().trim());
                    if let Some(p) = self.file_dialog().set_file_name(name).save_file() {
                        let p = if p.extension().is_none() { p.with_extension("ez") } else { p };
                        self.save_to(p);
                    }
                }
                if ui.button("Save as NEC (.nec)…").clicked() {
                    let name = format!("{}.nec", self.ant.name.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect::<String>().trim());
                    if let Some(p) = self.file_dialog().set_file_name(name).save_file() {
                        let p = if p.extension().is_none() { p.with_extension("nec") } else { p };
                        self.save_to(p);
                    }
                }
                ui.separator();
                ui.menu_button("Examples", |ui| {
                    for (i, n) in Antenna::example_names().iter().enumerate() {
                        if ui.button(*n).clicked() {
                            self.set_antenna(Antenna::example(i));
                            self.path = None;
                        }
                    }
                });
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("Wire antenna analyser for Linux.");
                ui.label("Method-of-moments solver running on all processor cores.");
                ui.label(format!("{} threads available.", self.max_threads));
            });
            ui.separator();
            for t in Tab::ALL {
                ui.selectable_value(&mut self.tab, t, t.label());
            }
        });
    }

    fn save_as(&mut self) {
        let name = format!("{}.maa", self.ant.name.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect::<String>().trim());
        if let Some(p) = self.file_dialog().set_file_name(name).save_file() {
            self.save_to(p);
        }
    }

    // ------------------------------------------------------------------ geometry

    fn geometry(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.ant.name).desired_width(320.0));
                ui.separator();
                ui.label("Frequency (MHz)");
                if ui.add(egui::DragValue::new(&mut self.ant.freq_mhz).speed(0.01).range(0.01..=30000.0).max_decimals(4)).changed() {
                    self.reset_sweep_band();
                }
                ui.label(format!("λ = {:.3} m", self.ant.lambda()));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Wires");
                ui.label(format!("({})", self.ant.wires.len()));
                if ui.button("➕ Add wire").clicked() {
                    let w = self.ant.wires.last().cloned().unwrap_or(Wire::new([0.0, -1.0, 0.0], [0.0, 1.0, 0.0], 1e-3));
                    let mut w2 = w.clone();
                    w2.p1[0] += 1.0;
                    w2.p2[0] += 1.0;
                    self.ant.wires.push(w2);
                }
                if let Some(s) = self.selected_wire {
                    if ui.button("Copy selected").clicked() && s < self.ant.wires.len() {
                        let w = self.ant.wires[s].clone();
                        self.ant.wires.push(w);
                    }
                }
            });
            let mut delete = None;
            egui::Grid::new("wires").striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
                for h in ["No.", "X1 (m)", "Y1 (m)", "Z1 (m)", "X2 (m)", "Y2 (m)", "Z2 (m)", "R (mm)", "Seg", "Length (m)", "Insulation (mm)", "εr", "VF", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, w) in self.ant.wires.iter_mut().enumerate() {
                    let sel = self.selected_wire == Some(i);
                    if ui.selectable_label(sel, format!("{}", i + 1)).clicked() {
                        self.selected_wire = if sel { None } else { Some(i) };
                    }
                    for v in w.p1.iter_mut().chain(w.p2.iter_mut()) {
                        ui.add(egui::DragValue::new(v).speed(0.01).max_decimals(4).min_decimals(3));
                    }
                    let mut rmm = w.r * 1000.0;
                    if ui.add(egui::DragValue::new(&mut rmm).speed(0.01).range(0.001..=1000.0).max_decimals(3)).changed() {
                        w.r = rmm / 1000.0;
                    }
                    let mut auto = w.seg <= 0;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut auto, "auto").changed() {
                            w.seg = if auto { -1 } else { 10 };
                        }
                        if !auto {
                            ui.add(egui::DragValue::new(&mut w.seg).range(1..=2000));
                        }
                    });
                    ui.label(format!("{:.4}", w.length()));
                    let mut tmm = w.ins_t * 1000.0;
                    if ui
                        .add(egui::DragValue::new(&mut tmm).speed(0.01).range(0.0..=100.0).max_decimals(3))
                        .on_hover_text("Insulation thickness (0 = bare wire)")
                        .changed()
                    {
                        w.ins_t = tmm / 1000.0;
                        if w.ins_t > 0.0 && w.ins_eps <= 1.0 {
                            w.ins_eps = 3.5;
                        }
                    }
                    ui.add_enabled(w.ins_t > 0.0, egui::DragValue::new(&mut w.ins_eps).speed(0.05).range(1.0..=20.0).max_decimals(2))
                        .on_hover_text("Relative permittivity: PVC 3.5, polyethylene 2.3, PTFE 2.1");
                    ui.label(if w.insulated() { format!("{:.3}", w.velocity_factor()) } else { "—".into() })
                        .on_hover_text("Estimated velocity factor of this wire from its insulation");
                    if ui.small_button("🗑").on_hover_text("Delete wire").clicked() {
                        delete = Some(i);
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Insulation for all wires:");
                ui.add(egui::DragValue::new(&mut self.ins_all_mm).speed(0.01).range(0.0..=100.0).suffix(" mm"));
                egui::ComboBox::from_id_salt("ins_mat")
                    .selected_text(format!("{} (εr {})", INSULATIONS[self.ins_all_kind].0, INSULATIONS[self.ins_all_kind].1))
                    .show_ui(ui, |ui| {
                        for (k, (name, e)) in INSULATIONS.iter().enumerate() {
                            ui.selectable_value(&mut self.ins_all_kind, k, format!("{name} (εr {e})"));
                        }
                    });
                if ui.button("Apply to all").clicked() {
                    for w in self.ant.wires.iter_mut() {
                        w.ins_t = self.ins_all_mm / 1000.0;
                        w.ins_eps = INSULATIONS[self.ins_all_kind].1;
                    }
                }
                if ui.button("Remove all").clicked() {
                    for w in self.ant.wires.iter_mut() {
                        w.ins_t = 0.0;
                    }
                }
            });
            if let Some(i) = delete {
                self.delete_wire(i);
            }
            let nw = self.ant.wires.len().max(1);
            ui.add_space(8.0);
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.horizontal(|ui| {
                    ui.heading("Sources");
                    if ui.button("➕ Add").clicked() {
                        self.ant.sources.push(Source { wire: 0, pos: 0.5, phase_deg: 0.0, volts: 1.0 });
                    }
                });
                let mut del = None;
                egui::Grid::new("sources").striped(true).show(ui, |ui| {
                    for h in ["Wire", "Position", "Phase (°)", "Volts", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, s) in self.ant.sources.iter_mut().enumerate() {
                        wire_pick(ui, ("sw", i), &mut s.wire, nw);
                        pos_pick(ui, ("sp", i), &mut s.pos);
                        ui.add(egui::DragValue::new(&mut s.phase_deg).speed(1.0).range(-360.0..=360.0));
                        ui.add(egui::DragValue::new(&mut s.volts).speed(0.01).range(0.0..=1e6));
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.ant.sources.remove(i);
                }
                let ui = &mut cols[1];
                ui.horizontal(|ui| {
                    ui.heading("Loads");
                    if ui.button("➕ R+jX").clicked() {
                        self.ant.loads.push(Load { wire: 0, pos: 0.5, kind: LoadKind::Rx { r: 0.0, x: 0.0 } });
                    }
                    if ui.button("➕ L/C").clicked() {
                        self.ant.loads.push(Load { wire: 0, pos: 0.5, kind: LoadKind::Lc { l_uh: 1.0, c_pf: 0.0, q: 0.0 } });
                    }
                });
                let mut del = None;
                egui::Grid::new("loads").striped(true).show(ui, |ui| {
                    for h in ["Wire", "Position", "Value", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, l) in self.ant.loads.iter_mut().enumerate() {
                        wire_pick(ui, ("lw", i), &mut l.wire, nw);
                        pos_pick(ui, ("lp", i), &mut l.pos);
                        ui.horizontal(|ui| match &mut l.kind {
                            LoadKind::Rx { r, x } => {
                                ui.label("R");
                                ui.add(egui::DragValue::new(r).speed(0.1));
                                ui.label("jX");
                                ui.add(egui::DragValue::new(x).speed(0.1));
                            }
                            LoadKind::Rlc { r, l_uh, c_pf, parallel } => {
                                ui.label("R");
                                ui.add(egui::DragValue::new(r).speed(0.1).range(0.0..=1e9));
                                ui.label("L µH");
                                ui.add(egui::DragValue::new(l_uh).speed(0.01).range(0.0..=1e6));
                                ui.label("C pF");
                                ui.add(egui::DragValue::new(c_pf).speed(0.5).range(0.0..=1e9));
                                ui.checkbox(parallel, "parallel");
                            }
                            LoadKind::Lc { l_uh, c_pf, q } => {
                                ui.label("L µH");
                                ui.add(egui::DragValue::new(l_uh).speed(0.01).range(0.0..=1e6));
                                ui.label("C pF");
                                ui.add(egui::DragValue::new(c_pf).speed(0.5).range(0.0..=1e9));
                                ui.label("Q");
                                ui.add(egui::DragValue::new(q).speed(1.0).range(0.0..=1e6))
                                    .on_hover_text("0 = lossless. With both L and C set, they form a parallel trap.");
                            }
                        });
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.ant.loads.remove(i);
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("Transmission lines");
                if ui.button("➕ Add").clicked() {
                    let b = if nw > 1 { 1 } else { 0 };
                    self.ant.lines.push(TLine::new(0, 0.5, b, 0.5));
                }
                ui.label(RichText::new("feed lines, phasing lines and stubs — wiremom and NEC-2 engines").weak());
            });
            if !self.ant.lines.is_empty() {
                let mut del = None;
                egui::Grid::new("lines").striped(true).show(ui, |ui| {
                    for h in ["From wire", "Position", "To wire", "Position", "Z0 (Ω)", "Length (m)", "Vel. factor", "Loss (dB/100 m)", "Reversed", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, t) in self.ant.lines.iter_mut().enumerate() {
                        wire_pick(ui, ("taw", i), &mut t.a_wire, nw);
                        pos_pick(ui, ("tap", i), &mut t.a_pos);
                        wire_pick(ui, ("tbw", i), &mut t.b_wire, nw);
                        pos_pick(ui, ("tbp", i), &mut t.b_pos);
                        ui.add(egui::DragValue::new(&mut t.z0).speed(0.5).range(1.0..=10000.0));
                        ui.add(egui::DragValue::new(&mut t.length).speed(0.01).range(0.0..=1e6).max_decimals(4))
                            .on_hover_text("0 = the straight distance between the two ends");
                        ui.add(egui::DragValue::new(&mut t.vf).speed(0.005).range(0.1..=1.0).max_decimals(3))
                            .on_hover_text("Velocity factor: 1 for an air-spaced line, about 0.66 for solid-PE coax, 0.8 for foam");
                        ui.add(egui::DragValue::new(&mut t.loss_db).speed(0.05).range(0.0..=100.0).max_decimals(2))
                            .on_hover_text("Matched-line loss at the calculation frequency (NEC-2 ignores it)");
                        ui.checkbox(&mut t.reversed, "").on_hover_text("Conductors swapped at the far end (a half twist)");
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.ant.lines.remove(i);
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("Plates");
                if ui.button("➕ Add").clicked() {
                    let l = self.ant.lambda() / 2.0;
                    self.ant.plates.push(Plate::new([-l / 2.0, -l / 2.0, 0.0], [l, 0.0, 0.0], [0.0, l, 0.0]));
                }
                ui.label(RichText::new("flat conducting plates (reflectors, ground screens); a wire ending on a plate connects to it — wiremom engine").weak());
            });
            if !self.ant.plates.is_empty() {
                let mut del = None;
                egui::Grid::new("plates").striped(true).show(ui, |ui| {
                    for h in ["Corner X, Y, Z (m)", "Edge 1 dX, dY, dZ (m)", "Edge 2 dX, dY, dZ (m)", "Divisions", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, p) in self.ant.plates.iter_mut().enumerate() {
                        for v in [&mut p.corner, &mut p.edge1, &mut p.edge2] {
                            ui.horizontal(|ui| {
                                for c in v.iter_mut() {
                                    ui.add(egui::DragValue::new(c).speed(0.01).max_decimals(4));
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut p.n1).speed(0.2).range(0..=200));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut p.n2).speed(0.2).range(0..=200));
                        })
                        .response
                        .on_hover_text("Mesh cells along each edge; 0 = automatic (about λ/10)");
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.ant.plates.remove(i);
                }
            }
            ui.add_space(8.0);
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.heading("Ground and wire");
                ground_controls(ui, &mut self.ant);
                let ui = &mut cols[1];
                ui.heading("Automatic segmentation");
                egui::Grid::new("segm").show(ui, |ui| {
                    ui.label("Shortest segment  λ /");
                    ui.add(egui::DragValue::new(&mut self.ant.seg.dm1).speed(1.0).range(10.0..=5000.0));
                    ui.end_row();
                    ui.label("Longest segment  λ /");
                    ui.add(egui::DragValue::new(&mut self.ant.seg.dm2).speed(1.0).range(4.0..=1000.0));
                    ui.end_row();
                    ui.label("Growth factor");
                    ui.add(egui::DragValue::new(&mut self.ant.seg.sc).speed(0.05).range(1.01..=3.0))
                        .on_hover_text("SC: the easing parameter (multiplier), 1 < SC <= 3");
                    ui.end_row();
                    ui.label("Shortest segments at each end");
                    ui.add(egui::DragValue::new(&mut self.ant.seg.ec).range(1..=64));
                    ui.end_row();
                });
            });
            ui.add_space(8.0);
            ui.heading("Comment");
            ui.add(egui::TextEdit::multiline(&mut self.ant.comment).desired_rows(3).desired_width(f32::INFINITY));
        });
    }

    // ------------------------------------------------------------------ calculate

    fn calc_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Calculate");
        egui::Grid::new("calcctl").spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Frequency (MHz)");
            ui.add(egui::DragValue::new(&mut self.ant.freq_mhz).speed(0.01).range(0.01..=30000.0).max_decimals(4));
            ui.end_row();
            ui.label("Engine");
            engine_pick(ui, "eng_calc", &mut self.ant.engine);
            ui.end_row();
        });
        ground_controls(ui, &mut self.ant);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Threads");
            ui.add(egui::Slider::new(&mut self.threads, 1..=self.max_threads));
        });
        ui.add_space(6.0);
        let busy = self.busy();
        let b = egui::Button::new(RichText::new(if busy { "Working…" } else { "▶  Calculate" }).size(17.0)).min_size(Vec2::new(200.0, 34.0));
        if ui.add_enabled(!busy, b).clicked() {
            self.start_calc(ui.ctx());
        }
        if busy {
            ui.spinner();
        }
        if let Some(c) = &self.calc {
            let r = &c.result;
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("Last result").strong());
            egui::Grid::new("lastres").show(ui, |ui| {
                let row = |ui: &mut egui::Ui, a: &str, b: String| {
                    ui.label(a);
                    ui.label(RichText::new(b).monospace());
                    ui.end_row();
                };
                row(ui, "Impedance", format!("{:.2} {} j{:.2} Ω", r.z.re, if r.z.im < 0.0 { "−" } else { "+" }, r.z.im.abs()));
                row(ui, &format!("SWR ({:.0} Ω)", self.ant.z0), format!("{:.2}", r.swr));
                row(ui, "Gain", format!("{:.2} dBi  ({:.2} dBd)", r.ga_dbi, r.ga_dbi - 2.15));
                row(ui, "F/B", format!("{:.2} dB", r.fb_db));
                row(ui, "Max at", format!("az {:.1}°  el {:.1}°", r.az_max, r.el_max));
                row(ui, "Polarization", (if r.horizontal_pol { "Horizontal" } else { "Vertical" }).to_string());
                row(ui, "Segments", format!("{} ({} unknowns)", r.n_segs, r.n_unknowns));
                row(ui, "Time", format!("{:.1} ms on {} threads", r.t_total_ms, r.threads));
                row(ui, "  matrix fill", format!("{:.1} ms", r.t_fill_ms));
                row(ui, "  solve", format!("{:.1} ms", r.t_solve_ms));
                row(ui, "  patterns", format!("{:.1} ms", r.t_far_ms));
            });
        }
    }

    fn results_table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Results");
            if ui.button("Clear").clicked() {
                self.history.clear();
            }
        });
        egui::ScrollArea::both().id_salt("hist").max_height(170.0).auto_shrink([false, true]).stick_to_bottom(true).show(ui, |ui| {
            egui::Grid::new("history").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                // column order and names as in the original's results grid (english.mmn Grid1)
                for h in ["No.", "F (MHz)", "R (Ohm)", "jX (Ohm)", "SWR", "Gh dBd", "Ga dBi", "F/B dB", "Elev.", "Ground", "Add H.", "Polar.", "ms", "Threads"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, r) in self.history.iter().enumerate() {
                    ui.label(format!("{}", i + 1));
                    ui.label(format!("{:.3}", r.freq_mhz));
                    ui.label(format!("{:.2}", r.z.re));
                    ui.label(format!("{:.2}", r.z.im));
                    let swr_col = if r.swr < 1.5 { Color32::from_rgb(0, 130, 40) } else if r.swr < 3.0 { Color32::from_rgb(170, 110, 0) } else { Color32::from_rgb(190, 30, 30) };
                    ui.label(RichText::new(format!("{:.2}", r.swr)).color(swr_col));
                    ui.label(format!("{:.2}", r.ga_dbi - 2.15));
                    ui.label(format!("{:.2}", r.ga_dbi));
                    ui.label(format!("{:.2}", r.fb_db));
                    ui.label(format!("{:.1}", r.el_max));
                    ui.label(match r.ground {
                        GroundKind::Free => "Free",
                        GroundKind::Perfect => "Perfect",
                        GroundKind::Real => "Real",
                    });
                    ui.label(if r.ground == GroundKind::Free { "—".into() } else { format!("{:.2}", r.add_height) });
                    ui.label(if r.horizontal_pol { "hori." } else { "vert." });
                    ui.label(format!("{:.0}", r.t_total_ms));
                    ui.label(format!("{}", r.threads));
                    ui.end_row();
                }
            });
            if self.history.is_empty() {
                ui.label(RichText::new("Press Calculate to analyse the antenna.").italics().color(Color32::GRAY));
            }
        });
    }

    fn patterns(&mut self, ui: &mut egui::Ui, size: f32) {
        let Some(c) = &self.calc else {
            ui.label(RichText::new("No patterns yet — press Calculate.").italics().color(Color32::GRAY));
            return;
        };
        let r = &c.result;
        let p = &c.patterns;
        let ground = !p.free_space;
        let info_h = vec![
            format!("Elevation {:.1}°   Ga {:.2} dBi   F/B {:.2} dB", p.el_cut_deg, r.ga_dbi, r.fb_db),
            format!("{:.3} MHz   Z = {:.1} {} j{:.1} Ω   SWR {:.2}", r.freq_mhz, r.z.re, if r.z.im < 0.0 { "−" } else { "+" }, r.z.im.abs(), r.swr),
        ];
        let info_v = vec![
            format!("Azimuth {:.1}°   max at {:.1}° elevation", p.az_cut_deg, r.el_max),
            format!("{}   {}", r.ground.label(), if r.horizontal_pol { "horizontal pol." } else { "vertical pol." }),
        ];
        let az = p.az_cut.clone();
        let el: Vec<_> = if ground { p.el_cut.iter().filter(|(t, _)| *t <= 180.0).cloned().collect() } else { p.el_cut.clone() };
        ui.horizontal(|ui| {
            plots::polar(ui, size, "Horizontal pattern (top view)", &az, PolarKind::Azimuth, &self.show, &info_h, &mut self.az_cursor);
            plots::polar(ui, size, "Vertical pattern (side view)", &el, PolarKind::Elevation { ground }, &self.show, &info_v, &mut self.el_cursor);
        });
    }

    fn calculate_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("calcleft").resizable(false).default_size(300.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.calc_controls(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.results_table(ui);
            ui.separator();
            let avail = ui.available_size();
            let size = ((avail.x - 20.0) / 2.0).min(avail.y - 50.0).clamp(200.0, 520.0);
            self.patterns(ui, size);
        });
    }

    fn farfield_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Show:");
            ui.checkbox(&mut self.show.total, RichText::new("Total").color(plots::TOTAL));
            ui.checkbox(&mut self.show.hor, RichText::new("Horizontal").color(plots::HORC));
            ui.checkbox(&mut self.show.ver, RichText::new("Vertical").color(plots::VERC));
            ui.separator();
            if ui.add_enabled(!self.busy(), egui::Button::new("▶ Calculate")).clicked() {
                self.start_calc(ui.ctx());
            }
            ui.label("Rings: 0, −3, −6, −10, −20, −30 dB below the maximum (logarithmic scale).");
        });
        ui.separator();
        let avail = ui.available_size();
        let size = ((avail.x - 20.0) / 2.0).min(avail.y - 50.0).clamp(220.0, 900.0);
        self.patterns(ui, size);
    }

    fn pattern3d_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy(), egui::Button::new("▶ Calculate")).clicked() {
                self.start_calc(ui.ctx());
            }
            ui.checkbox(&mut self.pat_mesh, "Grid lines");
            ui.label("Scale");
            for sc in [plots::Scale3D::Arrl, plots::Scale3D::Db, plots::Scale3D::Linear] {
                ui.selectable_value(&mut self.pat_scale, sc, sc.label());
            }
            if self.pat_scale == plots::Scale3D::Db {
                ui.label("Min of dB scale");
                if ui.small_button("−").clicked() {
                    self.pat_min_db = plots::step_min_db(self.pat_min_db, false);
                }
                ui.label(format!("{:.0} dB", self.pat_min_db));
                if ui.small_button("+").clicked() {
                    self.pat_min_db = plots::step_min_db(self.pat_min_db, true);
                }
            }
            if ui.button("Top").clicked() {
                self.view3d_pat.yaw = 0.0;
                self.view3d_pat.pitch = 1.5;
            }
            if ui.button("Front").clicked() {
                self.view3d_pat.yaw = 0.0;
                self.view3d_pat.pitch = 0.0;
            }
            if ui.button("Side").clicked() {
                self.view3d_pat.yaw = -std::f32::consts::FRAC_PI_2;
                self.view3d_pat.pitch = 0.0;
            }
            if ui.button("Reset").clicked() {
                self.view3d_pat = ViewState::default();
            }
            if let Some(c) = &self.calc {
                let r = &c.result;
                ui.separator();
                ui.label(format!(
                    "{:.3} MHz   Ga {:.2} dBi   F/B {:.2} dB   max at az {:.1}° el {:.1}°   ({})",
                    r.freq_mhz, r.ga_dbi, r.fb_db, r.az_max, r.el_max, r.ground.label()
                ));
            }
        });
        let avail = ui.available_size();
        match &self.calc {
            Some(c) => plots::pattern3d(ui, avail, &c.patterns, &mut self.view3d_pat, self.pat_mesh, self.pat_scale, self.pat_min_db),
            None => {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("No pattern yet — press Calculate.").italics().color(Color32::GRAY)));
            }
        }
    }

    fn sweep_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("From (MHz)");
            ui.add(egui::DragValue::new(&mut self.sweep_f0).speed(0.01).max_decimals(4));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut self.sweep_f1).speed(0.01).max_decimals(4));
            ui.label("points");
            ui.add(egui::DragValue::new(&mut self.sweep_steps).range(2..=2001));
            ui.checkbox(&mut self.sweep_gain, "Gain and F/B (slower)");
            ui.label("Engine");
            engine_pick(ui, "eng_sweep", &mut self.ant.engine);
            ui.label("Threads");
            ui.add(egui::Slider::new(&mut self.threads, 1..=self.max_threads));
            if ui.add_enabled(!self.busy(), egui::Button::new(RichText::new("▶ Run sweep").strong())).clicked() {
                self.start_sweep(ui.ctx());
            }
            if matches!(self.job, Some(Job::Sweep(..))) {
                ui.spinner();
            }
        });
        if let Some(s) = &self.sweep {
            ui.label(format!("{} frequencies in {:.0} ms using {} threads.", s.points.len(), s.t_ms, s.threads));
        }
        ui.separator();
        let avail = ui.available_size();
        let n = if self.sweep.as_ref().map(|s| s.points.iter().any(|p| p.ga_dbi.is_finite())).unwrap_or(true) { 3.0 } else { 2.0 };
        let h = ((avail.y - 10.0 * n) / n).max(140.0);
        let sz = Vec2::new(avail.x - 4.0, h);
        let pts = self.sweep.as_ref().map(|s| s.points.clone()).unwrap_or_default();
        let fm = Some(self.ant.freq_mhz);
        let swr: Vec<(f64, f64)> = pts.iter().map(|p| (p.freq_mhz, p.swr.min(10.0))).collect();
        let max_swr = swr.iter().map(|p| p.1).fold(1.0, f64::max).clamp(2.0, 10.0);
        plots::xy(ui, sz, &format!("SWR ({:.0} Ω)", self.ant.z0), "MHz", &[Series { name: "SWR", color: plots::TOTAL, pts: swr, right: false }], Some((1.0, max_swr.ceil())), fm);
        plots::xy(ui, sz, "Impedance", "MHz", &[
            Series { name: "R", color: plots::HORC, pts: pts.iter().map(|p| (p.freq_mhz, p.z.re)).collect(), right: false },
            Series { name: "jX", color: plots::VERC, pts: pts.iter().map(|p| (p.freq_mhz, p.z.im)).collect(), right: false },
        ], None, fm);
        if n > 2.0 {
            plots::xy(ui, sz, "Gain and front/back", "MHz", &[
                Series { name: "Ga dBi", color: plots::TOTAL, pts: pts.iter().map(|p| (p.freq_mhz, p.ga_dbi)).collect(), right: false },
                Series { name: "F/B dB", color: plots::HORC, pts: pts.iter().map(|p| (p.freq_mhz, p.fb_db)).collect(), right: true },
            ], None, fm);
        }
    }

    fn view_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.view.show_current, "Colour by current");
            ui.checkbox(&mut self.view.show_segments, "Segment marks");
            ui.checkbox(&mut self.view.show_labels, "Wire numbers");
            if ui.button("Top").clicked() {
                self.view.yaw = 0.0;
                self.view.pitch = 1.5;
            }
            if ui.button("Front").clicked() {
                self.view.yaw = 0.0;
                self.view.pitch = 0.0;
            }
            if ui.button("Side").clicked() {
                self.view.yaw = -std::f32::consts::FRAC_PI_2;
                self.view.pitch = 0.0;
            }
            if ui.button("Zoom out").clicked() {
                self.view.zoom = (self.view.zoom / 3.0).max(1.0);
                if self.view.zoom <= 1.0 {
                    self.view.focus = None;
                }
                self.view.pan = Vec2::ZERO;
            }
            if ui.button("Whole antenna").clicked() {
                let keep = (self.view.show_segments, self.view.show_current, self.view.show_labels);
                self.view = ViewState { show_segments: keep.0, show_current: keep.1, show_labels: keep.2, ..ViewState::default() };
            }
            if self.view.focus.is_some() {
                ui.label(RichText::new(format!("zoom ×{:.0}", self.view.zoom)).color(Color32::from_rgb(235, 140, 0)));
            }
            if ui.add_enabled(!self.busy(), egui::Button::new("▶ Calculate")).clicked() {
                self.start_calc(ui.ctx());
            }
            ui.label(RichText::new("● source").color(Color32::from_rgb(220, 30, 30)));
            ui.label(RichText::new("■ load").color(Color32::from_rgb(20, 160, 60)));
        });
        // the view with scroll bars to move it left/right and up/down
        let avail = ui.available_size();
        let bar = 16.0;
        let view_size = Vec2::new((avail.x - bar - 6.0).max(100.0), (avail.y - bar - 6.0).max(100.0));
        let mut edit = None;
        ui.horizontal(|ui| {
            edit = plots::view3d(ui, view_size, &self.ant, self.calc.as_ref(), &mut self.view);
            let r = scroll_range(view_size.y, self.view.zoom, self.view.pan.y);
            ui.spacing_mut().slider_width = view_size.y;
            ui.add(
                egui::Slider::new(&mut self.view.pan.y, -r..=r)
                    .vertical()
                    .show_value(false)
                    .clamping(egui::SliderClamping::Never)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 2.5 }),
            )
            .on_hover_text("Move the view up or down");
        });
        let r = scroll_range(view_size.x, self.view.zoom, self.view.pan.x);
        // scrolling right shows what is to the right: the scene moves left
        let mut x = -self.view.pan.x;
        ui.spacing_mut().slider_width = view_size.x;
        if ui
            .add(
                egui::Slider::new(&mut x, -r..=r)
                    .show_value(false)
                    .clamping(egui::SliderClamping::Never)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 2.5 }),
            )
            .on_hover_text("Move the view left or right")
            .changed()
        {
            self.view.pan.x = -x;
        }
        if let Some(w) = edit {
            self.wire_edit = Some(w);
        }
        self.wire_editor(ui.ctx());
    }

    /// Remove wire `i` with its sources, loads and transmission lines, renumbering the rest.
    fn delete_wire(&mut self, i: usize) {
        self.ant.wires.remove(i);
        self.ant.sources.retain(|s| s.wire != i);
        self.ant.loads.retain(|l| l.wire != i);
        self.ant.lines.retain(|t| t.a_wire != i && t.b_wire != i);
        let shift = |w: &mut usize| {
            if *w > i {
                *w -= 1;
            }
        };
        for s in self.ant.sources.iter_mut() {
            shift(&mut s.wire);
        }
        for l in self.ant.loads.iter_mut() {
            shift(&mut l.wire);
        }
        for t in self.ant.lines.iter_mut() {
            shift(&mut t.a_wire);
            shift(&mut t.b_wire);
        }
        self.selected_wire = None;
    }

    /// The wire editor window (opened by right-clicking a wire in the View).
    fn wire_editor(&mut self, ctx: &egui::Context) {
        let Some(i) = self.wire_edit else { return };
        if i >= self.ant.wires.len() {
            self.wire_edit = None;
            return;
        }
        let mut open = true;
        let mut delete = false;
        let old = (self.ant.wires[i].p1, self.ant.wires[i].p2);
        egui::Window::new(format!("Wire {}", i + 1)).id(egui::Id::new("wire_editor")).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            let w = &mut self.ant.wires[i];
            egui::Grid::new("we_grid").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("");
                for h in ["X (m)", "Y (m)", "Z (m)"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (name, p) in [("Start", &mut w.p1), ("End", &mut w.p2)] {
                    ui.label(name);
                    for c in p.iter_mut() {
                        ui.add(egui::DragValue::new(c).speed(0.01).max_decimals(4));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            let changed_len = ui
                .horizontal(|ui| {
                    ui.label("Length (m)");
                    let mut len = w.length();
                    let changed = ui.add(egui::DragValue::new(&mut len).speed(0.01).range(1e-4..=1e6).max_decimals(4)).changed();
                    ui.label("  keep");
                    ui.selectable_value(&mut self.edit_anchor, 0, "start");
                    ui.selectable_value(&mut self.edit_anchor, 1, "centre");
                    ui.selectable_value(&mut self.edit_anchor, 2, "end");
                    changed.then_some(len)
                })
                .inner;
            ui.add_space(4.0);
            egui::Grid::new("we_grid2").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                if let Some(len) = changed_len.filter(|_| w.length() > 0.0) {
                    let d = [0, 1, 2].map(|k| (w.p2[k] - w.p1[k]) / w.length());
                    let (a, b) = match self.edit_anchor {
                        0 => (w.p1, [0, 1, 2].map(|k| w.p1[k] + d[k] * len)),
                        2 => ([0, 1, 2].map(|k| w.p2[k] - d[k] * len), w.p2),
                        _ => {
                            let c = [0, 1, 2].map(|k| (w.p1[k] + w.p2[k]) / 2.0);
                            ([0, 1, 2].map(|k| c[k] - d[k] * len / 2.0), [0, 1, 2].map(|k| c[k] + d[k] * len / 2.0))
                        }
                    };
                    w.p1 = a;
                    w.p2 = b;
                }
                ui.label("Radius (mm)");
                let mut rmm = w.r * 1000.0;
                if ui.add(egui::DragValue::new(&mut rmm).speed(0.01).range(0.001..=1000.0).max_decimals(3)).changed() {
                    w.r = rmm / 1000.0;
                }
                ui.end_row();
                ui.label("Segments");
                ui.horizontal(|ui| {
                    let mut auto = w.seg <= 0;
                    if ui.checkbox(&mut auto, "auto").changed() {
                        w.seg = if auto { -1 } else { 21 };
                    }
                    if !auto {
                        ui.add(egui::DragValue::new(&mut w.seg).speed(0.2).range(1..=2000));
                    }
                });
                ui.end_row();
                ui.label("Insulation (mm)");
                let mut tmm = w.ins_t * 1000.0;
                if ui.add(egui::DragValue::new(&mut tmm).speed(0.01).range(0.0..=100.0).max_decimals(3)).changed() {
                    w.ins_t = tmm / 1000.0;
                }
                ui.horizontal(|ui| {
                    ui.label("εr");
                    ui.add(egui::DragValue::new(&mut w.ins_eps).speed(0.01).range(1.0..=20.0).max_decimals(2));
                });
                ui.end_row();
            });
            ui.checkbox(&mut self.edit_move_joined, "Move the ends of wires joined to this one")
                .on_hover_text("When an end moves, the wires connected there follow it, so junctions stay connected");
            ui.label(RichText::new(format!("{} — changes apply immediately; Calculate again for new results.", pos_label(i, 0.5))).weak());
            ui.horizontal(|ui| {
                if ui.button("🗑 Delete wire").clicked() {
                    delete = true;
                }
                if ui.button("Close").clicked() {
                    self.wire_edit = None;
                }
            });
        });
        let new = (self.ant.wires[i].p1, self.ant.wires[i].p2);
        if self.edit_move_joined && new != old {
            let same = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6);
            for (j, w) in self.ant.wires.iter_mut().enumerate() {
                if j == i {
                    continue;
                }
                for (o, n) in [(old.0, new.0), (old.1, new.1)] {
                    if o != n {
                        if same(w.p1, o) {
                            w.p1 = n;
                        }
                        if same(w.p2, o) {
                            w.p2 = n;
                        }
                    }
                }
            }
        }
        if delete {
            self.delete_wire(i);
            self.wire_edit = None;
        } else if !open {
            self.wire_edit = None;
        }
    }

    fn optimizer_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("optleft").resizable(false).default_size(380.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("What to change");
                let nw = self.ant.wires.len();
                let mut del = None;
                egui::Grid::new("optvars").striped(true).show(ui, |ui| {
                    for (i, v) in self.opt_vars.iter().enumerate() {
                        ui.label(format!("Wire {}", v.wire + 1));
                        ui.label(v.kind.label());
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.opt_vars.remove(i);
                }
                ui.horizontal(|ui| {
                    wire_pick(ui, "newvarw", &mut self.new_var_wire, nw.max(1));
                    egui::ComboBox::from_id_salt("newvark").selected_text(self.new_var_kind.label()).show_ui(ui, |ui| {
                        for k in VarKind::ALL {
                            ui.selectable_value(&mut self.new_var_kind, k, k.label());
                        }
                    });
                    if ui.button("➕ Add").clicked() && nw > 0 {
                        let v = OptVar { wire: self.new_var_wire.min(nw - 1), kind: self.new_var_kind };
                        if !self.opt_vars.contains(&v) {
                            self.opt_vars.push(v);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.heading("Goals (weight 0 = ignore)");
                egui::Grid::new("goals").show(ui, |ui| {
                    ui.label("Low SWR");
                    ui.add(egui::Slider::new(&mut self.goals.w_swr, 0.0..=10.0));
                    ui.end_row();
                    ui.label("jX close to 0");
                    ui.add(egui::Slider::new(&mut self.goals.w_jx, 0.0..=10.0));
                    ui.end_row();
                    ui.label("High gain");
                    ui.add(egui::Slider::new(&mut self.goals.w_ga, 0.0..=10.0));
                    ui.end_row();
                    ui.label("High F/B");
                    ui.add(egui::Slider::new(&mut self.goals.w_fb, 0.0..=10.0));
                    ui.end_row();
                    ui.label("Most steps");
                    ui.add(egui::DragValue::new(&mut self.goals.max_iter).range(1..=1000));
                    ui.end_row();
                    ui.label("Threads");
                    ui.add(egui::Slider::new(&mut self.threads, 1..=self.max_threads));
                    ui.end_row();
                });
                ui.label(format!("At {:.3} MHz, SWR against {:.0} Ω.", self.ant.freq_mhz, self.ant.z0));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let running = matches!(self.job, Some(Job::Opt(..)));
                    if ui.add_enabled(!self.busy() && !self.opt_vars.is_empty(), egui::Button::new(RichText::new("▶ Start").size(16.0))).clicked() {
                        self.start_opt(ui.ctx());
                    }
                    if ui.add_enabled(running, egui::Button::new(RichText::new("■ Stop").size(16.0))).clicked() {
                        self.opt_stop.store(true, Ordering::Relaxed);
                    }
                    if running {
                        ui.spinner();
                    }
                });
                if let Some((a, txt)) = self.opt_result.clone() {
                    ui.add_space(6.0);
                    ui.label(txt);
                    ui.horizontal(|ui| {
                        if ui.button("✔ Use optimized antenna").clicked() {
                            self.opt_backup = Some(self.ant.clone());
                            self.ant = a;
                            self.calc = None;
                            self.opt_result = None;
                            self.status = "Optimized antenna applied. Press Calculate to see it.".into();
                        }
                    });
                }
                if let Some(b) = self.opt_backup.clone() {
                    if ui.button("↶ Go back to the antenna before optimizing").clicked() {
                        self.ant = b;
                        self.opt_backup = None;
                        self.calc = None;
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Progress");
            let avail = ui.available_size();
            let cost: Vec<(f64, f64)> = self.opt_log.iter().map(|s| (s.iter as f64, s.cost)).collect();
            plots::xy(ui, Vec2::new(avail.x - 4.0, 200.0), "Score (lower is better)", "step", &[Series { name: "score", color: plots::TOTAL, pts: cost, right: false }], None, None);
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("optlog").striped(true).spacing([14.0, 2.0]).show(ui, |ui| {
                    for h in ["Step", "Score", "R (Ω)", "jX (Ω)", "SWR", "Ga (dBi)", "F/B (dB)", "Calcs"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for s in &self.opt_log {
                        ui.label(format!("{}", s.iter));
                        ui.label(format!("{:.4}", s.cost));
                        ui.label(format!("{:.2}", s.r));
                        ui.label(format!("{:.2}", s.x));
                        ui.label(format!("{:.2}", s.swr));
                        ui.label(if s.ga.is_finite() { format!("{:.2}", s.ga) } else { "—".into() });
                        ui.label(if s.fb.is_finite() { format!("{:.2}", s.fb) } else { "—".into() });
                        ui.label(format!("{}", s.evals));
                        ui.end_row();
                    }
                });
            });
        });
    }
}

fn status_line(r: &CalcResult) -> String {
    format!(
        "{:.3} MHz: Z = {:.2} {} j{:.2} Ω, SWR {:.2}, Ga {:.2} dBi, F/B {:.2} dB — {} segments solved in {:.0} ms on {} threads",
        r.freq_mhz, r.z.re, if r.z.im < 0.0 { "−" } else { "+" }, r.z.im.abs(), r.swr, r.ga_dbi, r.fb_db, r.n_segs, r.t_total_ms, r.threads
    )
}

fn engine_pick(ui: &mut egui::Ui, id: &str, e: &mut Engine) {
    egui::ComboBox::from_id_salt(id).selected_text(e.label()).show_ui(ui, |ui| {
        ui.selectable_value(e, Engine::Wiremom, Engine::Wiremom.label());
        ui.selectable_value(e, Engine::Mininec, Engine::Mininec.label());
        if crate::nec::available() {
            ui.selectable_value(e, Engine::Nec2, Engine::Nec2.label());
        } else {
            ui.label("NEC-2 wasn't built into this copy");
        }
    })
    .response
    .on_hover_text("wiremom (default): built-in MPIE solver (triangle currents, Galerkin) with Sommerfeld real ground, wires in the ground, transmission lines and plates; multi-threaded in-process.\nMININEC: the original's method, real ground only in the far field.\nNEC-2: public-domain NEC-2 (nec2c) with Sommerfeld-Norton real ground; each calculation runs as its own process, sweeps run many at once.");
}

fn wire_pick(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, wire: &mut usize, n: usize) {
    egui::ComboBox::from_id_salt(id).width(60.0).selected_text(format!("{}", *wire + 1)).show_ui(ui, |ui| {
        for i in 0..n {
            ui.selectable_value(wire, i, format!("{}", i + 1));
        }
    });
}

fn pos_pick(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, pos: &mut f64) {
    ui.horizontal(|ui| {
        let label = if *pos <= 1e-9 { "begin" } else if (*pos - 0.5).abs() < 1e-9 { "centre" } else if *pos >= 1.0 - 1e-9 { "end" } else { "custom" };
        egui::ComboBox::from_id_salt(id).width(70.0).selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(pos, 0.0, "begin");
            ui.selectable_value(pos, 0.5, "centre");
            ui.selectable_value(pos, 1.0, "end");
            if ui.selectable_label(label == "custom", "custom").clicked() && label != "custom" {
                *pos = 0.25;
            }
        });
        if label == "custom" {
            let mut pct = *pos * 100.0;
            if ui.add(egui::DragValue::new(&mut pct).speed(0.5).range(0.1..=99.9).suffix(" %")).changed() {
                *pos = pct / 100.0;
            }
        }
    });
}

fn ground_controls(ui: &mut egui::Ui, ant: &mut Antenna) {
    egui::Grid::new(ui.next_auto_id()).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("Ground");
        egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(ant.ground.kind.label()).show_ui(ui, |ui| {
            for k in [GroundKind::Free, GroundKind::Perfect, GroundKind::Real] {
                ui.selectable_value(&mut ant.ground.kind, k, k.label());
            }
        });
        ui.end_row();
        if ant.ground.kind != GroundKind::Free {
            ui.label("Add height (m)");
            ui.add(egui::DragValue::new(&mut ant.ground.add_height).speed(0.1).range(0.0..=10000.0));
            ui.end_row();
        }
        if ant.ground.kind == GroundKind::Real {
            ui.label("Ground εr");
            ui.add(egui::DragValue::new(&mut ant.ground.eps_r).speed(0.1).range(1.0..=100.0));
            ui.end_row();
            ui.label("Ground σ (mS/m)");
            ui.add(egui::DragValue::new(&mut ant.ground.sigma_ms).speed(0.1).range(0.0..=10000.0));
            ui.end_row();
        }
        ui.label("Wire material");
        egui::ComboBox::from_id_salt(ui.next_auto_id()).selected_text(ant.material.label()).show_ui(ui, |ui| {
            for m in Material::ALL {
                ui.selectable_value(&mut ant.material, m, m.label());
            }
        });
        ui.end_row();
        if ant.material.is_user() {
            ui.label("Resistivity (Ω·m)");
            ui.add(egui::DragValue::new(&mut ant.user_rho).speed(1e-9).range(1e-9..=1.0).max_decimals(12));
            ui.end_row();
            ui.label("Permeability");
            ui.add(egui::DragValue::new(&mut ant.user_mu).speed(0.1).range(1.0..=100000.0));
            ui.end_row();
        }
        ui.label("Rear range for F/B");
        ui.horizontal(|ui| {
            ui.label("azimuth");
            ui.add(egui::DragValue::new(&mut ant.fb_az).speed(1.0).range(0.0..=360.0).suffix("°"));
            ui.label("vertical");
            ui.add(egui::DragValue::new(&mut ant.fb_el).speed(1.0).range(0.0..=180.0).suffix("°"));
        });
        ui.end_row();
        ui.label("SWR reference (Ω)");
        ui.add(egui::DragValue::new(&mut ant.z0).speed(1.0).range(1.0..=10000.0));
        ui.end_row();
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        self.poll();
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(shot) = &mut self.shot {
            ctx.request_repaint();
            if shot.done.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if self.frame >= 120 && !shot.requested {
                shot.requested = true;
                let path = shot.path.clone();
                let done = shot.done.clone();
                let c2 = ctx.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let mut buf = Vec::with_capacity(w * h * 4);
                    for p in &img.pixels {
                        buf.extend_from_slice(&p.to_srgba_unmultiplied());
                    }
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                        Some(im) => {
                            if let Err(e) = im.save(&path) {
                                eprintln!("could not save screenshot: {e}");
                            }
                        }
                        None => eprintln!("bad screenshot buffer"),
                    }
                    done.store(true, Ordering::Relaxed);
                    c2.request_repaint();
                });
            }
        }
        let title = format!(
            "{}{} — Wire Antenna Analyser",
            self.ant.name,
            self.path.as_ref().map(|p| format!(" [{}]", p.file_name().unwrap_or_default().to_string_lossy())).unwrap_or_default()
        );
        if self.frame % 30 == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("⚙ {} / {} threads", self.threads, self.max_threads)).color(Color32::from_rgb(60, 90, 160)));
                ui.separator();
                ui.label(&self.status);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Geometry => self.geometry(ui),
            Tab::View => self.view_tab(ui),
            Tab::Calculate => self.calculate_tab(ui),
            Tab::FarField => self.farfield_tab(ui),
            Tab::Sweep => self.sweep_tab(ui),
            Tab::Pattern3D => self.pattern3d_tab(ui),
            Tab::Optimizer => self.optimizer_tab(ui),
        });
        if let Some(e) = self.error.clone() {
            let mut open = true;
            egui::Window::new(if e.starts_with("Opened with notes") { "Notes" } else { "Problem" }).collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(&ctx, |ui| {
                ui.label(e);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
            if !open {
                self.error = None;
            }
        }
    }
}

/// Range for a view scroll bar: a few view widths at the current zoom, always
/// including where the view is now.
fn scroll_range(extent: f32, zoom: f32, pan: f32) -> f32 {
    (extent * zoom.max(1.0) * 1.5).max(pan.abs() * 1.2).max(50.0)
}
