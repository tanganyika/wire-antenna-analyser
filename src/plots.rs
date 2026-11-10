//! Hand-drawn plots: polar radiation patterns, x/y charts and the 3D wire view.

use crate::model::*;
use crate::mom::{FullResult, Gain};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

pub const PLOT_BG: Color32 = Color32::from_rgb(252, 252, 248);
pub const GRID: Color32 = Color32::from_rgb(200, 200, 205);
pub const INK: Color32 = Color32::from_rgb(40, 40, 50);
pub const TOTAL: Color32 = Color32::from_rgb(200, 30, 40);
pub const HORC: Color32 = Color32::from_rgb(30, 90, 200);
pub const VERC: Color32 = Color32::from_rgb(20, 150, 70);

#[derive(Clone, Copy, PartialEq)]
pub enum PolarKind {
    Azimuth,
    Elevation { ground: bool },
}

pub struct PolarShow {
    pub total: bool,
    pub hor: bool,
    pub ver: bool,
}

fn ang_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

fn db(x: f64) -> f64 {
    crate::mom::db(x)
}

/// The original's cursor readout never shows less than -100 dBi (MMFarField 0x406480).
pub const CURSOR_FLOOR_DB: f64 = -100.0;

fn cursor_db(v: f64) -> f64 {
    if v <= CURSOR_FLOOR_DB { CURSOR_FLOOR_DB } else { v }
}

/// Radius scales of the original 3D viewer (MMFarField menu "Scale": ARRL, dB, Linear;
/// handler 0x407708). Only the dB scale uses the "min of dB scale" setting.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scale3D {
    Arrl,
    Db,
    Linear,
}

impl Scale3D {
    pub fn label(&self) -> &'static str {
        match self {
            Scale3D::Arrl => "ARRL",
            Scale3D::Db => "dB",
            Scale3D::Linear => "Linear",
        }
    }
}

/// Scene units panned per window width / height by a right-drag (MMFarField 0x404f14,
/// constants at 0x4050c0 and 0x4050c8).
pub const PAN_X_UNITS: f32 = 2.8;
pub const PAN_Y_UNITS: f32 = 2.5;

/// Default "min of dB scale" (MMFarField DAT_004b7238).
pub const MIN_DB_DEFAULT: f64 = -40.0;

/// One click of the original's min-dB spinner (MMFarField 0x407a5c): steps of 10 dB below
/// -20 dB, 5 dB between -20 and -10, 1 dB above; kept between -100 and -1 dB.
pub fn step_min_db(v: f64, up: bool) -> f64 {
    let step = if up {
        if v >= -20.0 { if v >= -10.0 { 1.0 } else { 5.0 } } else { 10.0 }
    } else if v > -20.0 {
        if v > -10.0 { -1.0 } else { -5.0 }
    } else {
        -10.0
    };
    (v + step).clamp(-100.0, -1.0)
}

fn scale_radius(rel_db: f64, scale: Scale3D, min_db: f64) -> f32 {
    match scale {
        Scale3D::Arrl => radius_frac(rel_db),
        Scale3D::Db => ((rel_db - min_db) / -min_db).clamp(0.0, 1.0) as f32,
        Scale3D::Linear => 10f64.powf(rel_db / 20.0).clamp(0.0, 1.0) as f32,
    }
}

/// ARRL-style logarithmic radius for a gain `rel_db` (<= 0) below the maximum.
fn radius_frac(rel_db: f64) -> f32 {
    let r = 0.89f64.powf(-rel_db / 2.0);
    r.clamp(0.0, 1.0) as f32
}

