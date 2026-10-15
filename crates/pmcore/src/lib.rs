//! `pmcore` (L0) — ProjectCraft core project model.
//!
//! Holds the unified document model shared by every lens:
//! - Work items (tasks, milestones, phases, epics, stories, bugs) with WBS codes
//! - Dependencies (`FS`, `SS`, `FF`, `SF`) with positive/negative lag (leads/lags)
//! - Task constraints (`ASAP`, `ALAP`, `SNET`, `SNLT`, `FNET`, `FNLT`, `MSO`, `MFO`)
//! - Calendars (working weekdays, hours/day, holidays)
//! - Resources, effective-dated rate tables, and task resource assignments
//! - Agile sprints, story points, WIP limits, and checklists
//! - Multiple named baselines for schedule & cost variance tracking
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum tasks permitted in a single project to bound untrusted file allocations.
pub const MAX_TASKS: usize = 100_000;

/// Core domain error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    NotFound(String),
    Validation(String),
    LimitExceeded(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::NotFound(m) => write!(f, "not found: {m}"),
            CoreError::Validation(m) => write!(f, "validation error: {m}"),
            CoreError::LimitExceeded(m) => write!(f, "limit exceeded: {m}"),
        }
    }
}

impl std::error::Error for CoreError {}

/// Work item classification across traditional PM and agile workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Task,
    Milestone,
    Phase,
    Epic,
    Story,
    Bug,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Task => "task",
            ItemKind::Milestone => "milestone",
            ItemKind::Phase => "phase",
            ItemKind::Epic => "epic",
            ItemKind::Story => "story",
            ItemKind::Bug => "bug",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "milestone" => ItemKind::Milestone,
            "phase" | "summary" => ItemKind::Phase,
            "epic" => ItemKind::Epic,
            "story" | "user_story" => ItemKind::Story,
            "bug" | "defect" => ItemKind::Bug,
            _ => ItemKind::Task,
        }
    }
}

/// Workflow status used by Kanban/Scrum boards and table filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Backlog,
    Todo,
    InProgress,
    Blocked,
    Review,
    Done,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 6] =
        [TaskStatus::Backlog, TaskStatus::Todo, TaskStatus::InProgress, TaskStatus::Blocked, TaskStatus::Review, TaskStatus::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Backlog => "Backlog",
            TaskStatus::Todo => "To Do",
            TaskStatus::InProgress => "In Progress",
            TaskStatus::Blocked => "Blocked",
            TaskStatus::Review => "Review",
            TaskStatus::Done => "Done",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "backlog" => TaskStatus::Backlog,
            "in_progress" | "inprogress" | "doing" | "active" => TaskStatus::InProgress,
            "blocked" | "on_hold" => TaskStatus::Blocked,
            "review" | "in_review" | "qa" => TaskStatus::Review,
            "done" | "closed" | "complete" | "completed" => TaskStatus::Done,
            _ => TaskStatus::Todo,
        }
    }
}

/// Work item priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
            Priority::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" | "minor" | "trivial" => Priority::Low,
            "high" | "major" => Priority::High,
            "critical" | "blocker" | "urgent" => Priority::Critical,
            _ => Priority::Medium,
        }
    }

    /// Numeric weight used by priority-rule resource levelling (higher schedules first).
    pub fn weight(self) -> u8 {
        match self {
            Priority::Low => 1,
            Priority::Medium => 2,
            Priority::High => 3,
            Priority::Critical => 4,
        }
    }
}

/// Precedence diagramming method (PDM) dependency type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepKind {
    /// Finish-to-Start (`FS`): successor can start once predecessor finishes + lag.
    #[serde(rename = "FS")]
    FinishToStart,
    /// Start-to-Start (`SS`): successor can start once predecessor starts + lag.
    #[serde(rename = "SS")]
    StartToStart,
    /// Finish-to-Finish (`FF`): successor can finish once predecessor finishes + lag.
    #[serde(rename = "FF")]
    FinishToFinish,
    /// Start-to-Finish (`SF`): successor can finish once predecessor starts + lag.
    #[serde(rename = "SF")]
    StartToFinish,
}

