//! `projectcraft_mcp` (L6) — ProjectCraft Model Context Protocol (MCP) server.
//!
//! JSON-RPC 2.0 over newline-delimited stdio, exposing `execute`, `list_commands`,
//! `project_summary`, `schedule_run`, `evm_compute`, and `monte_carlo` tools plus
//! `projectcraft://project`, `projectcraft://schedule`, and `projectcraft://commands` resources.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io::{BufRead, Write};

use projectcraft_engine::Session;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "ProjectCraft is an open-source project-management and scheduling engine \
(Jira / monday.com / Asana / ClickUp / Microsoft Project class). Use `execute` to run any engine \
command (`task.add`, `dep.add`, `schedule.run`, `resource.level`, `evm.compute`, `schedule.monte_carlo`, \
`export.mspdi`, `export.xer`, `export.gantt_svg`), or call the convenience tools directly.";

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn tool_result(payload: Value) -> Value {
    json!({"content": [{"type": "text", "text": payload.to_string()}]})
}

fn tool_error(message: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": message.into()}], "isError": true})
}

pub struct Server {
    session: Session,
    initialized: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Server { session: Session::new(), initialized: false }
    }

    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if let Some(reply) = self.handle_line(&line) {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) if batch.is_empty() => Some(error(Value::Null, INVALID_REQUEST, "empty batch")),
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => self.handle(msg),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    fn handle(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(o) = &msg else {
            return Some(error(Value::Null, INVALID_REQUEST, "message must be an object"));
        };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            if method == "notifications/initialized" {
                self.initialized = true;
            }
            return None;
        };
        Some(match self.request(method, &params) {
            Ok(r) => response(id, r),
            Err((code, m)) => error(id, code, m),
        })
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "ping" => Ok(json!({})),
            "initialize" => {
                let client_v = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED.contains(&client_v) { client_v } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}, "resources": {}},
                    "serverInfo": {"name": "projectcraft", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => self.call_tool(params),
            "resources/list" => Ok(json!({"resources": [
                {"uri": "projectcraft://project", "name": "Project summary & tasks",
                 "description": "Current project metadata, WBS tasks, dependencies and resources",
                 "mimeType": "application/json"},
                {"uri": "projectcraft://commands", "name": "Command catalogue",
                 "description": "Every ProjectCraft engine command id with parameter signature",
                 "mimeType": "application/json"},
            ]})),
            "resources/read" => self.read_resource(params),
            _ => Err((METHOD_NOT_FOUND, format!("unknown method `{method}`"))),
        }
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "tools/call needs `name`".to_string()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match name {
            "execute" => {
                let command = args.get("command").and_then(Value::as_str).ok_or((INVALID_PARAMS, "execute needs `command`".to_string()))?;
                let cmd_params = args.get("params").cloned().unwrap_or(json!({}));
                match self.session.execute(command, cmd_params) {
                    Ok(r) => Ok(tool_result(r)),
                    Err(e) => Ok(tool_error(e.to_string())),
                }
            }
            "list_commands" => match self.session.execute("engine.list", json!({})) {
                Ok(r) => Ok(tool_result(r)),
                Err(e) => Ok(tool_error(e.to_string())),
            },
            "project_summary" => match self.session.execute("project.summary", json!({})) {
                Ok(r) => Ok(tool_result(r)),
                Err(e) => Ok(tool_error(e.to_string())),
            },
            "schedule_run" => match self.session.execute("schedule.run", json!({})) {
                Ok(r) => Ok(tool_result(r)),
                Err(e) => Ok(tool_error(e.to_string())),
            },
            "evm_compute" => match self.session.execute("evm.compute", args) {
                Ok(r) => Ok(tool_result(r)),
                Err(e) => Ok(tool_error(e.to_string())),
            },
            "monte_carlo" => match self.session.execute("schedule.monte_carlo", args) {
                Ok(r) => Ok(tool_result(r)),
                Err(e) => Ok(tool_error(e.to_string())),
            },
            _ => Ok(tool_error(format!("unknown tool `{name}`"))),
        }
    }

    fn read_resource(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let uri = params.get("uri").and_then(Value::as_str).ok_or((INVALID_PARAMS, "resources/read needs `uri`".to_string()))?;
        let payload = match uri {
            "projectcraft://project" => serde_json::to_value(self.session.project()).unwrap_or_else(|_| json!({})),
            "projectcraft://commands" => self.session.execute("engine.list", json!({})).unwrap_or_else(|_| json!({})),
            _ => return Err((INVALID_PARAMS, format!("unknown resource `{uri}`"))),
        };
        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": payload.to_string(),
            }]
        }))
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "execute",
            "description": "Execute any ProjectCraft engine command by id (`task.add`, `dep.add`, `schedule.run`, `resource.level`, `evm.compute`, `export.mspdi`, `export.xer`, etc.).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Engine command id"},
                    "params": {"type": "object", "description": "Command parameters object"}
                },
                "required": ["command"]
            }
        },
        {
            "name": "list_commands",
            "description": "List all available ProjectCraft engine commands and their parameter schemas.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "project_summary",
            "description": "Return current project schedule duration, critical path, EVM KPIs, and resource overallocations.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "schedule_run",
            "description": "Run CPM forward and backward pass on the active project.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "evm_compute",
            "description": "Compute Earned Value Management (PV, EV, AC, BAC, SV, CV, SPI, CPI, EAC, TCPI) at a status day.",
            "inputSchema": {
                "type": "object",
                "properties": {"status_day": {"type": "number"}}
            }
        },
        {
            "name": "monte_carlo",
            "description": "Run a three-point PERT Monte Carlo schedule-risk simulation (P10/P50/P80/P90 and criticality index).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trials": {"type": "integer"},
                    "seed": {"type": "integer"}
                }
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_initialize_and_execute_commands() {
        let mut srv = Server::new();
        let init = srv.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
        let v: Value = serde_json::from_str(&init).unwrap();
        assert_eq!(v["result"]["serverInfo"]["name"], "projectcraft");

        let call = srv.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_summary","arguments":{}}}"#).unwrap();
        let cv: Value = serde_json::from_str(&call).unwrap();
        assert!(cv["result"]["content"][0]["text"].as_str().unwrap().contains("Orion Edge Compute Launch"));
    }
}
