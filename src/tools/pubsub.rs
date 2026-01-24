//! Pub/sub subscription tools.

use super::{get_bool, get_i32, get_string, get_uuid, make_tool};
use crate::db::Database;
use crate::types::TargetType;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "subscribe",
            "Subscribe to change events. target_type: 'task' (status changes), 'file' (lock/unlock), 'agent' (register/timeout). Events delivered to your inbox.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID subscribing"
                },
                "target_type": {
                    "type": "string",
                    "enum": ["task", "file", "agent"],
                    "description": "Type of target to subscribe to"
                },
                "target_id": {
                    "type": "string",
                    "description": "Target identifier (task UUID, file path, or agent ID)"
                }
            }),
            vec!["agent_id", "target_type", "target_id"],
        ),
        make_tool(
            "unsubscribe",
            "Unsubscribe from events.",
            json!({
                "subscription_id": {
                    "type": "string",
                    "description": "Subscription UUID"
                }
            }),
            vec!["subscription_id"],
        ),
        make_tool(
            "poll_inbox",
            "Check for new events from your subscriptions. Returns unread messages. Call periodically to stay informed of changes by other agents.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of messages to return"
                },
                "mark_read": {
                    "type": "boolean",
                    "description": "Whether to mark messages as read (default: true)"
                }
            }),
            vec!["agent_id"],
        ),
        make_tool(
            "clear_inbox",
            "Clear all messages in an agent's inbox.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID"
                }
            }),
            vec!["agent_id"],
        ),
        make_tool(
            "get_subscriptions",
            "Get all subscriptions for an agent.",
            json!({
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID"
                }
            }),
            vec!["agent_id"],
        ),
    ]
}

pub fn subscribe(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;
    let target_type_str = get_string(&args, "target_type")
        .ok_or_else(|| anyhow::anyhow!("target_type is required"))?;
    let target_type = TargetType::from_str(&target_type_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid target_type"))?;
    let target_id = get_string(&args, "target_id")
        .ok_or_else(|| anyhow::anyhow!("target_id is required"))?;

    let subscription_id = db.subscribe(&agent_id, target_type, target_id)?;

    Ok(json!({
        "subscription_id": subscription_id.to_string()
    }))
}

pub fn unsubscribe(db: &Database, args: Value) -> Result<Value> {
    let subscription_id = get_uuid(&args, "subscription_id")
        .ok_or_else(|| anyhow::anyhow!("subscription_id is required"))?;

    let success = db.unsubscribe(subscription_id)?;

    Ok(json!({
        "success": success
    }))
}

pub fn poll_inbox(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;
    let limit = get_i32(&args, "limit");
    let mark_read = get_bool(&args, "mark_read").unwrap_or(true);

    let messages = db.poll_inbox(&agent_id, limit, mark_read)?;

    Ok(json!({
        "messages": messages.iter().map(|m| json!({
            "id": m.id.to_string(),
            "event_type": m.event_type.as_str(),
            "payload": m.payload,
            "created_at": m.created_at
        })).collect::<Vec<_>>()
    }))
}

pub fn clear_inbox(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let cleared = db.clear_inbox(&agent_id)?;

    Ok(json!({
        "success": true,
        "cleared_count": cleared
    }))
}

pub fn get_subscriptions(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent_id")
        .ok_or_else(|| anyhow::anyhow!("agent_id is required"))?;

    let subscriptions = db.get_subscriptions(&agent_id)?;

    Ok(json!({
        "subscriptions": subscriptions.iter().map(|s| json!({
            "id": s.id.to_string(),
            "target_type": s.target_type.as_str(),
            "target_id": s.target_id,
            "created_at": s.created_at
        })).collect::<Vec<_>>()
    }))
}
