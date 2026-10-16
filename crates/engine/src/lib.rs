//! `projectcraft_engine` (L5) — The ProjectCraft Command Engine.
//!
//! Every user-visible action or query is a command with a stable id
//! (`task.add`, `dep.add`, `schedule.run`, `resource.level`, `evm.compute`, …)
//! executed via [`Session::execute`]. The Studio UI, `projectcraft-cli`, the
//! TCP control channel, and the MCP server all invoke the same command table.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pmcore::{CoreError, DepKind, FieldValue, ItemKind, Priority, Project, TaskConstraint, TaskStatus};
use pmformat::{
    from_csv, from_jira_json, from_mspdi_xml, from_primavera_xer, from_projectcraft_json, render_gantt_svg, render_report_html, to_csv, to_jira_json,
    to_mspdi_xml, to_primavera_xer, to_projectcraft_json, FormatError,
};
use pmsched::{
    compute_baseline_variance, compute_evm, compute_sprint_burndown, compute_workload, level_resources, run_monte_carlo, schedule_project,
    MonteCarloReport, SchedError,
};
use pmsync::{SyncBundle, SyncError, SyncRole};
use serde_json::{json, Value as Json};

/// Errors returned by engine commands — always structured, never a crash.
#[derive(Debug)]
pub enum EngineError {
    UnknownCommand(String),
    BadParams { cmd: String, msg: String },
    Core(CoreError),
    Sched(SchedError),
    Format(FormatError),
    Sync(SyncError),
    Io(String),
    Panic(String),
}

impl EngineError {
    pub fn bad_params(cmd: &str, msg: impl Into<String>) -> Self {
        EngineError::BadParams { cmd: cmd.to_string(), msg: msg.into() }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnknownCommand(id) => write!(f, "unknown command `{id}`"),
            EngineError::BadParams { cmd, msg } => write!(f, "invalid parameters for `{cmd}`: {msg}"),
            EngineError::Core(e) => write!(f, "{e}"),
            EngineError::Sched(e) => write!(f, "{e}"),
            EngineError::Format(e) => write!(f, "{e}"),
            EngineError::Sync(e) => write!(f, "{e}"),
            EngineError::Io(e) => write!(f, "I/O error: {e}"),
            EngineError::Panic(id) => write!(f, "internal error in `{id}` (project state was rolled back)"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<CoreError> for EngineError {
    fn from(e: CoreError) -> Self {
        EngineError::Core(e)
    }
}

impl From<SchedError> for EngineError {
    fn from(e: SchedError) -> Self {
        EngineError::Sched(e)
    }
}

impl From<FormatError> for EngineError {
    fn from(e: FormatError) -> Self {
        EngineError::Format(e)
    }
}

impl From<SyncError> for EngineError {
    fn from(e: SyncError) -> Self {
        EngineError::Sync(e)
    }
}

/// Static specification for one engine command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub params: &'static str,
    pub run: fn(&mut Session, &Json) -> Result<Json, EngineError>,
}

/// Guarded ProjectCraft session owning the current [`Project`], undo/redo
/// snapshots, sync bundle, and the most recent Monte Carlo report.
#[derive(Clone)]
pub struct Session {
    project: Project,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
    last_monte_carlo: Option<MonteCarloReport>,
    current_path: Option<String>,
    sync_bundle: SyncBundle,
    active_author: String,
    active_role: SyncRole,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// Create a session pre-seeded with the sample launch program and scheduled via CPM.
    pub fn new() -> Self {
        let mut project = Project::sample_launch_program();
        let _ = schedule_project(&mut project);
        let mc = run_monte_carlo(&project, 400, 42).ok();
        Session {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_monte_carlo: mc,
            current_path: None,
            sync_bundle: SyncBundle::new(),
            active_author: "local-owner".to_string(),
            active_role: SyncRole::Owner,
        }
    }

    pub fn empty(name: &str, code: &str) -> Self {
        Session {
            project: Project::new(name, code),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_monte_carlo: None,
            current_path: None,
            sync_bundle: SyncBundle::new(),
            active_author: "local-owner".to_string(),
            active_role: SyncRole::Owner,
        }
    }

    pub fn sync_bundle(&self) -> &SyncBundle {
        &self.sync_bundle
    }

    pub fn set_identity(&mut self, author: impl Into<String>, role: SyncRole) {
        self.active_author = author.into();
        self.active_role = role;
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }

    pub fn last_monte_carlo(&self) -> Option<&MonteCarloReport> {
        self.last_monte_carlo.as_ref()
    }

    pub fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }

    pub fn push_undo(&mut self) {
        if self.undo_stack.len() >= 64 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.project.clone());
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.project.clone());
            self.project = prev;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.project.clone());
            self.project = next;
            true
        } else {
            false
        }
    }

    /// Execute command `id` with JSON `params`. Any panic rolls the session
    /// back to its pre-call snapshot and returns [`EngineError::Panic`].
    pub fn execute(&mut self, id: &str, params: Json) -> Result<Json, EngineError> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        let snapshot = self.clone();
        match catch_unwind(AssertUnwindSafe(|| (spec.run)(self, &params))) {
            Ok(res) => res,
            Err(_) => {
                *self = snapshot;
                Err(EngineError::Panic(id.to_string()))
            }
        }
    }

    pub fn commands(&self) -> &'static [CommandSpec] {
        command_specs()
    }
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

