//! Attachment management tools.

use super::{get_bool, get_i32, get_string, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::{ErrorCode, ToolError};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::Path;

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "attach",
            "Add an attachment to a task. Use for notes, comments, or file references.\n\n\
             For inline content: provide 'content' directly.\n\
             For file reference: provide 'file' path (existing file, will be referenced).\n\
             For media storage: provide 'content' + 'store_as_file'=true (saves to .task-graph/media/).",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "task": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Task ID or array of Task IDs for bulk attachment"
                },
                "name": {
                    "type": "string",
                    "description": "Attachment name (use 'meta' for structured metadata). Same name replaces existing attachment."
                },
                "content": {
                    "type": "string",
                    "description": "Content (text or base64). Optional if 'file' is provided."
                },
                "mime": {
                    "type": "string",
                    "description": "MIME type (default: text/plain)"
                },
                "file": {
                    "type": "string",
                    "description": "Path to existing file to reference (alternative to content)"
                },
                "store_as_file": {
                    "type": "boolean",
                    "description": "If true, store content in .task-graph/media/ instead of database"
                }
            }),
            vec!["task", "name"],
            prompts,
        ),
        make_tool_with_prompts(
            "attachments",
            "Get attachments for a task. Use content=true to get full content.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "content": {
                    "type": "boolean",
                    "description": "Whether to include attachment content (default: false)"
                }
            }),
            vec!["task"],
            prompts,
        ),
        make_tool_with_prompts(
            "detach",
            "Delete an attachment by task and index. If the attachment references a file in .task-graph/media/, that file is also deleted.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "index": {
                    "type": "integer",
                    "description": "Attachment order index within the task"
                }
            }),
            vec!["task", "index"],
            prompts,
        ),
    ]
}

/// Generate a unique filename for media storage.
fn generate_media_filename(task_id: &str, name: &str, mime_type: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // Determine extension from mime type
    let ext = match mime_type {
        "application/json" => "json",
        "text/plain" => "txt",
        "text/markdown" => "md",
        "text/html" => "html",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        _ => "bin",
    };

    // Sanitize name for filename
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();

    format!("{}_{}_{}.{}", task_id, safe_name, timestamp, ext)
}

/// Check if a file path is within the media directory.
fn is_in_media_dir(file_path: &str, media_dir: &Path) -> bool {
    let file_path = Path::new(file_path);

    // Try to canonicalize both paths for comparison
    if let (Ok(file_abs), Ok(media_abs)) = (file_path.canonicalize(), media_dir.canonicalize()) {
        file_abs.starts_with(media_abs)
    } else {
        // Fall back to string prefix check
        file_path.starts_with(media_dir)
    }
}

pub fn attach(db: &Database, media_dir: &Path, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");
    
    // Task can be string or array of strings
    let task_ids: Vec<String> = if let Some(task_array) = args.get("task").and_then(|v| v.as_array()) {
        task_array
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(task_id) = get_string(&args, "task") {
        vec![task_id]
    } else {
        return Err(ToolError::missing_field("task").into());
    };

    if task_ids.is_empty() {
        return Err(ToolError::new(ErrorCode::InvalidFieldValue, "At least one task ID must be provided").into());
    }

    let name = get_string(&args, "name")
        .ok_or_else(|| ToolError::missing_field("name"))?;
    let content = get_string(&args, "content");
    let mime_type = get_string(&args, "mime").unwrap_or_else(|| "text/plain".to_string());
    let file_path = get_string(&args, "file");
    let store_as_file = get_bool(&args, "store_as_file").unwrap_or(false);

    // Validate: need either content or file
    if content.is_none() && file_path.is_none() {
        return Err(ToolError::new(ErrorCode::InvalidFieldValue, "Either 'content' or 'file' must be provided").into());
    }

    // Handle different attachment modes - prepare content/file once for all tasks
    let (base_content, base_file_path): (String, Option<String>) = if let Some(ref fp) = file_path {
        // File reference mode: verify file exists
        let path = Path::new(fp);
        if !path.exists() {
            return Err(ToolError::new(ErrorCode::FileNotFound, format!("File not found: {}", fp)).into());
        }
        (String::new(), Some(fp.clone()))
    } else if store_as_file {
        // For store_as_file with multiple tasks, we'll create per-task files
        (content.clone().unwrap(), None)
    } else {
        // Inline content mode
        (content.unwrap(), None)
    };

    let mut results = Vec::new();

    for task_id in &task_ids {
        // Replace behavior: delete existing attachment with same name
        if let Ok(Some(old_file_path)) = db.delete_attachment_by_name(task_id, &name) {
            // Clean up old media file if it was in media dir
            if is_in_media_dir(&old_file_path, media_dir) {
                let _ = std::fs::remove_file(&old_file_path);
            }
        }

        // Determine final content and file path for this task
        let (final_content, final_file_path): (String, Option<String>) = if store_as_file && file_path.is_none() {
            // Store content to media directory (per-task file)
            let filename = generate_media_filename(task_id, &name, &mime_type);
            let media_file_path = media_dir.join(&filename);

            // Ensure media directory exists
            std::fs::create_dir_all(media_dir)?;

            // Write content to file
            std::fs::write(&media_file_path, &base_content)?;

            let file_path_str = media_file_path.to_string_lossy().to_string();
            (String::new(), Some(file_path_str))
        } else {
            (base_content.clone(), base_file_path.clone())
        };

        let order_index = db.add_attachment(task_id, name.clone(), final_content, Some(mime_type.clone()), final_file_path.clone())?;

        let mut result = json!({
            "task_id": task_id,
            "order_index": order_index
        });

        if let Some(fp) = final_file_path {
            result["file_path"] = json!(fp);
        }

        results.push(result);
    }

    // Return single result for single task, array for bulk
    if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(json!({ "attachments": results }))
    }
}

