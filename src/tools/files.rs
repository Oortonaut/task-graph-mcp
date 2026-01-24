//! File coordination tools (advisory locking).

use super::{get_string, get_string_array, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

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
                    "type": "string",
                    "description": "Relative file path"
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
                    "type": "string",
                    "description": "Relative file path"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason/note for next claimant"
                }
            }),
            vec!["agent", "file"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_files",
            "Get current file locks.",
            json!({
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific file paths to check (optional)"
                },
                "agent": {
                    "type": "string",
                    "description": "Filter by agent ID (optional)"
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
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter to specific files (optional, omit for all)"
                }
            }),
            vec!["agent"],
            prompts,
        ),
    ]
}

pub fn claim_file(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let file_path = get_string(&args, "file")
        .ok_or_else(|| anyhow::anyhow!("file is required"))?;
    let reason = get_string(&args, "reason");

    // Lock the file
    let warning = db.lock_file(file_path.clone(), &agent_id, reason)?;

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

pub fn release_file(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let file_path = get_string(&args, "file")
        .ok_or_else(|| anyhow::anyhow!("file is required"))?;
    let reason = get_string(&args, "reason");

    // Unlock the file
    let released = db.unlock_file(&file_path, &agent_id, reason)?;

    Ok(json!({
        "success": released
    }))
}

pub fn list_files(db: &Database, args: Value) -> Result<Value> {
    let files = get_string_array(&args, "files");
    let agent_id = get_string(&args, "agent");

    let locks = db.get_file_locks(files, agent_id.as_deref())?;

    let locks_json: serde_json::Map<String, Value> = locks
        .into_iter()
        .map(|(path, agent)| (path, json!(agent)))
        .collect();

    Ok(json!({
        "locks": locks_json
    }))
}

pub fn claim_updates(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| anyhow::anyhow!("agent is required"))?;
    let files = get_string_array(&args, "files");

    let updates = db.claim_updates(&agent_id, files)?;

    Ok(json!({
        "new_claims": updates.new_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.agent_id,
            "reason": e.reason,
            "claimed_at": e.timestamp
        })).collect::<Vec<_>>(),
        "dropped_claims": updates.dropped_claims.iter().map(|e| json!({
            "file": e.file_path,
            "agent": e.agent_id,
            "reason": e.reason,
            "dropped_at": e.timestamp
        })).collect::<Vec<_>>(),
        "sequence": updates.sequence
    }))
}
