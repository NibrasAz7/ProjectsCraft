//! xtask — repo task runner. `cargo xtask ci` gates every commit.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "ci".into());
    match task.as_str() {
        "ci" => ci(),
        "fmt" => run(&["fmt", "--all", "--", "--check"]),
        "test" => run(&["test", "--workspace"]),
        "clippy" => run(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        "layers" => layers(),
        "wasm" => wasm(),
        other => {
            eprintln!("unknown xtask '{other}' (have: ci, fmt, test, clippy, layers, wasm)");
            ExitCode::FAILURE
        }
    }
}

fn ci() -> ExitCode {
    let steps: [&[&str]; 3] =
        [&["fmt", "--all", "--", "--check"], &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], &["test", "--workspace"]];
    for args in steps {
        if run(args) != ExitCode::SUCCESS {
            return ExitCode::FAILURE;
        }
    }
    if layers() != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    wasm()
}

fn wasm() -> ExitCode {
    run(&[
        "check",
        "--target",
        "wasm32-unknown-unknown",
        "-p",
        "pmcore",
        "-p",
        "pmsched",
        "-p",
        "pmformat",
        "-p",
        "pmsync",
        "-p",
        "projectcraft_engine",
        "-p",
        "projectcraft-ui-egui",
        "-p",
        "projectcraft-web",
    ])
}

fn run(args: &[&str]) -> ExitCode {
    let status = Command::new("cargo").args(args).status();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("cargo {} failed ({s})", args.join(" "));
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("could not run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Layer enforcement: a crate may only depend on crates at lower layers,
/// and nothing below L6 may depend on UI/toolkit crates.
fn layers() -> ExitCode {
    let layer = |name: &str| -> Option<u8> {
        Some(match name {
            "pmcore" => 0,
            "pmsched" => 1,
            "pmformat" => 2,
            "pmsync" => 3,
            "engine" | "projectcraft_engine" | "projectcraft-engine" => 5,
            "mcp" | "ui-egui" | "projectcraft_mcp" | "projectcraft-mcp" | "projectcraft_ui_egui" | "projectcraft-ui-egui" => 6,
            n if n.ends_with("-cli") || n.ends_with("-sync") || n == "projectcraft" || n == "projectcraft-web" => 7,
            _ => return None,
        })
    };
    const UI_DEPS: &[&str] = &["egui", "eframe", "winit", "wgpu", "rfd"];

    let roots = ["crates", "apps"];
    let mut ok = true;
    for root in roots {
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
            let pkg = entry.file_name().to_string_lossy().to_string();
            let Some(my_layer) = layer(&pkg) else { continue };
            let mut in_deps = false;
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with('[') {
                    in_deps = t.starts_with("[dependencies") || t.starts_with("[dev-dependencies");
                }
                if !in_deps {
                    continue;
                }
                let dep = t.split('=').next().unwrap_or("").trim();
                if dep.is_empty() {
                    continue;
                }
                if let Some(dep_layer) = layer(dep) {
                    if dep_layer >= my_layer {
                        eprintln!("layers: {pkg} (L{my_layer}) may not depend on {dep} (L{dep_layer})");
                        ok = false;
                    }
                } else if my_layer < 6 && UI_DEPS.contains(&dep) {
                    eprintln!("layers: {pkg} (L{my_layer}) may not depend on UI crate {dep}");
                    ok = false;
                }
            }
        }
    }
    if ok {
        println!("layers: ok");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