/// Polar pattern with a movable marker: click or drag on the plot (or use the arrow keys while
/// the mouse is over it) to read the gain at any angle; the angle is kept in `cursor`.
#[allow(clippy::too_many_arguments)]
pub fn polar(
    ui: &mut egui::Ui,
    size: f32,
    title: &str,
    data: &[(f64, Gain)],
    kind: PolarKind,
    show: &PolarShow,
    info: &[String],
    cursor: &mut Option<f64>,
) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(size, size + 18.0 + 15.0 * info.len() as f32), Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, PLOT_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    painter.text(rect.left_top() + Vec2::new(8.0, 4.0), Align2::LEFT_TOP, title, FontId::proportional(14.0), INK);
    let c = Pos2::new(rect.center().x, rect.top() + 22.0 + (size - 26.0) / 2.0);
    let r0 = (size - 46.0) / 2.0;
    let small = FontId::proportional(10.0);
    let la = 70f32.to_radians();
    for (db, lab) in [(0.0, ""), (-3.0, "-3"), (-6.0, "-6"), (-10.0, "-10"), (-20.0, "-20"), (-30.0, "-30")] {
        let rr = r0 * radius_frac(db);
        painter.circle_stroke(c, rr, Stroke::new(if db == 0.0 { 1.2 } else { 0.6 }, GRID));
        painter.text(c + Vec2::new(la.cos(), -la.sin()) * rr, Align2::LEFT_BOTTOM, lab, small.clone(), Color32::GRAY);
    }
    for k in 0..12 {
        let a = (k as f32 * 30.0).to_radians();
        painter.line_segment([c, c + Vec2::new(a.cos(), -a.sin()) * r0], Stroke::new(0.5, GRID));
    }
    if let PolarKind::Elevation { ground: true } = kind {
        painter.line_segment([c - Vec2::new(r0, 0.0), c + Vec2::new(r0, 0.0)], Stroke::new(1.5, Color32::from_rgb(140, 110, 70)));
    }
    let labels: [(f32, &str); 4] = match kind {
        PolarKind::Azimuth => [(0.0, "0"), (90.0, "90"), (180.0, "180"), (270.0, "270")],
        PolarKind::Elevation { .. } => [(0.0, "front"), (90.0, "90"), (180.0, "back"), (270.0, "")],
    };
    for (a, l) in labels {
        let ar = a.to_radians();
        let p = c + Vec2::new(ar.cos(), -ar.sin()) * (r0 + 9.0);
        painter.text(p, Align2::CENTER_CENTER, l, small.clone(), Color32::DARK_GRAY);
    }
    let gmax = data.iter().map(|(_, g)| g.total).fold(0.0f64, f64::max);
    if gmax > 0.0 {
        let maxdb = 10.0 * gmax.log10();
        let draw = |sel: fn(&Gain) -> f64, col: Color32, w: f32| {
            let pts: Vec<Pos2> = data
                .iter()
                .map(|(a, g)| {
                    let v = sel(g);
                    let rel = if v > 0.0 { 10.0 * v.log10() - maxdb } else { -100.0 };
                    let rr = r0 * radius_frac(rel);
                    let ar = (*a as f32).to_radians();
                    c + Vec2::new(ar.cos(), -ar.sin()) * rr
                })
                .collect();
            painter.line(pts, Stroke::new(w, col));
        };
        if show.hor {
            draw(|g| g.hor, HORC, 1.2);
        }
        if show.ver {
            draw(|g| g.ver, VERC, 1.2);
        }
        if show.total {
            draw(|g| g.total, TOTAL, 2.0);
        }
    }
    // movable marker
    let max_angle = match kind {
        PolarKind::Elevation { ground: true } => 180.0,
        _ => 360.0,
    };
    if resp.is_pointer_button_down_on() || resp.clicked() || resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            let d = p - c;
            if d.length() > 4.0 && d.length() < r0 + 14.0 {
                let mut a = (-d.y as f64).atan2(d.x as f64).to_degrees();
                if a < 0.0 {
                    a += 360.0;
                }
                if max_angle < 360.0 && a > max_angle {
                    a = if a > 270.0 { 0.0 } else { max_angle };
                }
                *cursor = Some(a.round());
            }
        }
    }
    if resp.hovered() {
        let (l, r) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
        if l || r {
            let a = cursor.unwrap_or(0.0) + if l { 1.0 } else { -1.0 };
            *cursor = Some(if max_angle < 360.0 { a.clamp(0.0, max_angle) } else { a.rem_euclid(360.0) });
        }
    }
    if let (Some(a), true) = (*cursor, !data.is_empty() && gmax > 0.0) {
        let maxdb = 10.0 * gmax.log10();
        let (_, g) = data
            .iter()
            .min_by(|x, y| ang_dist(x.0, a).partial_cmp(&ang_dist(y.0, a)).unwrap())
            .unwrap();
        let ar = (a as f32).to_radians();
        let dir = Vec2::new(ar.cos(), -ar.sin());
        let mark = Color32::from_rgb(235, 140, 0);
        painter.line_segment([c, c + dir * (r0 + 4.0)], Stroke::new(1.2, mark));
        let gdb = db(g.total);
        let rr = r0 * radius_frac(gdb - maxdb);
        painter.circle_filled(c + dir * rr, 4.5, mark);
        painter.circle_stroke(c + dir * rr, 4.5, Stroke::new(1.0, Color32::WHITE));
        let label = match kind {
            PolarKind::Azimuth => format!("Azimuth {:.0}°", a),
            PolarKind::Elevation { .. } => {
                let (e, side) = if a <= 90.0 {
                    (a, "front")
                } else if a <= 270.0 {
                    (180.0 - a, "back")
                } else {
                    (a - 360.0, "front")
                };
                format!("Elevation {:.0}° {}", e, side)
            }
        };
        let mut lines = vec![label, format!("G = {:.1} dBi   ({:+.1} dB)", cursor_db(gdb), cursor_db(gdb) - maxdb)];
        if show.hor || show.ver {
            lines.push(format!("hor {:.1}  vert {:.1} dBi", cursor_db(db(g.hor)), cursor_db(db(g.ver))));
        }
        let x = rect.right() - 8.0;
        let mut yy = rect.top() + 4.0;
        for (i, l) in lines.iter().enumerate() {
            painter.text(Pos2::new(x, yy), Align2::RIGHT_TOP, l, FontId::proportional(if i == 1 { 13.5 } else { 12.0 }), if i == 1 { mark } else { INK });
            yy += 16.0;
        }
    } else if resp.hovered() {
        painter.text(rect.right_top() + Vec2::new(-8.0, 4.0), Align2::RIGHT_TOP, "click the plot to read the gain", FontId::proportional(11.0), Color32::GRAY);
    }
    let mut y = c.y + r0 + 18.0;
    for line in info {
        painter.text(Pos2::new(rect.left() + 8.0, y), Align2::LEFT_TOP, line, FontId::proportional(12.0), INK);
        y += 15.0;
    }
}

