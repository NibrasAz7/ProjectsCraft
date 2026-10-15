//! `pmsched` (L1) — Scheduling, Resource Levelling, EVM & Monte Carlo Risk Engine.
//!
//! Pure, deterministic algorithms operating on [`pmcore::Project`]:
//! - Critical Path Method (CPM) forward/backward pass with `FS`/`SS`/`FF`/`SF`
//!   leads/lags and task constraints (`ASAP`, `ALAP`, `SNET`, `SNLT`, `FNET`,
//!   `FNLT`, `MSO`, `MFO`)
//! - Baseline vs. actual variance analysis
//! - Resource workload heatmaps and priority-rule heuristic resource levelling
//! - ANSI/EIA-748 Earned Value Management (`PV`, `EV`, `AC`, `BAC`, `SV`, `CV`,
//!   `SPI`, `CPI`, `EAC`, `ETC`, `VAC`, `TCPI` and cumulative S-curves)
//! - Deterministic Monte Carlo schedule-risk simulation (`P10`/`P50`/`P80`/`P90`
//!   and per-task criticality index)
//! - Agile sprint velocity and burndown metrics
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeMap;
use std::fmt;

use pmcore::{DepKind, Project, TaskConstraint, TaskStatus};
use serde::{Deserialize, Serialize};

/// Cap on Monte Carlo trials per invocation so untrusted requests stay fast.
pub const MAX_MONTE_CARLO_TRIALS: usize = 20_000;
/// Cap on horizon days for daily resource grids and EVM curves.
pub const MAX_HORIZON_DAYS: usize = 3_650;

/// Scheduling error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedError {
    CycleDetected(Vec<u32>),
    InvalidInput(String),
}

impl fmt::Display for SchedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedError::CycleDetected(ids) => write!(f, "dependency cycle detected involving task IDs: {ids:?}"),
            SchedError::InvalidInput(m) => write!(f, "invalid scheduling input: {m}"),
        }
    }
}

impl std::error::Error for SchedError {}

/// Summary of a CPM scheduling pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleSummary {
    pub project_duration_days: f64,
    pub critical_task_ids: Vec<u32>,
    pub topological_order: Vec<u32>,
    pub total_planned_cost: f64,
}

/// Topologically sort task indices (`0..project.tasks.len()`), returning
/// [`SchedError::CycleDetected`] if any dependency cycle exists.
pub fn topological_sort(project: &Project) -> Result<Vec<usize>, SchedError> {
    let n = project.tasks.len();
    let mut id_to_idx = BTreeMap::new();
    for (idx, t) in project.tasks.iter().enumerate() {
        id_to_idx.insert(t.id, idx);
    }

    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for dep in &project.dependencies {
        if let (Some(&u), Some(&v)) = (id_to_idx.get(&dep.predecessor_id), id_to_idx.get(&dep.successor_id)) {
            if let Some(list) = adj.get_mut(u) {
                list.push(v);
            }
            if let Some(deg) = in_degree.get_mut(v) {
                *deg = deg.saturating_add(1);
            }
        }
    }

    let mut queue: Vec<usize> = in_degree.iter().enumerate().filter_map(|(i, &d)| (d == 0).then_some(i)).collect();
    let mut order = Vec::with_capacity(n);
    let mut head = 0;
    while let Some(&u) = queue.get(head) {
        head += 1;
        order.push(u);
        if let Some(neighbors) = adj.get(u) {
            for &v in neighbors {
                if let Some(deg) = in_degree.get_mut(v) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push(v);
                    }
                }
            }
        }
    }

    if order.len() != n {
        let cycle_ids: Vec<u32> =
            project.tasks.iter().enumerate().filter_map(|(i, t)| (in_degree.get(i).copied().unwrap_or(0) > 0).then_some(t.id)).collect();
        return Err(SchedError::CycleDetected(cycle_ids));
    }
    Ok(order)
}

