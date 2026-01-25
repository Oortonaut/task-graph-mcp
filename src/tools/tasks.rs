//! Task CRUD tools.

use super::{get_bool, get_i32, get_i64, get_string, get_string_array, make_tool_with_prompts};
use crate::config::{DependenciesConfig, Prompts, StatesConfig};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{format_task_markdown, format_tasks_markdown, markdown_to_json, OutputFormat};
use crate::types::{parse_priority, TaskTreeInput};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts, states_config: &StatesConfig) -> Vec<Tool> {
    // Generate state enum from config
    let state_names: Vec<&str> = states_config.state_names();
    let state_enum: Vec<Value> = state_names.iter().map(|s| json!(s)).collect();

    vec![
        make_tool_with_prompts(
            "create",
            "Create a new task. Use parent for subtasks. Use the link system (block tool) for dependencies.",
            json!({
                "id": {
                    "type": "string",
                    "description": "Custom task ID (optional, UUID7 generated if not provided)"
                },
                "description": {
                    "type": "string",
                    "description": "Task description (required)"
                },
                "parent": {
                    "type": "string",
                    "description": "Parent task ID for nesting"
                },
                "priority": {
                    "type": "integer",
                    "description": "Task priority as integer (higher = more important, default 0)"
                },
                "points": {
                    "type": "integer",
                    "description": "Story points / complexity estimate"
                },
                "time_estimate_ms": {
                    "type": "integer",
                    "description": "Estimated duration in milliseconds"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Categorization/discovery tags (what the task IS, for querying)"
                }
            }),
            vec!["description"],
            prompts,
        ),
        make_tool_with_prompts(
            "create_tree",
            "Create a task tree from a nested structure. Use parallel=true for parallel children, false (default) for sequential (auto-creates follows dependencies).",
            json!({
                "tree": {
                    "type": "object",
                    "description": "Nested tree structure with id (optional), description (required), children[], parallel, etc.",
                    "properties": {
                        "id": { "type": "string", "description": "Custom task ID (optional, UUID7 generated if not provided)" },
                        "description": { "type": "string", "description": "Task description (required)" },
                        "priority": { "type": "integer", "description": "Priority as integer (higher = more important)" },
                        "parallel": { "type": "boolean", "description": "If true, children run in parallel; if false (default), sequential with follows deps" },
                        "points": { "type": "integer" },
                        "time_estimate_ms": { "type": "integer" },
                        "children": { "type": "array" }
                    }
                },
                "parent": {
                    "type": "string",
                    "description": "Optional parent task ID"
                }
            }),
            vec!["tree"],
            prompts,
        ),
        make_tool_with_prompts(
            "get",
            "Get a single task by ID. Returns detailed task with attachment metadata list and counts by type.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "markdown"],
                    "description": "Output format (default: json)"
                }
            }),
            vec!["task"],
            prompts,
        ),
        make_tool_with_prompts(
            "list_tasks",
            "Query tasks with flexible filters.",
            json!({
                "status": {
                    "oneOf": [
                        { "type": "string", "enum": state_enum },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Filter by status (single or array)"
                },
                "ready": {
                    "type": "boolean",
                    "description": "Only tasks with satisfied deps and unclaimed"
                },
                "blocked": {
                    "type": "boolean",
                    "description": "Only tasks with unsatisfied deps"
                },
                "owner": {
                    "type": "string",
                    "description": "Filter by owner worker ID"
                },
                "parent": {
                    "type": "string",
                    "description": "Filter by parent task ID (use 'null' for root tasks)"
                },
                "worker_id": {
                    "type": "string",
                    "description": "With ready=true, pre-filters by worker's tags"
                },
                "tags_any": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter tasks that have ANY of these tags (OR)"
                },
                "tags_all": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter tasks that have ALL of these tags (AND)"
                },
                "qualified_for": {
                    "type": "string",
                    "description": "Filter tasks that this worker is qualified to claim (checks needed_tags/wanted_tags)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tasks to return"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "markdown"],
                    "description": "Output format (default: json)"
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "update",
            "Update a task's properties. State changes handle ownership automatically: transitioning to a timed state (e.g., in_progress) claims the task, transitioning to non-timed releases it, transitioning to terminal (e.g., completed) completes it.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID making the update"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "state": {
                    "type": "string",
                    "enum": state_enum,
                    "description": "New status"
                },
                "title": {
                    "type": "string",
                    "description": "New title"
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                },
                "priority": {
                    "type": "string",
                    "enum": ["high", "medium", "low"],
                    "description": "New priority"
                },
                "points": {
                    "type": "integer",
                    "description": "New points estimate"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "New categorization/discovery tags"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force ownership changes even if owned by another worker (default: false)"
                }
            }),
            vec!["worker_id", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "delete",
            "Delete a task.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "cascade": {
                    "type": "boolean",
                    "description": "Whether to delete children (default: false)"
                }
            }),
            vec!["task"],
            prompts,
        ),
    ]
}

