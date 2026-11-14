// no console window behind the program on Windows
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod maa;
mod matching;
mod mcp;
mod model;
mod mom;
mod nec;
mod nec_file;
mod wm;
mod ez;
mod file_name;
mod help;
mod optimize;
mod plots;
mod progress;
mod tools;
mod pdf;
mod report;
mod settings;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut shot = None;
    let mut tab = None;
    let mut open = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => shot = args.next().map(PathBuf::from),
            "--tab" => tab = args.next().and_then(|t| app::Tab::from_name(&t)),
            "--mcp" => {
                // MCP server on stdin/stdout, no window
                if let Err(e) = mcp::run() {
                    eprintln!("wireant --mcp: {e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--help" | "-h" => {
                println!("wireant [antenna.maa] [--tab geometry|view|calculate|far|sweep|optimizer] [--screenshot out.png]");
                println!("wireant --mcp    run as an MCP server on stdin/stdout (no window)");
                return Ok(());
            }
            _ => open = Some(PathBuf::from(a)),
        }
    }
    // wgpu (DirectX 12 / Vulkan / Metal / OpenGL) by default; plain OpenGL (glow) when wgpu
    // can't start, e.g. old graphics drivers, virtual machines, remote desktop.
    let glow = std::env::var("WIREANT_RENDERER").map(|r| r.eq_ignore_ascii_case("glow")).unwrap_or(false);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 560.0])
            .with_title("wireant")
            .with_app_id("wireant"),
        renderer: if glow { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu },
        ..Default::default()
    };
    let result = eframe::run_native(
        "wireant",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open, tab, shot)))),
    );
    let Err(e) = result else { return Ok(()) };
    if !glow {
        // winit allows one event loop per process, so try OpenGL in a fresh one
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(status) = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("WIREANT_RENDERER", "glow").status() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }
    start_failed(&e);
    Err(e)
}

/// The program couldn't open its window: say why, as on Windows there is no console to see it.
fn start_failed(e: &eframe::Error) {
    let msg = format!(
        "wireant couldn't open its window:\n\n{e}\n\nThis usually means the graphics driver doesn't support \
         OpenGL 2 or newer (or DirectX 12 / Vulkan). Updating the graphics driver normally fixes it; \
         in a virtual machine, turn on 3D acceleration."
    );
    eprintln!("{msg}");
    if let Some(dir) = dirs::cache_dir() {
        let _ = std::fs::create_dir_all(dir.join("wireant"));
        let _ = std::fs::write(dir.join("wireant").join("startup-error.txt"), &msg);
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("wireant")
        .set_description(msg)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
