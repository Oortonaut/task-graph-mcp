//! MCP tool for querying workflow prompts and guidance.
//!
//! Exposes prompt triggers and expanded prompts to MCP agents,
//! letting them inspect what guidance applies before making transitions.

use super::{get_string, make_tool};
use crate::config::workflows::WorkflowsConfig;
use crate::config::{PhasesConfig, StatesConfig};
use crate::db::Database;
use crate::prompts::{self, PromptContext};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Get the prompt-related tools.
pub fn get_tools() -> Vec<Tool> {
    vec![make_tool(
        "get_prompts",
        "Get workflow prompts and guidance. Without parameters: lists all prompt triggers. \
         With status/phase: returns the prompts that would fire for entering that state. \
         Useful for understanding what guidance applies to a transition before making it.",
        json!({
            "status": {
                "type": "string",
                "description": "Show prompts for entering this status"
            },
            "phase": {
                "type": "string",
                "description": "Show prompts for entering this phase"
            },
            "task": {
                "type": "string",
                "description": "Task ID for template expansion context"
            },
            "worker_id": {
                "type": "string",
                "description": "Worker ID for template expansion context"
            }
        }),
        vec![],
    )]
}

/// Query workflow prompts.
///
/// Two modes:
/// 1. **List mode** (no status/phase): Returns all prompt trigger names.
/// 2. **Expand mode** (with status and/or phase): Returns the expanded prompts
///    that would fire when entering that state, with optional task/worker context
///    for template variable expansion.
pub fn get_prompts(db: &Database, workflows: &WorkflowsConfig, args: Value) -> Result<Value> {
    let status = get_string(&args, "status");
    let phase = get_string(&args, "phase");
    let task_id = get_string(&args, "task");
    let worker_id = get_string(&args, "worker_id");

    if status.is_none() && phase.is_none() {
        // List mode: return all trigger names
        let triggers = prompts::list_available_prompts(workflows);
        return Ok(json!({
            "triggers": triggers,
            "count": triggers.len(),
        }));
    }

    // Expand mode: build context and get prompts for entering the target state
    let states_config: StatesConfig = workflows.into();
    let phases_config: PhasesConfig = workflows.into();

    let target_status = status.as_deref().unwrap_or(&states_config.initial);
    let target_phase = phase.as_deref();

    // Build prompt context
    let mut ctx = PromptContext::new(target_status, target_phase, &states_config, &phases_config);

    // Enrich with task context if provided
    let task = task_id
        .as_deref()
        .and_then(|id| db.get_task(id).ok().flatten());
    let task_tags: Vec<String> = task.as_ref().map(|t| t.tags.clone()).unwrap_or_default();

    if let Some(ref t) = task {
        ctx = ctx.with_task(&t.id, &t.title, t.priority, &task_tags);
    }

    // Enrich with worker context if provided
    let worker = worker_id
        .as_deref()
        .and_then(|id| db.get_worker(id).ok().flatten());
    let worker_role = worker.as_ref().and_then(|w| workflows.match_role(&w.tags));

    if let Some(ref w) = worker {
        ctx = ctx.with_agent(&w.id, worker_role.as_deref(), &w.tags);
    }

    // Get prompts with source attribution: simulate transition from empty to target
    let attributed = prompts::get_transition_prompts_attributed(
        "",
        None,
        target_status,
        target_phase,
        workflows,
        &ctx,
    );

    // Build attributed prompt objects for JSON output
    let prompt_objects: Vec<Value> = attributed
        .iter()
        .map(|p| {
            json!({
                "text": p.text,
                "source": p.source,
            })
        })
        .collect();

    Ok(json!({
        "status": target_status,
        "phase": target_phase,
        "prompts": prompt_objects,
        "count": prompt_objects.len(),
    }))
}
