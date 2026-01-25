//! Agent connection and management tools.

use super::{get_i32, get_string, get_string_array, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{format_agents_markdown, markdown_to_json, OutputFormat};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "connect",
            "Connect as an agent. Call this FIRST before using other tools. Returns agent_id (save it for all subsequent calls). Tags enable task affinity matching.",
            json!({
                "agent": {
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
            prompts,
        ),
        make_tool_with_prompts(
            "disconnect",
            "Disconnect an agent, releasing all claims and locks.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "The agent's ID"
                }
            }),
            vec!["agent"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_agents",
            "List all connected agents with their current status, claim counts, and what they're working on.",
            json!({
                "format": {
                    "type": "string",
                    "enum": ["json", "markdown"],
                    "description": "Output format (default: json)"
                }
            }),
            vec![],
            prompts,
        ),
    ]
}

pub fn connect(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent");
    let name = get_string(&args, "name");
    let tags = get_string_array(&args, "tags").unwrap_or_default();
    let max_claims = get_i32(&args, "max_claims");

    let agent = db.register_agent(agent_id, name, tags, max_claims)?;

    Ok(json!({
        "agent_id": &agent.id,
        "name": agent.name,
        "tags": agent.tags,
        "max_claims": agent.max_claims,
        "registered_at": agent.registered_at
    }))
}

pub fn disconnect(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent")
        .ok_or_else(|| ToolError::missing_field("agent"))?;

    // Release agent locks before unregistering
    let _ = db.release_agent_locks(&agent_id);

    db.unregister_agent(&agent_id)?;

    Ok(json!({
        "success": true
    }))
}

pub fn list_agents(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(default_format);

    let agents = db.list_agents_info()?;

    match format {
        OutputFormat::Markdown => {
            Ok(markdown_to_json(format_agents_markdown(&agents)))
        }
        OutputFormat::Json => {
            Ok(json!({
                "agents": agents.iter().map(|a| json!({
                    "id": a.id,
                    "name": a.name,
                    "tags": a.tags,
                    "max_claims": a.max_claims,
                    "claim_count": a.claim_count,
                    "current_thought": a.current_thought,
                    "registered_at": a.registered_at,
                    "last_heartbeat": a.last_heartbeat
                })).collect::<Vec<_>>()
            }))
        }
    }
}
