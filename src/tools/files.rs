//! File coordination tools (advisory locking).

use super::{get_string, get_string_array, get_string_or_array, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{markdown_to_json, OutputFormat};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// Normalize a file path to an absolute, canonical form.
///
/// This function:
/// 1. Resolves relative paths against the current working directory
/// 2. Normalizes path components (removes `.`, resolves `..`)
/// 3. Uses forward slashes for consistency across platforms
/// 4. Works with non-existent files (doesn't require file to exist)
///
/// # Examples
/// - `src/main.rs` -> `/project/src/main.rs`
/// - `./src/../src/main.rs` -> `/project/src/main.rs`
/// - `/absolute/path.rs` -> `/absolute/path.rs`
fn normalize_file_path(path: &str) -> String {
    let path = Path::new(path);

    // Get absolute path
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        // Resolve relative to current directory
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    // Normalize the path (resolve . and ..)
    let normalized = normalize_path_components(&absolute);

    // Convert to string with forward slashes for consistency
    path_to_forward_slashes(&normalized)
}

/// Normalize path components without requiring the file to exist.
/// Handles `.` and `..` components.
fn normalize_path_components(path: &Path) -> PathBuf {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                // Windows drive prefix (e.g., C:)
                components.push(Component::Prefix(p));
            }
            Component::RootDir => {
                components.push(Component::RootDir);
            }
            Component::CurDir => {
                // Skip `.` - it refers to current directory
            }
            Component::ParentDir => {
                // Go up one directory if possible
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                } else {
                    // Can't go up from root, keep the component
                    // (this handles edge cases like `/../foo`)
                    components.push(Component::ParentDir);
                }
            }
            Component::Normal(name) => {
                components.push(Component::Normal(name));
            }
        }
    }

    components.iter().collect()
}

/// Convert path to string using forward slashes.
fn path_to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Normalize a vector of file paths.
fn normalize_file_paths(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().map(|p| normalize_file_path(&p)).collect()
}

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
            "Claim advisory lock on a file. Use for coordination - signals intent to work on a file. Returns warning if another agent holds the lock. Track changes via claim_updates.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
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
                    "description": "Optional reason for claiming (visible to other agents)"
                }
            }),
            vec!["agent", "file"],
            prompts,
        ),
        make_tool_with_prompts(
            "release_file",
            "Release advisory lock on a file. Optionally include a reason for the next agent.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "file": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Relative file path, array of paths, or '*' to release all files held by this agent"
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
            vec!["agent"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_files",
            "Get current file locks. Requires at least one filter: agent, task, or files.",
            json!({
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific file paths to check"
                },
                "agent": {
                    "type": "string",
                    "description": "Filter by agent ID"
                },
                "task": {
                    "type": "string",
                    "description": "Filter by task ID"
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "claim_updates",
            "Poll for file claim changes since last call. Returns new claims and releases. Use for coordination between agents.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID (tracks poll position)"
                }
            }),
            vec!["agent"],
            prompts,
        ),
    ]
}

