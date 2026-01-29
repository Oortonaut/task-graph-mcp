//! Dependency management tools.

use super::{
    IdList, get_string, get_string_or_array, get_string_or_array_or_wildcard,
    make_tool_with_prompts,
};
use crate::config::{DependenciesConfig, Prompts};
use crate::db::{AddDependencyResult, Database};
use crate::error::{ToolError, ToolWarning};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

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
        make_tool_with_prompts(
            "relink",
            "Atomically move dependencies: unlinks prev_from→prev_to then links from→to in a single transaction. Use for moving children between parents.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID performing the relink"
                },
                "prev_from": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Previous source task ID(s) to unlink from"
                },
                "prev_to": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Previous target task ID(s) to unlink"
                },
                "from": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "New source task ID(s) to link"
                },
                "to": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "New target task ID(s) to link"
                },
                "type": {
                    "type": "string",
                    "enum": dep_types,
                    "description": "Dependency type (default: 'contains')"
                }
            }),
            vec!["prev_from", "prev_to", "from", "to"],
            prompts,
        ),
    ]
}

pub fn link(db: &Database, deps_config: &DependenciesConfig, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");

    let from_ids =
        get_string_or_array(&args, "from").ok_or_else(|| ToolError::missing_field("from"))?;
    let to_ids = get_string_or_array(&args, "to").ok_or_else(|| ToolError::missing_field("to"))?;

    if from_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'from' task ID must be provided",
        )
        .into());
    }
    if to_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'to' task ID must be provided",
        )
        .into());
    }

    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    let mut created = Vec::new();
    let mut warnings: Vec<ToolWarning> = Vec::new();
    let mut errors = Vec::new();

    // Create all combinations of from x to
    for from_id in &from_ids {
        for to_id in &to_ids {
            match db.add_dependency_soft(from_id, to_id, &dep_type, deps_config) {
                Ok(AddDependencyResult::Created) => created.push(json!({
                    "from": from_id,
                    "to": to_id,
                    "type": &dep_type
                })),
                Ok(AddDependencyResult::AlreadyExists) => {
                    warnings.push(ToolWarning::duplicate(&format!(
                        "dependency {} -> {}",
                        from_id, to_id
                    )));
                }
                Ok(AddDependencyResult::FromTaskNotFound) => {
                    warnings.push(ToolWarning::task_not_found(from_id).with_field("from"));
                }
                Ok(AddDependencyResult::ToTaskNotFound) => {
                    warnings.push(ToolWarning::dependency_not_found(to_id, "to"));
                }
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
        "warnings": warnings,
        "errors": errors,
        "type": dep_type
    }))
}

pub fn unlink(db: &Database, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");

    let from_parsed = get_string_or_array_or_wildcard(&args, "from")
        .ok_or_else(|| ToolError::missing_field("from"))?;
    let to_parsed = get_string_or_array_or_wildcard(&args, "to")
        .ok_or_else(|| ToolError::missing_field("to"))?;

    let from_is_wildcard = matches!(&from_parsed, IdList::Wildcard);
    let to_is_wildcard = matches!(&to_parsed, IdList::Wildcard);

    if from_is_wildcard && to_is_wildcard {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "Cannot use wildcard '*' for both 'from' and 'to'",
        )
        .into());
    }

    let from_ids = match &from_parsed {
        IdList::Ids(ids) => ids,
        IdList::Wildcard => &vec![],
    };
    let to_ids = match &to_parsed {
        IdList::Ids(ids) => ids,
        IdList::Wildcard => &vec![],
    };

    if !from_is_wildcard && from_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'from' task ID must be provided",
        )
        .into());
    }
    if !to_is_wildcard && to_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'to' task ID must be provided",
        )
        .into());
    }

    let dep_type = get_string(&args, "type").unwrap_or_else(|| "blocks".to_string());

    let mut removed = Vec::new();
    let mut errors = Vec::new();

    if to_is_wildcard {
        // Remove all outgoing dependencies from the specified tasks
        for from_id in from_ids {
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
        for to_id in to_ids {
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
        for from_id in from_ids {
            for to_id in to_ids {
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

pub fn relink(db: &Database, deps_config: &DependenciesConfig, args: Value) -> Result<Value> {
    // Agent parameter is optional - for tracking/audit purposes
    let _agent_id = get_string(&args, "agent");

    let prev_from_ids = get_string_or_array(&args, "prev_from")
        .ok_or_else(|| ToolError::missing_field("prev_from"))?;
    let prev_to_ids =
        get_string_or_array(&args, "prev_to").ok_or_else(|| ToolError::missing_field("prev_to"))?;
    let from_ids =
        get_string_or_array(&args, "from").ok_or_else(|| ToolError::missing_field("from"))?;
    let to_ids = get_string_or_array(&args, "to").ok_or_else(|| ToolError::missing_field("to"))?;

    if prev_from_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'prev_from' task ID must be provided",
        )
        .into());
    }
    if prev_to_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'prev_to' task ID must be provided",
        )
        .into());
    }
    if from_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'from' task ID must be provided",
        )
        .into());
    }
    if to_ids.is_empty() {
        return Err(ToolError::new(
            crate::error::ErrorCode::InvalidFieldValue,
            "At least one 'to' task ID must be provided",
        )
        .into());
    }

    // Default to 'contains' for relink (moving children between parents)
    let dep_type = get_string(&args, "type").unwrap_or_else(|| "contains".to_string());

    match db.relink(
        &prev_from_ids,
        &prev_to_ids,
        &from_ids,
        &to_ids,
        &dep_type,
        deps_config,
    ) {
        Ok(result) => {
            let unlinked: Vec<Value> = result
                .unlinked
                .iter()
                .map(|(from, to)| json!({"from": from, "to": to}))
                .collect();
            let linked: Vec<Value> = result
                .linked
                .iter()
                .map(|(from, to)| json!({"from": from, "to": to}))
                .collect();

            Ok(json!({
                "success": true,
                "unlinked": unlinked,
                "unlinked_count": unlinked.len(),
                "linked": linked,
                "linked_count": linked.len(),
                "type": dep_type
            }))
        }
        Err(e) => Ok(json!({
            "success": false,
            "error": e.to_string(),
            "type": dep_type
        })),
    }
}
