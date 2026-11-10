mod app;
mod maa;
mod model;
mod mom;
mod nec;
mod nec_file;
mod wm;
mod ez;
mod optimize;
mod plots;
mod tools;

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
            "--help" | "-h" => {
                println!("wireant [antenna.maa] [--tab geometry|view|calculate|far|sweep|optimizer] [--screenshot out.png]");
                return Ok(());
            }
            _ => open = Some(PathBuf::from(a)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 560.0])
            .with_title("Wire Antenna Analyser")
            .with_app_id("wireant"),
        ..Default::default()
    };
    eframe::run_native(
        "Wire Antenna Analyser",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open, tab, shot)))),
    )
}