pub struct Series<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub pts: Vec<(f64, f64)>,
    pub right: bool,
}

fn nice_range(lo: f64, hi: f64) -> (f64, f64, f64) {
    let (mut lo, mut hi) = (lo, hi);
    // a flat series (equal to within rounding, e.g. F/B of a dipole) gets a ±1 range;
    // a step below the values' resolution would never advance the tick loops
    if !(hi - lo > 1e-9 * lo.abs().max(hi.abs()).max(1.0)) {
        lo -= 1.0;
        hi += 1.0;
    }
    let span = hi - lo;
    let raw = span / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    ((lo / step).floor() * step, (hi / step).ceil() * step, step)
}

/// Tick values a, a+step, … up to b, counted rather than accumulated (so a step
/// that is tiny next to the values can't stall), at most 200 of them.
fn ticks(a: f64, b: f64, step: f64) -> impl Iterator<Item = f64> {
    let n = if step > 0.0 && b >= a { (((b - a) / step) + 1e-9).floor().min(200.0) as usize } else { 0 };
    (0..=n).map(move |i| a + i as f64 * step)
}

/// Line chart with up to two y axes. `fixed` overrides the left axis range.
pub fn xy(ui: &mut egui::Ui, size: Vec2, title: &str, xlabel: &str, series: &[Series], fixed: Option<(f64, f64)>, marker: Option<f64>) {
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, PLOT_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    let plot = Rect::from_min_max(rect.min + Vec2::new(52.0, 24.0), rect.max - Vec2::new(52.0, 26.0));
    painter.text(rect.left_top() + Vec2::new(8.0, 4.0), Align2::LEFT_TOP, title, FontId::proportional(14.0), INK);
    let all: Vec<&(f64, f64)> = series.iter().flat_map(|s| s.pts.iter()).filter(|p| p.1.is_finite()).collect();
    if all.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, "No data yet", FontId::proportional(13.0), Color32::GRAY);
        return;
    }
    let x0 = all.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let x1 = all.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let range = |right: bool| -> Option<(f64, f64, f64)> {
        let v: Vec<f64> = series.iter().filter(|s| s.right == right).flat_map(|s| s.pts.iter().map(|p| p.1)).filter(|v| v.is_finite()).collect();
        if v.is_empty() {
            return None;
        }
        if !right {
            if let Some((a, b)) = fixed {
                let (_, _, st) = nice_range(a, b);
                return Some((a, b, st));
            }
        }
        Some(nice_range(v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max)))
    };
    let ly = range(false);
    let ry = range(true);
    let small = FontId::proportional(10.0);
    let xs = |x: f64| plot.left() + ((x - x0) / (x1 - x0).max(1e-12)) as f32 * plot.width();
    // x grid
    let (gx0, gx1, gstep) = nice_range(x0, x1);
    for gx in ticks(gx0, gx1, gstep) {
        if gx >= x0 - 1e-9 && gx <= x1 + 1e-9 {
            let x = xs(gx);
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(0.5, GRID));
            painter.text(Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP, format!("{}", trim(gx)), small.clone(), Color32::DARK_GRAY);
        }
    }
    painter.text(Pos2::new(plot.center().x, rect.bottom() - 3.0), Align2::CENTER_BOTTOM, xlabel, small.clone(), Color32::DARK_GRAY);
    if let Some((a, b, st)) = ly {
        for v in ticks(a, b, st) {
            let y = plot.bottom() - ((v - a) / (b - a)) as f32 * plot.height();
            painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(0.5, GRID));
            painter.text(Pos2::new(plot.left() - 4.0, y), Align2::RIGHT_CENTER, trim(v), small.clone(), Color32::DARK_GRAY);
        }
    }
    if let Some((a, b, st)) = ry {
        for v in ticks(a, b, st) {
            let y = plot.bottom() - ((v - a) / (b - a)) as f32 * plot.height();
            painter.text(Pos2::new(plot.right() + 4.0, y), Align2::LEFT_CENTER, trim(v), small.clone(), Color32::DARK_GRAY);
        }
    }
    painter.rect_stroke(plot, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Middle);
    if let Some(m) = marker {
        if m >= x0 && m <= x1 {
            let x = xs(m);
            painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, Color32::from_rgb(230, 150, 0)));
        }
    }
    let clip = painter.with_clip_rect(plot.expand(1.0));
    let legend_w: f32 = series.iter().map(|s| 9.0 + (s.name.len() + if s.right { 8 } else { 0 }) as f32 * 6.0).sum();
    let mut lx = (plot.right() - legend_w).max(plot.left() + 160.0);
    for s in series {
        let Some((a, b, _)) = (if s.right { ry } else { ly }) else { continue };
        let pts: Vec<Pos2> = s
            .pts
            .iter()
            .filter(|p| p.1.is_finite())
            .map(|p| Pos2::new(xs(p.0), plot.bottom() - ((p.1.clamp(a - (b - a), b + (b - a)) - a) / (b - a)) as f32 * plot.height()))
            .collect();
        clip.line(pts, Stroke::new(1.8, s.color));
        let label = format!("{}{}", s.name, if s.right { " (right)" } else { "" });
        painter.text(Pos2::new(lx, plot.top() - 4.0), Align2::LEFT_BOTTOM, &label, FontId::proportional(11.0), s.color);
        lx += 9.0 + label.len() as f32 * 6.0;
    }
}