pub fn command_specs() -> &'static [CommandSpec] {
    COMMANDS
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "engine.list",
        label: "List all registered engine commands",
        params: "{}",
        run: cmd_engine_list,
    },
    CommandSpec {
        id: "project.new",
        label: "Create a new empty project",
        params: "{name: string, code?: string}",
        run: cmd_project_new,
    },
    CommandSpec {
        id: "project.seed_demo",
        label: "Load the sample Orion Launch Program",
        params: "{}",
        run: cmd_project_seed_demo,
    },
    CommandSpec {
        id: "project.open",
        label: "Open project file (.projectcraft, .csv, .xml, .xer, .json)",
        params: "{path: string}",
        run: cmd_project_open,
    },
    CommandSpec {
        id: "project.save",
        label: "Save native .projectcraft file",
        params: "{path: string}",
        run: cmd_project_save,
    },
    CommandSpec {
        id: "project.summary",
        label: "Return project schedule, cost, and risk summary",
        params: "{}",
        run: cmd_project_summary,
    },
    CommandSpec {
        id: "project.set_status_day",
        label: "Set project status day for EVM",
        params: "{status_day: number}",
        run: cmd_project_set_status_day,
    },
    CommandSpec {
        id: "project.undo",
        label: "Undo last project mutation",
        params: "{}",
        run: cmd_project_undo,
    },
    CommandSpec {
        id: "project.redo",
        label: "Redo last undone project mutation",
        params: "{}",
        run: cmd_project_redo,
    },
    CommandSpec {
        id: "task.add",
        label: "Add a work item (task/milestone/epic/story/bug)",
        params: "{title: string, duration_days?: number, wbs?: string, kind?: string, status?: string, priority?: string, story_points?: number, fixed_cost?: number}",
        run: cmd_task_add,
    },
    CommandSpec {
        id: "task.update",
        label: "Update task properties and reschedule",
        params: "{id: number, title?: string, duration_days?: number, wbs?: string, kind?: string, status?: string, priority?: string, progress?: number, story_points?: number, sprint_id?: number, constraint_type?: string, constraint_day?: number, optimistic_days?: number, most_likely_days?: number, pessimistic_days?: number, fixed_cost?: number, actual_cost?: number}",
        run: cmd_task_update,
    },
    CommandSpec {
        id: "task.delete",
        label: "Delete a task and its dependencies",
        params: "{id: number}",
        run: cmd_task_delete,
    },
    CommandSpec {
        id: "task.list",
        label: "List tasks (optionally filtered by status or critical path)",
        params: "{status?: string, critical_only?: bool}",
        run: cmd_task_list,
    },
    CommandSpec {
        id: "task.set_status",
        label: "Move task to workflow status",
        params: "{id: number, status: string}",
        run: cmd_task_set_status,
    },
    CommandSpec {
        id: "task.set_progress",
        label: "Set task completion fraction (0.0..=1.0)",
        params: "{id: number, progress: number}",
        run: cmd_task_set_progress,
    },
    CommandSpec {
        id: "task.set_field",
        label: "Set a custom field on a task",
        params: "{id: number, key: string, value: any}",
        run: cmd_task_set_field,
    },
    CommandSpec {
        id: "dep.add",
        label: "Add or update a dependency (FS/SS/FF/SF + lag_days)",
        params: "{predecessor_id: number, successor_id: number, kind?: string, lag_days?: number}",
        run: cmd_dep_add,
    },
    CommandSpec {
        id: "dep.remove",
        label: "Remove a dependency edge",
        params: "{predecessor_id: number, successor_id: number}",
        run: cmd_dep_remove,
    },
    CommandSpec {
        id: "schedule.run",
        label: "Run CPM forward and backward pass",
        params: "{}",
        run: cmd_schedule_run,
    },
    CommandSpec {
        id: "schedule.critical_path",
        label: "Return critical path tasks and total duration",
        params: "{}",
        run: cmd_schedule_critical_path,
    },
    CommandSpec {
        id: "schedule.monte_carlo",
        label: "Run Monte Carlo schedule-risk simulation",
        params: "{trials?: number, seed?: number}",
        run: cmd_schedule_monte_carlo,
    },
    CommandSpec {
        id: "baseline.capture",
        label: "Capture a named baseline snapshot",
        params: "{name?: string}",
        run: cmd_baseline_capture,
    },
    CommandSpec {
        id: "baseline.variance",
        label: "Compute per-task schedule & cost variance against baseline",
        params: "{baseline_id?: number}",
        run: cmd_baseline_variance,
    },
    CommandSpec {
        id: "resource.add",
        label: "Add a resource to the pool",
        params: "{name: string, role?: string, max_units?: number, standard_rate_per_day?: number}",
        run: cmd_resource_add,
    },
    CommandSpec {
        id: "resource.assign",
        label: "Assign a resource to a task",
        params: "{task_id: number, resource_id: number, units?: number}",
        run: cmd_resource_assign,
    },
    CommandSpec {
        id: "resource.workload",
        label: "Compute daily resource workload and overallocations",
        params: "{}",
        run: cmd_resource_workload,
    },
    CommandSpec {
        id: "resource.level",
        label: "Level over-allocated resources using priority rules",
        params: "{}",
        run: cmd_resource_level,
    },
    CommandSpec {
        id: "evm.compute",
        label: "Compute Earned Value Management (PV, EV, AC, SV, CV, SPI, CPI, EAC)",
        params: "{status_day?: number}",
        run: cmd_evm_compute,
    },
    CommandSpec {
        id: "sprint.add",
        label: "Add an Agile sprint",
        params: "{name: string, goal?: string, start_day: number, end_day: number, capacity_points?: number}",
        run: cmd_sprint_add,
    },
    CommandSpec {
        id: "sprint.burndown",
        label: "Compute Agile sprint velocity and burndown",
        params: "{sprint_id?: number}",
        run: cmd_sprint_burndown,
    },
    CommandSpec {
        id: "export.csv",
        label: "Export schedule & WBS to CSV (and optionally write to path)",
        params: "{path?: string}",
        run: cmd_export_csv,
    },
    CommandSpec {
        id: "import.csv",
        label: "Import project from CSV text or path",
        params: "{csv?: string, path?: string, name?: string}",
        run: cmd_import_csv,
    },
    CommandSpec {
        id: "export.jira",
        label: "Export project to Jira JSON (and optionally write to path)",
        params: "{path?: string}",
        run: cmd_export_jira,
    },
    CommandSpec {
        id: "import.jira",
        label: "Import project from Jira JSON text or path",
        params: "{json?: string, path?: string}",
        run: cmd_import_jira,
    },
    CommandSpec {
        id: "export.mspdi",
        label: "Export project to Microsoft Project MSPDI XML",
        params: "{path?: string}",
        run: cmd_export_mspdi,
    },
    CommandSpec {
        id: "import.mspdi",
        label: "Import project from Microsoft Project MSPDI XML",
        params: "{xml?: string, path?: string}",
        run: cmd_import_mspdi,
    },
    CommandSpec {
        id: "export.xer",
        label: "Export project to Primavera P6 .xer format",
        params: "{path?: string}",
        run: cmd_export_xer,
    },
    CommandSpec {
        id: "import.xer",
        label: "Import project from Primavera P6 .xer format",
        params: "{xer?: string, path?: string}",
        run: cmd_import_xer,
    },
    CommandSpec {
        id: "export.gantt_svg",
        label: "Render Gantt chart as SVG (and optionally write to path)",
        params: "{path?: string}",
        run: cmd_export_gantt_svg,
    },
    CommandSpec {
        id: "export.report_html",
        label: "Render standalone HTML executive report",
        params: "{path?: string}",
        run: cmd_export_report_html,
    },
    CommandSpec {
        id: "sync.add_comment",
        label: "Add a synced comment with @mentions to a task",
        params: "{task_id: number, body: string, author?: string, role?: string}",
        run: cmd_sync_add_comment,
    },
    CommandSpec {
        id: "sync.activity_feed",
        label: "Return the change DAG audit log, comments, and post-merge conflict advisories",
        params: "{}",
        run: cmd_sync_activity_feed,
    },
    CommandSpec {
        id: "sync.check_permission",
        label: "Verify whether a role is permitted to run a command",
        params: "{role: string, command: string}",
        run: cmd_sync_check_permission,
    },
    CommandSpec {
        id: "sync.export_bundle",
        label: "Export the local sync change bundle as JSON",
        params: "{path?: string}",
        run: cmd_sync_export_bundle,
    },
    CommandSpec {
        id: "sync.import_bundle",
        label: "Merge a remote sync bundle and run post-merge conflict inspection",
        params: "{bundle?: object, path?: string}",
        run: cmd_sync_import_bundle,
    },
];

