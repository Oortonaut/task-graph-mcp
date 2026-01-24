//! Core types for the Task Graph MCP Server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

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

/// Task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "failed" => Some(TaskStatus::Failed),
            "cancelled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }
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

/// Join mode for sibling tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinMode {
    /// This task depends on the previous sibling completing.
    Then,
    /// This task runs in parallel with the previous sibling.
    Also,
}

impl JoinMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            JoinMode::Then => "then",
            JoinMode::Also => "also",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "then" => Some(JoinMode::Then),
            "also" => Some(JoinMode::Also),
            _ => None,
        }
    }
}

/// A task in the task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: Priority,
    pub join_mode: JoinMode,
    pub sibling_order: i32,
    pub owner_agent: Option<String>,
    pub claimed_at: Option<i64>,

    // Affinity (tag-based)
    pub needed_tags: Vec<String>,
    pub wanted_tags: Vec<String>,

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

    pub metadata: Option<serde_json::Value>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTreeInput {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub join_mode: Option<JoinMode>,
    pub points: Option<i32>,
    pub time_estimate_ms: Option<i64>,
    pub needed_tags: Option<Vec<String>>,
    pub wanted_tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub children: Vec<TaskTreeInput>,
}

/// A dependency between tasks (from blocks to).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub from_task_id: Uuid,
    pub to_task_id: Uuid,
}

/// An advisory file lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLock {
    pub file_path: String,
    pub agent_id: String,
    pub locked_at: i64,
}

/// A subscription to events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub agent_id: String,
    pub target_type: TargetType,
    pub target_id: String,
    pub created_at: i64,
}

/// Target type for subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    Task,
    File,
    Agent,
}

impl TargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetType::Task => "task",
            TargetType::File => "file",
            TargetType::Agent => "agent",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "task" => Some(TargetType::Task),
            "file" => Some(TargetType::File),
            "agent" => Some(TargetType::Agent),
            _ => None,
        }
    }
}

/// An inbox message for pub/sub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: Uuid,
    pub agent_id: String,
    pub event_type: EventType,
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub read: bool,
}

/// Event types for pub/sub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    TaskCreated,
    TaskUpdated,
    TaskDeleted,
    TaskClaimed,
    TaskReleased,
    FileLocked,
    FileUnlocked,
    AgentRegistered,
    AgentTimeout,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::TaskCreated => "task_created",
            EventType::TaskUpdated => "task_updated",
            EventType::TaskDeleted => "task_deleted",
            EventType::TaskClaimed => "task_claimed",
            EventType::TaskReleased => "task_released",
            EventType::FileLocked => "file_locked",
            EventType::FileUnlocked => "file_unlocked",
            EventType::AgentRegistered => "agent_registered",
            EventType::AgentTimeout => "agent_timeout",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "task_created" => Some(EventType::TaskCreated),
            "task_updated" => Some(EventType::TaskUpdated),
            "task_deleted" => Some(EventType::TaskDeleted),
            "task_claimed" => Some(EventType::TaskClaimed),
            "task_released" => Some(EventType::TaskReleased),
            "file_locked" => Some(EventType::FileLocked),
            "file_unlocked" => Some(EventType::FileUnlocked),
            "agent_registered" => Some(EventType::AgentRegistered),
            "agent_timeout" => Some(EventType::AgentTimeout),
            _ => None,
        }
    }
}

/// An attachment on a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Uuid,
    pub task_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub content: String,
    pub created_at: i64,
}

/// Attachment metadata (without content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub id: Uuid,
    pub task_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub created_at: i64,
}

