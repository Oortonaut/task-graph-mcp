//! Agent registration and management tools.

use super::{get_i32, get_string, get_string_array, make_tool};
use crate::db::Database;
use crate::types::{EventType, TargetType};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "register_agent",
            "Register a new agent session. Returns agent_id and config.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Optional custom agent ID (max 36 chars). If not provided, a UUID7 will be generated."
                },
                "name": {
                    "type": "string",
                    "description": "Optional display name for the agent"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Freeform tags for capabilities, roles, etc."
                },
                "max_claims": {
                    "type": "integer",
                    "description": "Maximum number of tasks this agent can claim (default: 5)"
                }
            }),
            vec![],
        ),
        make_tool(
            "update_agent",
            "Update an agent's properties.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "The agent's UUID"
                },
                "name": {
                    "type": "string",
                    "description": "New display name"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "New tags array"
                },
                "max_claims": {
                    "type": "integer",
                    "description": "New maximum claim limit"
                }
            }),
            vec!["agent_id"],
        ),
        make_tool(
            "heartbeat",
            "Refresh agent heartbeat. Returns current claim count.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "The agent's UUID"
                }
            }),
            vec!["agent_id"],
        ),
        make_tool(
            "unregister_agent",
            "Unregister an agent, releasing all claims and locks.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "The agent's UUID"
                }
            }),
            vec!["agent_id"],
        ),
    ]
}

pub fn register_agent(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id");
    let name = get_string(&args, "name");
    let tags = get_string_array(&args, "tags").unwrap_or_default();
    let max_claims = get_i32(&args, "max_claims");

    let agent = db.register_agent(agent_id, name, tags, max_claims)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Agent,
        &agent.id,
        EventType::AgentRegistered,
        json!({
            "agent_id": &agent.id,
            "name": agent.name,
            "tags": agent.tags
        }),
    );

    Ok(json!({
        "agent_id": &agent.id,
        "name": agent.name,
        "tags": agent.tags,
        "max_claims": agent.max_claims,
        "registered_at": agent.registered_at
    }))
}

pub fn update_agent(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;
    let name = if args.get("name").is_some() {
        Some(get_string(&args, "name"))
    } else {
        None
    };
    let tags = get_string_array(&args, "tags");
    let max_claims = get_i32(&args, "max_claims");

    let agent = db.update_agent(&agent_id, name, tags, max_claims)?;

    Ok(json!({
        "agent_id": &agent.id,
        "name": agent.name,
        "tags": agent.tags,
        "max_claims": agent.max_claims
    }))
}

pub fn heartbeat(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let claim_count = db.heartbeat(&agent_id)?;

    Ok(json!({
        "success": true,
        "claim_count": claim_count
    }))
}

pub fn unregister_agent(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    // Release agent locks before unregistering
    let _ = db.release_agent_locks(&agent_id);

    // Publish event for subscribers before unregistering
    let _ = db.publish_event(
        TargetType::Agent,
        &agent_id,
        EventType::AgentTimeout,
        json!({
            "agent_id": &agent_id
        }),
    );

    db.unregister_agent(&agent_id)?;

    Ok(json!({
        "success": true
    }))
}
