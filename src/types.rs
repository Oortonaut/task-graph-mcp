//! Core types for the Task Graph MCP Server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Worker (session-based) - represents a connected worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub tags: Vec<String>,
    pub max_claims: i32,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

/// Worker info with additional runtime details for list_workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub max_claims: i32,
    pub claim_count: i32,
    pub current_thought: Option<String>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

/// Task priority as an integer (higher = more important).
/// Default is 0. Typical range: -100 to 100.
pub type Priority = i32;

/// Priority constants for convenience.
pub const PRIORITY_HIGH: Priority = 1;
pub const PRIORITY_MEDIUM: Priority = 0;
pub const PRIORITY_LOW: Priority = -1;

/// Parse a priority string ("high", "medium", "low") to an integer.
/// Returns 0 (medium) for unrecognized values.
pub fn parse_priority(s: &str) -> Priority {
    match s.to_lowercase().as_str() {
        "high" => PRIORITY_HIGH,
        "medium" => PRIORITY_MEDIUM,
        "low" => PRIORITY_LOW,
        _ => s.parse().unwrap_or(PRIORITY_MEDIUM),
    }
}

/// Convert priority integer to string representation.
pub fn priority_to_str(p: Priority) -> &'static str {
    if p > 0 {
        "high"
    } else if p < 0 {
        "low"
    } else {
        "medium"
    }
}

/// A task in the task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: Priority,
    pub owner_agent: Option<String>,
    pub claimed_at: Option<i64>,

    // Affinity (tag-based claiming requirements)
    pub agent_tags_all: Vec<String>,
    pub agent_tags_any: Vec<String>,

    // Categorization/discovery tags
    pub tags: Vec<String>,

    // Estimation & tracking
    pub points: Option<i32>,
    pub time_estimate_ms: Option<i64>,
    pub time_actual_ms: Option<i64>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,

    // Live status
    pub current_thought: Option<String>,

    // Cost accounting
    pub cost_usd: f64,
    /// Fixed array of 8 integer metrics [metric_0..metric_7], aggregated on update
    pub metrics: [i64; 8],
    pub user_metrics: Option<HashMap<String, serde_json::Value>>,

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

/// Join mode for tree children: how children relate to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinMode {
    /// Children run sequentially with "follows" dependencies between them.
    #[default]
    Then,
    /// Children run in parallel with no dependencies between them.
    Also,
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
    
    /// Custom task ID (optional, UUID7 generated if not provided).
    /// Ignored if ref is set.
    pub id: Option<String>,
    
    /// Task title (required for new tasks, optional if ref is set).
    #[serde(default)]
    pub title: String,
    
    /// Task description.
    pub description: Option<String>,
    
    /// Task priority.
    pub priority: Option<Priority>,
    
    /// How children of this node relate to each other.
    /// "then" = sequential (follows deps), "also" = parallel (no deps).
    #[serde(default)]
    pub join_mode: JoinMode,
    
    /// Story points / complexity estimate.
    pub points: Option<i32>,
    
    /// Estimated duration in milliseconds.
    pub time_estimate_ms: Option<i64>,
    
    /// Tags that claiming agent must have ALL of (AND logic).
    pub agent_tags_all: Option<Vec<String>>,
    
    /// Tags that claiming agent must have AT LEAST ONE of (OR logic).
    pub agent_tags_any: Option<Vec<String>>,
    
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
    pub reason: Option<String>,
    pub locked_at: i64,
    pub task_id: Option<String>,
}

/// A claim event for file coordination tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimEvent {
    pub id: i64,
    pub file_path: String,
    pub worker_id: String,
    pub event: ClaimEventType,
    pub reason: Option<String>,
    pub timestamp: i64,
    pub end_timestamp: Option<i64>,
    /// For release events: the ID of the corresponding claim event.
    pub claim_id: Option<i64>,
}

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

    pub fn from_str(s: &str) -> Option<Self> {
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
/// Primary key is (task_id, order_index).
/// If file_path is set, content is stored in the referenced file; otherwise content is inline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub task_id: String,
    pub order_index: i32,
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
/// Primary key is (task_id, order_index).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub task_id: String,
    pub order_index: i32,
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
    pub tasks_by_state: HashMap<String, i64>,
    pub total_points: i64,
    pub completed_points: i64,
    pub total_time_estimate_ms: i64,
    pub total_time_actual_ms: i64,
    pub total_cost_usd: f64,
    /// Aggregated metrics [metric_0..metric_7]
    pub total_metrics: [i64; 8],
}

/// Compact task representation for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: Priority,
    pub owner_agent: Option<String>,
    pub points: Option<i32>,
    pub current_thought: Option<String>,
}

/// Result of scanning the task graph from a starting task.
/// Contains tasks organized by traversal direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// The task that was scanned from
    pub root: Task,
    /// Tasks that block this task (predecessors via blocks/follows)
    pub before: Vec<Task>,
    /// Tasks that this task blocks (successors via blocks/follows)
    pub after: Vec<Task>,
    /// Parent chain (ancestors via contains)
    pub above: Vec<Task>,
    /// Children tree (descendants via contains)
    pub below: Vec<Task>,
}


/// Summary of disconnect operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectSummary {
    /// Number of tasks that were released.
    pub tasks_released: i32,
    /// Number of file locks that were released.
    pub files_released: i32,
    /// The final state applied to released tasks.
    pub final_state: String,
}

#[cfg(test)]
mod tests {
    // Priority tests removed - Priority is now a type alias for i32



}
