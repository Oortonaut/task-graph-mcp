//! Worker connection and management tools.

use super::{get_bool, get_i32, get_string, get_string_array, make_tool_with_prompts};
use crate::config::workflows::WorkflowsConfig;
use crate::config::{AppConfig, Prompts, ServerPaths, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{OutputFormat, ToolResult, format_workers_markdown};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Options for connecting a worker to the task graph.
pub struct ConnectOptions<'a> {
    pub db: &'a Database,
    pub server_paths: &'a ServerPaths,
    pub config: &'a AppConfig,
    /// Per-connect workflow (may differ from config.workflows for named workflows).
    pub workflows: &'a WorkflowsConfig,
}

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "connect",
            "Connect as a worker. Call this FIRST before using other tools. Returns worker_id (save it for all subsequent calls). Tags enable task affinity matching.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Only use if assigned a unique name (e.g., 'worker-17', 'coordinator'). Avoid generic names like 'claude'. Leave empty for an auto-generated petname."
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
                },
                "workflow": {
                    "type": "string",
                    "description": "Named workflow to use (e.g., 'swarm' for workflow-swarm.yaml). If not specified, uses default workflows.yaml."
                },
                "overlays": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Overlay names to apply on top of the workflow, in order (e.g., ['git', 'user-request']). Use list_workflows to see available overlays."
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
        make_tool_with_prompts(
            "add_overlay",
            "Dynamically add an overlay to a connected worker's active overlay stack. The overlay is applied on top of existing overlays and persisted.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "The worker's ID"
                },
                "overlay": {
                    "type": "string",
                    "description": "Name of the overlay to add (e.g., 'git', 'troubleshooting'). Use list_workflows to see available overlays."
                }
            }),
            vec!["worker_id", "overlay"],
            prompts,
        ),
        make_tool_with_prompts(
            "remove_overlay",
            "Dynamically remove an overlay from a connected worker's active overlay stack. The change is persisted immediately.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "The worker's ID"
                },
                "overlay": {
                    "type": "string",
                    "description": "Name of the overlay to remove (must be currently active on this worker)"
                }
            }),
            vec!["worker_id", "overlay"],
            prompts,
        ),
    ]
}

pub fn connect(opts: ConnectOptions<'_>, args: Value) -> Result<Value> {
    let ConnectOptions {
        db,
        server_paths,
        config,
        workflows,
    } = opts;

    let states_config = &config.states;
    let phases_config = &config.phases;
    let deps_config = &config.deps;
    let tags_config = &config.tags;
    let ids_config = &config.ids;

    let worker_id = get_string(&args, "worker_id");
    let tags = get_string_array(&args, "tags").unwrap_or_default();
    let force = get_bool(&args, "force").unwrap_or(false);
    let workflow = get_string(&args, "workflow");

    // Validate tags if provided
    let tag_warnings = tags_config.validate_tags(&tags)?;

    // Check for path override requests (informational - paths are set at server startup)
    let mut path_notes: Vec<String> = Vec::new();

    if let Some(requested_db) = get_string(&args, "db_path")
        && server_paths.db_path.to_string_lossy() != requested_db
    {
        path_notes.push(format!(
                "db_path: requested '{}' but server is using '{}' (set TASK_GRAPH_DB_PATH before starting server)",
                requested_db,
                server_paths.db_path.display()
            ));
    }

    if let Some(requested_media) = get_string(&args, "media_dir")
        && server_paths.media_dir.to_string_lossy() != requested_media
    {
        path_notes.push(format!(
                "media_dir: requested '{}' but server is using '{}' (set TASK_GRAPH_MEDIA_DIR before starting server)",
                requested_media,
                server_paths.media_dir.display()
            ));
    }

    if let Some(requested_log) = get_string(&args, "log_dir")
        && server_paths.log_dir.to_string_lossy() != requested_log
    {
        path_notes.push(format!(
                "log_dir: requested '{}' but server is using '{}' (set TASK_GRAPH_LOG_DIR before starting server)",
                requested_log,
                server_paths.log_dir.display()
            ));
    }

    if let Some(requested_config) = get_string(&args, "config_path") {
        let current_config = server_paths
            .config_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        if current_config.as_deref() != Some(&requested_config) {
            path_notes.push(format!(
                "config_path: requested '{}' but server is using '{}' (set TASK_GRAPH_CONFIG_PATH before starting server)",
                requested_config,
                current_config.unwrap_or_else(|| "default locations".to_string())
            ));
        }
    }

    let overlays = get_string_array(&args, "overlays").unwrap_or_default();
    let worker = db.register_worker(worker_id, tags, force, ids_config, workflow, overlays)?;

    // Build config summary for the response
    let timed_states: Vec<&str> = states_config
        .definitions
        .iter()
        .filter(|(_, def)| def.timed)
        .map(|(name, _)| name.as_str())
        .collect();

    let terminal_states: Vec<&str> = states_config
        .definitions
        .iter()
        .filter(|(_, def)| def.exits.is_empty())
        .map(|(name, _)| name.as_str())
        .collect();

    let mut response = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "worker_id": &worker.id,
        "tags": worker.tags,
        "max_claims": worker.max_claims,
        "registered_at": worker.registered_at,
        "workflow": worker.workflow,
        "paths": {
            "db_path": server_paths.db_path.to_string_lossy(),
            "media_dir": server_paths.media_dir.to_string_lossy(),
            "log_dir": server_paths.log_dir.to_string_lossy(),
            "config_path": server_paths.config_path.as_ref().map(|p| p.to_string_lossy().to_string())
        },
        "config": {
            "states": states_config.state_names(),
            "initial_state": &states_config.initial,
            "timed_states": timed_states,
            "terminal_states": terminal_states,
            "blocking_states": &states_config.blocking_states,
            "phases": phases_config.phase_names(),
            "dependency_types": deps_config.dep_type_names(),
            "known_tags": tags_config.tag_names()
        }
    });

    if !path_notes.is_empty() {
        response["path_warnings"] = json!(path_notes);
    }

    if !tag_warnings.is_empty() {
        response["tag_warnings"] = json!(tag_warnings);
    }

    // Deliver workflow-specific role information and prompts
    if let Some(role_name) = workflows.match_role(&worker.tags) {
        let mut role_info = json!({
            "role": &role_name,
        });

        // Include role definition details
        if let Some(role_def) = workflows.get_role(&role_name) {
            if let Some(ref desc) = role_def.description {
                role_info["description"] = json!(desc);
            }
            if let Some(max) = role_def.max_claims {
                role_info["max_claims"] = json!(max);
            }
            if let Some(can_assign) = role_def.can_assign {
                role_info["can_assign"] = json!(can_assign);
            }
        }

        response["role"] = role_info;

        // Include role-specific prompts
        let prompts = workflows.get_role_prompts(&role_name);
        if !prompts.is_empty() {
            response["role_prompts"] = json!(prompts);
        }
    }

    // Include workflow description if available
    if let Some(ref desc) = workflows.description {
        response["workflow_description"] = json!(desc);
    }

    // Include overlay information if overlays were applied
    if !worker.overlays.is_empty() {
        response["overlays"] = json!(worker.overlays);
    }

    Ok(response)
}

