//! Task resource handlers.

use crate::config::{DependenciesConfig, StatesConfig};
use crate::db::Database;
use anyhow::Result;
use serde_json::{Value, json};

pub fn get_all_tasks(db: &Database) -> Result<Value> {
    let tasks = db.get_all_tasks()?;
    let deps = db.get_all_dependencies()?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| json!({
            "id": &t.id,
            "title": t.title,
            "description": t.description,
            "status": &t.status,
            "priority": t.priority,
            "worker_id": &t.worker_id,
            "claimed_at": t.claimed_at,
            "points": t.points,
            "time_estimate_ms": t.time_estimate_ms,
            "time_actual_ms": t.time_actual_ms,
            "current_thought": t.current_thought,
            "cost_usd": t.cost_usd,
            "metrics": t.metrics,
            "created_at": t.created_at,
            "updated_at": t.updated_at
        })).collect::<Vec<_>>(),
        "dependencies": deps.iter().map(|d| json!({
            "from": &d.from_task_id,
            "to": &d.to_task_id,
            "type": &d.dep_type
        })).collect::<Vec<_>>()
    }))
}

pub fn get_ready_tasks(
    db: &Database,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
) -> Result<Value> {
    let tasks = db.get_ready_tasks(None, states_config, deps_config, None, None)?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| json!({
            "id": &t.id,
            "title": t.title,
            "description": t.description,
            "priority": t.priority,
            "points": t.points,
            "needed_tags": t.needed_tags,
            "wanted_tags": t.wanted_tags
        })).collect::<Vec<_>>()
    }))
}

pub fn get_blocked_tasks(
    db: &Database,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
) -> Result<Value> {
    let tasks = db.get_blocked_tasks(states_config, deps_config, None, None)?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| {
            let blockers = db.get_blockers(&t.id).unwrap_or_default();
            json!({
                "id": &t.id,
                "title": t.title,
                "priority": t.priority,
                "blocked_by": &blockers
            })
        }).collect::<Vec<_>>()
    }))
}

pub fn get_claimed_tasks(db: &Database, agent_id: Option<&str>) -> Result<Value> {
    let tasks = db.get_claimed_tasks(agent_id)?;

    Ok(json!({
        "tasks": tasks.iter().map(|t| json!({
            "id": &t.id,
            "title": t.title,
            "status": &t.status,
            "priority": t.priority,
            "worker_id": &t.worker_id,
            "claimed_at": t.claimed_at,
            "current_thought": t.current_thought
        })).collect::<Vec<_>>()
    }))
}

pub fn get_task_tree(db: &Database, task_id: &str) -> Result<Value> {
    let tree = db
        .get_task_tree(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

    Ok(serde_json::to_value(tree)?)
}