/// Run the CPM forward and backward pass on `project`, updating every task's
/// `early_start`, `early_finish`, `late_start`, `late_finish`, `total_slack`,
/// `free_slack`, and `critical` flag in place.
pub fn schedule_project(project: &mut Project) -> Result<ScheduleSummary, SchedError> {
    let order = topological_sort(project)?;
    let n = project.tasks.len();
    let mut id_to_idx = BTreeMap::new();
    for (idx, t) in project.tasks.iter().enumerate() {
        id_to_idx.insert(t.id, idx);
    }

    let mut es = vec![0.0_f64; n];
    let mut ef = vec![0.0_f64; n];
    let mut dur = vec![0.0_f64; n];
    for (i, t) in project.tasks.iter().enumerate() {
        if let Some(d) = dur.get_mut(i) {
            *d = t.duration_days.max(0.0);
        }
        if let Some(s) = es.get_mut(i) {
            *s = t.levelled_delay.max(0.0);
        }
    }

    // Forward pass
    for &idx in &order {
        let task_id = project.tasks.get(idx).map(|t| t.id).unwrap_or(0);
        let d = dur.get(idx).copied().unwrap_or(0.0);
        let mut start = es.get(idx).copied().unwrap_or(0.0);

        for dep in project.dependencies.iter().filter(|dep| dep.successor_id == task_id) {
            if let Some(&pred_idx) = id_to_idx.get(&dep.predecessor_id) {
                let p_es = es.get(pred_idx).copied().unwrap_or(0.0);
                let p_ef = ef.get(pred_idx).copied().unwrap_or(0.0);
                let candidate = match dep.kind {
                    DepKind::FinishToStart => p_ef + dep.lag_days,
                    DepKind::StartToStart => p_es + dep.lag_days,
                    DepKind::FinishToFinish => p_ef + dep.lag_days - d,
                    DepKind::StartToFinish => p_es + dep.lag_days - d,
                };
                if candidate > start {
                    start = candidate;
                }
            }
        }

        if let Some(t) = project.tasks.get(idx) {
            match t.constraint {
                TaskConstraint::Snet(c) => start = start.max(c),
                TaskConstraint::Fnet(c) => start = start.max(c - d),
                TaskConstraint::Mso(c) => start = c.max(0.0),
                TaskConstraint::Mfo(c) => start = (c - d).max(0.0),
                TaskConstraint::Asap | TaskConstraint::Alap | TaskConstraint::Snlt(_) | TaskConstraint::Fnlt(_) => {}
            }
        }
        start = start.max(0.0);
        if let Some(s) = es.get_mut(idx) {
            *s = start;
        }
        if let Some(f) = ef.get_mut(idx) {
            *f = start + d;
        }
    }

    let project_duration = ef.iter().copied().fold(0.0_f64, f64::max);

    // Backward pass
    let mut ls = vec![project_duration; n];
    let mut lf = vec![project_duration; n];
    for &idx in order.iter().rev() {
        let task_id = project.tasks.get(idx).map(|t| t.id).unwrap_or(0);
        let d = dur.get(idx).copied().unwrap_or(0.0);
        let mut finish = project_duration;

        for dep in project.dependencies.iter().filter(|dep| dep.predecessor_id == task_id) {
            if let Some(&succ_idx) = id_to_idx.get(&dep.successor_id) {
                let s_ls = ls.get(succ_idx).copied().unwrap_or(project_duration);
                let s_lf = lf.get(succ_idx).copied().unwrap_or(project_duration);
                let candidate = match dep.kind {
                    DepKind::FinishToStart => s_ls - dep.lag_days,
                    DepKind::StartToStart => s_ls - dep.lag_days + d,
                    DepKind::FinishToFinish => s_lf - dep.lag_days,
                    DepKind::StartToFinish => s_lf - dep.lag_days + d,
                };
                if candidate < finish {
                    finish = candidate;
                }
            }
        }

        if let Some(t) = project.tasks.get(idx) {
            match t.constraint {
                TaskConstraint::Fnlt(c) | TaskConstraint::Mfo(c) => finish = finish.min(c),
                TaskConstraint::Snlt(c) | TaskConstraint::Mso(c) => finish = finish.min(c + d),
                TaskConstraint::Asap | TaskConstraint::Alap | TaskConstraint::Snet(_) | TaskConstraint::Fnet(_) => {}
            }
        }
        if let Some(f) = lf.get_mut(idx) {
            *f = finish;
        }
        if let Some(s) = ls.get_mut(idx) {
            *s = finish - d;
        }

        // If ALAP constraint, shift early dates to late dates when slack > 0
        if let Some(t) = project.tasks.get(idx) {
            if matches!(t.constraint, TaskConstraint::Alap) {
                let late_s = (finish - d).max(es.get(idx).copied().unwrap_or(0.0));
                if let Some(s) = es.get_mut(idx) {
                    *s = late_s;
                }
                if let Some(f) = ef.get_mut(idx) {
                    *f = late_s + d;
                }
            }
        }
    }

    // Compute free slack, total slack, and update task structs
    let mut critical_task_ids = Vec::new();
    for idx in 0..n {
        let task_id = project.tasks.get(idx).map(|t| t.id).unwrap_or(0);
        let my_es = es.get(idx).copied().unwrap_or(0.0);
        let my_ef = ef.get(idx).copied().unwrap_or(0.0);
        let my_ls = ls.get(idx).copied().unwrap_or(0.0);
        let my_lf = lf.get(idx).copied().unwrap_or(0.0);
        let total_slack = (my_ls - my_es).max(0.0);

        let mut min_succ_gap = f64::INFINITY;
        for dep in project.dependencies.iter().filter(|dep| dep.predecessor_id == task_id) {
            if let Some(&succ_idx) = id_to_idx.get(&dep.successor_id) {
                let s_es = es.get(succ_idx).copied().unwrap_or(0.0);
                let s_ef = ef.get(succ_idx).copied().unwrap_or(0.0);
                let gap = match dep.kind {
                    DepKind::FinishToStart => s_es - dep.lag_days - my_ef,
                    DepKind::StartToStart => s_es - dep.lag_days - my_es,
                    DepKind::FinishToFinish => s_ef - dep.lag_days - my_ef,
                    DepKind::StartToFinish => s_ef - dep.lag_days - my_es,
                };
                if gap < min_succ_gap {
                    min_succ_gap = gap;
                }
            }
        }
        let free_slack = if min_succ_gap.is_finite() { min_succ_gap.max(0.0) } else { total_slack };
        let is_critical = total_slack <= 1e-6;
        if is_critical {
            critical_task_ids.push(task_id);
        }

        if let Some(task) = project.tasks.get_mut(idx) {
            task.early_start = my_es;
            task.early_finish = my_ef;
            task.late_start = my_ls;
            task.late_finish = my_lf;
            task.total_slack = total_slack;
            task.free_slack = free_slack;
            task.critical = is_critical;
        }
    }

    let topo_ids: Vec<u32> = order.iter().filter_map(|&i| project.tasks.get(i).map(|t| t.id)).collect();
    let total_planned_cost: f64 = project.tasks.iter().map(|t| project.planned_task_cost(t.id)).sum();

    Ok(ScheduleSummary { project_duration_days: project_duration, critical_task_ids, topological_order: topo_ids, total_planned_cost })
}