fn trim(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

// ------------------------------------------------------------------ 3D view

pub struct ViewState {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
    pub show_segments: bool,
    pub show_current: bool,
    pub show_labels: bool,
    /// Point the 3D view is centred on after clicking part of the antenna (None = whole antenna).
    pub focus: Option<[f64; 3]>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState { yaw: -0.6, pitch: 0.45, zoom: 1.0, pan: Vec2::ZERO, show_segments: true, show_current: true, show_labels: true, focus: None }
    }
}

fn heat(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let r = (255.0 * (t * 2.0).min(1.0)) as u8;
    let g = (255.0 * (1.0 - (t - 0.5).abs() * 2.0).max(0.0) * 0.85) as u8;
    let b = (255.0 * (1.0 - t * 2.0).max(0.0)) as u8;
    Color32::from_rgb(r, g, b)
}

/// The 3D view.  Returns the wire that was right-clicked, if any (to edit it).
pub fn view3d(ui: &mut egui::Ui, size: Vec2, ant: &Antenna, calc: Option<&FullResult>, st: &mut ViewState) -> Option<usize> {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, Color32::from_rgb(246, 248, 252));
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        st.yaw += d.x * 0.01;
        st.pitch = (st.pitch + d.y * 0.01).clamp(-1.55, 1.55);
    }
    if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
        st.pan += resp.drag_delta();
    }
    if resp.double_clicked() {
        *st = ViewState { show_segments: st.show_segments, show_current: st.show_current, show_labels: st.show_labels, ..Default::default() };
    }
    let dz = if ant.ground.kind != GroundKind::Free { ant.ground.add_height } else { 0.0 };
    let pts: Vec<[f64; 3]> = ant.wires.iter().flat_map(|w| [w.p1, w.p2]).map(|p| [p[0], p[1], p[2] + dz]).collect();
    if pts.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, "No wires", FontId::proportional(14.0), Color32::GRAY);
        return None;
    }
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in &pts {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    if dz != 0.0 || ant.ground.kind != GroundKind::Free {
        lo[2] = lo[2].min(0.0);
    }
    let ctr = st.focus.unwrap_or([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]);
    let ext = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt().max(1e-3);
    let sc = (size.x.min(size.y) * 0.8) as f64 / ext * st.zoom as f64;
    let (cy, sy, cp, sp) = (st.yaw.cos() as f64, st.yaw.sin() as f64, st.pitch.cos() as f64, st.pitch.sin() as f64);
    let origin = rect.center() + st.pan;
    let proj = |p: [f64; 3]| -> Pos2 {
        let (x, y, z) = (p[0] - ctr[0], p[1] - ctr[1], p[2] - ctr[2]);
        let x1 = x * cy - y * sy;
        let y1 = x * sy + y * cy;
        let sx = x1;
        let syv = z * cp - y1 * sp;
        origin + Vec2::new((sx * sc) as f32, (-syv * sc) as f32)
    };
    // ---- zoom into a part of the antenna --------------------------------------------
    let wire_world = |w: &Wire| -> ([f64; 3], [f64; 3]) { ([w.p1[0], w.p1[1], w.p1[2] + dz], [w.p2[0], w.p2[1], w.p2[2] + dz]) };
    // nearest wire point under the mouse: (wire index, world point, distance in pixels)
    let pick = |m: Pos2| -> Option<(usize, [f64; 3], f32)> {
        let mut best: Option<(usize, [f64; 3], f32)> = None;
        for (i, w) in ant.wires.iter().enumerate() {
            let (a, b) = wire_world(w);
            let (pa, pb) = (proj(a), proj(b));
            let ab = pb - pa;
            let l2 = ab.length_sq();
            let t = if l2 > 0.0 { ((m - pa).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (pa + ab * t - m).length();
            if best.map(|bb| d < bb.2).unwrap_or(true) {
                let tw = t as f64;
                best = Some((i, [a[0] + (b[0] - a[0]) * tw, a[1] + (b[1] - a[1]) * tw, a[2] + (b[2] - a[2]) * tw], d));
            }
        }
        best
    };
    let hover = resp.hover_pos().and_then(|m| pick(m)).filter(|h| h.2 < 14.0);
    if resp.clicked() && !ui.input(|i| i.modifiers.shift) {
        if let Some((_, p, d)) = resp.interact_pointer_pos().and_then(|m| pick(m)) {
            if d < 25.0 {
                st.focus = Some(p);
                st.zoom = (st.zoom * 3.0).min(5000.0);
                st.pan = Vec2::ZERO;
                ui.ctx().request_repaint();
            }
        }
    }
    // right-click on a wire: edit it; elsewhere (or shift-click): zoom out
    let mut edit = None;
    if resp.secondary_clicked() {
        edit = resp.interact_pointer_pos().and_then(|m| pick(m)).filter(|h| h.2 < 14.0).map(|h| h.0);
    }
    if (resp.secondary_clicked() && edit.is_none()) || (resp.clicked() && ui.input(|i| i.modifiers.shift)) {
        st.zoom = (st.zoom / 3.0).max(0.1);
        if st.zoom <= 1.0 {
            st.focus = None;
            st.zoom = st.zoom.max(1.0);
        }
        st.pan = Vec2::ZERO;
        ui.ctx().request_repaint();
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            // zoom toward the mouse: the point under it stays put
            let f = (1.0 + s * 0.002).max(0.1);
            let nz = (st.zoom * f).clamp(0.1, 5000.0);
            let f = nz / st.zoom;
            if let Some(m) = resp.hover_pos() {
                let o2 = m - (m - origin) * f;
                st.pan = o2 - rect.center();
            }
            st.zoom = nz;
            ui.ctx().request_repaint();
        }
    }
    // ground grid
    if ant.ground.kind != GroundKind::Free {
        let g = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.75).max(ext * 0.25);
        let n = 8;
        for i in 0..=n {
            let t = -g + 2.0 * g * i as f64 / n as f64;
            let col = Stroke::new(0.6, Color32::from_rgb(190, 170, 130));
            painter.line_segment([proj([ctr[0] + t, ctr[1] - g, 0.0]), proj([ctr[0] + t, ctr[1] + g, 0.0])], col);
            painter.line_segment([proj([ctr[0] - g, ctr[1] + t, 0.0]), proj([ctr[0] + g, ctr[1] + t, 0.0])], col);
        }
    }
    // axes
    let al = ext * 0.15;
    let o = [ctr[0] - (hi[0] - lo[0]) / 2.0 - al * 0.5, ctr[1] - (hi[1] - lo[1]) / 2.0 - al * 0.5, lo[2]];
    for (i, (col, name)) in [(Color32::from_rgb(210, 40, 40), "X"), (Color32::from_rgb(30, 150, 50), "Y"), (Color32::from_rgb(40, 70, 220), "Z")].into_iter().enumerate() {
        let mut e = o;
        e[i] += al;
        painter.line_segment([proj(o), proj(e)], Stroke::new(1.5, col));
        painter.text(proj(e), Align2::CENTER_CENTER, name, FontId::proportional(11.0), col);
    }
    // current magnitude per half segment
    let mut half_i: Vec<[f64; 2]> = Vec::new();
    let mut imax = 0.0f64;
    let calc_ok = calc.filter(|c| c.mesh.wire_segs.len() == ant.wires.len());
    if let (true, Some(c)) = (st.show_current, calc_ok) {
        let mut acc = vec![[num_complex::Complex64::new(0.0, 0.0); 2]; c.mesh.segs.len()];
        for (b, i) in c.mesh.bases.iter().zip(c.currents.iter()) {
            for p in &b.parts {
                acc[p.seg][p.half as usize] += *i * p.dir;
            }
        }
        half_i = acc.iter().map(|a| [a[0].norm(), a[1].norm()]).collect();
        imax = half_i.iter().flat_map(|a| a.iter().cloned()).fold(0.0, f64::max);
    }
    let wire_col = Color32::from_rgb(30, 50, 110);
    // currents from an external engine (NEC-2): one magnitude per segment
    let ext = calc_ok.and_then(|c| c.seg_currents.as_ref()).filter(|_| st.show_current);
    let ext_max = ext.map(|v| v.iter().map(|s| s.2).fold(0.0, f64::max)).unwrap_or(0.0);
    if let (Some(v), true) = (ext, ext_max > 0.0) {
        imax = ext_max;
        for (a, b, i) in v {
            painter.line_segment([proj(*a), proj(*b)], Stroke::new(3.0, heat((i / ext_max) as f32)));
        }
        if st.show_segments {
            for (a, _, _) in v {
                painter.circle_filled(proj(*a), 1.6, Color32::from_rgb(90, 90, 100));
            }
        }
    } else if let (true, Some(c)) = (imax > 0.0, calc_ok) {
        for (si, s) in c.mesh.segs.iter().enumerate() {
            let m = [(s.a[0] + s.b[0]) / 2.0, (s.a[1] + s.b[1]) / 2.0, (s.a[2] + s.b[2]) / 2.0];
            painter.line_segment([proj(s.a), proj(m)], Stroke::new(3.0, heat((half_i[si][0] / imax) as f32)));
            painter.line_segment([proj(m), proj(s.b)], Stroke::new(3.0, heat((half_i[si][1] / imax) as f32)));
        }
    } else {
        for w in &ant.wires {
            let a = [w.p1[0], w.p1[1], w.p1[2] + dz];
            let b = [w.p2[0], w.p2[1], w.p2[2] + dz];
            painter.line_segment([proj(a), proj(b)], Stroke::new(2.0, wire_col));
        }
    }
    if st.show_segments {
        if let Some(c) = calc_ok {
            for s in &c.mesh.segs {
                painter.circle_filled(proj(s.a), 1.6, Color32::from_rgb(90, 90, 100));
            }
        }
    }
    let at = |w: &Wire, f: f64| -> [f64; 3] {
        [w.p1[0] + (w.p2[0] - w.p1[0]) * f, w.p1[1] + (w.p2[1] - w.p1[1]) * f, w.p1[2] + (w.p2[2] - w.p1[2]) * f + dz]
    };
    if st.show_labels {
        for (i, w) in ant.wires.iter().enumerate() {
            painter.text(proj(at(w, 0.5)) + Vec2::new(4.0, -4.0), Align2::LEFT_BOTTOM, format!("{}", i + 1), FontId::proportional(10.0), Color32::from_rgb(90, 90, 120));
        }
    }
    // plates: a translucent fill with their outline
    for p in &ant.plates {
        let c = p.corners().map(|v| proj([v[0], v[1], v[2] + dz]));
        painter.add(egui::Shape::convex_polygon(c.to_vec(), Color32::from_rgba_unmultiplied(120, 150, 200, 60), Stroke::new(1.2, Color32::from_rgb(70, 100, 160))));
    }
    // transmission lines: a dashed link between their ends
    for t in &ant.lines {
        if let (Some(wa), Some(wb)) = (ant.wires.get(t.a_wire), ant.wires.get(t.b_wire)) {
            let (p, q) = (proj(at(wa, t.a_pos)), proj(at(wb, t.b_pos)));
            let n = ((q - p).length() / 6.0).ceil().max(1.0) as usize;
            for k in (0..n).step_by(2) {
                let (u, v) = (k as f32 / n as f32, ((k + 1) as f32 / n as f32).min(1.0));
                painter.line_segment([p + (q - p) * u, p + (q - p) * v], Stroke::new(1.5, Color32::from_rgb(200, 120, 20)));
            }
        }
    }
    for l in &ant.loads {
        if let Some(w) = ant.wires.get(l.wire) {
            let p = proj(at(w, l.pos));
            painter.rect_filled(Rect::from_center_size(p, Vec2::splat(8.0)), 1.0, Color32::from_rgb(20, 160, 60));
        }
    }
    for s in &ant.sources {
        if let Some(w) = ant.wires.get(s.wire) {
            let p = proj(at(w, s.pos));
            painter.circle_filled(p, 5.0, Color32::from_rgb(220, 30, 30));
            painter.circle_stroke(p, 7.0, Stroke::new(1.0, Color32::from_rgb(220, 30, 30)));
        }
    }
    if let Some((i, _, _)) = hover {
        let w = &ant.wires[i];
        let (a, b) = wire_world(w);
        painter.line_segment([proj(a), proj(b)], Stroke::new(5.0, Color32::from_rgba_unmultiplied(235, 140, 0, 140)));
        let loads: Vec<String> = ant
            .loads
            .iter()
            .filter(|l| l.wire == i)
            .map(|l| match l.kind {
                LoadKind::Rx { r, x } => format!("load {:.1}{:+.1}j Ω", r, x),
                LoadKind::Lc { l_uh, c_pf, q } => format!("load L {} µH C {} pF Q {}", l_uh, c_pf, q),
                LoadKind::Rlc { r, l_uh, c_pf, parallel } => format!("load R {} L {} µH C {} pF {}", r, l_uh, c_pf, if parallel { "parallel" } else { "series" }),
            })
            .collect();
        let mut txt = format!("Wire {}   length {:.4} m   R {:.2} mm   {}", i + 1, w.length(), w.r * 1000.0, if w.seg > 0 { format!("{} seg", w.seg) } else { "auto seg".into() });
        if w.insulated() {
            txt += &format!("   insulation {:.2} mm εr {} (VF {:.3})", w.ins_t * 1000.0, w.ins_eps, w.velocity_factor());
        }
        if !loads.is_empty() {
            txt += &format!("\n{}", loads.join(", "));
        }
        txt += "\nclick: zoom in here   right-click: edit this wire";
        painter.text(rect.left_top() + Vec2::new(10.0, 8.0), Align2::LEFT_TOP, txt, FontId::proportional(12.0), INK);
    }
    // scale bar
    {
        let target = 120.0f64 / sc; // metres in ~120 px
        let mag = 10f64.powf(target.log10().floor());
        let len_m = [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * mag).filter(|v| *v <= target).last().unwrap_or(mag);
        let px = (len_m * sc) as f32;
        let y = rect.bottom() - 32.0;
        let x0 = rect.right() - 150.0 - px;
        painter.line_segment([Pos2::new(x0, y), Pos2::new(x0 + px, y)], Stroke::new(2.0, INK));
        painter.line_segment([Pos2::new(x0, y - 4.0), Pos2::new(x0, y + 4.0)], Stroke::new(1.5, INK));
        painter.line_segment([Pos2::new(x0 + px, y - 4.0), Pos2::new(x0 + px, y + 4.0)], Stroke::new(1.5, INK));
        let label = if len_m >= 1.0 { format!("{} m", len_m) } else if len_m >= 0.01 { format!("{} cm", (len_m * 100.0 * 1000.0).round() / 1000.0) } else { format!("{} mm", (len_m * 1000.0 * 1000.0).round() / 1000.0) };
        painter.text(Pos2::new(x0 + px / 2.0, y - 6.0), Align2::CENTER_BOTTOM, label, FontId::proportional(11.0), INK);
    }
    let hint = "click a wire: zoom in there   right-click a wire: edit it   right-click elsewhere: zoom out   drag: rotate   right-drag or scroll bars: move   wheel: zoom at mouse   double-click: whole antenna";
    painter.text(rect.left_bottom() + Vec2::new(8.0, -6.0), Align2::LEFT_BOTTOM, hint, FontId::proportional(10.0), Color32::GRAY);
    if imax > 0.0 {
        let x = rect.right() - 130.0;
        let y = rect.bottom() - 16.0;
        for k in 0..100 {
            let t = k as f32 / 99.0;
            painter.line_segment([Pos2::new(x + k as f32, y), Pos2::new(x + k as f32, y - 8.0)], Stroke::new(1.0, heat(t)));
        }
        painter.text(Pos2::new(x, y - 10.0), Align2::LEFT_BOTTOM, "current: low → high", FontId::proportional(10.0), Color32::DARK_GRAY);
    }
    edit
}

