//! Tools menu calculations: finding resonances and their bandwidth, and matching
//! networks (L networks, gamma match, series-section coax transformer).

use crate::model::Antenna;
use eframe::egui::{self, Color32, RichText};
use num_complex::Complex64 as C;
use std::f64::consts::PI;

/// Sweeps `ant` from `f0` to `f1` MHz (impedance only, all cores), then finds and
/// refines the resonances (see [`analyse`]).
pub fn find(ant: &Antenna, f0: f64, f1: f64, steps: usize, limit: f64) -> Result<ResonanceReport, String> {
    let sw = crate::mom::sweep(ant, f0, f1, steps, false)?;
    let pts: Vec<(f64, C)> = sw.points.iter().map(|p| (p.freq_mhz, p.z)).collect();
    // the same segmentation as the sweep
    let seg_f = f1.max(f0).max(ant.freq_mhz);
    analyse(pts, ant.z0, limit, |f| crate::mom::quick(ant, f, seg_f, false).map(|r| r.0.z))
}

/// Quarter-wave transformer for a complex load: a length of the `z0` feeder from
/// the load to where the impedance is real (R max or R min), then a quarter wave
/// of sqrt(z0 R).  Returns (feeder length °, R there, transformer impedance).
pub fn quarter_wave(zl: C, z0: f64) -> Vec<(f64, f64, f64)> {
    let g = (zl - z0) / (zl + z0);
    let m = g.norm();
    if m >= 1.0 {
        return vec![];
    }
    let s = (1.0 + m) / (1.0 - m);
    // moving toward the generator by θ rotates Γ by -2θ: real positive at θ = arg/2
    let t_max = if m < 1e-9 { 0.0 } else { (0.5 * g.arg()).rem_euclid(PI) };
    let t_min = (t_max + PI / 2.0).rem_euclid(PI);
    let mut v = vec![(t_max.to_degrees(), z0 * s, (z0 * z0 * s).sqrt()), (t_min.to_degrees(), z0 / s, (z0 * z0 / s).sqrt())];
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    v
}

// ------------------------------------------------------------------ windows

#[derive(Clone, Copy, PartialEq)]
pub enum MatchKind {
    LNetwork,
    Gamma,
    Coax,
}

pub struct ToolsState {
    pub res_open: bool,
    pub res_f0: f64,
    pub res_f1: f64,
    pub res_steps: usize,
    pub res_limit: f64,
    pub report: Option<ResonanceReport>,
    pub sel: usize,
    pub match_open: bool,
    pub m_f: f64,
    pub m_r: f64,
    pub m_x: f64,
    pub m_z0: f64,
    pub m_kind: MatchKind,
    /// gamma: element and rod diameters, spacing (mm)
    pub g_del: f64,
    pub g_drod: f64,
    pub g_s: f64,
    /// coax section impedance and velocity factor
    pub c_z1: f64,
    pub c_vf: f64,
}

impl Default for ToolsState {
    fn default() -> Self {
        ToolsState {
            res_open: false,
            res_f0: 0.0,
            res_f1: 0.0,
            res_steps: 31,
            res_limit: 2.0,
            report: None,
            sel: 0,
            match_open: false,
            m_f: 14.2,
            m_r: 25.0,
            m_x: -15.0,
            m_z0: 50.0,
            m_kind: MatchKind::LNetwork,
            g_del: 25.0,
            g_drod: 12.0,
            g_s: 150.0,
            c_z1: 75.0,
            c_vf: 0.66,
        }
    }
}

impl ToolsState {
    /// A resonance search range of ±15% around `f_mhz`.
    pub fn range_around(&mut self, f_mhz: f64) {
        self.res_f0 = (f_mhz * 0.85 * 1000.0).round() / 1000.0;
        self.res_f1 = (f_mhz * 1.15 * 1000.0).round() / 1000.0;
    }

    /// A different antenna was loaded: its frequency for the resonance range and
    /// the match calculator, and none of the previous antenna's results.
    pub fn for_antenna(&mut self, ant: &Antenna) {
        self.range_around(ant.freq_mhz);
        self.report = None;
        self.sel = 0;
        self.m_f = ant.freq_mhz;
        self.m_z0 = ant.z0;
        self.m_r = ant.z0;
        self.m_x = 0.0;
    }

    /// Before opening the resonance finder: a range that holds the design
    /// frequency (kept if the user set one that does).
    pub fn range_for(&mut self, f_mhz: f64) {
        if !(self.res_f0 > 0.0 && self.res_f0 < f_mhz && f_mhz < self.res_f1) {
            self.range_around(f_mhz);
        }
    }
}

pub enum ToolAction {
    FindResonances,
    SetFreq(f64),
    /// match the impedance found at this resonance
    MatchAt(f64, C),
    /// fill the match calculator from the last calculation
    UseLast,
}

fn khz(mhz: f64) -> String {
    format!("{:.1} kHz", mhz * 1000.0)
}

