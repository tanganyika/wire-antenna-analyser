//! Reading and writing .maa antenna files (the plain-text layout this class of program uses).
//! The layout here follows the commonly published format; unknown lines are skipped.

use crate::model::*;

fn nums(line: &str) -> Vec<String> {
    line.split(|c: char| c == ',' || c == '\t' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn f(s: Option<&String>, d: f64) -> f64 {
    s.and_then(|x| x.parse::<f64>().ok()).unwrap_or(d)
}

/// Parse "w3c", "w1b", "w2e" or "w2p25.0" into (wire index, fraction).
pub fn parse_pos(s: &str) -> Option<(usize, f64)> {
    let s = s.trim().to_ascii_lowercase();
    let rest = s.strip_prefix('w')?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let w: usize = digits.parse().ok()?;
    let tail = &rest[digits.len()..];
    let mut ch = tail.chars();
    let pos = match ch.next() {
        Some('b') => 0.0,
        Some('e') => 1.0,
        Some('p') => ch.as_str().parse::<f64>().ok()? / 100.0,
        _ => 0.5,
    };
    Some((w.checked_sub(1)?, pos.clamp(0.0, 1.0)))
}

/// Files written by the original are Windows-1251 (Russian headings and comments) or plain ASCII.
pub fn decode(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.trim_start_matches('\u{feff}').to_string();
    }
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7f => b as char,
            0xc0..=0xff => char::from_u32(0x0410 + (b as u32 - 0xc0)).unwrap(),
            0xa8 => 'Ё',
            0xb8 => 'ё',
            0xb9 => '№',
            0x96 => '–',
            0x97 => '—',
            0xab => '«',
            0xbb => '»',
            0xb0 => '°',
            0xb1 => '±',
            _ => '?',
        })
        .collect()
}