impl DepKind {
    pub fn code(self) -> &'static str {
        match self {
            DepKind::FinishToStart => "FS",
            DepKind::StartToStart => "SS",
            DepKind::FinishToFinish => "FF",
            DepKind::StartToFinish => "SF",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "SS" | "START_TO_START" | "PR_SS" => DepKind::StartToStart,
            "FF" | "FINISH_TO_FINISH" | "PR_FF" => DepKind::FinishToFinish,
            "SF" | "START_TO_FINISH" | "PR_SF" => DepKind::StartToFinish,
            _ => DepKind::FinishToStart,
        }
    }
}

/// Scheduling constraint on a task (in working days from project start `0.0`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "day", rename_all = "snake_case")]
pub enum TaskConstraint {
    Asap,
    Alap,
    Snet(f64),
    Snlt(f64),
    Fnet(f64),
    Fnlt(f64),
    Mso(f64),
    Mfo(f64),
}

impl TaskConstraint {
    pub fn label(self) -> String {
        match self {
            TaskConstraint::Asap => "ASAP".to_string(),
            TaskConstraint::Alap => "ALAP".to_string(),
            TaskConstraint::Snet(d) => format!("SNET d{d:.1}"),
            TaskConstraint::Snlt(d) => format!("SNLT d{d:.1}"),
            TaskConstraint::Fnet(d) => format!("FNET d{d:.1}"),
            TaskConstraint::Fnlt(d) => format!("FNLT d{d:.1}"),
            TaskConstraint::Mso(d) => format!("MSO d{d:.1}"),
            TaskConstraint::Mfo(d) => format!("MFO d{d:.1}"),
        }
    }

    pub fn parse(kind: &str, day: Option<f64>) -> Self {
        let d = day.unwrap_or(0.0).max(0.0);
        match kind.trim().to_ascii_uppercase().as_str() {
            "ALAP" => TaskConstraint::Alap,
            "SNET" => TaskConstraint::Snet(d),
            "SNLT" => TaskConstraint::Snlt(d),
            "FNET" => TaskConstraint::Fnet(d),
            "FNLT" => TaskConstraint::Fnlt(d),
            "MSO" => TaskConstraint::Mso(d),
            "MFO" => TaskConstraint::Mfo(d),
            _ => TaskConstraint::Asap,
        }
    }
}

/// Custom field value on a work item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::Bool(b) => write!(f, "{b}"),
            FieldValue::Number(n) => write!(f, "{n}"),
            FieldValue::Text(s) => write!(f, "{s}"),
        }
    }
}

/// Lightweight checklist sub-item inside a task card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub text: String,
    pub done: bool,
}

/// A single work item — simultaneously a WBS row, Gantt bar, Kanban card, and EVM cost account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u32,
    pub key: String,
    pub wbs: String,
    pub parent_id: Option<u32>,
    pub title: String,
    pub description: String,
    pub kind: ItemKind,
    pub status: TaskStatus,
    pub priority: Priority,
    /// Planned duration in working days (`0.0` for milestones).
    pub duration_days: f64,
    /// Optional three-point PERT / Monte Carlo optimistic duration (days).
    pub optimistic_days: Option<f64>,
    /// Optional three-point PERT / Monte Carlo most-likely duration (days).
    pub most_likely_days: Option<f64>,
    /// Optional three-point PERT / Monte Carlo pessimistic duration (days).
    pub pessimistic_days: Option<f64>,
    /// Completion fraction in `0.0..=1.0`.
    pub progress: f64,
    pub story_points: Option<f64>,
    pub sprint_id: Option<u32>,
    pub constraint: TaskConstraint,
    /// Fixed non-labor cost assigned directly to the task.
    pub fixed_cost: f64,
    /// Actual cost incurred to date (`AC` / `ACWP`).
    pub actual_cost: f64,
    /// Optional actual start day (working days from project start).
    pub actual_start_day: Option<f64>,
    /// Optional actual duration logged so far (working days).
    pub actual_duration_days: Option<f64>,
    pub custom_fields: BTreeMap<String, FieldValue>,
    pub checklist: Vec<ChecklistItem>,

    // Computed schedule fields (updated by `pmsched::schedule_project`)
    pub early_start: f64,
    pub early_finish: f64,
    pub late_start: f64,
    pub late_finish: f64,
    pub total_slack: f64,
    pub free_slack: f64,
    pub critical: bool,
    /// Delay in working days added by resource levelling.
    pub levelled_delay: f64,
}

