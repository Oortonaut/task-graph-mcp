//! Core types for the Task Graph MCP Server.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

// Skip-if helpers (serde requires function paths, not closures)
fn is_zero<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_default_priority(p: &Priority) -> bool {
    *p == PRIORITY_DEFAULT
}

/// Metrics array - serializes with trailing zeros trimmed, deserializes back to [i64; 8]
mod metrics_serde {
    use super::*;

    pub fn serialize<S: Serializer>(metrics: &[i64; 8], s: S) -> Result<S::Ok, S::Error> {
        // Find last non-zero index
        let len = metrics
            .iter()
            .rposition(|&x| x != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        s.collect_seq(&metrics[..len])
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[i64; 8], D::Error> {
        let v: Vec<i64> = Vec::deserialize(d)?;
        let mut arr = [0i64; 8];
        for (i, val) in v.into_iter().take(8).enumerate() {
            arr[i] = val;
        }
        Ok(arr)
    }

    pub fn is_empty(metrics: &[i64; 8]) -> bool {
        metrics.iter().all(|&x| x == 0)
    }
}

/// Worker (session-based) - represents a connected worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub max_claims: i32,
    pub registered_at: i64,
    pub last_heartbeat: i64,
    /// Last status the worker transitioned to (for prompts/dashboard)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Last phase the worker transitioned to (for prompts/dashboard)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_phase: Option<String>,
    /// Named workflow this worker is using (e.g., "swarm" for workflow-swarm.yaml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
}

/// Worker info with additional runtime details for list_workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub max_claims: i32,
    #[serde(skip_serializing_if = "is_zero")]
    pub claim_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_thought: Option<String>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
    /// Last status the worker transitioned to (for prompts/dashboard)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Last phase the worker transitioned to (for prompts/dashboard)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_phase: Option<String>,
    /// Named workflow this worker is using (e.g., "swarm" for workflow-swarm.yaml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
}

/// Task priority as an integer (higher = more important).
/// Range: 0-10, where 10 is highest priority. Default is 5.
pub type Priority = i32;

/// Default priority (middle of 0-10 range).
pub const PRIORITY_DEFAULT: Priority = 5;

/// Parse a priority value, clamping to 0-10 range.
pub fn parse_priority(s: &str) -> Priority {
    s.parse().unwrap_or(PRIORITY_DEFAULT).clamp(0, 10)
}

/// Clamp priority to valid range.
pub fn clamp_priority(p: Priority) -> Priority {
    p.clamp(0, 10)
}

/// A task in the task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "is_default_priority")]
    pub priority: Priority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<i64>,

    // Affinity (tag-based claiming requirements)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needed_tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wanted_tags: Vec<String>,

    // Categorization/discovery tags
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    // Estimation & tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_estimate_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_actual_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    // Live status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_thought: Option<String>,

    // Cost accounting
    #[serde(skip_serializing_if = "is_zero")]
    pub cost_usd: f64,
    /// Fixed array of 8 integer metrics [metric_0..metric_7], aggregated on update
    #[serde(
        with = "metrics_serde",
        skip_serializing_if = "metrics_serde::is_empty",
        default
    )]
    pub metrics: [i64; 8],

    pub created_at: i64,
    pub updated_at: i64,
}

/// A task with its children for tree operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTree {
    #[serde(flatten)]
    pub task: Task,
    pub children: Vec<TaskTree>,
}

/// Input for creating a task tree.
/// Supports all fields from task creation, plus tree-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTreeInput {
    /// Reference to an existing task ID to include in the tree.
    /// If set, this node references an existing task rather than creating a new one.
    /// Other fields are ignored when ref is set.
    #[serde(rename = "ref")]
    pub ref_id: Option<String>,

    /// Custom task ID (optional, petname ID generated if not provided).
    /// Ignored if ref is set.
    pub id: Option<String>,

    /// Task title (optional; derived from description if omitted).
    pub title: Option<String>,

    /// Task description.
    pub description: Option<String>,

    /// Task phase (type of work: explore, design, implement, etc.).
    pub phase: Option<String>,

    /// Task priority.
    pub priority: Option<Priority>,

    /// Story points / complexity estimate.
    pub points: Option<i32>,

    /// Estimated duration in milliseconds.
    pub time_estimate_ms: Option<i64>,

    /// Tags that claiming agent must have ALL of (AND logic).
    pub needed_tags: Option<Vec<String>>,

    /// Tags that claiming agent must have AT LEAST ONE of (OR logic).
    pub wanted_tags: Option<Vec<String>>,

    /// Categorization/discovery tags for the task.
    pub tags: Option<Vec<String>>,

    /// Child nodes in the tree.
    #[serde(default)]
    pub children: Vec<TaskTreeInput>,
}

