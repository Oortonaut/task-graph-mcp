//! Workflow and overlay resources - expose available workflows and overlays via MCP resources.
//!
//! These resources allow agents to discover available workflows and overlays at runtime,
//! including descriptions of what each is designed for.

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

    // Include overlays
    let mut overlay_list: Vec<Value> = workflows
        .named_overlays
        .iter()
        .map(|(name, config)| {
            json!({
                "name": name,
                "description": config.description,
            })
        })
        .collect();

    overlay_list.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });

    Ok(json!({
        "workflows": workflow_list,
        "overlays": overlay_list,
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

/// List all available overlays with their metadata.
pub fn list_overlays(workflows: &WorkflowsConfig) -> Result<Value> {
    let mut overlay_list: Vec<Value> = Vec::new();

    for (name, config) in &workflows.named_overlays {
        let source = config.source_file.as_ref().map(|p| p.display().to_string());

        overlay_list.push(json!({
            "name": name,
            "description": config.description,
            "source_file": source,
        }));
    }

    // Sort by name for consistent output
    overlay_list.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });

    Ok(json!({
        "overlays": overlay_list,
        "count": workflows.named_overlays.len(),
    }))
}

/// Get detailed information about a specific overlay.
pub fn get_overlay(workflows: &WorkflowsConfig, name: &str) -> Result<Value> {
    let config = workflows
        .named_overlays
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Overlay '{}' not found", name))?;

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

    // Build gate details
    let gates: Vec<Value> = config
        .gates
        .iter()
        .map(|(gate_key, gate_defs)| {
            let defs: Vec<Value> = gate_defs
                .iter()
                .map(|g| {
                    json!({
                        "type": g.gate_type,
                        "enforcement": format!("{:?}", g.enforcement),
                        "description": g.description,
                    })
                })
                .collect();
            json!({
                "key": gate_key,
                "definitions": defs,
            })
        })
        .collect();

    // Build role details
    let roles: Vec<Value> = config
        .roles
        .iter()
        .map(|(role_name, role)| {
            json!({
                "name": role_name,
                "description": role.description,
                "tags": role.tags,
                "max_claims": role.max_claims,
                "can_assign": role.can_assign,
                "can_create_subtasks": role.can_create_subtasks,
            })
        })
        .collect();

    // Build advisory details
    let advisories: Vec<Value> = config
        .advisories
        .iter()
        .map(|(topic, advisory)| {
            json!({
                "topic": topic,
                "level": advisory.level,
                "phase": advisory.phase,
                "role": advisory.role,
                "domain": advisory.domain,
                "content": advisory.content,
            })
        })
        .collect();

    // Build role_prompts details
    let role_prompts: Value = config
        .role_prompts
        .iter()
        .map(|(role_name, prompts)| (role_name.clone(), json!(prompts)))
        .collect::<serde_json::Map<String, Value>>()
        .into();

    Ok(json!({
        "name": name,
        "description": config.description,
        "source_file": source,
        "states": states,
        "phases": phases,
        "gates": gates,
        "roles": roles,
        "advisories": advisories,
        "role_prompts": role_prompts,
        "combo_count": config.combos.len(),
    }))
}
