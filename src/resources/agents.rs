//! Worker resource handlers.

use crate::config::StatesConfig;
use crate::db::Database;
use anyhow::Result;
use serde_json::{Value, json};

pub fn get_all_workers(db: &Database, states_config: &StatesConfig) -> Result<Value> {
    let workers = db.list_workers()?;

    Ok(json!({
        "workers": workers.iter().map(|w| {
            let claim_count = db.get_claim_count(&w.id, states_config).unwrap_or(0);
            json!({
                "id": &w.id,
                "tags": w.tags,
                "max_claims": w.max_claims,
                "current_claims": claim_count,
                "registered_at": w.registered_at,
                "last_heartbeat": w.last_heartbeat
            })
        }).collect::<Vec<_>>()
    }))
}