pub fn resonance_window(ctx: &egui::Context, st: &mut ToolsState, z0: f64, busy: bool) -> Vec<ToolAction> {
    let mut acts = vec![];
    let mut open = st.res_open;
    egui::Window::new("Resonance finder").open(&mut open).default_width(640.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("From");
            ui.add(egui::DragValue::new(&mut st.res_f0).speed(0.01).range(0.001..=100000.0).suffix(" MHz"));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut st.res_f1).speed(0.01).range(0.001..=100000.0).suffix(" MHz"));
            ui.label("points");
            ui.add(egui::DragValue::new(&mut st.res_steps).speed(1.0).range(11..=2001));
            ui.label("bandwidth at SWR");
            ui.add(egui::DragValue::new(&mut st.res_limit).speed(0.05).range(1.05..=10.0));
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy && st.res_f1 > st.res_f0, egui::Button::new("▶ Find resonances")).clicked() {
                acts.push(ToolAction::FindResonances);
            }
            if busy {
                ui.spinner();
            }
            ui.label(RichText::new(format!("SWR against {z0} Ω (the SWR reference on the Calculate tab)")).small().color(Color32::GRAY));
        });
        let Some(rep) = &st.report else {
            ui.label("Sweeps the range, finds where the reactance crosses zero, and measures the SWR bandwidth around each resonance.");
            return;
        };
        ui.separator();
        if rep.resonances.is_empty() {
            ui.label("No resonance (reactance zero) in this range.");
        } else {
            egui::Grid::new("res_table").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                for h in ["", "Frequency", "Type", "R (Ω)", "SWR", &format!("SWR ≤ {} vs {} Ω", rep.limit, rep.z0), "matched to R", "Q", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (i, r) in rep.resonances.iter().enumerate() {
                    ui.radio_value(&mut st.sel, i, "");
                    ui.label(format!("{:.4} MHz", r.f_mhz));
                    ui.label(if r.series { "series" } else { "parallel" });
                    ui.label(format!("{:.1}", r.z.re));
                    ui.label(if r.swr.is_finite() { format!("{:.2}", r.swr) } else { "∞".into() });
                    let bw = |b: Option<(f64, f64)>| match b {
                        Some((lo, hi)) => {
                            let edge = |f: f64| (f - rep.points[0].0).abs() < 1e-9 || (f - rep.points.last().unwrap().0).abs() < 1e-9;
                            let more = if edge(lo) || edge(hi) { " (at range edge)" } else { "" };
                            format!("{} ({:.2} %){more}\n{:.4}–{:.4}", khz(hi - lo), 100.0 * (hi - lo) / r.f_mhz, lo, hi)
                        }
                        None => "—".into(),
                    };
                    ui.label(bw(r.bw_ref));
                    ui.label(bw(r.bw_matched));
                    ui.label(if r.q.is_finite() { format!("{:.1}", r.q) } else { "—".into() });
                    ui.horizontal(|ui| {
                        if ui.small_button("Use frequency").clicked() {
                            acts.push(ToolAction::SetFreq(r.f_mhz));
                        }
                        if ui.small_button("Match…").on_hover_text("Open the match calculator with this impedance").clicked() {
                            acts.push(ToolAction::MatchAt(r.f_mhz, r.z));
                        }
                    });
                    ui.end_row();
                }
            });
        }
        ui.label(format!("Lowest SWR in the range: {:.2} at {:.4} MHz.", rep.min_swr.1, rep.min_swr.0));
        ui.label(
            RichText::new("\"matched to R\": the bandwidth once the antenna is matched at resonance (SWR against its own resistance). Q = f·|dZ/df| / 2R.")
                .small()
                .color(Color32::GRAY),
        );
        let w = ui.available_width();
        let sz = egui::Vec2::new(w, 170.0);
        let mark = rep.resonances.get(st.sel).map(|r| r.f_mhz);
        let swr_pts: Vec<(f64, f64)> = rep.points.iter().map(|&(f, z)| (f, swr(z, rep.z0).min(10.0))).collect();
        crate::plots::xy(ui, sz, &format!("SWR ({} Ω)", rep.z0), "MHz", &[crate::plots::Series { name: "SWR", color: crate::plots::TOTAL, pts: swr_pts, right: false }], Some((1.0, 10.0)), mark);
        crate::plots::xy(
            ui,
            sz,
            "Impedance",
            "MHz",
            &[
                crate::plots::Series { name: "R", color: crate::plots::HORC, pts: rep.points.iter().map(|&(f, z)| (f, z.re)).collect(), right: false },
                crate::plots::Series { name: "jX", color: crate::plots::VERC, pts: rep.points.iter().map(|&(f, z)| (f, z.im)).collect(), right: false },
            ],
            None,
            mark,
        );
    });
    st.res_open = open;
    acts
}

