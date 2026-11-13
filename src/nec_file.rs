//! Reading and writing NEC card decks (.nec, as used by NEC-2, 4nec2 and similar programs).
//!
//! Read: CM/CE comments, SY symbols with formulas (4nec2 style, angles in degrees),
//! GW wires, GA arcs, GH helices, GM move/rotate/copy, GR rotate-copy about Z, GX reflect,
//! GS scale, GE/GN ground, EX voltage sources, LD loads (0 series RLC, 1 parallel RLC,
//! 4 R+jX, 5 conductivity), FR frequency. Other cards are listed as warnings and skipped.
//! Write: the antenna as GW/GE/GN/LD/EX/FR/RP cards, plus wire insulation (CM + LD 2).

use crate::model::*;
use std::collections::HashMap;

// ------------------------------------------------------------------ formulas (SY)

struct Expr<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a HashMap<String, f64>,
}

impl<'a> Expr<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(c) = self.peek() {
            if c == b'+' {
                self.i += 1;
                v += self.term()?;
            } else if c == b'-' {
                self.i += 1;
                v -= self.term()?;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        while let Some(c) = self.peek() {
            if c == b'*' {
                self.i += 1;
                v *= self.power()?;
            } else if c == b'/' {
                self.i += 1;
                v /= self.power()?;
            } else {
                break;
            }
        }
        Ok(v)
    }
    fn power(&mut self) -> Result<f64, String> {
        let b = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let e = self.power()?;
            return Ok(b.powf(e));
        }
        Ok(b)
    }
    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() == Some(b')') {
                    self.i += 1;
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let st = self.i;
                while self.i < self.s.len() {
                    let d = self.s[self.i];
                    if d.is_ascii_digit() || d == b'.' {
                        self.i += 1;
                    } else if (d == b'e' || d == b'E')
                        && self.i + 1 < self.s.len()
                        && (self.s[self.i + 1].is_ascii_digit() || self.s[self.i + 1] == b'-' || self.s[self.i + 1] == b'+')
                    {
                        self.i += 2;
                    } else {
                        break;
                    }
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).unwrap();
                t.parse::<f64>().map_err(|_| format!("bad number '{t}'"))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let st = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[st..self.i]).unwrap().to_ascii_lowercase();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let a = self.expr()?;
                    if self.peek() == Some(b')') {
                        self.i += 1;
                    }
                    let r = a.to_radians();
                    return Ok(match name.as_str() {
                        "sqr" | "sqrt" => a.sqrt(),
                        "sin" => r.sin(),
                        "cos" => r.cos(),
                        "tan" => r.tan(),
                        "atn" | "atan" => a.atan().to_degrees(),
                        "asin" => a.asin().to_degrees(),
                        "acos" => a.acos().to_degrees(),
                        "exp" => a.exp(),
                        "log" | "ln" => a.ln(),
                        "log10" => a.log10(),
                        "abs" => a.abs(),
                        "int" => a.trunc(),
                        _ => return Err(format!("unknown function '{name}'")),
                    });
                }
                if name == "pi" {
                    return Ok(std::f64::consts::PI);
                }
                self.vars.get(&name).copied().ok_or_else(|| format!("unknown symbol '{name}'"))
            }
            other => Err(format!("unexpected '{}'", other.map(|c| c as char).unwrap_or(' '))),
        }
    }
}

fn eval(s: &str, vars: &HashMap<String, f64>) -> Result<f64, String> {
    let mut e = Expr { s: s.as_bytes(), i: 0, vars };
    let v = e.expr()?;
    if e.peek().is_some() {
        return Err(format!("couldn't read '{s}'"));
    }
    Ok(v)
}

// ------------------------------------------------------------------ geometry helpers

#[derive(Clone, Debug)]
struct NWire {
    tag: i64,
    nseg: i64,
    a: [f64; 3],
    b: [f64; 3],
    r: f64,
}

fn rotate(p: [f64; 3], rx: f64, ry: f64, rz: f64) -> [f64; 3] {
    // NEC GM: rotation about X, then Y, then Z (degrees)
    let (sx, cx) = rx.to_radians().sin_cos();
    let (sy, cy) = ry.to_radians().sin_cos();
    let (sz, cz) = rz.to_radians().sin_cos();
    let (x, y, z) = (p[0], p[1], p[2]);
    let (y, z) = (y * cx - z * sx, y * sx + z * cx);
    let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
    let (x, y) = (x * cz - y * sz, x * sz + y * cz);
    [x, y, z]
}