pub fn parse(text: &str) -> Result<Antenna, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    if lines.len() < 4 {
        return Err("The file is too short to be an antenna file.".into());
    }
    let mut ant = Antenna::example(0);
    ant.wires.clear();
    ant.sources.clear();
    ant.loads.clear();
    ant.name = lines[0].trim().to_string();
    let mut i = 1;
    while i < lines.len() && lines[i].trim() == "*" {
        i += 1;
    }
    ant.freq_mhz = lines.get(i).and_then(|l| nums(l).first().and_then(|x| x.parse().ok())).unwrap_or(14.0);
    i += 1;
    let mut section = String::new();
    let mut star_sections = 0usize;
    let mut pending = 0usize;
    let mut comment = Vec::new();
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        let is_header = line.starts_with("###")
            || (line.len() > 2 && line.starts_with('*') && line.ends_with('*') && line.trim_matches('*').trim().len() > 0);
        if is_header {
            // Section names may be English or Russian; fall back to the fixed order
            // wires, source, load, segmentation, G/H/M/R/AzEl/X.
            let raw = line.trim_matches(|c| c == '*' || c == '#' || c == ' ').to_lowercase();
            const ORDER: [&str; 5] = ["wires", "source", "load", "segmentation", "g/h/m/r/azel/x"];
            section = if line.starts_with("###") {
                "comment".to_string()
            } else if raw.starts_with("wire") || raw.starts_with("провод") {
                "wires".into()
            } else if raw.starts_with("source") || raw.starts_with("источ") {
                "source".into()
            } else if raw.starts_with("load") || raw.starts_with("нагруз") {
                "load".into()
            } else if raw.starts_with("segm") || raw.starts_with("автосегм") {
                "segmentation".into()
            } else if raw.starts_with("g/h") {
                "g/h/m/r/azel/x".into()
            } else if raw.starts_with("second ground") {
                "second ground".into()
            } else if raw.starts_with("ground") {
                "ground".into()
            } else if raw.starts_with("insulation") {
                "insulation".into()
            } else {
                ORDER.get(star_sections).map(|s| s.to_string()).unwrap_or_default()
            };
            star_sections += 1;
            pending = usize::MAX; // next line holds the count
            continue;
        }
        if section.starts_with("comment") {
            comment.push(lines[i - 1].to_string());
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let v = nums(line);
        match section.as_str() {
            "wires" => {
                if pending == usize::MAX {
                    pending = f(v.first(), 0.0) as usize;
                    continue;
                }
                if v.len() >= 7 && pending > 0 {
                    let g = |k: usize| f(v.get(k), 0.0);
                    ant.wires.push(Wire {
                        p1: [g(0), g(1), g(2)],
                        p2: [g(3), g(4), g(5)],
                        r: g(6).abs().max(1e-6),
                        seg: v.get(7).and_then(|x| x.parse::<f64>().ok()).unwrap_or(-1.0) as i32,
                        ins_t: 0.0,
                        ins_eps: 1.0,
                    });
                    pending -= 1;
                }
            }
            "source" => {
                if pending == usize::MAX {
                    pending = f(v.first(), 0.0) as usize;
                    continue;
                }
                if let Some((w, p)) = v.first().and_then(|s| parse_pos(s)) {
                    ant.sources.push(Source { wire: w, pos: p, phase_deg: f(v.get(1), 0.0), volts: f(v.get(2), 1.0) });
                }
            }
            "load" => {
                if pending == usize::MAX {
                    pending = f(v.first(), 0.0) as usize;
                    continue;
                }
                if let Some((w, p)) = v.first().and_then(|s| parse_pos(s)) {
                    let t = f(v.get(1), 1.0) as i32;
                    // type 0: L (uH), C (pF), Q; type 1: R, jX (FFSolver 0x408200)
                    let kind = if t == 0 {
                        LoadKind::Lc { l_uh: f(v.get(2), 0.0), c_pf: f(v.get(3), 0.0), q: f(v.get(4), 0.0) }
                    } else {
                        LoadKind::Rx { r: f(v.get(2), 0.0), x: f(v.get(3), 0.0) }
                    };
                    ant.loads.push(Load { wire: w, pos: p, kind });
                }
            }
            "segmentation" => {
                if v.len() >= 4 {
                    ant.seg = Segmentation { dm1: f(v.first(), 400.0), dm2: f(v.get(1), 40.0), sc: f(v.get(2), 2.0), ec: f(v.get(3), 1.0) as i32 };
                }
            }
            s if s.starts_with("g/h") => {
                if v.len() >= 4 {
                    ant.ground.kind = match f(v.first(), 0.0) as i32 {
                        1 => GroundKind::Perfect,
                        2 => GroundKind::Real,
                        _ => GroundKind::Free,
                    };
                    ant.ground.add_height = f(v.get(1), 0.0);
                    ant.material = Material::from_code(f(v.get(2), 0.0) as i32);
                    ant.z0 = f(v.get(3), 50.0);
                    ant.fb_az = f(v.get(4), 120.0);
                    ant.fb_el = f(v.get(5), 60.0);
                }
            }
            "insulation" => {
                if pending == usize::MAX {
                    pending = f(v.first(), 0.0) as usize;
                    continue;
                }
                let wi = f(v.first(), 0.0) as usize;
                if wi >= 1 && wi <= ant.wires.len() {
                    ant.wires[wi - 1].ins_t = f(v.get(1), 0.0);
                    ant.wires[wi - 1].ins_eps = f(v.get(2), 1.0);
                }
            }
            "second ground" => {
                // this program's extension: εr, σ (mS/m), line|circle, distance,
                // bearing, drop, fresnel|specular
                if let Some(sm) = crate::model::SecondMedium::decode(&format!("wireant ground2 {}", line.replace(',', " "))) {
                    ant.ground.second = sm;
                }
            }
            s if s.starts_with("ground") => {
                if v.len() >= 2 {
                    ant.ground.eps_r = f(v.first(), 13.0);
                    ant.ground.sigma_ms = f(v.get(1), 5.0);
                }
            }
            _ => {}
        }
    }
    while comment.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        comment.pop();
    }
    ant.comment = comment.join("\n");
    if ant.wires.is_empty() {
        return Err("No wires were found in the file.".into());
    }
    ant.sources.retain(|s| s.wire < ant.wires.len());
    ant.loads.retain(|l| l.wire < ant.wires.len());
    Ok(ant)
}

fn g(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" || s.is_empty() { "0.0".into() } else if s.contains('.') { s } else { format!("{s}.0") }
}

