//! Structured error and warning types for tool responses.

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
    InvalidPath,
    InvalidPrefix,

    // Not found errors
    AgentNotFound,
    TaskNotFound,
    FileNotFound,
    AttachmentNotFound,

    // Conflict errors
    AlreadyClaimed,
    AlreadyExists,
    DependencyCycle,
    TagMismatch,
    NotOwner,
    DependencyNotSatisfied,
    GatesNotSatisfied,

    // Internal errors
    DatabaseError,
    InternalError,
    UnknownTool,
}

/// Warning codes for non-fatal issues.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WarningCode {
    /// Referenced task does not exist (link skipped)
    TaskNotFound,
    /// Referenced dependency does not exist (link skipped)
    DependencyNotFound,
    /// Tag is not in the known tags list
    UnknownTag,
    /// Phase is not in the known phases list
    UnknownPhase,
    /// Duplicate operation (no-op)
    Duplicate,
    /// Deprecated feature or parameter
    Deprecated,
}

/// A warning about a non-fatal issue in a tool operation.
#[derive(Debug, Clone, Serialize)]
pub struct ToolWarning {
    pub code: WarningCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl ToolWarning {
    pub fn new(code: WarningCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
            value: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    // Convenience constructors

    pub fn task_not_found(task_id: &str) -> Self {
        Self::new(
            WarningCode::TaskNotFound,
            format!("Task '{}' not found, skipped", task_id),
        )
        .with_value(task_id)
    }

    pub fn dependency_not_found(task_id: &str, field: &str) -> Self {
        Self::new(
            WarningCode::DependencyNotFound,
            format!("Dependency target '{}' not found, link skipped", task_id),
        )
        .with_field(field)
        .with_value(task_id)
    }

    pub fn unknown_tag(tag: &str) -> Self {
        Self::new(
            WarningCode::UnknownTag,
            format!("Tag '{}' is not in known tags list", tag),
        )
        .with_value(tag)
    }

    pub fn unknown_phase(phase: &str) -> Self {
        Self::new(
            WarningCode::UnknownPhase,
            format!("Phase '{}' is not in known phases list", phase),
        )
        .with_value(phase)
    }

    pub fn duplicate(what: &str) -> Self {
        Self::new(WarningCode::Duplicate, format!("{} already exists", what))
    }

    pub fn deprecated(feature: &str, alternative: &str) -> Self {
        Self::new(
            WarningCode::Deprecated,
            format!("'{}' is deprecated, use '{}' instead", feature, alternative),
        )
    }
}

impl fmt::Display for ToolWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
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

    pub fn gates_not_satisfied(status: &str, gates: &[String]) -> Self {
        Self::new(
            ErrorCode::GatesNotSatisfied,
            format!(
                "Cannot exit '{}': unsatisfied gates: {}",
                status,
                gates.join(", ")
            ),
        )
    }

    pub fn invalid_path(path: &str, reason: &str) -> Self {
        Self::new(
            ErrorCode::InvalidPath,
            format!("Invalid path '{}': {}", path, reason),
        )
    }

    pub fn prefix_not_lowercase(prefix: &str) -> Self {
        Self::new(
            ErrorCode::InvalidPrefix,
            format!("Path prefix '{}' must be lowercase", prefix),
        )
    }

    pub fn unknown_prefix(prefix: &str) -> Self {
        Self::new(
            ErrorCode::InvalidPrefix,
            format!("Unknown path prefix: {}", prefix),
        )
    }

    pub fn sandbox_escape(path: &str, root: &str) -> Self {
        Self::new(
            ErrorCode::InvalidPath,
            format!("Path '{}' escapes sandbox root '{}'", path, root),
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
