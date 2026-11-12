//! Help → Help topics: the pages in docs/help/*.md, built into the program and shown in a
//! window with a topic list. The pages are plain Markdown so they read the same on GitHub;
//! only the parts they use are rendered: headings, paragraphs, bullet and numbered lists,
//! tables, code blocks, **bold**, `code` and links (to another page as `name.md`, or a web address).

use eframe::egui::{self, Color32, RichText};

pub struct Topic {
    pub id: &'static str,
    pub text: &'static str,
}

macro_rules! topic {
    ($id:literal) => {
        Topic { id: $id, text: include_str!(concat!("../docs/help/", $id, ".md")) }
    };
}

/// In the order the topic list shows them.
pub const TOPICS: &[Topic] = &[
    topic!("getting-started"),
    topic!("geometry"),
    topic!("view"),
    topic!("calculate"),
    topic!("patterns"),
    topic!("sweep"),
    topic!("optimizer"),
    topic!("tools"),
    topic!("ground"),
    topic!("engines"),
    topic!("files"),
];

/// The page's `# ` heading.
pub fn title(t: &Topic) -> &'static str {
    t.text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or(t.id).trim()
}

pub fn index_of(id: &str) -> Option<usize> {
    TOPICS.iter().position(|t| t.id == id)
}

#[derive(Default)]
pub struct HelpState {
    pub open: bool,
    pub topic: usize,
}

impl HelpState {
    pub fn show(&mut self, id: &str) {
        self.topic = index_of(id).unwrap_or(0);
        self.open = true;
    }
}

// ------------------------------------------------------------------ parsing

#[derive(Debug, PartialEq)]
enum Block {
    H1(String),
    H2(String),
    Para { indent: usize, text: String },
    /// `marker` is "•" or "1." etc.
    Item { indent: usize, marker: String, text: String },
    Table(Vec<Vec<String>>),
    Code(String),
}

fn indent_of(line: &str) -> usize {
    (line.len() - line.trim_start().len()) / 2
}

/// "- x" or "1. x" → (marker, rest).
fn list_marker(s: &str) -> Option<(String, &str)> {
    if let Some(r) = s.strip_prefix("- ") {
        return Some(("•".into(), r));
    }
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(r) = s[digits..].strip_prefix(". ") {
            return Some((s[..digits + 1].to_string(), r));
        }
    }
    None
}

fn parse(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    // whether the last block may take continuation lines
    let mut open = false;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let s = line.trim();
        if s.is_empty() {
            open = false;
            continue;
        }
        if s.starts_with("```") {
            let mut code = vec![];
            for l in lines.by_ref() {
                if l.trim().starts_with("```") {
                    break;
                }
                code.push(l);
            }
            out.push(Block::Code(code.join("\n")));
            open = false;
            continue;
        }
        if let Some(h) = s.strip_prefix("# ") {
            out.push(Block::H1(h.into()));
            open = false;
            continue;
        }
        if let Some(h) = s.strip_prefix("## ") {
            out.push(Block::H2(h.into()));
            open = false;
            continue;
        }
        if s.starts_with('|') {
            let cells: Vec<String> = s.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
                continue; // the |---| line
            }
            match out.last_mut() {
                Some(Block::Table(rows)) if open => rows.push(cells),
                _ => out.push(Block::Table(vec![cells])),
            }
            open = true;
            continue;
        }
        let indent = indent_of(line);
        if let Some((marker, rest)) = list_marker(s) {
            out.push(Block::Item { indent, marker, text: rest.into() });
            open = true;
            continue;
        }
        if open {
            if let Some(Block::Para { text, .. } | Block::Item { text, .. }) = out.last_mut() {
                text.push(' ');
                text.push_str(s);
                continue;
            }
        }
        // an indented paragraph after a blank line lines up with the text of a list item
        out.push(Block::Para { indent, text: s.into() });
        open = true;
    }
    out
}

#[derive(Debug, PartialEq)]
enum Span {
    Text { s: String, bold: bool, code: bool },
    Link { s: String, target: String },
}