// ------------------------------------------------------------------ 3D pattern

/// Solid 3D radiation pattern (log radius like the polar plots), coloured by gain and shaded.
/// Drag to rotate, wheel to zoom, double-click to reset; hovering shows the gain under the mouse.
pub fn pattern3d(ui: &mut egui::Ui, size: Vec2, p: &crate::mom::Patterns, st: &mut ViewState, show_mesh: bool, rscale: Scale3D, min_db: f64) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, Color32::from_rgb(246, 248, 252));
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, GRID), StrokeKind::Inside);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        st.yaw += d.x * 0.01;
        st.pitch = (st.pitch + d.y * 0.01).clamp(-1.55, 1.55);
    }
    if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
        // right-drag pan as in the original 3D viewer (MMFarField 0x404f14): the scene moves by
        // Δx·2.8/width and Δy·2.5/height scene units, which are then drawn at the current scale
        let d = resp.drag_delta();
        let px_per_unit = size.x.min(size.y) * 0.42 * st.zoom;
        st.pan += Vec2::new(d.x * PAN_X_UNITS / rect.width() * px_per_unit, d.y * PAN_Y_UNITS / rect.height() * px_per_unit);
    }
    if resp.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            st.zoom = (st.zoom * (1.0 + s * 0.002)).clamp(0.2, 20.0);
        }
    }
    if resp.double_clicked() {
        st.yaw = -0.6;
        st.pitch = 0.45;
        st.zoom = 1.0;
        st.pan = Vec2::ZERO;
    }
    let n_el = p.grid.len();
    if n_el < 2 {
        painter.text(rect.center(), Align2::CENTER_CENTER, "No pattern yet — press Calculate", FontId::proportional(14.0), Color32::GRAY);
        return;
    }
    let n_az = p.grid[0].len();
    let gmax = p.grid.iter().flatten().cloned().fold(0.0f64, f64::max);
    if gmax <= 0.0 {
        return;
    }
    let maxdb = db(gmax);
    let floor = if rscale == Scale3D::Db { min_db } else { -40.0 };
    let rel = |g: f64| (db(g) - maxdb).max(floor);
    let scale = size.x.min(size.y) * 0.42 * st.zoom;
    let (cy, sy, cp, sp) = (st.yaw.cos(), st.yaw.sin(), st.pitch.cos(), st.pitch.sin());
    let origin = rect.center() + st.pan;
    // camera space: x right, y up, z away from the viewer
    let cam = |v: [f32; 3]| -> [f32; 3] {
        let x1 = v[0] * cy - v[1] * sy;
        let y1 = v[0] * sy + v[1] * cy;
        [x1, v[2] * cp - y1 * sp, v[2] * sp + y1 * cp]
    };
    let to_screen = |c: [f32; 3]| origin + Vec2::new(c[0] * scale, -c[1] * scale);
    let point = |i: usize, j: usize| -> ([f32; 3], f64) {
        let g = p.grid[i][j % n_az];
        let r = scale_radius(rel(g), rscale, min_db);
        let e = (p.grid_els[i] as f32).to_radians();
        let a = ((j % n_az) as f32 * p.grid_step as f32).to_radians();
        ([r * e.cos() * a.cos(), r * e.cos() * a.sin(), r * e.sin()], rel(g))
    };
    // ground disc / axes behind the pattern
    if !p.free_space {
        let ring: Vec<Pos2> = (0..=72).map(|k| {
            let a = (k as f32 * 5.0).to_radians();
            to_screen(cam([1.05 * a.cos(), 1.05 * a.sin(), 0.0]))
        }).collect();
        painter.add(egui::Shape::convex_polygon(ring.clone(), Color32::from_rgba_unmultiplied(200, 180, 140, 60), Stroke::NONE));
        painter.line(ring, Stroke::new(1.0, Color32::from_rgb(170, 140, 90)));
    }
    for (i, (col, name)) in [(Color32::from_rgb(210, 40, 40), "X"), (Color32::from_rgb(30, 150, 50), "Y"), (Color32::from_rgb(40, 70, 220), "Z")].into_iter().enumerate() {
        let mut e = [0.0f32; 3];
        e[i] = 1.2;
        painter.line_segment([to_screen(cam([0.0; 3])), to_screen(cam(e))], Stroke::new(1.2, col));
        painter.text(to_screen(cam(e)), Align2::CENTER_CENTER, name, FontId::proportional(12.0), col);
    }
    // triangles, far to near
    let mut tris: Vec<(f32, [[f32; 3]; 3], [f64; 3])> = Vec::with_capacity(n_el * n_az * 2);
    for i in 0..n_el - 1 {
        for j in 0..n_az {
            let (a, ga) = point(i, j);
            let (b, gb) = point(i, j + 1);
            let (c, gc) = point(i + 1, j + 1);
            let (d, gd) = point(i + 1, j);
            for (t, g) in [([a, b, c], [ga, gb, gc]), ([a, c, d], [ga, gc, gd])] {
                let ct = [cam(t[0]), cam(t[1]), cam(t[2])];
                let depth = (ct[0][2] + ct[1][2] + ct[2][2]) / 3.0;
                tris.push((depth, ct, g));
            }
        }
    }
    tris.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut mesh = Mesh::default();
    let mut lines = Vec::new();
    for (_, t, g) in &tris {
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let light = if nl > 0.0 { 0.55 + 0.45 * (n[2].abs() / nl) } else { 0.8 };
        let base = mesh.vertices.len() as u32;
        for k in 0..3 {
            let col = heat(((g[k] - floor) / -floor) as f32);
            let shade = |c: u8| (c as f32 * light) as u8;
            mesh.vertices.push(Vertex { pos: to_screen(t[k]), uv: WHITE_UV, color: Color32::from_rgb(shade(col.r()), shade(col.g()), shade(col.b())) });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
        if show_mesh {
            lines.push([to_screen(t[0]), to_screen(t[1])]);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    for l in lines {
        painter.line_segment(l, Stroke::new(0.3, Color32::from_rgba_unmultiplied(40, 40, 40, 60)));
    }
    // hover readout: nearest grid direction under the mouse (front-most)
    if let Some(m) = resp.hover_pos() {
        let mut best: Option<(f32, usize, usize)> = None;
        for i in 0..n_el {
            for j in 0..n_az {
                let (v, _) = point(i, j);
                let cv = cam(v);
                let sp_ = to_screen(cv);
                if (sp_ - m).length() < 10.0 && best.map(|b| cv[2] < b.0).unwrap_or(true) {
                    best = Some((cv[2], i, j));
                }
            }
        }
        if let Some((_, i, j)) = best {
            let (v, _) = point(i, j);
            let g = p.grid[i][j];
            painter.circle_filled(to_screen(cam(v)), 4.0, Color32::from_rgb(235, 140, 0));
            painter.text(
                rect.right_top() + Vec2::new(-10.0, 8.0),
                Align2::RIGHT_TOP,
                format!("Az angle = {:.0}°,  Ev angle = {:.0}°\nG = {:.1} dBi  ({:+.1} dB)", j as f64 * p.grid_step, p.grid_els[i], cursor_db(db(g)), cursor_db(db(g)) - maxdb),
                FontId::proportional(13.0),
                INK,
            );
        }
    }
    // legend
    let x = rect.left() + 12.0;
    let y = rect.bottom() - 22.0;
    for k in 0..120 {
        let t = k as f32 / 119.0;
        painter.line_segment([Pos2::new(x + k as f32, y), Pos2::new(x + k as f32, y - 9.0)], Stroke::new(1.0, heat(t)));
    }
    painter.text(Pos2::new(x, y - 11.0), Align2::LEFT_BOTTOM, format!("{:.0} dB … max {:.1} dBi  ({} scale)", floor, maxdb, rscale.label()), FontId::proportional(11.0), Color32::DARK_GRAY);
    painter.text(rect.left_bottom() + Vec2::new(140.0, -6.0), Align2::LEFT_BOTTOM, "drag: rotate   right-drag: move   wheel: zoom   double-click: reset   hover: read gain", FontId::proportional(10.0), Color32::GRAY);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flat_and_near_flat_ranges_have_few_ticks() {
        for (lo, hi) in [(0.0, 0.0), (14.0, 14.0 + 1e-15), (-3e-16, 4e-16), (1e9, 1e9 + 1e-6), (1.0, 2.0)] {
            let (a, b, st) = nice_range(lo, hi);
            let n = ticks(a, b, st).count();
            assert!((2..=12).contains(&n), "{lo}..{hi}: {n} ticks");
            assert!(a + st > a, "{lo}..{hi}: step {st} does not advance");
        }
    }

    #[test]
    fn min_db_steps_like_the_original() {
        assert_eq!(step_min_db(-40.0, true), -30.0);
        assert_eq!(step_min_db(-20.0, true), -15.0);
        assert_eq!(step_min_db(-10.0, true), -9.0);
        assert_eq!(step_min_db(-2.0, true), -1.0);
        assert_eq!(step_min_db(-1.0, true), -1.0);
        assert_eq!(step_min_db(-15.0, false), -20.0);
        assert_eq!(step_min_db(-5.0, false), -6.0);
        assert_eq!(step_min_db(-95.0, false), -100.0);
    }
}