impl Task {
    pub fn new(id: u32, key: impl Into<String>, wbs: impl Into<String>, title: impl Into<String>, duration_days: f64) -> Self {
        let dur = duration_days.max(0.0);
        Task {
            id,
            key: key.into(),
            wbs: wbs.into(),
            parent_id: None,
            title: title.into(),
            description: String::new(),
            kind: if dur == 0.0 { ItemKind::Milestone } else { ItemKind::Task },
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            duration_days: dur,
            optimistic_days: None,
            most_likely_days: None,
            pessimistic_days: None,
            progress: 0.0,
            story_points: None,
            sprint_id: None,
            constraint: TaskConstraint::Asap,
            fixed_cost: 0.0,
            actual_cost: 0.0,
            actual_start_day: None,
            actual_duration_days: None,
            custom_fields: BTreeMap::new(),
            checklist: Vec::new(),
            early_start: 0.0,
            early_finish: dur,
            late_start: 0.0,
            late_finish: dur,
            total_slack: 0.0,
            free_slack: 0.0,
            critical: false,
            levelled_delay: 0.0,
        }
    }

    /// Three-point PERT expected duration `(O + 4M + P) / 6`, falling back to `duration_days`.
    pub fn pert_expected_days(&self) -> f64 {
        match (self.optimistic_days, self.most_likely_days, self.pessimistic_days) {
            (Some(o), Some(m), Some(p)) if o >= 0.0 && m >= o && p >= m => (o + 4.0 * m + p) / 6.0,
            _ => self.duration_days,
        }
    }

    /// Three-point PERT standard deviation `(P - O) / 6`.
    pub fn pert_std_dev(&self) -> f64 {
        match (self.optimistic_days, self.pessimistic_days) {
            (Some(o), Some(p)) if p >= o => (p - o) / 6.0,
            _ => 0.0,
        }
    }

    /// WBS depth (0-based) derived from dot-separated segments (`"1"` -> 0, `"1.2.1"` -> 2).
    pub fn wbs_depth(&self) -> usize {
        self.wbs.split('.').filter(|s| !s.is_empty()).count().saturating_sub(1)
    }
}

/// Directed dependency edge between two tasks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    pub predecessor_id: u32,
    pub successor_id: u32,
    pub kind: DepKind,
    /// Lag in working days (negative values represent a lead).
    pub lag_days: f64,
}

/// Effective-dated cost rate entry in a resource's rate table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateEntry {
    pub effective_from_day: f64,
    pub rate_per_day: f64,
}

/// A project resource (person, role, or equipment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: u32,
    pub name: String,
    pub role: String,
    /// Maximum availability in units (`1.0` = 100% full-time equivalent).
    pub max_units: f64,
    pub standard_rate_per_day: f64,
    pub overtime_rate_per_day: f64,
    pub cost_per_use: f64,
    pub rate_table: Vec<RateEntry>,
    pub calendar_id: Option<u32>,
}

