//! Live status and tracking tools.

use super::{get_f64, get_i64, get_string, get_string_array, make_tool_with_prompts};
use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "thinking",
            "Broadcast real-time status updates (what you're doing right now). Also refreshes heartbeat. Call frequently during work to show live progress.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "thought": {
                    "type": "string",
                    "description": "What the agent is currently doing"
                },
                "tasks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific task IDs to update (default: all claimed tasks)"
                }
            }),
            vec!["agent", "thought"],
            prompts,
        ),
        make_tool_with_prompts(
            "get_state_history",
            "Get the state transition history for a task, including automatic time tracking data.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                }
            }),
            vec!["task"],
            prompts,
        ),
        make_tool_with_prompts(
            "log_cost",
            "Log token usage and cost for a task.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "tokens_in": {
                    "type": "integer",
                    "description": "Input tokens to add"
                },
                "tokens_cached": {
                    "type": "integer",
                    "description": "Cache hit tokens to add"
                },
                "tokens_out": {
                    "type": "integer",
                    "description": "Output tokens to add"
                },
                "tokens_thinking": {
                    "type": "integer",
                    "description": "Extended thinking tokens to add"
                },
                "tokens_image": {
                    "type": "integer",
                    "description": "Image tokens to add"
                },
                "tokens_audio": {
                    "type": "integer",
                    "description": "Audio tokens to add"
                },
                "cost_usd": {
                    "type": "number",
                    "description": "Cost in USD to add"
                },
                "user_metrics": {
                    "type": "object",
                    "description": "Custom metrics to merge"
                }
            }),
            vec!["agent", "task"],
            prompts,
        ),
    ]
}

pub fn thinking(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let thought = get_string(&args, "thought")
        .ok_or_else(|| ToolError::missing_field("thought"))?;
    let task_ids = get_string_array(&args, "tasks");

    // Also refresh heartbeat since updating thought implies activity
    let _ = db.heartbeat(&agent_id);

    let updated = db.set_thought(&agent_id, Some(thought), task_ids)?;

    Ok(json!({
        "success": true,
        "updated_count": updated
    }))
}

pub fn get_state_history(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;

    let history = db.get_task_state_history(&task_id)?;
    let current_duration = db.get_current_state_duration(&task_id, states_config)?;

    Ok(json!({
        "history": history,
        "current_duration_ms": current_duration
    }))
}

pub fn log_cost(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;

    let tokens_in = get_i64(&args, "tokens_in");
    let tokens_cached = get_i64(&args, "tokens_cached");
    let tokens_out = get_i64(&args, "tokens_out");
    let tokens_thinking = get_i64(&args, "tokens_thinking");
    let tokens_image = get_i64(&args, "tokens_image");
    let tokens_audio = get_i64(&args, "tokens_audio");
    let cost_usd = get_f64(&args, "cost_usd");

    let user_metrics: Option<HashMap<String, serde_json::Value>> = args
        .get("user_metrics")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let task = db.log_cost(
        &task_id,
        tokens_in,
        tokens_cached,
        tokens_out,
        tokens_thinking,
        tokens_image,
        tokens_audio,
        cost_usd,
        user_metrics,
    )?;

    Ok(json!({
        "success": true,
        "tokens_in": task.tokens_in,
        "tokens_cached": task.tokens_cached,
        "tokens_out": task.tokens_out,
        "tokens_thinking": task.tokens_thinking,
        "tokens_image": task.tokens_image,
        "tokens_audio": task.tokens_audio,
        "cost_usd": task.cost_usd
    }))
}
