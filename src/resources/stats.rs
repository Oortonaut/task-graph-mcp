//! Stats and plan resource handlers.

use crate::config::StatesConfig;
use crate::db::Database;
use anyhow::Result;
use serde_json::{json, Value};

pub fn get_stats_summary(db: &Database, states_config: &StatesConfig) -> Result<Value> {
    let stats = db.get_stats(None, None, states_config)?;

    Ok(json!({
        "total_tasks": stats.total_tasks,
        "by_status": stats.tasks_by_state,
        "points": {
            "total": stats.total_points,
            "completed": stats.completed_points,
            "remaining": stats.total_points - stats.completed_points
        },
        "time": {
            "estimated_ms": stats.total_time_estimate_ms,
            "actual_ms": stats.total_time_actual_ms
        },
        "tokens": {
            "in": stats.total_tokens_in,
            "cached": stats.total_tokens_cached,
            "out": stats.total_tokens_out,
            "thinking": stats.total_tokens_thinking,
            "image": stats.total_tokens_image,
            "audio": stats.total_tokens_audio
        },
        "cost_usd": stats.total_cost_usd
    }))
}

/// Export tasks in ACP (Agent Coordination Protocol) compatible format.
pub fn get_acp_plan(db: &Database) -> Result<Value> {
    let tasks = db.get_all_tasks()?;
    let deps = db.get_all_dependencies()?;

    // Build dependency map
    let mut blockers_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for dep in &deps {
        blockers_map
            .entry(dep.to_task_id.to_string())
            .or_default()
            .push(dep.from_task_id.to_string());
    }

    // Convert tasks to ACP format
    let acp_tasks: Vec<Value> = tasks.iter().map(|t| {
        let blockers = blockers_map.get(&t.id.to_string()).cloned().unwrap_or_default();

        // Map status to ACP format
        let status = match t.status.as_str() {
            "pending" => "todo",
            "in_progress" => "in_progress",
            "completed" => "done",
            _ => &t.status, // Pass through other states
        };

        json!({
            "id": t.id.to_string(),
            "title": t.title,
            "description": t.description,
            "status": status,
            "priority": t.priority.as_str(),
            "blockedBy": blockers,
            "assignee": &t.owner_agent,
            "metadata": {
                "points": t.points,
                "timeEstimateMs": t.time_estimate_ms,
                "timeActualMs": t.time_actual_ms,
                "cost": {
                    "tokensIn": t.tokens_in,
                    "tokensOut": t.tokens_out,
                    "usd": t.cost_usd
                }
            }
        })
    }).collect();

    Ok(json!({
        "version": "1.0",
        "tasks": acp_tasks
    }))
}