pub fn create(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let id = get_string(&args, "id");
    let description = get_string(&args, "description")
        .ok_or_else(|| ToolError::missing_field("description"))?;
    let parent_id = get_string(&args, "parent");
    // Support both integer and string priority
    let priority = get_i32(&args, "priority")
        .or_else(|| get_string(&args, "priority").map(|s| parse_priority(&s)));
    let points = get_i32(&args, "points");
    let time_estimate_ms = get_i64(&args, "time_estimate_ms");
    let tags = get_string_array(&args, "tags");

    // Deferred: needed_tags and wanted_tags are not exposed in the API for now
    // They can still be set via update or task tree
    let task = db.create_task(
        id,
        description,
        parent_id,
        priority,
        points,
        time_estimate_ms,
        None, // needed_tags - deferred
        None, // wanted_tags - deferred
        tags,
        states_config,
    )?;

    Ok(json!({
        "id": &task.id,
        "description": task.description,
        "status": task.status,
        "priority": task.priority,
        "created_at": task.created_at
    }))
}

pub fn create_tree(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let tree: TaskTreeInput = serde_json::from_value(
        args.get("tree")
            .cloned()
            .ok_or_else(|| ToolError::missing_field("tree"))?,
    )?;
    let parent_id = get_string(&args, "parent");

    let (root_id, all_ids) = db.create_task_tree(tree, parent_id, states_config)?;

    Ok(json!({
        "root_task_id": root_id,
        "all_ids": all_ids
    }))
}

pub fn get(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(default_format);

    let task = db.get_task(&task_id)?
        .ok_or_else(|| ToolError::new(crate::error::ErrorCode::TaskNotFound, "Task not found"))?;

    let blocked_by = db.get_blockers(&task_id)?;

    // Get attachment metadata
    let attachments = db.get_attachments(&task_id)?;

    // Calculate attachment counts by MIME type
    let mut attachment_counts: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for att in &attachments {
        *attachment_counts.entry(att.mime_type.clone()).or_insert(0) += 1;
    }

    match format {
        OutputFormat::Markdown => {
            let mut md = format_task_markdown(&task, &blocked_by);

            // Add attachment section if there are attachments
            if !attachments.is_empty() {
                md.push_str("\n### Attachments\n");
                for att in &attachments {
                    let file_indicator = if att.file_path.is_some() { " (file)" } else { "" };
                    md.push_str(&format!("- **{}** [{}]{}\n", att.name, att.mime_type, file_indicator));
                }

                // Add counts by type
                md.push_str("\n**Counts by type:**\n");
                for (mime_type, count) in &attachment_counts {
                    md.push_str(&format!("- {}: {}\n", mime_type, count));
                }
            }

            Ok(markdown_to_json(md))
        }
        OutputFormat::Json => {
            let mut task_json = serde_json::to_value(&task)?;
            if let Some(obj) = task_json.as_object_mut() {
                obj.insert("blocked_by".to_string(), json!(blocked_by));
                obj.insert("attachments".to_string(), serde_json::to_value(&attachments)?);
                obj.insert("attachment_counts".to_string(), serde_json::to_value(&attachment_counts)?);
            }
            Ok(task_json)
        }
    }
}

