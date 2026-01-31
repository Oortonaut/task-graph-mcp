//! Task claiming tools.
//!
//! The `claim` tool is a convenience wrapper around `update` that transitions
//! a task to the first timed state. For releasing tasks, use `update` with
//! a non-timed state (ownership clears automatically).

use super::{get_bool, get_string, make_tool_with_prompts};
use crate::config::{AppConfig, Prompts, StatesConfig};
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
    config: &AppConfig,
    workflows: &crate::config::workflows::WorkflowsConfig,
    args: Value,
) -> Result<Value> {
    // Derive states from the per-worker workflow so overlay-added states are recognized
    let states_config_owned: StatesConfig = workflows.into();
    let states_config = &states_config_owned;
    let phases_config = &config.phases;
    let deps_config = &config.deps;
    let auto_advance = &config.auto_advance;
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
    let (task, _unblocked, _auto_advanced) = match db.update_task_unified(
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
    ) {
        Ok(result) => result,
        Err(e) => {
            // Check if this is a dependency-blocked error and enrich with structured info
            let err_msg = e.to_string();
            if err_msg.contains("unsatisfied dependencies") {
                // Query the actual blockers to provide structured info
                let blockers = db
                    .get_start_blockers(&task_id, deps_config)
                    .unwrap_or_default();
                if !blockers.is_empty() {
                    return Err(ToolError::deps_not_satisfied(&blockers).into());
                }
            }
            return Err(e);
        }
    };

    // Pre-fetch worker info for context-sensitive prompts (must outlive ctx)
    let worker_info = db.get_worker(&worker_id).ok().flatten();
    let worker_role = worker_info
        .as_ref()
        .map(|w| workflows.match_role(&w.tags))
        .unwrap_or(None);

    // Get transition prompts for claiming (with context-sensitive template expansion)
    let mut transition_prompt_list: Vec<String> = {
        match db.update_worker_state(&worker_id, Some(&task.status), task.phase.as_deref()) {
            Ok((old_status, old_phase)) => {
                // Create context with task and agent info for rich template expansion
                let mut ctx = PromptContext::new(
                    &task.status,
                    task.phase.as_deref(),
                    states_config,
                    phases_config,
                )
                .with_task(&task.id, &task.title, task.priority, &task.tags);

                // Add agent context if worker info is available
                if let Some(ref worker) = worker_info {
                    ctx = ctx.with_agent(&worker_id, worker_role.as_deref(), &worker.tags);
                }

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

    // Add role-specific prompts: both "claiming" guidance and "reporting" guidance
    // This gives the agent full context on how to work and communicate from the start
    if let Some(ref role_name) = worker_role {
        if let Some(claiming_prompt) = workflows.get_role_prompt(role_name, "claiming") {
            transition_prompt_list.push(claiming_prompt.to_string());
        }
        // Also deliver the "reporting" prompt so the agent knows how to communicate
        // progress from the moment they start working
        if let Some(reporting_prompt) = workflows.get_role_prompt(role_name, "reporting") {
            transition_prompt_list.push(reporting_prompt.to_string());
        }
    }

    // Add prompts if any
    if !transition_prompt_list.is_empty()
        && let Value::Object(ref mut map) = response
    {
        map.insert("prompts".to_string(), json!(transition_prompt_list));
    }

    Ok(response)
}