fn req_str<'a>(cmd: &str, p: &'a Json, key: &str) -> Result<&'a str, EngineError> {
    p.get(key).and_then(Json::as_str).ok_or_else(|| EngineError::bad_params(cmd, format!("missing string parameter `{key}`")))
}

fn req_u32(cmd: &str, p: &Json, key: &str) -> Result<u32, EngineError> {
    p.get(key)
        .and_then(Json::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| EngineError::bad_params(cmd, format!("missing positive integer parameter `{key}`")))
}

fn maybe_write_path(p: &Json, content: &str) -> Result<Option<String>, EngineError> {
    if let Some(path) = p.get("path").and_then(Json::as_str).filter(|s| !s.trim().is_empty()) {
        std::fs::write(path, content).map_err(|e| EngineError::Io(format!("cannot write {path}: {e}")))?;
        Ok(Some(path.to_string()))
    } else {
        Ok(None)
    }
}

fn read_text_or_path(cmd: &str, p: &Json, inline_key: &str) -> Result<String, EngineError> {
    if let Some(s) = p.get(inline_key).and_then(Json::as_str) {
        return Ok(s.to_string());
    }
    let path = req_str(cmd, p, "path")?;
    std::fs::read_to_string(path).map_err(|e| EngineError::Io(format!("cannot read {path}: {e}")))
}