fn spans(text: &str) -> Vec<Span> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut bold = false;
    let mut rest = text;
    let flush = |cur: &mut String, out: &mut Vec<Span>, bold: bool| {
        if !cur.is_empty() {
            out.push(Span::Text { s: std::mem::take(cur), bold, code: false });
        }
    };
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("**") {
            flush(&mut cur, &mut out, bold);
            bold = !bold;
            rest = r;
        } else if c == '`' {
            if let Some(end) = rest[1..].find('`') {
                flush(&mut cur, &mut out, bold);
                out.push(Span::Text { s: rest[1..1 + end].into(), bold, code: true });
                rest = &rest[end + 2..];
            } else {
                cur.push(c);
                rest = &rest[1..];
            }
        } else if c == '[' {
            // [text](target)
            let link = rest.find("](").and_then(|m| rest[m..].find(')').map(|e| (m, m + e)));
            match link {
                Some((m, e)) if !rest[1..m].contains('[') => {
                    flush(&mut cur, &mut out, bold);
                    out.push(Span::Link { s: rest[1..m].replace("**", ""), target: rest[m + 2..e].into() });
                    rest = &rest[e + 1..];
                }
                _ => {
                    cur.push(c);
                    rest = &rest[1..];
                }
            }
        } else {
            cur.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    flush(&mut cur, &mut out, bold);
    out
}

// ------------------------------------------------------------------ drawing

const INDENT: f32 = 18.0;

/// Draws text with its bold, code and links, wrapping at the available width (or on one line
/// when `wrap` is false, for table cells). Returns the topic id of a page link that was clicked.
fn inline(ui: &mut egui::Ui, text: &str, wrap: bool) -> Option<String> {
    let mut go = None;
    let body = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
        let all = spans(text);
        // each word is its own label so the line can wrap between them; a word keeps the
        // space that follows it
        for sp in &all {
            let (s, link) = match sp {
                Span::Text { s, .. } => (s.as_str(), None),
                Span::Link { s, target } => (s.as_str(), Some(target.as_str())),
            };
            let mut words: Vec<String> = vec![];
            for (i, w) in s.split(' ').enumerate() {
                if i > 0 {
                    match words.last_mut() {
                        Some(last) => last.push(' '),
                        None => words.push(" ".into()),
                    }
                }
                if !w.is_empty() {
                    words.push(w.into());
                }
            }
            for w in words {
                match (sp, link) {
                    (Span::Text { bold, code, .. }, _) => {
                        let mut r = RichText::new(w);
                        if *code {
                            r = r.code();
                        }
                        if *bold {
                            r = r.strong();
                        }
                        ui.label(r);
                    }
                    (Span::Link { .. }, Some(t)) => {
                        if ui.link(w).clicked() {
                            if t.starts_with("http") {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(t));
                            } else {
                                go = Some(t.split('#').next().unwrap_or("").trim_end_matches(".md").to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    };
    if wrap {
        ui.horizontal_wrapped(body);
    } else {
        ui.horizontal(body);
    }
    go
}

fn page(ui: &mut egui::Ui, text: &str) -> Option<String> {
    let mut go = None;
    for b in parse(text) {
        let g = match &b {
            Block::H1(h) => {
                ui.heading(h);
                ui.add_space(4.0);
                None
            }
            Block::H2(h) => {
                ui.add_space(8.0);
                ui.label(RichText::new(h).size(17.0).strong());
                ui.add_space(2.0);
                None
            }
            Block::Para { indent, text } => {
                ui.add_space(4.0);
                let r = ui.horizontal_top(|ui| {
                    ui.add_space(*indent as f32 * INDENT);
                    ui.vertical(|ui| inline(ui, text, true)).inner
                });
                r.inner
            }
            Block::Item { indent, marker, text } => {
                ui.add_space(2.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(*indent as f32 * INDENT);
                    ui.add_sized([INDENT, 0.0], egui::Label::new(marker.as_str()));
                    ui.vertical(|ui| inline(ui, text, true)).inner
                })
                .inner
            }
            Block::Table(rows) => {
                ui.add_space(4.0);
                let mut g = None;
                egui::Grid::new(ui.next_auto_id()).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (i, row) in rows.iter().enumerate() {
                        for c in row {
                            if i == 0 {
                                ui.label(RichText::new(c.replace("**", "")).strong());
                            } else if let Some(t) = inline(ui, c, false) {
                                g = Some(t);
                            }
                        }
                        ui.end_row();
                    }
                });
                g
            }
            Block::Code(c) => {
                ui.add_space(4.0);
                egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
                    ui.label(RichText::new(c).monospace());
                });
                None
            }
        };
        if g.is_some() {
            go = g;
        }
    }
    go
}