pub fn disconnect(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;

    // Get final_status from args or fall back to config
    let final_status =
        get_string(&args, "final_status").unwrap_or_else(|| states_config.disconnect_state.clone());

    // Validate final_status is untimed
    if states_config.is_timed_state(&final_status) {
        return Err(ToolError::invalid_value(
            "final_status",
            &format!(
                "must be an untimed status, got '{}'. Valid statuses: {:?}",
                final_status,
                states_config.untimed_state_names()
            ),
        )
        .into());
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

pub fn list_agents(
    db: &Database,
    states_config: &StatesConfig,
    format: OutputFormat,
    args: Value,
) -> Result<ToolResult> {
    // Extract filter parameters
    let tags = get_string_array(&args, "tags");
    let file = get_string(&args, "file");
    let task = get_string(&args, "task");
    let depth = get_i32(&args, "depth").unwrap_or(0).clamp(-3, 3);

    // Auto-cleanup stale workers (default 5 minutes, 0 to disable)
    let stale_timeout = get_i32(&args, "stale_timeout").unwrap_or(300);
    let cleanup_summary = if stale_timeout > 0 {
        let final_status = states_config.disconnect_state.clone();
        db.cleanup_stale_workers(stale_timeout as i64, &final_status)
            .ok()
    } else {
        None
    };

    // Get workers with filters
    let workers =
        db.list_workers_filtered(tags.as_ref(), file.as_deref(), task.as_deref(), depth)?;

    // Get current time for heartbeat age calculation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    match format {
        OutputFormat::Markdown => {
            let mut output = String::new();
            if let Some(ref summary) = cleanup_summary
                && summary.workers_evicted > 0
            {
                output.push_str(&format!(
                    "**Evicted {} stale worker(s)**: {} (released {} task(s), {} file(s))\n\n",
                    summary.workers_evicted,
                    summary.evicted_worker_ids.join(", "),
                    summary.tasks_released,
                    summary.files_released
                ));
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
                    "heartbeat_age_ms": now - w.last_heartbeat,
                    "workflow": w.workflow
                })).collect::<Vec<_>>()
            });

            if let Some(summary) = cleanup_summary
                && summary.workers_evicted > 0
            {
                result["cleanup"] = json!({
                    "workers_evicted": summary.workers_evicted,
                    "evicted_worker_ids": summary.evicted_worker_ids,
                    "tasks_released": summary.tasks_released,
                    "files_released": summary.files_released
                });
            }

            Ok(ToolResult::Json(result))
        }
    }
}

