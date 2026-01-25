//! Worker connection and management tools.

use super::{get_bool, get_i32, get_string, get_string_array, make_tool_with_prompts};
use crate::config::{Prompts, ServerPaths, StatesConfig};
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
                },
                "db_path": {
                    "type": "string",
                    "description": "Override database file path (same as TASK_GRAPH_DB_PATH env var). Note: Can only be set before server starts."
                },
                "media_dir": {
                    "type": "string",
                    "description": "Override media directory path (same as TASK_GRAPH_MEDIA_DIR env var). Note: Can only be set before server starts."
                },
                "log_dir": {
                    "type": "string",
                    "description": "Override log directory path (same as TASK_GRAPH_LOG_DIR env var). Note: Can only be set before server starts."
                },
                "config_path": {
                    "type": "string",
                    "description": "Override config file path (same as TASK_GRAPH_CONFIG_PATH env var). Note: Can only be set before server starts."
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
                "final_status": {
                    "type": "string",
                    "enum": ["pending", "completed", "cancelled", "failed"],
                    "description": "Status to set released tasks to (default: config disconnect_status, typically 'pending'). Must be an untimed status."
                }
            }),
            vec!["worker_id"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_agents",
            "List all connected workers with their current status, claim counts, and what they're working on. Automatically evicts stale workers (no heartbeat within timeout).",
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
                },
                "stale_timeout": {
                    "type": "integer",
                    "description": "Seconds without heartbeat before a worker is considered stale and evicted. Set to 0 to disable auto-cleanup. Default: 300 (5 minutes)."
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "cleanup_stale",
            "Evict stale workers that haven't sent a heartbeat within the timeout period. Releases their task claims and file locks.",
            json!({
                "timeout": {
                    "type": "integer",
                    "description": "Seconds without heartbeat before a worker is considered stale. Default: 300 (5 minutes)."
                },
                "final_status": {
                    "type": "string",
                    "enum": ["pending", "completed", "cancelled", "failed"],
                    "description": "Status to set released tasks to (default: config disconnect_status, typically 'pending'). Must be an untimed status."
                }
            }),
            vec![],
            prompts,
        ),
    ]
}

pub fn connect(db: &Database, server_paths: &ServerPaths, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id");
    let tags = get_string_array(&args, "tags").unwrap_or_default();
    let force = get_bool(&args, "force").unwrap_or(false);

    // Check for path override requests (informational - paths are set at server startup)
    let mut path_notes: Vec<String> = Vec::new();

    if let Some(requested_db) = get_string(&args, "db_path") {
        if server_paths.db_path.to_string_lossy() != requested_db {
            path_notes.push(format!(
                "db_path: requested '{}' but server is using '{}' (set TASK_GRAPH_DB_PATH before starting server)",
                requested_db,
                server_paths.db_path.display()
            ));
        }
    }

    if let Some(requested_media) = get_string(&args, "media_dir") {
        if server_paths.media_dir.to_string_lossy() != requested_media {
            path_notes.push(format!(
                "media_dir: requested '{}' but server is using '{}' (set TASK_GRAPH_MEDIA_DIR before starting server)",
                requested_media,
                server_paths.media_dir.display()
            ));
        }
    }

    if let Some(requested_log) = get_string(&args, "log_dir") {
        if server_paths.log_dir.to_string_lossy() != requested_log {
            path_notes.push(format!(
                "log_dir: requested '{}' but server is using '{}' (set TASK_GRAPH_LOG_DIR before starting server)",
                requested_log,
                server_paths.log_dir.display()
            ));
        }
    }

    if let Some(requested_config) = get_string(&args, "config_path") {
        let current_config = server_paths.config_path.as_ref().map(|p| p.to_string_lossy().to_string());
        if current_config.as_deref() != Some(&requested_config) {
            path_notes.push(format!(
                "config_path: requested '{}' but server is using '{}' (set TASK_GRAPH_CONFIG_PATH before starting server)",
                requested_config,
                current_config.unwrap_or_else(|| "default locations".to_string())
            ));
        }
    }

    let worker = db.register_worker(worker_id, tags, force)?;

    let mut response = json!({
        "worker_id": &worker.id,
        "tags": worker.tags,
        "max_claims": worker.max_claims,
        "registered_at": worker.registered_at,
        "paths": {
            "db_path": server_paths.db_path.to_string_lossy(),
            "media_dir": server_paths.media_dir.to_string_lossy(),
            "log_dir": server_paths.log_dir.to_string_lossy(),
            "config_path": server_paths.config_path.as_ref().map(|p| p.to_string_lossy().to_string())
        }
    });

    if !path_notes.is_empty() {
        response["path_warnings"] = json!(path_notes);
    }

    Ok(response)
}

