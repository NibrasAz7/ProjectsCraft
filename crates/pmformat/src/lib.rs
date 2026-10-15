//! `pmformat` (L2) — Interop & Headless Rendering for ProjectCraft.
//!
//! Supported formats:
//! - Native `.projectcraft` JSON (`to_projectcraft_json` / `from_projectcraft_json`)
//! - CSV schedule & WBS tables (`to_csv` / `from_csv`)
//! - Jira-compatible issue JSON (`to_jira_json` / `from_jira_json`)
//! - Microsoft Project MSPDI `.xml` (`to_mspdi_xml` / `from_mspdi_xml`)
//! - Primavera P6 `.xer` tabular exchange (`to_primavera_xer` / `from_primavera_xer`)
//! - Headless SVG Gantt chart (`render_gantt_svg`) and printable HTML executive report (`render_report_html`)
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::fmt::{self, Write as _};

use pmcore::{DepKind, ItemKind, Priority, Project, TaskStatus, MAX_TASKS};
use pmsched::{compute_evm, compute_workload, schedule_project};
use serde_json::{json, Value as Json};

/// Maximum input payload size (32 MiB) accepted from untrusted files.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// Format conversion or parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    Parse(String),
    Validation(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Parse(m) => write!(f, "format parse error: {m}"),
            FormatError::Validation(m) => write!(f, "format validation error: {m}"),
        }
    }
}

impl std::error::Error for FormatError {}

fn check_size(input: &str) -> Result<(), FormatError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(FormatError::Validation(format!("input exceeds maximum size of {MAX_INPUT_BYTES} bytes")));
    }
    Ok(())
}

/// Serialize `Project` to native `.projectcraft` pretty UTF-8 JSON.
pub fn to_projectcraft_json(project: &Project) -> Result<String, FormatError> {
    serde_json::to_string_pretty(project).map_err(|e| FormatError::Parse(e.to_string()))
}

/// Parse a native `.projectcraft` JSON document, validating its header and repairing counters.
pub fn from_projectcraft_json(src: &str) -> Result<Project, FormatError> {
    check_size(src)?;
    let mut project: Project = serde_json::from_str(src).map_err(|e| FormatError::Parse(e.to_string()))?;
    if project.format != "projectcraft" {
        return Err(FormatError::Validation(format!("expected format 'projectcraft', got '{}'", project.format)));
    }
    if project.tasks.len() > MAX_TASKS {
        return Err(FormatError::Validation("too many tasks in project file".to_string()));
    }
    project.repair_counters();
    let _ = schedule_project(&mut project);
    Ok(project)
}

/// Export project tasks and schedule columns to RFC-4180 CSV.
pub fn to_csv(project: &Project) -> String {
    let mut out = String::from(
        "id,key,wbs,title,kind,status,priority,duration_days,progress,early_start,early_finish,total_slack,critical,predecessors,fixed_cost,actual_cost\n",
    );
    for t in &project.tasks {
        let preds: Vec<String> = project
            .dependencies
            .iter()
            .filter(|d| d.successor_id == t.id)
            .map(|d| {
                if d.lag_days.abs() < 1e-6 {
                    format!("{}{}", d.predecessor_id, d.kind.code())
                } else if d.lag_days > 0.0 {
                    format!("{}{}+{}", d.predecessor_id, d.kind.code(), d.lag_days)
                } else {
                    format!("{}{}{}", d.predecessor_id, d.kind.code(), d.lag_days)
                }
            })
            .collect();
        let pred_str = preds.join(";");
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{},{},{:.2},{:.2}",
            t.id,
            csv_escape(&t.key),
            csv_escape(&t.wbs),
            csv_escape(&t.title),
            t.kind.as_str(),
            t.status.as_str(),
            t.priority.as_str(),
            t.duration_days,
            t.progress,
            t.early_start,
            t.early_finish,
            t.total_slack,
            t.critical,
            csv_escape(&pred_str),
            t.fixed_cost,
            t.actual_cost
        );
    }
    out
}

/// Import a project from CSV (`id,key,wbs,title,kind,status,priority,duration_days,...`).
pub fn from_csv(src: &str, name: &str) -> Result<Project, FormatError> {
    check_size(src)?;
    let mut project = Project::new(name, "CSV");
    let mut lines = src.lines().filter(|l| !l.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Err(FormatError::Parse("empty CSV input".to_string()));
    };
    let headers: Vec<String> = split_csv_row(header_line).into_iter().map(|h| h.trim().to_ascii_lowercase()).collect();

    let col = |row: &[String], name: &str| -> Option<String> {
        let idx = headers.iter().position(|h| h == name)?;
        row.get(idx).map(|s| s.trim().to_string())
    };

    let mut pending_preds: Vec<(u32, String)> = Vec::new();
    for line in lines.take(MAX_TASKS) {
        let row = split_csv_row(line);
        let title = col(&row, "title").or_else(|| col(&row, "name")).or_else(|| col(&row, "summary")).unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let dur = col(&row, "duration_days").or_else(|| col(&row, "duration")).and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
        let wbs = col(&row, "wbs").filter(|s| !s.is_empty());
        let id = project.add_task(title, dur, wbs).map_err(|e| FormatError::Validation(e.to_string()))?;
        if let Some(explicit_id) = col(&row, "id").and_then(|s| s.parse::<u32>().ok()) {
            if let Some(t) = project.task_mut(id) {
                t.id = explicit_id;
            }
        }
        let final_id = project.tasks.last().map(|t| t.id).unwrap_or(id);
        if let Some(t) = project.task_mut(final_id) {
            if let Some(k) = col(&row, "key").filter(|s| !s.is_empty()) {
                t.key = k;
            }
            if let Some(kind) = col(&row, "kind").or_else(|| col(&row, "type")) {
                t.kind = ItemKind::parse(&kind);
            }
            if let Some(st) = col(&row, "status") {
                t.status = TaskStatus::parse(&st);
            }
            if let Some(pr) = col(&row, "priority") {
                t.priority = Priority::parse(&pr);
            }
            if let Some(prog) = col(&row, "progress").and_then(|s| s.parse::<f64>().ok()) {
                t.progress = if prog > 1.0 { (prog / 100.0).clamp(0.0, 1.0) } else { prog.clamp(0.0, 1.0) };
            }
            if let Some(fc) = col(&row, "fixed_cost").and_then(|s| s.parse::<f64>().ok()) {
                t.fixed_cost = fc.max(0.0);
            }
            if let Some(ac) = col(&row, "actual_cost").and_then(|s| s.parse::<f64>().ok()) {
                t.actual_cost = ac.max(0.0);
            }
        }
        if let Some(preds) = col(&row, "predecessors").filter(|s| !s.is_empty()) {
            pending_preds.push((final_id, preds));
        }
    }

    project.repair_counters();
    for (succ_id, preds) in pending_preds {
        for token in preds.split([';', ',']) {
            if let Some((pred_id, kind, lag)) = parse_pred_token(token.trim()) {
                let _ = project.add_dependency(pred_id, succ_id, kind, lag);
            }
        }
    }
    let _ = schedule_project(&mut project);
    Ok(project)
}