pub fn cleanup_stale(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    // Default timeout: 5 minutes
    let timeout = get_i32(&args, "timeout").unwrap_or(300) as i64;

    // Get final_status from args or fall back to config
    let final_status =
        get_string(&args, "final_status").unwrap_or_else(|| states_config.disconnect_state.clone());

    // Validate final_status is untimed
    if states_config.is_timed_state(&final_status) {
        return Err(ToolError::invalid_value(
            "final_status",
            &format!(
                "must be an untimed status, got '{}'. Valid statuses: {:?}",
                final_status,
                states_config.untimed_state_names()
            ),
        )
        .into());
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

pub fn add_overlay(db: &Database, config: &AppConfig, args: Value) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let overlay_name =
        get_string(&args, "overlay").ok_or_else(|| ToolError::missing_field("overlay"))?;

    // Validate overlay exists in named_overlays
    if !config.workflows.named_overlays.contains_key(&overlay_name) {
        let available: Vec<&String> = config.workflows.named_overlays.keys().collect();
        return Err(ToolError::invalid_value(
            "overlay",
            &format!(
                "unknown overlay '{}'. Available overlays: {:?}",
                overlay_name, available
            ),
        )
        .into());
    }

    // Get current worker
    let worker = db
        .get_worker(&worker_id)?
        .ok_or_else(|| ToolError::agent_not_found(&worker_id))?;

    // Check not already active
    if worker.overlays.contains(&overlay_name) {
        return Err(ToolError::invalid_value(
            "overlay",
            &format!(
                "overlay '{}' is already active on worker '{}'",
                overlay_name, worker_id
            ),
        )
        .into());
    }

    // Build new overlays list (append)
    let mut new_overlays = worker.overlays.clone();
    new_overlays.push(overlay_name);

    // Persist
    let updated_worker = db.update_worker_overlays(&worker_id, new_overlays)?;

    // Build merged workflow to compute diff
    let base = resolve_base_workflow(&updated_worker, config);
    let mut merged = (*base).clone();
    for name in &updated_worker.overlays {
        if let Some(overlay) = config.workflows.named_overlays.get(name) {
            merged.apply_overlay(overlay);
        }
    }
    merged.active_overlays = updated_worker.overlays.clone();

    let overlay_diff = merged.compute_overlay_diff(&base);

    let mut response = json!({
        "success": true,
        "worker_id": updated_worker.id,
        "overlays": updated_worker.overlays,
        "overlay_diff": overlay_diff,
    });

    // Include role info if applicable
    if let Some(role_name) = merged.match_role(&updated_worker.tags) {
        response["role"] = json!(role_name);
        let prompts = merged.get_role_prompts(&role_name);
        if !prompts.is_empty() {
            response["role_prompts"] = json!(prompts);
        }
    }

    Ok(response)
}

pub fn remove_overlay(db: &Database, config: &AppConfig, args: Value) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let overlay_name =
        get_string(&args, "overlay").ok_or_else(|| ToolError::missing_field("overlay"))?;

    // Get current worker
    let worker = db
        .get_worker(&worker_id)?
        .ok_or_else(|| ToolError::agent_not_found(&worker_id))?;

    // Check overlay is currently active
    if !worker.overlays.contains(&overlay_name) {
        return Err(ToolError::invalid_value(
            "overlay",
            &format!(
                "overlay '{}' is not active on worker '{}'. Active overlays: {:?}",
                overlay_name, worker_id, worker.overlays
            ),
        )
        .into());
    }

    // Build new overlays list (remove)
    let new_overlays: Vec<String> = worker
        .overlays
        .into_iter()
        .filter(|o| o != &overlay_name)
        .collect();

    // Persist
    let updated_worker = db.update_worker_overlays(&worker_id, new_overlays)?;

    // Build merged workflow to compute diff
    let base = resolve_base_workflow(&updated_worker, config);
    let mut merged = (*base).clone();
    for name in &updated_worker.overlays {
        if let Some(overlay) = config.workflows.named_overlays.get(name) {
            merged.apply_overlay(overlay);
        }
    }
    merged.active_overlays = updated_worker.overlays.clone();

    let overlay_diff = merged.compute_overlay_diff(&base);

    let mut response = json!({
        "success": true,
        "worker_id": updated_worker.id,
        "overlays": updated_worker.overlays,
        "overlay_diff": overlay_diff,
    });

    // Include role info if applicable
    if let Some(role_name) = merged.match_role(&updated_worker.tags) {
        response["role"] = json!(role_name);
        let prompts = merged.get_role_prompts(&role_name);
        if !prompts.is_empty() {
            response["role_prompts"] = json!(prompts);
        }
    }

    Ok(response)
}

/// Resolve the base workflow for a worker (before overlays).
fn resolve_base_workflow(
    worker: &crate::types::Worker,
    config: &AppConfig,
) -> std::sync::Arc<WorkflowsConfig> {
    if let Some(ref workflow_name) = worker.workflow {
        config
            .workflows
            .get_named_workflow(workflow_name)
            .map(std::sync::Arc::clone)
    } else {
        None
    }
    .or_else(|| {
        config
            .workflows
            .get_default_workflow()
            .map(std::sync::Arc::clone)
    })
    .unwrap_or_else(|| std::sync::Arc::clone(&config.workflows))
}