/// Per-task variance against a captured baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskVariance {
    pub task_id: u32,
    pub key: String,
    pub title: String,
    pub baseline_start: f64,
    pub current_start: f64,
    pub start_variance_days: f64,
    pub baseline_finish: f64,
    pub current_finish: f64,
    pub finish_variance_days: f64,
    pub baseline_cost: f64,
    pub current_planned_cost: f64,
    pub cost_variance: f64,
}

/// Compare current scheduled dates & costs against a baseline (`baseline_id = None` uses latest).
pub fn compute_baseline_variance(project: &Project, baseline_id: Option<u32>) -> Vec<TaskVariance> {
    let baseline = match baseline_id {
        Some(id) => project.baselines.iter().find(|b| b.id == id),
        None => project.baselines.last(),
    };
    let Some(base) = baseline else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for t in &project.tasks {
        if let Some(bt) = base.tasks.iter().find(|bt| bt.task_id == t.id) {
            let current_cost = project.planned_task_cost(t.id);
            out.push(TaskVariance {
                task_id: t.id,
                key: t.key.clone(),
                title: t.title.clone(),
                baseline_start: bt.start_day,
                current_start: t.early_start,
                start_variance_days: t.early_start - bt.start_day,
                baseline_finish: bt.finish_day,
                current_finish: t.early_finish,
                finish_variance_days: t.early_finish - bt.finish_day,
                baseline_cost: bt.planned_cost,
                current_planned_cost: current_cost,
                cost_variance: current_cost - bt.planned_cost,
            });
        }
    }
    out
}

/// One resource's daily load row in the workload heatmap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceWorkload {
    pub resource_id: u32,
    pub name: String,
    pub role: String,
    pub max_units: f64,
    pub daily_units: Vec<f64>,
    pub overallocated_days: Vec<usize>,
    pub peak_units: f64,
}

/// Overall workload report across the project horizon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadReport {
    pub horizon_days: usize,
    pub resources: Vec<ResourceWorkload>,
    pub total_overallocated_person_days: usize,
}

