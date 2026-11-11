//! Compiles the bundled public-domain NEC-2 engine (nec2c 1.3.1, in nec2c/) into an executable
//! that the app embeds and runs as a separate process (one per calculation, in parallel).
//!
//! The C compiler is the one the `cc` crate picks for the target (cc/gcc on Linux, clang on
//! macOS, including the right -arch when cross-building), except on Windows: nec2c uses C99
//! complex numbers, which MSVC lacks, so it is built with MinGW's gcc (statically, so the
//! executable needs no MinGW DLLs).  NEC2C_CC overrides the compiler.  If none works, the app
//! is built without the NEC-2 engine (or, with WIREANT_REQUIRE_NEC2 set, fails).
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("nec2c");
    println!("cargo:rerun-if-changed=nec2c");
    println!("cargo:rerun-if-env-changed=NEC2C_CC");
    println!("cargo:rerun-if-env-changed=WIREANT_REQUIRE_NEC2");
    println!("cargo:rerun-if-changed=packaging/wireant.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // the program's icon in Explorer and the taskbar
        let mut res = winresource::WindowsResource::new();
        res.set_icon(&dir.join("packaging/wireant.ico").to_string_lossy());
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
    let src = dir.join("nec2c");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "c").unwrap_or(false)).collect())
        .unwrap_or_default();
    files.sort();
    let ok = !files.is_empty() && compile(&src, &files, &out);
    if !ok {
        // CI release builds must not quietly ship without NEC-2
        if std::env::var_os("WIREANT_REQUIRE_NEC2").is_some() {
            panic!("could not compile the bundled NEC-2 engine (nec2c)");
        }
        println!("cargo:warning=could not compile the bundled NEC-2 engine; the NEC-2 option will be unavailable");
        let _ = std::fs::write(&out, b"");
    }
}

fn compile(src: &Path, files: &[PathBuf], out: &Path) -> bool {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let mut cmd = if let Ok(cc) = std::env::var("NEC2C_CC") {
        Command::new(cc)
    } else if windows {
        Command::new("gcc")
    } else {
        cc::Build::new().cargo_metadata(false).get_compiler().to_command()
    };
    // gcc on Windows appends .exe to the output name; write that name and move it
    let target = if windows { out.with_extension("exe") } else { out.to_path_buf() };
    cmd.args(["-O2", "-w", "-DHAVE_CONFIG_H"]).arg(format!("-I{}", src.display())).arg("-o").arg(&target).args(files).arg("-lm");
    if windows {
        cmd.arg("-static");
    }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    ok && (target == out || std::fs::rename(&target, out).is_ok())
}
