//! Attachment management tools.

use super::{get_bool, get_string, get_string_or_array, make_tool_with_prompts};
use crate::config::{AttachmentsConfig, Prompts, UnknownKeyBehavior};
use crate::db::Database;
use crate::error::{ErrorCode, ToolError};
use crate::format::{OutputFormat, format_attachments_markdown, markdown_to_json};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::path::Path;

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "attach",
            "Add an attachment to a task. Use for notes, comments, or file references.\n\n\
             Attachments are indexed by (task_id, type, sequence). Each type auto-increments its own sequence.\n\n\
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
                "type": {
                    "type": "string",
                    "description": "Attachment type/category (e.g., 'commit', 'note', 'changelist'). Used for indexing and replace operations."
                },
                "name": {
                    "type": "string",
                    "description": "Optional label/name for the attachment (arbitrary string, not used for indexing)"
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
                },
                "mode": {
                    "type": "string",
                    "enum": ["append", "replace"],
                    "description": "How to handle existing attachments of the same type: 'append' (default) adds new, 'replace' deletes all existing of this type first"
                }
            }),
            vec!["task", "type"],
            prompts,
        ),
        make_tool_with_prompts(
            "attachments",
            "Get attachments for a task. Returns metadata only.\n\n\
             To retrieve attachment content, use the `get_attachment` API (not yet available via MCP).",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "type": {
                    "type": "string",
                    "description": "Filter by attachment type pattern (glob syntax: * matches any chars)"
                },
                "mime": {
                    "type": "string",
                    "description": "Filter by MIME type prefix (e.g., 'image/' matches image/png, image/jpeg)"
                }
            }),
            vec!["task"],
            prompts,
        ),
        make_tool_with_prompts(
            "detach",
            "Delete attachments by task and type. Deletes all attachments of the specified type.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "type": {
                    "type": "string",
                    "description": "Attachment type to delete (all attachments of this type will be removed)"
                },
                "delete_files": {
                    "type": "boolean",
                    "description": "If true, also delete files from .task-graph/media/ (default: false)"
                }
            }),
            vec!["agent", "task", "type"],
            prompts,
        ),
    ]
}

/// Validate a MIME type string per RFC 6838 basics.
///
/// Requires exactly one `/` separating non-empty type and subtype,
/// each at most 127 bytes, using only restricted-name characters:
/// alphanumeric, `!`, `#`, `$`, `&`, `-`, `^`, `_`, `.`, `+`.
fn validate_mime_type(mime: &str) -> Result<()> {
    let parts: Vec<&str> = mime.split('/').collect();
    if parts.len() != 2 {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            format!("Invalid MIME type '{}': must contain exactly one '/'", mime),
        )
        .into());
    }
    let (type_part, subtype_part) = (parts[0], parts[1]);
    if type_part.is_empty() || subtype_part.is_empty() {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            format!(
                "Invalid MIME type '{}': type and subtype must be non-empty",
                mime
            ),
        )
        .into());
    }
    if type_part.len() > 127 || subtype_part.len() > 127 {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            format!(
                "Invalid MIME type '{}': type and subtype must be at most 127 bytes",
                mime
            ),
        )
        .into());
    }
    let is_valid_char = |c: char| -> bool {
        c.is_ascii_alphanumeric()
            || matches!(c, '!' | '#' | '$' | '&' | '-' | '^' | '_' | '.' | '+')
    };
    for (label, part) in [("type", type_part), ("subtype", subtype_part)] {
        if let Some(bad) = part.chars().find(|c| !is_valid_char(*c)) {
            return Err(ToolError::new(
                ErrorCode::InvalidFieldValue,
                format!(
                    "Invalid MIME type '{}': {} contains invalid character '{}'",
                    mime, label, bad
                ),
            )
            .into());
        }
    }
    Ok(())
}

/// Maximum filename length — universal limit across ext4, NTFS, APFS.
/// Since sanitized filenames are pure ASCII, bytes == chars == UTF-16 units.
const MAX_FILENAME_LEN: usize = 255;