/// Compute time-phased daily resource allocations and detect overallocations.
pub fn compute_workload(project: &Project) -> WorkloadReport {
    let max_ef = project.tasks.iter().map(|t| t.early_finish.ceil() as usize).max().unwrap_or(0);
    let horizon = max_ef.clamp(1, MAX_HORIZON_DAYS);
    let mut rows = Vec::with_capacity(project.resources.len());
    let mut total_over = 0usize;

    for res in &project.resources {
        let mut daily = vec![0.0_f64; horizon];
        for a in project.assignments.iter().filter(|a| a.resource_id == res.id) {
            if let Some(task) = project.task(a.task_id) {
                if task.duration_days <= 0.0 {
                    continue;
                }
                let start_day = (task.early_start.floor().max(0.0) as usize).min(horizon);
                let end_day = (task.early_finish.ceil().max(0.0) as usize).min(horizon);
                for slot in daily.get_mut(start_day..end_day).unwrap_or(&mut []) {
                    *slot += a.units;
                }
            }
        }
        let mut over_days = Vec::new();
        let mut peak = 0.0_f64;
        for (d, &u) in daily.iter().enumerate() {
            if u > peak {
                peak = u;
            }
            if u > res.max_units + 1e-6 {
                over_days.push(d);
            }
        }
        total_over = total_over.saturating_add(over_days.len());
        rows.push(ResourceWorkload {
            resource_id: res.id,
            name: res.name.clone(),
            role: res.role.clone(),
            max_units: res.max_units,
            daily_units: daily,
            overallocated_days: over_days,
            peak_units: peak,
        });
    }

    WorkloadReport { horizon_days: horizon, resources: rows, total_overallocated_person_days: total_over }
}

/// User-visible explanation of a single levelling adjustment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelAction {
    pub task_id: u32,
    pub task_key: String,
    pub resource_id: u32,
    pub resource_name: String,
    pub conflict_day: usize,
    pub added_delay_days: f64,
    pub new_early_start: f64,
    pub reason: String,
}

/// Result of priority-rule resource levelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelResult {
    pub actions: Vec<LevelAction>,
    pub before_overallocated_days: usize,
    pub after_overallocated_days: usize,
    pub new_project_duration_days: f64,
}

/// Resolve resource overallocations using a priority-rule serial heuristic:
/// higher `Priority::weight()`, smaller `total_slack`, and earlier ID win the
/// contested resource; lower-priority conflicting tasks are delayed past the
/// conflict window with an explicit [`LevelAction`] explanation.
pub fn level_resources(project: &mut Project) -> Result<LevelResult, SchedError> {
    let _ = schedule_project(project)?;
    let before_over = compute_workload(project).total_overallocated_days();
    let mut actions = Vec::new();

    // Bound iterations so pathological schedules always terminate cleanly.
    let max_passes = project.tasks.len().saturating_mul(8).clamp(8, 256);
    for _ in 0..max_passes {
        let report = compute_workload(project);
        let Some((res_row, conflict_day)) = report.resources.iter().find_map(|r| r.overallocated_days.first().copied().map(|d| (r, d))) else {
            break;
        };

        let res_id = res_row.resource_id;
        let res_name = res_row.name.clone();
        let day_f = conflict_day as f64;

        // Collect all tasks using `res_id` during `conflict_day`
        let mut active: Vec<(u32, String, u8, f64, f64, f64)> = project
            .assignments
            .iter()
            .filter(|a| a.resource_id == res_id && a.units > 0.0)
            .filter_map(|a| {
                let t = project.task(a.task_id)?;
                if t.duration_days > 0.0 && t.early_start <= day_f && t.early_finish > day_f {
                    Some((t.id, t.key.clone(), t.priority.weight(), t.total_slack, t.early_start, t.early_finish))
                } else {
                    None
                }
            })
            .collect();

        if active.len() < 2 {
            break;
        }

        // Sort so index 0 is highest priority (kept in place) and last is lowest priority (shifted)
        active.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)).then_with(|| a.0.cmp(&b.0)));

        let Some(winner) = active.first().cloned() else { break };
        let Some(loser) = active.last().cloned() else { break };
        if winner.0 == loser.0 {
            break;
        }

        let shift = (winner.5 - loser.4).max(1.0);
        if let Some(t) = project.task_mut(loser.0) {
            t.levelled_delay += shift;
        }
        let _ = schedule_project(project)?;
        let new_es = project.task(loser.0).map(|t| t.early_start).unwrap_or(loser.4 + shift);
        actions.push(LevelAction {
            task_id: loser.0,
            task_key: loser.1,
            resource_id: res_id,
            resource_name: res_name.clone(),
            conflict_day,
            added_delay_days: shift,
            new_early_start: new_es,
            reason: format!(
                "Delayed by {shift:.1}d to resolve {res_name} overallocation on day {conflict_day} in favor of higher-priority {}",
                winner.1
            ),
        });
    }

    let after_over = compute_workload(project).total_overallocated_days();
    let new_dur = project.tasks.iter().map(|t| t.early_finish).fold(0.0_f64, f64::max);
    Ok(LevelResult { actions, before_overallocated_days: before_over, after_overallocated_days: after_over, new_project_duration_days: new_dur })
}

