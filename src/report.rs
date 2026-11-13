//! The antenna report (Tools → Antenna report): the antenna, the Calculate tab's
//! results and patterns, the resonance finder's and the match calculator's
//! results, as a PDF.

use crate::model::*;
use crate::mom::{CalcResult, FullResult, Gain};
use crate::pdf::{Font, PAGE_H, PAGE_W, Pdf, text_width, wrap};
use crate::tools::{self, ToolsState};
use num_complex::Complex64 as C;
use std::path::Path;

const M: f64 = 42.0; // margin
const BOTTOM: f64 = PAGE_H - 46.0;
const INK: [f64; 3] = [0.12, 0.12, 0.16];
const GREY: [f64; 3] = [0.45, 0.45, 0.5];
const LIGHT: [f64; 3] = [0.8, 0.8, 0.83];
const ACCENT: [f64; 3] = [0.10, 0.27, 0.55];
const TOTAL: [f64; 3] = [0.78, 0.12, 0.16];
const HOR: [f64; 3] = [0.12, 0.35, 0.78];
const VER: [f64; 3] = [0.08, 0.59, 0.27];

struct Doc {
    pdf: Pdf,
    y: f64,
    title: String,
    stamp: String,
}

impl Doc {
    fn footer(&mut self) {
        let n = self.pdf.page_count();
        let t = format!("{} — {}", self.title, self.stamp);
        self.pdf.line((M, PAGE_H - 34.0), (PAGE_W - M, PAGE_H - 34.0), 0.4, LIGHT);
        self.pdf.text(M, PAGE_H - 22.0, 8.0, Font::Regular, GREY, &t);
        self.pdf.text_right(PAGE_W - M, PAGE_H - 22.0, 8.0, Font::Regular, GREY, &format!("page {n}"));
    }

    fn new_page(&mut self) {
        self.footer();
        self.pdf.new_page();
        self.y = M + 10.0;
    }

    /// Makes room for `h` points, starting a page if needed.
    fn need(&mut self, h: f64) {
        if self.y + h > BOTTOM {
            self.new_page();
        }
    }

    fn heading(&mut self, s: &str) {
        self.need(60.0);
        self.y += 10.0;
        self.pdf.text(M, self.y + 12.0, 14.0, Font::Bold, ACCENT, s);
        self.y += 18.0;
        self.pdf.line((M, self.y), (PAGE_W - M, self.y), 0.8, ACCENT);
        self.y += 10.0;
    }

    fn sub(&mut self, s: &str) {
        self.need(40.0);
        self.y += 4.0;
        self.pdf.text(M, self.y + 10.0, 11.0, Font::Bold, INK, s);
        self.y += 16.0;
    }

    fn para(&mut self, s: &str, size: f64, rgb: [f64; 3]) {
        for line in wrap(s, size, PAGE_W - 2.0 * M) {
            self.need(size * 1.35);
            self.pdf.text(M, self.y + size, size, Font::Regular, rgb, &line);
            self.y += size * 1.35;
        }
    }

    fn bullet(&mut self, s: &str) {
        let size = 9.5;
        for (i, line) in wrap(s, size, PAGE_W - 2.0 * M - 12.0).into_iter().enumerate() {
            self.need(size * 1.35);
            if i == 0 {
                self.pdf.text(M + 2.0, self.y + size, size, Font::Regular, INK, "•");
            }
            self.pdf.text(M + 12.0, self.y + size, size, Font::Regular, INK, &line);
            self.y += size * 1.35;
        }
    }

    /// Label / value pairs in two columns.
    fn pairs(&mut self, kv: &[(String, String)]) {
        let size = 9.5;
        let lw = kv.iter().map(|(k, _)| text_width(k, size, Font::Bold)).fold(0.0, f64::max) + 12.0;
        for (k, v) in kv {
            let lines = wrap(v, size, PAGE_W - 2.0 * M - lw);
            self.need(size * 1.4 * lines.len() as f64);
            self.pdf.text(M, self.y + size, size, Font::Bold, INK, k);
            for l in lines {
                self.pdf.text(M + lw, self.y + size, size, Font::Regular, INK, &l);
                self.y += size * 1.4;
            }
        }
        self.y += 4.0;
    }

    /// A table: column widths are fitted to the contents and scaled to the page;
    /// numbers are right-aligned.
    fn table(&mut self, head: &[&str], rows: &[Vec<String>], size: f64) {
        let n = head.len();
        let mut w: Vec<f64> = head.iter().map(|h| text_width(h, size, Font::Bold)).collect();
        for r in rows {
            for (i, c) in r.iter().enumerate().take(n) {
                w[i] = w[i].max(text_width(c, size, Font::Regular));
            }
        }
        let pad = 8.0;
        let total: f64 = w.iter().map(|x| x + pad).sum();
        let avail = PAGE_W - 2.0 * M;
        let k = if total > avail { avail / total } else { 1.0 };
        let size = if k < 1.0 { (size * k).max(6.0) } else { size };
        let w: Vec<f64> = w.iter().map(|x| (x + pad) * k).collect();
        let rh = size * 1.55;
        let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || "+-−.,% ".contains(c));
        let draw_head = |d: &mut Doc| {
            d.pdf.fill_rect(M, d.y, w.iter().sum(), rh, [0.91, 0.93, 0.97]);
            let mut x = M;
            for (i, h) in head.iter().enumerate() {
                d.pdf.text(x + 3.0, d.y + rh - size * 0.45, size, Font::Bold, INK, h);
                x += w[i];
            }
            d.y += rh;
        };
        self.need(rh * 3.0);
        draw_head(self);
        for (ri, r) in rows.iter().enumerate() {
            if self.y + rh > BOTTOM {
                self.new_page();
                draw_head(self);
            }
            if ri % 2 == 1 {
                self.pdf.fill_rect(M, self.y, w.iter().sum(), rh, [0.97, 0.97, 0.98]);
            }
            let mut x = M;
            for (i, c) in r.iter().enumerate().take(n) {
                if numeric(c) {
                    self.pdf.text_right(x + w[i] - 3.0, self.y + rh - size * 0.45, size, Font::Regular, INK, c);
                } else {
                    self.pdf.text(x + 3.0, self.y + rh - size * 0.45, size, Font::Regular, INK, c);
                }
                x += w[i];
            }
            self.y += rh;
        }
        self.y += 6.0;
    }
}

