//! A small PDF writer: A4 pages with text in the standard Helvetica fonts (plus
//! the Symbol font for Greek letters and maths signs, so nothing is embedded) and
//! vector lines.  Coordinates are points from the top-left corner.

pub const PAGE_W: f64 = 595.28;
pub const PAGE_H: f64 = 841.89;

#[derive(Clone, Copy, PartialEq)]
pub enum Font {
    Regular,
    Bold,
}

pub struct Pdf {
    pages: Vec<String>,
    cur: String,
}

impl Default for Pdf {
    fn default() -> Self {
        Self::new()
    }
}

/// A character's byte in WinAnsi (Helvetica) or in the Symbol font.
enum Glyph {
    Ansi(u8),
    Symbol(u8),
}

fn glyph(c: char) -> Glyph {
    let ansi = |b: u8| Glyph::Ansi(b);
    match c {
        ' '..='~' => ansi(c as u8),
        '°' => ansi(0xB0),
        'µ' => ansi(0xB5),
        '²' => ansi(0xB2),
        '³' => ansi(0xB3),
        '·' => ansi(0xB7),
        '±' => ansi(0xB1),
        '×' => ansi(0xD7),
        'é' => ansi(0xE9),
        '–' => ansi(0x96),
        '—' => ansi(0x97),
        '…' => ansi(0x85),
        '‘' => ansi(0x91),
        '’' => ansi(0x92),
        '“' => ansi(0x93),
        '”' => ansi(0x94),
        '•' => ansi(0x95),
        '€' => ansi(0x80),
        'Ω' => Glyph::Symbol(0x57),
        'ε' => Glyph::Symbol(0x65),
        'σ' => Glyph::Symbol(0x73),
        'λ' => Glyph::Symbol(0x6C),
        'θ' => Glyph::Symbol(0x71),
        'φ' => Glyph::Symbol(0x66),
        'π' => Glyph::Symbol(0x70),
        'Δ' => Glyph::Symbol(0x44),
        'Γ' => Glyph::Symbol(0x47),
        'β' => Glyph::Symbol(0x62),
        'η' => Glyph::Symbol(0x68),
        '−' => Glyph::Symbol(0x2D),
        '≤' => Glyph::Symbol(0xA3),
        '≥' => Glyph::Symbol(0xB3),
        '≈' => Glyph::Symbol(0xBB),
        '∞' => Glyph::Symbol(0xA5),
        '→' => Glyph::Symbol(0xAE),
        '√' => Glyph::Symbol(0xD6),
        _ => ansi(b'?'),
    }
}

/// Approximate advance width (1/1000 em) in Helvetica, for layout.
fn width(c: char) -> f64 {
    match c {
        ' ' | '.' | ',' | ':' | ';' | '/' | 'f' | 't' | 'I' | '!' | '[' | ']' => 278.0,
        'i' | 'j' | 'l' | '\'' => 222.0,
        '(' | ')' | '-' | 'r' | '`' => 333.0,
        'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' | 'J' => 500.0,
        'm' | 'M' => 833.0,
        'w' => 722.0,
        'W' => 944.0,
        '%' => 889.0,
        'A' | 'B' | 'E' | 'K' | 'P' | 'S' | 'V' | 'X' | 'Y' => 667.0,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' => 722.0,
        'F' | 'T' | 'Z' => 611.0,
        'G' | 'O' | 'Q' => 778.0,
        'L' => 556.0,
        '+' | '=' | '<' | '>' | '±' | '×' => 584.0,
        '°' => 400.0,
        'Ω' => 768.0,
        '—' => 1000.0,
        _ => 556.0,
    }
}

/// Width of `s` in points at `size`.
pub fn text_width(s: &str, size: f64, font: Font) -> f64 {
    let k = if font == Font::Bold { 1.06 } else { 1.0 };
    s.chars().map(width).sum::<f64>() * size / 1000.0 * k
}

/// Splits `s` into lines no wider than `w` points.
pub fn wrap(s: &str, size: f64, w: f64) -> Vec<String> {
    let mut out = vec![];
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let trial = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&trial, size, Font::Regular) > w && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = trial;
            }
        }
        out.push(line);
    }
    out
}

impl Pdf {
    pub fn new() -> Pdf {
        Pdf { pages: vec![], cur: String::new() }
    }

