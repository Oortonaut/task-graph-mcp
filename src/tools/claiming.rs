//! Task claiming tools.
//!
//! The `claim` tool is a convenience wrapper around `update` that transitions
//! a task to the first timed state. For releasing tasks, use `update` with
//! a non-timed state (ownership clears automatically).

use super::{get_bool, get_string, make_tool_with_prompts};
use crate::config::{AutoAdvanceConfig, DependenciesConfig, PhasesConfig, Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::prompts::PromptContext;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

pub fn get_tools(prompts: &Prompts, _states_config: &StatesConfig) -> Vec<Tool> {
    vec![make_tool_with_prompts(
        "claim",
        "Commit to working on a task (like adding to a changelist). Fails if: already claimed, deps unsatisfied, or worker lacks required tags. Sets status to timed (working) status.",
        json!({
            "worker_id": {
                "type": "string",
                "description": "Worker ID claiming the task"
            },
            "task": {
                "type": "string",
                "description": "Task ID to claim"
            },
            "force": {
                "type": "boolean",
                "description": "Force claim even if owned by another agent (default: false)"
            }
        }),
        vec!["worker_id", "task"],
        prompts,
    )]
}

pub fn claim(
    db: &Database,
    states_config: &StatesConfig,
    phases_config: &PhasesConfig,
    deps_config: &DependenciesConfig,
    auto_advance: &AutoAdvanceConfig,
    workflows: &crate::config::workflows::WorkflowsConfig,
    args: Value,
) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let force = get_bool(&args, "force").unwrap_or(false);

    // Find the first timed state to use for claiming
    let claim_status = states_config
        .definitions
        .iter()
        .find(|(_, def)| def.timed)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "working".to_string());

    // Use unified update which handles claiming when transitioning to timed state
    // Claim transitions TO a blocking state, so unblocked/auto_advanced will be empty
    let (task, _unblocked, _auto_advanced) = db.update_task_unified(
        &task_id,
        &worker_id,
        None,               // assignee (not assigning to another agent)
        None,               // title
        None,               // description
        Some(claim_status), // status - first timed state
        None,               // phase
        None,               // priority
        None,               // points
        None,               // tags
        None,               // needed_tags
        None,               // wanted_tags
        None,               // time_estimate_ms
        None,               // reason
        force,
        states_config,
        deps_config,
        auto_advance,
    )?;

    // Get transition prompts for claiming (with template expansion)
    let transition_prompt_list: Vec<String> = {
        match db.update_worker_state(&worker_id, Some(&task.status), task.phase.as_deref()) {
            Ok((old_status, old_phase)) => {
                // Create context for template expansion
                let ctx = PromptContext::new(
                    &task.status,
                    task.phase.as_deref(),
                    states_config,
                    phases_config,
                );
                crate::prompts::get_transition_prompts_with_context(
                    old_status.as_deref().unwrap_or(""),
                    old_phase.as_deref(),
                    &task.status,
                    task.phase.as_deref(),
                    workflows,
                    &ctx,
                )
            }
            Err(_) => vec![],
        }
    };

    let mut response = json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status,
            "worker_id": task.worker_id,
            "claimed_at": task.claimed_at
        }
    });

    // Add prompts if any
    if !transition_prompt_list.is_empty()
        && let Value::Object(ref mut map) = response
    {
        map.insert("prompts".to_string(), json!(transition_prompt_list));
    }

    Ok(response)
}
