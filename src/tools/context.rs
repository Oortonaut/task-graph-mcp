//! Per-request context passed to tool functions.

use crate::logging::Logger;

/// Per-request context passed to all tools.
///
/// This provides access to:
/// - Unified logger for outputting to both tracing and MCP client
#[derive(Clone)]
pub struct ToolContext {
    /// Unified logger for this request.
    pub logger: Logger,
}

impl ToolContext {
    /// Create a new tool context with the given logger.
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}