/// Generate a unique filename for media storage.
fn generate_media_filename(task_id: &str, attachment_type: &str, mime_type: &str) -> String {
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

    // Sanitize type for filename
    let safe_type: String = attachment_type
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Truncate safe_type to fit within MAX_FILENAME_LEN.
    // Fixed parts: {task_id}_{safe_type}_{timestamp}.{ext}
    //   separators: 2 underscores + 1 dot = 3
    //   timestamp: up to 13 digits for millis (until year 2286)
    let timestamp_str = timestamp.to_string();
    let fixed_len = task_id.len() + 1 + 1 + timestamp_str.len() + 1 + ext.len();
    // budget = MAX_FILENAME_LEN - fixed_len (for safe_type portion)
    let safe_type = if fixed_len >= MAX_FILENAME_LEN {
        // No room for safe_type at all — still unique via timestamp
        String::new()
    } else {
        let budget = MAX_FILENAME_LEN - fixed_len;
        if safe_type.len() > budget {
            safe_type[..budget].to_string()
        } else {
            safe_type
        }
    };

    format!("{}_{}_{}.{}", task_id, safe_type, timestamp_str, ext)
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

pub fn attach(
    db: &Database,
    media_dir: &Path,
    attachments_config: &AttachmentsConfig,
    args: Value,
) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");

    let task_ids =
        get_string_or_array(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;

    if task_ids.is_empty() {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            "At least one task ID must be provided",
        )
        .into());
    }

    let attachment_type =
        get_string(&args, "type").ok_or_else(|| ToolError::missing_field("type"))?;
    let name = get_string(&args, "name").unwrap_or_default();
    let content = get_string(&args, "content");
    let file_path = get_string(&args, "file");
    let store_as_file = get_bool(&args, "store_as_file").unwrap_or(false);

    // Check if this is a known key and handle unknown_key behavior
    let is_known = attachments_config.is_known_key(&attachment_type);
    let warning: Option<String> = if !is_known {
        match attachments_config.unknown_key {
            UnknownKeyBehavior::Reject => {
                return Err(ToolError::new(
                    ErrorCode::InvalidFieldValue,
                    format!("Unknown attachment type '{}'. Configure it in attachments.definitions or set unknown_key to 'allow' or 'warn'.", attachment_type)
                ).into());
            }
            UnknownKeyBehavior::Warn => {
                Some(format!("Unknown attachment type '{}'", attachment_type))
            }
            UnknownKeyBehavior::Allow => None,
        }
    } else {
        None
    };

    // Use config defaults for mime/mode, but allow explicit overrides from args
    let mime_type = get_string(&args, "mime").unwrap_or_else(|| {
        attachments_config
            .get_mime_default(&attachment_type)
            .to_string()
    });
    validate_mime_type(&mime_type)?;

    let mode = get_string(&args, "mode").unwrap_or_else(|| {
        attachments_config
            .get_mode_default(&attachment_type)
            .to_string()
    });

    // Validate mode
    if mode != "append" && mode != "replace" {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            "mode must be 'append' or 'replace'",
        )
        .into());
    }

    // Validate: need either content or file
    if content.is_none() && file_path.is_none() {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            "Either 'content' or 'file' must be provided",
        )
        .into());
    }

    // Handle different attachment modes - prepare content/file once for all tasks
    let (base_content, base_file_path): (String, Option<String>) = if let Some(ref fp) = file_path {
        // File reference mode: verify file exists
        let path = Path::new(fp);
        if !path.exists() {
            return Err(
                ToolError::new(ErrorCode::FileNotFound, format!("File not found: {}", fp)).into(),
            );
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
        // Replace mode: delete all existing attachments of this type before adding new one
        if mode == "replace" {
            let old_file_paths = db.delete_attachments_by_type(task_id, &attachment_type)?;
            // Clean up old media files if they were in media dir
            for old_fp in old_file_paths {
                if is_in_media_dir(&old_fp, media_dir) {
                    let _ = std::fs::remove_file(&old_fp);
                }
            }
        }

        // Determine final content and file path for this task
        let (final_content, final_file_path): (String, Option<String>) =
            if store_as_file && file_path.is_none() {
                // Store content to media directory (per-task file)
                let filename = generate_media_filename(task_id, &attachment_type, &mime_type);
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

        let sequence = db.add_attachment(
            task_id,
            attachment_type.clone(),
            name.clone(),
            final_content,
            Some(mime_type.clone()),
            final_file_path.clone(),
        )?;

        let mut result = json!({
            "task_id": task_id,
            "type": &attachment_type,
            "sequence": sequence
        });

        if !name.is_empty() {
            result["name"] = json!(&name);
        }

        if let Some(fp) = final_file_path {
            result["file_path"] = json!(fp);
        }

        results.push(result);
    }

    // Return single result for single task, array for bulk
    let mut response = if results.len() == 1 {
        results.into_iter().next().unwrap()
    } else {
        json!({ "attachments": results })
    };

    // Add warning if unknown key behavior is "warn"
    if let Some(warn_msg) = warning {
        response["warning"] = json!(warn_msg);
    }

    Ok(response)
}

pub fn attachments(
    db: &Database,
    _media_dir: &Path,
    default_format: OutputFormat,
    args: Value,
) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let type_pattern = get_string(&args, "type");
    let mime_pattern = get_string(&args, "mime");
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    // Get filtered attachments (metadata only)
    let attachments =
        db.get_attachments_filtered(&task_id, type_pattern.as_deref(), mime_pattern.as_deref())?;

    match format {
        OutputFormat::Markdown => Ok(markdown_to_json(format_attachments_markdown(&attachments))),
        OutputFormat::Json => {
            let results: Vec<Value> = attachments
                .iter()
                .map(|a| {
                    let mut obj = json!({
                        "task_id": &a.task_id,
                        "type": &a.attachment_type,
                        "sequence": a.sequence,
                        "name": &a.name,
                        "mime_type": &a.mime_type,
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
}

pub fn detach(db: &Database, media_dir: &Path, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");

    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let attachment_type =
        get_string(&args, "type").ok_or_else(|| ToolError::missing_field("type"))?;
    let delete_files = get_bool(&args, "delete_files").unwrap_or(false);

    // Delete from database (returns count and file_paths)
    let (deleted_count, file_paths) =
        db.delete_attachments_by_type_ex(&task_id, &attachment_type)?;

    // If delete_files is true, delete files that were in media dir
    let mut files_deleted = 0;
    if delete_files {
        for fp in &file_paths {
            if is_in_media_dir(fp, media_dir) {
                let path = Path::new(fp);
                if path.exists() && std::fs::remove_file(path).is_ok() {
                    files_deleted += 1;
                }
            }
        }
    }

    Ok(json!({
        "deleted_count": deleted_count,
        "files_deleted": files_deleted
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_mime_type tests ---

    #[test]
    fn test_mime_valid_standard() {
        assert!(validate_mime_type("text/plain").is_ok());
        assert!(validate_mime_type("application/json").is_ok());
        assert!(validate_mime_type("image/png").is_ok());
    }

    #[test]
    fn test_mime_valid_special_chars() {
        // dot, dash, plus are valid restricted-name-chars
        assert!(validate_mime_type("text/git.hash").is_ok());
        assert!(validate_mime_type("text/x-diff").is_ok());
        assert!(validate_mime_type("text/p4.changelist").is_ok());
        assert!(validate_mime_type("application/vnd.api+json").is_ok());
    }

    #[test]
    fn test_mime_missing_slash() {
        assert!(validate_mime_type("textplain").is_err());
    }

    #[test]
    fn test_mime_empty_parts() {
        assert!(validate_mime_type("/plain").is_err());
        assert!(validate_mime_type("text/").is_err());
        assert!(validate_mime_type("/").is_err());
    }

    #[test]
    fn test_mime_multiple_slashes() {
        assert!(validate_mime_type("text/plain/extra").is_err());
    }

    #[test]
    fn test_mime_invalid_chars() {
        assert!(validate_mime_type("text/pla in").is_err()); // space
        assert!(validate_mime_type("text/pla@in").is_err()); // @
        assert!(validate_mime_type("text/pla{in").is_err()); // {
    }

    #[test]
    fn test_mime_too_long_parts() {
        let long = "a".repeat(128);
        assert!(validate_mime_type(&format!("{}/plain", long)).is_err());
        assert!(validate_mime_type(&format!("text/{}", long)).is_err());
        // Exactly 127 is fine
        let max = "a".repeat(127);
        assert!(validate_mime_type(&format!("{}/plain", max)).is_ok());
    }

    // --- generate_media_filename tests ---

    #[test]
    fn test_filename_basic_format() {
        let name = generate_media_filename("task-1", "note", "text/plain");
        assert!(name.starts_with("task-1_note_"));
        assert!(name.ends_with(".txt"));
    }

    #[test]
    fn test_filename_sanitization() {
        let name = generate_media_filename("t1", "my type/here", "text/plain");
        // Spaces and slashes become underscores
        assert!(name.starts_with("t1_my_type_here_"));
    }

    #[test]
    fn test_filename_extension_mapping() {
        assert!(generate_media_filename("t", "x", "application/json").ends_with(".json"));
        assert!(generate_media_filename("t", "x", "text/markdown").ends_with(".md"));
        assert!(generate_media_filename("t", "x", "image/png").ends_with(".png"));
        assert!(generate_media_filename("t", "x", "image/jpeg").ends_with(".jpg"));
        assert!(generate_media_filename("t", "x", "unknown/type").ends_with(".bin"));
    }

    #[test]
    fn test_filename_length_limit() {
        let long_type = "a".repeat(300);
        let name = generate_media_filename("task-1", &long_type, "text/plain");
        assert!(
            name.len() <= MAX_FILENAME_LEN,
            "filename length {} exceeds {}",
            name.len(),
            MAX_FILENAME_LEN
        );
        // Should still have the expected structure
        assert!(name.starts_with("task-1_"));
        assert!(name.ends_with(".txt"));
    }

    #[test]
    fn test_filename_long_task_id() {
        // Even with a very long task_id, filename should still be bounded
        let long_id = "x".repeat(250);
        let name = generate_media_filename(&long_id, "note", "text/plain");
        // safe_type will be empty since task_id eats the budget
        assert!(name.len() <= MAX_FILENAME_LEN || name.starts_with(&long_id));
    }

    #[test]
    fn test_filename_empty_type() {
        let name = generate_media_filename("task-1", "", "text/plain");
        assert!(name.starts_with("task-1_"));
        assert!(name.ends_with(".txt"));
    }
}
