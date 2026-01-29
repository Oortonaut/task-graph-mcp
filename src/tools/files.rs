//! File coordination tools (advisory marks and exclusive locks).
//!
//! Supports two modes:
//! - **Advisory marks** (default): warns if another agent holds a file mark.
//! - **Exclusive locks** (`lock:` prefix): rejects with error if another agent holds the lock.
//!
//! The `lock:` namespace uses the same `file_locks` table but enforces mutual exclusion.
//! Example: `mark_file(file="lock:git-commit")` acquires an exclusive lock on the
//! resource "git-commit". Another agent attempting the same lock will receive an error.

use super::{
    IdList, get_string, get_string_or_array, get_string_or_array_or_wildcard,
    make_tool_with_prompts,
};
use crate::config::Prompts;
use crate::db::Database;
use crate::db::locks::ExclusiveLockResult;
use crate::error::ToolError;
use crate::format::{OutputFormat, markdown_to_json};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

/// The prefix that triggers exclusive lock semantics.
const LOCK_PREFIX: &str = "lock:";

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
            "mark_file",
            "Mark a file to signal intent to work on it (advisory, non-blocking). Returns warning if another agent has marked the file. Track changes via mark_updates.\n\nUse the `lock:` prefix for exclusive locks: `lock:resource-name` will reject (not just warn) if another agent holds the lock. Example: `mark_file(file=\"lock:git-commit\")` acquires a mutual-exclusion lock on the resource \"git-commit\".",
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
                    "description": "Relative file path, array of file paths, or lock resource(s) with 'lock:' prefix (e.g. 'lock:git-commit' for exclusive locks)"
                },
                "task": {
                    "type": "string",
                    "description": "Optional task ID to associate with the mark (for auto-cleanup when task completes)"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for marking (visible to other agents)"
                }
            }),
            vec!["agent", "file"],
            prompts,
        ),
        make_tool_with_prompts(
            "unmark_file",
            "Remove mark from a file. Optionally include a note for the next agent.",
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
                    "description": "Relative file path, array of paths, or '*' to unmark all files held by this agent"
                },
                "task": {
                    "type": "string",
                    "description": "Optional task ID - unmark all files associated with this task"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason/note for next agent"
                }
            }),
            vec!["agent"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_marks",
            "Get current file marks. Requires at least one filter: agent, task, or files.",
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
            "mark_updates",
            "Poll for file mark changes since last call. Returns new marks and removals. Use for coordination between agents.",
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

pub fn mark_file(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent").ok_or_else(|| ToolError::missing_field("agent"))?;
    let file_paths =
        get_string_or_array(&args, "file").ok_or_else(|| ToolError::missing_field("file"))?;
    let task_id = get_string(&args, "task");
    let reason = get_string(&args, "reason");

    // Separate lock: prefixed paths from regular file paths
    let mut lock_paths: Vec<String> = Vec::new();
    let mut regular_paths: Vec<String> = Vec::new();

    for path in file_paths {
        if path.starts_with(LOCK_PREFIX) {
            // lock: namespace - store as-is (no path normalization)
            lock_paths.push(path);
        } else {
            regular_paths.push(path);
        }
    }

    // Normalize regular file paths to absolute canonical form
    let normalized_regular = normalize_file_paths(regular_paths);

    let mut results = Vec::new();
    let mut warnings = Vec::new();
    let mut locks_acquired = Vec::new();

    // Process exclusive locks first - fail fast on conflicts
    for lock_path in &lock_paths {
        let result = db.lock_file_exclusive(
            lock_path.clone(),
            &worker_id,
            reason.clone(),
            task_id.clone(),
        )?;

        match result {
            ExclusiveLockResult::HeldByOther(other_agent) => {
                // Exclusive lock conflict - return error immediately
                return Err(ToolError::lock_conflict(lock_path, &other_agent).into());
            }
            ExclusiveLockResult::Acquired => {
                locks_acquired.push(lock_path.clone());
            }
            ExclusiveLockResult::AlreadyHeldBySelf => {
                locks_acquired.push(lock_path.clone());
            }
        }
    }

    // Process advisory marks (existing behavior)
    for file_path in &normalized_regular {
        let warning = db.lock_file(
            file_path.clone(),
            &worker_id,
            reason.clone(),
            task_id.clone(),
        )?;

        if let Some(other_agent) = warning {
            warnings.push(json!({
                "file": file_path,
                "marked_by": other_agent
            }));
        }
        results.push(file_path.clone());
    }

    let mut response = json!({
        "success": true,
        "marked": results
    });

    if !locks_acquired.is_empty() {
        response["locks_acquired"] = json!(locks_acquired);
    }

    if !warnings.is_empty() {
        response["warnings"] = json!(warnings);
    }

    Ok(response)
}

pub fn unmark_file(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent").ok_or_else(|| ToolError::missing_field("agent"))?;
    let reason = get_string(&args, "reason");
    let task_id = get_string(&args, "task");

    // If task_id is provided, unmark all files for that task
    if let Some(tid) = task_id {
        let unmarked = db.release_task_locks_verbose(&tid, reason)?;
        return Ok(json!({
            "success": true,
            "unmarked": unmarked.iter().map(|(f, w)| json!({
                "file": f,
                "agent": w
            })).collect::<Vec<_>>(),
            "count": unmarked.len()
        }));
    }

    // Get file parameter - can be string, array, or '*'
    let file_param = get_string_or_array_or_wildcard(&args, "file");

    match file_param {
        Some(IdList::Wildcard) => {
            // Wildcard: unmark all files held by this agent
            let unmarked = db.release_worker_locks_verbose(&worker_id, reason)?;
            Ok(json!({
                "success": true,
                "unmarked": unmarked.iter().map(|(f, w)| json!({
                    "file": f,
                    "agent": w
                })).collect::<Vec<_>>(),
                "count": unmarked.len()
            }))
        }
        Some(IdList::Ids(files)) => {
            // Separate lock: paths (no normalization) from regular paths (normalize)
            let mut all_paths: Vec<String> = Vec::new();
            for f in files {
                if f.starts_with(LOCK_PREFIX) {
                    all_paths.push(f);
                } else {
                    all_paths.push(normalize_file_path(&f));
                }
            }
            // Unmark each one
            let unmarked = db.unlock_files_verbose(all_paths, &worker_id, reason)?;
            Ok(json!({
                "success": true,
                "unmarked": unmarked.iter().map(|(f, w)| json!({
                    "file": f,
                    "agent": w
                })).collect::<Vec<_>>(),
                "count": unmarked.len()
            }))
        }
        None => {
            // No file specified and no task - error
            Err(ToolError::missing_field("file or task").into())
        }
    }
}

pub fn list_marks(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let files = get_string_or_array(&args, "files");
    let worker_id = get_string(&args, "agent");
    let task_id = get_string(&args, "task");
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    // Require at least one filter
    if files.is_none() && worker_id.is_none() && task_id.is_none() {
        return Err(ToolError::invalid_value(
            "filter",
            "At least one filter required: agent, task, or files",
        )
        .into());
    }

    // Normalize file paths in the filter if provided (skip lock: prefixed paths)
    let normalized_files = files.map(|paths| {
        paths
            .into_iter()
            .map(|p| {
                if p.starts_with(LOCK_PREFIX) {
                    p
                } else {
                    normalize_file_path(&p)
                }
            })
            .collect()
    });

    let marks = db.get_file_locks(normalized_files, worker_id.as_deref(), task_id.as_deref())?;
    let now = crate::db::now_ms();

    match format {
        OutputFormat::Markdown => {
            let mut md = String::from("# File Marks\n\n");
            if marks.is_empty() {
                md.push_str("No marks found.\n");
            } else {
                md.push_str("| File | Type | Agent | Task | Reason | Age |\n");
                md.push_str("|------|------|-------|------|--------|-----|\n");
                for (path, mark) in &marks {
                    let age_ms = now - mark.locked_at;
                    let age_str = format_duration(age_ms);
                    let lock_type = if path.starts_with(LOCK_PREFIX) {
                        "exclusive"
                    } else {
                        "advisory"
                    };
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} |\n",
                        path,
                        lock_type,
                        mark.worker_id,
                        mark.task_id.as_deref().unwrap_or("-"),
                        mark.reason.as_deref().unwrap_or("-"),
                        age_str
                    ));
                }
            }
            Ok(markdown_to_json(md))
        }
        OutputFormat::Json => {
            let marks_json: Vec<Value> = marks
                .into_iter()
                .map(|(path, mark)| {
                    let is_lock = path.starts_with(LOCK_PREFIX);
                    let age_ms = now - mark.locked_at;
                    json!({
                        "file": path,
                        "is_lock": is_lock,
                        "agent": mark.worker_id,
                        "task_id": mark.task_id,
                        "reason": mark.reason,
                        "marked_at": mark.locked_at,
                        "mark_age_ms": age_ms
                    })
                })
                .collect();

            Ok(json!({ "marks": marks_json }))
        }
    }
}

/// Async version of mark_updates.
pub async fn mark_updates_async(db: std::sync::Arc<Database>, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent").ok_or_else(|| ToolError::missing_field("agent"))?;

    // Run on blocking thread pool since db operations are synchronous
    let updates = tokio::task::spawn_blocking(move || db.claim_updates(&worker_id))
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    Ok(json!({
        "new_marks": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "marked_at": e.timestamp
        })).collect::<Vec<_>>(),
        "removed_marks": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "removed_at": e.timestamp
        })).collect::<Vec<_>>(),
        "sequence": updates.sequence
    }))
}

/// Synchronous version of mark_updates.
pub fn mark_updates(db: &Database, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "agent").ok_or_else(|| ToolError::missing_field("agent"))?;

    let updates = db.claim_updates(&worker_id)?;

    Ok(json!({
        "new_marks": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "marked_at": e.timestamp
        })).collect::<Vec<_>>(),
        "removed_marks": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.worker_id,
            "reason": e.reason,
            "removed_at": e.timestamp
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
        let paths = vec!["src/main.rs".to_string(), "./src/lib.rs".to_string()];
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