impl WorkloadReport {
    pub fn total_overallocated_days(&self) -> usize {
        self.total_overallocated_person_days
    }
}

/// Time-phased cumulative EVM curve point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvmCurvePoint {
    pub day: f64,
    pub pv: f64,
    pub ev: f64,
    pub ac: f64,
}

/// Complete ANSI/EIA-748 Earned Value Management metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvmMetrics {
    pub status_day: f64,
    /// Budget at Completion (`BAC`).
    pub bac: f64,
    /// Planned Value (`PV` / `BCWS`) at `status_day`.
    pub pv: f64,
    /// Earned Value (`EV` / `BCWP`) at `status_day`.
    pub ev: f64,
    /// Actual Cost (`AC` / `ACWP`) at `status_day`.
    pub ac: f64,
    /// Schedule Variance (`SV = EV - PV`).
    pub sv: f64,
    /// Cost Variance (`CV = EV - AC`).
    pub cv: f64,
    /// Schedule Performance Index (`SPI = EV / PV`).
    pub spi: f64,
    /// Cost Performance Index (`CPI = EV / AC`).
    pub cpi: f64,
    /// Estimate at Completion (`EAC = BAC / CPI`).
    pub eac: f64,
    /// Estimate to Complete (`ETC = max(0, EAC - AC)`).
    pub etc: f64,
    /// Variance at Completion (`VAC = BAC - EAC`).
    pub vac: f64,
    /// To-Complete Performance Index (`TCPI = (BAC - EV) / (BAC - AC)`).
    pub tcpi: f64,
    pub curve: Vec<EvmCurvePoint>,
}

/// Compute Earned Value Management metrics and cumulative `PV`/`EV`/`AC` cost curves.
pub fn compute_evm(project: &Project, status_day_override: Option<f64>) -> EvmMetrics {
    let status_day = status_day_override.unwrap_or(project.status_day).max(0.0);
    let baseline = project.baselines.first();

    let mut bac = 0.0_f64;
    let mut pv = 0.0_f64;
    let mut ev = 0.0_f64;
    let mut ac = 0.0_f64;

    let mut task_budgets: Vec<(f64, f64, f64, f64, f64)> = Vec::with_capacity(project.tasks.len());
    for t in &project.tasks {
        let (start, finish, dur, budget) = match baseline.and_then(|b| b.tasks.iter().find(|bt| bt.task_id == t.id)) {
            Some(bt) => (bt.start_day, bt.finish_day, bt.duration_days, bt.planned_cost),
            None => (t.early_start, t.early_finish, t.duration_days, project.planned_task_cost(t.id)),
        };
        let planned_pct = planned_fraction_at(start, finish, dur, status_day);
        bac += budget;
        pv += budget * planned_pct;
        ev += budget * t.progress.clamp(0.0, 1.0);
        ac += t.actual_cost.max(0.0);
        task_budgets.push((start, finish, dur, budget, t.progress.clamp(0.0, 1.0)));
    }

    let sv = ev - pv;
    let cv = ev - ac;
    let spi = if pv > 1e-9 { ev / pv } else { 1.0 };
    let cpi = if ac > 1e-9 { ev / ac } else { 1.0 };
    let eac = if cpi > 1e-9 { bac / cpi } else { bac + (ac - ev) };
    let etc = (eac - ac).max(0.0);
    let vac = bac - eac;
    let tcpi = if (bac - ac).abs() > 1e-9 { (bac - ev) / (bac - ac) } else { 1.0 };

    let horizon =
        project.tasks.iter().map(|t| t.early_finish.ceil() as usize).max().unwrap_or(1).max(status_day.ceil() as usize).clamp(1, MAX_HORIZON_DAYS);

    let mut curve = Vec::with_capacity(horizon + 1);
    for d in 0..=horizon {
        let day = d as f64;
        let day_pv: f64 = task_budgets.iter().map(|&(s, f, dur, b, _)| b * planned_fraction_at(s, f, dur, day)).sum();
        let ratio = if status_day > 1e-9 { (day / status_day).clamp(0.0, 1.0) } else { 1.0 };
        let day_ev = if day <= status_day { ev * ratio } else { ev };
        let day_ac = if day <= status_day { ac * ratio } else { ac };
        curve.push(EvmCurvePoint { day, pv: day_pv, ev: day_ev, ac: day_ac });
    }

    EvmMetrics { status_day, bac, pv, ev, ac, sv, cv, spi, cpi, eac, etc, vac, tcpi, curve }
}