fn parse_pred_token(token: &str) -> Option<(u32, DepKind, f64)> {
    if token.is_empty() {
        return None;
    }
    let digits_end = token.find(|c: char| !c.is_ascii_digit()).unwrap_or(token.len());
    let pred_id = token.get(..digits_end)?.parse::<u32>().ok()?;
    let rest = token.get(digits_end..)?.trim();
    if rest.is_empty() {
        return Some((pred_id, DepKind::FinishToStart, 0.0));
    }
    let (kind_str, lag_str) = if rest.len() >= 2 && rest.get(..2).map(|s| s.chars().all(|c| c.is_ascii_alphabetic())).unwrap_or(false) {
        (rest.get(..2).unwrap_or("FS"), rest.get(2..).unwrap_or(""))
    } else {
        ("FS", rest)
    };
    let kind = DepKind::parse(kind_str);
    let lag = if lag_str.trim().is_empty() { 0.0 } else { lag_str.trim().parse::<f64>().unwrap_or(0.0) };
    Some((pred_id, kind, lag))
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', ';']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn split_csv_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                out.push(cur);
                cur = String::new();
            }
            other => cur.push(other),
        }
    }
    out.push(cur);
    out
}

/// Export a project to a Jira-compatible JSON export payload (`{"project": ..., "issues": [...]}`).
pub fn to_jira_json(project: &Project) -> String {
    let issues: Vec<Json> = project
        .tasks
        .iter()
        .map(|t| {
            let links: Vec<Json> = project
                .dependencies
                .iter()
                .filter(|d| d.successor_id == t.id)
                .map(|d| {
                    let pred_key =
                        project.task(d.predecessor_id).map(|p| p.key.clone()).unwrap_or_else(|| format!("{}-{}", project.code, d.predecessor_id));
                    json!({
                        "type": d.kind.code(),
                        "lag_days": d.lag_days,
                        "inwardIssue": { "id": d.predecessor_id, "key": pred_key }
                    })
                })
                .collect();
            json!({
                "id": t.id.to_string(),
                "key": t.key,
                "fields": {
                    "summary": t.title,
                    "description": t.description,
                    "wbs": t.wbs,
                    "issuetype": { "name": t.kind.as_str() },
                    "status": { "name": t.status.as_str() },
                    "priority": { "name": t.priority.as_str() },
                    "timeoriginalestimate": (t.duration_days * 8.0 * 3600.0).round() as i64,
                    "story_points": t.story_points,
                    "progress": t.progress,
                    "issuelinks": links,
                }
            })
        })
        .collect();
    json!({
        "project": {
            "key": project.code,
            "name": project.name,
            "description": project.description,
        },
        "issues": issues,
    })
    .to_string()
}