pub fn match_window(ctx: &egui::Context, st: &mut ToolsState, have_last: bool) -> Vec<ToolAction> {
    let mut acts = vec![];
    let mut open = st.match_open;
    egui::Window::new("Match calculator").open(&mut open).default_width(560.0).show(ctx, |ui| {
        egui::Grid::new("match_in").spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Antenna impedance");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut st.m_r).speed(0.1).range(0.01..=100000.0).prefix("R ").suffix(" Ω"));
                ui.add(egui::DragValue::new(&mut st.m_x).speed(0.1).range(-100000.0..=100000.0).prefix("X ").suffix(" Ω"));
                if ui.add_enabled(have_last, egui::Button::new("From last calculation")).clicked() {
                    acts.push(ToolAction::UseLast);
                }
            });
            ui.end_row();
            ui.label("Frequency");
            ui.add(egui::DragValue::new(&mut st.m_f).speed(0.01).range(0.001..=100000.0).suffix(" MHz"));
            ui.end_row();
            ui.label("Match to");
            ui.add(egui::DragValue::new(&mut st.m_z0).speed(0.5).range(1.0..=10000.0).suffix(" Ω"));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.m_kind, MatchKind::LNetwork, "L and C (L network)");
            ui.selectable_value(&mut st.m_kind, MatchKind::Gamma, "Gamma match");
            ui.selectable_value(&mut st.m_kind, MatchKind::Coax, "Coax sections");
        });
        ui.separator();
        let zl = C::new(st.m_r, st.m_x);
        let f_hz = st.m_f * 1e6;
        let r0 = st.m_z0;
        ui.label(format!("SWR now: {:.2} against {} Ω", swr(zl, r0), r0));
        match st.m_kind {
            MatchKind::LNetwork => {
                let sols = l_networks(zl, r0);
                if sols.is_empty() {
                    ui.label("No L network: the resistance must be positive.");
                }
                for (k, (xp, xs, at_load)) in sols.iter().enumerate() {
                    let kind = |x: f64| if x > 0.0 { "L" } else { "C" };
                    let (first, second) = if *at_load { ("across the antenna", "in series toward the transmitter") } else { ("in series with the antenna", "across the transmitter side") };
                    let (x1, x2) = if *at_load { (*xp, *xs) } else { (*xs, *xp) };
                    let name = format!("{} {} / {} {}", kind(x1), if *at_load { "shunt" } else { "series" }, kind(x2), if *at_load { "series" } else { "shunt" });
                    egui::CollapsingHeader::new(RichText::new(format!("Option {}: {name}", k + 1)).strong()).default_open(true).id_salt(("lnet", k)).show(ui, |ui| {
                        let show = |ui: &mut egui::Ui, x: f64, where_: &str| {
                            if x.is_infinite() {
                                ui.label(format!("• nothing {where_} (not needed)"));
                            } else if x.abs() < 1e-6 {
                                ui.label(format!("• nothing {where_}"));
                            } else {
                                ui.label(format!("• {} {where_}", part(x, f_hz)));
                            }
                        };
                        show(ui, x1, first);
                        show(ui, x2, second);
                        let tag = if *at_load {
                            if x1 < 0.0 && x2 > 0.0 { "low-pass" } else if x1 > 0.0 && x2 < 0.0 { "high-pass" } else { "" }
                        } else if x1 > 0.0 && x2 < 0.0 {
                            "low-pass"
                        } else if x1 < 0.0 && x2 > 0.0 {
                            "high-pass"
                        } else {
                            ""
                        };
                        if !tag.is_empty() {
                            ui.label(RichText::new(format!("{tag} (harmonics {})", if tag == "low-pass" { "attenuated" } else { "passed" })).small().color(Color32::GRAY));
                        }
                    });
                }
                ui.label(RichText::new("Components are ideal (no loss). \"Across the antenna\" = shunt at the feedpoint.").small().color(Color32::GRAY));
            }
            MatchKind::Gamma => {
                egui::Grid::new("gamma_in").spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Element diameter");
                    ui.add(egui::DragValue::new(&mut st.g_del).speed(0.1).range(0.1..=1000.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Rod diameter");
                    ui.add(egui::DragValue::new(&mut st.g_drod).speed(0.1).range(0.1..=1000.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Rod spacing (centres)");
                    ui.add(egui::DragValue::new(&mut st.g_s).speed(1.0).range(1.0..=5000.0).suffix(" mm"));
                    ui.end_row();
                });
                if st.g_s * 2.0 <= st.g_del + st.g_drod {
                    ui.label("The rod overlaps the element: increase the spacing.");
                } else {
                    match gamma_match(zl, r0, f_hz, st.g_del * 1e-3, st.g_drod * 1e-3, st.g_s * 1e-3) {
                        Some((l, xc)) => {
                            ui.label(RichText::new(format!("Rod length (feed to shorting strap): {:.3} m", l)).strong());
                            if xc < 0.0 {
                                ui.label(RichText::new(format!("Series capacitor: {}", part(xc, f_hz))).strong());
                            } else {
                                ui.label(format!("Remaining reactance is capacitive ({:.1} Ω): add {} in series instead of a capacitor, or shorten the element.", -xc, part(xc, f_hz)));
                            }
                        }
                        None => {
                            ui.label("No rod length up to λ/4 reaches the target resistance: use a thinner rod or wider spacing (more step-up), or match a higher element impedance.");
                        }
                    }
                }
                ui.label(
                    RichText::new(
                        "Enter the element's centre impedance as a split dipole. Transmission-line model (ARRL Antenna Book, after Healey): \
                         approximate, so model the rod as wires and fine-tune with the calculator's Z.",
                    )
                    .small()
                    .color(Color32::GRAY),
                );
            }
            MatchKind::Coax => {
                ui.horizontal(|ui| {
                    ui.label("Section impedance");
                    egui::ComboBox::from_id_salt("coax_z1").selected_text(format!("{} Ω", st.c_z1)).show_ui(ui, |ui| {
                        for z in [35.0, 50.0, 52.0, 75.0, 93.0, 300.0, 450.0, 600.0] {
                            ui.selectable_value(&mut st.c_z1, z, format!("{z} Ω"));
                        }
                    });
                    ui.add(egui::DragValue::new(&mut st.c_z1).speed(0.5).range(1.0..=1000.0).suffix(" Ω"));
                    ui.label("velocity factor");
                    ui.add(egui::DragValue::new(&mut st.c_vf).speed(0.01).range(0.3..=1.0));
                });
                let lam = 299.792458 / st.m_f; // free-space wavelength, m
                let len = |deg: f64, vf: f64| deg / 360.0 * lam * vf;
                ui.label(RichText::new("Series section (Regier)").strong());
                let ss = series_section(zl, r0, st.c_z1);
                if ss.is_empty() {
                    let s_load = swr(zl, r0);
                    ui.label(format!(
                        "Not possible with {} Ω: a series section reaches SWR up to ({}/{})² = {:.2}; this load is {:.2}. Pick a section impedance further from {} Ω.",
                        st.c_z1, st.c_z1, r0, (st.c_z1 / r0).max(r0 / st.c_z1).powi(2), s_load, r0
                    ));
                } else {
                    for (th2, th1) in &ss {
                        ui.label(format!(
                            "• from the antenna: {:.1}° of {} Ω feeder ({:.3} m at VF {}), then {:.1}° of {} Ω ({:.3} m at VF {}), then the {} Ω feeder",
                            th2, r0, len(*th2, st.c_vf), st.c_vf, th1, st.c_z1, len(*th1, st.c_vf), st.c_vf, r0
                        ));
                    }
                }
                ui.add_space(4.0);
                ui.label(RichText::new("Quarter-wave transformer").strong());
                for (th, r, zq) in quarter_wave(zl, r0) {
                    ui.label(format!(
                        "• {:.1}° of {} Ω feeder ({:.3} m) to where it is {:.1} Ω, then a quarter wave ({:.3} m at VF {}) of {:.1} Ω",
                        th, r0, len(th, st.c_vf), r, len(90.0, st.c_vf), st.c_vf, zq
                    ));
                }
                ui.label(RichText::new("Lossless lines; the same velocity factor is used for both kinds of line.").small().color(Color32::GRAY));
            }
        }
    });
    st.match_open = open;
    acts
}

