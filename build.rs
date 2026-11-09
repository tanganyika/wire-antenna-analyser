//! Compiles the bundled public-domain NEC-2 engine (nec2c 1.3.1, in nec2c/) into an executable
//! that the app embeds and runs as a separate process (one per calculation, in parallel).
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("nec2c");
    println!("cargo:rerun-if-changed=nec2c");
    println!("cargo:rerun-if-changed=tools/cc.sh");
    let src = dir.join("nec2c");
    let files: Vec<PathBuf> = std::fs::read_dir(&src)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "c").unwrap_or(false)).collect())
        .unwrap_or_default();
    let cc = std::env::var("NEC2C_CC").map(PathBuf::from).unwrap_or_else(|_| dir.join("tools/cc.sh"));
    let ok = !files.is_empty()
        && Command::new(&cc)
            .args(["-O2", "-w", "-DHAVE_CONFIG_H"])
            .arg(format!("-I{}", src.display()))
            .arg("-o")
            .arg(&out)
            .args(&files)
            .arg("-lm")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    if !ok {
        println!("cargo:warning=could not compile the bundled NEC-2 engine; the NEC-2 option will be unavailable");
        let _ = std::fs::write(&out, b"");
    }
}