    pub fn new_page(&mut self) {
        if !self.cur.is_empty() || self.pages.is_empty() {
            self.pages.push(std::mem::take(&mut self.cur));
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len() + usize::from(!self.cur.is_empty())
    }

    /// Text with its baseline at (x, y).
    pub fn text(&mut self, x: f64, y: f64, size: f64, font: Font, rgb: [f64; 3], s: &str) {
        let main = if font == Font::Bold { "F2" } else { "F1" };
        let mut runs: Vec<(bool, Vec<u8>)> = vec![];
        for c in s.chars() {
            let (sym, b) = match glyph(c) {
                Glyph::Ansi(b) => (false, b),
                Glyph::Symbol(b) => (true, b),
            };
            match runs.last_mut() {
                Some((s0, v)) if *s0 == sym => v.push(b),
                _ => runs.push((sym, vec![b])),
            }
        }
        let mut o = format!("BT {:.3} {:.3} {:.3} rg {:.2} {:.2} Td ", rgb[0], rgb[1], rgb[2], x, PAGE_H - y);
        for (sym, bytes) in runs {
            o += &format!("/{} {:.2} Tf (", if sym { "F3" } else { main }, size);
            for b in bytes {
                match b {
                    b'(' | b')' | b'\\' => {
                        o.push('\\');
                        o.push(b as char);
                    }
                    32..=126 => o.push(b as char),
                    _ => o += &format!("\\{:03o}", b),
                }
            }
            o += ") Tj ";
        }
        o += "ET\n";
        self.cur += &o;
    }

    /// Right-aligned text ending at x.
    pub fn text_right(&mut self, x: f64, y: f64, size: f64, font: Font, rgb: [f64; 3], s: &str) {
        let w = text_width(s, size, font);
        self.text(x - w, y, size, font, rgb, s);
    }

    pub fn text_centre(&mut self, x: f64, y: f64, size: f64, font: Font, rgb: [f64; 3], s: &str) {
        let w = text_width(s, size, font);
        self.text(x - w / 2.0, y, size, font, rgb, s);
    }

    pub fn polyline(&mut self, pts: &[(f64, f64)], width: f64, rgb: [f64; 3], dash: bool) {
        if pts.len() < 2 {
            return;
        }
        let mut o = format!("q {:.3} {:.3} {:.3} RG {:.2} w 1 J 1 j {} ", rgb[0], rgb[1], rgb[2], width, if dash { "[3 2] 0 d" } else { "[] 0 d" });
        for (i, (x, y)) in pts.iter().enumerate() {
            o += &format!("{:.2} {:.2} {} ", x, PAGE_H - y, if i == 0 { "m" } else { "l" });
        }
        o += "S Q\n";
        self.cur += &o;
    }

    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), width: f64, rgb: [f64; 3]) {
        self.polyline(&[a, b], width, rgb, false);
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, rgb: [f64; 3]) {
        self.cur += &format!("q {:.3} {:.3} {:.3} rg {:.2} {:.2} {:.2} {:.2} re f Q\n", rgb[0], rgb[1], rgb[2], x, PAGE_H - y - h, w, h);
    }

    /// Restrict drawing to a rectangle until `clip_end`.
    pub fn clip_begin(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.cur += &format!("q {:.2} {:.2} {:.2} {:.2} re W n\n", x, PAGE_H - y - h, w, h);
    }

    pub fn clip_end(&mut self) {
        self.cur += "Q\n";
    }

    /// A filled circle.
    pub fn disc(&mut self, cx: f64, cy: f64, r: f64, rgb: [f64; 3]) {
        let mut o = format!("q {:.3} {:.3} {:.3} rg ", rgb[0], rgb[1], rgb[2]);
        for i in 0..=24 {
            let a = i as f64 * std::f64::consts::TAU / 24.0;
            o += &format!("{:.2} {:.2} {} ", cx + r * a.cos(), PAGE_H - (cy + r * a.sin()), if i == 0 { "m" } else { "l" });
        }
        o += "f Q\n";
        self.cur += &o;
    }

    pub fn circle(&mut self, cx: f64, cy: f64, r: f64, width: f64, rgb: [f64; 3]) {
        let pts: Vec<(f64, f64)> = (0..=72).map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 72.0;
            (cx + r * a.cos(), cy + r * a.sin())
        }).collect();
        self.polyline(&pts, width, rgb, false);
    }

    /// The finished file.
    pub fn bytes(mut self, title: &str) -> Vec<u8> {
        if !self.cur.is_empty() || self.pages.is_empty() {
            self.pages.push(std::mem::take(&mut self.cur));
        }
        let n = self.pages.len();
        // objects: 1 catalog, 2 pages, 3-5 fonts, 6 info, then (page, content) pairs
        let mut objs: Vec<Vec<u8>> = vec![];
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 7 + 2 * i)).collect();
        objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n).into_bytes());
        objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
        objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
        objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>".to_vec());
        let esc: String = title.chars().map(|c| if c.is_ascii() && c != '(' && c != ')' && c != '\\' { c } else { '_' }).collect();
        objs.push(format!("<< /Title ({esc}) /Producer (wireant) >>").into_bytes());
        for (i, content) in self.pages.iter().enumerate() {
            objs.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {} 0 R >>",
                    8 + 2 * i
                )
                .into_bytes(),
            );
            let mut c = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            c.extend_from_slice(content.as_bytes());
            c.extend_from_slice(b"\nendstream");
            objs.push(c);
        }
        let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, xref).as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_well_formed_file() {
        let mut p = Pdf::new();
        p.text(40.0, 60.0, 12.0, Font::Bold, [0.0; 3], "Z = 50 − j12 Ω (εr 13, σ 5 mS/m) ≤ 2:1 (x)");
        p.line((40.0, 70.0), (200.0, 70.0), 1.0, [1.0, 0.0, 0.0]);
        p.new_page();
        p.text(40.0, 60.0, 12.0, Font::Regular, [0.0; 3], "page 2");
        let b = p.bytes("t");
        // the header's binary marker aside, the file is ASCII: offsets are byte offsets
        let s: String = b.iter().map(|&c| if c < 128 { c as char } else { '#' }).collect();
        assert!(s.starts_with("%PDF-1.4") && s.ends_with("%%EOF\n"));
        assert!(s.contains("/Count 2") && s.contains("/F3 ") && s.contains("\\(x\\)"));
        // xref offsets point at the objects
        let xref_at: usize = s.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(s[xref_at..].starts_with("xref"));
        for (k, line) in s[xref_at..].lines().skip(3).take(5).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(s[off..].starts_with(&format!("{} 0 obj", k + 1)), "object {}", k + 1);
        }
    }
}