fn cmd_engine_list(_s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let list: Vec<Json> = COMMANDS.iter().map(|c| json!({"id": c.id, "label": c.label, "params": c.params})).collect();
    Ok(json!({"commands": list, "count": list.len()}))
}

fn cmd_project_new(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let name = p.get("name").and_then(Json::as_str).unwrap_or("Untitled Project");
    let code = p.get("code").and_then(Json::as_str).unwrap_or("PC");
    s.push_undo();
    s.project = Project::new(name, code);
    s.last_monte_carlo = None;
    s.current_path = None;
    Ok(json!({"name": s.project.name, "code": s.project.code}))
}

fn cmd_project_seed_demo(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    s.push_undo();
    let mut proj = Project::sample_launch_program();
    let summary = schedule_project(&mut proj)?;
    let mc = run_monte_carlo(&proj, 400, 42).ok();
    s.project = proj;
    s.last_monte_carlo = mc;
    s.current_path = None;
    Ok(serde_json::to_value(summary).unwrap_or_else(|_| json!({})))
}

fn cmd_project_open(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let path = req_str("project.open", p, "path")?;
    let src = std::fs::read_to_string(path).map_err(|e| EngineError::Io(format!("cannot read {path}: {e}")))?;
    let lower = path.to_ascii_lowercase();
    let proj = if lower.ends_with(".csv") {
        from_csv(&src, "Imported CSV")?
    } else if lower.ends_with(".xml") {
        from_mspdi_xml(&src)?
    } else if lower.ends_with(".xer") {
        from_primavera_xer(&src)?
    } else if src.contains("\"issues\"") && !src.contains("\"format\": \"projectcraft\"") {
        from_jira_json(&src)?
    } else {
        from_projectcraft_json(&src)?
    };
    s.push_undo();
    s.project = proj;
    s.current_path = Some(path.to_string());
    s.last_monte_carlo = run_monte_carlo(&s.project, 400, 42).ok();
    cmd_project_summary(s, &json!({}))
}

fn cmd_project_save(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let path = req_str("project.save", p, "path")?;
    let content = to_projectcraft_json(&s.project)?;
    std::fs::write(path, &content).map_err(|e| EngineError::Io(format!("cannot write {path}: {e}")))?;
    s.current_path = Some(path.to_string());
    Ok(json!({"path": path, "bytes": content.len()}))
}

fn cmd_project_summary(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let summary = schedule_project(&mut s.project)?;
    let evm = compute_evm(&s.project, None);
    let workload = compute_workload(&s.project);
    Ok(json!({
        "name": s.project.name,
        "code": s.project.code,
        "tasks": s.project.tasks.len(),
        "dependencies": s.project.dependencies.len(),
        "resources": s.project.resources.len(),
        "sprints": s.project.sprints.len(),
        "baselines": s.project.baselines.len(),
        "project_duration_days": summary.project_duration_days,
        "critical_task_ids": summary.critical_task_ids,
        "bac": evm.bac,
        "pv": evm.pv,
        "ev": evm.ev,
        "ac": evm.ac,
        "spi": evm.spi,
        "cpi": evm.cpi,
        "eac": evm.eac,
        "overallocated_person_days": workload.total_overallocated_person_days,
    }))
}

fn cmd_project_set_status_day(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let day =
        p.get("status_day").and_then(Json::as_f64).ok_or_else(|| EngineError::bad_params("project.set_status_day", "missing number `status_day`"))?;
    s.push_undo();
    s.project.status_day = day.max(0.0);
    let evm = compute_evm(&s.project, None);
    Ok(serde_json::to_value(evm).unwrap_or_else(|_| json!({})))
}