/// Import a project from a Jira JSON export (`{"issues": [...]}`).
pub fn from_jira_json(src: &str) -> Result<Project, FormatError> {
    check_size(src)?;
    let root: Json = serde_json::from_str(src).map_err(|e| FormatError::Parse(e.to_string()))?;
    let proj_name = root.pointer("/project/name").and_then(Json::as_str).unwrap_or("Imported Jira Project");
    let proj_key = root.pointer("/project/key").and_then(Json::as_str).unwrap_or("JIRA");
    let mut project = Project::new(proj_name, proj_key);

    let issues =
        root.get("issues").and_then(Json::as_array).ok_or_else(|| FormatError::Validation("missing `issues` array in Jira JSON".to_string()))?;

    let mut key_to_id = std::collections::BTreeMap::new();
    let mut pending_links: Vec<(u32, String, DepKind, f64)> = Vec::new();

    for (idx, issue) in issues.iter().take(MAX_TASKS).enumerate() {
        let fields = issue.get("fields").unwrap_or(issue);
        let summary = fields.get("summary").and_then(Json::as_str).unwrap_or("Untitled Issue");
        let secs = fields.get("timeoriginalestimate").and_then(Json::as_f64).unwrap_or(28_800.0);
        let dur_days = (secs / (8.0 * 3600.0)).max(0.0);
        let wbs = fields.get("wbs").and_then(Json::as_str).map(str::to_string).or_else(|| Some(format!("{}", idx + 1)));
        let id = project.add_task(summary, dur_days, wbs).map_err(|e| FormatError::Validation(e.to_string()))?;
        let key = issue.get("key").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| format!("{proj_key}-{id}"));
        key_to_id.insert(key.clone(), id);

        if let Some(t) = project.task_mut(id) {
            t.key = key;
            if let Some(desc) = fields.get("description").and_then(Json::as_str) {
                t.description = desc.to_string();
            }
            if let Some(it) = fields.pointer("/issuetype/name").and_then(Json::as_str) {
                t.kind = ItemKind::parse(it);
            }
            if let Some(st) = fields.pointer("/status/name").and_then(Json::as_str) {
                t.status = TaskStatus::parse(st);
            }
            if let Some(pr) = fields.pointer("/priority/name").and_then(Json::as_str) {
                t.priority = Priority::parse(pr);
            }
            t.story_points = fields.get("story_points").and_then(Json::as_f64);
            if let Some(prog) = fields.get("progress").and_then(Json::as_f64) {
                t.progress = prog.clamp(0.0, 1.0);
            }
        }

        if let Some(links) = fields.get("issuelinks").and_then(Json::as_array) {
            for link in links {
                if let Some(inward_key) = link.pointer("/inwardIssue/key").and_then(Json::as_str) {
                    let kind = link.get("type").and_then(Json::as_str).map(DepKind::parse).unwrap_or(DepKind::FinishToStart);
                    let lag = link.get("lag_days").and_then(Json::as_f64).unwrap_or(0.0);
                    pending_links.push((id, inward_key.to_string(), kind, lag));
                }
            }
        }
    }

    for (succ_id, pred_key, kind, lag) in pending_links {
        if let Some(&pred_id) = key_to_id.get(&pred_key) {
            let _ = project.add_dependency(pred_id, succ_id, kind, lag);
        }
    }
    project.repair_counters();
    let _ = schedule_project(&mut project);
    Ok(project)
}

/// Export a project to Microsoft Project MSPDI XML format (`.xml`).
pub fn to_mspdi_xml(project: &Project) -> String {
    let mut xml = String::new();
    let _ = writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(xml, "<Project xmlns=\"http://schemas.microsoft.com/project\">");
    let _ = writeln!(xml, "  <Name>{}</Name>", xml_escape(&project.name));
    let _ = writeln!(xml, "  <Title>{}</Title>", xml_escape(&project.name));
    let _ = writeln!(xml, "  <MinutesPerDay>480</MinutesPerDay>");
    let _ = writeln!(xml, "  <Tasks>");
    for t in &project.tasks {
        let hours = (t.duration_days * 8.0).round() as i64;
        let pct = (t.progress * 100.0).round() as i64;
        let milestone = i32::from(t.kind == ItemKind::Milestone || t.duration_days == 0.0);
        let _ = writeln!(xml, "    <Task>");
        let _ = writeln!(xml, "      <UID>{}</UID>", t.id);
        let _ = writeln!(xml, "      <ID>{}</ID>", t.id);
        let _ = writeln!(xml, "      <Name>{}</Name>", xml_escape(&t.title));
        let _ = writeln!(xml, "      <WBS>{}</WBS>", xml_escape(&t.wbs));
        let _ = writeln!(xml, "      <Duration>PT{hours}H0M0S</Duration>");
        let _ = writeln!(xml, "      <PercentComplete>{pct}</PercentComplete>");
        let _ = writeln!(xml, "      <Milestone>{milestone}</Milestone>");
        let _ = writeln!(xml, "      <FixedCost>{:.2}</FixedCost>", t.fixed_cost);
        let _ = writeln!(xml, "      <ActualCost>{:.2}</ActualCost>", t.actual_cost);
        for d in project.dependencies.iter().filter(|d| d.successor_id == t.id) {
            let mspdi_type = match d.kind {
                DepKind::FinishToFinish => 0,
                DepKind::FinishToStart => 1,
                DepKind::StartToFinish => 2,
                DepKind::StartToStart => 3,
            };
            // MSPDI LinkLag is in tenths of a minute (1 day = 480 min = 4800 tenths)
            let link_lag = (d.lag_days * 4800.0).round() as i64;
            let _ = writeln!(xml, "      <PredecessorLink>");
            let _ = writeln!(xml, "        <PredecessorUID>{}</PredecessorUID>", d.predecessor_id);
            let _ = writeln!(xml, "        <Type>{mspdi_type}</Type>");
            let _ = writeln!(xml, "        <LinkLag>{link_lag}</LinkLag>");
            let _ = writeln!(xml, "      </PredecessorLink>");
        }
        let _ = writeln!(xml, "    </Task>");
    }
    let _ = writeln!(xml, "  </Tasks>");
    let _ = writeln!(xml, "  <Resources>");
    for r in &project.resources {
        let _ = writeln!(xml, "    <Resource>");
        let _ = writeln!(xml, "      <UID>{}</UID>", r.id);
        let _ = writeln!(xml, "      <Name>{}</Name>", xml_escape(&r.name));
        let _ = writeln!(xml, "      <Group>{}</Group>", xml_escape(&r.role));
        let _ = writeln!(xml, "      <MaxUnits>{:.2}</MaxUnits>", r.max_units);
        let _ = writeln!(xml, "      <StandardRate>{:.2}</StandardRate>", r.standard_rate_per_day / 8.0);
        let _ = writeln!(xml, "    </Resource>");
    }
    let _ = writeln!(xml, "  </Resources>");
    let _ = writeln!(xml, "  <Assignments>");
    for (idx, a) in project.assignments.iter().enumerate() {
        let _ = writeln!(xml, "    <Assignment>");
        let _ = writeln!(xml, "      <UID>{}</UID>", idx + 1);
        let _ = writeln!(xml, "      <TaskUID>{}</TaskUID>", a.task_id);
        let _ = writeln!(xml, "      <ResourceUID>{}</ResourceUID>", a.resource_id);
        let _ = writeln!(xml, "      <Units>{:.2}</Units>", a.units);
        let _ = writeln!(xml, "    </Assignment>");
    }
    let _ = writeln!(xml, "  </Assignments>");
    let _ = writeln!(xml, "</Project>");
    xml
}

