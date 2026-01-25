//! Task claiming tools.
//!
//! The `claim` tool is a convenience wrapper around `update` that transitions
//! a task to the first timed state. For releasing tasks, use `update` with
//! a non-timed state (ownership clears automatically).

use super::{get_bool, get_string, make_tool_with_prompts};
use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts, _states_config: &StatesConfig) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "claim",
            "Commit to working on a task (like adding to a changelist). Fails if: already claimed, deps unsatisfied, worker at max_claims limit, or worker lacks required tags. Sets status to timed (working) state.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID claiming the task"
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
            vec!["worker_id", "task"],
            prompts,
        ),
    ]
}

pub fn claim(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let force = get_bool(&args, "force").unwrap_or(false);

    // Find the first timed state to use for claiming
    let claim_status = states_config
        .definitions
        .iter()
        .find(|(_, def)| def.timed)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "in_progress".to_string());

    // Use unified update which handles claiming when transitioning to timed state
    let task = db.update_task_unified(
        &task_id,
        &worker_id,
        None,             // title
        None,             // description
        Some(claim_status), // status - first timed state
        None,             // priority
        None,             // points
        None,             // tags
        force,
        states_config,
    )?;

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