pub fn list_tasks(
    db: &Database,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
    default_format: OutputFormat,
    args: Value,
) -> Result<Value> {
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(default_format);

    let ready = get_bool(&args, "ready").unwrap_or(false);
    let blocked = get_bool(&args, "blocked").unwrap_or(false);
    let limit = get_i32(&args, "limit");

    // Extract tag filtering parameters
    let tags_any = get_string_array(&args, "tags_any");
    let tags_all = get_string_array(&args, "tags_all");
    let qualified_for = get_string(&args, "qualified_for");

    // Get tasks based on filters
    let tasks = if ready {
        // Ready tasks: in initial state, unclaimed, all deps satisfied
        let worker_id = get_string(&args, "worker_id");
        db.get_ready_tasks(worker_id.as_deref(), states_config, deps_config)?
    } else if blocked {
        // Blocked tasks: have unsatisfied deps
        db.get_blocked_tasks(states_config, deps_config)?
    } else {
        // General query with filters
        // Handle status which can be string or array
        let status_vec: Option<Vec<String>> = if let Some(status_val) = args.get("status") {
            if let Some(s) = status_val.as_str() {
                Some(vec![s.to_string()])
            } else if let Some(arr) = status_val.as_array() {
                Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            } else {
                None
            }
        } else {
            None
        };
        let owner = get_string(&args, "owner");
        let parent_id_str = get_string(&args, "parent");
        let parent_id: Option<Option<&str>> = match &parent_id_str {
            Some(pid_str) if pid_str == "null" => Some(None), // Root tasks
            Some(pid_str) => Some(Some(pid_str.as_str())),
            None => None,
        };

        // Check if tag filtering is needed
        let has_tag_filters = tags_any.is_some() || tags_all.is_some() || qualified_for.is_some();

        if has_tag_filters {
            // Use the tag-filtered query
            let qualified_agent_tags = if let Some(agent_id) = &qualified_for {
                Some(db.get_agent_tags(agent_id)?)
            } else {
                None
            };

            db.list_tasks_with_tag_filters(
                status_vec,
                owner.as_deref(),
                parent_id,
                tags_any,
                tags_all,
                qualified_agent_tags,
                limit,
            )?
        } else {
            // Use list_tasks but get full Task objects (only supports single status)
            let status = status_vec.as_ref().and_then(|v| v.first().map(|s| s.as_str()));
            let summaries = db.list_tasks(status, owner.as_deref(), parent_id, limit)?;

            // Convert summaries to full tasks
            let mut tasks = Vec::new();
            for summary in summaries {
                if let Some(task) = db.get_task(&summary.id)? {
                    tasks.push(task);
                }
            }
            tasks
        }
    };

    // Apply limit
    let tasks: Vec<_> = if let Some(l) = limit {
        tasks.into_iter().take(l as usize).collect()
    } else {
        tasks
    };

    // Get blockers for each task
    let tasks_with_blockers: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let blockers = db.get_blockers(&task.id).unwrap_or_default();
            (task, blockers)
        })
        .collect();

    match format {
        OutputFormat::Markdown => Ok(markdown_to_json(format_tasks_markdown(
            &tasks_with_blockers,
            states_config,
        ))),
        OutputFormat::Json => Ok(json!({
            "tasks": tasks_with_blockers.iter().map(|(task, blockers)| {
                let mut task_json = serde_json::to_value(task).unwrap();
                if let Some(obj) = task_json.as_object_mut() {
                    obj.insert("blocked_by".to_string(), json!(blockers));
                }
                task_json
            }).collect::<Vec<_>>()
        })),
    }
}

pub fn update(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let worker_id = get_string(&args, "worker_id")
        .ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let title = get_string(&args, "title");
    let description = if args.get("description").is_some() {
        Some(get_string(&args, "description"))
    } else {
        None
    };
    let status = get_string(&args, "state");
    let priority = get_string(&args, "priority").map(|s| parse_priority(&s));
    let points = if args.get("points").is_some() {
        Some(get_i32(&args, "points"))
    } else {
        None
    };
    let tags = if args.get("tags").is_some() {
        Some(get_string_array(&args, "tags").unwrap_or_default())
    } else {
        None
    };
    let force = get_bool(&args, "force").unwrap_or(false);

    let task = db.update_task_unified(
        &task_id,
        &worker_id,
        title,
        description,
        status,
        priority,
        points,
        tags,
        force,
        states_config,
    )?;

    Ok(serde_json::to_value(task)?)
}

pub fn delete(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| ToolError::missing_field("task"))?;
    let cascade = get_bool(&args, "cascade").unwrap_or(false);

    db.delete_task(&task_id, cascade)?;

    Ok(json!({
        "success": true
    }))
}
