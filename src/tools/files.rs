//! File lock tools (advisory).

use super::{get_string, get_string_array, make_tool};
use crate::db::Database;
use crate::types::{EventType, TargetType};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "lock_file",
            "Declare intent to work on a file (advisory lock).",
            json!({
                "file_path": {
                    "type": "string",
                    "description": "Relative file path"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID"
                }
            }),
            vec!["file_path", "agent_id"],
        ),
        make_tool(
            "unlock_file",
            "Release an advisory file lock.",
            json!({
                "file_path": {
                    "type": "string",
                    "description": "Relative file path"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID"
                }
            }),
            vec!["file_path", "agent_id"],
        ),
        make_tool(
            "get_file_locks",
            "Get current file locks.",
            json!({
                "file_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific file paths to check (optional)"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Filter by agent ID (optional)"
                }
            }),
            vec![],
        ),
    ]
}

pub fn lock_file(db: &Database, args: Value) -> Result<Value> {
    let file_path = get_string(&args, "file_path")
        .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let warning = db.lock_file(file_path.clone(), &agent_id)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::File,
        &file_path,
        EventType::FileLocked,
        json!({
            "file_path": file_path,
            "agent_id": &agent_id
        }),
    );

    if let Some(other_agent) = warning {
        Ok(json!({
            "success": true,
            "warning": format!("File already locked by agent {}", other_agent),
            "locked_by": other_agent
        }))
    } else {
        Ok(json!({
            "success": true
        }))
    }
}

pub fn unlock_file(db: &Database, args: Value) -> Result<Value> {
    let file_path = get_string(&args, "file_path")
        .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let unlocked = db.unlock_file(&file_path, &agent_id)?;

    if unlocked {
        // Publish event for subscribers
        let _ = db.publish_event(
            TargetType::File,
            &file_path,
            EventType::FileUnlocked,
            json!({
                "file_path": file_path,
                "agent_id": &agent_id
            }),
        );
    }

    Ok(json!({
        "success": unlocked
    }))
}

pub fn get_file_locks(db: &Database, args: Value) -> Result<Value> {
    let file_paths = get_string_array(&args, "file_paths");
    let agent_id = get_string(&args, "agent_id");

    let locks = db.get_file_locks(file_paths, agent_id.as_deref())?;

    let locks_json: serde_json::Map<String, Value> = locks
        .into_iter()
        .map(|(path, agent)| (path, json!(agent)))
        .collect();

    Ok(json!({
        "locks": locks_json
    }))
}