fn planned_fraction_at(start: f64, finish: f64, dur: f64, day: f64) -> f64 {
    if day <= start {
        0.0
    } else if day >= finish {
        1.0
    } else if dur > 1e-9 {
        ((day - start) / dur).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Per-task criticality index from Monte Carlo simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCriticality {
    pub task_id: u32,
    pub key: String,
    pub title: String,
    /// Fraction of trials (`0.0..=1.0`) in which this task lay on the critical path.
    pub criticality_index: f64,
    pub mean_duration_days: f64,
}

/// Histogram bin of simulated project durations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskBin {
    pub low_days: f64,
    pub high_days: f64,
    pub count: usize,
}

/// Complete Monte Carlo schedule-risk output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloReport {
    pub trials: usize,
    pub seed: u64,
    pub deterministic_duration_days: f64,
    pub mean_duration_days: f64,
    pub std_dev_days: f64,
    pub min_duration_days: f64,
    pub p10_days: f64,
    pub p50_days: f64,
    pub p80_days: f64,
    pub p90_days: f64,
    pub max_duration_days: f64,
    pub histogram: Vec<RiskBin>,
    pub criticality: Vec<TaskCriticality>,
}

/// Deterministic `xorshift64*` PRNG so simulations are 100% reproducible across platforms and WASM.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        XorShift64(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform float in `[0.0, 1.0)`.
    fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    /// Sample from a triangular distribution `(a, c, b)` with `a <= c <= b`.
    fn sample_triangular(&mut self, a: f64, c: f64, b: f64) -> f64 {
        if (b - a).abs() <= 1e-9 {
            return a;
        }
        let u = self.next_f64();
        let fc = ((c - a) / (b - a)).clamp(0.0, 1.0);
        if u < fc {
            a + (u * (b - a) * (c - a)).sqrt()
        } else {
            b - ((1.0 - u) * (b - a) * (b - c)).sqrt()
        }
    }
}