pub struct Imported {
    pub ant: Antenna,
    pub warnings: Vec<String>,
}

pub fn parse(text: &str, file_name: &str) -> Result<Imported, String> {
    let mut vars: HashMap<String, f64> = HashMap::new();
    let mut warn: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let mut wires: Vec<NWire> = Vec::new();
    let mut freq = None;
    let mut ge = 0i64;
    let mut gn: Option<(i64, f64, f64)> = None;
    let mut gd: Option<(f64, f64, f64, f64)> = None;
    let mut second_cm: Option<crate::model::SecondMedium> = None;
    let mut ex: Vec<(i64, i64, f64, f64)> = Vec::new();
    let mut ld: Vec<(i64, i64, i64, i64, f64, f64, f64)> = Vec::new();
    let mut insul: HashMap<usize, (f64, f64)> = HashMap::new();
    let mut tl: Vec<(i64, i64, i64, i64, f64, f64, bool)> = Vec::new();
    // plates: SM (or rectangular SP) gives corners 1 and 2, the next SC corner 3
    let mut plates: Vec<Plate> = Vec::new();
    let mut pending_plate: Option<([f64; 3], [f64; 3], usize, usize)> = None;
    // exact line values written by this program (NEC-2 TL can't hold them): index -> (vf, loss)
    let mut tl_extra: HashMap<usize, (f64, f64)> = HashMap::new();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        let t = line.trim();
        if t.is_empty() || t.starts_with('\'') || t.starts_with('!') {
            continue;
        }
        let card = t.get(..2).unwrap_or("").to_ascii_uppercase();
        let rest = t.get(2..).unwrap_or("").trim();
        let rest = rest.split('\'').next().unwrap_or("").trim();
        match card.as_str() {
            "CM" | "CE" => {
                let c = t.get(2..).unwrap_or("").trim().to_string();
                if let Some(x) = c.strip_prefix("wireant insulation") {
                    let v: Vec<f64> = x.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                    if v.len() >= 3 && v[0] >= 1.0 {
                        insul.insert(v[0] as usize - 1, (v[1], v[2]));
                    }
                } else if let Some(sm) = crate::model::SecondMedium::decode(&c) {
                    second_cm = Some(sm);
                } else if let Some(x) = c.strip_prefix("wireant line") {
                    let v: Vec<f64> = x.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                    if v.len() >= 3 && v[0] >= 1.0 {
                        tl_extra.insert(v[0] as usize - 1, (v[1], v[2]));
                    }
                } else if !c.is_empty() {
                    comments.push(c);
                }
                continue;
            }
            "SY" => {
                for def in rest.split(',') {
                    if let Some((k, v)) = def.split_once('=') {
                        let val = eval(v.trim(), &vars).map_err(|e| format!("line {}: {}", ln + 1, e))?;
                        vars.insert(k.trim().to_ascii_lowercase(), val);
                    }
                }
                continue;
            }
            _ => {}
        }
        let f: Vec<&str> = rest.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        let num = |i: usize| -> Result<f64, String> {
            match f.get(i) {
                Some(s) => eval(s, &vars).map_err(|e| format!("line {}: {}", ln + 1, e)),
                None => Ok(0.0),
            }
        };
        let int = |i: usize| -> Result<i64, String> { Ok(num(i)?.round() as i64) };
        match card.as_str() {
            "GW" => {
                let w = NWire {
                    tag: int(0)?,
                    nseg: int(1)?,
                    a: [num(2)?, num(3)?, num(4)?],
                    b: [num(5)?, num(6)?, num(7)?],
                    r: num(8)?,
                };
                if w.r == 0.0 {
                    warn.push(format!("line {}: tapered wire (GC) read with a 1 mm radius", ln + 1));
                }
                wires.push(NWire { r: if w.r == 0.0 { 1e-3 } else { w.r }, ..w });
            }
            "GA" => {
                // arc in the XZ plane, centred on the origin
                let (tag, ns, rad, a1, a2, r) = (int(0)?, int(1)?.max(1), num(2)?, num(3)?, num(4)?, num(5)?);
                for k in 0..ns {
                    let t0 = (a1 + (a2 - a1) * k as f64 / ns as f64).to_radians();
                    let t1 = (a1 + (a2 - a1) * (k + 1) as f64 / ns as f64).to_radians();
                    wires.push(NWire { tag, nseg: 1, a: [rad * t0.cos(), 0.0, rad * t0.sin()], b: [rad * t1.cos(), 0.0, rad * t1.sin()], r });
                }
            }
            "GH" => {
                // helix along Z: turn spacing, total length, radii at both ends
                let (tag, ns, s, hl, a1, b1, a2, b2, r) = (int(0)?, int(1)?.max(1), num(2)?, num(3)?, num(4)?, num(5)?, num(6)?, num(7)?, num(8)?);
                let turns = if s != 0.0 { hl.abs() / s } else { 1.0 };
                let sign = if hl < 0.0 { -1.0 } else { 1.0 };
                let pt = |u: f64| {
                    let ang = 2.0 * std::f64::consts::PI * turns * u * sign;
                    let (ax, by) = (a1 + (a2 - a1) * u, b1 + (b2 - b1) * u);
                    [ax * ang.cos(), by * ang.sin(), hl.abs() * u]
                };
                for k in 0..ns {
                    wires.push(NWire { tag, nseg: 1, a: pt(k as f64 / ns as f64), b: pt((k + 1) as f64 / ns as f64), r });
                }
            }
            "GM" => {
                let (inc, nrpt, rx, ry, rz, dx, dy, dz, its) = (int(0)?, int(1)?, num(2)?, num(3)?, num(4)?, num(5)?, num(6)?, num(7)?, int(8)?);
                let tf = |p: [f64; 3]| {
                    let q = rotate(p, rx, ry, rz);
                    [q[0] + dx, q[1] + dy, q[2] + dz]
                };
                let sel: Vec<NWire> = wires.iter().filter(|w| w.tag >= its).cloned().collect();
                if nrpt == 0 {
                    for w in wires.iter_mut().filter(|w| w.tag >= its) {
                        w.a = tf(w.a);
                        w.b = tf(w.b);
                        if w.tag > 0 {
                            w.tag += inc;
                        }
                    }
                } else {
                    let mut last = sel;
                    for _ in 0..nrpt {
                        last = last
                            .iter()
                            .map(|w| NWire { tag: if w.tag > 0 { w.tag + inc } else { 0 }, a: tf(w.a), b: tf(w.b), ..w.clone() })
                            .collect();
                        wires.extend(last.iter().cloned());
                    }
                }
            }
            "GR" => {
                let (inc, n) = (int(0)?, int(1)?.max(1));
                let base: Vec<NWire> = wires.clone();
                for k in 1..n {
                    let ang = 360.0 * k as f64 / n as f64;
                    for w in &base {
                        wires.push(NWire {
                            tag: if w.tag > 0 { w.tag + inc * k } else { 0 },
                            a: rotate(w.a, 0.0, 0.0, ang),
                            b: rotate(w.b, 0.0, 0.0, ang),
                            ..w.clone()
                        });
                    }
                }
            }
            "GX" => {
                let (inc, code) = (int(0)?, int(1)?);
                let flags = [code / 100 % 10 == 1, code / 10 % 10 == 1, code % 10 == 1];
                // NEC order: reflect in the Z plane, then Y, then X
                for (axis, on) in [(2usize, flags[2]), (1, flags[1]), (0, flags[0])] {
                    if !on {
                        continue;
                    }
                    let copy: Vec<NWire> = wires
                        .iter()
                        .map(|w| {
                            let mut a = w.a;
                            let mut b = w.b;
                            a[axis] = -a[axis];
                            b[axis] = -b[axis];
                            NWire { tag: if w.tag > 0 { w.tag + inc } else { 0 }, a: b, b: a, ..w.clone() }
                        })
                        .collect();
                    wires.extend(copy);
                }
            }
            "GS" => {
                let s = num(2)?;
                if s != 0.0 {
                    for w in wires.iter_mut() {
                        for i in 0..3 {
                            w.a[i] *= s;
                            w.b[i] *= s;
                        }
                        w.r *= s;
                    }
                }
            }
            "GE" => ge = int(0)?,
            "GN" => gn = Some((int(0)?, num(4)?, num(5)?)),
            // second medium: εr, σ, CLT, CHT (linear along +X unless a CM line says more)
            "GD" => gd = Some((num(4)?, num(5)?, num(6)?, num(7)?)),
            "EX" => {
                let ty = int(0)?;
                if ty != 0 {
                    warn.push(format!("line {}: EX type {} isn't supported (only voltage sources)", ln + 1, ty));
                } else {
                    ex.push((int(1)?, int(2)?, num(4)?, num(5)?));
                }
            }
            "LD" => ld.push((int(0)?, int(1)?, int(2)?, int(3)?, num(4)?, num(5)?, num(6)?)),
            "SM" | "SP" => {
                let (a, b) = ([num(2)?, num(3)?, num(4)?], [num(5)?, num(6)?, num(7)?]);
                if card == "SM" {
                    // 0 divisions = automatic meshing (the wiremom engine picks ~λ/10)
                    pending_plate = Some((a, b, int(0)?.max(0) as usize, int(1)?.max(0) as usize));
                } else if int(1)? == 1 {
                    pending_plate = Some((a, b, 0, 0));
                } else {
                    warn.push(format!("line {}: SP patch of shape {} skipped (only rectangular plates)", ln + 1, int(1)?));
                }
            }
            "SC" => {
                if let Some((a, b, n1, n2)) = pending_plate.take() {
                    let c = [num(2)?, num(3)?, num(4)?];
                    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let e2 = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
                    plates.push(Plate { corner: a, edge1: e1, edge2: e2, n1, n2 });
                }
            }
            "TL" => {
                let shunts = (6..10).map(num).collect::<Result<Vec<f64>, String>>()?;
                tl.push((int(0)?, int(1)?, int(2)?, int(3)?, num(4)?, num(5)?, shunts.iter().any(|y| *y != 0.0)));
            }
            "FR" => {
                if freq.is_none() {
                    freq = Some(num(4)?);
                }
            }
            "EN" => break,
            "GC" | "GF" | "RP" | "XQ" | "NE" | "NH" | "PQ" | "PT" | "KH" | "PL" | "WG" | "NX" => {}
            other => warn.push(format!("line {}: card {} skipped", ln + 1, other)),
        }
    }
    if wires.is_empty() {
        return Err("No wires (GW cards) were found.".into());
    }
    let mut ant = Antenna::example(0);
    ant.wires = wires
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let (t, e) = insul.get(&i).copied().unwrap_or((0.0, 1.0));
            Wire { p1: w.a, p2: w.b, r: w.r.abs(), seg: w.nseg.max(1) as i32, ins_t: t, ins_eps: e }
        })
        .collect();
    ant.name = comments.first().cloned().unwrap_or_else(|| file_name.to_string());
    ant.comment = comments.iter().skip(1).cloned().collect::<Vec<_>>().join("\n");
    ant.freq_mhz = freq.filter(|f| *f > 0.0).unwrap_or_else(|| {
        warn.push("No FR card: frequency set to 14.15 MHz".into());
        14.15
    });
    ant.material = Material::Lossless;
    ant.ground.add_height = 0.0;
    ant.ground.kind = if ge == 0 {
        GroundKind::Free
    } else {
        match gn {
            Some((-1, ..)) => GroundKind::Free,
            Some((1, ..)) | None => GroundKind::Perfect,
            Some((_, eps, sig)) => {
                ant.ground.eps_r = if eps > 0.0 { eps } else { 13.0 };
                ant.ground.sigma_ms = if sig > 0.0 { sig * 1000.0 } else { 5.0 };
                GroundKind::Real
            }
        }
    };
    if ant.ground.kind != GroundKind::Free {
        if let Some(sm) = second_cm {
            ant.ground.second = sm;
        } else if let Some((eps, sig, clt, cht)) = gd {
            ant.ground.second = crate::model::SecondMedium {
                enabled: true,
                eps_r: eps,
                sigma_ms: sig * 1000.0,
                circle: false,
                distance: clt,
                bearing_deg: 0.0,
                drop: cht,
                fresnel: false,
            };
            warn.push("GD card: taken as a linear cliff along +X with NEC-2's reflection-point rule (the RP card decides linear or circular)".into());
        }
    }
    // (tag, segment) -> (wire, position along it)
    let locate = |tag: i64, seg: i64| -> Option<(usize, f64)> {
        if tag > 0 {
            let mut k = seg;
            for (i, w) in wires.iter().enumerate().filter(|(_, w)| w.tag == tag) {
                if k <= w.nseg {
                    return Some((i, ((k.max(1) as f64) - 0.5) / w.nseg as f64));
                }
                k -= w.nseg;
            }
            None
        } else {
            let mut k = seg;
            for (i, w) in wires.iter().enumerate() {
                if k <= w.nseg {
                    return Some((i, ((k.max(1) as f64) - 0.5) / w.nseg as f64));
                }
                k -= w.nseg;
            }
            None
        }
    };
    ant.sources = ex
        .iter()
        .filter_map(|&(tag, seg, vr, vi)| {
            let (w, pos) = locate(tag, seg).or_else(|| {
                warn.push(format!("source on tag {tag} segment {seg} not found"));
                None
            })?;
            let v = num_complex::Complex64::new(vr, vi);
            Some(Source { wire: w, pos, phase_deg: v.arg().to_degrees(), volts: v.norm() })
        })
        .collect();
    ant.loads.clear();
    for &(ty, tag, s1, s2, f1, f2, f3) in &ld {
        match ty {
            5 => {
                let sig = f1;
                ant.material = if sig <= 0.0 {
                    Material::Lossless
                } else if (sig - 5.8e7).abs() / 5.8e7 < 0.15 {
                    Material::CuWire
                } else if (sig - 3.5e7).abs() / 3.5e7 < 0.15 {
                    Material::AlWire
                } else {
                    ant.user_rho = 1.0 / sig;
                    ant.user_mu = 1.0;
                    Material::UserWire
                };
                if tag != 0 {
                    warn.push("wire conductivity (LD 5) applied to all wires".into());
                }
            }
            0 | 1 | 4 => {
                let seg = if s1 > 0 { if s2 > s1 { (s1 + s2) / 2 } else { s1 } } else { 0 };
                let targets: Vec<(usize, f64)> = if tag > 0 && seg == 0 {
                    // whole tag: put it in the middle of each wire with that tag
                    wires.iter().enumerate().filter(|(_, w)| w.tag == tag).map(|(i, _)| (i, 0.5)).collect()
                } else {
                    locate(tag, seg.max(1)).into_iter().collect()
                };
                if s2 > s1 && s1 > 0 {
                    warn.push(format!("load on segments {s1}-{s2} of tag {tag} placed as one load at segment {seg}"));
                }
                for (w, pos) in targets {
                    let kind = if ty == 4 {
                        LoadKind::Rx { r: f1, x: f2 }
                    } else {
                        LoadKind::Rlc { r: f1, l_uh: f2 * 1e6, c_pf: f3 * 1e12, parallel: ty == 1 }
                    };
                    ant.loads.push(Load { wire: w, pos, kind });
                }
            }
            2 | 3 => {
                let ours = wires.iter().enumerate().any(|(i, w)| w.tag == tag && insul.contains_key(&i));
                if !ours {
                    warn.push(format!("distributed load LD {ty} on tag {tag} skipped"));
                }
            }
            _ => warn.push(format!("load type LD {ty} skipped")),
        }
    }
    ant.plates = plates;
    ant.lines.clear();
    for (i, &(t1, s1, t2, s2, z0, len, shunts)) in tl.iter().enumerate() {
        match (locate(t1, s1), locate(t2, s2)) {
            (Some((wa, pa)), Some((wb, pb))) => {
                let (vf, loss) = tl_extra.get(&i).copied().unwrap_or((1.0, 0.0));
                let mut t = TLine::new(wa, pa, wb, pb);
                t.z0 = z0.abs();
                t.reversed = z0 < 0.0;
                // NEC-2 lengths are electrical (at c); with a stored velocity factor the
                // physical length is shorter
                t.length = len.max(0.0) * vf;
                t.vf = vf;
                t.loss_db = loss;
                ant.lines.push(t);
                if shunts {
                    warn.push(format!("transmission line {}: end shunt admittances skipped", i + 1));
                }
            }
            _ => warn.push(format!("transmission line {} skipped: tag {t1} segment {s1} or tag {t2} segment {s2} not found", i + 1)),
        }
    }
    if ant.sources.is_empty() {
        warn.push("No voltage source (EX 0) found".into());
    }
    Ok(Imported { ant, warnings: warn })
}

