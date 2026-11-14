//! File names as the user knows them, and safe names to suggest when saving.
//!
//! Files opened through the Google Drive entry in the GNOME file dialog come from
//! the gvfs FUSE mount, where every file and folder is named by its Drive ID
//! (e.g. "1xP7v-AkPm…") with no extension. The real name is a GIO attribute.

use std::path::Path;

/// The name the user sees for this file: the gvfs display name where there is
/// one, otherwise the last part of the path.
pub fn display_name(p: &Path) -> String {
    let own = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if on_gvfs(p) {
        if let Some(n) = gio_display_name(p) {
            return n;
        }
    }
    own
}

fn on_gvfs(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == "gvfs")
}

fn gio_display_name(p: &Path) -> Option<String> {
    let out = std::process::Command::new("gio").args(["info", "-a", "standard::display-name"]).arg(p).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_gio_info(&String::from_utf8_lossy(&out.stdout))
}

/// Picks the display name out of `gio info -a standard::display-name` output.
fn parse_gio_info(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("standard::display-name:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The kind of antenna file, from the extension of its (display) name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Maa,
    Nec,
    Ez,
}

pub fn kind(name: &str) -> Option<Kind> {
    let ext = Path::new(name).extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "maa" => Some(Kind::Maa),
        "nec" => Some(Kind::Nec),
        "ez" => Some(Kind::Ez),
        _ => None,
    }
}

/// `name` without its extension.
pub fn stem(name: &str) -> String {
    Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file name that's allowed on Windows, macOS and Linux: characters such as
/// / \ : * ? " < > | and control characters are dropped, as are leading and
/// trailing spaces and dots. Gives "untitled" when nothing is left.
pub fn safe(s: &str) -> String {
    let kept: String = s.chars().filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c)).collect();
    let t = kept.trim_matches(|c: char| c.is_whitespace() || c == '.').to_string();
    if t.is_empty() {
        return "untitled".into();
    }
    // Windows won't create these names whatever the extension
    let upper = t.to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || ((upper.starts_with("COM") || upper.starts_with("LPT")) && upper.len() == 4 && upper.as_bytes()[3].is_ascii_digit());
    if reserved { format!("{t}_") } else { t }
}

/// The name Save As offers: the opened file's own name, else the antenna's,
/// made safe, with the given extension.
pub fn suggested(opened: Option<&str>, antenna: &str, ext: &str) -> String {
    let base = opened.map(stem).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| antenna.to_string());
    format!("{}.{ext}", safe(&base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_display_name_from_gio() {
        let out = "display name: T2LT_21.1MHz.maa\nuri: google-drive://x/1abc\nattributes:\n  standard::display-name: T2LT_21.1MHz.maa\n";
        assert_eq!(parse_gio_info(out).as_deref(), Some("T2LT_21.1MHz.maa"));
        assert_eq!(parse_gio_info("attributes:\n"), None);
    }

    #[test]
    fn gvfs_paths_are_recognised() {
        assert!(on_gvfs(Path::new("/run/user/1000/gvfs/google-drive:host=x,user=y/0AB/1xP7v")));
        assert!(!on_gvfs(Path::new("/home/x/Antennas/gvfs.maa")));
        // an ordinary file keeps its own name without asking gio
        assert_eq!(display_name(Path::new("/home/x/Antennas/T2LT 21.1MHz.maa")), "T2LT 21.1MHz.maa");
    }

    #[test]
    fn kind_comes_from_the_extension() {
        assert_eq!(kind("a.MAA"), Some(Kind::Maa));
        assert_eq!(kind("b.nec"), Some(Kind::Nec));
        assert_eq!(kind("c.Ez"), Some(Kind::Ez));
        assert_eq!(kind("1xP7v-AkPmBOqq6XmkfgeX_czSrsPwygk"), None);
    }

    #[test]
    fn unsafe_characters_are_removed() {
        assert_eq!(safe("40m: dipole / \"inverted V\"?"), "40m dipole  inverted V");
        assert_eq!(safe("  ..\u{7}  "), "untitled");
        assert_eq!(safe("con"), "con_");
        assert_eq!(safe("COM3"), "COM3_");
        assert_eq!(safe("Combo"), "Combo");
        assert_eq!(safe("21.1 MHz T2LT"), "21.1 MHz T2LT");
    }

    #[test]
    fn suggests_the_opened_file_name_first() {
        assert_eq!(suggested(Some("Banana_T2LT_7.04MHz.maa"), "7.04MHz T2LT", "nec"), "Banana_T2LT_7.04MHz.nec");
        assert_eq!(suggested(None, "28.5 MHz Bazooka", "maa"), "28.5 MHz Bazooka.maa");
        assert_eq!(suggested(None, "a<b>|c", "maa"), "abc.maa");
        assert_eq!(suggested(None, "", "ez"), "untitled.ez");
    }
}
