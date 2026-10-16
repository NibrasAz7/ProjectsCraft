//! `pmsync` (L3) — Local-first collaboration, role enforcement, and change DAG.
//!
//! Implements the collaboration architecture from `vision.md`:
//! - **Role permissions (`SyncRole`):** `Owner`, `Editor`, `Commenter`, `Viewer`, `Guest`
//! - **Content-addressed change DAG (`ChangeEnvelope`):** every command mutation or
//!   comment is recorded as an immutable envelope with a deterministic 64-bit hex ID
//!   and parent pointer, doubling as the enterprise audit log
//! - **Social layer (`TaskComment`):** comments and `@mentions` synced alongside tasks
//! - **Post-merge semantic conflict advisories (`SemanticConflict`):** detects when
//!   converged concurrent edits create overallocations or negative slack and flags
//!   them in the activity feed instead of silently corrupting the plan
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeSet;
use std::fmt;

use pmcore::Project;
use pmsched::{compute_workload, schedule_project};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Maximum envelopes retained before shallow snapshot compaction.
pub const MAX_LOG_ENVELOPES: usize = 50_000;

/// Role-based access control levels enforced by `projectcraft-sync` and the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncRole {
    Guest,
    Viewer,
    Commenter,
    Editor,
    Owner,
}

impl SyncRole {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncRole::Guest => "guest",
            SyncRole::Viewer => "viewer",
            SyncRole::Commenter => "commenter",
            SyncRole::Editor => "editor",
            SyncRole::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "owner" | "admin" => SyncRole::Owner,
            "editor" | "member" | "write" => SyncRole::Editor,
            "commenter" | "comment" => SyncRole::Commenter,
            "guest" => SyncRole::Guest,
            _ => SyncRole::Viewer,
        }
    }

    pub fn can_read(self) -> bool {
        true
    }

    pub fn can_comment(self) -> bool {
        matches!(self, SyncRole::Commenter | SyncRole::Editor | SyncRole::Owner)
    }

    pub fn can_write_tasks(self) -> bool {
        matches!(self, SyncRole::Editor | SyncRole::Owner)
    }

    pub fn can_manage_roles(self) -> bool {
        matches!(self, SyncRole::Owner)
    }

    /// Check whether `self` is permitted to execute engine command `cmd_id`.
    pub fn allows_command(self, cmd_id: &str) -> bool {
        if cmd_id.starts_with("export.")
            || matches!(
                cmd_id,
                "engine.list"
                    | "project.summary"
                    | "task.list"
                    | "schedule.critical_path"
                    | "schedule.monte_carlo"
                    | "baseline.variance"
                    | "resource.workload"
                    | "evm.compute"
                    | "sprint.burndown"
                    | "sync.activity_feed"
                    | "sync.check_permission"
                    | "sync.export_bundle"
            )
        {
            return self.can_read();
        }
        if cmd_id == "sync.add_comment" {
            return self.can_comment();
        }
        self.can_write_tasks()
    }
}

/// Sync or permission error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    PermissionDenied { role: String, action: String },
    InvalidEnvelope(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::PermissionDenied { role, action } => {
                write!(f, "permission denied: role `{role}` cannot perform `{action}`")
            }
            SyncError::InvalidEnvelope(m) => write!(f, "invalid sync envelope: {m}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// A comment attached to a work item inside the synced document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskComment {
    pub id: String,
    pub task_id: u32,
    pub author: String,
    pub body: String,
    pub mentions: Vec<String>,
    pub seq: u64,
}

/// Post-merge semantic schedule conflict surfaced in the activity feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticConflict {
    pub after_change_id: String,
    pub kind: String,
    pub description: String,
}

/// Immutable change-set envelope in the append-only audit / sync DAG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeEnvelope {
    pub seq: u64,
    pub change_id: String,
    pub parent_id: Option<String>,
    pub author: String,
    pub role: SyncRole,
    pub command: String,
    pub params: Json,
}

impl ChangeEnvelope {
    pub fn new(seq: u64, parent_id: Option<String>, author: impl Into<String>, role: SyncRole, command: impl Into<String>, params: Json) -> Self {
        let author = author.into();
        let command = command.into();
        let payload = format!("{seq}:{}:{author}:{}:{command}:{params}", parent_id.as_deref().unwrap_or("root"), role.as_str());
        let change_id = fnv1a_hex(payload.as_bytes());
        ChangeEnvelope { seq, change_id, parent_id, author, role, command, params }
    }
}

/// Portable sync bundle exchanged between peers and `projectcraft-sync`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncBundle {
    pub head_change_id: Option<String>,
    pub envelopes: Vec<ChangeEnvelope>,
    pub comments: Vec<TaskComment>,
    pub conflicts: Vec<SemanticConflict>,
}