/// Import a project from Microsoft Project MSPDI XML (`.xml`).
pub fn from_mspdi_xml(src: &str) -> Result<Project, FormatError> {
    check_size(src)?;
    let proj_name = extract_tag(src, "Title").or_else(|| extract_tag(src, "Name")).unwrap_or_else(|| "MSPDI Project".to_string());
    let mut project = Project::new(proj_name, "MSP");

    let mut pending_deps: Vec<(u32, u32, DepKind, f64)> = Vec::new();
    for task_block in extract_blocks(src, "Task").into_iter().take(MAX_TASKS) {
        let Some(uid) = extract_tag(task_block, "UID").and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if uid == 0 {
            // UID 0 in MSPDI is the project summary root node
            continue;
        }
        let name = extract_tag(task_block, "Name").unwrap_or_else(|| format!("Task {uid}"));
        let wbs = extract_tag(task_block, "WBS").unwrap_or_else(|| uid.to_string());
        let dur_days = extract_tag(task_block, "Duration").map(|d| parse_iso8601_duration_days(&d)).unwrap_or(1.0);
        let pct = extract_tag(task_block, "PercentComplete").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let is_milestone = extract_tag(task_block, "Milestone").map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(dur_days == 0.0);

        let id = project.add_task(name, if is_milestone { 0.0 } else { dur_days }, Some(wbs)).map_err(|e| FormatError::Validation(e.to_string()))?;
        if let Some(t) = project.task_mut(id) {
            t.id = uid;
            t.key = format!("MSP-{uid}");
            t.progress = (pct / 100.0).clamp(0.0, 1.0);
            t.status = if t.progress >= 1.0 {
                TaskStatus::Done
            } else if t.progress > 0.0 {
                TaskStatus::InProgress
            } else {
                TaskStatus::Todo
            };
            t.fixed_cost = extract_tag(task_block, "FixedCost").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            t.actual_cost = extract_tag(task_block, "ActualCost").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        }

        for pred_block in extract_blocks(task_block, "PredecessorLink") {
            if let Some(pred_uid) = extract_tag(pred_block, "PredecessorUID").and_then(|s| s.parse::<u32>().ok()) {
                let kind = match extract_tag(pred_block, "Type").and_then(|s| s.parse::<i32>().ok()).unwrap_or(1) {
                    0 => DepKind::FinishToFinish,
                    2 => DepKind::StartToFinish,
                    3 => DepKind::StartToStart,
                    _ => DepKind::FinishToStart,
                };
                let lag_tenths = extract_tag(pred_block, "LinkLag").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                pending_deps.push((pred_uid, uid, kind, lag_tenths / 4800.0));
            }
        }
    }

    for res_block in extract_blocks(src, "Resource") {
        let Some(uid) = extract_tag(res_block, "UID").and_then(|s| s.parse::<u32>().ok()) else { continue };
        if uid == 0 {
            continue;
        }
        let name = extract_tag(res_block, "Name").unwrap_or_else(|| format!("Resource {uid}"));
        let role = extract_tag(res_block, "Group").unwrap_or_else(|| "Member".to_string());
        let max_units = extract_tag(res_block, "MaxUnits").and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
        let hourly = extract_tag(res_block, "StandardRate").and_then(|s| s.parse::<f64>().ok()).unwrap_or(50.0);
        if let Ok(added_id) = project.add_resource(name, role, max_units, hourly * 8.0) {
            if let Some(r) = project.resources.iter_mut().find(|r| r.id == added_id) {
                r.id = uid;
            }
        }
    }

    project.repair_counters();
    for (pred, succ, kind, lag) in pending_deps {
        let _ = project.add_dependency(pred, succ, kind, lag);
    }
    for assign_block in extract_blocks(src, "Assignment") {
        let task_uid = extract_tag(assign_block, "TaskUID").and_then(|s| s.parse::<u32>().ok());
        let res_uid = extract_tag(assign_block, "ResourceUID").and_then(|s| s.parse::<u32>().ok());
        let units = extract_tag(assign_block, "Units").and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
        if let (Some(t), Some(r)) = (task_uid, res_uid) {
            let _ = project.assign_resource(t, r, units);
        }
    }
    let _ = schedule_project(&mut project);
    Ok(project)
}

fn parse_iso8601_duration_days(s: &str) -> f64 {
    // Handles PT40H0M0S or P5D
    let s = s.trim();
    if let Some(after_pt) = s.strip_prefix("PT") {
        if let Some(h_pos) = after_pt.find('H') {
            if let Ok(hours) = after_pt.get(..h_pos).unwrap_or("0").parse::<f64>() {
                return (hours / 8.0).max(0.0);
            }
        }
    }
    if let Some(after_p) = s.strip_prefix('P') {
        if let Some(d_pos) = after_p.find('D') {
            if let Ok(days) = after_p.get(..d_pos).unwrap_or("0").parse::<f64>() {
                return days.max(0.0);
            }
        }
    }
    1.0
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn extract_tag(src: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = src.find(&open)? + open.len();
    let end = src.get(start..)?.find(&close)? + start;
    Some(xml_unescape(src.get(start..end)?.trim()))
}

fn extract_blocks<'a>(src: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel_start) = src.get(cursor..).and_then(|s| s.find(&open)) {
        let body_start = cursor + rel_start + open.len();
        let Some(rel_end) = src.get(body_start..).and_then(|s| s.find(&close)) else {
            break;
        };
        let body_end = body_start + rel_end;
        if let Some(slice) = src.get(body_start..body_end) {
            out.push(slice);
        }
        cursor = body_end + close.len();
    }
    out
}

