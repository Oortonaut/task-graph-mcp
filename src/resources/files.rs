//! File lock resource handlers.

use crate::db::Database;
use anyhow::Result;
use serde_json::{json, Value};

pub fn get_all_file_locks(db: &Database) -> Result<Value> {
    let locks = db.get_all_file_locks()?;

    Ok(json!({
        "locks": locks.iter().map(|l| json!({
            "file_path": l.file_path,
            "worker_id": l.worker_id.to_string(),
            "locked_at": l.locked_at
        })).collect::<Vec<_>>()
    }))
}
