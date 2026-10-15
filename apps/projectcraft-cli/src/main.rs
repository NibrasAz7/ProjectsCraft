//! `projectcraft-cli` (L7) — Headless ProjectCraft CLI.
//!
//!   projectcraft-cli demo [--cmd 'id {json}']…         load sample Orion program, then run commands
//!   projectcraft-cli run <file> [--cmd 'id {json}']…   open project (.projectcraft/.csv/.xml/.xer/.json), then run commands
//!   projectcraft-cli schedule [file]                   run CPM and print critical-path schedule
//!   projectcraft-cli evm [file]                        compute Earned Value Management metrics
//!   projectcraft-cli risk [file]                       run Monte Carlo schedule-risk simulation
//!   projectcraft-cli commands                          list all registered engine commands
//!   projectcraft-cli mcp                               serve MCP (JSON-RPC 2.0 on stdio)
//!   projectcraft-cli --cmd 'id {json}'…                run commands on a seeded session
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::process::ExitCode;

use projectcraft_engine::Session;
use serde_json::{json, Value as Json};

const USAGE: &str = "\
projectcraft-cli — headless ProjectCraft

USAGE:
  projectcraft-cli demo [--cmd 'id {json}']…         load sample Orion launch program & run commands
  projectcraft-cli run <file> [--cmd 'id {json}']…   open project (.projectcraft/.csv/.xml/.xer/.json) & run commands
  projectcraft-cli schedule [file]                   run CPM and print critical-path summary
  projectcraft-cli evm [file]                        compute ANSI/EIA-748 Earned Value Management
  projectcraft-cli risk [file]                       run Monte Carlo schedule-risk simulation
  projectcraft-cli commands                          list all engine commands
  projectcraft-cli mcp                               serve MCP (JSON-RPC 2.0 on stdio)
  projectcraft-cli --cmd 'id {json}'…                execute engine commands directly
  projectcraft-cli --version                         print version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("demo") => {
            let (_, cmds) = match split_cmds(args.get(1..).unwrap_or(&[])) {
                Ok(v) => v,
                Err(e) => return fail(&e),
            };
            let mut session = Session::new();
            if cmds.is_empty() {
                match session.execute("project.summary", json!({})) {
                    Ok(v) => println!("{v}"),
                    Err(e) => return fail(&e.to_string()),
                }
                ExitCode::SUCCESS
            } else {
                run_cmds(&mut session, &cmds)
            }
        }
        Some("run") => {
            let (input, cmds) = match split_cmds(args.get(1..).unwrap_or(&[])) {
                Ok(v) => v,
                Err(e) => return fail(&e),
            };
            let Some(path) = input.first() else {
                return fail("run needs a project file path");
            };
            let mut session = Session::new();
            if let Err(e) = session.execute("project.open", json!({"path": path})) {
                return fail(&e.to_string());
            }
            if cmds.is_empty() {
                match session.execute("project.summary", json!({})) {
                    Ok(v) => println!("{v}"),
                    Err(e) => return fail(&e.to_string()),
                }
                ExitCode::SUCCESS
            } else {
                run_cmds(&mut session, &cmds)
            }
        }
        Some("schedule") => run_single("schedule.critical_path", args.get(1)),
        Some("evm") => run_single("evm.compute", args.get(1)),
        Some("risk") => run_single("schedule.monte_carlo", args.get(1)),
        Some("--cmd") => {
            let (_, cmds) = match split_cmds(&args) {
                Ok(v) => v,
                Err(e) => return fail(&e),
            };
            let mut session = Session::new();
            run_cmds(&mut session, &cmds)
        }
        Some("commands") => {
            let session = Session::new();
            for c in session.commands() {
                println!("{:>24}  {:<52} {}", c.id, c.label, c.params);
            }
            ExitCode::SUCCESS
        }
        Some("mcp") => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut server = projectcraft_mcp::Server::new();
            match server.serve(stdin.lock(), stdout.lock()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&format!("mcp: {e}")),
            }
        }
        Some("--version") | Some("-V") => {
            println!("projectcraft-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => fail(&format!("unknown subcommand '{other}'\n\n{USAGE}")),
    }
}

fn run_single(cmd_id: &str, optional_path: Option<&String>) -> ExitCode {
    let mut session = Session::new();
    if let Some(p) = optional_path {
        if let Err(e) = session.execute("project.open", json!({"path": p})) {
            return fail(&e.to_string());
        }
    }
    match session.execute(cmd_id, json!({})) {
        Ok(res) => {
            println!("{}", serde_json::to_string_pretty(&res).unwrap_or_else(|_| res.to_string()));
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

type Cmd = (String, Json);

fn split_cmds(args: &[String]) -> Result<(Vec<String>, Vec<Cmd>), String> {
    let mut input = Vec::new();
    let mut cmds = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--cmd" {
            let raw = args.get(i + 1).ok_or("--cmd needs a value: 'id {json}'")?;
            let (id, rest) = raw.split_once(char::is_whitespace).unwrap_or((raw.as_str(), "{}"));
            let params: Json = serde_json::from_str(rest.trim()).map_err(|e| format!("--cmd '{id}': bad JSON ({e})"))?;
            cmds.push((id.to_string(), params));
            i += 2;
        } else {
            input.push(arg.clone());
            i += 1;
        }
    }
    Ok((input, cmds))
}

fn run_cmds(session: &mut Session, cmds: &[Cmd]) -> ExitCode {
    for (id, params) in cmds {
        match session.execute(id, params.clone()) {
            Ok(result) => println!("{}", json!({"ok": true, "command": id, "result": result})),
            Err(e) => {
                println!("{}", json!({"ok": false, "command": id, "error": e.to_string()}));
                return fail(&format!("command '{id}' failed: {e}"));
            }
        }
    }
    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_cmds_parses_multiple_commands() {
        let args = vec![
            "project.projectcraft".to_string(),
            "--cmd".to_string(),
            "task.add {\"title\":\"Spec\",\"duration_days\":2}".to_string(),
            "--cmd".to_string(),
            "schedule.run {}".to_string(),
        ];
        let (pos, cmds) = split_cmds(&args).unwrap();
        assert_eq!(pos, vec!["project.projectcraft".to_string()]);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].0, "task.add");
        assert_eq!(cmds[1].0, "schedule.run");
    }
}