/// Aggregate statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub total_tasks: i64,
    pub pending_tasks: i64,
    pub in_progress_tasks: i64,
    pub completed_tasks: i64,
    pub failed_tasks: i64,
    pub cancelled_tasks: i64,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub owner_agent: Option<String>,
    pub points: Option<i32>,
    pub current_thought: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod task_status_tests {
        use super::*;

        #[test]
        fn as_str_returns_correct_string_for_all_variants() {
            assert_eq!(TaskStatus::Pending.as_str(), "pending");
            assert_eq!(TaskStatus::InProgress.as_str(), "in_progress");
            assert_eq!(TaskStatus::Completed.as_str(), "completed");
            assert_eq!(TaskStatus::Failed.as_str(), "failed");
            assert_eq!(TaskStatus::Cancelled.as_str(), "cancelled");
        }

        #[test]
        fn from_str_parses_valid_strings() {
            assert_eq!(TaskStatus::from_str("pending"), Some(TaskStatus::Pending));
            assert_eq!(TaskStatus::from_str("in_progress"), Some(TaskStatus::InProgress));
            assert_eq!(TaskStatus::from_str("completed"), Some(TaskStatus::Completed));
            assert_eq!(TaskStatus::from_str("failed"), Some(TaskStatus::Failed));
            assert_eq!(TaskStatus::from_str("cancelled"), Some(TaskStatus::Cancelled));
        }

        #[test]
        fn from_str_returns_none_for_invalid_strings() {
            assert_eq!(TaskStatus::from_str("invalid"), None);
            assert_eq!(TaskStatus::from_str("PENDING"), None);
            assert_eq!(TaskStatus::from_str(""), None);
            assert_eq!(TaskStatus::from_str("in-progress"), None);
        }

        #[test]
        fn roundtrip_conversion_is_lossless() {
            for status in [
                TaskStatus::Pending,
                TaskStatus::InProgress,
                TaskStatus::Completed,
                TaskStatus::Failed,
                TaskStatus::Cancelled,
            ] {
                assert_eq!(TaskStatus::from_str(status.as_str()), Some(status));
            }
        }
    }

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

    mod join_mode_tests {
        use super::*;

        #[test]
        fn as_str_returns_correct_string_for_all_variants() {
            assert_eq!(JoinMode::Then.as_str(), "then");
            assert_eq!(JoinMode::Also.as_str(), "also");
        }

        #[test]
        fn from_str_parses_valid_strings() {
            assert_eq!(JoinMode::from_str("then"), Some(JoinMode::Then));
            assert_eq!(JoinMode::from_str("also"), Some(JoinMode::Also));
        }

        #[test]
        fn from_str_returns_none_for_invalid_strings() {
            assert_eq!(JoinMode::from_str("invalid"), None);
            assert_eq!(JoinMode::from_str("THEN"), None);
            assert_eq!(JoinMode::from_str("parallel"), None);
        }

        #[test]
        fn roundtrip_conversion_is_lossless() {
            for mode in [JoinMode::Then, JoinMode::Also] {
                assert_eq!(JoinMode::from_str(mode.as_str()), Some(mode));
            }
        }
    }

    mod target_type_tests {
        use super::*;

        #[test]
        fn as_str_returns_correct_string_for_all_variants() {
            assert_eq!(TargetType::Task.as_str(), "task");
            assert_eq!(TargetType::File.as_str(), "file");
            assert_eq!(TargetType::Agent.as_str(), "agent");
        }

        #[test]
        fn from_str_parses_valid_strings() {
            assert_eq!(TargetType::from_str("task"), Some(TargetType::Task));
            assert_eq!(TargetType::from_str("file"), Some(TargetType::File));
            assert_eq!(TargetType::from_str("agent"), Some(TargetType::Agent));
        }

        #[test]
        fn from_str_returns_none_for_invalid_strings() {
            assert_eq!(TargetType::from_str("invalid"), None);
            assert_eq!(TargetType::from_str("TASK"), None);
        }

        #[test]
        fn roundtrip_conversion_is_lossless() {
            for target in [TargetType::Task, TargetType::File, TargetType::Agent] {
                assert_eq!(TargetType::from_str(target.as_str()), Some(target));
            }
        }
    }

    mod event_type_tests {
        use super::*;

        #[test]
        fn as_str_returns_correct_string_for_all_variants() {
            assert_eq!(EventType::TaskCreated.as_str(), "task_created");
            assert_eq!(EventType::TaskUpdated.as_str(), "task_updated");
            assert_eq!(EventType::TaskDeleted.as_str(), "task_deleted");
            assert_eq!(EventType::TaskClaimed.as_str(), "task_claimed");
            assert_eq!(EventType::TaskReleased.as_str(), "task_released");
            assert_eq!(EventType::FileLocked.as_str(), "file_locked");
            assert_eq!(EventType::FileUnlocked.as_str(), "file_unlocked");
            assert_eq!(EventType::AgentRegistered.as_str(), "agent_registered");
            assert_eq!(EventType::AgentTimeout.as_str(), "agent_timeout");
        }

        #[test]
        fn from_str_parses_valid_strings() {
            assert_eq!(EventType::from_str("task_created"), Some(EventType::TaskCreated));
            assert_eq!(EventType::from_str("task_updated"), Some(EventType::TaskUpdated));
            assert_eq!(EventType::from_str("task_deleted"), Some(EventType::TaskDeleted));
            assert_eq!(EventType::from_str("task_claimed"), Some(EventType::TaskClaimed));
            assert_eq!(EventType::from_str("task_released"), Some(EventType::TaskReleased));
            assert_eq!(EventType::from_str("file_locked"), Some(EventType::FileLocked));
            assert_eq!(EventType::from_str("file_unlocked"), Some(EventType::FileUnlocked));
            assert_eq!(EventType::from_str("agent_registered"), Some(EventType::AgentRegistered));
            assert_eq!(EventType::from_str("agent_timeout"), Some(EventType::AgentTimeout));
        }

        #[test]
        fn from_str_returns_none_for_invalid_strings() {
            assert_eq!(EventType::from_str("invalid"), None);
            assert_eq!(EventType::from_str("TASK_CREATED"), None);
        }

        #[test]
        fn roundtrip_conversion_is_lossless() {
            for event in [
                EventType::TaskCreated,
                EventType::TaskUpdated,
                EventType::TaskDeleted,
                EventType::TaskClaimed,
                EventType::TaskReleased,
                EventType::FileLocked,
                EventType::FileUnlocked,
                EventType::AgentRegistered,
                EventType::AgentTimeout,
            ] {
                assert_eq!(EventType::from_str(event.as_str()), Some(event));
            }
        }
    }
}