// ------------------------------------------------------------------ resonances

/// One resonance (reactance crossing zero).
#[derive(Clone, Debug)]
pub struct Resonance {
    pub f_mhz: f64,
    /// impedance at the resonance (X ~ 0)
    pub z: C,
    /// series resonance (X rising through zero, low R); otherwise parallel
    /// (anti-resonance, X falling, high R)
    pub series: bool,
    /// SWR against the reference impedance at the resonance
    pub swr: f64,
    /// band where SWR <= limit against the reference impedance (MHz), if any,
    /// and whether each edge lies inside the swept range
    pub bw_ref: Option<(f64, f64)>,
    /// band where SWR <= limit against the resonant resistance: the bandwidth once
    /// the antenna is matched there
    pub bw_matched: Option<(f64, f64)>,
    /// Q from the impedance slope: f0 |dZ/df| / (2 R)
    pub q: f64,
}

#[derive(Clone, Debug)]
pub struct ResonanceReport {
    pub resonances: Vec<Resonance>,
    /// the swept points (MHz, Z)
    pub points: Vec<(f64, C)>,
    /// lowest SWR in the range (MHz, SWR against the reference)
    pub min_swr: (f64, f64),
    pub z0: f64,
    pub limit: f64,
}

pub fn swr(z: C, z0: f64) -> f64 {
    let g = ((z - z0) / (z + z0)).norm();
    if g >= 1.0 { f64::INFINITY } else { (1.0 + g) / (1.0 - g) }
}

