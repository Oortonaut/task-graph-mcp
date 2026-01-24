//! Task claiming and release tools.

use super::{get_bool, get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::types::TaskStatus;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "claim",
            "Commit to working on a task (like adding to a changelist). Fails if: already claimed, deps unsatisfied, agent at max_claims limit, or agent lacks required tags. Sets status to in_progress.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID claiming the task"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID to claim"
                },
                "state": {
                    "type": "string",
                    "enum": ["pending", "in_progress"],
                    "description": "Optional state to set (default: in_progress)"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force claim even if owned by another agent (default: false)"
                }
            }),
            vec!["agent", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "release",
            "Release a claimed task. Use state='pending' for handoff to another agent, or state='failed'/'cancelled' to end the task.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID releasing the task"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID to release"
                },
                "state": {
                    "type": "string",
                    "enum": ["pending", "failed", "cancelled"],
                    "description": "State to set after release (default: pending)"
                }
            }),
            vec!["agent", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "complete",
            "Mark a task as completed. Shorthand for release with state=completed.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID completing the task"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID to complete"
                }
            }),
            vec!["agent", "task"],
            prompts,
        ),
    ]
}

pub fn claim(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;
    let force = get_bool(&args, "force").unwrap_or(false);

    let task = if force {
        db.force_claim_task(&task_id, &agent_id)?
    } else {
        db.claim_task(&task_id, &agent_id)?
    };

    Ok(json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status.as_str(),
            "owner_agent": task.owner_agent,
            "claimed_at": task.claimed_at
        }
    }))
}

pub fn release(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;
    let state = get_string(&args, "state")
        .and_then(|s| TaskStatus::from_str(&s))
        .unwrap_or(TaskStatus::Pending);

    db.release_task_with_state(&task_id, &agent_id, state)?;

    Ok(json!({
        "success": true
    }))
}

pub fn complete(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;

    let task = db.complete_task(&task_id, &agent_id)?;

    Ok(json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status.as_str(),
            "completed_at": task.completed_at
        }
    }))
}