/// UTC date and time now, "YYYY-MM-DD HH:MM UTC".
fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days (H. Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

fn z_text(z: C) -> String {
    format!("{:.2} {} j{:.2} Ω", z.re, if z.im < 0.0 { "−" } else { "+" }, z.im.abs())
}

fn f(v: f64, d: usize) -> String {
    let s = format!("{v:.d$}");
    if s.starts_with('-') { format!("−{}", &s[1..]) } else { s }
}

fn load_text(k: &LoadKind) -> String {
    match *k {
        LoadKind::Rx { r, x } => format!("{} {} j{} Ω", f(r, 2), if x < 0.0 { "−" } else { "+" }, f(x.abs(), 2)),
        LoadKind::Lc { l_uh, c_pf, q } => format!("L {l_uh} µH, C {c_pf} pF, Q {q}"),
        LoadKind::Rlc { r, l_uh, c_pf, parallel } => format!("R {r} Ω, L {l_uh} µH, C {c_pf} pF ({})", if parallel { "parallel" } else { "series" }),
    }
}

/// Polar pattern like the app's: rings at 0, −3, −6, −10, −20, −30 dB below the
/// maximum (ARRL log scale), total (red), horizontal (blue), vertical (green).
fn polar(pdf: &mut Pdf, cx: f64, cy: f64, r0: f64, data: &[(f64, Gain)], elevation: Option<bool>, title: &str, info: &[String]) {
    pdf.text_centre(cx, cy - r0 - 26.0, 10.5, Font::Bold, INK, title);
    let half = elevation == Some(true);
    for (db, lab) in [(0.0, ""), (-3.0, "−3"), (-6.0, "−6"), (-10.0, "−10"), (-20.0, "−20"), (-30.0, "−30")] {
        let rr = r0 * crate::plots::radius_frac(db) as f64;
        let n = 96;
        let (a0, a1) = if half { (0.0, 180.0f64) } else { (0.0, 360.0) };
        let pts: Vec<(f64, f64)> = (0..=n).map(|i| {
            let a = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
            (cx + rr * a.cos(), cy - rr * a.sin())
        }).collect();
        pdf.polyline(&pts, if db == 0.0 { 0.8 } else { 0.4 }, LIGHT, false);
        if !lab.is_empty() {
            let a = 70f64.to_radians();
            pdf.text(cx + rr * a.cos() + 1.0, cy - rr * a.sin() - 1.0, 6.5, Font::Regular, GREY, lab);
        }
    }
    let spokes = if half { 7 } else { 12 };
    for k in 0..spokes {
        let a = (k as f64 * 30.0).to_radians();
        pdf.line((cx, cy), (cx + r0 * a.cos(), cy - r0 * a.sin()), 0.3, LIGHT);
    }
    if half {
        pdf.line((cx - r0, cy), (cx + r0, cy), 0.6, GREY);
    }
    let labels: Vec<(f64, &str)> = match elevation {
        None => vec![(0.0, "0°"), (90.0, "90°"), (180.0, "180°"), (270.0, "270°")],
        Some(true) => vec![(0.0, "front"), (90.0, "90°"), (180.0, "back")],
        Some(false) => vec![(0.0, "front"), (90.0, "90°"), (180.0, "back"), (270.0, "−90°")],
    };
    for (a, l) in labels {
        let ar = a.to_radians();
        let (x, y) = (cx + (r0 + 9.0) * ar.cos(), cy - (r0 + 9.0) * ar.sin() + 3.0);
        pdf.text_centre(x, y, 7.5, Font::Regular, GREY, l);
    }
    let gmax = data.iter().map(|(_, g)| g.total).fold(0.0, f64::max);
    if gmax > 0.0 {
        let maxdb = 10.0 * gmax.log10();
        for (pick, col, w) in [(2u8, VER, 0.7), (1u8, HOR, 0.7), (0u8, TOTAL, 1.3)] {
            let pts: Vec<(f64, f64)> = data
                .iter()
                .map(|(a, g)| {
                    let v = match pick {
                        0 => g.total,
                        1 => g.hor,
                        _ => g.ver,
                    };
                    let rel = if v > 0.0 { 10.0 * v.log10() - maxdb } else { -100.0 };
                    let rr = r0 * crate::plots::radius_frac(rel) as f64;
                    let ar = a.to_radians();
                    (cx + rr * ar.cos(), cy - rr * ar.sin())
                })
                .collect();
            pdf.polyline(&pts, w, col, false);
        }
    }
    let mut y = cy + if half { 16.0 } else { r0 + 22.0 };
    for l in info {
        pdf.text_centre(cx, y, 8.0, Font::Regular, INK, l);
        y += 10.5;
    }
}

/// Line chart: series (points, colour, name), optional fixed y range and marker.
fn xy(pdf: &mut Pdf, x: f64, y: f64, w: f64, h: f64, title: &str, xlabel: &str, series: &[(&[(f64, f64)], [f64; 3], &str)], fixed: Option<(f64, f64)>, marker: Option<f64>) {
    pdf.text(x, y - 5.0, 10.0, Font::Bold, INK, title);
    let (l, t, pw, ph) = (x + 38.0, y + 4.0, w - 46.0, h - 30.0);
    let all = series.iter().flat_map(|s| s.0.iter());
    let (x0, x1) = all.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (ylo, yhi) = fixed.unwrap_or_else(|| all.fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.1), a.1.max(p.1))));
    if !(x1 > x0) || !ylo.is_finite() {
        return;
    }
    let (ya, yb, ys) = if fixed.is_some() { let (_, _, s) = crate::plots::nice_range(ylo, yhi); (ylo, yhi, s) } else { crate::plots::nice_range(ylo, yhi) };
    let (xa, xb, xs) = crate::plots::nice_range(x0, x1);
    let (xa, xb) = (xa.max(x0), xb.min(x1));
    let px = |v: f64| l + (v - xa) / (xb - xa) * pw;
    let py = |v: f64| t + ph - (v - ya) / (yb - ya) * ph;
    for v in crate::plots::ticks((ya / ys).ceil() * ys, yb, ys) {
        pdf.line((l, py(v)), (l + pw, py(v)), 0.3, LIGHT);
        pdf.text_right(l - 3.0, py(v) + 2.5, 7.0, Font::Regular, GREY, &f(v, if ys < 1.0 { 1 } else { 0 }));
    }
    for v in crate::plots::ticks((xa / xs).ceil() * xs, xb, xs) {
        pdf.line((px(v), t), (px(v), t + ph), 0.3, LIGHT);
        pdf.text_centre(px(v), t + ph + 9.0, 7.0, Font::Regular, GREY, &format!("{}", (v * 1000.0).round() / 1000.0));
    }
    pdf.polyline(&[(l, t), (l + pw, t), (l + pw, t + ph), (l, t + ph), (l, t)], 0.6, GREY, false);
    pdf.text_centre(l + pw / 2.0, t + ph + 19.0, 7.5, Font::Regular, GREY, xlabel);
    if let Some(m) = marker {
        if m >= xa && m <= xb {
            pdf.polyline(&[(px(m), t), (px(m), t + ph)], 0.7, GREY, true);
        }
    }
    let mut lx = l + pw;
    for (pts, col, name) in series.iter().rev() {
        let clipped: Vec<(f64, f64)> = pts.iter().map(|&(a, b)| (px(a), py(b.clamp(ya, yb)))).collect();
        pdf.polyline(&clipped, 1.1, *col, false);
        lx -= text_width(name, 8.0, Font::Bold) + 8.0;
        pdf.text(lx, y - 5.0, 8.0, Font::Bold, *col, name);
    }
}

