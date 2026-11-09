//! Reading and writing EZNEC .ez files (binary).
//!
//! Layout (from the published notes on the format, as collected by the open-source ezparse
//! project, and checked against 52 real files): a 170-byte global record, then one 170-byte
//! record per wire/source/load index (record i holds wire i, source i and load i together),
//! a near-field record and a reserved record (170 bytes each), then tagged blocks
//! (u16 type, u32 length including the 7-byte header, u8 revision). Block 12 holds wire
//! insulation (εr, thickness m, loss tangent per wire), block 13 per-wire loss (ρ, μr).
//! Coordinates and wire diameters are stored in metres whatever the display units.

use crate::model::*;

const BS: usize = 170;

struct Rd<'a>(&'a [u8]);
impl Rd<'_> {
    fn u8(&self, o: usize) -> u8 {
        *self.0.get(o).unwrap_or(&0)
    }
    fn u16(&self, o: usize) -> u16 {
        self.0.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
    }
    fn u32(&self, o: usize) -> u32 {
        self.0.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0)
    }
    fn f(&self, o: usize) -> f64 {
        self.0.get(o..o + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64).unwrap_or(0.0)
    }
}

pub struct Imported {
    pub ant: Antenna,
    pub warnings: Vec<String>,
}

fn awg_diameter_m(ga: u16) -> f64 {
    0.005 * 92f64.powf((36.0 - ga as f64) / 39.0) * 0.0254
}

fn material_from_rho(ant: &mut Antenna, rho: f64, mu: f64) {
    ant.material = if rho <= 0.0 {
        Material::Lossless
    } else if mu <= 1.01 && (rho - 1.72e-8).abs() / 1.72e-8 < 0.1 {
        Material::CuWire
    } else if mu <= 1.01 && (rho - 2.8e-8).abs() / 2.8e-8 < 0.12 {
        Material::AlWire
    } else {
        ant.user_rho = rho;
        ant.user_mu = mu.max(1.0);
        Material::UserWire
    };
}