/// A typed dependency between tasks.
/// The dependency indicates that from_task_id affects to_task_id based on dep_type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub from_task_id: String,
    pub to_task_id: String,
    /// Dependency type: "blocks", "follows", "contains", or custom types.
    pub dep_type: String,
}

/// An advisory file lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLock {
    pub file_path: String,
    pub worker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub locked_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// A claim event for file coordination tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimEvent {
    pub id: i64,
    pub file_path: String,
    pub worker_id: String,
    pub event: ClaimEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<i64>,
    /// For release events: the ID of the corresponding claim event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<i64>,
}

/// A unified task sequence event for tracking status and phase changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSequenceEvent {
    pub id: i64,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// Status value (None if phase-only change)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Phase value (None if status-only change)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<i64>,
}

/// Legacy alias for backward compatibility in exports.
/// A task state transition event for time tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateEvent {
    pub id: i64,
    pub task_id: String,
    pub worker_id: Option<String>,
    pub event: String,
    pub reason: Option<String>,
    pub timestamp: i64,
    pub end_timestamp: Option<i64>,
}

/// Type of claim event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimEventType {
    Claimed,
    Released,
}

impl ClaimEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimEventType::Claimed => "claimed",
            ClaimEventType::Released => "released",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claimed" => Some(ClaimEventType::Claimed),
            "released" => Some(ClaimEventType::Released),
            _ => None,
        }
    }
}

/// Result of polling claim updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimUpdates {
    pub new_claims: Vec<ClaimEvent>,
    pub dropped_claims: Vec<ClaimEvent>,
    pub sequence: i64,
}

/// An attachment on a task.
/// Primary key is (task_id, attachment_type, sequence).
/// If file_path is set, content is stored in the referenced file; otherwise content is inline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub task_id: String,
    pub attachment_type: String,
    pub sequence: i32,
    pub name: String,
    pub mime_type: String,
    pub content: String,
    /// Path to the file containing the content (relative to media dir or absolute).
    /// If set, content is read from this file; if None, content is stored inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub created_at: i64,
}

/// Attachment metadata (without content).
/// Primary key is (task_id, attachment_type, sequence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub task_id: String,
    pub attachment_type: String,
    pub sequence: i32,
    pub name: String,
    pub mime_type: String,
    /// Path to the file containing the content (if stored as file).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub created_at: i64,
}

/// Aggregate statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub total_tasks: i64,
    /// Task counts by state (dynamic based on config).
    pub tasks_by_status: HashMap<String, i64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub total_points: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub completed_points: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub total_time_estimate_ms: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub total_time_actual_ms: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub total_cost_usd: f64,
    /// Aggregated metrics [metric_0..metric_7]
    #[serde(
        with = "metrics_serde",
        skip_serializing_if = "metrics_serde::is_empty",
        default
    )]
    pub total_metrics: [i64; 8],
}

/// Compact task representation for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "is_default_priority")]
    pub priority: Priority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_thought: Option<String>,
}

/// Result of scanning the task graph from a starting task.
/// Contains tasks organized by traversal direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// The task that was scanned from
    pub root: Task,
    /// Tasks that block this task (predecessors via blocks/follows)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<Task>,
    /// Tasks that this task blocks (successors via blocks/follows)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<Task>,
    /// Parent chain (ancestors via contains)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub above: Vec<Task>,
    /// Children tree (descendants via contains)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub below: Vec<Task>,
}

/// Summary of disconnect operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectSummary {
    /// Number of tasks that were released.
    pub tasks_released: i32,
    /// Number of file locks that were released.
    pub files_released: i32,
    /// The final status applied to released tasks.
    pub final_status: String,
}

/// Summary of stale worker cleanup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupSummary {
    /// Number of stale workers evicted.
    pub workers_evicted: i32,
    /// Total number of tasks released across all evicted workers.
    pub tasks_released: i32,
    /// Total number of file locks released across all evicted workers.
    pub files_released: i32,
    /// The final status applied to released tasks.
    pub final_status: String,
    /// IDs of evicted workers.
    pub evicted_worker_ids: Vec<String>,
}

/// A task tag row for export/import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTagRow {
    pub task_id: String,
    pub tag: String,
}

/// A task needed tag row for export/import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNeededTagRow {
    pub task_id: String,
    pub tag: String,
}

/// A task wanted tag row for export/import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskWantedTagRow {
    pub task_id: String,
    pub tag: String,
}

/// Exported tables container for database export.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportTables {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<Task>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<Dependency>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_tags: Option<Vec<TaskTagRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_needed_tags: Option<Vec<TaskNeededTagRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_wanted_tags: Option<Vec<TaskWantedTagRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_sequence: Option<Vec<TaskSequenceEvent>>,
}

#[cfg(test)]
mod tests {
    // Priority tests removed - Priority is now a type alias for i32
}