/// The whole report.
/// The antenna as on the View tab (same viewing direction, wire numbers and
/// current colours), fitted into the box (x, y, w, h).
fn antenna_picture(pdf: &mut Pdf, x: f64, y: f64, w: f64, h: f64, ant: &Antenna, calc: Option<&FullResult>, view: &crate::plots::ViewState) {
    let col = |c: eframe::egui::Color32| [c.r() as f64 / 255.0, c.g() as f64 / 255.0, c.b() as f64 / 255.0];
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let world = |p: [f64; 3]| [p[0], p[1], p[2] + dz];
    let pts: Vec<[f64; 3]> = ant.wires.iter().flat_map(|w| [world(w.p1), world(w.p2)]).collect();
    pdf.polyline(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)], 0.5, LIGHT, false);
    if pts.is_empty() {
        pdf.text_centre(x + w / 2.0, y + h / 2.0, 10.0, Font::Regular, GREY, "No wires");
        return;
    }
    let (cy, sy, cp, sp) = ((view.yaw as f64).cos(), (view.yaw as f64).sin(), (view.pitch as f64).cos(), (view.pitch as f64).sin());
    // projection without scale; screen y down
    let raw = |p: [f64; 3]| {
        let x1 = p[0] * cy - p[1] * sy;
        let y1 = p[0] * sy + p[1] * cy;
        (x1, -(p[2] * cp - y1 * sp))
    };
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in &pts {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    if grounded {
        lo[2] = lo[2].min(0.0);
    }
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in pts.iter().copied().chain(grounded.then_some([lo[0], lo[1], 0.0])).chain(grounded.then_some([hi[0], hi[1], 0.0])) {
        let (u, v) = raw(p);
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let pad = 24.0;
    let span = (u1 - u0).max(v1 - v0).max(1e-6);
    let sc = ((w - 2.0 * pad) / (u1 - u0).max(span * 1e-3)).min((h - 2.0 * pad) / (v1 - v0).max(span * 1e-3));
    let (ox, oy) = (x + w / 2.0 - sc * (u0 + u1) / 2.0, y + h / 2.0 - sc * (v0 + v1) / 2.0);
    let proj = |p: [f64; 3]| {
        let (u, v) = raw(p);
        (ox + sc * u, oy + sc * v)
    };
    pdf.clip_begin(x, y, w, h);
    let ext = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt().max(1e-3);
    let ctr = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    if grounded {
        let g = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.75).max(ext * 0.25);
        let gc = [0.75, 0.67, 0.51];
        for i in 0..=8 {
            let t = -g + 2.0 * g * i as f64 / 8.0;
            pdf.line(proj([ctr[0] + t, ctr[1] - g, 0.0]), proj([ctr[0] + t, ctr[1] + g, 0.0]), 0.35, gc);
            pdf.line(proj([ctr[0] - g, ctr[1] + t, 0.0]), proj([ctr[0] + g, ctr[1] + t, 0.0]), 0.35, gc);
        }
        let s2 = &ant.ground.second;
        if s2.enabled {
            let blue = [0.16, 0.43, 0.78];
            if s2.circle {
                let r = s2.distance.max(0.0);
                let c: Vec<(f64, f64)> = (0..=96).map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 96.0;
                    proj([r * a.cos(), r * a.sin(), 0.0])
                }).collect();
                pdf.polyline(&c, 1.2, blue, false);
            } else {
                let b = s2.bearing_deg.to_radians();
                let (nx, ny) = (b.cos(), b.sin());
                let (px, py) = (s2.distance * nx, s2.distance * ny);
                let l = (g + s2.distance.abs()) * 2.0;
                pdf.line(proj([px - l * ny, py + l * nx, 0.0]), proj([px + l * ny, py - l * nx, 0.0]), 1.2, blue);
                let tip = proj([px + g * 0.15 * nx, py + g * 0.15 * ny, 0.0]);
                pdf.line(proj([px, py, 0.0]), tip, 1.2, blue);
                pdf.text(tip.0 + 2.0, tip.1, 7.0, Font::Regular, blue, "second ground");
            }
        }
    }
    // axes
    let al = ext * 0.12;
    let o = [lo[0] - al * 0.6, lo[1] - al * 0.6, lo[2]];
    for (i, (c, name)) in [([0.82, 0.16, 0.16], "X"), ([0.12, 0.59, 0.2], "Y"), ([0.16, 0.27, 0.86], "Z")].into_iter().enumerate() {
        let mut e = o;
        e[i] += al;
        pdf.line(proj(o), proj(e), 1.0, c);
        let p = proj(e);
        pdf.text(p.0 + 2.0, p.1 + 2.0, 7.0, Font::Bold, c, name);
    }
    // wires, coloured by current like the View
    let calc_ok = calc.filter(|c| c.mesh.wire_segs.len() == ant.wires.len()).filter(|_| view.show_current);
    let mut imax = 0.0f64;
    let mut drawn = false;
    if let Some(c) = calc_ok {
        if let Some(v) = c.seg_currents.as_ref() {
            imax = v.iter().map(|s| s.2).fold(0.0, f64::max);
            if imax > 0.0 {
                for (a, b, i) in v {
                    pdf.line(proj(*a), proj(*b), 1.8, col(crate::plots::heat((i / imax) as f32)));
                }
                drawn = true;
            }
        } else {
            let mut acc = vec![[C::new(0.0, 0.0); 2]; c.mesh.segs.len()];
            for (b, i) in c.mesh.bases.iter().zip(c.currents.iter()) {
                for p in &b.parts {
                    acc[p.seg][p.half as usize] += *i * p.dir;
                }
            }
            imax = acc.iter().flat_map(|a| [a[0].norm(), a[1].norm()]).fold(0.0, f64::max);
            if imax > 0.0 {
                for (si, s) in c.mesh.segs.iter().enumerate() {
                    let m = [(s.a[0] + s.b[0]) / 2.0, (s.a[1] + s.b[1]) / 2.0, (s.a[2] + s.b[2]) / 2.0];
                    pdf.line(proj(s.a), proj(m), 1.8, col(crate::plots::heat((acc[si][0].norm() / imax) as f32)));
                    pdf.line(proj(m), proj(s.b), 1.8, col(crate::plots::heat((acc[si][1].norm() / imax) as f32)));
                }
                drawn = true;
            }
        }
    }
    if !drawn {
        imax = 0.0;
        for wi in &ant.wires {
            pdf.line(proj(world(wi.p1)), proj(world(wi.p2)), 1.4, [0.12, 0.2, 0.43]);
        }
    }
    let at = |wi: &Wire, f: f64| world([wi.p1[0] + (wi.p2[0] - wi.p1[0]) * f, wi.p1[1] + (wi.p2[1] - wi.p1[1]) * f, wi.p1[2] + (wi.p2[2] - wi.p1[2]) * f]);
    for p in &ant.plates {
        let c = p.corners();
        let mut poly: Vec<(f64, f64)> = c.iter().map(|v| proj(world(*v))).collect();
        poly.push(poly[0]);
        pdf.polyline(&poly, 0.9, [0.27, 0.39, 0.63], false);
    }
    for t in &ant.lines {
        if let (Some(a), Some(b)) = (ant.wires.get(t.a_wire), ant.wires.get(t.b_wire)) {
            pdf.polyline(&[proj(at(a, t.a_pos)), proj(at(b, t.b_pos))], 1.0, [0.78, 0.47, 0.08], true);
        }
    }
    if view.show_labels {
        for (i, wi) in ant.wires.iter().enumerate() {
            let p = proj(at(wi, 0.5));
            pdf.text(p.0 + 2.5, p.1 - 2.5, 6.5, Font::Regular, [0.35, 0.35, 0.47], &format!("{}", i + 1));
        }
    }
    for l in &ant.loads {
        if let Some(wi) = ant.wires.get(l.wire) {
            let p = proj(at(wi, l.pos));
            pdf.fill_rect(p.0 - 3.0, p.1 - 3.0, 6.0, 6.0, [0.08, 0.63, 0.24]);
        }
    }
    for s in &ant.sources {
        if let Some(wi) = ant.wires.get(s.wire) {
            let p = proj(at(wi, s.pos));
            pdf.disc(p.0, p.1, 3.2, [0.86, 0.12, 0.12]);
            pdf.circle(p.0, p.1, 5.0, 0.6, [0.86, 0.12, 0.12]);
        }
    }
    pdf.clip_end();
    // scale bar, legend
    let target = 90.0 / sc;
    let mag = 10f64.powf(target.log10().floor());
    let len_m = [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * mag).filter(|v| *v <= target).last().unwrap_or(mag);
    let px = len_m * sc;
    let (bx, by) = (x + w - 12.0 - px, y + h - 12.0);
    pdf.line((bx, by), (bx + px, by), 1.2, INK);
    pdf.line((bx, by - 3.0), (bx, by + 3.0), 1.0, INK);
    pdf.line((bx + px, by - 3.0), (bx + px, by + 3.0), 1.0, INK);
    let lab = if len_m >= 1.0 { format!("{} m", len_m) } else { format!("{} cm", (len_m * 100.0 * 1000.0).round() / 1000.0) };
    pdf.text_centre(bx + px / 2.0, by - 5.0, 7.5, Font::Regular, INK, &lab);
    let mut lx = x + 8.0;
    let ly = y + h - 9.0;
    pdf.disc(lx + 3.0, ly - 2.5, 3.0, [0.86, 0.12, 0.12]);
    pdf.text(lx + 9.0, ly, 7.5, Font::Regular, INK, "source");
    lx += 44.0;
    if !ant.loads.is_empty() {
        pdf.fill_rect(lx, ly - 5.5, 6.0, 6.0, [0.08, 0.63, 0.24]);
        pdf.text(lx + 9.0, ly, 7.5, Font::Regular, INK, "load");
        lx += 36.0;
    }
    if !ant.lines.is_empty() {
        pdf.polyline(&[(lx, ly - 2.5), (lx + 14.0, ly - 2.5)], 1.0, [0.78, 0.47, 0.08], true);
        pdf.text(lx + 17.0, ly, 7.5, Font::Regular, INK, "line");
        lx += 42.0;
    }
    if imax > 0.0 {
        for k in 0..60 {
            let t = k as f32 / 59.0;
            pdf.line((lx + k as f64, ly - 6.0), (lx + k as f64, ly + 0.5), 1.1, col(crate::plots::heat(t)));
        }
        pdf.text(lx + 64.0, ly, 7.5, Font::Regular, INK, "current: low → high");
    }
}

