//! Live status and tracking tools.

use super::{
    get_f64, get_i64, get_string, get_string_array, get_string_or_array, make_tool_with_prompts,
};
use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{OutputFormat, markdown_to_json};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Format a duration in milliseconds to a human-readable string.
fn format_duration_ms(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 3_600_000 {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m {}s", mins, secs)
    } else {
        let hours = ms / 3_600_000;
        let mins = (ms % 3_600_000) / 60_000;
        format!("{}h {}m", hours, mins)
    }
}

/// Format a timestamp (ms since epoch) to ISO-like string.
fn format_timestamp(ts: i64) -> String {
    let secs = ts / 1000;
    let datetime = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn get_tools(prompts: &Prompts, states_config: &StatesConfig) -> Vec<Tool> {
    // Build state enum from config
    let state_names: Vec<&str> = states_config.state_names();
    let state_enum: Vec<Value> = state_names.iter().map(|s| json!(s)).collect();

    vec![
        make_tool_with_prompts(
            "thinking",
            "Broadcast real-time status updates (what you're doing right now). Also refreshes heartbeat. Call frequently during work to show live progress.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "thought": {
                    "type": "string",
                    "description": "What the agent is currently doing"
                },
                "tasks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific task IDs to update (default: all claimed tasks)"
                }
            }),
            vec!["agent", "thought"],
            prompts,
        ),
        make_tool_with_prompts(
            "task_history",
            "Get the status transition history for a task, including automatic time tracking data and aggregate statistics.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "states": {
                    "type": "array",
                    "items": { "type": "string", "enum": state_enum },
                    "description": "Filter to only show transitions involving these statuses"
                }
            }),
            vec!["task"],
            prompts,
        ),
        make_tool_with_prompts(
            "log_metrics",
            "Log metrics and cost for a task. Values are aggregated (added to existing).",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "cost_usd": {
                    "type": "number",
                    "description": "Cost in USD to add"
                },
                "values": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Array of up to 8 integer metric values [metric_0..metric_7] to aggregate"
                }
            }),
            vec!["agent", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "project_history",
            "Get project-wide status transition history and aggregate statistics. Like task_history but across all tasks with date/time range filters.",
            json!({
                "from": {
                    "type": "string",
                    "description": "Start of time range (ISO 8601 datetime or milliseconds since epoch)"
                },
                "to": {
                    "type": "string",
                    "description": "End of time range (ISO 8601 datetime or milliseconds since epoch)"
                },
                "states": {
                    "type": "array",
                    "items": { "type": "string", "enum": state_enum },
                    "description": "Filter to only show transitions involving these statuses"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of transitions to return (default: 100)"
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "get_metrics",
            "Get metrics and cost for one or more tasks. Returns cost_usd and metrics array, aggregated across all tasks if multiple provided.",
            json!({
                "task": {
                    "oneOf": [
                        { "type": "string", "description": "Single task ID" },
                        { "type": "array", "items": { "type": "string" }, "description": "Array of task IDs" }
                    ],
                    "description": "Task ID or array of task IDs to get metrics for"
                }
            }),
            vec!["task"],
            prompts,
        ),
    ]
}

pub fn thinking(db: &Database, args: Value) -> Result<Value> {
    let agent_id = get_string(&args, "agent").ok_or_else(|| ToolError::missing_field("agent"))?;
    let thought =
        get_string(&args, "thought").ok_or_else(|| ToolError::missing_field("thought"))?;
    let task_ids = get_string_or_array(&args, "tasks");

    // Also refresh heartbeat since updating thought implies activity
    let _ = db.heartbeat(&agent_id);

    let updated = db.set_thought(&agent_id, Some(thought), task_ids)?;

    Ok(json!({
        "success": true,
        "updated_count": updated
    }))
}

pub fn task_history(
    db: &Database,
    states_config: &StatesConfig,
    default_format: OutputFormat,
    args: Value,
) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let state_filter = get_string_array(&args, "states");
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    let history = db.get_task_state_history(&task_id)?;
    let current_duration = db.get_current_state_duration(&task_id, states_config)?;

    // Filter history by statuses if specified
    let filtered_history: Vec<_> = if let Some(ref states) = state_filter {
        history
            .into_iter()
            .filter(|e| e.status.as_ref().is_some_and(|s| states.contains(s)))
            .collect()
    } else {
        history
    };

    // Calculate aggregate stats
    let mut time_per_status: HashMap<String, i64> = HashMap::new();
    let mut time_per_agent: HashMap<String, i64> = HashMap::new();

    for event in &filtered_history {
        if let Some(end_ts) = event.end_timestamp {
            let duration = end_ts - event.timestamp;
            if let Some(ref status) = event.status {
                *time_per_status.entry(status.clone()).or_insert(0) += duration;
            }
            if let Some(ref agent) = event.worker_id {
                *time_per_agent.entry(agent.clone()).or_insert(0) += duration;
            }
        }
    }

    // Add current duration to the current state if applicable
    if let Some(current_dur) = current_duration
        && let Some(last_event) = filtered_history.last()
        && last_event.end_timestamp.is_none()
    {
        // Include in state filter check
        if let Some(ref status) = last_event.status
            && (state_filter.is_none() || state_filter.as_ref().unwrap().contains(status))
        {
            *time_per_status.entry(status.clone()).or_insert(0) += current_dur;
            if let Some(ref agent) = last_event.worker_id {
                *time_per_agent.entry(agent.clone()).or_insert(0) += current_dur;
            }
        }
    }

    match format {
        OutputFormat::Markdown => {
            let mut md = String::from("# Task History\n\n");

            // History table
            md.push_str("## Status Transitions\n\n");
            if filtered_history.is_empty() {
                md.push_str("No status transitions found.\n");
            } else {
                md.push_str("| # | Status | Agent | Timestamp | Duration |\n");
                md.push_str("|---|-------|-------|-----------|----------|\n");
                for (i, event) in filtered_history.iter().enumerate() {
                    let duration = if let Some(end_ts) = event.end_timestamp {
                        format_duration_ms(end_ts - event.timestamp)
                    } else if let Some(dur) = current_duration {
                        format!("{} (ongoing)", format_duration_ms(dur))
                    } else {
                        "ongoing".to_string()
                    };
                    let agent = event.worker_id.as_deref().unwrap_or("-");
                    let status = event.status.as_deref().unwrap_or("-");
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} |\n",
                        i + 1,
                        status,
                        agent,
                        format_timestamp(event.timestamp),
                        duration
                    ));
                }
            }

            // Aggregate stats
            md.push_str("\n## Time per Status\n\n");
            if time_per_status.is_empty() {
                md.push_str("No completed status durations.\n");
            } else {
                md.push_str("| Status | Total Time |\n");
                md.push_str("|--------|------------|\n");
                let mut sorted_statuses: Vec<_> = time_per_status.iter().collect();
                sorted_statuses.sort_by_key(|(k, _)| k.as_str());
                for (status, time) in sorted_statuses {
                    md.push_str(&format!("| {} | {} |\n", status, format_duration_ms(*time)));
                }
            }

            md.push_str("\n## Time per Agent\n\n");
            if time_per_agent.is_empty() {
                md.push_str("No agent time tracked.\n");
            } else {
                md.push_str("| Agent | Total Time |\n");
                md.push_str("|-------|------------|\n");
                let mut sorted_agents: Vec<_> = time_per_agent.iter().collect();
                sorted_agents.sort_by_key(|(k, _)| k.as_str());
                for (agent, time) in sorted_agents {
                    md.push_str(&format!("| {} | {} |\n", agent, format_duration_ms(*time)));
                }
            }

            Ok(markdown_to_json(md))
        }
        OutputFormat::Json => Ok(json!({
            "history": filtered_history,
            "current_duration_ms": current_duration,
            "time_per_status_ms": time_per_status,
            "time_per_agent_ms": time_per_agent
        })),
    }
}

