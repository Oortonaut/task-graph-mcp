//! Live status and tracking tools.

use super::{get_f64, get_i64, get_string, get_uuid, get_uuid_array, make_tool};
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "set_thought",
            "Set the current thought for tasks owned by an agent.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent UUID"
                },
                "thought": {
                    "type": "string",
                    "description": "What the agent is currently doing"
                },
                "task_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific task UUIDs to update (default: all claimed tasks)"
                }
            }),
            vec!["agent_id"],
        ),
        make_tool(
            "log_time",
            "Log time spent on a task.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                },
                "duration_ms": {
                    "type": "integer",
                    "description": "Duration in milliseconds to add"
                }
            }),
            vec!["task_id", "duration_ms"],
        ),
        make_tool(
            "log_cost",
            "Log token usage and cost for a task.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
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
            vec!["task_id"],
        ),
    ]
}

pub fn set_thought(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;
    let thought = get_string(&args, "thought");
    let task_ids = get_uuid_array(&args, "task_ids");

    let updated = db.set_thought(&agent_id, thought, task_ids)?;

    Ok(json!({
        "success": true,
        "updated_count": updated
    }))
}

pub fn log_time(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let duration_ms = get_i64(&args, "duration_ms")
        .ok_or_else(|| anyhow::anyhow!("duration_ms is required"))?;

    let total = db.log_time(task_id, duration_ms)?;

    Ok(json!({
        "success": true,
        "time_actual_ms": total
    }))
}

pub fn log_cost(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;

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
        task_id,
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
