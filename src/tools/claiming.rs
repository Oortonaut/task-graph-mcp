//! Task claiming and release tools.

use super::{get_bool, get_string, make_tool_with_prompts};
use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts, states_config: &StatesConfig) -> Vec<Tool> {
    // Generate state enum from config
    let state_names: Vec<&str> = states_config.state_names();

    // For release, find states that can be exited from timed states (typically in_progress)
    let release_states: Vec<Value> = state_names
        .iter()
        .filter(|s| {
            // Include initial state and all non-blocking states
            *s == &states_config.initial.as_str()
                || !states_config.is_blocking_state(s)
        })
        .map(|s| json!(s))
        .collect();

    vec![
        make_tool_with_prompts(
            "claim",
            "Commit to working on a task (like adding to a changelist). Fails if: already claimed, deps unsatisfied, agent at max_claims limit, or agent lacks required tags. Sets status to timed (working) state.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID claiming the task"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID to claim"
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
            "Release a claimed task. Use initial state for handoff to another agent, or a terminal state to end the task.",
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
                    "enum": release_states,
                    "description": format!("State to set after release (default: {})", states_config.initial)
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

pub fn claim(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let force = get_bool(&args, "force").unwrap_or(false);

    let task = if force {
        db.force_claim_task(&task_id, &agent_id, states_config)?
    } else {
        db.claim_task(&task_id, &agent_id, states_config)?
    };

    Ok(json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status,
            "owner_agent": task.owner_agent,
            "claimed_at": task.claimed_at
        }
    }))
}

pub fn release(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let state = get_string(&args, "state").unwrap_or_else(|| states_config.initial.clone());

    db.release_task_with_state(&task_id, &agent_id, &state, states_config)?;

    Ok(json!({
        "success": true
    }))
}

pub fn complete(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;

    let task = db.complete_task(&task_id, &agent_id, states_config)?;

    Ok(json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status,
            "completed_at": task.completed_at
        }
    }))
}