/// Finds resonances from samples `pts` (ascending frequency), refining each
/// reactance zero with `z_at` (a few extra solves), and measures SWR bandwidths
/// against `z0` and against the resonant resistance (limit `limit`, e.g. 2).
pub fn analyse(pts: Vec<(f64, C)>, z0: f64, limit: f64, mut z_at: impl FnMut(f64) -> Result<C, String>) -> Result<ResonanceReport, String> {
    if pts.len() < 2 {
        return Err("The sweep needs at least two frequencies.".into());
    }
    let min_swr = pts.iter().map(|&(f, z)| (f, swr(z, z0))).fold((pts[0].0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a });
    let mut res = Vec::new();
    for w in pts.windows(2) {
        let ((fa, za), (fb, zb)) = (w[0], w[1]);
        if za.im == 0.0 || za.im.signum() == zb.im.signum() {
            continue;
        }
        // secant / bisection on X between the two samples
        let (mut lo, mut hi, mut xlo, mut xhi) = (fa, fb, za.im, zb.im);
        let (mut f, mut z) = (fa, za);
        for _ in 0..6 {
            let t = (xlo / (xlo - xhi)).clamp(0.05, 0.95);
            f = lo + (hi - lo) * t;
            z = z_at(f)?;
            if z.im.abs() < 1e-6 * z.norm().max(1e-9) {
                break;
            }
            if z.im.signum() == xlo.signum() {
                lo = f;
                xlo = z.im;
            } else {
                hi = f;
                xhi = z.im;
            }
            if (hi - lo) < 1e-7 * f {
                break;
            }
        }
        let series = zb.im > za.im;
        // slope from a small step either side
        let h = f * 2e-4;
        let (zm, zp) = (z_at(f - h)?, z_at(f + h)?);
        let dz = (zp - zm) / (2.0 * h);
        let q = if z.re > 0.0 { f * dz.norm() / (2.0 * z.re) } else { f64::INFINITY };
        // the samples with the refined point in its place, for the bandwidths
        let mut ps: Vec<(f64, C)> = pts.iter().cloned().filter(|p| (p.0 - f).abs() > 1e-12).collect();
        ps.push((f, z));
        ps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let r_res = z.re.max(1e-9);
        let bw_ref = refine_band(&ps, f, |z| swr(z, z0), limit, &mut z_at)?;
        let bw_matched = refine_band(&ps, f, |z| swr(z, r_res), limit, &mut z_at)?;
        res.push(Resonance { f_mhz: f, z, series, swr: swr(z, z0), bw_ref, bw_matched, q });
    }
    Ok(ResonanceReport { resonances: res, points: pts, min_swr, z0, limit })
}

/// The band around `f` (MHz) where `metric(Z) <= limit`: the samples bracket each
/// edge, which is then found with a few solves (regula falsi, starting from Z
/// interpolated linearly between the bracketing samples), so the edges are
/// accurate even when the band is narrower than the sample spacing.  None if the
/// limit isn't met at `f`; an edge beyond the swept range is the range's end.
fn refine_band(
    ps: &[(f64, C)],
    f: f64,
    metric: impl Fn(C) -> f64,
    limit: f64,
    z_at: &mut impl FnMut(f64) -> Result<C, String>,
) -> Result<Option<(f64, f64)>, String> {
    let Some(i) = ps.iter().position(|p| p.0 == f) else { return Ok(None) };
    if metric(ps[i].1) > limit {
        return Ok(None);
    }
    let g = |z: C| metric(z) - limit;
    let mut edge = |a: (f64, C), b: (f64, C)| -> Result<f64, String> {
        // a inside (g <= 0), b outside: first guess along the straight line in Z
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let t = 0.5 * (lo + hi);
            if g(a.1 + (b.1 - a.1) * t) <= 0.0 {
                lo = t;
            } else {
                hi = t;
            }
        }
        let (mut fa, mut ga, mut fb, mut gb) = (a.0, g(a.1), b.0, g(b.1));
        let mut x = a.0 + (b.0 - a.0) * lo;
        for _ in 0..5 {
            let gx = g(z_at(x)?);
            if gx.abs() < 1e-3 {
                return Ok(x);
            }
            if gx <= 0.0 {
                fa = x;
                ga = gx;
            } else {
                fb = x;
                gb = gx;
            }
            if (fb - fa).abs() < 2e-6 * f {
                break;
            }
            x = fa + (fb - fa) * (ga / (ga - gb)).clamp(0.05, 0.95);
        }
        Ok(x)
    };
    let mut lo = ps[0].0;
    for k in (0..i).rev() {
        if g(ps[k].1) > 0.0 {
            lo = edge(ps[k + 1], ps[k])?;
            break;
        }
    }
    let mut hi = ps[ps.len() - 1].0;
    for k in i + 1..ps.len() {
        if g(ps[k].1) > 0.0 {
            hi = edge(ps[k - 1], ps[k])?;
            break;
        }
    }
    Ok(Some((lo, hi)))
}

// ------------------------------------------------------------------ matching

/// Reactance -> component description at `f_hz` (positive: inductor).
pub fn part(x: f64, f_hz: f64) -> String {
    let w = 2.0 * PI * f_hz;
    if x.abs() < 1e-9 {
        "a short (0 Ω)".into()
    } else if x > 0.0 {
        format!("L {} (X = +{:.1} Ω)", eng(x / w, "H"), x)
    } else {
        format!("C {} (X = {:.1} Ω)", eng(1.0 / (w * -x), "F"), x)
    }
}

