//! `projectcraft-web` (L7) — ProjectCraft in the browser (`wasm32-unknown-unknown`).
//!
//! Runs [`projectcraft_ui_egui::ProjectApp`] inside `eframe::WebRunner`.
//! Build with `trunk build --release` from `apps/projectcraft-web` (or verify
//! with `cargo xtask wasm`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use projectcraft_ui_egui::{ProjectApp, ProjectAppOptions};
    use wasm_bindgen::JsCast as _;

    let _ = projectcraft_engine::Session::new;
    let _ = egui::Color32::BLACK;
    let _ = serde_json::Value::Null;

    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(canvas) = document.get_element_by_id("projectcraft_canvas").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            return;
        };
        let options = eframe::WebOptions::default();
        let result =
            eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(ProjectApp::new(cc, ProjectAppOptions::default()))))).await;
        if let Some(el) = document.get_element_by_id("projectcraft_loading") {
            if result.is_ok() {
                el.remove();
            } else {
                el.set_inner_html("<p>ProjectCraft requires a browser with WebGPU or WebGL2 enabled.</p>");
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("projectcraft-web targets wasm32-unknown-unknown; build with `trunk build --release` or check with `cargo xtask wasm`");
}
