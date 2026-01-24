//! Task CRUD tools.

use super::{get_bool, get_i32, get_i64, get_string, get_string_array, get_uuid, make_tool};
use crate::db::Database;
use crate::types::{EventType, Priority, TargetType, TaskStatus, TaskTreeInput};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};

pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "create_task",
            "Create a new task.",
            json!({
                "title": {
                    "type": "string",
                    "description": "Task title"
                },
                "description": {
                    "type": "string",
                    "description": "Task description"
                },
                "parent_id": {
                    "type": "string",
                    "description": "Parent task UUID for nesting"
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
                "metadata": {
                    "type": "object",
                    "description": "Arbitrary metadata"
                }
            }),
            vec!["title"],
        ),
        make_tool(
            "create_task_tree",
            "Create a task tree from a nested structure.",
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
                "parent_id": {
                    "type": "string",
                    "description": "Optional parent task UUID"
                }
            }),
            vec!["tree"],
        ),
        make_tool(
            "get_task",
            "Get a task by ID.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                },
                "include_children": {
                    "type": "boolean",
                    "description": "Whether to include all descendants"
                }
            }),
            vec!["task_id"],
        ),
        make_tool(
            "update_task",
            "Update a task's properties.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                },
                "title": {
                    "type": "string",
                    "description": "New title"
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "cancelled"],
                    "description": "New status"
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
                "metadata": {
                    "type": "object",
                    "description": "New metadata"
                }
            }),
            vec!["task_id"],
        ),
        make_tool(
            "delete_task",
            "Delete a task.",
            json!({
                "task_id": {
                    "type": "string",
                    "description": "Task UUID"
                },
                "cascade": {
                    "type": "boolean",
                    "description": "Whether to delete children (default: false)"
                }
            }),
            vec!["task_id"],
        ),
        make_tool(
            "list_tasks",
            "List tasks with optional filters.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "cancelled"],
                    "description": "Filter by status"
                },
                "owner": {
                    "type": "string",
                    "description": "Filter by owner agent UUID"
                },
                "parent_id": {
                    "type": "string",
                    "description": "Filter by parent task UUID (use 'null' for root tasks)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tasks to return"
                }
            }),
            vec![],
        ),
    ]
}

pub fn create_task(db: &Database, args: Value) -> Result<Value> {
    let title = get_string(&args, "title")
        .ok_or_else(|| anyhow::anyhow!("title is required"))?;
    let description = get_string(&args, "description");
    let parent_id = get_uuid(&args, "parent_id");
    let priority = get_string(&args, "priority")
        .and_then(|s| Priority::from_str(&s));
    let points = get_i32(&args, "points");
    let time_estimate_ms = get_i64(&args, "time_estimate_ms");
    let needed_tags = get_string_array(&args, "needed_tags");
    let wanted_tags = get_string_array(&args, "wanted_tags");
    let metadata = args.get("metadata").cloned();

    let task = db.create_task(
        title,
        description,
        parent_id,
        priority,
        points,
        time_estimate_ms,
        needed_tags,
        wanted_tags,
        metadata,
    )?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task.id.to_string(),
        EventType::TaskCreated,
        json!({
            "task_id": task.id.to_string(),
            "title": task.title,
            "status": task.status.as_str()
        }),
    );

    Ok(json!({
        "task_id": task.id.to_string(),
        "parent_id": task.parent_id.map(|id| id.to_string()),
        "title": task.title,
        "status": task.status.as_str(),
        "priority": task.priority.as_str(),
        "created_at": task.created_at
    }))
}

pub fn create_task_tree(db: &Database, args: Value) -> Result<Value> {
    let tree: TaskTreeInput = serde_json::from_value(
        args.get("tree").cloned().ok_or_else(|| anyhow::anyhow!("tree is required"))?
    )?;
    let parent_id = get_uuid(&args, "parent_id");

    let (root_id, all_ids) = db.create_task_tree(tree, parent_id)?;

    Ok(json!({
        "root_task_id": root_id.to_string(),
        "all_ids": all_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
    }))
}

pub fn get_task(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let include_children = get_bool(&args, "include_children").unwrap_or(false);

    if include_children {
        let tree = db.get_task_tree(task_id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;
        Ok(serde_json::to_value(tree)?)
    } else {
        let task = db.get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;
        Ok(serde_json::to_value(task)?)
    }
}

pub fn update_task(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let title = get_string(&args, "title");
    let description = if args.get("description").is_some() {
        Some(get_string(&args, "description"))
    } else {
        None
    };
    let status = get_string(&args, "status")
        .and_then(|s| TaskStatus::from_str(&s));
    let priority = get_string(&args, "priority")
        .and_then(|s| Priority::from_str(&s));
    let points = if args.get("points").is_some() {
        Some(get_i32(&args, "points"))
    } else {
        None
    };
    let metadata = if args.get("metadata").is_some() {
        Some(args.get("metadata").cloned())
    } else {
        None
    };

    let task = db.update_task(task_id, title, description, status, priority, points, metadata)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task.id.to_string(),
        EventType::TaskUpdated,
        json!({
            "task_id": task.id.to_string(),
            "title": task.title,
            "status": task.status.as_str()
        }),
    );

    Ok(serde_json::to_value(task)?)
}

pub fn delete_task(db: &Database, args: Value) -> Result<Value> {
    let task_id = get_uuid(&args, "task_id")
        .ok_or_else(|| anyhow::anyhow!("task_id is required"))?;
    let cascade = get_bool(&args, "cascade").unwrap_or(false);

    db.delete_task(task_id, cascade)?;

    // Publish event for subscribers
    let _ = db.publish_event(
        TargetType::Task,
        &task_id.to_string(),
        EventType::TaskDeleted,
        json!({
            "task_id": task_id.to_string(),
            "cascade": cascade
        }),
    );

    Ok(json!({
        "success": true
    }))
}

pub fn list_tasks(db: &Database, args: Value) -> Result<Value> {
    let status = get_string(&args, "status")
        .and_then(|s| TaskStatus::from_str(&s));
    let owner = get_uuid(&args, "owner");
    let parent_id = if let Some(pid_str) = get_string(&args, "parent_id") {
        if pid_str == "null" {
            Some(None) // Root tasks
        } else {
            Some(Some(uuid::Uuid::parse_str(&pid_str)?))
        }
    } else {
        None
    };
    let limit = get_i32(&args, "limit");

    let tasks = db.list_tasks(status, owner, parent_id, limit)?;

    Ok(json!({
        "tasks": tasks
    }))
}
