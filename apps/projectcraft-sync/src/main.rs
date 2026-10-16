//! `projectcraft-sync` (L7) — Self-hosted Local-First Collaboration Relay.
//!
//! A single-binary JSON-lines TCP mailbox relay implementing the team-mode
//! collaboration model from `vision.md`:
//! - Enforces `Owner` / `Editor` / `Commenter` / `Viewer` / `Guest` roles
//! - Persists `SyncBundle` envelopes and comments to a local JSON file
//! - Performs idempotent merge and post-merge semantic conflict inspection
//!
//! Usage:
//!   projectcraft-sync [--bind 127.0.0.1:7985] [--store mailbox.sync.json]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use pmsync::{SyncBundle, SyncRole};
use projectcraft_engine::Session;
use serde_json::{json, Value as Json};

const USAGE: &str = "\
projectcraft-sync — ProjectCraft self-hosted collaboration relay

USAGE:
  projectcraft-sync [--bind 127.0.0.1:7985] [--store mailbox.sync.json]
  projectcraft-sync --version
  projectcraft-sync --help

JSON-lines protocol methods:
  ping          {}
  push          {author, role, command, params}
  comment       {author, role, task_id, body}
  pull          {since_seq?: number}
  bundle        {}
";

const MAX_LINE: usize = 4 * 1024 * 1024;

pub struct RelayState {
    session: Session,
    bundle: SyncBundle,
    store_path: Option<PathBuf>,
}

impl RelayState {
    pub fn new(store_path: Option<PathBuf>) -> Self {
        let mut bundle = SyncBundle::new();
        if let Some(p) = &store_path {
            if let Ok(raw) = std::fs::read_to_string(p) {
                if let Ok(loaded) = serde_json::from_str::<SyncBundle>(&raw) {
                    bundle = loaded;
                }
            }
        }
        RelayState { session: Session::new(), bundle, store_path }
    }

    fn persist(&self) {
        if let Some(p) = &self.store_path {
            if let Ok(txt) = serde_json::to_string_pretty(&self.bundle) {
                let _ = std::fs::write(p, txt);
            }
        }
    }

    pub fn handle_message(&mut self, msg: &Json) -> Json {
        let id = msg.get("id").cloned().unwrap_or(Json::Null);
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        match method {
            "ping" => json!({"id": id, "ok": true, "pong": true, "head": self.bundle.head_change_id}),
            "push" => {
                let author = params.get("author").and_then(Json::as_str).unwrap_or("peer").to_string();
                let role = params.get("role").and_then(Json::as_str).map(SyncRole::parse).unwrap_or(SyncRole::Viewer);
                let command = params.get("command").and_then(Json::as_str).unwrap_or("").to_string();
                let cmd_params = params.get("params").cloned().unwrap_or_else(|| json!({}));

                let env = match self.bundle.record_command(author, role, command.clone(), cmd_params.clone()) {
                    Ok(e) => e,
                    Err(e) => return json!({"id": id, "ok": false, "error": e.to_string()}),
                };
                if let Err(e) = self.session.execute(&command, cmd_params) {
                    return json!({"id": id, "ok": false, "error": e.to_string()});
                }
                let conflicts = self.bundle.inspect_post_merge_conflicts(self.session.project_mut());
                self.persist();
                json!({
                    "id": id,
                    "ok": true,
                    "envelope": env,
                    "conflicts": conflicts,
                })
            }
            "comment" => {
                let author = params.get("author").and_then(Json::as_str).unwrap_or("peer").to_string();
                let role = params.get("role").and_then(Json::as_str).map(SyncRole::parse).unwrap_or(SyncRole::Commenter);
                let task_id = params.get("task_id").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(1);
                let body = params.get("body").and_then(Json::as_str).unwrap_or("").to_string();
                match self.bundle.add_comment(author, role, task_id, body) {
                    Ok(c) => {
                        self.persist();
                        json!({"id": id, "ok": true, "comment": c})
                    }
                    Err(e) => json!({"id": id, "ok": false, "error": e.to_string()}),
                }
            }
            "pull" => {
                let since = params.get("since_seq").and_then(Json::as_u64).unwrap_or(0);
                let envs: Vec<_> = self.bundle.envelopes.iter().filter(|e| e.seq > since).collect();
                let comments: Vec<_> = self.bundle.comments.iter().filter(|c| c.seq > since).collect();
                json!({
                    "id": id,
                    "ok": true,
                    "head": self.bundle.head_change_id,
                    "envelopes": envs,
                    "comments": comments,
                    "conflicts": self.bundle.conflicts,
                })
            }
            "bundle" => json!({"id": id, "ok": true, "bundle": self.bundle}),
            other => json!({"id": id, "ok": false, "error": format!("unknown relay method `{other}`")}),
        }
    }
}

fn main() -> ExitCode {
    let mut bind = String::from("127.0.0.1:7985");
    let mut store: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bind" => {
                if let Some(b) = args.next() {
                    bind = b;
                }
            }
            "--store" => {
                if let Some(s) = args.next() {
                    store = Some(Path::new(&s).to_path_buf());
                }
            }
            "--version" | "-V" => {
                println!("projectcraft-sync {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument `{other}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("projectcraft-sync failed to bind {bind}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("projectcraft-sync listening on {bind}");
    let state = Arc::new(Mutex::new(RelayState::new(store)));

    for stream in listener.incoming().flatten() {
        let st = Arc::clone(&state);
        let _ = std::thread::spawn(move || serve_client(stream, st));
    }
    ExitCode::SUCCESS
}

fn serve_client(stream: std::net::TcpStream, state: Arc<Mutex<RelayState>>) {
    let Ok(read_half) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) if n > MAX_LINE => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": "line too long"}));
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Json>(trimmed) {
            Ok(msg) => match state.lock() {
                Ok(mut guard) => guard.handle_message(&msg),
                Err(_) => json!({"ok": false, "error": "relay lock poisoned"}),
            },
            Err(e) => json!({"ok": false, "error": format!("bad JSON: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_enforces_roles_and_syncs_envelopes() {
        let mut relay = RelayState::new(None);
        let denied = relay.handle_message(&json!({
            "id": 1,
            "method": "push",
            "params": {
                "author": "guest1",
                "role": "guest",
                "command": "task.add",
                "params": {"title": "Unauthorized Task", "duration_days": 2.0}
            }
        }));
        assert_eq!(denied["ok"], json!(false));

        let ok_push = relay.handle_message(&json!({
            "id": 2,
            "method": "push",
            "params": {
                "author": "alice",
                "role": "editor",
                "command": "task.add",
                "params": {"title": "Synced Milestone", "duration_days": 0.0}
            }
        }));
        assert_eq!(ok_push["ok"], json!(true));

        let pull = relay.handle_message(&json!({"id": 3, "method": "pull", "params": {"since_seq": 0}}));
        assert_eq!(pull["ok"], json!(true));
        assert_eq!(pull["envelopes"].as_array().unwrap().len(), 1);
    }
}
