//! nec-stub: a stand-in for a NEC program (NEC-2, NEC-4, NEC-5). Point another program (such as
//! EZNEC Pro's NEC-5 option) at it instead of the real NEC. It calculates nothing: it records how
//! it was called and what it was given, then exits, so the calling program will report an error.
//!
//! Each run makes a new folder `nec-stub-log\<date>_<time>` on the Desktop (or next to this
//! program if there is no Desktop) holding:
//!   report.txt  the command line, working folder, typed answers and a list of the folder's files
//!   files\      copies of the input files: any argument or typed answer that names a file, and
//!               files in the working folder changed in the last 30 minutes
//!
//! Some NEC builds ask for their file names instead of taking arguments, so the stub also prints
//! two prompts and records anything typed (or piped) in reply, waiting a few seconds for each.

use std::fmt::Write as _;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_COPY: u64 = 20 * 1024 * 1024;
const RECENT: Duration = Duration::from_secs(30 * 60);
const ANSWER_WAIT: Duration = Duration::from_secs(5);

fn main() {
    let started = SystemTime::now();
    let mut r = String::new();
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let log = log_dir(started);
    let files = log.join("files");
    let _ = std::fs::create_dir_all(&files);

    let _ = writeln!(r, "nec-stub {} run at {} UTC", env!("CARGO_PKG_VERSION"), stamp(started, true));
    let _ = writeln!(r, "program:        {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default());
    let _ = writeln!(r, "working folder: {}", cwd.display());
    let _ = writeln!(r, "\narguments ({}):", args.len().saturating_sub(1));
    for (i, a) in args.iter().enumerate().skip(1) {
        let _ = writeln!(r, "  [{i}] {a:?}");
    }
    if let Some(raw) = raw_command_line() {
        let _ = writeln!(r, "\nraw command line, exactly as given (with its quotes):\n  {raw}");
    }

    // environment variables that look NEC-related (not the whole environment)
    let nec_env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| nec_related(k)).collect();
    if !nec_env.is_empty() {
        let _ = writeln!(r, "\nNEC-related environment variables:");
        for (k, v) in &nec_env {
            let _ = writeln!(r, "  {k}={v}");
        }
    }

    // typed or piped answers
    let mut answers = vec![];
    let stdin_kind = if std::io::stdin().is_terminal() { "a console window" } else { "a pipe, file or nothing" };
    let _ = writeln!(r, "\nstandard input is {stdin_kind}.");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    for prompt in ["ENTER INPUT FILE NAME: ", "ENTER OUTPUT FILE NAME: "] {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        match rx.recv_timeout(ANSWER_WAIT) {
            Ok(a) => {
                println!();
                answers.push(a);
            }
            Err(_) => {
                println!("(no answer)");
                break;
            }
        }
    }
    // anything else already waiting
    while let Ok(a) = rx.recv_timeout(Duration::from_millis(300)) {
        answers.push(a);
    }
    let _ = writeln!(r, "typed or piped answers ({}):", answers.len());
    for a in &answers {
        let _ = writeln!(r, "  {a:?}");
    }

    // files named by arguments or answers
    let mut copied: Vec<PathBuf> = vec![];
    let _ = writeln!(r, "\nfiles named in the arguments or answers:");
    let mut named_any = false;
    for s in args.iter().skip(1).chain(answers.iter()) {
        let full = |c: &Path| if c.is_absolute() { c.to_path_buf() } else { cwd.join(c) };
        // the whole string first; its words only if it isn't a file itself ("-i in.nec")
        let all = candidates(s);
        let found: Vec<PathBuf> = match all.first().map(|c| full(c)).filter(|p| p.is_file()) {
            Some(p) => vec![p],
            None => all.iter().skip(1).map(|c| full(c)).filter(|p| p.is_file()).collect(),
        };
        for p in &found {
            named_any = true;
            let _ = writeln!(r, "  {} ({} bytes) -> copied", p.display(), size(p));
            copy(p, &files, &mut copied);
        }
        if found.is_empty() && looks_like_path(s) {
            named_any = true;
            let _ = writeln!(r, "  {} (does not exist yet; probably the output file)", full(&all[0]).display());
        }
    }
    if !named_any {
        let _ = writeln!(r, "  (none)");
    }

    // the working folder
    let _ = writeln!(r, "\nworking folder contents (newest first; * = copied):");
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = std::fs::read_dir(&cwd)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| {
                    let m = e.metadata().ok();
                    (e.path(), m.as_ref().map(|m| m.len()).unwrap_or(0), m.and_then(|m| m.modified().ok()).unwrap_or(UNIX_EPOCH))
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| b.2.cmp(&a.2));
    for (p, len, modified) in &entries {
        let recent = started.duration_since(*modified).map(|d| d < RECENT).unwrap_or(true);
        if recent {
            copy(p, &files, &mut copied);
        }
        let star = if copied.iter().any(|c| c == p) { "*" } else { " " };
        let _ = writeln!(r, " {star} {:>10}  {}  {}", len, stamp(*modified, true), p.file_name().unwrap_or_default().to_string_lossy());
    }
    if entries.is_empty() {
        let _ = writeln!(r, "  (no files)");
    }

    let _ = writeln!(r, "\nThis is nec-stub, not NEC: no calculation was done, so the calling program will report an error.");
    let _ = writeln!(r, "log folder: {}", log.display());
    let _ = std::fs::write(log.join("report.txt"), r.replace('\n', "\r\n"));
    println!("nec-stub: no calculation done. What was received is in {}", log.display());
    eprintln!("nec-stub: this is a stand-in for NEC, not NEC itself.");
}

