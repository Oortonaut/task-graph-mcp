//! Agent resource handlers.

use crate::db::Database;
use anyhow::Result;
use serde_json::{json, Value};

pub fn get_all_agents(db: &Database) -> Result<Value> {
    let agents = db.list_agents()?;

    Ok(json!({
        "agents": agents.iter().map(|a| {
            let claim_count = db.get_claim_count(&a.id).unwrap_or(0);
            json!({
                "id": &a.id,
                "tags": a.tags,
                "max_claims": a.max_claims,
                "current_claims": claim_count,
                "registered_at": a.registered_at,
                "last_heartbeat": a.last_heartbeat
            })
        }).collect::<Vec<_>>()
    }))
}