fn n(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" || s.is_empty() { "0".into() } else { s }
}

/// Write the antenna as a NEC-2 card deck (one GW per wire, tag = wire number).
pub fn write(ant: &Antenna) -> String {
    let mut o = String::new();
    o += &format!("CM {}\n", ant.name);
    for l in ant.comment.lines() {
        o += &format!("CM {}\n", l);
    }
    o += "CM written by wireant\n";
    for (i, w) in ant.wires.iter().enumerate().filter(|(_, w)| w.insulated()) {
        o += &format!("CM wireant insulation {} {} {}\n", i + 1, w.ins_t, w.ins_eps);
    }
    for (i, t) in ant.lines.iter().enumerate().filter(|(_, t)| t.vf != 1.0 || t.loss_db != 0.0) {
        o += &format!("CM wireant line {} {} {}\n", i + 1, t.vf, t.loss_db);
    }
    if ant.ground.second.enabled && ant.ground.kind != GroundKind::Free {
        // the full second-medium description; NEC-2 itself only reads the GD card
        // (its linear cliff runs along +X: other bearings need the antenna turned)
        o += &format!("CM {}\n", ant.ground.second.encode());
    }
    o += "CE\n";
    let lam = ant.lambda();
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let mut nsegs = Vec::new();
    for (i, w) in ant.wires.iter().enumerate() {
        let mut ns = if w.seg > 0 { w.seg as usize } else { crate::mom::wire_segment_lengths(w.length(), lam, w.seg, w.r, &ant.seg).len() };
        // NEC feeds at segment centres: a centre source or load needs an odd count
        let centre = ant.sources.iter().any(|s| s.wire == i && (s.pos - 0.5).abs() < 1e-9)
            || ant.loads.iter().any(|l| l.wire == i && (l.pos - 0.5).abs() < 1e-9)
            || ant.lines.iter().any(|t| (t.a_wire == i && (t.a_pos - 0.5).abs() < 1e-9) || (t.b_wire == i && (t.b_pos - 0.5).abs() < 1e-9));
        if centre && ns % 2 == 0 {
            ns += 1;
        }
        nsegs.push(ns.max(1));
        o += &format!(
            "GW {} {} {} {} {} {} {} {} {}\n",
            i + 1,
            nsegs[i],
            n(w.p1[0]),
            n(w.p1[1]),
            n(w.p1[2] + dz),
            n(w.p2[0]),
            n(w.p2[1]),
            n(w.p2[2] + dz),
            format!("{:.6E}", w.r.abs())
        );
    }
    o += if grounded { "GE 1\n" } else { "GE 0\n" };
    match ant.ground.kind {
        GroundKind::Free => {}
        GroundKind::Perfect => o += "GN 1\n",
        GroundKind::Real => o += &format!("GN 2 0 0 0 {} {}\n", n(ant.ground.eps_r), n(ant.ground.sigma_ms * 1e-3)),
    }
    let sm = &ant.ground.second;
    let second = sm.enabled && grounded;
    if second {
        o += &format!("GD 0 0 0 0 {} {} {} {}\n", n(sm.eps_r), n(sm.sigma_ms * 1e-3), n(sm.distance.max(0.0)), n(sm.drop.max(0.0)));
    }
    let seg_of = |wire: usize, pos: f64| -> usize { ((pos * nsegs[wire] as f64).floor() as usize + 1).clamp(1, nsegs[wire]) };
    if let Some((sig, mu)) = ant.material.conduction(ant.user_rho, ant.user_mu) {
        o += &format!("LD 5 0 0 0 {:.6E}\n", sig / mu);
    }
    for (i, w) in ant.wires.iter().enumerate().filter(|(_, w)| w.insulated()) {
        o += &format!("LD 2 {} 0 0 0 {:.6E} 0\n", i + 1, w.insulation_l_per_m());
    }
    let f_hz = ant.freq_mhz * 1e6;
    let w = 2.0 * std::f64::consts::PI * f_hz;
    for l in &ant.loads {
        if l.wire >= ant.wires.len() {
            continue;
        }
        let (tag, s) = (l.wire + 1, seg_of(l.wire, l.pos));
        match l.kind {
            LoadKind::Rx { r, x } => o += &format!("LD 4 {tag} {s} {s} {} {}\n", n(r), n(x)),
            LoadKind::Rlc { r, l_uh, c_pf, parallel } => {
                o += &format!("LD {} {tag} {s} {s} {} {:.6E} {:.6E}\n", if parallel { 1 } else { 0 }, n(r), l_uh * 1e-6, c_pf * 1e-12)
            }
            LoadKind::Lc { l_uh, c_pf, q } => {
                if l_uh != 0.0 && c_pf != 0.0 {
                    // trap: parallel L-C; the coil's loss becomes an equivalent parallel R = Q·XL
                    let rp = if q > 0.0 { q * w * l_uh * 1e-6 } else { 0.0 };
                    o += &format!("LD 1 {tag} {s} {s} {} {:.6E} {:.6E}\n", n(rp), l_uh * 1e-6, c_pf * 1e-12);
                } else {
                    // series L or C; its loss R = |X|/Q at the design frequency
                    let z = l.impedance(f_hz);
                    o += &format!("LD 0 {tag} {s} {s} {} {:.6E} {:.6E}\n", n(z.re), l_uh * 1e-6, c_pf * 1e-12);
                }
            }
        }
    }
    for s in &ant.sources {
        if s.wire >= ant.wires.len() {
            continue;
        }
        let v = num_complex::Complex64::from_polar(s.volts, s.phase_deg.to_radians());
        o += &format!("EX 0 {} {} 0 {} {}\n", s.wire + 1, seg_of(s.wire, s.pos), n(v.re), n(v.im));
    }
    for p in &ant.plates {
        // NEC-2 would treat these patches as part of a closed surface
        let c = p.corners();
        let lift = |v: [f64; 3]| [v[0], v[1], v[2] + dz];
        let (a, b, d) = (lift(c[0]), lift(c[1]), lift(c[2]));
        o += &format!("SM {} {} {} {} {} {} {} {}\n", p.n1, p.n2, n(a[0]), n(a[1]), n(a[2]), n(b[0]), n(b[1]), n(b[2]));
        o += &format!("SC 0 0 {} {} {}\n", n(d[0]), n(d[1]), n(d[2]));
    }
    for t in &ant.lines {
        if t.a_wire >= ant.wires.len() || t.b_wire >= ant.wires.len() {
            continue;
        }
        let z0 = if t.reversed { -t.z0.abs() } else { t.z0.abs() };
        // NEC-2 lines travel at c: write the electrical length (0 = straight distance,
        // only exact when the velocity factor is 1)
        let len = if t.length > 0.0 { t.length / t.vf.max(1e-6) } else { 0.0 };
        o += &format!("TL {} {} {} {} {} {} 0 0 0 0\n", t.a_wire + 1, seg_of(t.a_wire, t.a_pos), t.b_wire + 1, seg_of(t.b_wire, t.b_pos), n(z0), n(len));
    }
    o += &format!("FR 0 1 0 0 {} 0\n", n(ant.freq_mhz));
    // RP mode 2 = linear cliff (along +X in NEC-2), 3 = circular
    let mode = if second { if sm.circle { 3 } else { 2 } } else { 0 };
    o += &format!("RP {mode} 91 361 1000 0 0 1 1\nEN\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plates_round_trip() {
        let mut a = Antenna::example(0);
        a.plates.push(Plate { corner: [-1.0, -2.0, 3.0], edge1: [2.0, 0.0, 0.0], edge2: [0.0, 4.0, 0.5], n1: 6, n2: 8 });
        let text = write(&a);
        let back = parse(&text, "x").unwrap();
        assert_eq!(back.ant.plates, a.plates, "{text}");
    }

    #[test]
    fn transmission_lines_round_trip() {
        let mut a = Antenna::example(0);
        a.wires.push(Wire::new([0.0, 0.0, 20.0], [0.0, 0.0, 20.1], 0.001));
        a.wires[1].seg = 3;
        let mut t = TLine::new(1, 0.5, 0, 0.5);
        t.z0 = 75.0;
        t.length = 9.1;
        t.vf = 0.66;
        t.loss_db = 1.5;
        t.reversed = true;
        a.lines.push(t.clone());
        let text = write(&a);
        assert!(text.contains("TL 2 2 1 "), "{text}");
        let back = parse(&text, "x").unwrap().ant;
        assert_eq!(back.lines.len(), 1);
        let b = &back.lines[0];
        assert_eq!((b.a_wire, b.b_wire, b.reversed), (1, 0, true));
        assert!((b.z0 - 75.0).abs() < 1e-9 && (b.length - 9.1).abs() < 1e-6 && (b.vf - 0.66).abs() < 1e-9 && (b.loss_db - 1.5).abs() < 1e-9, "{b:?}");
        assert!((b.a_pos - 0.5).abs() < 1e-9 && (b.b_pos - 0.5).abs() < 1e-9);
    }

    #[test]
    fn second_ground_round_trip_and_plain_gd() {
        let mut a = Antenna::example(0);
        a.ground.kind = GroundKind::Real;
        a.ground.second = crate::model::SecondMedium { enabled: true, circle: false, distance: 25.0, bearing_deg: 45.0, drop: 3.0, fresnel: true, eps_r: 81.0, sigma_ms: 5000.0 };
        let text = write(&a);
        assert!(text.contains("GD 0 0 0 0 ") && text.contains("RP 2 "), "{text}");
        assert_eq!(parse(&text, "x").unwrap().ant.ground.second, a.ground.second);
        // a deck from elsewhere: GD alone is NEC-2's linear cliff along +X
        let deck = "CM x\nCE\nGW 1 11 0 -5 10 0 5 10 0.001\nGE 1\nGN 2 0 0 0 13 0.005\nGD 0 0 0 0 80 4 40 2\nEX 0 1 6 0 1 0\nFR 0 1 0 0 14 0\nRP 2 1 1 1000 80 0 0 0\nEN\n";
        let s = parse(deck, "x").unwrap().ant.ground.second;
        assert!(s.enabled && !s.circle && !s.fresnel && s.distance == 40.0 && s.drop == 2.0 && s.sigma_ms == 4000.0, "{s:?}");
    }

    #[test]
    fn round_trip_examples() {
        for i in 0..Antenna::example_names().len() {
            let mut a = Antenna::example(i);
            a.wires[0].ins_t = 0.5e-3;
            a.wires[0].ins_eps = 3.5;
            let t = write(&a);
            let b = parse(&t, "x").unwrap();
            assert_eq!(b.ant.wires.len(), a.wires.len(), "{t}");
            assert_eq!(b.ant.sources.len(), a.sources.len());
            assert_eq!(b.ant.ground.kind, a.ground.kind);
            assert!((b.ant.freq_mhz - a.freq_mhz).abs() < 1e-9);
            assert!((b.ant.wires[0].ins_eps - 3.5).abs() < 1e-9);
            assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        }
    }

    #[test]
    fn reads_4nec2_symbols_and_helix() {
        let deck = "CM test yagi\nCE\nSY f=14.2, lam=299.79/f, h=lam/4*0.97\nSY r=0.001\n\
GW 1 21 0 -h 10 0 h 10 r\nGH 2 40 0.01 0.4 0.03 0.03 0.03 0.03 0.0008\nGM 0 0 0 0 0 5 0 0 2\n\
GE 1\nGN 2 0 0 0 13 0.005\nLD 5 0 0 0 5.8E7\nLD 0 1 11 11 0 1E-6 0\nEX 0 1 11 0 1 0\nFR 0 1 0 0 14.2 0\nEN\n";
        let r = parse(deck, "x").unwrap();
        assert_eq!(r.ant.wires.len(), 41);
        assert_eq!(r.ant.ground.kind, GroundKind::Real);
        assert_eq!(r.ant.material, Material::CuWire);
        assert!((r.ant.sources[0].pos - 0.5).abs() < 1e-9);
        assert!((r.ant.wires[0].p2[1] - 299.79 / 14.2 / 4.0 * 0.97).abs() < 1e-9);
        assert!((r.ant.wires[1].p1[0] - 5.03).abs() < 1e-6, "{:?}", r.ant.wires[1].p1);
        assert!(matches!(r.ant.loads[0].kind, LoadKind::Rlc { .. }));
    }

    #[test]
    fn insulation_lowers_resonance() {
        let mut a = Antenna::example(0);
        a.material = Material::Lossless;
        let bare = crate::mom::calculate(&a).unwrap().result;
        a.wires[0].ins_t = 0.5e-3;
        a.wires[0].ins_eps = 3.5;
        let ins = crate::mom::calculate(&a).unwrap().result;
        println!("bare {:.2}  insulated {:.2}  VF {:.3}", bare.z, ins.z, a.wires[0].velocity_factor());
        assert!(ins.z.im > bare.z.im + 5.0);
    }
}

#[cfg(test)]
mod nec_engine_insulation {
    use crate::model::*;
    #[test]
    fn nec2_sees_insulation_too() {
        if !crate::nec::available() {
            return;
        }
        let mut a = Antenna::example(0);
        a.material = Material::Lossless;
        a.engine = Engine::Nec2;
        let bare = crate::nec::quick(&a, a.freq_mhz, a.freq_mhz, false).unwrap();
        a.wires[0].ins_t = 0.5e-3;
        a.wires[0].ins_eps = 3.5;
        let ins = crate::nec::quick(&a, a.freq_mhz, a.freq_mhz, false).unwrap();
        println!("NEC-2 bare {:.2} insulated {:.2}", bare.z, ins.z);
        assert!(ins.z.im > bare.z.im + 5.0);
    }
}
