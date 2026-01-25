//! Worker connection and management tools.

use super::{get_bool, get_i32, get_string, get_string_array, make_tool_with_prompts};
use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{format_workers_markdown, OutputFormat, ToolResult};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "connect",
            "Connect as a worker. Call this FIRST before using other tools. Returns worker_id (save it for all subsequent calls). Tags enable task affinity matching.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Use a session ID, GUID, hash, or assigned name. Leave empty for a random human petname."
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Freeform tags for capabilities, roles, etc."
                },
                "force": {
                    "type": "boolean",
                    "description": "Force reconnection if worker ID already exists (default: false). Use for stuck worker recovery."
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "disconnect",
            "Disconnect a worker, releasing all claims and locks.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "The worker's ID"
                },
                "final_state": {
                    "type": "string",
                    "enum": ["pending", "completed", "cancelled", "failed"],
                    "description": "State to set released tasks to (default: config disconnect_state, typically 'pending'). Must be an untimed state."
                }
            }),
            vec!["worker_id"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_agents",
            "List all connected workers with their current status, claim counts, and what they're working on.",
            json!({
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter workers that have ALL of these tags"
                },
                "file": {
                    "type": "string",
                    "description": "Filter workers that have claimed this file"
                },
                "task": {
                    "type": "string",
                    "description": "Filter workers related to this task ID"
                },
                "depth": {
                    "type": "integer",
                    "description": "Task relationship depth (-3 to 3). Negative: ancestors, positive: descendants. Used with 'task' filter."
                }
            }),
            vec![],
            prompts,
        ),
    ]
}

pub fn connect(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id");
    let tags = get_string_array(&args, "tags").unwrap_or_default();
    let force = get_bool(&args, "force").unwrap_or(false);

    let worker = db.register_worker(worker_id, tags, force)?;

    Ok(json!({
        "worker_id": &worker.id,
        "tags": worker.tags,
        "max_claims": worker.max_claims,
        "registered_at": worker.registered_at
    }))
}

pub fn disconnect(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;

    // Get final_state from args or fall back to config
    let final_state = get_string(&args, "final_state")
        .unwrap_or_else(|| states_config.disconnect_state.clone());

    // Validate final_state is untimed
    if states_config.is_timed_state(&final_state) {
        return Err(ToolError::invalid_value(
            "final_state",
            &format!("must be an untimed state, got '{}'. Valid states: {:?}", 
                final_state, 
                states_config.untimed_state_names())
        ).into());
    }

    // Release worker locks before unregistering (close claim_sequence records)
    let _ = db.release_worker_locks(&worker_id);

    // Unregister and get summary
    let summary = db.unregister_worker(&worker_id, &final_state)?;

    Ok(json!({
        "success": true,
        "tasks_released": summary.tasks_released,
        "files_released": summary.files_released,
        "final_state": summary.final_state
    }))
}

pub fn list_agents(db: &Database, format: OutputFormat, args: Value) -> Result<ToolResult> {
    // Extract filter parameters
    let tags = get_string_array(&args, "tags");
    let file = get_string(&args, "file");
    let task = get_string(&args, "task");
    let depth = get_i32(&args, "depth").unwrap_or(0).clamp(-3, 3);

    // Get workers with filters
    let workers = db.list_workers_filtered(tags.as_ref(), file.as_deref(), task.as_deref(), depth)?;

    // Get current time for heartbeat age calculation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    match format {
        OutputFormat::Markdown => {
            Ok(ToolResult::Raw(format_workers_markdown(&workers)))
        }
        OutputFormat::Json => {
            Ok(ToolResult::Json(json!({
                "workers": workers.iter().map(|w| json!({
                    "id": w.id,
                    "tags": w.tags,
                    "max_claims": w.max_claims,
                    "claim_count": w.claim_count,
                    "current_thought": w.current_thought,
                    "registered_at": w.registered_at,
                    "last_heartbeat": w.last_heartbeat,
                    "heartbeat_age_ms": now - w.last_heartbeat
                })).collect::<Vec<_>>()
            })))
        }
    }
}
