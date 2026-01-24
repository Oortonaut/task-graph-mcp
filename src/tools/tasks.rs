//! Task CRUD tools.

use super::{get_bool, get_i32, get_i64, get_string, get_string_array, make_tool_with_prompts};
use crate::config::Prompts;
use crate::db::Database;
use crate::format::{format_task_markdown, format_tasks_markdown, markdown_to_json, OutputFormat};
use crate::types::{Priority, TaskStatus, TaskTreeInput};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools(prompts: &Prompts) -> Vec<Tool> {
    vec![
        make_tool_with_prompts(
            "create",
            "Create a new task. Use parent for subtasks. Set needed_tags (AND) or wanted_tags (OR) to restrict which agents can claim it. Use blocked_by to set initial dependencies.",
            json!({
                "title": {
                    "type": "string",
                    "description": "Task title"
                },
                "description": {
                    "type": "string",
                    "description": "Task description"
                },
                "parent": {
                    "type": "string",
                    "description": "Parent task ID for nesting"
                },
                "priority": {
                    "type": "string",
                    "enum": ["high", "medium", "low"],
                    "description": "Task priority"
                },
                "points": {
                    "type": "integer",
                    "description": "Story points / complexity estimate"
                },
                "time_estimate_ms": {
                    "type": "integer",
                    "description": "Estimated duration in milliseconds"
                },
                "needed_tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags agent must have ALL of to claim (AND)"
                },
                "wanted_tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags agent must have AT LEAST ONE of to claim (OR)"
                },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that must complete before this task can be claimed"
                }
            }),
            vec!["title"],
            prompts,
        ),
        make_tool_with_prompts(
            "create_tree",
            "Create a task tree from a nested structure. Use join_mode='then' for sequential children (auto-creates dependencies), 'also' for parallel.",
            json!({
                "tree": {
                    "type": "object",
                    "description": "Nested tree structure with title, children[], join_mode, etc.",
                    "properties": {
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "priority": { "type": "string" },
                        "join_mode": { "type": "string", "enum": ["then", "also"] },
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
            "Get a single task by ID with optional children and formatting.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "children": {
                    "type": "boolean",
                    "description": "Whether to include all descendants"
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
                        { "type": "string", "enum": ["pending", "in_progress", "completed", "failed", "cancelled"] },
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
                    "description": "Filter by owner agent ID"
                },
                "parent": {
                    "type": "string",
                    "description": "Filter by parent task ID (use 'null' for root tasks)"
                },
                "agent": {
                    "type": "string",
                    "description": "With ready=true, pre-filters by agent's tags"
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
            "Update a task's properties.",
            json!({
                "agent": {
                    "type": "string",
                    "description": "Agent ID making the update"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "state": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "cancelled"],
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
                }
            }),
            vec!["agent", "task", "state"],
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

pub fn create(db: &Database, args: Value) -> Result<Value> {
    let title = get_string(&args, "title")
        .ok_or_else(|| anyhow::anyhow!("title is required"))?;
    let description = get_string(&args, "description");
    let parent_id = get_string(&args, "parent");
    let priority = get_string(&args, "priority")
        .and_then(|s| Priority::from_str(&s));
    let points = get_i32(&args, "points");
    let time_estimate_ms = get_i64(&args, "time_estimate_ms");
    let needed_tags = get_string_array(&args, "needed_tags");
    let wanted_tags = get_string_array(&args, "wanted_tags");
    let blocked_by = get_string_array(&args, "blocked_by");

    let task = db.create_task(
        title,
        description,
        parent_id,
        priority,
        points,
        time_estimate_ms,
        needed_tags,
        wanted_tags,
        blocked_by,
    )?;

    Ok(json!({
        "task_id": &task.id,
        "parent_id": task.parent_id,
        "title": task.title,
        "status": task.status.as_str(),
        "priority": task.priority.as_str(),
        "created_at": task.created_at
    }))
}

pub fn create_tree(db: &Database, args: Value) -> Result<Value> {
    let tree: TaskTreeInput = serde_json::from_value(
        args.get("tree").cloned().ok_or_else(|| anyhow::anyhow!("tree is required"))?
    )?;
    let parent_id = get_string(&args, "parent");

    let (root_id, all_ids) = db.create_task_tree(tree, parent_id)?;

    Ok(json!({
        "root_task_id": root_id,
        "all_ids": all_ids
    }))
}

pub fn get(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;
    let include_children = get_bool(&args, "children").unwrap_or(false);
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(OutputFormat::Json);

    if include_children {
        let tree = db.get_task_tree(&task_id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;
        Ok(serde_json::to_value(tree)?)
    } else {
        let task = db.get_task(&task_id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

        let blocked_by = db.get_blockers(&task_id)?;

        match format {
            OutputFormat::Markdown => {
                Ok(markdown_to_json(format_task_markdown(&task, &blocked_by)))
            }
            OutputFormat::Json => {
                let mut task_json = serde_json::to_value(&task)?;
                if let Some(obj) = task_json.as_object_mut() {
                    obj.insert("blocked_by".to_string(), json!(blocked_by));
                }
                Ok(task_json)
            }
        }
    }
}

pub fn list_tasks(db: &Database, args: Value) -> Result<Value> {
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::from_str(&s))
        .unwrap_or(OutputFormat::Json);

    let ready = get_bool(&args, "ready").unwrap_or(false);
    let blocked = get_bool(&args, "blocked").unwrap_or(false);
    let limit = get_i32(&args, "limit");

    // Get tasks based on filters
    let tasks = if ready {
        // Ready tasks: pending, unclaimed, all deps satisfied
        let agent_id = get_string(&args, "agent");
        db.get_ready_tasks(agent_id.as_deref())?
    } else if blocked {
        // Blocked tasks: have unsatisfied deps
        db.get_blocked_tasks()?
    } else {
        // General query with filters
        let status = get_string(&args, "status")
            .and_then(|s| TaskStatus::from_str(&s));
        let owner = get_string(&args, "owner");
        let parent_id_str = get_string(&args, "parent");
        let parent_id: Option<Option<&str>> = match &parent_id_str {
            Some(pid_str) if pid_str == "null" => Some(None), // Root tasks
            Some(pid_str) => Some(Some(pid_str.as_str())),
            None => None,
        };

        // Use list_tasks but get full Task objects
        let summaries = db.list_tasks(status, owner.as_deref(), parent_id, limit)?;

        // Convert summaries to full tasks
        let mut tasks = Vec::new();
        for summary in summaries {
            if let Some(task) = db.get_task(&summary.id)? {
                tasks.push(task);
            }
        }
        tasks
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
        OutputFormat::Markdown => {
            Ok(markdown_to_json(format_tasks_markdown(&tasks_with_blockers)))
        }
        OutputFormat::Json => {
            Ok(json!({
                "tasks": tasks_with_blockers.iter().map(|(task, blockers)| {
                    let mut task_json = serde_json::to_value(task).unwrap();
                    if let Some(obj) = task_json.as_object_mut() {
                        obj.insert("blocked_by".to_string(), json!(blockers));
                    }
                    task_json
                }).collect::<Vec<_>>()
            }))
        }
    }
}

pub fn update(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;
    let title = get_string(&args, "title");
    let description = if args.get("description").is_some() {
        Some(get_string(&args, "description"))
    } else {
        None
    };
    let status = get_string(&args, "state")
        .and_then(|s| TaskStatus::from_str(&s));
    let priority = get_string(&args, "priority")
        .and_then(|s| Priority::from_str(&s));
    let points = if args.get("points").is_some() {
        Some(get_i32(&args, "points"))
    } else {
        None
    };

    let task = db.update_task(&task_id, title, description, status, priority, points)?;

    Ok(serde_json::to_value(task)?)
}

pub fn delete(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task")
        .ok_or_else(|| anyhow::anyhow!("task is required"))?;
    let cascade = get_bool(&args, "cascade").unwrap_or(false);

    db.delete_task(&task_id, cascade)?;

    Ok(json!({
        "success": true
    }))
}