pub fn write(ant: &Antenna) -> String {
    let mut o = String::new();
    o += &format!("{}\n*\n{}\n", ant.name, g(ant.freq_mhz));
    o += &format!("*** Wires ***\n{}\n", ant.wires.len());
    for w in &ant.wires {
        o += &format!(
            "{},\t{},\t{},\t{},\t{},\t{},\t{:.3e},\t{}\n",
            g(w.p1[0]), g(w.p1[1]), g(w.p1[2]), g(w.p2[0]), g(w.p2[1]), g(w.p2[2]), w.r, if w.seg > 0 { w.seg } else { -1 }
        );
    }
    o += &format!("*** Source ***\n{},\t1\n", ant.sources.len());
    for s in &ant.sources {
        o += &format!("{},\t{},\t{}\n", pos_label(s.wire, s.pos), g(s.phase_deg), g(s.volts));
    }
    o += &format!("*** Load ***\n{},\t1\n", ant.loads.len());
    for l in &ant.loads {
        match l.kind {
            LoadKind::Rx { r, x } => o += &format!("{},\t1,\t{},\t{}\n", pos_label(l.wire, l.pos), g(r), g(x)),
            LoadKind::Lc { l_uh, c_pf, q } => {
                o += &format!("{},\t0,\t{},\t{},\t{}\n", pos_label(l.wire, l.pos), g(l_uh), g(c_pf), g(q))
            }
            LoadKind::Rlc { .. } => {
                // .maa has no general R-L-C load: store its impedance at the design frequency
                let z = l.impedance(ant.freq_mhz * 1e6);
                o += &format!("{},\t1,\t{},\t{}\n", pos_label(l.wire, l.pos), g(z.re), g(z.im))
            }
        }
    }
    o += &format!(
        "*** Segmentation ***\n{},\t{},\t{},\t{}\n",
        g(ant.seg.dm1), g(ant.seg.dm2), g(ant.seg.sc), ant.seg.ec
    );
    let gk = match ant.ground.kind {
        GroundKind::Free => 0,
        GroundKind::Perfect => 1,
        GroundKind::Real => 2,
    };
    o += &format!(
        "*** G/H/M/R/AzEl/X ***\n{},\t{},\t{},\t{},\t{},\t{},\t0.0\n",
        gk, g(ant.ground.add_height), ant.material.code(), g(ant.z0), g(ant.fb_az).trim_end_matches(".0"), g(ant.fb_el).trim_end_matches(".0")
    );
    let ins: Vec<String> = ant
        .wires
        .iter()
        .enumerate()
        .filter(|(_, w)| w.insulated())
        .map(|(i, w)| format!("{},\t{},\t{}", i + 1, g(w.ins_t), g(w.ins_eps)))
        .collect();
    if !ins.is_empty() {
        o += &format!("*** Insulation ***\n{}\n{}\n", ins.len(), ins.join("\n"));
    }
    if ant.ground.eps_r != 13.0 || ant.ground.sigma_ms != 5.0 {
        o += &format!("*** Ground ***\n{},\t{}\n", g(ant.ground.eps_r), g(ant.ground.sigma_ms));
    }
    if ant.ground.second.enabled {
        let e = ant.ground.second.encode();
        let vals: Vec<&str> = e.split_whitespace().skip(2).collect();
        o += &format!("*** Second ground ***\n{}\n", vals.join(",\t"));
    }
    o += "### Comment ###\n";
    o += &ant.comment;
    o += "\n";
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        for i in 0..Antenna::example_names().len() {
            let mut a = Antenna::example(i);
            a.loads.push(Load { wire: 0, pos: 0.25, kind: LoadKind::Lc { l_uh: 1.5, c_pf: 0.0, q: 200.0 } });
            a.comment = "hello\nworld".into();
            a.ground.second = crate::model::SecondMedium { enabled: true, circle: true, distance: 42.5, bearing_deg: 30.0, drop: 2.0, fresnel: false, eps_r: 70.0, sigma_ms: 4000.0 };
            let t = write(&a);
            let b = parse(&t).unwrap();
            assert_eq!(a.wires.len(), b.wires.len());
            assert_eq!(a.sources, b.sources);
            assert_eq!(a.loads, b.loads, "{t}");
            assert_eq!(a.ground.kind, b.ground.kind);
            assert_eq!(a.comment, b.comment);
            assert_eq!(a.ground.second, b.ground.second);
        }
    }

    #[test]
    fn original_examples() {
        let dir = "/tmp/mmana/ANT";
        if !std::path::Path::new(dir).exists() {
            return;
        }
        let mut stack = vec![std::path::PathBuf::from(dir)];
        let (mut ok, mut bad, mut no_src) = (0, 0, 0);
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("maa")).unwrap_or(false) {
                    match parse(&decode(&std::fs::read(&p).unwrap())) {
                        Ok(a) => {
                            ok += 1;
                            if a.sources.is_empty() {
                                no_src += 1;
                                println!("no source: {}", p.display());
                            }
                        }
                        Err(e) => {
                            bad += 1;
                            println!("{}: {}", p.display(), e);
                        }
                    }
                }
            }
        }
        println!("parsed {ok}, failed {bad}, without source {no_src}");
        assert!(bad * 50 < ok);
    }
}

#[cfg(test)]
mod solve_examples {
    use super::*;
    #[test]
    fn solve_original_examples() {
        let dir = "/tmp/mmana/ANT";
        if !std::path::Path::new(dir).exists() {
            return;
        }
        let mut files = vec![];
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() { stack.push(p) } else if p.extension().map(|x| x.eq_ignore_ascii_case("maa")).unwrap_or(false) { files.push(p) }
            }
        }
        files.sort();
        let (mut ok, mut err, mut big) = (0, 0, 0);
        let t = std::time::Instant::now();
        for p in &files {
            let a = parse(&decode(&std::fs::read(p).unwrap())).unwrap();
            let lam = a.lambda();
            let n: usize = a.wires.iter().map(|w| crate::mom::wire_segment_lengths(w.length(), lam, w.seg, w.r, &a.seg).len()).sum();
            if n > 2500 { big += 1; continue; }
            match crate::mom::quick(&a, a.freq_mhz, a.freq_mhz, false) {
                Ok((r, ..)) => { ok += 1; if !r.z.re.is_finite() { println!("nan {}", p.display()); } }
                Err(e) => { err += 1; println!("{}: {}", p.display(), e); }
            }
        }
        println!("solved {ok}, errors {err}, skipped big {big}, {:.1}s", t.elapsed().as_secs_f64());
    }
}
