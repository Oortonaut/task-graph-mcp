//! Workflow resources - expose available workflows and their metadata via MCP resources.
//!
//! These resources allow agents to discover available workflows at runtime,
//! including descriptions of what each workflow is designed for.

use crate::config::workflows::WorkflowsConfig;
use anyhow::Result;
use serde_json::{Value, json};

/// List all available workflows with their metadata.
pub fn list_workflows(workflows: &WorkflowsConfig) -> Result<Value> {
    let mut workflow_list: Vec<Value> = Vec::new();

    for (name, config) in &workflows.named_workflows {
        let source = config.source_file.as_ref().map(|p| p.display().to_string());

        workflow_list.push(json!({
            "name": name,
            "description": config.description,
            "source_file": source,
            "states": config.states.keys().collect::<Vec<_>>(),
            "phases": config.phases.keys().collect::<Vec<_>>(),
        }));
    }

    // Sort by name for consistent output
    workflow_list.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });

    // Include info about default workflow if configured
    let default_workflow = workflows.default_workflow_key.as_ref();

    Ok(json!({
        "workflows": workflow_list,
        "default_workflow": default_workflow,
        "count": workflows.named_workflows.len(),
    }))
}

/// Get detailed information about a specific workflow.
pub fn get_workflow(workflows: &WorkflowsConfig, name: &str) -> Result<Value> {
    let config = workflows
        .named_workflows
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found", name))?;

    let source = config.source_file.as_ref().map(|p| p.display().to_string());

    // Build state details
    let states: Vec<Value> = config
        .states
        .iter()
        .map(|(state_name, state)| {
            json!({
                "name": state_name,
                "exits": state.exits,
                "timed": state.timed,
                "has_enter_prompt": state.prompts.enter.is_some(),
                "has_exit_prompt": state.prompts.exit.is_some(),
            })
        })
        .collect();

    // Build phase details
    let phases: Vec<Value> = config
        .phases
        .iter()
        .map(|(phase_name, phase)| {
            json!({
                "name": phase_name,
                "has_enter_prompt": phase.prompts.enter.is_some(),
                "has_exit_prompt": phase.prompts.exit.is_some(),
            })
        })
        .collect();

    Ok(json!({
        "name": name,
        "description": config.description,
        "source_file": source,
        "settings": {
            "initial_state": config.settings.initial_state,
            "disconnect_state": config.settings.disconnect_state,
            "blocking_states": config.settings.blocking_states,
        },
        "states": states,
        "phases": phases,
        "combo_count": config.combos.len(),
    }))
}