/// Export a project to Primavera P6 `.xer` tabular format (`PROJECT`, `TASK`, `TASKPRED`, `RSRC`, `TASKRSRC`).
pub fn to_primavera_xer(project: &Project) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "ERMHDR\t20.12\t2026-10-15\tProject\tprojectcraft\tProjectCraft\tdbxDatabaseNoName\tProject Management\tUSD");
    let _ = writeln!(out, "%T\tPROJECT");
    let _ = writeln!(out, "%F\tproj_id\tproj_short_name\tproj_name");
    let _ = writeln!(out, "%R\t1\t{}\t{}", clean_tab(&project.code), clean_tab(&project.name));

    let _ = writeln!(out, "%T\tTASK");
    let _ = writeln!(out, "%F\ttask_id\tproj_id\ttask_code\ttask_name\ttask_type\tstatus_code\ttarget_drtn_hr_cnt\tphys_complete_pct\twbs_code");
    for t in &project.tasks {
        let task_type = if t.kind == ItemKind::Milestone || t.duration_days == 0.0 { "TT_Mile" } else { "TT_Task" };
        let status_code = match t.status {
            TaskStatus::Done => "TK_Complete",
            TaskStatus::InProgress | TaskStatus::Review => "TK_Active",
            _ => "TK_NotStart",
        };
        let _ = writeln!(
            out,
            "%R\t{}\t1\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{}",
            t.id,
            clean_tab(&t.key),
            clean_tab(&t.title),
            task_type,
            status_code,
            t.duration_days * 8.0,
            t.progress * 100.0,
            clean_tab(&t.wbs)
        );
    }

    let _ = writeln!(out, "%T\tTASKPRED");
    let _ = writeln!(out, "%F\ttask_pred_id\ttask_id\tpred_task_id\tpred_type\tlag_hr_cnt");
    for (idx, d) in project.dependencies.iter().enumerate() {
        let pred_type = match d.kind {
            DepKind::FinishToStart => "PR_FS",
            DepKind::StartToStart => "PR_SS",
            DepKind::FinishToFinish => "PR_FF",
            DepKind::StartToFinish => "PR_SF",
        };
        let _ = writeln!(out, "%R\t{}\t{}\t{}\t{}\t{:.2}", idx + 1, d.successor_id, d.predecessor_id, pred_type, d.lag_days * 8.0);
    }

    let _ = writeln!(out, "%T\tRSRC");
    let _ = writeln!(out, "%F\trsrc_id\trsrc_name\trsrc_title_name\tmax_qty_per_hr\tcost_per_qty");
    for r in &project.resources {
        let _ =
            writeln!(out, "%R\t{}\t{}\t{}\t{:.2}\t{:.2}", r.id, clean_tab(&r.name), clean_tab(&r.role), r.max_units, r.standard_rate_per_day / 8.0);
    }

    let _ = writeln!(out, "%T\tTASKRSRC");
    let _ = writeln!(out, "%F\ttaskrsrc_id\ttask_id\trsrc_id\ttarget_qty_per_hr");
    for (idx, a) in project.assignments.iter().enumerate() {
        let _ = writeln!(out, "%R\t{}\t{}\t{}\t{:.2}", idx + 1, a.task_id, a.resource_id, a.units);
    }
    let _ = writeln!(out, "%E");
    out
}