pub fn parse(d: &[u8], file_name: &str) -> Result<Imported, String> {
    let r = Rd(d);
    if d.len() < 4 * BS || r.u16(63) != 1304 {
        return Err("This doesn't look like an EZNEC .ez file (check value 1304 missing).".into());
    }
    let mut warn = Vec::new();
    let old_max = r.u16(0) as usize;
    let max_rec = if old_max < 32767 { old_max } else { r.u32(124) as usize };
    let freq = r.f(5);
    let title: String = d[18..48].iter().take_while(|b| **b != 0).map(|&b| b as char).collect::<String>().trim().to_string();
    let nw = { let n = r.u32(128) as usize; if n > 0 && n <= max_rec { n } else { r.u16(48) as usize } };
    let nsrc = r.u16(50) as usize;
    let nl = r.u16(52) as usize;
    let gtype = r.u8(54) as char;
    let ltype = r.u8(117) as char;
    let ganal = r.u8(123) as char;
    let nt = r.u16(120) as usize;
    if d.len() < BS * (max_rec + 3) {
        return Err("The file is shorter than its header says.".into());
    }
    let rec = |i: usize| BS + BS * i;
    let mut ant = Antenna::example(0);
    ant.name = if title.is_empty() { file_name.to_string() } else { title };
    ant.comment.clear();
    ant.freq_mhz = if freq > 0.0 { freq } else { 14.15 };
    ant.z0 = { let z = r.f(110); if z > 0.0 { z } else { 50.0 } };
    ant.ground.add_height = 0.0;
    ant.ground.kind = match gtype {
        'P' => GroundKind::Perfect,
        'R' => GroundKind::Real,
        _ => GroundKind::Free,
    };
    if ant.ground.kind == GroundKind::Real {
        let o = rec(0);
        let (sig, eps) = (r.f(o), r.f(o + 4));
        if eps > 0.0 {
            ant.ground.eps_r = eps;
        }
        if sig > 0.0 {
            ant.ground.sigma_ms = sig * 1000.0;
        }
        if r.u16(2) > 1 {
            warn.push("Only the first ground medium is used.".into());
        }
        if r.u16(55) > 0 {
            warn.push("Ground radials (a radial screen) aren't modelled.".into());
        }
    }
    let _ = ganal;
    ant.wires = (0..nw)
        .map(|i| {
            let o = rec(i) + 16;
            let mut dia = r.f(o + 24);
            let ga = r.u16(o + 28);
            if dia <= 0.0 && ga != 0xffff {
                dia = awg_diameter_m(ga);
            }
            let segs = r.u16(o + 30) as i32;
            Wire {
                p1: [r.f(o), r.f(o + 4), r.f(o + 8)],
                p2: [r.f(o + 12), r.f(o + 16), r.f(o + 20)],
                r: (dia / 2.0).max(1e-6),
                seg: if segs > 0 { segs } else { -1 },
                ins_t: 0.0,
                ins_eps: 1.0,
            }
        })
        .collect();
    ant.sources.clear();
    let mut current_sources = 0;
    for i in 0..nsrc {
        let o = rec(i);
        let w = r.u16(o + 48) as usize;
        if w == 0 || w > nw {
            warn.push(format!("Source {} is on wire {}, which doesn't exist.", i + 1, w));
            continue;
        }
        let st = r.u8(o + 62) as char;
        if st == 'I' || st == 'J' {
            current_sources += 1;
        }
        ant.sources.push(Source {
            wire: w - 1,
            pos: (r.f(o + 50) / 100.0).clamp(0.0, 1.0),
            phase_deg: r.f(o + 58),
            volts: r.f(o + 54).max(1e-9),
        });
    }
    if current_sources > 1 {
        warn.push("EZNEC current sources were read as voltage sources; with several sources the currents' ratios can differ.".into());
    }
    ant.loads.clear();
    for i in 0..nl {
        let o = rec(i);
        let w = r.u16(o + 63) as usize;
        if w == 0 || w > nw {
            warn.push(format!("Load {} is on wire {}, which doesn't exist.", i + 1, w));
            continue;
        }
        let pos = (r.f(o + 65) / 100.0).clamp(0.0, 1.0);
        let kind = match ltype {
            'R' => {
                let (rr, l, c, fr) = (r.f(o + 151), r.f(o + 155), r.f(o + 159), r.f(o + 163));
                match r.u8(o + 150) as char {
                    'T' => {
                        // trap: R in series with L, all in parallel with C
                        let f = if fr > 0.0 { fr } else { ant.freq_mhz };
                        let q = if rr > 0.0 { 2.0 * std::f64::consts::PI * f * 1e6 * l / rr } else { 0.0 };
                        LoadKind::Lc { l_uh: l * 1e6, c_pf: c * 1e12, q }
                    }
                    'P' => LoadKind::Rlc { r: rr, l_uh: l * 1e6, c_pf: c * 1e12, parallel: true },
                    _ => LoadKind::Rlc { r: rr, l_uh: l * 1e6, c_pf: c * 1e12, parallel: false },
                }
            }
            'L' => {
                warn.push(format!("Load {} is a Laplace load, read as R+jX = 0 (not supported).", i + 1));
                LoadKind::Rx { r: 0.0, x: 0.0 }
            }
            _ => LoadKind::Rx { r: r.f(o + 69), x: r.f(o + 73) },
        };
        ant.loads.push(Load { wire: w - 1, pos, kind });
    }
    if nt > 0 {
        warn.push(format!("{nt} transmission line(s) skipped: transmission lines aren't supported yet."));
    }
    if r.u16(156) > 0 || r.u16(152) > 0 || r.u16(154) > 0 {
        warn.push("Transformers, networks or L-networks were skipped (not supported).".into());
    }
    // global wire loss
    material_from_rho(&mut ant, r.f(98), r.f(102));
    // variable-length blocks
    let mut pos = BS * (max_rec + 3);
    while pos + 7 <= d.len() {
        let bt = r.u16(pos);
        let bl = r.u32(pos + 2) as usize;
        if bl < 7 || pos + bl > d.len() {
            break;
        }
        let body = pos + 7;
        match bt {
            12 => {
                let n = (r.u32(body) as usize).min(nw);
                for i in 0..n {
                    let o = body + 4 + 12 * i;
                    let (eps, t) = (r.f(o), r.f(o + 4));
                    if t > 0.0 && eps > 1.0 {
                        ant.wires[i].ins_t = t;
                        ant.wires[i].ins_eps = eps;
                    }
                }
            }
            13 => {
                let n = (r.u32(body) as usize).min(nw);
                let vals: Vec<(f64, f64)> = (0..n).map(|i| (r.f(body + 4 + 8 * i), r.f(body + 8 + 8 * i))).collect();
                if let Some(&(rho, mu)) = vals.first() {
                    material_from_rho(&mut ant, rho, mu);
                    if vals.iter().any(|v| (v.0 - rho).abs() > rho * 1e-3) {
                        warn.push("Wires have different materials; the first wire's is used for all.".into());
                    }
                }
            }
            _ => {}
        }
        pos += bl;
    }
    if ant.sources.is_empty() {
        warn.push("No source found.".into());
    }
    Ok(Imported { ant, warnings: warn })
}