fn cmd_project_undo(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let ok = s.undo();
    Ok(json!({"undone": ok, "tasks": s.project.tasks.len()}))
}

fn cmd_project_redo(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let ok = s.redo();
    Ok(json!({"redone": ok, "tasks": s.project.tasks.len()}))
}

fn cmd_task_add(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let title = req_str("task.add", p, "title")?;
    let dur = p.get("duration_days").and_then(Json::as_f64).unwrap_or(1.0);
    let wbs = p.get("wbs").and_then(Json::as_str).map(str::to_string);
    s.push_undo();
    let id = s.project.add_task(title, dur, wbs)?;
    if let Some(t) = s.project.task_mut(id) {
        if let Some(kind) = p.get("kind").and_then(Json::as_str) {
            t.kind = ItemKind::parse(kind);
        }
        if let Some(st) = p.get("status").and_then(Json::as_str) {
            t.status = TaskStatus::parse(st);
        }
        if let Some(pr) = p.get("priority").and_then(Json::as_str) {
            t.priority = Priority::parse(pr);
        }
        if let Some(pts) = p.get("story_points").and_then(Json::as_f64) {
            t.story_points = Some(pts.max(0.0));
        }
        if let Some(fc) = p.get("fixed_cost").and_then(Json::as_f64) {
            t.fixed_cost = fc.max(0.0);
        }
    }
    let _ = schedule_project(&mut s.project)?;
    let task = s.project.task(id).cloned();
    Ok(serde_json::to_value(task).unwrap_or_else(|_| json!({"id": id})))
}

fn cmd_task_update(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let id = req_u32("task.update", p, "id")?;
    if s.project.task(id).is_none() {
        return Err(EngineError::Core(CoreError::NotFound(format!("task {id}"))));
    }
    s.push_undo();
    if let Some(t) = s.project.task_mut(id) {
        if let Some(title) = p.get("title").and_then(Json::as_str) {
            if !title.trim().is_empty() {
                t.title = title.to_string();
            }
        }
        if let Some(desc) = p.get("description").and_then(Json::as_str) {
            t.description = desc.to_string();
        }
        if let Some(dur) = p.get("duration_days").and_then(Json::as_f64) {
            t.duration_days = dur.max(0.0);
        }
        if let Some(wbs) = p.get("wbs").and_then(Json::as_str) {
            t.wbs = wbs.to_string();
        }
        if let Some(kind) = p.get("kind").and_then(Json::as_str) {
            t.kind = ItemKind::parse(kind);
        }
        if let Some(st) = p.get("status").and_then(Json::as_str) {
            t.status = TaskStatus::parse(st);
            if t.status == TaskStatus::Done {
                t.progress = 1.0;
            }
        }
        if let Some(pr) = p.get("priority").and_then(Json::as_str) {
            t.priority = Priority::parse(pr);
        }
        if let Some(prog) = p.get("progress").and_then(Json::as_f64) {
            t.progress = prog.clamp(0.0, 1.0);
        }
        if let Some(pts) = p.get("story_points").and_then(Json::as_f64) {
            t.story_points = Some(pts.max(0.0));
        }
        if let Some(sp) = p.get("sprint_id").and_then(Json::as_u64) {
            t.sprint_id = if sp == 0 { None } else { u32::try_from(sp).ok() };
        }
        if let Some(ct) = p.get("constraint_type").and_then(Json::as_str) {
            let cd = p.get("constraint_day").and_then(Json::as_f64);
            t.constraint = TaskConstraint::parse(ct, cd);
        }
        if let Some(o) = p.get("optimistic_days").and_then(Json::as_f64) {
            t.optimistic_days = Some(o.max(0.0));
        }
        if let Some(m) = p.get("most_likely_days").and_then(Json::as_f64) {
            t.most_likely_days = Some(m.max(0.0));
        }
        if let Some(pe) = p.get("pessimistic_days").and_then(Json::as_f64) {
            t.pessimistic_days = Some(pe.max(0.0));
        }
        if let Some(fc) = p.get("fixed_cost").and_then(Json::as_f64) {
            t.fixed_cost = fc.max(0.0);
        }
        if let Some(ac) = p.get("actual_cost").and_then(Json::as_f64) {
            t.actual_cost = ac.max(0.0);
        }
    }
    let _ = schedule_project(&mut s.project)?;
    let task = s.project.task(id).cloned();
    Ok(serde_json::to_value(task).unwrap_or_else(|_| json!({"id": id})))
}

fn cmd_task_delete(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let id = req_u32("task.delete", p, "id")?;
    s.push_undo();
    s.project.delete_task(id)?;
    let _ = schedule_project(&mut s.project)?;
    Ok(json!({"deleted": id, "remaining_tasks": s.project.tasks.len()}))
}