pub fn log_metrics(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;

    let cost_usd = get_f64(&args, "cost_usd");

    // Parse values array
    let values: Vec<i64> = args
        .get("values")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let task = db.log_metrics(&task_id, cost_usd, &values)?;

    Ok(json!({
        "success": true,
        "cost_usd": task.cost_usd,
        "metrics": task.metrics
    }))
}

/// Parse a timestamp from either ISO 8601 string or milliseconds.
fn parse_timestamp(s: &str) -> Option<i64> {
    // Try parsing as milliseconds first
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms);
    }

    // Try parsing as ISO 8601 datetime
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }

    // Try parsing common datetime formats
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc().timestamp_millis());
    }

    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc().timestamp_millis());
    }

    // Try parsing date only
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis());
    }

    None
}

pub fn project_history(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let from_timestamp = get_string(&args, "from").and_then(|s| parse_timestamp(&s));
    let to_timestamp = get_string(&args, "to").and_then(|s| parse_timestamp(&s));
    let state_filter = get_string_array(&args, "states");
    let limit = get_i64(&args, "limit").or(Some(100));
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    // Get transitions
    let history =
        db.get_project_state_history(from_timestamp, to_timestamp, state_filter.as_deref(), limit)?;

    // Get aggregate stats
    let stats = db.get_project_state_stats(from_timestamp, to_timestamp)?;

    match format {
        OutputFormat::Markdown => {
            let mut md = String::from("# Project History\n\n");

            // Time range info
            md.push_str("## Time Range\n\n");
            let from_str = from_timestamp
                .map(format_timestamp)
                .unwrap_or_else(|| "beginning".to_string());
            let to_str = to_timestamp
                .map(format_timestamp)
                .unwrap_or_else(|| "now".to_string());
            md.push_str(&format!("**From:** {} **To:** {}\n\n", from_str, to_str));

            // Summary stats
            md.push_str("## Summary\n\n");
            md.push_str(&format!(
                "- **Total Transitions:** {}\n",
                stats.total_transitions
            ));
            md.push_str(&format!("- **Tasks Affected:** {}\n", stats.tasks_affected));
            md.push_str(&format!(
                "- **Total Time Tracked:** {}\n\n",
                format_duration_ms(stats.total_time_ms)
            ));

            // Recent transitions table
            md.push_str("## Recent Transitions\n\n");
            if history.is_empty() {
                md.push_str("No status transitions found.\n");
            } else {
                md.push_str("| # | Task | Status | Agent | Timestamp | Duration |\n");
                md.push_str("|---|------|-------|-------|-----------|----------|\n");
                for (i, event) in history.iter().enumerate() {
                    let duration = if let Some(end_ts) = event.end_timestamp {
                        format_duration_ms(end_ts - event.timestamp)
                    } else {
                        "ongoing".to_string()
                    };
                    let agent = event.worker_id.as_deref().unwrap_or("-");
                    let short_task = if event.task_id.len() > 12 {
                        format!("{}...", &event.task_id[..12])
                    } else {
                        event.task_id.clone()
                    };
                    let status = event.status.as_deref().unwrap_or("-");
                    md.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} |\n",
                        i + 1,
                        short_task,
                        status,
                        agent,
                        format_timestamp(event.timestamp),
                        duration
                    ));
                }
            }

            // Transitions by status
            md.push_str("\n## Transitions by Status\n\n");
            if stats.transitions_by_status.is_empty() {
                md.push_str("No transitions found.\n");
            } else {
                md.push_str("| Status | Count | Total Time |\n");
                md.push_str("|-------|-------|------------|\n");
                let mut sorted_statuses: Vec<_> = stats.transitions_by_status.iter().collect();
                sorted_statuses.sort_by_key(|(k, _)| k.as_str());
                for (status, count) in sorted_statuses {
                    let time = stats.time_by_status_ms.get(status).copied().unwrap_or(0);
                    md.push_str(&format!(
                        "| {} | {} | {} |\n",
                        status,
                        count,
                        format_duration_ms(time)
                    ));
                }
            }

            // Transitions by agent
            md.push_str("\n## Transitions by Agent\n\n");
            if stats.transitions_by_agent.is_empty() {
                md.push_str("No agent activity tracked.\n");
            } else {
                md.push_str("| Agent | Count | Total Time |\n");
                md.push_str("|-------|-------|------------|\n");
                let mut sorted_agents: Vec<_> = stats.transitions_by_agent.iter().collect();
                sorted_agents.sort_by(|(_, a), (_, b)| b.cmp(a)); // Sort by count descending
                for (agent, count) in sorted_agents {
                    let time = stats.time_by_agent_ms.get(agent).copied().unwrap_or(0);
                    md.push_str(&format!(
                        "| {} | {} | {} |\n",
                        agent,
                        count,
                        format_duration_ms(time)
                    ));
                }
            }

            Ok(markdown_to_json(md))
        }
        OutputFormat::Json => Ok(json!({
            "time_range": {
                "from_ms": from_timestamp,
                "to_ms": to_timestamp
            },
            "summary": {
                "total_transitions": stats.total_transitions,
                "tasks_affected": stats.tasks_affected,
                "total_time_ms": stats.total_time_ms
            },
            "transitions": history,
            "transitions_by_status": stats.transitions_by_status,
            "time_by_status_ms": stats.time_by_status_ms,
            "transitions_by_agent": stats.transitions_by_agent,
            "time_by_agent_ms": stats.time_by_agent_ms
        })),
    }
}

