//! Dependency management tools.

use super::{get_uuid, make_tool};
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "add_dependency",
            "Add a dependency: from_task_id must complete before to_task_id can be claimed. Rejects cycles.",
            json!({
                "from_task_id": {
                    "type": "string",
                    "description": "Task UUID that blocks"
                },
                "to_task_id": {
                    "type": "string",
                    "description": "Task UUID that is blocked"
                }
            }),
            vec!["from_task_id", "to_task_id"],
        ),
        make_tool(
            "remove_dependency",
            "Remove a dependency.",
            json!({
                "from_task_id": {
                    "type": "string",
                    "description": "Task UUID that blocks"
                },
                "to_task_id": {
                    "type": "string",
                    "description": "Task UUID that is blocked"
                }
            }),
            vec!["from_task_id", "to_task_id"],
        ),
        make_tool(
            "get_blocked_tasks",
            "Get all tasks that are blocked by incomplete dependencies.",
            json!({}),
            vec![],
        ),
        make_tool(
            "get_ready_tasks",
            "Get tasks ready to work on: unclaimed, pending status, all dependencies completed. Returns needed_tags/wanted_tags so you can filter by your capabilities.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent UUID to exclude from results (optional)"
                }
            }),
            vec![],
        ),
    ]
}

pub fn add_dependency(db: &Database, args: Value) -> Result<Value> {
    let from_task_id = get_uuid(&args, "from_task_id")
        .ok_or_else(|| anyhow::anyhow!("from_task_id is required"))?;
    let to_task_id = get_uuid(&args, "to_task_id")
        .ok_or_else(|| anyhow::anyhow!("to_task_id is required"))?;

    db.add_dependency(from_task_id, to_task_id)?;

    Ok(json!({
        "success": true
    }))
}

pub fn remove_dependency(db: &Database, args: Value) -> Result<Value> {
    let from_task_id = get_uuid(&args, "from_task_id")
        .ok_or_else(|| anyhow::anyhow!("from_task_id is required"))?;
    let to_task_id = get_uuid(&args, "to_task_id")
        .ok_or_else(|| anyhow::anyhow!("to_task_id is required"))?;

    db.remove_dependency(from_task_id, to_task_id)?;

    Ok(json!({
        "success": true
    }))
}

pub fn get_blocked_tasks(db: &Database, _args: Value) -> Result<Value> {
    let tasks = db.get_blocked_tasks()?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| json!({
            "id": t.id.to_string(),
            "title": t.title,
            "status": t.status.as_str(),
            "priority": t.priority.as_str()
        })).collect::<Vec<_>>()
    }))
}

pub fn get_ready_tasks(db: &Database, args: Value) -> Result<Value> {
    let exclude_agent = get_uuid(&args, "agent_id");

    let tasks = db.get_ready_tasks(exclude_agent)?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| json!({
            "id": t.id.to_string(),
            "title": t.title,
            "status": t.status.as_str(),
            "priority": t.priority.as_str(),
            "points": t.points,
            "needed_tags": t.needed_tags,
            "wanted_tags": t.wanted_tags
        })).collect::<Vec<_>>()
    }))
}
