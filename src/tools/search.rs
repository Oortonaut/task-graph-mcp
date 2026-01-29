//! Full-text search tool.

use super::{get_bool, get_i32, get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

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
            "offset": {
                "type": "integer",
                "description": "Number of results to skip for pagination (default: 0)"
            },
            "include_attachments": {
                "type": "boolean",
                "description": "Whether to also search attachment content (default: false)"
            },
            "status_filter": {
                "type": "string",
                "description": "Optional status to filter results by (e.g., 'pending', 'working')"
            }
        }),
        vec!["query"],
        prompts,
    )]
}

pub fn search(db: &Database, default_page_size: i32, args: Value) -> Result<Value> {
    let query = get_string(&args, "query").ok_or_else(|| ToolError::missing_field("query"))?;
    let limit = get_i32(&args, "limit")
        .unwrap_or(default_page_size.min(20))
        .clamp(1, 100);
    let offset = get_i32(&args, "offset").unwrap_or(0).max(0);
    let include_attachments = get_bool(&args, "include_attachments").unwrap_or(false);
    let status_filter = get_string(&args, "status_filter");

    // Fetch limit+1 to detect if there are more results
    let fetch_limit = limit + 1;
    let results = db.search_tasks(
        &query,
        Some(fetch_limit),
        offset,
        include_attachments,
        status_filter.as_deref(),
    )?;

    let has_more = results.len() > limit as usize;
    let results: Vec<_> = results.into_iter().take(limit as usize).collect();
    let result_count = results.len() as i32;

    Ok(json!({
        "query": query,
        "result_count": result_count,
        "has_more": has_more,
        "offset": offset,
        "limit": limit,
        "results": results
    }))
}