pub fn get_metrics(db: &Database, args: Value) -> Result<Value> {
    use super::get_string_or_array;

    let task_ids =
        get_string_or_array(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;

    if task_ids.is_empty() {
        return Err(ToolError::missing_field("task").into());
    }

    // Get metrics for all specified tasks
    let mut total_cost_usd: f64 = 0.0;
    let mut total_metrics: [i64; 8] = [0; 8];
    let mut found_count = 0;

    for task_id in &task_ids {
        if let Some(task) = db.get_task(task_id)? {
            total_cost_usd += task.cost_usd;
            for (total, task_metric) in total_metrics.iter_mut().zip(task.metrics.iter()) {
                *total += task_metric;
            }
            found_count += 1;
        }
    }

    if found_count == 0 {
        return Err(anyhow::anyhow!("No tasks found with the provided IDs"));
    }

    let response = if task_ids.len() == 1 {
        // Single task - return flat response
        json!({
            "task": task_ids[0],
            "cost_usd": total_cost_usd,
            "metrics": total_metrics
        })
    } else {
        // Multiple tasks - return aggregated response
        json!({
            "tasks": task_ids,
            "task_count": found_count,
            "cost_usd": total_cost_usd,
            "metrics": total_metrics
        })
    };

    Ok(response)
}