/// Import a project from Primavera P6 `.xer` format.
pub fn from_primavera_xer(src: &str) -> Result<Project, FormatError> {
    check_size(src)?;
    if !src.starts_with("ERMHDR") && !src.contains("%T\t") {
        return Err(FormatError::Validation("not a valid Primavera P6 .xer file (missing ERMHDR / %T)".to_string()));
    }
    let mut project = Project::new("Primavera XER Project", "XER");
    let mut current_table = String::new();
    let mut fields: Vec<String> = Vec::new();
    let mut pending_preds: Vec<(u32, u32, DepKind, f64)> = Vec::new();
    let mut pending_assigns: Vec<(u32, u32, f64)> = Vec::new();

    let col = |row: &[&str], fields: &[String], name: &str| -> Option<String> {
        let idx = fields.iter().position(|f| f == name)?;
        row.get(idx).map(|s| (*s).trim().to_string())
    };

    for line in src.lines().take(MAX_TASKS * 6) {
        let parts: Vec<&str> = line.split('\t').collect();
        let Some(&tag) = parts.first() else { continue };
        match tag {
            "%T" => {
                current_table = parts.get(1).copied().unwrap_or("").trim().to_string();
                fields.clear();
            }
            "%F" => {
                fields = parts.iter().skip(1).map(|s| (*s).trim().to_string()).collect();
            }
            "%R" => {
                let row: Vec<&str> = parts.into_iter().skip(1).collect();
                match current_table.as_str() {
                    "PROJECT" => {
                        if let Some(code) = col(&row, &fields, "proj_short_name").filter(|s| !s.is_empty()) {
                            project.code = code;
                        }
                        if let Some(name) = col(&row, &fields, "proj_name").filter(|s| !s.is_empty()) {
                            project.name = name;
                        }
                    }
                    "TASK" => {
                        let Some(tid) = col(&row, &fields, "task_id").and_then(|s| s.parse::<u32>().ok()) else { continue };
                        let title = col(&row, &fields, "task_name").unwrap_or_else(|| format!("Task {tid}"));
                        let hrs = col(&row, &fields, "target_drtn_hr_cnt").and_then(|s| s.parse::<f64>().ok()).unwrap_or(8.0);
                        let ttype = col(&row, &fields, "task_type").unwrap_or_default();
                        let is_mile = ttype.contains("Mile") || hrs == 0.0;
                        let wbs = col(&row, &fields, "wbs_code").unwrap_or_else(|| tid.to_string());
                        let id = project
                            .add_task(title, if is_mile { 0.0 } else { (hrs / 8.0).max(0.0) }, Some(wbs))
                            .map_err(|e| FormatError::Validation(e.to_string()))?;
                        if let Some(t) = project.task_mut(id) {
                            t.id = tid;
                            if let Some(code) = col(&row, &fields, "task_code").filter(|s| !s.is_empty()) {
                                t.key = code;
                            }
                            if let Some(pct) = col(&row, &fields, "phys_complete_pct").and_then(|s| s.parse::<f64>().ok()) {
                                t.progress = (pct / 100.0).clamp(0.0, 1.0);
                            }
                            if let Some(sc) = col(&row, &fields, "status_code") {
                                t.status = match sc.as_str() {
                                    "TK_Complete" => TaskStatus::Done,
                                    "TK_Active" => TaskStatus::InProgress,
                                    _ => TaskStatus::Todo,
                                };
                            }
                        }
                    }
                    "TASKPRED" => {
                        let succ = col(&row, &fields, "task_id").and_then(|s| s.parse::<u32>().ok());
                        let pred = col(&row, &fields, "pred_task_id").and_then(|s| s.parse::<u32>().ok());
                        let ptype = col(&row, &fields, "pred_type").map(|s| DepKind::parse(&s)).unwrap_or(DepKind::FinishToStart);
                        let lag_hrs = col(&row, &fields, "lag_hr_cnt").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        if let (Some(p), Some(s)) = (pred, succ) {
                            pending_preds.push((p, s, ptype, lag_hrs / 8.0));
                        }
                    }
                    "RSRC" => {
                        let Some(rid) = col(&row, &fields, "rsrc_id").and_then(|s| s.parse::<u32>().ok()) else { continue };
                        let name = col(&row, &fields, "rsrc_name").unwrap_or_else(|| format!("Resource {rid}"));
                        let role = col(&row, &fields, "rsrc_title_name").unwrap_or_else(|| "Role".to_string());
                        let max_u = col(&row, &fields, "max_qty_per_hr").and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
                        let hourly = col(&row, &fields, "cost_per_qty").and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
                        if let Ok(added) = project.add_resource(name, role, max_u, hourly * 8.0) {
                            if let Some(r) = project.resources.iter_mut().find(|r| r.id == added) {
                                r.id = rid;
                            }
                        }
                    }
                    "TASKRSRC" => {
                        let tid = col(&row, &fields, "task_id").and_then(|s| s.parse::<u32>().ok());
                        let rid = col(&row, &fields, "rsrc_id").and_then(|s| s.parse::<u32>().ok());
                        let u = col(&row, &fields, "target_qty_per_hr").and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
                        if let (Some(t), Some(r)) = (tid, rid) {
                            pending_assigns.push((t, r, u));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    project.repair_counters();
    for (p, s, k, lag) in pending_preds {
        let _ = project.add_dependency(p, s, k, lag);
    }
    for (t, r, u) in pending_assigns {
        let _ = project.assign_resource(t, r, u);
    }
    let _ = schedule_project(&mut project);
    Ok(project)
}

fn clean_tab(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

/// Render a standalone, publication-quality SVG Gantt chart with WBS table,
/// day grid, baseline comparison bars, progress fill, critical-path highlighting,
/// milestone diamonds, and dependency connector arrows.
pub fn render_gantt_svg(project: &Project) -> String {
    let row_h = 32.0_f64;
    let header_h = 56.0_f64;
    let left_w = 340.0_f64;
    let day_w = 32.0_f64;
    let max_day = project.tasks.iter().map(|t| t.early_finish.ceil()).fold(10.0_f64, f64::max) + 2.0;
    let width = left_w + max_day * day_w + 32.0;
    let height = header_h + (project.tasks.len().max(1) as f64) * row_h + 44.0;

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\" font-family=\"Inter, Segoe UI, system-ui, sans-serif\">"
    );
    let _ = writeln!(svg, "  <rect width=\"100%\" height=\"100%\" fill=\"#0f141c\"/>");

    // Title & header banner
    let _ = writeln!(svg, "  <rect x=\"0\" y=\"0\" width=\"{width:.0}\" height=\"{header_h:.0}\" fill=\"#161d27\"/>");
    let _ = writeln!(
        svg,
        "  <text x=\"16\" y=\"24\" fill=\"#e6edf3\" font-size=\"15\" font-weight=\"700\">{} ({}) — Gantt Schedule</text>",
        xml_escape(&project.name),
        xml_escape(&project.code)
    );
    let _ = writeln!(
        svg,
        "  <text x=\"16\" y=\"44\" fill=\"#8b949e\" font-size=\"11\">WBS · Task · Duration · ES–EF · Red = Critical Path · Gold Line = Status Day ({:.1}d)</text>",
        project.status_day
    );

    // Day columns
    let total_days = max_day as usize;
    for d in 0..=total_days {
        let x = left_w + (d as f64) * day_w;
        let is_weekend = (d % 7 == 5) || (d % 7 == 6);
        if is_weekend && d < total_days {
            let _ =
                writeln!(svg, "  <rect x=\"{x:.1}\" y=\"{header_h:.1}\" width=\"{day_w:.1}\" height=\"{:.1}\" fill=\"#131922\"/>", height - header_h);
        }
        let _ = writeln!(svg, "  <line x1=\"{x:.1}\" y1=\"32\" x2=\"{x:.1}\" y2=\"{height:.1}\" stroke=\"#212936\" stroke-width=\"1\"/>");
        if d % 2 == 0 {
            let _ = writeln!(svg, "  <text x=\"{:.1}\" y=\"48\" fill=\"#8b949e\" font-size=\"10\">D{d}</text>", x + 4.0);
        }
    }

    let baseline = project.baselines.first();
    let mut task_row_map = std::collections::BTreeMap::new();

    for (idx, t) in project.tasks.iter().enumerate() {
        task_row_map.insert(t.id, idx);
        let y = header_h + (idx as f64) * row_h;
        if idx % 2 == 1 {
            let _ = writeln!(svg, "  <rect x=\"0\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{row_h:.1}\" fill=\"#121820\" fill-opacity=\"0.55\"/>");
        }
        let indent = (t.wbs_depth() as f64) * 10.0;
        let label = format!("{}  {}  {}", t.wbs, t.key, t.title);
        let _ =
            writeln!(svg, "  <text x=\"{:.1}\" y=\"{:.1}\" fill=\"#d0d7de\" font-size=\"12\">{}</text>", 14.0 + indent, y + 20.0, xml_escape(&label));
        let _ = writeln!(
            svg,
            "  <text x=\"{:.1}\" y=\"{:.1}\" fill=\"#8b949e\" font-size=\"11\" text-anchor=\"end\">{:.1}d</text>",
            left_w - 10.0,
            y + 20.0,
            t.duration_days
        );

        // Optional baseline bar underneath
        if let Some(bt) = baseline.and_then(|b| b.tasks.iter().find(|bt| bt.task_id == t.id)) {
            if bt.duration_days > 0.0 {
                let bx = left_w + bt.start_day * day_w;
                let bw = (bt.duration_days * day_w).max(4.0);
                let _ = writeln!(
                    svg,
                    "  <rect x=\"{bx:.1}\" y=\"{:.1}\" width=\"{bw:.1}\" height=\"4\" rx=\"2\" fill=\"#6e7681\" fill-opacity=\"0.65\"/>",
                    y + 23.0
                );
            }
        }

        let x0 = left_w + t.early_start * day_w;
        if t.kind == ItemKind::Milestone || t.duration_days <= 0.0 {
            let cy = y + 15.0;
            let color = if t.critical { "#f85149" } else { "#d29922" };
            let _ = writeln!(
                svg,
                "  <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{color}\" stroke=\"#ffffff\" stroke-width=\"1\"/>",
                x0,
                cy - 7.0,
                x0 + 7.0,
                cy,
                x0,
                cy + 7.0,
                x0 - 7.0,
                cy
            );
        } else {
            let bw = (t.duration_days * day_w).max(6.0);
            let bar_y = y + 7.0;
            let bar_h = 15.0;
            let base_color = if t.critical { "#da3633" } else { "#2f81f7" };
            let fill_color = if t.critical { "#ff7b72" } else { "#58a6ff" };
            let _ = writeln!(
                svg,
                "  <rect x=\"{x0:.1}\" y=\"{bar_y:.1}\" width=\"{bw:.1}\" height=\"{bar_h:.1}\" rx=\"3\" fill=\"{base_color}\" fill-opacity=\"0.42\" stroke=\"{fill_color}\" stroke-width=\"1.2\"/>"
            );
            let prog_w = bw * t.progress.clamp(0.0, 1.0);
            if prog_w > 0.5 {
                let _ = writeln!(
                    svg,
                    "  <rect x=\"{x0:.1}\" y=\"{bar_y:.1}\" width=\"{prog_w:.1}\" height=\"{bar_h:.1}\" rx=\"3\" fill=\"{fill_color}\"/>"
                );
            }
        }
    }

    // Dependency connector lines
    for dep in &project.dependencies {
        if let (Some(&p_row), Some(&s_row), Some(p_task), Some(s_task)) = (
            task_row_map.get(&dep.predecessor_id),
            task_row_map.get(&dep.successor_id),
            project.task(dep.predecessor_id),
            project.task(dep.successor_id),
        ) {
            let x1 = left_w + p_task.early_finish * day_w;
            let y1 = header_h + (p_row as f64) * row_h + 14.5;
            let x2 = left_w + s_task.early_start * day_w;
            let y2 = header_h + (s_row as f64) * row_h + 14.5;
            let mid_x = (x1 + 8.0).min(x2 - 4.0).max(x1 + 4.0);
            let stroke = if p_task.critical && s_task.critical { "#ff7b72" } else { "#8b949e" };
            let _ = writeln!(
                svg,
                "  <path d=\"M {x1:.1} {y1:.1} L {mid_x:.1} {y1:.1} L {mid_x:.1} {y2:.1} L {x2:.1} {y2:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.2\"/>"
            );
        }
    }

    // Status day vertical line
    let sx = left_w + project.status_day * day_w;
    let _ = writeln!(
        svg,
        "  <line x1=\"{sx:.1}\" y1=\"{header_h:.1}\" x2=\"{sx:.1}\" y2=\"{height:.1}\" stroke=\"#e3b341\" stroke-width=\"1.5\" stroke-dasharray=\"5,4\"/>"
    );

    let _ = writeln!(svg, "</svg>");
    svg
}

/// Render a standalone HTML executive & EVM report containing the embedded SVG Gantt chart,
/// EVM metrics table, resource workload summary, and full WBS schedule table.
pub fn render_report_html(project: &Project) -> String {
    let evm = compute_evm(project, None);
    let workload = compute_workload(project);
    let gantt_svg = render_gantt_svg(project);
    let mut html = String::new();
    let _ = writeln!(html, "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    let _ = writeln!(html, "<title>{} — ProjectCraft Report</title>", xml_escape(&project.name));
    let _ = writeln!(
        html,
        "<style>body{{font-family:system-ui,-apple-system,sans-serif;background:#0d1117;color:#e6edf3;margin:24px;}} \
         .kpis{{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin:18px 0;}} \
         .card{{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:12px;}} \
         .card .label{{font-size:12px;color:#8b949e;}} .card .val{{font-size:20px;font-weight:700;margin-top:4px;}} \
         table{{width:100%;border-collapse:collapse;margin-top:16px;font-size:13px;}} \
         th,td{{border:1px solid #30363d;padding:8px 10px;text-align:left;}} th{{background:#161b22;}}</style></head><body>"
    );
    let _ = writeln!(html, "<h1>{} ({})</h1><p>{}</p>", xml_escape(&project.name), xml_escape(&project.code), xml_escape(&project.description));
    let _ = writeln!(
        html,
        "<div class=\"kpis\">\
         <div class=\"card\"><div class=\"label\">BAC (Budget)</div><div class=\"val\">${:.0}</div></div>\
         <div class=\"card\"><div class=\"label\">PV (Planned)</div><div class=\"val\">${:.0}</div></div>\
         <div class=\"card\"><div class=\"label\">EV (Earned)</div><div class=\"val\">${:.0}</div></div>\
         <div class=\"card\"><div class=\"label\">AC (Actual)</div><div class=\"val\">${:.0}</div></div>\
         <div class=\"card\"><div class=\"label\">SPI / CPI</div><div class=\"val\">{:.2} / {:.2}</div></div>\
         <div class=\"card\"><div class=\"label\">EAC (Forecast)</div><div class=\"val\">${:.0}</div></div>\
         <div class=\"card\"><div class=\"label\">Overallocated Days</div><div class=\"val\">{}</div></div>\
         </div>",
        evm.bac, evm.pv, evm.ev, evm.ac, evm.spi, evm.cpi, evm.eac, workload.total_overallocated_person_days
    );
    let _ = writeln!(html, "<div style=\"overflow-x:auto;border:1px solid #30363d;border-radius:8px;\">{gantt_svg}</div>");
    let _ = writeln!(
        html,
        "<h2>WBS &amp; Critical Path Schedule</h2><table><thead><tr>\
         <th>WBS</th><th>Key</th><th>Title</th><th>Status</th><th>Dur (d)</th><th>ES–EF</th><th>Slack (d)</th><th>Critical</th><th>Planned Cost</th>\
         </tr></thead><tbody>"
    );
    for t in &project.tasks {
        let _ = writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1}</td><td>{:.1}–{:.1}</td><td>{:.1}</td><td>{}</td><td>${:.0}</td></tr>",
            xml_escape(&t.wbs),
            xml_escape(&t.key),
            xml_escape(&t.title),
            t.status.label(),
            t.duration_days,
            t.early_start,
            t.early_finish,
            t.total_slack,
            if t.critical { "YES" } else { "no" },
            project.planned_task_cost(t.id)
        );
    }
    let _ = writeln!(html, "</tbody></table></body></html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_native_json_csv_jira_mspdi_and_xer() {
        let mut p = Project::sample_launch_program();
        schedule_project(&mut p).unwrap();

        // 1. Native .projectcraft JSON
        let js = to_projectcraft_json(&p).unwrap();
        let p_js = from_projectcraft_json(&js).unwrap();
        assert_eq!(p_js.tasks.len(), p.tasks.len());

        // 2. CSV
        let csv = to_csv(&p);
        let p_csv = from_csv(&csv, "CSV Roundtrip").unwrap();
        assert_eq!(p_csv.tasks.len(), p.tasks.len());
        assert_eq!(p_csv.dependencies.len(), p.dependencies.len());

        // 3. Jira JSON
        let jira = to_jira_json(&p);
        let p_jira = from_jira_json(&jira).unwrap();
        assert_eq!(p_jira.tasks.len(), p.tasks.len());
        assert_eq!(p_jira.dependencies.len(), p.dependencies.len());

        // 4. MSPDI XML
        let xml = to_mspdi_xml(&p);
        let p_xml = from_mspdi_xml(&xml).unwrap();
        assert_eq!(p_xml.tasks.len(), p.tasks.len());
        assert_eq!(p_xml.dependencies.len(), p.dependencies.len());
        assert_eq!(p_xml.resources.len(), p.resources.len());

        // 5. Primavera P6 .xer
        let xer = to_primavera_xer(&p);
        let p_xer = from_primavera_xer(&xer).unwrap();
        assert_eq!(p_xer.tasks.len(), p.tasks.len());
        assert_eq!(p_xer.dependencies.len(), p.dependencies.len());
        assert_eq!(p_xer.resources.len(), p.resources.len());

        // 6. SVG + HTML exports
        let svg = render_gantt_svg(&p);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
        let html = render_report_html(&p);
        assert!(html.contains("<!DOCTYPE html>"));
    }
}