fn cmd_task_list(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let status_filter = p.get("status").and_then(Json::as_str).map(TaskStatus::parse);
    let crit_only = p.get("critical_only").and_then(Json::as_bool).unwrap_or(false);
    let tasks: Vec<&pmcore::Task> =
        s.project.tasks.iter().filter(|t| status_filter.map(|st| t.status == st).unwrap_or(true) && (!crit_only || t.critical)).collect();
    Ok(json!({"tasks": tasks, "count": tasks.len()}))
}

fn cmd_task_set_status(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let id = req_u32("task.set_status", p, "id")?;
    let status_str = req_str("task.set_status", p, "status")?;
    let st = TaskStatus::parse(status_str);
    s.push_undo();
    let Some(t) = s.project.task_mut(id) else {
        return Err(EngineError::Core(CoreError::NotFound(format!("task {id}"))));
    };
    t.status = st;
    if st == TaskStatus::Done {
        t.progress = 1.0;
    } else if st == TaskStatus::Todo && t.progress >= 1.0 {
        t.progress = 0.0;
    }
    Ok(json!({"id": id, "status": st.as_str(), "progress": t.progress}))
}

fn cmd_task_set_progress(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let id = req_u32("task.set_progress", p, "id")?;
    let prog = p.get("progress").and_then(Json::as_f64).ok_or_else(|| EngineError::bad_params("task.set_progress", "missing number `progress`"))?;
    s.push_undo();
    let Some(t) = s.project.task_mut(id) else {
        return Err(EngineError::Core(CoreError::NotFound(format!("task {id}"))));
    };
    t.progress = prog.clamp(0.0, 1.0);
    if t.progress >= 1.0 {
        t.status = TaskStatus::Done;
    } else if t.progress > 0.0 && matches!(t.status, TaskStatus::Backlog | TaskStatus::Todo) {
        t.status = TaskStatus::InProgress;
    }
    Ok(json!({"id": id, "progress": t.progress, "status": t.status.as_str()}))
}

fn cmd_task_set_field(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let id = req_u32("task.set_field", p, "id")?;
    let key = req_str("task.set_field", p, "key")?.to_string();
    let val_json = p.get("value").cloned().unwrap_or(Json::Null);
    let val = match val_json {
        Json::Bool(b) => FieldValue::Bool(b),
        Json::Number(n) => FieldValue::Number(n.as_f64().unwrap_or(0.0)),
        Json::String(st) => FieldValue::Text(st),
        other => FieldValue::Text(other.to_string()),
    };
    s.push_undo();
    let Some(t) = s.project.task_mut(id) else {
        return Err(EngineError::Core(CoreError::NotFound(format!("task {id}"))));
    };
    t.custom_fields.insert(key.clone(), val);
    Ok(json!({"id": id, "key": key}))
}

fn cmd_dep_add(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let pred = req_u32("dep.add", p, "predecessor_id")?;
    let succ = req_u32("dep.add", p, "successor_id")?;
    let kind = p.get("kind").and_then(Json::as_str).map(DepKind::parse).unwrap_or(DepKind::FinishToStart);
    let lag = p.get("lag_days").and_then(Json::as_f64).unwrap_or(0.0);

    let prev = s.project.clone();
    s.project.add_dependency(pred, succ, kind, lag)?;
    match schedule_project(&mut s.project) {
        Ok(summary) => {
            s.undo_stack.push(prev);
            s.redo_stack.clear();
            Ok(serde_json::to_value(summary).unwrap_or_else(|_| json!({})))
        }
        Err(e) => {
            s.project = prev;
            Err(EngineError::Sched(e))
        }
    }
}

fn cmd_dep_remove(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let pred = req_u32("dep.remove", p, "predecessor_id")?;
    let succ = req_u32("dep.remove", p, "successor_id")?;
    s.push_undo();
    let removed = s.project.remove_dependency(pred, succ);
    let summary = schedule_project(&mut s.project)?;
    Ok(json!({"removed": removed, "project_duration_days": summary.project_duration_days}))
}

fn cmd_schedule_run(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let summary = schedule_project(&mut s.project)?;
    Ok(serde_json::to_value(summary).unwrap_or_else(|_| json!({})))
}

fn cmd_schedule_critical_path(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let summary = schedule_project(&mut s.project)?;
    let crit_tasks: Vec<Json> = s
        .project
        .tasks
        .iter()
        .filter(|t| t.critical)
        .map(|t| json!({"id": t.id, "key": t.key, "wbs": t.wbs, "title": t.title, "early_start": t.early_start, "early_finish": t.early_finish}))
        .collect();
    Ok(json!({
        "project_duration_days": summary.project_duration_days,
        "critical_tasks": crit_tasks,
    }))
}