impl Resource {
    /// Resolve the active daily rate on a given working day using `rate_table` if populated.
    pub fn rate_on_day(&self, day: f64) -> f64 {
        let mut active = self.standard_rate_per_day;
        let mut best_start = f64::NEG_INFINITY;
        for entry in &self.rate_table {
            if entry.effective_from_day <= day && entry.effective_from_day >= best_start {
                best_start = entry.effective_from_day;
                active = entry.rate_per_day;
            }
        }
        active
    }
}

/// Assignment of a resource to a task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub task_id: u32,
    pub resource_id: u32,
    /// Allocation fraction (`1.0` = 100%).
    pub units: f64,
}

/// Working calendar definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calendar {
    pub id: u32,
    pub name: String,
    pub hours_per_day: f64,
    /// `Mon..Sun` (`true` = working day).
    pub working_weekdays: [bool; 7],
    /// 0-based calendar day offsets from project start that are non-working holidays.
    pub non_working_days: Vec<u32>,
}

impl Default for Calendar {
    fn default() -> Self {
        Calendar {
            id: 1,
            name: "Standard (Mon-Fri, 8h/d)".to_string(),
            hours_per_day: 8.0,
            working_weekdays: [true, true, true, true, true, false, false],
            non_working_days: Vec::new(),
        }
    }
}

impl Calendar {
    /// Convert working-day offset `work_day` into a calendar-day offset from project start,
    /// skipping weekends and `non_working_days`.
    pub fn work_day_to_calendar_day(&self, work_day: u32) -> u32 {
        if !self.working_weekdays.iter().any(|&w| w) {
            return work_day;
        }
        let mut remaining = work_day;
        let mut cal_day: u32 = 0;
        let max_iter = work_day.saturating_mul(10).saturating_add(365);
        for _ in 0..max_iter {
            let weekday = (cal_day as usize) % 7;
            let is_work = self.working_weekdays.get(weekday).copied().unwrap_or(true) && !self.non_working_days.contains(&cal_day);
            if is_work {
                if remaining == 0 {
                    return cal_day;
                }
                remaining = remaining.saturating_sub(1);
            }
            cal_day = cal_day.saturating_add(1);
        }
        cal_day
    }
}

/// Agile sprint container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprint {
    pub id: u32,
    pub name: String,
    pub goal: String,
    pub start_day: f64,
    pub end_day: f64,
    pub capacity_points: f64,
}

/// Frozen snapshot of a single task inside a named [`Baseline`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineTask {
    pub task_id: u32,
    pub start_day: f64,
    pub finish_day: f64,
    pub duration_days: f64,
    pub planned_cost: f64,
}

/// Named project baseline (`Baseline 0`, `Rebaseline Q2`, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub id: u32,
    pub name: String,
    pub status_day: f64,
    pub tasks: Vec<BaselineTask>,
}

/// Top-level `.projectcraft` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub code: String,
    pub description: String,
    /// Current status date in working days from project start (used by EVM).
    pub status_day: f64,
    pub calendars: Vec<Calendar>,
    pub tasks: Vec<Task>,
    pub dependencies: Vec<Dependency>,
    pub resources: Vec<Resource>,
    pub assignments: Vec<Assignment>,
    pub sprints: Vec<Sprint>,
    pub baselines: Vec<Baseline>,
    /// Optional WIP limits per status key (`"in_progress": 3`).
    pub wip_limits: BTreeMap<String, usize>,
    next_task_id: u32,
    next_resource_id: u32,
    next_sprint_id: u32,
    next_baseline_id: u32,
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled Project", "PC")
    }
}