struct Wr(Vec<u8>);
impl Wr {
    fn u8(&mut self, o: usize, v: u8) {
        self.0[o] = v;
    }
    fn u16(&mut self, o: usize, v: u16) {
        self.0[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, o: usize, v: u32) {
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn f(&mut self, o: usize, v: f64) {
        self.0[o..o + 4].copy_from_slice(&(v as f32).to_le_bytes());
    }
}

/// Write the antenna as an EZNEC .ez file (metres, mm diameters).
pub fn write(ant: &Antenna) -> Vec<u8> {
    let nw = ant.wires.len();
    let max_rec = nw.max(ant.sources.len()).max(ant.loads.len()).max(1);
    let mut w = Wr(vec![0u8; BS * (max_rec + 3)]);
    let grounded = ant.ground.kind != GroundKind::Free;
    let dz = if grounded { ant.ground.add_height } else { 0.0 };
    let rlc = ant.loads.iter().any(|l| !matches!(l.kind, LoadKind::Rx { .. }));
    // global record
    w.u16(0, max_rec.min(32767) as u16);
    w.u16(2, 1);
    w.u8(4, b'X');
    w.f(5, ant.freq_mhz);
    w.u8(9, b'E');
    w.f(10, 0.0);
    w.f(14, 1.0);
    for (i, b) in ant.name.bytes().filter(|b| b.is_ascii()).take(30).enumerate() {
        w.u8(18 + i, b);
    }
    w.u16(48, nw.min(65535) as u16);
    w.u16(50, ant.sources.len() as u16);
    w.u16(52, ant.loads.len() as u16);
    w.u8(54, match ant.ground.kind { GroundKind::Free => b'F', GroundKind::Perfect => b'P', GroundKind::Real => b'R' });
    w.u16(61, 0xffff);
    w.u16(63, 1304);
    w.u8(65, b'M');
    w.u8(66, b'F');
    w.u8(67, b'T');
    w.f(68, 1304.0);
    w.f(72, 0.0);
    w.f(76, 360.0);
    w.u8(80, b'Z');
    w.u8(89, b'Z');
    w.f(90, 1.0);
    let (rho, mu) = match ant.material.conduction(ant.user_rho, ant.user_mu) {
        Some((sig, mu)) => (1.0 / sig, mu),
        None => (0.0, 1.0),
    };
    w.f(98, rho);
    w.f(102, mu);
    w.f(110, ant.z0);
    w.u16(114, 51);
    w.u8(116, b'L');
    w.u8(117, if rlc { b'R' } else { b'Z' });
    w.u16(118, 1);
    w.u8(122, b'E');
    w.u8(123, if ant.engine == Engine::Nec2 { b'H' } else { b'M' });
    w.u32(124, max_rec as u32);
    w.u32(128, nw as u32);
    w.f(136, 5.0);
    w.u32(144, 62000000);
    w.u8(162, b'H');
    let rec = |i: usize| BS + BS * i;
    w.f(rec(0), ant.ground.sigma_ms * 1e-3);
    w.f(rec(0) + 4, ant.ground.eps_r);
    // wires (EZNEC is NEC-based: a centre feed needs an odd segment count)
    let lam = ant.lambda();
    for (i, wi) in ant.wires.iter().enumerate() {
        let o = rec(i) + 16;
        let mut ns = if wi.seg > 0 { wi.seg as usize } else { crate::mom::wire_segment_lengths(wi.length(), lam, wi.seg, wi.r, &ant.seg).len() };
        let centre = ant.sources.iter().any(|s| s.wire == i && (s.pos - 0.5).abs() < 1e-9)
            || ant.loads.iter().any(|l| l.wire == i && (l.pos - 0.5).abs() < 1e-9);
        if centre && ns % 2 == 0 {
            ns += 1;
        }
        for (k, v) in [wi.p1[0], wi.p1[1], wi.p1[2] + dz, wi.p2[0], wi.p2[1], wi.p2[2] + dz, wi.r.abs() * 2.0].iter().enumerate() {
            w.f(o + 4 * k, *v);
        }
        w.u16(o + 28, 0xffff);
        w.u16(o + 30, ns.min(65535) as u16);
    }
    for (i, s) in ant.sources.iter().enumerate() {
        let o = rec(i);
        w.u16(o + 48, (s.wire + 1) as u16);
        w.f(o + 50, s.pos * 100.0);
        w.f(o + 54, s.volts);
        w.f(o + 58, s.phase_deg);
        w.u8(o + 62, b'V');
    }
    let f_hz = ant.freq_mhz * 1e6;
    for (i, l) in ant.loads.iter().enumerate() {
        let o = rec(i);
        w.u16(o + 63, (l.wire + 1) as u16);
        w.f(o + 65, l.pos * 100.0);
        let z = l.impedance(f_hz);
        w.f(o + 69, z.re);
        w.f(o + 73, z.im);
        w.u8(o + 149, b'S');
        match l.kind {
            LoadKind::Rx { .. } => {
                // in an RLC file a fixed R+jX can only be kept at this frequency: as series R and L or C
                if rlc {
                    w.u8(o + 150, b'S');
                    w.f(o + 151, z.re);
                    if z.im > 0.0 {
                        w.f(o + 155, z.im / (2.0 * std::f64::consts::PI * f_hz));
                    } else if z.im < 0.0 {
                        w.f(o + 159, -1.0 / (2.0 * std::f64::consts::PI * f_hz * z.im));
                    }
                }
            }
            LoadKind::Rlc { r, l_uh, c_pf, parallel } => {
                w.u8(o + 150, if parallel { b'P' } else { b'S' });
                w.f(o + 151, r);
                w.f(o + 155, l_uh * 1e-6);
                w.f(o + 159, c_pf * 1e-12);
                w.f(o + 163, ant.freq_mhz);
            }
            LoadKind::Lc { l_uh, c_pf, q } => {
                let xl = 2.0 * std::f64::consts::PI * f_hz * l_uh * 1e-6;
                let rl = if q > 0.0 && l_uh != 0.0 { xl / q } else { 0.0 };
                if l_uh != 0.0 && c_pf != 0.0 {
                    w.u8(o + 150, b'T');
                    w.f(o + 151, rl);
                } else {
                    w.u8(o + 150, b'S');
                    w.f(o + 151, z.re);
                }
                w.f(o + 155, l_uh * 1e-6);
                w.f(o + 159, c_pf * 1e-12);
                w.f(o + 163, ant.freq_mhz);
            }
        }
    }
    let mut out = w.0;
    let block = |out: &mut Vec<u8>, ty: u16, body: Vec<u8>| {
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&((body.len() + 7) as u32).to_le_bytes());
        out.push(1);
        out.extend_from_slice(&body);
    };
    if ant.wires.iter().any(|x| x.insulated()) {
        let mut b = (nw as u32).to_le_bytes().to_vec();
        for x in &ant.wires {
            let (e, t) = if x.insulated() { (x.ins_eps, x.ins_t) } else { (1.0, 0.0) };
            b.extend_from_slice(&(e as f32).to_le_bytes());
            b.extend_from_slice(&(t as f32).to_le_bytes());
            b.extend_from_slice(&0f32.to_le_bytes());
        }
        block(&mut out, 12, b);
    }
    let mut b = (nw as u32).to_le_bytes().to_vec();
    for _ in 0..nw {
        b.extend_from_slice(&(rho as f32).to_le_bytes());
        b.extend_from_slice(&(mu as f32).to_le_bytes());
    }
    block(&mut out, 13, b);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_examples() {
        for i in 0..Antenna::example_names().len() {
            let mut a = Antenna::example(i);
            a.wires[0].ins_t = 0.5e-3;
            a.wires[0].ins_eps = 3.5;
            let b = parse(&write(&a), "x").unwrap();
            assert!(b.warnings.is_empty(), "{:?}", b.warnings);
            let b = b.ant;
            assert_eq!(b.wires.len(), a.wires.len());
            assert_eq!(b.sources.len(), a.sources.len());
            assert_eq!(b.ground.kind, a.ground.kind);
            // EZNEC stores resistivity only, so "pipe" comes back as "wire" with the same loss
            let (ca, cb) = (a.material.conduction(a.user_rho, a.user_mu), b.material.conduction(b.user_rho, b.user_mu));
            assert_eq!(ca.is_some(), cb.is_some());
            if let (Some(x), Some(y)) = (ca, cb) {
                assert!((x.0 - y.0).abs() / x.0 < 0.02, "{:?} vs {:?}", a.material, b.material);
            }
            assert!((b.freq_mhz - a.freq_mhz).abs() < 1e-4);
            assert!((b.wires[0].ins_eps - 3.5).abs() < 1e-6);
            assert!((b.wires[0].r - a.wires[0].r).abs() < 1e-7);
            assert!((b.sources[0].pos - a.sources[0].pos).abs() < 1e-6);
        }
    }

    #[test]
    fn reads_real_files() {
        let dir = std::env::var("EZ_SAMPLES").unwrap_or_default();
        if dir.is_empty() || !std::path::Path::new(&dir).exists() {
            return;
        }
        let mut stack = vec![std::path::PathBuf::from(dir)];
        let (mut ok, mut solved) = (0, 0);
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("ez")).unwrap_or(false) {
                    let r = parse(&std::fs::read(&p).unwrap(), "x").unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                    ok += 1;
                    if let Ok((res, ..)) = crate::mom::quick(&r.ant, r.ant.freq_mhz, r.ant.freq_mhz, false) {
                        solved += 1;
                        println!("{}: {} wires, Z = {:.1}  {:?}", p.file_name().unwrap().to_string_lossy(), r.ant.wires.len(), res.z, r.warnings);
                    } else {
                        println!("{}: not solved {:?}", p.display(), r.warnings);
                    }
                }
            }
        }
        println!("read {ok}, solved {solved}");
        assert!(ok > 0);
    }
}
