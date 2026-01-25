//! File coordination tools (advisory locking).

use super::{get_i64, get_string, get_string_array, get_string_or_array, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

/// Format milliseconds as human-readable duration (e.g., "5m 30s", "2h 15m")
fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        return format!("{}ms", ms);
    }
    let secs = ms / 1000;
    if secs < 60 {
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        let rem_secs = secs % 60;
        return if rem_secs > 0 {
            format!("{}m {}s", mins, rem_secs)
        } else {
            format!("{}m", mins)
        };
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if rem_mins > 0 {
        format!("{}h {}m", hours, rem_mins)
    } else {
        format!("{}h", hours)
    }
}

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "claim_file",
            "Claim advisory lock on a file. Use for coordination - signals intent to work on a file. Returns warning if another worker holds the lock. Track changes via claim_updates.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID"
                },
                "file": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Relative file path or array of file paths"
                },
                "task": {
                    "type": "string",
                    "description": "Optional task ID to associate with the lock (for auto-cleanup when task completes)"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for claiming (visible to other workers)"
                }
            }),
            vec!["worker_id", "file"],
            prompts,
        ),
        make_tool_with_prompts(
            "release_file",
            "Release advisory lock on a file. Optionally include a reason for the next worker.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID"
                },
                "file": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Relative file path, array of paths, or '*' to release all files held by this worker"
                },
                "task": {
                    "type": "string",
                    "description": "Optional task ID - release all files associated with this task"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason/note for next claimant"
                }
            }),
            vec!["worker_id"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_files",
            "Get current file locks. Requires at least one filter: worker_id, task, or files.",
            json!({
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific file paths to check"
                },
                "worker_id": {
                    "type": "string",
                    "description": "Filter by worker ID"
                },
                "task": {
                    "type": "string",
                    "description": "Filter by task ID"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "markdown"],
                    "description": "Output format (default: json)"
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "claim_updates",
            "Poll for file claim changes since last call. Returns new claims and releases. Use for coordination between workers.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID (tracks poll position)"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter to specific files (optional, omit for all)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max time to wait for updates in milliseconds (long-polling). If 0 or omitted, returns immediately."
                }
            }),
            vec!["worker_id"],
            prompts,
        ),
    ]
}

pub fn claim_file(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let file_paths = get_string_or_array(&args, "file")
        .ok_or_else(|| ToolError::missing_field("file"))?;
    let task_id = get_string(&args, "task");
    let reason = get_string(&args, "reason");

    let mut results = Vec::new();
    let mut warnings = Vec::new();

    for file_path in &file_paths {
        let warning = db.lock_file(file_path.clone(), &worker_id, reason.clone(), task_id.clone())?;

        if let Some(other_agent) = warning {
            warnings.push(json!({
                "file": file_path,
                "locked_by": other_agent
            }));
        }
        results.push(file_path.clone());
    }

    let mut response = json!({
        "success": true,
        "claimed": results
    });

    if !warnings.is_empty() {
        response["warnings"] = json!(warnings);
    }

    Ok(response)
}

pub fn release_file(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let reason = get_string(&args, "reason");
    let task_id = get_string(&args, "task");

    // If task_id is provided, release all files for that task
    if let Some(tid) = task_id {
        let released = db.release_task_locks_verbose(&tid, reason)?;
        return Ok(json!({
            "success": true,
            "released": released.iter().map(|(f, w)| json!({
                "file": f,
                "worker_id": w
            })).collect::<Vec<_>>(),
            "count": released.len()
        }));
    }

    // Get file parameter - can be string, array, or '*'
    let file_param = get_string_or_array(&args, "file");

    match file_param {
        Some(files) if files.len() == 1 && files[0] == "*" => {
            // Wildcard: release all files held by this worker
            let released = db.release_worker_locks_verbose(&worker_id, reason)?;
            Ok(json!({
                "success": true,
                "released": released.iter().map(|(f, w)| json!({
                    "file": f,
                    "worker_id": w
                })).collect::<Vec<_>>(),
                "count": released.len()
            }))
        }
        Some(files) => {
            // Specific files: release each one
            let released = db.unlock_files_verbose(files, &worker_id, reason)?;
            Ok(json!({
                "success": true,
                "released": released.iter().map(|(f, w)| json!({
                    "file": f,
                    "worker_id": w
                })).collect::<Vec<_>>(),
                "count": released.len()
            }))
        }
        None => {
            // No file specified and no task - error
            Err(ToolError::missing_field("file or task").into())
        }
    }
}

pub fn list_files(db: &Database, args: Value) -> Result<Value> {
    let files = get_string_array(&args, "files");
    let worker_id = get_string(&args, "worker_id");
    let task_id = get_string(&args, "task");
    let format = get_string(&args, "format").unwrap_or_else(|| "json".to_string());

    // Require at least one filter
    if files.is_none() && worker_id.is_none() && task_id.is_none() {
        return Err(ToolError::invalid_value(
            "filter",
            "At least one filter required: worker_id, task, or files"
        ).into());
    }

    let locks = db.get_file_locks(files, worker_id.as_deref(), task_id.as_deref())?;
    let now = crate::db::now_ms();

    if format == "markdown" {
        let mut md = String::from("# File Locks\n\n");
        if locks.is_empty() {
            md.push_str("No locks found.\n");
        } else {
            md.push_str("| File | Worker | Task | Reason | Age |\n");
            md.push_str("|------|--------|------|--------|-----|\n");
            for (path, lock) in &locks {
                let age_ms = now - lock.locked_at;
                let age_str = format_duration(age_ms);
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    path,
                    lock.worker_id,
                    lock.task_id.as_deref().unwrap_or("-"),
                    lock.reason.as_deref().unwrap_or("-"),
                    age_str
                ));
            }
        }
        Ok(json!({ "markdown": md }))
    } else {
        let locks_json: Vec<Value> = locks
            .into_iter()
            .map(|(path, lock)| {
                let age_ms = now - lock.locked_at;
                json!({
                    "file": path,
                    "worker_id": lock.worker_id,
                    "task_id": lock.task_id,
                    "reason": lock.reason,
                    "locked_at": lock.locked_at,
                    "lock_age_ms": age_ms
                })
            })
            .collect();

        Ok(json!({ "locks": locks_json }))
    }
}

pub fn claim_updates(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let files = get_string_array(&args, "files");
    let timeout_ms = get_i64(&args, "timeout");

    let updates = db.claim_updates(&worker_id, files, timeout_ms)?;

    Ok(json!({
        "new_claims": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "worker": e.worker_id,
            "reason": e.reason,
            "claimed_at": e.timestamp
        })).collect::<Vec<_>>(),
        "dropped_claims": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "worker": e.worker_id,
            "reason": e.reason,
            "dropped_at": e.timestamp
        })).collect::<Vec<_>>(),
        "sequence": updates.sequence
    }))
}
