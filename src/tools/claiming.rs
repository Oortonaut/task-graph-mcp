//! Task claiming tools.
//!
//! The `claim` tool is a convenience wrapper around `update` that transitions
//! a task to the first timed state. For releasing tasks, use `update` with
//! a non-timed state (ownership clears automatically).

use super::{get_bool, get_string, get_string_or_array, make_tool_with_prompts};
use crate::config::{AppConfig, Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::prompts::{AttributedPrompt, PromptContext};
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
            },
            "files": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "type": "string" } }
                ],
                "description": "File paths to mark as being worked on (auto-marks with task ID for cleanup on completion)"
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

    // Capture the task's pre-claim status so we can deliver the correct transition prompts.
    // When a coordinator assigns a task (pending -> assigned), the assigned-state prompts
    // go to the coordinator. The worker never sees them. By recording the pre-claim status
    // here, we can include assigned->working transition prompts in the claim response.
    let pre_claim_status = db.get_task(&task_id)?.map(|t| (t.status, t.phase));

    // Find the first timed state to use for claiming
    let claim_status = states_config
        .definitions
        .iter()
        .find(|(_, def)| def.timed)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "working".to_string());

    // Use unified update which handles claiming when transitioning to timed state
    // Claim transitions TO a blocking state, so unblocked/auto_advanced will be empty
    let (task, _unblocked, _auto_advanced, _auto_completed) = match db.update_task_unified(
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

    // Auto-mark files if provided
    let files_marked: Vec<String> = if let Some(file_paths) = get_string_or_array(&args, "files") {
        let mut marked = Vec::new();
        for path in file_paths {
            let normalized = super::files::normalize_file_path(&path);
            // Advisory mark with task_id for auto-cleanup on task completion
            let _warning = db.lock_file(
                normalized.clone(),
                &worker_id,
                None,                  // no reason
                Some(task_id.clone()), // associate with task
            )?;
            marked.push(normalized);
        }
        marked
    } else {
        Vec::new()
    };

    // Pre-fetch worker info for context-sensitive prompts (must outlive ctx)
    let worker_info = db.get_worker(&worker_id).ok().flatten();
    let worker_role = worker_info
        .as_ref()
        .map(|w| workflows.match_role(&w.tags))
        .unwrap_or(None);

    // Get transition prompts for claiming (with context-sensitive template expansion + attribution).
    //
    // Use the task's actual pre-claim status (e.g., "assigned") as the from-state,
    // not the worker's last_status. This ensures the worker receives the full set of
    // transition prompts including any overlay contributions for the assigned->working
    // transition. Previously, prompts were based on the worker's last_status which
    // might be unrelated (e.g., "completed" from a prior task or None if just connected).
    let mut transition_prompt_list: Vec<AttributedPrompt> = {
        // Still update the worker's tracked state for consistency
        let _ = db.update_worker_state(
            &worker_id,
            Some(&task.status),
            task.phase.as_deref(),
            Some(&task.id),
        );

        // Use the task's pre-claim status for prompt computation
        let (from_status, from_phase) = match &pre_claim_status {
            Some((status, phase)) => (status.as_str(), phase.as_deref()),
            None => ("", None),
        };

        // Create context with task and agent info for rich template expansion
        let mut ctx = PromptContext::new(
            &task.status,
            task.phase.as_deref(),
            states_config,
            phases_config,
        )
        .with_task(&task.id, &task.title, task.priority, &task.tags);

        // Add hierarchy level context from level:* tags
        let task_level_str: Option<String> = task
            .tags
            .iter()
            .find(|t| t.starts_with("level:"))
            .map(|t| t.strip_prefix("level:").unwrap_or(t).to_string());
        let child_count = db.get_children_ids(&task.id).ok().map(|ids| ids.len());
        let task_level_ref = task_level_str.as_deref();
        ctx = ctx.with_level(task_level_ref, child_count);

        // Add agent context if worker info is available
        if let Some(ref worker) = worker_info {
            ctx = ctx.with_agent(&worker_id, worker_role.as_deref(), &worker.tags);
        }

        crate::prompts::get_transition_prompts_attributed(
            from_status,
            from_phase,
            &task.status,
            task.phase.as_deref(),
            workflows,
            &ctx,
        )
    };

    let mut response = json!({
        "success": true,
        "task": {
            "id": &task.id,
            "title": task.title,
            "status": task.status,
            "worker_id": task.worker_id,
            "claimed_at": task.claimed_at.map(crate::types::ms_to_iso)
        }
    });

    // Include the pre-claim status so the worker knows the task's prior state
    // (e.g., "assigned" indicates it was push-assigned by a coordinator)
    if let Some((pre_status, pre_phase)) = &pre_claim_status {
        if let Value::Object(ref mut map) = response {
            map.insert("pre_claim_status".to_string(), json!(pre_status));
            if let Some(phase) = pre_phase {
                map.insert("pre_claim_phase".to_string(), json!(phase));
            }
        }
    }

    // Add role-specific prompts: both "claiming" guidance and "reporting" guidance
    // This gives the agent full context on how to work and communicate from the start
    if let Some(ref role_name) = worker_role {
        if let Some(claiming_prompt) = workflows.get_role_prompt(role_name, "claiming") {
            transition_prompt_list.push(AttributedPrompt {
                text: claiming_prompt.to_string(),
                source: format!("role:{}", role_name),
            });
        }
        // Also deliver the "reporting" prompt so the agent knows how to communicate
        // progress from the moment they start working
        if let Some(reporting_prompt) = workflows.get_role_prompt(role_name, "reporting") {
            transition_prompt_list.push(AttributedPrompt {
                text: reporting_prompt.to_string(),
                source: format!("role:{}", role_name),
            });
        }
    }

    // Check for file contention: warn if files marked for this task overlap
    // with files marked by other currently-active tasks/workers.
    // This is advisory -- it does not block the claim.
    let file_contention = db.find_file_contention(&task.id, &worker_id);

    if let Value::Object(ref mut map) = response {
        // Add files_marked if any files were auto-marked
        if !files_marked.is_empty() {
            map.insert("files_marked".to_string(), json!(files_marked));
        }

        // Add prompts if any (with source attribution)
        if !transition_prompt_list.is_empty() {
            let prompt_objects: Vec<Value> = transition_prompt_list
                .iter()
                .map(|p| json!({"text": p.text, "source": p.source}))
                .collect();
            map.insert("prompts".to_string(), json!(prompt_objects));
        }

        // Include relevant advisory hints based on task tags, phase, and worker role
        let advisory_hints = super::advisories::relevant_advisory_topics(
            workflows,
            &task.tags,
            task.phase.as_deref(),
            worker_role.as_deref(),
        );
        if !advisory_hints.is_empty() {
            map.insert("advisory_hints".to_string(), json!(advisory_hints));
        }

        // Include file contention warnings if any overlapping marks found
        if let Ok(ref contentions) = file_contention {
            if !contentions.is_empty() {
                let contention_entries: Vec<Value> = contentions
                    .iter()
                    .map(|(file_path, other_task_id, other_worker_id)| {
                        json!({
                            "file": file_path,
                            "other_task": other_task_id,
                            "other_worker": other_worker_id
                        })
                    })
                    .collect();
                map.insert("file_contention".to_string(), json!(contention_entries));
            }
        }
    }

    Ok(response)
}