/// The command line as one string, quotes included (Windows keeps it; elsewhere there is none).
#[cfg(windows)]
fn raw_command_line() -> Option<String> {
    unsafe extern "system" {
        fn GetCommandLineW() -> *const u16;
    }
    // SAFETY: GetCommandLineW returns a pointer to the process's NUL-terminated command line,
    // valid for the life of the process.
    unsafe {
        let p = GetCommandLineW();
        if p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)))
    }
}

#[cfg(not(windows))]
fn raw_command_line() -> Option<String> {
    None
}

/// Where to keep this run's records.
fn log_dir(t: SystemTime) -> PathBuf {
    let mut desktops = vec![];
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        desktops.push(PathBuf::from(home).join("Desktop"));
    }
    if let Some(od) = std::env::var_os("OneDrive") {
        desktops.push(PathBuf::from(od).join("Desktop"));
    }
    let base = desktops.into_iter().find(|d| d.is_dir()).unwrap_or_else(|| {
        std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
    });
    let mut dir = base.join("nec-stub-log").join(stamp(t, false));
    let mut n = 2;
    while dir.exists() {
        dir = base.join("nec-stub-log").join(format!("{}_{n}", stamp(t, false)));
        n += 1;
    }
    dir
}

/// The ways a string might name a file: as a whole, and each word of it.
fn candidates(s: &str) -> Vec<PathBuf> {
    let t = s.trim().trim_matches('"');
    let mut v = vec![PathBuf::from(t)];
    for w in t.split([' ', ',', '=']) {
        let w = w.trim_matches('"');
        if !w.is_empty() && w != t {
            v.push(PathBuf::from(w));
        }
    }
    v
}

/// NEC_PATH, NEC4DIR, EZNEC_x: a part of the name (between underscores) that starts or ends
/// with NEC, so CONNECTION or WINECONFIGDIR don't count.
fn nec_related(name: &str) -> bool {
    name.to_ascii_uppercase().split('_').any(|p| p.starts_with("NEC") || p.ends_with("NEC"))
}

fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    s.contains('.') || s.contains('\\') || s.contains('/')
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn copy(p: &Path, into: &Path, done: &mut Vec<PathBuf>) {
    if done.iter().any(|d| d == p) || size(p) > MAX_COPY {
        return;
    }
    let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut dest = into.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = into.join(format!("{n}_{name}"));
        n += 1;
    }
    if std::fs::copy(p, &dest).is_ok() {
        done.push(p.to_path_buf());
    }
}

/// UTC date and time: "2026-11-12 16:05:09" (readable) or "2026-11-12_160509" (folder name).
fn stamp(t: SystemTime, readable: bool) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if readable { format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}") } else { format!("{y:04}-{m:02}-{d:02}_{h:02}{mi:02}{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_names() {
        for n in ["NEC_PATH", "NEC4DIR", "EZNEC_HOME", "nec5"] {
            assert!(nec_related(n), "{n}");
        }
        for n in ["MCP_CONNECTION_NONBLOCKING", "WINECONFIGDIR", "PATH"] {
            assert!(!nec_related(n), "{n}");
        }
    }

    #[test]
    fn stamps() {
        let t = UNIX_EPOCH + Duration::from_secs(1_794_499_509); // 2026-11-12 16:05:09 UTC
        assert_eq!(stamp(t, true), "2026-11-12 16:05:09");
        assert_eq!(stamp(t, false), "2026-11-12_160509");
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(951_782_400), true), "2000-02-29 00:00:00");
    }
}