/// Number with an SI prefix (µ, n, p…).
pub fn eng(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (s, p) = if a >= 1.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else if a >= 1e-6 {
        (1e6, "µ")
    } else if a >= 1e-9 {
        (1e9, "n")
    } else {
        (1e12, "p")
    };
    let x = v * s;
    // about four significant figures
    let d = if x.abs() >= 100.0 { 1 } else if x.abs() >= 10.0 { 2 } else { 3 };
    format!("{x:.d$} {p}{unit}")
}

/// The L networks (two reactances) that match load `zl` to resistance `r0`.
/// Each solution: (shunt reactance, series reactance, shunt at the load?).
/// Shunt at the load works when Re(1/zl) <= 1/r0 (i.e. G_L <= 1/R0); series
/// first when R_L <= r0.
pub fn l_networks(zl: C, r0: f64) -> Vec<(f64, f64, bool)> {
    let mut out = Vec::new();
    let (rl, xl) = (zl.re, zl.im);
    if rl <= 0.0 {
        return out;
    }
    // shunt element at the load, then series toward the source:
    // 1/(1/zl + 1/(jXp)) must have real part r0
    let yl = 1.0 / zl;
    let (g, b) = (yl.re, yl.im);
    if g <= 1.0 / r0 + 1e-15 {
        // need (G, B + Bp) with G / (G² + B'²) = r0  ->  B'² = G/r0 - G²
        let d = (g / r0 - g * g).max(0.0).sqrt();
        for bp in [d, -d] {
            let b_shunt = bp - b; // susceptance to add
            let z = 1.0 / C::new(g, bp);
            let xs = -z.im; // series reactance to cancel what remains
            let xp = if b_shunt.abs() < 1e-15 { f64::INFINITY } else { -1.0 / b_shunt };
            out.push((xp, xs, true));
        }
    }
    // series element at the load, then shunt across the source:
    // zl + jXs must have conductance 1/r0
    if rl <= r0 + 1e-12 {
        let d = (rl * (r0 - rl)).max(0.0).sqrt(); // X' with Re(1/(rl + jX')) = 1/r0
        for xp_total in [d, -d] {
            let xs = xp_total - xl;
            let y = 1.0 / C::new(rl, xp_total);
            let b_shunt = -y.im;
            let xp = if b_shunt.abs() < 1e-15 { f64::INFINITY } else { -1.0 / b_shunt };
            out.push((xp, xs, false));
        }
    }
    out
}

/// Impedance seen through an L network (for checking).
#[cfg(test)]
fn through_l(zl: C, xp: f64, xs: f64, shunt_at_load: bool) -> C {
    let par = |a: C, x: f64| if x.is_infinite() { a } else { let zp = C::new(0.0, x); a * zp / (a + zp) };
    if shunt_at_load { par(zl, xp) + C::new(0.0, xs) } else { par(zl + C::new(0.0, xs), xp) }
}

/// Series-section transformer (Regier): along a feeder of `z0`, a length `θ2`
/// of the same `z0` line from the load, then a section of `z1` of length `θ1`,
/// matches `zl`.  Returns the solutions (θ2, θ1) in degrees, 0..180.
pub fn series_section(zl: C, z0: f64, z1: f64) -> Vec<(f64, f64)> {
    // search θ1 for which some θ2 works: the z0 line moves the load round its SWR
    // circle, so the section must turn a point of that circle into z0.  Equivalent:
    // looking back from the feeder through the section, z0 must land on the circle.
    let g_load = ((zl - z0) / (zl + z0)).norm();
    let mut out = vec![];
    let f = |th1: f64| {
        // impedance the section presents looking toward the load must be z0 when
        // fed with the point of the circle; invert: z0 seen back through the section
        let back = line_in(C::new(z0, 0.0), z1, -th1);
        ((back - z0) / (back + z0)).norm() - g_load
    };
    let n = 720;
    for k in 0..n {
        let (a, b) = (PI * k as f64 / n as f64, PI * (k + 1) as f64 / n as f64);
        let (fa, fb) = (f(a), f(b));
        if fa == 0.0 || fa.signum() != fb.signum() {
            let (mut lo, mut hi, mut flo) = (a, b, fa);
            for _ in 0..60 {
                let m = 0.5 * (lo + hi);
                let fm = f(m);
                if fm.signum() == flo.signum() {
                    lo = m;
                    flo = fm;
                } else {
                    hi = m;
                }
            }
            let th1 = 0.5 * (lo + hi);
            if th1 < 1e-6 {
                continue;
            }
            // the point of the load's circle the section needs, and how far along
            // the z0 line it is
            let target = line_in(C::new(z0, 0.0), z1, -th1);
            let gt = (target - z0) / (target + z0);
            let gl = (zl - z0) / (zl + z0);
            // moving toward the generator by θ rotates Γ by -2θ
            let th2 = (0.5 * (gl.arg() - gt.arg())).rem_euclid(PI);
            out.push((th2.to_degrees(), th1.to_degrees()));
        }
    }
    out
}

