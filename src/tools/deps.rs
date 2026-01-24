//! Dependency management tools.

use super::{get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "block",
            "Add a dependency: blocker must complete before blocked can be claimed. Rejects cycles.",
            json!({
                "blocker": {
                    "type": "string",
                    "description": "Task ID that blocks"
                },
                "blocked": {
                    "type": "string",
                    "description": "Task ID that is blocked"
                }
            }),
            vec!["blocker", "blocked"],
            prompts,
        ),
        make_tool_with_prompts(
            "unblock",
            "Remove a dependency.",
            json!({
                "blocker": {
                    "type": "string",
                    "description": "Task ID that blocks"
                },
                "blocked": {
                    "type": "string",
                    "description": "Task ID that is blocked"
                }
            }),
            vec!["blocker", "blocked"],
            prompts,
        ),
    ]
}

pub fn block(db: &Database, args: Value) -> Result<Value> {
    let blocker = get_string(&args, "blocker")
        .ok_or_else(|| anyhow::anyhow!("blocker is required"))?;
    let blocked = get_string(&args, "blocked")
        .ok_or_else(|| anyhow::anyhow!("blocked is required"))?;

    db.add_dependency(&blocker, &blocked)?;

    Ok(json!({
        "success": true
    }))
}

pub fn unblock(db: &Database, args: Value) -> Result<Value> {
    let blocker = get_string(&args, "blocker")
        .ok_or_else(|| anyhow::anyhow!("blocker is required"))?;
    let blocked = get_string(&args, "blocked")
        .ok_or_else(|| anyhow::anyhow!("blocked is required"))?;

    db.remove_dependency(&blocker, &blocked)?;

    Ok(json!({
        "success": true
    }))
}
