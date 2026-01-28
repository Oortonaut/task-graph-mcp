//! Configuration resources - expose states, phases, tags, and dependency configuration via MCP resources.
//!
//! These resources allow agents to discover valid configuration values at runtime.

use crate::config::{DependenciesConfig, PhasesConfig, StatesConfig, TagsConfig};
use anyhow::Result;
use serde_json::{Value, json};

/// Get all states configuration as JSON.
pub fn get_states_config(states_config: &StatesConfig) -> Result<Value> {
    // Build detailed state information
    let states: Vec<Value> = states_config
        .definitions
        .iter()
        .map(|(name, def)| {
            json!({
                "name": name,
                "timed": def.timed,
                "exits": def.exits,
                "terminal": def.exits.is_empty(),
                "blocking": states_config.blocking_states.contains(name),
            })
        })
        .collect();

    Ok(json!({
        "states": states,
        "initial_state": &states_config.initial,
        "disconnect_state": &states_config.disconnect_state,
        "blocking_states": &states_config.blocking_states,
        "summary": {
            "total": states_config.definitions.len(),
            "timed_count": states_config.definitions.values().filter(|d| d.timed).count(),
            "terminal_count": states_config.definitions.values().filter(|d| d.exits.is_empty()).count(),
        }
    }))
}

/// Get all phases configuration as JSON.
pub fn get_phases_config(phases_config: &PhasesConfig) -> Result<Value> {
    let mut phases: Vec<&str> = phases_config
        .definitions
        .iter()
        .map(|s| s.as_str())
        .collect();
    phases.sort(); // Sort for consistent output

    Ok(json!({
        "phases": phases,
        "unknown_phase_behavior": format!("{:?}", phases_config.unknown_phase).to_lowercase(),
        "count": phases.len(),
    }))
}

/// Get all dependency types configuration as JSON.
pub fn get_dependencies_config(deps_config: &DependenciesConfig) -> Result<Value> {
    let dep_types: Vec<Value> = deps_config
        .definitions
        .iter()
        .map(|(name, def)| {
            json!({
                "name": name,
                "display": format!("{:?}", def.display).to_lowercase(),
                "blocks": format!("{:?}", def.blocks).to_lowercase(),
            })
        })
        .collect();

    Ok(json!({
        "dependency_types": dep_types,
        "start_blocking_types": deps_config.start_blocking_types(),
        "completion_blocking_types": deps_config.completion_blocking_types(),
        "count": deps_config.definitions.len(),
    }))
}

/// Get all tags configuration as JSON.
pub fn get_tags_config(tags_config: &TagsConfig) -> Result<Value> {
    // Build detailed tag information grouped by category
    let mut tags_by_category: std::collections::HashMap<&str, Vec<Value>> =
        std::collections::HashMap::new();

    for (name, def) in &tags_config.definitions {
        let category = def.category.as_deref().unwrap_or("uncategorized");
        let tag_info = json!({
            "name": name,
            "description": def.description,
        });
        tags_by_category.entry(category).or_default().push(tag_info);
    }

    // Sort tags within each category
    for tags in tags_by_category.values_mut() {
        tags.sort_by(|a, b| {
            a.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        });
    }

    // Build flat list of all tags
    let mut all_tags: Vec<&str> = tags_config.tag_names();
    all_tags.sort();

    Ok(json!({
        "tags": all_tags,
        "by_category": tags_by_category,
        "categories": tags_config.categories(),
        "unknown_tag_behavior": format!("{:?}", tags_config.unknown_tag).to_lowercase(),
        "count": tags_config.definitions.len(),
    }))
}
