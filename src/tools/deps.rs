//! Dependency management tools.

use super::{get_string, make_tool_with_prompts};
use crate::config::{DependenciesConfig, Prompts};
use crate::db::Database;
use crate::error::ToolError;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts, deps_config: &DependenciesConfig) -> Vec<Tool> {
    // Build enum of dependency types from config
    let dep_types: Vec<Value> = deps_config
        .dep_type_names()
        .into_iter()
        .map(|s| json!(s))
        .collect();

    vec![
        make_tool_with_prompts(
            "link",
            "Create dependency links between tasks. Supports bulk: from and to accept string or array. Example: link(from=['A','B'], to='C', type='blocks') creates A->C and B->C dependencies.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID creating the link"
                },
                "from": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Source task ID(s) - the task(s) that block/precede"
                },
                "to": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Target task ID(s) - the task(s) that are blocked/follow"
                },
                "type": {
                    "type": "string",
                    "enum": dep_types,
                    "description": "Dependency type (default: 'blocks')"
                }
            }),
            vec!["from", "to"],
            prompts,
        ),
        make_tool_with_prompts(
            "unlink",
            "Remove dependency links between tasks. Supports bulk: from and to accept string or array. Use '*' as wildcard to unlink all (e.g., from='taskA', to='*' removes all outgoing deps from taskA).",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID removing the link"
                },
                "from": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Source task ID(s), or '*' to match all"
                },
                "to": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Target task ID(s), or '*' to match all"
                },
                "type": {
                    "type": "string",
                    "enum": dep_types,
                    "description": "Dependency type (default: 'blocks')"
                }
            }),
            vec!["from", "to"],
            prompts,
        ),
    ]
}

pub fn link(db: &Database, deps_config: &DependenciesConfig, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");
    
    // Parse from: string or array of strings
    let from_ids: Vec<String> = if let Some(from_array) = args.get("from").and_then(|v| v.as_array()) {
        from_array
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(from_id) = get_string(&args, "from") {
        vec![from_id]
    } else {
        return Err(ToolError::missing_field("from").into());
    };

    // Parse to: string or array of strings
    let to_ids: Vec<String> = if let Some(to_array) = args.get("to").and_then(|v| v.as_array()) {
        to_array
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(to_id) = get_string(&args, "to") {
        vec![to_id]
    } else {
        return Err(ToolError::missing_field("to").into());
    };

    if from_ids.is_empty() {
        return Err(ToolError::new(crate::error::ErrorCode::InvalidFieldValue, "At least one 'from' task ID must be provided").into());
    }
    if to_ids.is_empty() {
        return Err(ToolError::new(crate::error::ErrorCode::InvalidFieldValue, "At least one 'to' task ID must be provided").into());
    }

    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    let mut created = Vec::new();
    let mut errors = Vec::new();

    // Create all combinations of from x to
    for from_id in &from_ids {
        for to_id in &to_ids {
            match db.add_dependency(from_id, to_id, &dep_type, deps_config) {
                Ok(()) => created.push(json!({
                    "from": from_id,
                    "to": to_id,
                    "type": &dep_type
                })),
                Err(e) => errors.push(json!({
                    "from": from_id,
                    "to": to_id,
                    "error": e.to_string()
                })),
            }
        }
    }

    Ok(json!({
        "success": errors.is_empty(),
        "created": created,
        "errors": errors,
        "type": dep_type
    }))
}

pub fn unlink(db: &Database, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");
    
    // Parse from: string or array of strings
    let from_ids: Vec<String> = if let Some(from_array) = args.get("from").and_then(|v| v.as_array()) {
        from_array
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(from_id) = get_string(&args, "from") {
        vec![from_id]
    } else {
        return Err(ToolError::missing_field("from").into());
    };

    // Parse to: string or array of strings
    let to_ids: Vec<String> = if let Some(to_array) = args.get("to").and_then(|v| v.as_array()) {
        to_array
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(to_id) = get_string(&args, "to") {
        vec![to_id]
    } else {
        return Err(ToolError::missing_field("to").into());
    };

    if from_ids.is_empty() {
        return Err(ToolError::new(crate::error::ErrorCode::InvalidFieldValue, "At least one 'from' task ID must be provided").into());
    }
    if to_ids.is_empty() {
        return Err(ToolError::new(crate::error::ErrorCode::InvalidFieldValue, "At least one 'to' task ID must be provided").into());
    }

    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    let mut removed = Vec::new();
    let mut errors = Vec::new();

    // Check for wildcard patterns
    let from_is_wildcard = from_ids.len() == 1 && from_ids[0] == "*";
    let to_is_wildcard = to_ids.len() == 1 && to_ids[0] == "*";

    if from_is_wildcard && to_is_wildcard {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "Cannot use wildcard '*' for both 'from' and 'to'"
        ).into());
    }

    if to_is_wildcard {
        // Remove all outgoing dependencies from the specified tasks
        for from_id in &from_ids {
            match db.remove_all_outgoing_dependencies(from_id, &dep_type) {
                Ok(deps) => {
                    for dep in deps {
                        removed.push(json!({
                            "from": dep.from_task_id,
                            "to": dep.to_task_id,
                            "type": dep.dep_type
                        }));
                    }
                }
                Err(e) => errors.push(json!({
                    "from": from_id,
                    "to": "*",
                    "error": e.to_string()
                })),
            }
        }
    } else if from_is_wildcard {
        // Remove all incoming dependencies to the specified tasks
        for to_id in &to_ids {
            match db.remove_all_incoming_dependencies(to_id, &dep_type) {
                Ok(deps) => {
                    for dep in deps {
                        removed.push(json!({
                            "from": dep.from_task_id,
                            "to": dep.to_task_id,
                            "type": dep.dep_type
                        }));
                    }
                }
                Err(e) => errors.push(json!({
                    "from": "*",
                    "to": to_id,
                    "error": e.to_string()
                })),
            }
        }
    } else {
        // Remove specific combinations of from x to
        for from_id in &from_ids {
            for to_id in &to_ids {
                match db.remove_dependency(from_id, to_id, &dep_type) {
                    Ok(was_removed) => {
                        if was_removed {
                            removed.push(json!({
                                "from": from_id,
                                "to": to_id,
                                "type": &dep_type
                            }));
                        }
                    }
                    Err(e) => errors.push(json!({
                        "from": from_id,
                        "to": to_id,
                        "error": e.to_string()
                    })),
                }
            }
        }
    }

    Ok(json!({
        "success": errors.is_empty(),
        "removed": removed,
        "removed_count": removed.len(),
        "errors": errors
    }))
}
