//! Core types for the Task Graph MCP Server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent (session-based) - represents a connected agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: Option<String>,
    pub tags: Vec<String>,
    pub max_claims: i32,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

/// Agent info with additional runtime details for list_agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: Option<String>,
    pub tags: Vec<String>,
    pub max_claims: i32,
    pub claim_count: i32,
    pub current_thought: Option<String>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
}

/// Task priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "low" => Some(Priority::Low),
            _ => None,
        }
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
    pub needed_tags: Vec<String>,
    pub wanted_tags: Vec<String>,

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

    // Cost accounting (fixed categories)
    pub tokens_in: i64,
    pub tokens_cached: i64,
    pub tokens_out: i64,
    pub tokens_thinking: i64,
    pub tokens_image: i64,
    pub tokens_audio: i64,
    pub cost_usd: f64,
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

/// Input for creating a task tree.
/// Input for creating a task tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTreeInput {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    /// If true, children run in parallel (no follows dependencies).
    /// If false (default), children run sequentially (follows dependencies created).
    #[serde(default)]
    pub parallel: bool,
    pub points: Option<i32>,
    pub time_estimate_ms: Option<i64>,
    pub needed_tags: Option<Vec<String>>,
    pub wanted_tags: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
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
    pub agent_id: String,
    pub reason: Option<String>,
    pub locked_at: i64,
}

/// A claim event for file coordination tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimEvent {
    pub id: i64,
    pub file_path: String,
    pub agent_id: String,
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
    pub agent_id: Option<String>,
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
    pub total_tokens_in: i64,
    pub total_tokens_cached: i64,
    pub total_tokens_out: i64,
    pub total_tokens_thinking: i64,
    pub total_tokens_image: i64,
    pub total_tokens_audio: i64,
    pub total_cost_usd: f64,
}

/// Compact task representation for list views.
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

#[cfg(test)]
mod tests {
    use super::*;

    mod priority_tests {
        use super::*;

        #[test]
        fn as_str_returns_correct_string_for_all_variants() {
            assert_eq!(Priority::High.as_str(), "high");
            assert_eq!(Priority::Medium.as_str(), "medium");
            assert_eq!(Priority::Low.as_str(), "low");
        }

        #[test]
        fn from_str_parses_valid_strings() {
            assert_eq!(Priority::from_str("high"), Some(Priority::High));
            assert_eq!(Priority::from_str("medium"), Some(Priority::Medium));
            assert_eq!(Priority::from_str("low"), Some(Priority::Low));
        }

        #[test]
        fn from_str_returns_none_for_invalid_strings() {
            assert_eq!(Priority::from_str("invalid"), None);
            assert_eq!(Priority::from_str("HIGH"), None);
            assert_eq!(Priority::from_str("critical"), None);
        }

        #[test]
        fn roundtrip_conversion_is_lossless() {
            for priority in [Priority::High, Priority::Medium, Priority::Low] {
                assert_eq!(Priority::from_str(priority.as_str()), Some(priority));
            }
        }
    }



}
