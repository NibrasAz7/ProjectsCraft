//! `projectcraft` (L7) — ProjectCraft Desktop Studio.
//!
//!   projectcraft [--control PORT] [file.projectcraft]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod control_server;

use projectcraft_ui_egui::{ProjectApp, ProjectAppOptions};

const USAGE: &str = "ProjectCraft Studio\n\nusage: projectcraft [--control PORT] [file]\n\n  --control PORT  JSON-lines control channel on 127.0.0.1:PORT\n  file            open .projectcraft/.csv/.xml/.xer/.json on startup\n";

fn main() -> eframe::Result<()> {
    let mut port: Option<u16> = None;
    let mut file: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => {
                port = args.next().and_then(|s| s.parse::<u16>().ok());
                if port.is_none() {
                    eprintln!("--control needs a port number");
                }
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            s if !s.starts_with('-') && file.is_none() => file = Some(std::path::PathBuf::from(s)),
            s => eprintln!("unknown argument `{s}`"),
        }
    }
    let native = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]), ..Default::default() };
    eframe::run_native(
        "ProjectCraft",
        native,
        Box::new(move |cc| {
            let rx = port.map(|p| control_server::spawn(p, cc.egui_ctx.clone()));
            let opts = ProjectAppOptions { control_rx: rx, startup_file: file.clone() };
            Ok(Box::new(ProjectApp::new(cc, opts)))
        }),
    )
}