/// Input impedance of a lossless line of impedance `zc`, electrical length
/// `theta` (radians), terminated in `zl`.
pub fn line_in(zl: C, zc: f64, theta: f64) -> C {
    let t = C::new(0.0, theta.tan());
    zc * (zl + zc * t) / (zc + zl * t)
}

/// Gamma match, transmission-line model (as in the ARRL Antenna Book, after
/// Healey): the rod and the element form a shorted line of impedance
/// `276 log10(2S / sqrt(D d))`, and the element's feed impedance, seen from the
/// rod, is stepped up by `(1 + ln(2S/d)/ln(2S/D))²` (unequal conductors) and
/// halved (one side of the element).  That appears in parallel with the shorted
/// line; a series capacitor cancels the remaining reactance.
///
/// `z_el`: the element's centre impedance (split, as a dipole), `d_el`/`d_rod`
/// diameters and `s` centre spacing (m).  Returns the rod length (m) for which the
/// resistance is `r0`, and the series capacitor's reactance (Ω, negative); None if
/// no length up to a quarter wave reaches `r0`.  Approximate: check by modelling
/// the rod.
#[allow(clippy::too_many_arguments)]
pub fn gamma_match(z_el: C, r0: f64, f_hz: f64, d_el: f64, d_rod: f64, s: f64) -> Option<(f64, f64)> {
    let z_line = 276.0 * (2.0 * s / (d_el * d_rod).sqrt()).log10();
    let step = (1.0 + (2.0 * s / d_rod).ln() / (2.0 * s / d_el).ln()).powi(2);
    let lambda = 299_792_458.0 / f_hz;
    let beta = 2.0 * PI / lambda;
    let zt = z_el * step / 2.0;
    let z_in = |l: f64| {
        let zs = C::new(0.0, z_line * (beta * l).tan());
        zt * zs / (zt + zs)
    };
    // R rises from 0 with the rod length: find where it reaches r0
    let n = 400;
    let lmax = 0.24 * lambda;
    let mut prev = (0.0, -r0);
    for k in 1..=n {
        let l = lmax * k as f64 / n as f64;
        let fv = z_in(l).re - r0;
        if prev.1 < 0.0 && fv >= 0.0 {
            let (mut lo, mut hi) = (prev.0, l);
            for _ in 0..60 {
                let m = 0.5 * (lo + hi);
                if z_in(m).re < r0 {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let l = 0.5 * (lo + hi);
            return Some((l, -z_in(l).im));
        }
        prev = (l, fv);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// series RLC: R 50, resonant at 14 MHz with Q 10
    fn rlc(f_mhz: f64) -> C {
        let (r, f0, q) = (50.0, 14.0, 10.0);
        let l = q * r / (2.0 * PI * f0 * 1e6);
        let c = 1.0 / ((2.0 * PI * f0 * 1e6).powi(2) * l);
        let w = 2.0 * PI * f_mhz * 1e6;
        C::new(r, w * l - 1.0 / (w * c))
    }

    #[test]
    fn tools_follow_a_new_antenna() {
        let mut st = ToolsState::default();
        let mut a = Antenna::example(0);
        a.freq_mhz = 7.1;
        st.for_antenna(&a);
        assert!(st.res_f0 < 7.1 && st.res_f1 > 7.1);
        st.report = Some(ResonanceReport { resonances: vec![], points: vec![(7.0, C::new(50.0, 0.0))], min_swr: (7.0, 1.0), z0: 50.0, limit: 2.0 });
        a.freq_mhz = 50.2;
        st.for_antenna(&a);
        assert!(st.report.is_none() && st.res_f0 < 50.2 && st.res_f1 > 50.2 && st.m_f == 50.2);
        // a user range around the frequency is kept; one that misses it is reset
        st.res_f0 = 49.0;
        st.res_f1 = 52.0;
        st.range_for(50.2);
        assert_eq!((st.res_f0, st.res_f1), (49.0, 52.0));
        st.range_for(144.3);
        assert!(st.res_f0 < 144.3 && st.res_f1 > 144.3);
    }

    #[test]
    fn finds_an_rlc_resonance_its_q_and_bandwidth() {
        let pts: Vec<(f64, C)> = (0..41).map(|i| 12.0 + 0.1 * i as f64).map(|f| (f, rlc(f))).collect();
        let rep = analyse(pts, 50.0, 2.0, |f| Ok(rlc(f))).unwrap();
        assert_eq!(rep.resonances.len(), 1);
        let r = &rep.resonances[0];
        assert!(r.series && (r.f_mhz - 14.0).abs() < 1e-6, "{r:?}");
        assert!((r.q - 10.0).abs() < 0.05, "{}", r.q);
        // SWR 2 bandwidth of a series RLC: |X| = R (s-1)/sqrt(s) at the edges
        let (lo, hi) = r.bw_ref.unwrap();
        let want = 14.0 * (1.0 / 2f64.sqrt()) / 10.0; // (s-1)/(Q sqrt s) f0
        assert!(((hi - lo) - want).abs() < 0.01 * want, "{} vs {want}", hi - lo);
        assert!((rep.min_swr.1 - 1.0).abs() < 0.05);
    }

    #[test]
    fn narrow_band_between_samples_is_found_by_solving() {
        // Q 200: the 2:1 band (50 kHz) is narrower than the 100 kHz sample spacing
        let hq = |f_mhz: f64| {
            let (r, f0, q) = (50.0, 14.0, 200.0);
            let l = q * r / (2.0 * PI * f0 * 1e6);
            let c = 1.0 / ((2.0 * PI * f0 * 1e6).powi(2) * l);
            let w = 2.0 * PI * f_mhz * 1e6;
            C::new(r, w * l - 1.0 / (w * c))
        };
        let pts: Vec<(f64, C)> = (0..21).map(|i| 13.0 + 0.1 * i as f64).map(|f| (f, hq(f))).collect();
        let mut solves = 0;
        let rep = analyse(pts, 50.0, 2.0, |f| { solves += 1; Ok(hq(f)) }).unwrap();
        let r = &rep.resonances[0];
        let (lo, hi) = r.bw_ref.unwrap();
        let want = 14.0 / 2f64.sqrt() / 200.0;
        assert!(((hi - lo) - want).abs() < 0.01 * want, "{} vs {want}", hi - lo);
        assert!(solves < 40, "{solves}");
    }

    #[test]
    fn parallel_resonance_is_found_too() {
        // the dual of the series circuit: Z = 2500 * 50 / Z_series, 2500 ohm at resonance
        let par = |f: f64| C::new(2500.0 * 50.0, 0.0) / rlc(f);
        let pts: Vec<(f64, C)> = (0..41).map(|i| 12.0 + 0.1 * i as f64).map(|f| (f, par(f))).collect();
        let rep = analyse(pts, 50.0, 2.0, |f| Ok(par(f))).unwrap();
        assert_eq!(rep.resonances.len(), 1);
        let r = &rep.resonances[0];
        assert!(!r.series && (r.z.re - 2500.0).abs() < 1.0, "{r:?}");
        assert!(r.bw_ref.is_none() && r.bw_matched.is_some());
    }

    #[test]
    fn component_values_read_well() {
        assert_eq!(eng(281.8482e-9, "H"), "281.8 nH");
        assert_eq!(eng(2.2e-6, "H"), "2.200 µH");
        assert_eq!(eng(47e-12, "F"), "47.00 pF");
    }

    #[test]
    fn l_networks_match() {
        for zl in [C::new(12.0, -30.0), C::new(200.0, 80.0), C::new(50.0, 40.0), C::new(25.0, 0.0), C::new(3000.0, -500.0)] {
            let sols = l_networks(zl, 50.0);
            assert!(!sols.is_empty(), "{zl}");
            for (xp, xs, at_load) in sols {
                let z = through_l(zl, xp, xs, at_load);
                assert!((z - C::new(50.0, 0.0)).norm() < 1e-6, "{zl}: {xp} {xs} {at_load} -> {z}");
            }
        }
    }

    #[test]
    fn gamma_match_reaches_the_resistance() {
        // 14 MHz element 25 mm, rod 12 mm at 150 mm, element 25 ohm
        let (l, xc) = gamma_match(C::new(25.0, 0.0), 50.0, 14.2e6, 0.025, 0.012, 0.15).unwrap();
        assert!(l > 0.1 && l < 3.0 && xc < 0.0, "{l} {xc}");
    }

    #[test]
    fn quarter_wave_points_are_real() {
        for zl in [C::new(25.0, 30.0), C::new(120.0, -40.0), C::new(10.0, 0.0)] {
            for (th, r, zq) in quarter_wave(zl, 50.0) {
                let z = line_in(zl, 50.0, th.to_radians());
                assert!((z - C::new(r, 0.0)).norm() < 1e-6 * r.max(1.0), "{zl}: {th} -> {z} vs {r}");
                // the quarter wave of zq turns r into 50
                assert!((zq * zq / r - 50.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn series_section_matches() {
        // a z1 section on z0 reaches loads up to SWR (z1/z0)²: 25+j20 has SWR 2.4,
        // beyond 75 ohm's 2.25 but within 93 ohm's 3.5
        assert!(series_section(C::new(25.0, 20.0), 50.0, 75.0).is_empty());
        for (zl, z1) in [(C::new(100.0, 0.0), 75.0), (C::new(25.0, 20.0), 93.0), (C::new(30.0, -15.0), 35.0)] {
            let sols = series_section(zl, 50.0, z1);
            assert!(!sols.is_empty(), "{zl}");
            for (th2, th1) in sols {
                // z0 line (θ2) from the load, then the z1 section (θ1)
                let z = line_in(line_in(zl, 50.0, th2.to_radians()), z1, th1.to_radians());
                assert!((z - C::new(50.0, 0.0)).norm() < 1e-6, "{zl} z1 {z1}: {th1} {th2} -> {z}");
            }
        }
    }
}