impl Project {
    pub fn new(name: impl Into<String>, code: impl Into<String>) -> Self {
        let mut wip_limits = BTreeMap::new();
        wip_limits.insert(TaskStatus::InProgress.as_str().to_string(), 4);
        wip_limits.insert(TaskStatus::Review.as_str().to_string(), 3);
        Project {
            format: "projectcraft".to_string(),
            version: 1,
            name: name.into(),
            code: code.into(),
            description: String::new(),
            status_day: 10.0,
            calendars: vec![Calendar::default()],
            tasks: Vec::new(),
            dependencies: Vec::new(),
            resources: Vec::new(),
            assignments: Vec::new(),
            sprints: Vec::new(),
            baselines: Vec::new(),
            wip_limits,
            next_task_id: 1,
            next_resource_id: 1,
            next_sprint_id: 1,
            next_baseline_id: 1,
        }
    }

    /// Reconcile internal monotonic ID counters after deserializing or importing.
    pub fn repair_counters(&mut self) {
        self.next_task_id = self.tasks.iter().map(|t| t.id).max().unwrap_or(0).saturating_add(1);
        self.next_resource_id = self.resources.iter().map(|r| r.id).max().unwrap_or(0).saturating_add(1);
        self.next_sprint_id = self.sprints.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1);
        self.next_baseline_id = self.baselines.iter().map(|b| b.id).max().unwrap_or(0).saturating_add(1);
        if self.calendars.is_empty() {
            self.calendars.push(Calendar::default());
        }
    }

    pub fn add_task(&mut self, title: impl Into<String>, duration_days: f64, wbs: Option<String>) -> Result<u32, CoreError> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(CoreError::LimitExceeded(format!("project cannot exceed {MAX_TASKS} tasks")));
        }
        let title = title.into();
        if title.trim().is_empty() {
            return Err(CoreError::Validation("task title cannot be empty".to_string()));
        }
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        let key = format!("{}-{id}", self.code);
        let wbs_code = wbs.unwrap_or_else(|| format!("{}", self.tasks.len() + 1));
        let task = Task::new(id, key, wbs_code, title, duration_days);
        self.tasks.push(task);
        Ok(id)
    }

    pub fn task(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: u32) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn delete_task(&mut self, id: u32) -> Result<(), CoreError> {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        if self.tasks.len() == before {
            return Err(CoreError::NotFound(format!("task {id}")));
        }
        self.dependencies.retain(|d| d.predecessor_id != id && d.successor_id != id);
        self.assignments.retain(|a| a.task_id != id);
        for t in &mut self.tasks {
            if t.parent_id == Some(id) {
                t.parent_id = None;
            }
        }
        Ok(())
    }

    pub fn add_dependency(&mut self, pred: u32, succ: u32, kind: DepKind, lag_days: f64) -> Result<(), CoreError> {
        if pred == succ {
            return Err(CoreError::Validation("a task cannot depend on itself".to_string()));
        }
        if self.task(pred).is_none() {
            return Err(CoreError::NotFound(format!("predecessor task {pred}")));
        }
        if self.task(succ).is_none() {
            return Err(CoreError::NotFound(format!("successor task {succ}")));
        }
        if let Some(existing) = self.dependencies.iter_mut().find(|d| d.predecessor_id == pred && d.successor_id == succ) {
            existing.kind = kind;
            existing.lag_days = lag_days;
            return Ok(());
        }
        self.dependencies.push(Dependency { predecessor_id: pred, successor_id: succ, kind, lag_days });
        Ok(())
    }

    pub fn remove_dependency(&mut self, pred: u32, succ: u32) -> bool {
        let before = self.dependencies.len();
        self.dependencies.retain(|d| !(d.predecessor_id == pred && d.successor_id == succ));
        self.dependencies.len() < before
    }

    pub fn add_resource(
        &mut self,
        name: impl Into<String>,
        role: impl Into<String>,
        max_units: f64,
        standard_rate_per_day: f64,
    ) -> Result<u32, CoreError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(CoreError::Validation("resource name cannot be empty".to_string()));
        }
        let id = self.next_resource_id;
        self.next_resource_id = self.next_resource_id.saturating_add(1);
        self.resources.push(Resource {
            id,
            name,
            role: role.into(),
            max_units: max_units.max(0.1),
            standard_rate_per_day: standard_rate_per_day.max(0.0),
            overtime_rate_per_day: (standard_rate_per_day * 1.5).max(0.0),
            cost_per_use: 0.0,
            rate_table: Vec::new(),
            calendar_id: Some(1),
        });
        Ok(id)
    }

    pub fn assign_resource(&mut self, task_id: u32, resource_id: u32, units: f64) -> Result<(), CoreError> {
        if self.task(task_id).is_none() {
            return Err(CoreError::NotFound(format!("task {task_id}")));
        }
        if !self.resources.iter().any(|r| r.id == resource_id) {
            return Err(CoreError::NotFound(format!("resource {resource_id}")));
        }
        let u = units.max(0.0);
        if let Some(existing) = self.assignments.iter_mut().find(|a| a.task_id == task_id && a.resource_id == resource_id) {
            existing.units = u;
            return Ok(());
        }
        self.assignments.push(Assignment { task_id, resource_id, units: u });
        Ok(())
    }

    pub fn add_sprint(&mut self, name: impl Into<String>, goal: impl Into<String>, start_day: f64, end_day: f64, capacity_points: f64) -> u32 {
        let id = self.next_sprint_id;
        self.next_sprint_id = self.next_sprint_id.saturating_add(1);
        self.sprints.push(Sprint {
            id,
            name: name.into(),
            goal: goal.into(),
            start_day: start_day.max(0.0),
            end_day: end_day.max(start_day),
            capacity_points: capacity_points.max(0.0),
        });
        id
    }

    /// Planned total cost (fixed cost + assigned resource labor + cost-per-use) for a single task.
    pub fn planned_task_cost(&self, task_id: u32) -> f64 {
        let Some(task) = self.task(task_id) else {
            return 0.0;
        };
        let mut total = task.fixed_cost;
        for a in self.assignments.iter().filter(|a| a.task_id == task_id) {
            if let Some(res) = self.resources.iter().find(|r| r.id == a.resource_id) {
                let rate = res.rate_on_day(task.early_start);
                total += res.cost_per_use + task.duration_days * a.units * rate;
            }
        }
        total
    }

    /// Capture a named baseline from the current scheduled dates and planned costs.
    pub fn capture_baseline(&mut self, name: impl Into<String>) -> u32 {
        let id = self.next_baseline_id;
        self.next_baseline_id = self.next_baseline_id.saturating_add(1);
        let tasks: Vec<BaselineTask> = self
            .tasks
            .iter()
            .map(|t| BaselineTask {
                task_id: t.id,
                start_day: t.early_start,
                finish_day: t.early_finish,
                duration_days: t.duration_days,
                planned_cost: self.planned_task_cost(t.id),
            })
            .collect();
        self.baselines.push(Baseline { id, name: name.into(), status_day: self.status_day, tasks });
        id
    }

    /// Build a rich, realistic cross-functional engineering program demonstrating WBS,
    /// FS/SS/FF dependencies, constraints, resource overallocation, agile sprints,
    /// three-point PERT risk estimates, and baseline/EVM tracking.
    pub fn sample_launch_program() -> Self {
        let mut p = Project::new("Orion Edge Compute Launch", "ORN");
        p.description = "Cross-functional firmware, Rust SDK, and cloud relay release program.".to_string();
        p.status_day = 10.0;

        let s1 = p.add_sprint("Sprint 1 — Foundation", "Freeze protocol & scheduler core", 0.0, 10.0, 26.0);
        let s2 = p.add_sprint("Sprint 2 — Integration", "End-to-end sync & release candidate", 10.0, 22.0, 28.0);

        let r_alice = p.add_resource("Alice Mansour", "Systems Architect", 1.0, 800.0).unwrap_or(1);
        let r_omar = p.add_resource("Omar Haddad", "Rust Kernel Engineer", 1.0, 750.0).unwrap_or(2);
        let r_lina = p.add_resource("Lina Rossi", "UI & WASM Engineer", 1.0, 700.0).unwrap_or(3);
        let r_qa = p.add_resource("Tariq Chen", "QA & Release Lead", 1.0, 650.0).unwrap_or(4);

        if let Some(res) = p.resources.iter_mut().find(|r| r.id == r_omar) {
            res.rate_table.push(RateEntry { effective_from_day: 0.0, rate_per_day: 750.0 });
            res.rate_table.push(RateEntry { effective_from_day: 12.0, rate_per_day: 820.0 });
        }

        let t1 = p.add_task("System Architecture & Protocol Spec", 4.0, Some("1.1".into())).unwrap_or(1);
        let t2 = p.add_task("CRDT Sync Engine & Storage Schema", 6.0, Some("1.2".into())).unwrap_or(2);
        let t3 = p.add_task("Parallel CPM & Levelling Kernel", 5.0, Some("1.3".into())).unwrap_or(3);
        let t4 = p.add_task("Architecture Freeze Milestone", 0.0, Some("1.4".into())).unwrap_or(4);
        let t5 = p.add_task("Studio Gantt & Kanban UI", 6.0, Some("2.1".into())).unwrap_or(5);
        let t6 = p.add_task("MSPDI XML & Primavera XER Interop", 4.0, Some("2.2".into())).unwrap_or(6);
        let t7 = p.add_task("Adversarial Fuzz & Load Verification", 4.0, Some("3.1".into())).unwrap_or(7);
        let t8 = p.add_task("v1.0 GA Release Sign-off", 0.0, Some("3.2".into())).unwrap_or(8);

        // Configure task metadata, statuses, PERT 3-point estimates, actual costs & agile points
        if let Some(t) = p.task_mut(t1) {
            t.kind = ItemKind::Epic;
            t.status = TaskStatus::Done;
            t.priority = Priority::Critical;
            t.progress = 1.0;
            t.story_points = Some(5.0);
            t.sprint_id = Some(s1);
            t.optimistic_days = Some(3.0);
            t.most_likely_days = Some(4.0);
            t.pessimistic_days = Some(6.0);
            t.fixed_cost = 400.0;
            t.actual_cost = 3500.0;
            t.early_start = 0.0;
            t.early_finish = 4.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("spec".into()));
        }
        if let Some(t) = p.task_mut(t2) {
            t.kind = ItemKind::Story;
            t.status = TaskStatus::Done;
            t.priority = Priority::Critical;
            t.progress = 1.0;
            t.story_points = Some(8.0);
            t.sprint_id = Some(s1);
            t.optimistic_days = Some(5.0);
            t.most_likely_days = Some(6.0);
            t.pessimistic_days = Some(10.0);
            t.fixed_cost = 250.0;
            t.actual_cost = 5100.0;
            t.early_start = 4.0;
            t.early_finish = 10.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("storage".into()));
        }
        if let Some(t) = p.task_mut(t3) {
            t.kind = ItemKind::Story;
            t.status = TaskStatus::InProgress;
            t.priority = Priority::High;
            t.progress = 0.65;
            t.story_points = Some(8.0);
            t.sprint_id = Some(s1);
            t.optimistic_days = Some(4.0);
            t.most_likely_days = Some(5.0);
            t.pessimistic_days = Some(9.0);
            t.fixed_cost = 200.0;
            t.actual_cost = 3100.0;
            t.early_start = 4.0;
            t.early_finish = 9.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("scheduler".into()));
        }
        if let Some(t) = p.task_mut(t4) {
            t.kind = ItemKind::Milestone;
            t.status = TaskStatus::Review;
            t.priority = Priority::Critical;
            t.progress = 0.0;
            t.sprint_id = Some(s1);
            t.early_start = 10.0;
            t.early_finish = 10.0;
        }
        if let Some(t) = p.task_mut(t5) {
            t.kind = ItemKind::Story;
            t.status = TaskStatus::InProgress;
            t.priority = Priority::High;
            t.progress = 0.35;
            t.story_points = Some(13.0);
            t.sprint_id = Some(s2);
            t.optimistic_days = Some(5.0);
            t.most_likely_days = Some(6.0);
            t.pessimistic_days = Some(11.0);
            t.fixed_cost = 300.0;
            t.actual_cost = 1800.0;
            t.early_start = 10.0;
            t.early_finish = 16.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("studio-ui".into()));
        }
        if let Some(t) = p.task_mut(t6) {
            t.kind = ItemKind::Task;
            t.status = TaskStatus::Todo;
            t.priority = Priority::Medium;
            t.progress = 0.0;
            t.story_points = Some(5.0);
            t.sprint_id = Some(s2);
            t.optimistic_days = Some(3.0);
            t.most_likely_days = Some(4.0);
            t.pessimistic_days = Some(7.0);
            t.early_start = 10.0;
            t.early_finish = 14.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("interop".into()));
        }
        if let Some(t) = p.task_mut(t7) {
            t.kind = ItemKind::Bug;
            t.status = TaskStatus::Todo;
            t.priority = Priority::Critical;
            t.progress = 0.0;
            t.story_points = Some(8.0);
            t.sprint_id = Some(s2);
            t.optimistic_days = Some(3.0);
            t.most_likely_days = Some(4.0);
            t.pessimistic_days = Some(8.0);
            t.early_start = 16.0;
            t.early_finish = 20.0;
            t.custom_fields.insert("component".into(), FieldValue::Text("qa".into()));
        }
        if let Some(t) = p.task_mut(t8) {
            t.kind = ItemKind::Milestone;
            t.status = TaskStatus::Backlog;
            t.priority = Priority::Critical;
            t.progress = 0.0;
            t.sprint_id = Some(s2);
            t.early_start = 20.0;
            t.early_finish = 20.0;
        }

        // Dependencies (FS + SS with lag)
        let _ = p.add_dependency(t1, t2, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t1, t3, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t2, t4, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t3, t4, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t4, t5, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t4, t6, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t5, t7, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t6, t7, DepKind::FinishToStart, 0.0);
        let _ = p.add_dependency(t7, t8, DepKind::FinishToStart, 0.0);

        // Resource assignments (note: Omar is assigned to both t2 and t3 in parallel on days 4..9,
        // intentionally creating a 200% overallocation that `resource.level` resolves cleanly!)
        let _ = p.assign_resource(t1, r_alice, 1.0);
        let _ = p.assign_resource(t2, r_omar, 1.0);
        let _ = p.assign_resource(t3, r_omar, 1.0);
        let _ = p.assign_resource(t5, r_lina, 1.0);
        let _ = p.assign_resource(t6, r_alice, 1.0);
        let _ = p.assign_resource(t7, r_qa, 1.0);

        // Capture Baseline 0
        let _ = p.capture_baseline("Baseline 0 — Approved Charter");
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_program_builds_and_tracks_costs() {
        let p = Project::sample_launch_program();
        assert_eq!(p.tasks.len(), 8);
        assert_eq!(p.resources.len(), 4);
        assert_eq!(p.baselines.len(), 1);
        assert!(p.planned_task_cost(1) > 3000.0);
    }

    #[test]
    fn calendar_skips_weekends_and_holidays() {
        let mut cal = Calendar::default();
        cal.non_working_days.push(2); // Wednesday of week 1 is a holiday
                                      // Work day 0 = Mon(0), 1 = Tue(1), 2 = Thu(3), 3 = Fri(4), 4 = Mon(7)
        assert_eq!(cal.work_day_to_calendar_day(0), 0);
        assert_eq!(cal.work_day_to_calendar_day(1), 1);
        assert_eq!(cal.work_day_to_calendar_day(2), 3);
        assert_eq!(cal.work_day_to_calendar_day(4), 7);
    }
}
