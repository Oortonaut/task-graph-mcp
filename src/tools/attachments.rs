//! Attachment management tools.

use super::{get_string, get_uuid, make_tool};
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "add_attachment",
            "Add an attachment to a task.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                },
                "name": {
                    "type": "string",
                    "description": "Attachment name"
                },
                "content": {
                    "type": "string",
                    "description": "Content (text or base64)"
                },
                "mime_type": {
                    "type": "string",
                    "description": "MIME type (default: text/plain)"
                }
            }),
            vec!["task_id", "name", "content"],
        ),
        make_tool(
            "get_attachments",
            "Get attachments for a task (metadata only).",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                }
            }),
            vec!["task_id"],
        ),
        make_tool(
            "get_attachment",
            "Get a full attachment with content.",
            json!({
                "attachment_id": {
                    "type": "string",
                    "description": "Attachment UUID"
                }
            }),
            vec!["attachment_id"],
        ),
        make_tool(
            "delete_attachment",
            "Delete an attachment.",
            json!({
                "attachment_id": {
                    "type": "string",
                    "description": "Attachment UUID"
                }
            }),
            vec!["attachment_id"],
        ),
    ]
}

pub fn add_attachment(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let name = get_string(&args, "name")
        .ok_or_else(|| anyhow::anyhow!("name is required"))?;
    let content = get_string(&args, "content")
        .ok_or_else(|| anyhow::anyhow!("content is required"))?;
    let mime_type = get_string(&args, "mime_type");

    let attachment_id = db.add_attachment(task_id, name, content, mime_type)?;

    Ok(json!({
        "attachment_id": attachment_id.to_string()
    }))
}

pub fn get_attachments(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;

    let attachments = db.get_attachments(task_id)?;

    Ok(json!({
        "attachments": attachments.iter().map(|a| json!({
            "id": a.id.to_string(),
            "task_id": a.task_id.to_string(),
            "name": a.name,
            "mime_type": a.mime_type,
            "created_at": a.created_at
        })).collect::<Vec<_>>()
    }))
}

pub fn get_attachment(db: &Database, args: Value) -> Result<Value> {
    let attachment_id = get_uuid(&args, "attachment_id")
        .ok_or_else(|| anyhow::anyhow!("attachment_id is required"))?;

    let attachment = db.get_attachment(attachment_id)?
        .ok_or_else(|| anyhow::anyhow!("Attachment not found"))?;

    Ok(json!({
        "id": attachment.id.to_string(),
        "task_id": attachment.task_id.to_string(),
        "name": attachment.name,
        "mime_type": attachment.mime_type,
        "content": attachment.content,
        "created_at": attachment.created_at
    }))
}

pub fn delete_attachment(db: &Database, args: Value) -> Result<Value> {
    let attachment_id = get_uuid(&args, "attachment_id")
        .ok_or_else(|| anyhow::anyhow!("attachment_id is required"))?;

    let deleted = db.delete_attachment(attachment_id)?;

    Ok(json!({
        "success": deleted
    }))
}
