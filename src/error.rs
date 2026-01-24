//! Structured error types for tool responses.

use serde::Serialize;
use std::fmt;

/// Error codes for programmatic error handling.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // Validation errors (4xx-like)
    MissingRequiredField,
    InvalidFieldValue,
    InvalidState,

    // Not found errors
    AgentNotFound,
    TaskNotFound,
    FileNotFound,
    AttachmentNotFound,

    // Conflict errors
    AlreadyClaimed,
    AlreadyExists,
    DependencyCycle,
    ClaimLimitExceeded,
    TagMismatch,
    NotOwner,
    DependencyNotSatisfied,

    // Internal errors
    DatabaseError,
    InternalError,
    UnknownTool,
}

/// Structured error for tool responses.
#[derive(Debug, Serialize)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
            details: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    // Convenience constructors

    pub fn missing_field(field: &str) -> Self {
        Self::new(
            ErrorCode::MissingRequiredField,
            format!("{} is required", field),
        )
        .with_field(field)
    }

    pub fn invalid_value(field: &str, reason: &str) -> Self {
        Self::new(ErrorCode::InvalidFieldValue, reason).with_field(field)
    }

    pub fn agent_not_found(agent_id: &str) -> Self {
        Self::new(
            ErrorCode::AgentNotFound,
            format!("Agent not found: {}", agent_id),
        )
    }

    pub fn task_not_found(task_id: &str) -> Self {
        Self::new(
            ErrorCode::TaskNotFound,
            format!("Task not found: {}", task_id),
        )
    }

    pub fn already_claimed(task_id: &str, owner: &str) -> Self {
        Self::new(
            ErrorCode::AlreadyClaimed,
            format!("Task {} already claimed by {}", task_id, owner),
        )
    }

    pub fn not_owner(task_id: &str, agent_id: &str) -> Self {
        Self::new(
            ErrorCode::NotOwner,
            format!("Agent {} does not own task {}", agent_id, task_id),
        )
    }

    pub fn dependency_cycle(blocker: &str, blocked: &str) -> Self {
        Self::new(
            ErrorCode::DependencyCycle,
            format!(
                "Adding dependency {} -> {} would create a cycle",
                blocker, blocked
            ),
        )
    }

    pub fn claim_limit(agent_id: &str, limit: i32) -> Self {
        Self::new(
            ErrorCode::ClaimLimitExceeded,
            format!("Agent {} has reached claim limit of {}", agent_id, limit),
        )
    }

    pub fn tag_mismatch(missing: &str) -> Self {
        Self::new(
            ErrorCode::TagMismatch,
            format!("Agent missing required tag(s): {}", missing),
        )
    }

    pub fn deps_not_satisfied(blockers: &[String]) -> Self {
        Self::new(
            ErrorCode::DependencyNotSatisfied,
            format!("Task blocked by: {}", blockers.join(", ")),
        )
    }

    pub fn database(err: impl fmt::Display) -> Self {
        Self::new(ErrorCode::DatabaseError, err.to_string())
    }

    pub fn internal(err: impl fmt::Display) -> Self {
        Self::new(ErrorCode::InternalError, err.to_string())
    }

    pub fn unknown_tool(name: &str) -> Self {
        Self::new(ErrorCode::UnknownTool, format!("Unknown tool: {}", name))
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolError {}

// Allow using ? with anyhow errors by converting them
impl From<anyhow::Error> for ToolError {
    fn from(err: anyhow::Error) -> Self {
        // Try to downcast to ToolError first
        match err.downcast::<ToolError>() {
            Ok(tool_err) => tool_err,
            Err(err) => ToolError::internal(err),
        }
    }
}


/// Result type for tool operations.
pub type ToolResult<T> = std::result::Result<T, ToolError>;
