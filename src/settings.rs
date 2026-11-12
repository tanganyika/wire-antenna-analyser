//! Program settings kept between runs in a small `key=value` file:
//! ~/.config/wireant/settings.txt, %APPDATA%\wireant\settings.txt or
//! ~/Library/Application Support/wireant/settings.txt (`WIREANT_CONFIG_DIR` overrides the folder).
//! Engines read them from any thread through [`get`]; the Preferences dialog changes them with [`set`].

use crate::model::Engine;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Folder last used to open or save an antenna.
    pub last_dir: Option<PathBuf>,
    /// Engine chosen at start-up (None = wiremom). `WIREANT_ENGINE` overrides it.
    pub default_engine: Option<Engine>,
    /// An external NEC program (NEC-4, NEC-5 or another NEC-2 build) the user has a copy of.
    pub nec_path: String,
    /// Its arguments; `{in}` and `{out}` become the input and output file names.
    pub nec_args: String,
    /// Type the input and output file names on its standard input, one per line,
    /// for programs that ask for them instead of taking arguments.
    pub nec_stdin: bool,
    /// How many copies may run at once in sweeps and the optimizer (0 = one per processor thread).
    pub nec_jobs: usize,
}

pub const DEFAULT_NEC_ARGS: &str = "{in} {out}";

impl Default for Settings {
    fn default() -> Self {
        Settings { last_dir: None, default_engine: None, nec_path: String::new(), nec_args: DEFAULT_NEC_ARGS.into(), nec_stdin: false, nec_jobs: 0 }
    }
}

pub fn engine_key(e: Engine) -> &'static str {
    match e {
        Engine::Wiremom => "wiremom",
        Engine::Mininec => "mininec",
        Engine::Nec2 => "nec2",
        Engine::External => "external",
    }
}

pub fn engine_from_key(s: &str) -> Option<Engine> {
    match s.trim().to_ascii_lowercase().as_str() {
        "wiremom" => Some(Engine::Wiremom),
        "mininec" => Some(Engine::Mininec),
        "nec2" | "nec" | "nec-2" => Some(Engine::Nec2),
        "external" => Some(Engine::External),
        _ => None,
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "last_dir" => s.last_dir = Some(PathBuf::from(v)).filter(|p| !p.as_os_str().is_empty()),
                "default_engine" => s.default_engine = engine_from_key(v),
                "nec_path" => s.nec_path = v.trim().to_string(),
                "nec_args" => s.nec_args = v.trim().to_string(),
                "nec_stdin" => s.nec_stdin = v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"),
                "nec_jobs" => s.nec_jobs = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    pub fn format(&self) -> String {
        let mut t = String::new();
        if let Some(d) = &self.last_dir {
            t += &format!("last_dir={}\n", d.display());
        }
        if let Some(e) = self.default_engine {
            t += &format!("default_engine={}\n", engine_key(e));
        }
        t += &format!("nec_path={}\nnec_args={}\nnec_stdin={}\nnec_jobs={}\n", self.nec_path, self.nec_args, self.nec_stdin as u8, self.nec_jobs);
        t
    }

    /// Whether an external NEC program has been set and exists.
    pub fn external_ready(&self) -> bool {
        !self.nec_path.trim().is_empty() && std::path::Path::new(self.nec_path.trim()).is_file()
    }
}

pub fn path() -> Option<PathBuf> {
    // ~/.config, %APPDATA%, ~/Library/Application Support
    let base = match std::env::var_os("WIREANT_CONFIG_DIR") {
        Some(d) => PathBuf::from(d),
        None => dirs::config_dir()?.join("wireant"),
    };
    Some(base.join("settings.txt"))
}

fn store() -> &'static RwLock<Settings> {
    static S: OnceLock<RwLock<Settings>> = OnceLock::new();
    S.get_or_init(|| {
        let s = path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Settings::parse(&t)).unwrap_or_default();
        RwLock::new(s)
    })
}

pub fn get() -> Settings {
    store().read().map(|s| s.clone()).unwrap_or_default()
}

/// Changes the settings and writes the file.
pub fn set(f: impl FnOnce(&mut Settings)) {
    let text = {
        let Ok(mut s) = store().write() else { return };
        let before = s.clone();
        f(&mut s);
        if *s == before {
            return;
        }
        s.format()
    };
    if let Some(p) = path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings {
            last_dir: Some(PathBuf::from("/home/x/Antennas")),
            default_engine: Some(Engine::External),
            nec_path: "/opt/nec5/nec5 x".into(),
            nec_args: "-i {in} -o {out}".into(),
            nec_stdin: true,
            nec_jobs: 4,
        };
        assert_eq!(Settings::parse(&s.format()), s);
        // the old file held only last_dir
        let old = Settings::parse("last_dir=/tmp\n");
        assert_eq!(old.last_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(old.nec_args, DEFAULT_NEC_ARGS);
    }
}
