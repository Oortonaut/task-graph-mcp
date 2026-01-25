//! Dependency management tools.

use super::{get_string, make_tool_with_prompts};
use crate::config::{DependenciesConfig, Prompts};
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts, deps_config: &DependenciesConfig) -> Vec<Tool> {
    // Build enum of dependency types from config
    let dep_types: Vec<Value> = deps_config
        .dep_type_names()
        .into_iter()
        .map(|s| json!(s))
        .collect();

    vec![
        make_tool_with_prompts(
            "block",
            "Add a typed dependency: blocker must complete before blocked can start/complete (based on type). Rejects cycles.",
            json!({
                "blocker": {
                    "type": "string",
                    "description": "Task ID that blocks"
                },
                "blocked": {
                    "type": "string",
                    "description": "Task ID that is blocked"
                },
                "type": {
                    "type": "string",
                    "enum": dep_types,
                    "description": "Dependency type (default: 'blocks')"
                }
            }),
            vec!["blocker", "blocked"],
            prompts,
        ),
        make_tool_with_prompts(
            "unblock",
            "Remove a typed dependency.",
            json!({
                "blocker": {
                    "type": "string",
                    "description": "Task ID that blocks"
                },
                "blocked": {
                    "type": "string",
                    "description": "Task ID that is blocked"
                },
                "type": {
                    "type": "string",
                    "enum": dep_types,
                    "description": "Dependency type (default: 'blocks')"
                }
            }),
            vec!["blocker", "blocked"],
            prompts,
        ),
    ]
}

pub fn block(db: &Database, deps_config: &DependenciesConfig, args: Value) -> Result<Value> {
    let blocker = get_string(&args, "blocker").ok_or_else(|| ToolError::missing_field("blocker"))?;
    let blocked = get_string(&args, "blocked").ok_or_else(|| ToolError::missing_field("blocked"))?;
    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    db.add_dependency(&blocker, &blocked, &dep_type, deps_config)?;

    Ok(json!({
        "success": true,
        "type": dep_type
    }))
}

pub fn unblock(db: &Database, args: Value) -> Result<Value> {
    let blocker = get_string(&args, "blocker").ok_or_else(|| ToolError::missing_field("blocker"))?;
    let blocked = get_string(&args, "blocked").ok_or_else(|| ToolError::missing_field("blocked"))?;
    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    db.remove_dependency(&blocker, &blocked, &dep_type)?;

    Ok(json!({
        "success": true
    }))
}
