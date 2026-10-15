//! Control-channel dispatch — ProjectCraft Studio TCP JSON-lines protocol.
//!
//! Supported methods:
//! - `engine.execute` `{command, params}` — run any engine command.
//! - `engine.list` / `commands` — list all registered engine commands.
//! - `ui.state` — active lens, selected task, task/resource counts, duration, EVM KPIs.
//! - `ui.render` `{path?}` — render Gantt SVG (`{ok, svg, bytes, path?}`).
//! - `ui.screenshot` `{path?}` — capture PNG of the Studio viewport on next frame.
//! - `ping` — `{ok: true, pong: true}`.

use std::sync::{Arc, Condvar, Mutex};

use pmformat::render_gantt_svg;
use pmsched::{compute_evm, compute_workload};
use serde_json::{json, Value as Json};

use crate::ProjectApp;

type Slot = Arc<(Mutex<Option<Json>>, Condvar)>;

pub struct ControlRequest {
    pub method: String,
    pub params: Json,
    slot: Slot,
}

pub struct PendingReply {
    slot: Slot,
}

impl ControlRequest {
    pub fn new(method: String, params: Json) -> (Self, PendingReply) {
        let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
        (ControlRequest { method, params, slot: slot.clone() }, PendingReply { slot })
    }

    pub fn reply(self, v: Json) {
        let (m, cv) = &*self.slot;
        if let Ok(mut g) = m.lock() {
            *g = Some(v);
        }
        cv.notify_one();
    }
}

impl PendingReply {
    pub fn wait(&self, dur: std::time::Duration) -> Json {
        let (m, cv) = &*self.slot;
        let Ok(mut g) = m.lock() else {
            return json!({"ok": false, "error": "reply lock poisoned"});
        };
        loop {
            if let Some(v) = g.take() {
                return v;
            }
            match cv.wait_timeout(g, dur) {
                Ok((g2, timed_out)) => {
                    g = g2;
                    if timed_out.timed_out() {
                        return json!({"ok": false, "error": "control request timed out"});
                    }
                }
                Err(_) => return json!({"ok": false, "error": "reply lock poisoned"}),
            }
        }
    }
}

pub fn handle(app: &mut ProjectApp, ctx: &egui::Context, req: ControlRequest) {
    let params = req.params.clone();
    let method = req.method.clone();
    match method.as_str() {
        "engine.execute" | "command" => {
            let cmd = params.get("command").and_then(Json::as_str).unwrap_or("").to_string();
            let p = params.get("params").cloned().unwrap_or(json!({}));
            let reply = app.exec_command(&cmd, p);
            req.reply(reply);
        }
        "engine.list" | "commands" => {
            let cmds: Vec<Json> = app.session.commands().iter().map(|c| json!({"id": c.id, "label": c.label, "params": c.params})).collect();
            req.reply(json!({"ok": true, "commands": cmds}));
        }
        "ping" => req.reply(json!({"ok": true, "pong": true})),
        "ui.state" => {
            let proj = app.session.project();
            let evm = compute_evm(proj, None);
            let wl = compute_workload(proj);
            let dur = proj.tasks.iter().map(|t| t.early_finish).fold(0.0_f64, f64::max);
            req.reply(json!({
                "ok": true,
                "app": "projectcraft",
                "lens": app.active_lens.label(),
                "project": proj.name,
                "tasks": proj.tasks.len(),
                "dependencies": proj.dependencies.len(),
                "resources": proj.resources.len(),
                "durationDays": dur,
                "spi": evm.spi,
                "cpi": evm.cpi,
                "overallocatedDays": wl.total_overallocated_person_days,
                "selectedTask": app.selected_task,
            }));
        }
        "ui.render" => {
            let svg = render_gantt_svg(app.session.project());
            let path = params.get("path").and_then(Json::as_str).map(str::to_string);
            let mut out = json!({
                "ok": true,
                "bytes": svg.len(),
                "svg": svg,
            });
            if let Some(p) = &path {
                match std::fs::write(p, out["svg"].as_str().unwrap_or_default()) {
                    Ok(()) => out["path"] = json!(p),
                    Err(e) => out = json!({"ok": false, "error": format!("cannot write {p}: {e}")}),
                }
            }
            req.reply(out);
        }
        "ui.screenshot" => {
            let path = params.get("path").and_then(Json::as_str).map(str::to_string);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            app.pending_shots.push((req, path));
            ctx.request_repaint();
        }
        other => req.reply(json!({"ok": false, "error": format!("unknown method `{other}`")})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectApp, ProjectAppOptions};

    fn dispatch(app: &mut ProjectApp, method: &str, params: Json) -> Json {
        let (req, pending) = ControlRequest::new(method.to_string(), params);
        handle(app, &egui::Context::default(), req);
        pending.wait(std::time::Duration::from_secs(5))
    }

    #[test]
    fn control_ping_state_and_execute() {
        let mut app = ProjectApp::with_options(ProjectAppOptions::default());
        assert_eq!(dispatch(&mut app, "ping", json!({}))["pong"], json!(true));
        let st = dispatch(&mut app, "ui.state", json!({}));
        assert_eq!(st["ok"], json!(true));
        assert_eq!(st["tasks"], json!(8));

        let r =
            dispatch(&mut app, "engine.execute", json!({"command": "task.add", "params": {"title": "Post-launch telemetry", "duration_days": 3.0}}));
        assert_eq!(r["ok"], json!(true));
        assert_eq!(app.session.project().tasks.len(), 9);
    }
}