fn cmd_schedule_monte_carlo(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let trials = p.get("trials").and_then(Json::as_u64).map(|v| v as usize).unwrap_or(500);
    let seed = p.get("seed").and_then(Json::as_u64).unwrap_or(42);
    let _ = schedule_project(&mut s.project)?;
    let report = run_monte_carlo(&s.project, trials, seed)?;
    let out = serde_json::to_value(&report).unwrap_or_else(|_| json!({}));
    s.last_monte_carlo = Some(report);
    Ok(out)
}

fn cmd_baseline_capture(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let default_name = format!("Baseline {}", s.project.baselines.len() + 1);
    let name = p.get("name").and_then(Json::as_str).unwrap_or(&default_name).to_string();
    s.push_undo();
    let id = s.project.capture_baseline(name.clone());
    Ok(json!({"baseline_id": id, "name": name, "tasks": s.project.tasks.len()}))
}

fn cmd_baseline_variance(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let bid = p.get("baseline_id").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok());
    let rows = compute_baseline_variance(&s.project, bid);
    Ok(json!({"variances": rows, "count": rows.len()}))
}

fn cmd_resource_add(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let name = req_str("resource.add", p, "name")?;
    let role = p.get("role").and_then(Json::as_str).unwrap_or("Contributor");
    let max_units = p.get("max_units").and_then(Json::as_f64).unwrap_or(1.0);
    let rate = p.get("standard_rate_per_day").and_then(Json::as_f64).unwrap_or(600.0);
    s.push_undo();
    let id = s.project.add_resource(name, role, max_units, rate)?;
    Ok(json!({"resource_id": id, "name": name}))
}

fn cmd_resource_assign(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let task_id = req_u32("resource.assign", p, "task_id")?;
    let res_id = req_u32("resource.assign", p, "resource_id")?;
    let units = p.get("units").and_then(Json::as_f64).unwrap_or(1.0);
    s.push_undo();
    s.project.assign_resource(task_id, res_id, units)?;
    let workload = compute_workload(&s.project);
    Ok(json!({
        "task_id": task_id,
        "resource_id": res_id,
        "units": units,
        "overallocated_person_days": workload.total_overallocated_person_days
    }))
}

fn cmd_resource_workload(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let report = compute_workload(&s.project);
    Ok(serde_json::to_value(report).unwrap_or_else(|_| json!({})))
}

fn cmd_resource_level(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    s.push_undo();
    let res = level_resources(&mut s.project)?;
    Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({})))
}

fn cmd_evm_compute(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let status_day = p.get("status_day").and_then(Json::as_f64);
    let evm = compute_evm(&s.project, status_day);
    Ok(serde_json::to_value(evm).unwrap_or_else(|_| json!({})))
}

fn cmd_sprint_add(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let name = req_str("sprint.add", p, "name")?;
    let goal = p.get("goal").and_then(Json::as_str).unwrap_or("");
    let start_day = p.get("start_day").and_then(Json::as_f64).unwrap_or(0.0);
    let end_day = p.get("end_day").and_then(Json::as_f64).unwrap_or(start_day + 10.0);
    let cap = p.get("capacity_points").and_then(Json::as_f64).unwrap_or(25.0);
    s.push_undo();
    let id = s.project.add_sprint(name, goal, start_day, end_day, cap);
    Ok(json!({"sprint_id": id, "name": name}))
}

fn cmd_sprint_burndown(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let sid = p.get("sprint_id").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok());
    let bd =
        compute_sprint_burndown(&s.project, sid).ok_or_else(|| EngineError::Core(CoreError::NotFound("no matching sprint found".to_string())))?;
    Ok(serde_json::to_value(bd).unwrap_or_else(|_| json!({})))
}

fn cmd_export_csv(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let csv = to_csv(&s.project);
    let written = maybe_write_path(p, &csv)?;
    Ok(json!({"csv": csv, "path": written, "bytes": csv.len()}))
}

fn cmd_import_csv(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let raw = read_text_or_path("import.csv", p, "csv")?;
    let name = p.get("name").and_then(Json::as_str).unwrap_or("Imported CSV Project");
    let proj = from_csv(&raw, name)?;
    s.push_undo();
    s.project = proj;
    cmd_project_summary(s, &json!({}))
}

fn cmd_export_jira(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let out = to_jira_json(&s.project);
    let written = maybe_write_path(p, &out)?;
    Ok(json!({"jira_json": out, "path": written, "bytes": out.len()}))
}

fn cmd_import_jira(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let raw = read_text_or_path("import.jira", p, "json")?;
    let proj = from_jira_json(&raw)?;
    s.push_undo();
    s.project = proj;
    cmd_project_summary(s, &json!({}))
}

fn cmd_export_mspdi(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let xml = to_mspdi_xml(&s.project);
    let written = maybe_write_path(p, &xml)?;
    Ok(json!({"xml": xml, "path": written, "bytes": xml.len()}))
}

