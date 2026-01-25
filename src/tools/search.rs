//! Full-text search tool.

use super::{get_bool, get_i32, get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![make_tool_with_prompts(
        "search",
        "Search tasks using full-text search. Supports FTS5 query syntax: simple words, phrases (\"exact phrase\"), prefix (word*), boolean (AND, OR, NOT), and column-specific (title:word, description:word). Returns ranked results with highlighted snippets.",
        json!({
            "query": {
                "type": "string",
                "description": "Search query string. Supports FTS5 syntax: words, \"phrases\", prefix*, AND/OR/NOT, title:word"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results to return (default: 20, max: 100)"
            },
            "include_attachments": {
                "type": "boolean",
                "description": "Whether to also search attachment content (default: false)"
            },
            "status_filter": {
                "type": "string",
                "description": "Optional status to filter results by (e.g., 'pending', 'in_progress')"
            }
        }),
        vec!["query"],
        prompts,
    )]
}

pub fn search(db: &Database, args: Value) -> Result<Value> {
    let query = get_string(&args, "query")
        .ok_or_else(|| ToolError::missing_field("query"))?;
    let limit = get_i32(&args, "limit");
    let include_attachments = get_bool(&args, "include_attachments").unwrap_or(false);
    let status_filter = get_string(&args, "status_filter");

    let results = db.search_tasks(
        &query,
        limit,
        include_attachments,
        status_filter.as_deref(),
    )?;

    Ok(json!({
        "query": query,
        "result_count": results.len(),
        "results": results
    }))
}