pub fn claim_file(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let file_paths = get_string_or_array(&args, "file")
        .ok_or_else(|| ToolError::missing_field("file"))?;
    let task_id = get_string(&args, "task");
    let reason = get_string(&args, "reason");

    // Normalize all file paths to absolute canonical form
    let normalized_paths = normalize_file_paths(file_paths);

    let mut results = Vec::new();
    let mut warnings = Vec::new();

    for file_path in &normalized_paths {
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
    let worker_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;
    let reason = get_string(&args, "reason");
    let task_id = get_string(&args, "task");

    // If task_id is provided, release all files for that task
    if let Some(tid) = task_id {
        let released = db.release_task_locks_verbose(&tid, reason)?;
        return Ok(json!({
            "success": true,
            "released": released.iter().map(|(f, w)| json!({
                "file": f,
                "agent": w
            })).collect::<Vec<_>>(),
            "count": released.len()
        }));
    }

    // Get file parameter - can be string, array, or '*'
    let file_param = get_string_or_array(&args, "file");

    match file_param {
        Some(files) if files.len() == 1 && files[0] == "*" => {
            // Wildcard: release all files held by this agent
            let released = db.release_worker_locks_verbose(&worker_id, reason)?;
            Ok(json!({
                "success": true,
                "released": released.iter().map(|(f, w)| json!({
                    "file": f,
                    "agent": w
                })).collect::<Vec<_>>(),
                "count": released.len()
            }))
        }
        Some(files) => {
            // Normalize the file paths before releasing
            let normalized_files = normalize_file_paths(files);
            // Specific files: release each one
            let released = db.unlock_files_verbose(normalized_files, &worker_id, reason)?;
            Ok(json!({
                "success": true,
                "released": released.iter().map(|(f, w)| json!({
                    "file": f,
                    "agent": w
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

pub fn list_files(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let files = get_string_array(&args, "files");
    let worker_id = get_string(&args, "agent");
    let task_id = get_string(&args, "task");
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(default_format);

    // Require at least one filter
    if files.is_none() && worker_id.is_none() && task_id.is_none() {
        return Err(ToolError::invalid_value(
            "filter",
            "At least one filter required: agent, task, or files"
        ).into());
    }

    // Normalize file paths in the filter if provided
    let normalized_files = files.map(normalize_file_paths);

    let locks = db.get_file_locks(normalized_files, worker_id.as_deref(), task_id.as_deref())?;
    let now = crate::db::now_ms();

    match format {
        OutputFormat::Markdown => {
            let mut md = String::from("# File Locks\n\n");
            if locks.is_empty() {
                md.push_str("No locks found.\n");
            } else {
                md.push_str("| File | Agent | Task | Reason | Age |\n");
                md.push_str("|------|-------|------|--------|-----|\n");
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
            Ok(markdown_to_json(md))
        }
        OutputFormat::Json => {
            let locks_json: Vec<Value> = locks
                .into_iter()
                .map(|(path, lock)| {
                    let age_ms = now - lock.locked_at;
                    json!({
                        "file": path,
                        "agent": lock.worker_id,
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
}

/// Async version of claim_updates.
pub async fn claim_updates_async(db: std::sync::Arc<Database>, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;

    // Run on blocking thread pool since db operations are synchronous
    let updates = tokio::task::spawn_blocking(move || {
        db.claim_updates(&worker_id)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    Ok(json!({
        "new_claims": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "claimed_at": e.timestamp
        })).collect::<Vec<_>>(),
        "dropped_claims": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "dropped_at": e.timestamp
        })).collect::<Vec<_>>(),
        "sequence": updates.sequence
    }))
}

/// Synchronous version of claim_updates.
pub fn claim_updates(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;

    let updates = db.claim_updates(&worker_id)?;

    Ok(json!({
        "new_claims": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "claimed_at": e.timestamp
        })).collect::<Vec<_>>(),
        "dropped_claims": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "dropped_at": e.timestamp
        })).collect::<Vec<_>>(),
        "sequence": updates.sequence
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_components() {
        // Test removing current directory markers
        let path = Path::new("/foo/./bar/./baz");
        let normalized = normalize_path_components(path);
        assert_eq!(path_to_forward_slashes(&normalized), "/foo/bar/baz");

        // Test resolving parent directory markers
        let path = Path::new("/foo/bar/../baz");
        let normalized = normalize_path_components(path);
        assert_eq!(path_to_forward_slashes(&normalized), "/foo/baz");

        // Test complex case
        let path = Path::new("/foo/bar/./baz/../qux");
        let normalized = normalize_path_components(path);
        assert_eq!(path_to_forward_slashes(&normalized), "/foo/bar/qux");
    }

    #[test]
    fn test_path_to_forward_slashes() {
        // Test Windows-style path
        let path = Path::new("C:\\foo\\bar\\baz");
        assert_eq!(path_to_forward_slashes(path), "C:/foo/bar/baz");

        // Test Unix-style path (no change)
        let path = Path::new("/foo/bar/baz");
        assert_eq!(path_to_forward_slashes(path), "/foo/bar/baz");
    }

    #[test]
    fn test_normalize_file_paths() {
        // Test that normalization is applied to all paths in a vector
        let paths = vec![
            "src/main.rs".to_string(),
            "./src/lib.rs".to_string(),
        ];
        let normalized = normalize_file_paths(paths);

        // All paths should be absolute (start with / or drive letter on Windows)
        for path in &normalized {
            assert!(
                path.starts_with('/') || (path.len() > 2 && path.chars().nth(1) == Some(':')),
                "Path should be absolute: {}",
                path
            );
        }
    }
}