pub fn disconnect(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;

    // Get final_status from args or fall back to config
    let final_status = get_string(&args, "final_status")
        .unwrap_or_else(|| states_config.disconnect_state.clone());

    // Validate final_status is untimed
    if states_config.is_timed_state(&final_status) {
        return Err(ToolError::invalid_value(
            "final_status",
            &format!("must be an untimed status, got '{}'. Valid statuses: {:?}", 
                final_status, 
                states_config.untimed_state_names())
        ).into());
    }

    // Release worker locks before unregistering (close claim_sequence records)
    let _ = db.release_worker_locks(&worker_id);

    // Unregister and get summary
    let summary = db.unregister_worker(&worker_id, &final_status)?;

    Ok(json!({
        "success": true,
        "tasks_released": summary.tasks_released,
        "files_released": summary.files_released,
        "final_status": summary.final_status
    }))
}

pub fn list_agents(db: &Database, states_config: &StatesConfig, format: OutputFormat, args: Value) -> Result<ToolResult> {
    // Extract filter parameters
    let tags = get_string_array(&args, "tags");
    let file = get_string(&args, "file");
    let task = get_string(&args, "task");
    let depth = get_i32(&args, "depth").unwrap_or(0).clamp(-3, 3);
    
    // Auto-cleanup stale workers (default 5 minutes, 0 to disable)
    let stale_timeout = get_i32(&args, "stale_timeout").unwrap_or(300);
    let cleanup_summary = if stale_timeout > 0 {
        let final_status = states_config.disconnect_state.clone();
        db.cleanup_stale_workers(stale_timeout as i64, &final_status).ok()
    } else {
        None
    };

    // Get workers with filters
    let workers = db.list_workers_filtered(tags.as_ref(), file.as_deref(), task.as_deref(), depth)?;

    // Get current time for heartbeat age calculation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    match format {
        OutputFormat::Markdown => {
            let mut output = String::new();
            if let Some(ref summary) = cleanup_summary {
                if summary.workers_evicted > 0 {
                    output.push_str(&format!(
                        "**Evicted {} stale worker(s)**: {} (released {} task(s), {} file(s))\n\n",
                        summary.workers_evicted,
                        summary.evicted_worker_ids.join(", "),
                        summary.tasks_released,
                        summary.files_released
                    ));
                }
            }
            output.push_str(&format_workers_markdown(&workers));
            Ok(ToolResult::Raw(output))
        }
        OutputFormat::Json => {
            let mut result = json!({
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
            });
            
            if let Some(summary) = cleanup_summary {
                if summary.workers_evicted > 0 {
                    result["cleanup"] = json!({
                        "workers_evicted": summary.workers_evicted,
                        "evicted_worker_ids": summary.evicted_worker_ids,
                        "tasks_released": summary.tasks_released,
                        "files_released": summary.files_released
                    });
                }
            }
            
            Ok(ToolResult::Json(result))
        }
    }
}


pub fn cleanup_stale(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    // Default timeout: 5 minutes
    let timeout = get_i32(&args, "timeout").unwrap_or(300) as i64;
    
    // Get final_status from args or fall back to config
    let final_status = get_string(&args, "final_status")
        .unwrap_or_else(|| states_config.disconnect_state.clone());

    // Validate final_status is untimed
    if states_config.is_timed_state(&final_status) {
        return Err(ToolError::invalid_value(
            "final_status",
            &format!("must be an untimed status, got '{}'. Valid statuses: {:?}", 
                final_status, 
                states_config.untimed_state_names())
        ).into());
    }

    let summary = db.cleanup_stale_workers(timeout, &final_status)?;

    Ok(json!({
        "workers_evicted": summary.workers_evicted,
        "evicted_worker_ids": summary.evicted_worker_ids,
        "tasks_released": summary.tasks_released,
        "files_released": summary.files_released,
        "final_status": summary.final_status
    }))
}