pub fn attachments(db: &Database, media_dir: &Path, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let include_content = get_bool(&args, "content").unwrap_or(false);

    // Suppress unused warning - media_dir may be used for relative path resolution in the future
    let _ = media_dir;

    if include_content {
        let attachments = db.get_attachments_full(&task_id, true)?;
        let results: Vec<Value> = attachments
            .iter()
            .map(|a| {
                // If file_path is set, try to read content from file
                let content = if let Some(ref fp) = a.file_path {
                    let path = Path::new(fp);
                    if path.exists() {
                        std::fs::read_to_string(path).unwrap_or_else(|_| {
                            // For binary files, read as base64
                            std::fs::read(path)
                                .map(|bytes| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
                                .unwrap_or_else(|e| format!("[Error reading file: {}]", e))
                        })
                    } else {
                        format!("[File not found: {}]", fp)
                    }
                } else {
                    a.content.clone()
                };

                let mut obj = json!({
                    "task_id": &a.task_id,
                    "order_index": a.order_index,
                    "name": a.name,
                    "mime_type": a.mime_type,
                    "content": content,
                    "created_at": a.created_at
                });

                if let Some(ref fp) = a.file_path {
                    obj["file_path"] = json!(fp);
                }

                obj
            })
            .collect();

        Ok(json!({ "attachments": results }))
    } else {
        let attachments = db.get_attachments(&task_id)?;
        let results: Vec<Value> = attachments
            .iter()
            .map(|a| {
                let mut obj = json!({
                    "task_id": &a.task_id,
                    "order_index": a.order_index,
                    "name": a.name,
                    "mime_type": a.mime_type,
                    "created_at": a.created_at
                });

                if let Some(ref fp) = a.file_path {
                    obj["file_path"] = json!(fp);
                }

                obj
            })
            .collect();

        Ok(json!({ "attachments": results }))
    }
}

pub fn detach(db: &Database, media_dir: &Path, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let order_index = get_i32(&args, "index")
        .ok_or_else(|| ToolError::missing_field("index"))?;

    // Get file path before deletion (to clean up media files)
    let file_path = db.get_attachment_file_path(&task_id, order_index)?;

    // Delete from database
    let deleted = db.delete_attachment(&task_id, order_index)?;

    // If attachment had a file in media dir, delete it
    let mut file_deleted = false;
    if deleted {
        if let Some(fp) = file_path {
            if is_in_media_dir(&fp, media_dir) {
                let path = Path::new(&fp);
                if path.exists() {
                    if let Ok(()) = std::fs::remove_file(path) {
                        file_deleted = true;
                    }
                }
            }
        }
    }

    Ok(json!({
        "success": deleted,
        "file_deleted": file_deleted
    }))
}