pub fn window(ctx: &egui::Context, st: &mut HelpState) {
    if !st.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Help").open(&mut open).default_size([820.0, 600.0]).resizable(true).collapsible(false).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(180.0);
                for (i, t) in TOPICS.iter().enumerate() {
                    if ui.selectable_label(st.topic == i, title(t)).clicked() {
                        st.topic = i;
                    }
                }
                ui.add_space(12.0);
                ui.label(RichText::new("F1 opens the page for the tab you are on.").small().color(Color32::GRAY));
            });
            ui.separator();
            let t = &TOPICS[st.topic.min(TOPICS.len() - 1)];
            egui::ScrollArea::vertical().id_salt(("help_page", t.id)).auto_shrink([false, false]).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(640.0);
                    if let Some(id) = page(ui, t.text) {
                        if let Some(i) = index_of(&id) {
                            st.topic = i;
                        }
                    }
                });
            });
        });
    });
    if !open {
        st.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_parses_and_links_resolve() {
        for t in TOPICS {
            assert!(t.text.starts_with("# "), "{} has no title", t.id);
            for b in parse(t.text) {
                let text = match &b {
                    Block::Para { text, .. } | Block::Item { text, .. } => text.clone(),
                    Block::Table(rows) => rows.concat().join(" "),
                    _ => continue,
                };
                for sp in spans(&text) {
                    match sp {
                        Span::Text { s, code: false, .. } => assert!(!s.contains("**") && !s.contains('`'), "{}: unrendered markup in {s:?}", t.id),
                        Span::Link { target, .. } if !target.starts_with("http") => {
                            let id = target.split('#').next().unwrap().trim_end_matches(".md");
                            assert!(index_of(id).is_some(), "{}: link to missing page {target}", t.id);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[test]
    fn markdown_subset() {
        let b = parse("# T\n\n1. one\n   more\n2. two\n\n- a\n  - b\n\n  inner\n\n| h | i |\n|---|---|\n| x | y |\n\nplain\ntext\n");
        assert_eq!(
            b,
            vec![
                Block::H1("T".into()),
                Block::Item { indent: 0, marker: "1.".into(), text: "one more".into() },
                Block::Item { indent: 0, marker: "2.".into(), text: "two".into() },
                Block::Item { indent: 0, marker: "•".into(), text: "a".into() },
                Block::Item { indent: 1, marker: "•".into(), text: "b".into() },
                Block::Para { indent: 1, text: "inner".into() },
                Block::Table(vec![vec!["h".into(), "i".into()], vec!["x".into(), "y".into()]]),
                Block::Para { indent: 0, text: "plain text".into() },
            ]
        );
        assert_eq!(
            spans("a **b** `c` [d](x.md)."),
            vec![
                Span::Text { s: "a ".into(), bold: false, code: false },
                Span::Text { s: "b".into(), bold: true, code: false },
                Span::Text { s: " ".into(), bold: false, code: false },
                Span::Text { s: "c".into(), bold: false, code: true },
                Span::Text { s: " ".into(), bold: false, code: false },
                Span::Link { s: "d".into(), target: "x.md".into() },
                Span::Text { s: ".".into(), bold: false, code: false },
            ]
        );
    }
}