/// Run a Monte Carlo schedule-risk simulation over `trials` iterations.
pub fn run_monte_carlo(project: &Project, trials: usize, seed: u64) -> Result<MonteCarloReport, SchedError> {
    let trials = trials.clamp(10, MAX_MONTE_CARLO_TRIALS);
    let order = topological_sort(project)?;
    let n = project.tasks.len();
    if n == 0 {
        return Err(SchedError::InvalidInput("cannot run Monte Carlo on an empty project".to_string()));
    }

    let mut id_to_idx = BTreeMap::new();
    for (idx, t) in project.tasks.iter().enumerate() {
        id_to_idx.insert(t.id, idx);
    }

    let mut rng = XorShift64::new(seed);
    let mut durations_out = Vec::with_capacity(trials);
    let mut crit_counts = vec![0usize; n];
    let mut sum_task_dur = vec![0.0_f64; n];

    let mut sample_dur = vec![0.0_f64; n];
    let mut es = vec![0.0_f64; n];
    let mut ef = vec![0.0_f64; n];
    let mut ls = vec![0.0_f64; n];
    let mut lf = vec![0.0_f64; n];

    for _ in 0..trials {
        for (i, t) in project.tasks.iter().enumerate() {
            let d = if t.duration_days <= 0.0 {
                0.0
            } else {
                let (o, m, p) = match (t.optimistic_days, t.most_likely_days, t.pessimistic_days) {
                    (Some(o), Some(m), Some(p)) if o >= 0.0 && m >= o && p >= m => (o, m, p),
                    _ => (t.duration_days * 0.85, t.duration_days, t.duration_days * 1.35),
                };
                rng.sample_triangular(o, m, p)
            };
            if let Some(sd) = sample_dur.get_mut(i) {
                *sd = d;
            }
            if let Some(st) = sum_task_dur.get_mut(i) {
                *st += d;
            }
            if let Some(s) = es.get_mut(i) {
                *s = t.levelled_delay.max(0.0);
            }
        }

        for &idx in &order {
            let task_id = project.tasks.get(idx).map(|t| t.id).unwrap_or(0);
            let d = sample_dur.get(idx).copied().unwrap_or(0.0);
            let mut start = es.get(idx).copied().unwrap_or(0.0);
            for dep in project.dependencies.iter().filter(|dep| dep.successor_id == task_id) {
                if let Some(&pred_idx) = id_to_idx.get(&dep.predecessor_id) {
                    let p_es = es.get(pred_idx).copied().unwrap_or(0.0);
                    let p_ef = ef.get(pred_idx).copied().unwrap_or(0.0);
                    let cand = match dep.kind {
                        DepKind::FinishToStart => p_ef + dep.lag_days,
                        DepKind::StartToStart => p_es + dep.lag_days,
                        DepKind::FinishToFinish => p_ef + dep.lag_days - d,
                        DepKind::StartToFinish => p_es + dep.lag_days - d,
                    };
                    if cand > start {
                        start = cand;
                    }
                }
            }
            start = start.max(0.0);
            if let Some(s) = es.get_mut(idx) {
                *s = start;
            }
            if let Some(f) = ef.get_mut(idx) {
                *f = start + d;
            }
        }

        let proj_dur = ef.iter().copied().fold(0.0_f64, f64::max);
        durations_out.push(proj_dur);

        for &idx in order.iter().rev() {
            let task_id = project.tasks.get(idx).map(|t| t.id).unwrap_or(0);
            let d = sample_dur.get(idx).copied().unwrap_or(0.0);
            let mut finish = proj_dur;
            for dep in project.dependencies.iter().filter(|dep| dep.predecessor_id == task_id) {
                if let Some(&succ_idx) = id_to_idx.get(&dep.successor_id) {
                    let s_ls = ls.get(succ_idx).copied().unwrap_or(proj_dur);
                    let s_lf = lf.get(succ_idx).copied().unwrap_or(proj_dur);
                    let cand = match dep.kind {
                        DepKind::FinishToStart => s_ls - dep.lag_days,
                        DepKind::StartToStart => s_ls - dep.lag_days + d,
                        DepKind::FinishToFinish => s_lf - dep.lag_days,
                        DepKind::StartToFinish => s_lf - dep.lag_days + d,
                    };
                    if cand < finish {
                        finish = cand;
                    }
                }
            }
            if let Some(f) = lf.get_mut(idx) {
                *f = finish;
            }
            if let Some(s) = ls.get_mut(idx) {
                *s = finish - d;
            }
            let my_es = es.get(idx).copied().unwrap_or(0.0);
            if (finish - d - my_es).abs() <= 1e-5 {
                if let Some(c) = crit_counts.get_mut(idx) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    durations_out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = durations_out.iter().sum::<f64>() / (trials as f64);
    let variance = durations_out.iter().map(|&x| (x - mean) * (x - mean)).sum::<f64>() / (trials as f64);
    let std_dev = variance.sqrt();

    let percentile = |p: f64| -> f64 {
        let idx = ((trials as f64 - 1.0) * p).round() as usize;
        durations_out.get(idx).copied().unwrap_or(mean)
    };

    let min_d = durations_out.first().copied().unwrap_or(mean);
    let max_d = durations_out.last().copied().unwrap_or(mean);
    let bin_count = 10usize;
    let span = (max_d - min_d).max(1e-6);
    let mut histogram = Vec::with_capacity(bin_count);
    for b in 0..bin_count {
        let low = min_d + span * (b as f64 / bin_count as f64);
        let high = min_d + span * ((b + 1) as f64 / bin_count as f64);
        let count = durations_out.iter().filter(|&&v| if b + 1 == bin_count { v >= low && v <= high + 1e-9 } else { v >= low && v < high }).count();
        histogram.push(RiskBin { low_days: low, high_days: high, count });
    }

    let criticality: Vec<TaskCriticality> = project
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| TaskCriticality {
            task_id: t.id,
            key: t.key.clone(),
            title: t.title.clone(),
            criticality_index: crit_counts.get(i).copied().unwrap_or(0) as f64 / (trials as f64),
            mean_duration_days: sum_task_dur.get(i).copied().unwrap_or(0.0) / (trials as f64),
        })
        .collect();

    let deterministic = project.tasks.iter().map(|t| t.early_finish).fold(0.0_f64, f64::max);

    Ok(MonteCarloReport {
        trials,
        seed,
        deterministic_duration_days: deterministic,
        mean_duration_days: mean,
        std_dev_days: std_dev,
        min_duration_days: min_d,
        p10_days: percentile(0.10),
        p50_days: percentile(0.50),
        p80_days: percentile(0.80),
        p90_days: percentile(0.90),
        max_duration_days: max_d,
        histogram,
        criticality,
    })
}

/// Agile sprint burndown summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprintBurndown {
    pub sprint_id: u32,
    pub sprint_name: String,
    pub capacity_points: f64,
    pub committed_points: f64,
    pub completed_points: f64,
    pub remaining_points: f64,
    pub task_counts_by_status: BTreeMap<String, usize>,
}

/// Compute agile burndown and status breakdown for a sprint (`None` = first sprint).
pub fn compute_sprint_burndown(project: &Project, sprint_id: Option<u32>) -> Option<SprintBurndown> {
    let sprint = match sprint_id {
        Some(id) => project.sprints.iter().find(|s| s.id == id)?,
        None => project.sprints.first()?,
    };
    let mut committed = 0.0_f64;
    let mut completed = 0.0_f64;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for st in TaskStatus::ALL {
        counts.insert(st.as_str().to_string(), 0usize);
    }
    for t in project.tasks.iter().filter(|t| t.sprint_id == Some(sprint.id)) {
        let pts = t.story_points.unwrap_or(t.duration_days.max(1.0));
        committed += pts;
        completed += if t.status == TaskStatus::Done { pts } else { pts * t.progress.clamp(0.0, 1.0) };
        if let Some(c) = counts.get_mut(t.status.as_str()) {
            *c = c.saturating_add(1);
        }
    }
    Some(SprintBurndown {
        sprint_id: sprint.id,
        sprint_name: sprint.name.clone(),
        capacity_points: sprint.capacity_points,
        committed_points: committed,
        completed_points: completed,
        remaining_points: (committed - completed).max(0.0),
        task_counts_by_status: counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmcore::Project;

    #[test]
    fn cpm_computes_critical_path_and_slack() {
        let mut p = Project::sample_launch_program();
        let summary = schedule_project(&mut p).unwrap();
        assert!((summary.project_duration_days - 20.0).abs() < 1e-6);
        // Task 2 (6d) is critical; parallel Task 3 (5d) has 1.0d of total & free slack
        let t2 = p.task(2).unwrap();
        let t3 = p.task(3).unwrap();
        assert!(t2.critical);
        assert!(!t3.critical);
        assert!((t3.total_slack - 1.0).abs() < 1e-6);
        assert!((t3.free_slack - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cycle_detection_rejects_circular_dependencies() {
        let mut p = Project::new("Cycle Test", "CYC");
        let a = p.add_task("A", 2.0, None).unwrap();
        let b = p.add_task("B", 3.0, None).unwrap();
        p.add_dependency(a, b, DepKind::FinishToStart, 0.0).unwrap();
        p.add_dependency(b, a, DepKind::FinishToStart, 0.0).unwrap();
        let err = schedule_project(&mut p).unwrap_err();
        assert!(matches!(err, SchedError::CycleDetected(_)));
    }

    #[test]
    fn resource_levelling_eliminates_overallocation() {
        let mut p = Project::sample_launch_program();
        schedule_project(&mut p).unwrap();
        let res = level_resources(&mut p).unwrap();
        assert!(res.before_overallocated_days > 0);
        assert_eq!(res.after_overallocated_days, 0);
        assert!(!res.actions.is_empty());
        assert!(res.new_project_duration_days >= 20.0);
    }

    #[test]
    fn evm_and_monte_carlo_produce_valid_metrics() {
        let mut p = Project::sample_launch_program();
        schedule_project(&mut p).unwrap();
        let evm = compute_evm(&p, Some(10.0));
        assert!(evm.bac > 0.0);
        assert!(evm.pv > 0.0);
        assert!(evm.ev > 0.0);
        assert!(evm.ac > 0.0);
        assert!(evm.spi > 0.0);
        assert!(evm.cpi > 0.0);

        let mc = run_monte_carlo(&p, 500, 42).unwrap();
        assert_eq!(mc.trials, 500);
        assert!(mc.p10_days <= mc.p50_days);
        assert!(mc.p50_days <= mc.p80_days);
        assert!(mc.p80_days <= mc.p90_days);
    }
}