pub fn build(ant: &Antenna, source: Option<&Path>, calc: Option<&FullResult>, history: &[CalcResult], st: &ToolsState, view: &crate::plots::ViewState) -> Vec<u8> {
    let stamp = now_utc();
    let mut d = Doc { pdf: Pdf::new(), y: M, title: format!("Antenna report: {}", ant.name), stamp: stamp.clone() };

    // ---- title
    d.pdf.text(M, d.y + 20.0, 20.0, Font::Bold, INK, &format!("Antenna report: {}", ant.name));
    d.y += 30.0;
    let src = source.map(|p| p.display().to_string()).unwrap_or_else(|| "not saved".into());
    d.para(&format!("File: {src}"), 9.0, GREY);
    d.para(&format!("Produced by wireant {} on {stamp}", env!("CARGO_PKG_VERSION")), 9.0, GREY);

    // ---- antenna
    d.heading("Antenna");
    let g = &ant.ground;
    let mut kv: Vec<(String, String)> = vec![
        ("Frequency".into(), format!("{} MHz (λ = {:.3} m)", ant.freq_mhz, ant.lambda())),
        ("Engine".into(), ant.engine.label().into()),
        ("Ground".into(), match g.kind {
            GroundKind::Free => "free space".into(),
            GroundKind::Perfect => format!("perfect ground, height added {} m", g.add_height),
            GroundKind::Real => format!("real ground εr {}, σ {} mS/m, height added {} m", g.eps_r, g.sigma_ms, g.add_height),
        }),
    ];
    if g.kind != GroundKind::Free && g.second.enabled {
        let s = &g.second;
        let at = if s.circle { format!("outside a {} m radius circle round the origin", s.distance) } else { format!("beyond a line {} m from the origin toward {}°", s.distance, s.bearing_deg) };
        kv.push((
            "Second ground".into(),
            format!(
                "εr {}, σ {} mS/m, {at}, {} m lower; {} (affects the pattern only)",
                s.eps_r,
                s.sigma_ms,
                s.drop,
                if s.fresnel { "Fresnel-zone transition" } else { "reflection-point rule (NEC-2)" }
            ),
        ));
    }
    kv.push(("Wire material".into(), ant.material.label().into()));
    kv.push(("SWR reference".into(), format!("{} Ω", ant.z0)));
    kv.push(("Rear range for F/B".into(), format!("azimuth {}°, vertical {}°", ant.fb_az, ant.fb_el)));
    kv.push(("Segmentation".into(), format!("shortest λ/{}, longest λ/{}, growth {}, {} short segment(s) at each end", ant.seg.dm1, ant.seg.dm2, ant.seg.sc, ant.seg.ec)));
    d.pairs(&kv);

    let total: f64 = ant.wires.iter().map(|w| w.length()).sum();
    d.sub(&format!("Antenna ({} wire{}, {:.2} m of wire, {} source{})", ant.wires.len(), if ant.wires.len() == 1 { "" } else { "s" }, total, ant.sources.len(), if ant.sources.len() == 1 { "" } else { "s" }));
    let ph = 330.0;
    d.need(ph + 8.0);
    antenna_picture(&mut d.pdf, M, d.y, PAGE_W - 2.0 * M, ph, ant, calc, view);
    d.y += ph + 6.0;
    d.para("As seen on the View tab (its viewing direction, wire numbers and current colours).", 8.0, GREY);
    if !ant.loads.is_empty() {
        d.sub("Loads");
        let rows: Vec<Vec<String>> = ant.loads.iter().map(|l| vec![pos_label(l.wire, l.pos), load_text(&l.kind)]).collect();
        d.table(&["Position", "Value"], &rows, 9.0);
    }
    if !ant.lines.is_empty() {
        d.sub("Transmission lines");
        let rows: Vec<Vec<String>> = ant
            .lines
            .iter()
            .map(|t| {
                if t.series.is_some() {
                    let no = || "—".to_string();
                    return vec![pos_label(t.a_wire, t.a_pos), pos_label(t.b_wire, t.b_pos), no(), no(), no(), no(), no(), crate::app::parts_text(t)];
                }
                vec![
                    pos_label(t.a_wire, t.a_pos),
                    pos_label(t.b_wire, t.b_pos),
                    f(t.z0, 1),
                    if t.length > 0.0 { f(t.length, 3) } else { "straight".into() },
                    f(t.vf, 3),
                    f(t.loss_db, 2),
                    if t.reversed { "yes".into() } else { "no".into() },
                    crate::app::parts_text(t),
                ]
            })
            .collect();
        d.table(&["From", "To", "Z0 (Ω)", "Length (m)", "VF", "Loss (dB/100 m)", "Reversed", "Parts"], &rows, 9.0);
    }
    if !ant.plates.is_empty() {
        d.sub("Plates");
        let v = |p: [f64; 3]| format!("({}, {}, {})", f(p[0], 3), f(p[1], 3), f(p[2], 3));
        let rows: Vec<Vec<String>> = ant.plates.iter().map(|p| vec![v(p.corner), v(p.edge1), v(p.edge2), format!("{} × {}", p.n1, p.n2)]).collect();
        d.table(&["Corner", "Edge 1", "Edge 2", "Cells"], &rows, 9.0);
    }
    if !ant.comment.trim().is_empty() {
        d.sub("Comment");
        d.para(ant.comment.trim(), 9.5, INK);
    }

    // ---- calculation
    d.heading("Calculation");
    match calc {
        None => d.para("No calculation yet: press Calculate before making the report to include results and patterns.", 9.5, GREY),
        Some(c) => {
            let r = &c.result;
            d.pairs(&[
                ("Frequency".into(), format!("{:.4} MHz", r.freq_mhz)),
                ("Impedance".into(), z_text(r.z)),
                ("SWR".into(), format!("{:.2} ({} Ω)", r.swr, ant.z0)),
                ("Gain".into(), format!("{:.2} dBi ({:.2} dBd)", r.ga_dbi, r.ga_dbi - 2.15)),
                ("Front/back".into(), format!("{:.2} dB", r.fb_db)),
                ("Efficiency".into(), format!("{} radiated", crate::mom::pct(r.eff))),
                ("NEC efficiency".into(), format!("{} (ground loss not counted)", crate::mom::pct(c.power.nec_efficiency()))),
                ("Maximum".into(), format!("azimuth {:.1}°, elevation {:.1}°", r.az_max, r.el_max)),
                ("Polarisation".into(), if r.horizontal_pol { "horizontal".into() } else { "vertical".into() }),
                ("Ground".into(), r.ground.label().into()),
                ("Problem size".into(), format!("{} unknowns, {} segments", r.n_unknowns, r.n_segs)),
                ("Time".into(), format!("{:.0} ms on {} threads", r.t_total_ms, r.threads)),
            ]);
            d.sub("Power budget");
            let p = &c.power;
            let mut rows = vec![vec!["Radiated".to_string(), crate::mom::pct(p.radiated)]];
            rows.extend(p.losses.iter().map(|(k, v)| vec![format!("Lost in {}", k.to_lowercase()), crate::mom::pct(*v)]));
            if let Some(g) = p.ground {
                rows.push(vec![if p.reflection_only { "Lost in the ground's reflection".into() } else { "Lost in the ground".into() }, crate::mom::pct(g)]);
            }
            rows.push(vec!["NEC efficiency (ground loss not counted)".into(), crate::mom::pct(p.nec_efficiency())]);
            d.table(&["Input power", "Share"], &rows, 9.0);
            let mut how = crate::mom::RADIATED_NOTE.to_string();
            if p.ground.is_some() {
                how += " The ground's share is what the radiated power and the other losses leave, including the ground wave.";
            }
            if p.reflection_only {
                how += " ";
                how += crate::mom::REFLECTION_ONLY_NOTE;
            }
            how += " ";
            how += crate::mom::NEC_EFFICIENCY_NOTE;
            d.para(&how, 8.0, GREY);
            if !c.warnings.is_empty() {
                d.sub("Notes");
                for w in &c.warnings {
                    d.bullet(w);
                }
            }
        }
    }
    if !history.is_empty() {
        d.sub("Results");
        let rows: Vec<Vec<String>> = history
            .iter()
            .enumerate()
            .map(|(i, r)| {
                vec![
                    format!("{}", i + 1),
                    f(r.freq_mhz, 3),
                    f(r.z.re, 2),
                    f(r.z.im, 2),
                    f(r.swr, 2),
                    f(r.ga_dbi - 2.15, 2),
                    f(r.ga_dbi, 2),
                    f(r.fb_db, 2),
                    f(r.el_max, 1),
                    if r.eff.is_finite() { f(r.eff * 100.0, 1) } else { "—".into() },
                    r.ground.label().replace(" ground", "").into(),
                    if r.ground == GroundKind::Free { "—".into() } else { f(r.add_height, 2) },
                    if r.horizontal_pol { "hori.".into() } else { "vert.".into() },
                ]
            })
            .collect();
        d.table(&["No.", "F (MHz)", "R (Ω)", "jX (Ω)", "SWR", "Gh dBd", "Ga dBi", "F/B dB", "Elev.", "Eff. %", "Ground", "Add H.", "Polar."], &rows, 8.5);
    }
    if let Some(c) = calc {
        let p = &c.patterns;
        let r = &c.result;
        let ground = !p.free_space;
        let r0: f64 = 105.0;
        d.need(2.0 * r0 + 80.0);
        d.y += 8.0;
        let cy = d.y + r0 + 34.0;
        let el: Vec<(f64, Gain)> = if ground { p.el_cut.iter().filter(|(t, _)| *t <= 180.0).cloned().collect() } else { p.el_cut.clone() };
        polar(
            &mut d.pdf,
            M + (PAGE_W - 2.0 * M) * 0.25,
            cy,
            r0,
            &p.az_cut,
            None,
            "Horizontal pattern (top view)",
            &[format!("elevation {:.1}°   Ga {:.2} dBi   F/B {:.2} dB", p.el_cut_deg, r.ga_dbi, r.fb_db)],
        );
        let ecy = if ground { cy + r0 * 0.35 } else { cy };
        polar(
            &mut d.pdf,
            M + (PAGE_W - 2.0 * M) * 0.75,
            ecy,
            r0,
            &el,
            Some(ground),
            "Vertical pattern (side view)",
            &[format!("azimuth {:.1}°   maximum at {:.1}° elevation", p.az_cut_deg, r.el_max)],
        );
        d.y = cy + r0 + 48.0;
        d.pdf.text(M, d.y, 8.0, Font::Bold, TOTAL, "total");
        d.pdf.text(M + 28.0, d.y, 8.0, Font::Bold, HOR, "horizontal");
        d.pdf.text(M + 76.0, d.y, 8.0, Font::Bold, VER, "vertical");
        d.pdf.text(M + 115.0, d.y, 8.0, Font::Regular, GREY, "rings 0, −3, −6, −10, −20, −30 dB below the maximum");
        d.y += 10.0;
    }

    // ---- resonances
    d.heading("Resonances");
    match &st.report {
        None => d.para("The resonance finder hasn't been run (Tools → Resonance finder).", 9.5, GREY),
        Some(rep) => {
            d.para(
                &format!(
                    "Searched {:.4}–{:.4} MHz at {} points; SWR against {} Ω, bandwidth at SWR ≤ {}.",
                    rep.points[0].0,
                    rep.points.last().unwrap().0,
                    rep.points.len(),
                    rep.z0,
                    rep.limit
                ),
                9.5,
                INK,
            );
            if rep.resonances.is_empty() {
                d.para("No resonance (reactance zero) in this range.", 9.5, INK);
            } else {
                let bw = |b: Option<(f64, f64)>, f0: f64| match b {
                    Some((lo, hi)) => format!("{:.1} kHz ({:.2} %), {:.4}–{:.4}", (hi - lo) * 1000.0, 100.0 * (hi - lo) / f0, lo, hi),
                    None => "—".into(),
                };
                let rows: Vec<Vec<String>> = rep
                    .resonances
                    .iter()
                    .map(|r| {
                        vec![
                            format!("{:.4}", r.f_mhz),
                            if r.series { "series".into() } else { "parallel".into() },
                            f(r.z.re, 1),
                            if r.swr.is_finite() { f(r.swr, 2) } else { "∞".into() },
                            bw(r.bw_ref, r.f_mhz),
                            bw(r.bw_matched, r.f_mhz),
                            if r.q.is_finite() { f(r.q, 1) } else { "—".into() },
                        ]
                    })
                    .collect();
                d.table(&["F (MHz)", "Type", "R (Ω)", "SWR", &format!("SWR ≤ {} vs {} Ω", rep.limit, rep.z0), "Matched to R", "Q"], &rows, 8.5);
            }
            d.para(&format!("Lowest SWR in the range: {:.2} at {:.4} MHz.", rep.min_swr.1, rep.min_swr.0), 9.5, INK);
            d.para("\"Matched to R\": the bandwidth once the antenna is matched at the resonance (SWR against its own resistance). Q = f·|dZ/df| / 2R.", 8.0, GREY);
            let w = PAGE_W - 2.0 * M;
            let mark = rep.resonances.get(st.sel).map(|r| r.f_mhz);
            let swr_pts: Vec<(f64, f64)> = rep.points.iter().map(|&(f, z)| (f, tools::swr(z, rep.z0).min(10.0))).collect();
            let rr: Vec<(f64, f64)> = rep.points.iter().map(|&(f, z)| (f, z.re)).collect();
            let xx: Vec<(f64, f64)> = rep.points.iter().map(|&(f, z)| (f, z.im)).collect();
            d.need(330.0);
            d.y += 16.0;
            xy(&mut d.pdf, M, d.y, w, 150.0, &format!("SWR ({} Ω)", rep.z0), "MHz", &[(&swr_pts, TOTAL, "SWR")], Some((1.0, 10.0)), mark);
            d.y += 172.0;
            xy(&mut d.pdf, M, d.y, w, 150.0, "Impedance (Ω)", "MHz", &[(&rr, HOR, "R"), (&xx, VER, "jX")], None, mark);
            d.y += 158.0;
        }
    }

    // ---- matching
    d.heading("Matching");
    let zl = C::new(st.m_r, st.m_x);
    let f_hz = st.m_f * 1e6;
    let r0 = st.m_z0;
    d.pairs(&[
        ("Antenna impedance".into(), z_text(zl)),
        ("Frequency".into(), format!("{} MHz", st.m_f)),
        ("Matched to".into(), format!("{} Ω (SWR now {:.2})", r0, tools::swr(zl, r0))),
    ]);
    d.sub("L networks (L and C)");
    let sols = tools::l_networks(zl, r0);
    if sols.is_empty() {
        d.bullet("No L network: the resistance must be positive.");
    }
    for (k, (xp, xs, at_load)) in sols.iter().enumerate() {
        let (x1, x2) = if *at_load { (*xp, *xs) } else { (*xs, *xp) };
        let (w1, w2) = if *at_load { ("across the antenna", "in series toward the transmitter") } else { ("in series with the antenna", "across the transmitter side") };
        let one = |x: f64, w: &str| if x.is_infinite() || x.abs() < 1e-6 { format!("nothing {w}") } else { format!("{} {w}", tools::part(x, f_hz)) };
        let kind = if (x1 > 0.0) != (x2 > 0.0) && !x1.is_infinite() && !x2.is_infinite() {
            let low = if *at_load { x1 < 0.0 } else { x1 > 0.0 };
            if low { " (low-pass)" } else { " (high-pass)" }
        } else {
            ""
        };
        d.bullet(&format!("Option {}{kind}: {}; then {}.", k + 1, one(x1, w1), one(x2, w2)));
    }
    d.sub("Gamma match");
    d.para(&format!("Element {} mm, rod {} mm, spacing {} mm (centres).", st.g_del, st.g_drod, st.g_s), 9.5, INK);
    if st.g_s * 2.0 <= st.g_del + st.g_drod {
        d.bullet("The rod overlaps the element: the spacing is too small.");
    } else {
        match tools::gamma_match(zl, r0, f_hz, st.g_del * 1e-3, st.g_drod * 1e-3, st.g_s * 1e-3) {
            Some((l, xc)) if xc < 0.0 => d.bullet(&format!("Rod length (feed to shorting strap) {:.3} m; series capacitor {}.", l, tools::part(xc, f_hz))),
            Some((l, xc)) => d.bullet(&format!("Rod length {:.3} m; the remaining reactance is capacitive: {} in series instead of a capacitor.", l, tools::part(xc, f_hz))),
            None => d.bullet("No rod length up to λ/4 reaches the target: use a thinner rod or wider spacing."),
        }
    }
    d.para("Transmission-line model (ARRL Antenna Book, after Healey); approximate, so model the rod as wires to confirm.", 8.0, GREY);
    d.sub("Coax sections");
    let lam = 299.792458 / st.m_f;
    let len = |deg: f64| deg / 360.0 * lam * st.c_vf;
    let ss = tools::series_section(zl, r0, st.c_z1);
    if ss.is_empty() {
        d.bullet(&format!(
            "Series section: not possible with {} Ω (it reaches SWR up to {:.2}; this load is {:.2}).",
            st.c_z1,
            (st.c_z1 / r0).max(r0 / st.c_z1).powi(2),
            tools::swr(zl, r0)
        ));
    }
    for (th2, th1) in &ss {
        d.bullet(&format!(
            "Series section: from the antenna, {:.1}° of {} Ω feeder ({:.3} m), then {:.1}° of {} Ω ({:.3} m), then the {} Ω feeder.",
            th2, r0, len(*th2), th1, st.c_z1, len(*th1), r0
        ));
    }
    for (th, r, zq) in tools::quarter_wave(zl, r0) {
        d.bullet(&format!(
            "Quarter-wave transformer: {:.1}° of {} Ω feeder ({:.3} m) to where it is {:.1} Ω, then a quarter wave ({:.3} m) of {:.1} Ω.",
            th, r0, len(th), r, len(90.0), zq
        ));
    }
    d.para(&format!("Lengths at velocity factor {} (lossless lines).", st.c_vf), 8.0, GREY);

    d.footer();
    let title = d.title.clone();
    d.pdf.bytes(&title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_with_and_without_results() {
        let a = Antenna::example(2);
        let empty = build(&a, None, None, &[], &ToolsState::default(), &crate::plots::ViewState::default());
        assert!(empty.starts_with(b"%PDF"));
        let calc = crate::mom::calculate(&a).unwrap();
        let mut st = ToolsState::default();
        st.report = Some(tools::find(&a, 13.5, 14.8, 31, 2.0).unwrap());
        let full = build(&a, Some(Path::new("/tmp/x.maa")), Some(&calc), &[calc.result.clone()], &st, &crate::plots::ViewState::default());
        let s: String = full.iter().map(|&c| if c < 128 { c as char } else { '#' }).collect();
        assert!(s.contains("/Count ") && full.len() > empty.len());
        let dir = std::env::temp_dir().join("wireant-report-test.pdf");
        std::fs::write(&dir, &full).unwrap();
    }

    #[test]
    fn utc_date_looks_right() {
        let s = now_utc();
        assert!(s.len() == 20 && s.ends_with("UTC") && s.starts_with("20"), "{s}");
    }
}

#[cfg(test)]
mod blank_tests {
    use super::*;

    #[test]
    fn a_new_antenna_is_empty_and_nothing_breaks_on_it() {
        let a = Antenna::blank();
        assert!(a.wires.is_empty() && a.sources.is_empty() && a.loads.is_empty());
        // calculations refuse politely, on every engine
        for e in [Engine::Wiremom, Engine::Mininec, Engine::Nec2] {
            let mut b = a.clone();
            b.engine = e;
            assert!(crate::mom::calculate(&b).is_err(), "{e:?}");
            assert!(crate::mom::sweep(&b, 14.0, 14.3, 5, false).is_err(), "{e:?}");
            assert!(tools::find(&b, 14.0, 14.3, 5, 2.0).is_err(), "{e:?}");
        }
        // report and the file formats cope
        assert!(build(&a, None, None, &[], &ToolsState::default(), &crate::plots::ViewState::default()).starts_with(b"%PDF"));
        let _ = crate::maa::write(&a);
        let _ = crate::nec_file::write(&a);
        let _ = crate::ez::write(&a);
    }
}