fn cmd_import_mspdi(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let raw = read_text_or_path("import.mspdi", p, "xml")?;
    let proj = from_mspdi_xml(&raw)?;
    s.push_undo();
    s.project = proj;
    cmd_project_summary(s, &json!({}))
}

fn cmd_export_xer(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let xer = to_primavera_xer(&s.project);
    let written = maybe_write_path(p, &xer)?;
    Ok(json!({"xer": xer, "path": written, "bytes": xer.len()}))
}

fn cmd_import_xer(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let raw = read_text_or_path("import.xer", p, "xer")?;
    let proj = from_primavera_xer(&raw)?;
    s.push_undo();
    s.project = proj;
    cmd_project_summary(s, &json!({}))
}

fn cmd_export_gantt_svg(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let svg = render_gantt_svg(&s.project);
    let written = maybe_write_path(p, &svg)?;
    Ok(json!({"svg": svg, "path": written, "bytes": svg.len()}))
}

fn cmd_export_report_html(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = schedule_project(&mut s.project)?;
    let html = render_report_html(&s.project);
    let written = maybe_write_path(p, &html)?;
    Ok(json!({"html": html, "path": written, "bytes": html.len()}))
}

fn cmd_sync_add_comment(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let tid = req_u32("sync.add_comment", p, "task_id")?;
    let body = req_str("sync.add_comment", p, "body")?;
    let author = p.get("author").and_then(Json::as_str).unwrap_or(&s.active_author).to_string();
    let role = p.get("role").and_then(Json::as_str).map(SyncRole::parse).unwrap_or(s.active_role);
    if s.project.task(tid).is_none() {
        return Err(EngineError::Core(CoreError::NotFound(format!("task {tid}"))));
    }
    let comment = s.sync_bundle.add_comment(author, role, tid, body)?;
    Ok(serde_json::to_value(comment).unwrap_or_else(|_| json!({})))
}

fn cmd_sync_activity_feed(s: &mut Session, _p: &Json) -> Result<Json, EngineError> {
    let conflicts = s.sync_bundle.inspect_post_merge_conflicts(&mut s.project);
    Ok(json!({
        "head_change_id": s.sync_bundle.head_change_id,
        "envelopes": s.sync_bundle.envelopes,
        "comments": s.sync_bundle.comments,
        "conflicts": conflicts,
    }))
}

fn cmd_sync_check_permission(_s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let role_str = req_str("sync.check_permission", p, "role")?;
    let command = req_str("sync.check_permission", p, "command")?;
    let role = SyncRole::parse(role_str);
    let allowed = role.allows_command(command);
    Ok(json!({"role": role.as_str(), "command": command, "allowed": allowed}))
}

fn cmd_sync_export_bundle(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let _ = s.sync_bundle.inspect_post_merge_conflicts(&mut s.project);
    let text = serde_json::to_string_pretty(&s.sync_bundle).map_err(|e| EngineError::Io(e.to_string()))?;
    let written = maybe_write_path(p, &text)?;
    Ok(json!({"bundle": s.sync_bundle, "path": written, "bytes": text.len()}))
}

fn cmd_sync_import_bundle(s: &mut Session, p: &Json) -> Result<Json, EngineError> {
    let remote: SyncBundle = if let Some(b) = p.get("bundle") {
        serde_json::from_value(b.clone()).map_err(|e| EngineError::bad_params("sync.import_bundle", e.to_string()))?
    } else {
        let raw = read_text_or_path("sync.import_bundle", p, "json")?;
        serde_json::from_str(&raw).map_err(|e| EngineError::bad_params("sync.import_bundle", e.to_string()))?
    };
    let merged = s.sync_bundle.merge_remote(&remote);
    let conflicts = s.sync_bundle.inspect_post_merge_conflicts(&mut s.project);
    Ok(json!({
        "merged_envelopes": merged,
        "total_envelopes": s.sync_bundle.envelopes.len(),
        "total_comments": s.sync_bundle.comments.len(),
        "conflicts": conflicts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_engine_executes_and_rolls_back_cycles() {
        let mut s = Session::new();
        let sum = s.execute("project.summary", json!({})).unwrap();
        assert_eq!(sum["tasks"], json!(8));

        // Adding a cycle via dep.add errors and preserves project state
        let bad = s.execute("dep.add", json!({"predecessor_id": 8, "successor_id": 1}));
        assert!(bad.is_err());
        assert_eq!(s.project().dependencies.len(), 9);

        // Resource levelling resolves the seeded overallocation
        let lvl = s.execute("resource.level", json!({})).unwrap();
        assert_eq!(lvl["after_overallocated_days"], json!(0));

        // Undo restores the pre-levelled schedule
        let u = s.execute("project.undo", json!({})).unwrap();
        assert_eq!(u["undone"], json!(true));
    }
}
