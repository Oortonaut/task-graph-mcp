//! Workflow discovery tool for listing available workflows before connecting.

use super::make_tool;
use crate::config::workflows::WorkflowsConfig;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Get workflow discovery tools.
pub fn get_tools() -> Vec<Tool> {
    vec![make_tool(
        "list_workflows",
        "List all available workflows. Returns name + description for each workflow. \
         Call BEFORE connecting to discover which workflows exist. \
         Use the workflow name with the `connect` tool's `workflow` parameter.",
        json!({}),
        vec![],
    )]
}

/// List available workflows from the named_workflows cache.
pub fn list_workflows(workflows: &WorkflowsConfig) -> Result<Value> {
    let mut entries: Vec<Value> = workflows
        .named_workflows
        .iter()
        .map(|(key, config)| {
            json!({
                "name": key,
                "description": config.description.as_deref().unwrap_or(""),
            })
        })
        .collect();

    // Sort by name for consistent output
    entries.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    // Collect overlays
    let mut overlay_entries: Vec<Value> = workflows
        .named_overlays
        .iter()
        .map(|(key, config)| {
            json!({
                "name": key,
                "description": config.description.as_deref().unwrap_or(""),
            })
        })
        .collect();

    overlay_entries.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    Ok(json!({
        "count": entries.len(),
        "workflows": entries,
        "overlays": overlay_entries,
    }))
}
