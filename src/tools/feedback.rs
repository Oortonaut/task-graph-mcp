//! Agent feedback tools.
//!
//! Feedback is stored as a simple, human-readable, append-only markdown file.

use crate::error::ToolError;
use crate::tools::{get_string, make_tool};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Valid feedback categories.
const CATEGORIES: &[&str] = &["tool", "workflow", "config", "ux", "general"];

/// Valid feedback sentiments.
const SENTIMENTS: &[&str] = &["positive", "negative", "neutral", "suggestion"];

/// Feedback file name.
const FEEDBACK_FILE: &str = "feedback.md";

/// Get feedback tool definitions.
pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "give_feedback",
            "Submit feedback about tools, workflows, configuration, or UX. \
             Appends to a human-readable markdown file. Never shared automatically.",
            json!({
                "message": {
                    "type": "string",
                    "description": "The feedback message"
                },
                "category": {
                    "type": "string",
                    "enum": CATEGORIES,
                    "description": "Feedback category (default: general)",
                    "default": "general"
                },
                "sentiment": {
                    "type": "string",
                    "enum": SENTIMENTS,
                    "description": "Sentiment of the feedback (default: neutral)",
                    "default": "neutral"
                },
                "agent_id": {
                    "type": "string",
                    "description": "ID of the agent submitting feedback"
                },
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool this feedback is about"
                },
                "task_id": {
                    "type": "string",
                    "description": "ID of the task this feedback relates to"
                }
            }),
            vec!["message"],
        ),
        make_tool(
            "list_feedback",
            "Read the feedback markdown file. Returns the raw contents.",
            json!({}),
            vec![],
        ),
    ]
}

/// Resolve the feedback file path next to the database file.
fn feedback_path(db_dir: &Path) -> std::path::PathBuf {
    db_dir.join(FEEDBACK_FILE)
}

/// Handle the give_feedback tool call.
pub fn give_feedback(db_dir: &Path, args: Value) -> Result<Value> {
    let message =
        get_string(&args, "message").ok_or_else(|| ToolError::missing_field("message"))?;

    if message.trim().is_empty() {
        return Err(ToolError::invalid_value("message", "message cannot be empty").into());
    }

    let category = get_string(&args, "category").unwrap_or_else(|| "general".to_string());
    if !CATEGORIES.contains(&category.as_str()) {
        return Err(ToolError::invalid_value(
            "category",
            &format!(
                "Invalid category '{}'. Must be one of: {}",
                category,
                CATEGORIES.join(", ")
            ),
        )
        .into());
    }

    let sentiment = get_string(&args, "sentiment").unwrap_or_else(|| "neutral".to_string());
    if !SENTIMENTS.contains(&sentiment.as_str()) {
        return Err(ToolError::invalid_value(
            "sentiment",
            &format!(
                "Invalid sentiment '{}'. Must be one of: {}",
                sentiment,
                SENTIMENTS.join(", ")
            ),
        )
        .into());
    }

    let agent_id = get_string(&args, "agent_id");
    let tool_name = get_string(&args, "tool_name");
    let task_id = get_string(&args, "task_id");

    let path = feedback_path(db_dir);

    // If file doesn't exist yet, write the header
    let needs_header = !path.exists();

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

    if needs_header {
        writeln!(file, "# Agent Feedback\n")?;
    }

    // Build the entry
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    writeln!(file, "---\n")?;
    writeln!(file, "### {} | {} | {}\n", timestamp, category, sentiment)?;

    // Optional metadata lines
    if let Some(ref agent) = agent_id {
        writeln!(file, "- **Agent:** {}", agent)?;
    }
    if let Some(ref tool) = tool_name {
        writeln!(file, "- **Tool:** {}", tool)?;
    }
    if let Some(ref task) = task_id {
        writeln!(file, "- **Task:** {}", task)?;
    }
    if agent_id.is_some() || tool_name.is_some() || task_id.is_some() {
        writeln!(file)?;
    }

    writeln!(file, "{}\n", message)?;

    Ok(json!({
        "status": "recorded",
        "file": path.display().to_string()
    }))
}

/// Handle the list_feedback tool call.
pub fn list_feedback(db_dir: &Path) -> Result<Value> {
    let path = feedback_path(db_dir);

    if !path.exists() {
        return Ok(json!({
            "content": "",
            "file": path.display().to_string(),
            "message": "No feedback recorded yet."
        }));
    }

    let content = fs::read_to_string(&path)?;

    Ok(json!({
        "content": content,
        "file": path.display().to_string()
    }))
}