impl SyncBundle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a command execution into the append-only change DAG after verifying role permissions.
    pub fn record_command(
        &mut self,
        author: impl Into<String>,
        role: SyncRole,
        command: impl Into<String>,
        params: Json,
    ) -> Result<ChangeEnvelope, SyncError> {
        let command = command.into();
        if !role.allows_command(&command) {
            return Err(SyncError::PermissionDenied { role: role.as_str().to_string(), action: command });
        }
        let seq = (self.envelopes.len() as u64).saturating_add(1);
        let env = ChangeEnvelope::new(seq, self.head_change_id.clone(), author, role, command, params);
        self.head_change_id = Some(env.change_id.clone());
        if self.envelopes.len() >= MAX_LOG_ENVELOPES {
            self.envelopes.remove(0);
        }
        self.envelopes.push(env.clone());
        Ok(env)
    }

    /// Add a task comment, extracting `@handle` mentions automatically.
    pub fn add_comment(
        &mut self,
        author: impl Into<String>,
        role: SyncRole,
        task_id: u32,
        body: impl Into<String>,
    ) -> Result<TaskComment, SyncError> {
        if !role.can_comment() {
            return Err(SyncError::PermissionDenied { role: role.as_str().to_string(), action: "sync.add_comment".to_string() });
        }
        let author = author.into();
        let body = body.into();
        let mentions = extract_mentions(&body);
        let env = self.record_command(
            author.clone(),
            role,
            "sync.add_comment",
            serde_json::json!({"task_id": task_id, "body": body, "mentions": mentions}),
        )?;
        let comment = TaskComment { id: env.change_id, task_id, author, body, mentions, seq: env.seq };
        self.comments.push(comment.clone());
        Ok(comment)
    }

    /// Merge remote envelopes and comments idempotently by `change_id` / `id`.
    pub fn merge_remote(&mut self, remote: &SyncBundle) -> usize {
        let known: BTreeSet<String> = self.envelopes.iter().map(|e| e.change_id.clone()).collect();
        let mut added = 0usize;
        for env in &remote.envelopes {
            if !known.contains(&env.change_id) && self.envelopes.len() < MAX_LOG_ENVELOPES {
                self.head_change_id = Some(env.change_id.clone());
                self.envelopes.push(env.clone());
                added = added.saturating_add(1);
            }
        }
        let known_comments: BTreeSet<String> = self.comments.iter().map(|c| c.id.clone()).collect();
        for c in &remote.comments {
            if !known_comments.contains(&c.id) {
                self.comments.push(c.clone());
            }
        }
        added
    }

    /// Inspect `project` after a merge and record any semantic schedule conflicts
    /// (resource overallocations or tasks violating late-finish constraints).
    pub fn inspect_post_merge_conflicts(&mut self, project: &mut Project) -> Vec<SemanticConflict> {
        let _ = schedule_project(project);
        let head = self.head_change_id.clone().unwrap_or_else(|| "initial".to_string());
        let mut found = Vec::new();

        let wl = compute_workload(project);
        if wl.total_overallocated_person_days > 0 {
            found.push(SemanticConflict {
                after_change_id: head.clone(),
                kind: "resource_overallocation".to_string(),
                description: format!(
                    "Post-merge schedule has {} overallocated person-days across resources; run `resource.level` to resolve.",
                    wl.total_overallocated_person_days
                ),
            });
        }

        for t in &project.tasks {
            if t.early_finish > t.late_finish + 1e-6 {
                found.push(SemanticConflict {
                    after_change_id: head.clone(),
                    kind: "constraint_violation".to_string(),
                    description: format!(
                        "Task {} ({}) early finish ({:.1}d) exceeds late finish ({:.1}d).",
                        t.key, t.title, t.early_finish, t.late_finish
                    ),
                });
            }
        }

        self.conflicts = found.clone();
        found
    }
}

fn extract_mentions(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in body.split_whitespace() {
        if let Some(rest) = word.strip_prefix('@') {
            let clean: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            if !clean.is_empty() && !out.contains(&clean) {
                out.push(clean);
            }
        }
    }
    out
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_permissions_and_idempotent_merge() {
        let mut a = SyncBundle::new();
        assert!(a.record_command("viewer1", SyncRole::Viewer, "task.add", serde_json::json!({"title": "X"})).is_err());
        assert!(a.record_command("editor1", SyncRole::Editor, "task.add", serde_json::json!({"title": "X"})).is_ok());

        let c = a.add_comment("commenter1", SyncRole::Commenter, 1, "Looks ready @alice and @omar!").unwrap();
        assert_eq!(c.mentions, vec!["alice".to_string(), "omar".to_string()]);

        let mut b = SyncBundle::new();
        assert_eq!(b.merge_remote(&a), 2);
        // Second merge of the same bundle is a no-op (idempotent)
        assert_eq!(b.merge_remote(&a), 0);
        assert_eq!(b.comments.len(), 1);
    }
}
