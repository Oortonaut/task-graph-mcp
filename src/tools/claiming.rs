//! Task claiming and release tools.

use super::{get_i64, get_string, get_uuid, make_tool};
use crate::db::Database;
use crate::types::{EventType, TargetType};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "claim_task",
            "Claim a task before working on it. Fails if: already claimed, dependencies unsatisfied, agent at max_claims limit, or agent lacks required tags. Sets status to in_progress.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID to claim"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID claiming the task"
                }
            }),
            vec!["task_id", "agent_id"],
        ),
        make_tool(
            "release_task",
            "Release a claimed task without completing it. Resets status to pending so another agent can claim it. Must be the current owner.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID to release"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID releasing the task"
                }
            }),
            vec!["task_id", "agent_id"],
        ),
        make_tool(
            "force_release",
            "Force release a task regardless of owner.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID to force release"
                }
            }),
            vec!["task_id"],
        ),
        make_tool(
            "force_release_stale",
            "Release all claims older than the timeout.",
            json!({
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 900 = 15 min)"
                }
            }),
            vec![],
        ),
    ]
}

pub fn claim_task(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let task = db.claim_task(task_id, &agent_id)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task.id.to_string(),
        EventType::TaskClaimed,
        json!({
            "task_id": task.id.to_string(),
            "agent_id": &agent_id
        }),
    );

    Ok(json!({
        "success": true,
        "task": {
            "id": task.id.to_string(),
            "title": task.title,
            "status": task.status.as_str(),
            "owner_agent": task.owner_agent,
            "claimed_at": task.claimed_at
        }
    }))
}

pub fn release_task(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    db.release_task(task_id, &agent_id)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task_id.to_string(),
        EventType::TaskReleased,
        json!({
            "task_id": task_id.to_string(),
            "agent_id": &agent_id
        }),
    );

    Ok(json!({
        "success": true
    }))
}

pub fn force_release(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;

    db.force_release(task_id)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task_id.to_string(),
        EventType::TaskReleased,
        json!({
            "task_id": task_id.to_string(),
            "forced": true
        }),
    );

    Ok(json!({
        "success": true
    }))
}

pub fn force_release_stale(db: &Database, args: Value) -> Result<Value> {
    let timeout_seconds = get_i64(&args, "timeout_seconds").unwrap_or(900);

    let released = db.force_release_stale(timeout_seconds)?;

    Ok(json!({
        "success": true,
        "released_count": released
    }))
}
