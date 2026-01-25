//! Task CRUD tools.

use super::{get_bool, get_i32, get_i64, get_string, get_string_array, make_tool_with_prompts};
use crate::config::{
    AttachmentsConfig, AutoAdvanceConfig, DependenciesConfig, Prompts, StatesConfig,
    UnknownKeyBehavior,
};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{
    OutputFormat, format_scan_result_markdown, format_task_markdown, format_tasks_markdown,
    markdown_to_json,
};
use crate::types::{ScanResult, TaskTreeInput, parse_priority};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

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
                    "description": "Task priority 0-10 (higher = more important, default 5)"
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
            "Create a task tree from nested structure. child_type (default 'contains') links parent→children, sibling_type ('follows' or null) links siblings. Use 'ref' in nodes to include existing tasks.",
            json!({
                "tree": {
                    "type": "object",
                    "description": "Nested tree structure with title, children[], etc. Use 'ref' to reference existing tasks.",
                    "properties": {
                        "ref": { "type": "string", "description": "Reference to an existing task ID (other fields ignored when set)" },
                        "id": { "type": "string", "description": "Custom task ID (optional, UUID7 generated if not provided)" },
                        "title": { "type": "string", "description": "Task title (required for new tasks)" },
                        "description": { "type": "string", "description": "Task description" },
                        "priority": { "type": "integer", "description": "Task priority 0-10 (default 5)" },
                        "points": { "type": "integer", "description": "Story points / complexity estimate" },
                        "time_estimate_ms": { "type": "integer", "description": "Estimated duration in milliseconds" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Categorization/discovery tags" },
                        "needed_tags": { "type": "array", "items": { "type": "string" }, "description": "Tags agent must have ALL of to claim (AND)" },
                        "wanted_tags": { "type": "array", "items": { "type": "string" }, "description": "Tags agent must have AT LEAST ONE of to claim (OR)" },
                        "children": { "type": "array", "description": "Child nodes (same structure, recursive)" }
                    }
                },
                "parent": {
                    "type": "string",
                    "description": "Optional parent task ID for the tree root"
                },
                "child_type": {
                    "type": "string",
                    "description": "Dependency type from parent to children (default: 'contains'). Set to null for no parent-child deps."
                },
                "sibling_type": {
                    "type": "string",
                    "description": "Dependency type between consecutive siblings (default: null/parallel). Use 'follows' for sequential."
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
                    "description": "Filter for claimable tasks: in initial status, unclaimed, all start-blocking deps satisfied. When combined with 'agent', also filters by agent's tag qualifications."
                },
                "blocked": {
                    "type": "boolean",
                    "description": "Filter for blocked tasks: have unsatisfied start-blocking dependencies"
                },
                "claimed": {
                    "type": "boolean",
                    "description": "Filter for claimed tasks: currently owned by any agent (owner_agent IS NOT NULL)"
                },
                "owner": {
                    "type": "string",
                    "description": "Filter by owner agent ID (tasks currently claimed by this specific agent)"
                },
                "parent": {
                    "type": "string",
                    "description": "Filter by parent task ID (use 'null' for root tasks)"
                },
                "agent": {
                    "type": "string",
                    "description": "Agent ID for filtering. With ready=true, filters tasks the agent is qualified to claim based on agent_tags_all/agent_tags_any requirements."
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
                "sort_by": {
                    "type": "string",
                    "enum": ["priority", "created_at", "updated_at"],
                    "description": "Field to sort by (default: created_at for general queries, priority then created_at for ready queries)"
                },
                "sort_order": {
                    "type": "string",
                    "enum": ["asc", "desc"],
                    "description": "Sort order: 'asc' for ascending, 'desc' for descending (default: desc for created_at/updated_at, priority always high-to-low)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tasks to return"
                }
            }),
            vec![],
            prompts,
        ),
        make_tool_with_prompts(
            "update",
            "Update a task's properties. Status changes handle ownership automatically: transitioning to a timed status (e.g., in_progress) claims the task, transitioning to non-timed releases it, transitioning to terminal (e.g., completed) completes it. For push coordination: use assignee to assign a task to another agent (sets owner and transitions to 'assigned' status). Only the owner can update a claimed task unless force=true.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID making the update"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "assignee": {
                    "type": "string",
                    "description": "Agent ID to assign the task to (push coordination). Sets owner_agent to assignee and transitions to 'assigned' status. The assignee can then claim (transition to in_progress) when ready."
                },
                "status": {
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
                    "type": "integer",
                    "description": "New priority 0-10 (higher = more important)"
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
                "time_estimate_ms": {
                    "type": "integer",
                    "description": "Estimated duration in milliseconds"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for the update (stored in audit trail for state transitions)"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force ownership changes even if owned by another worker (default: false)"
                },
                "attachments": {
                    "type": "array",
                    "description": "List of attachments to add to the task (e.g., commit hashes, changelists, notes)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Attachment name/key (e.g., 'commit', 'changelist', 'note')"
                            },
                            "content": {
                                "type": "string",
                                "description": "Attachment content (text)"
                            },
                            "mime": {
                                "type": "string",
                                "description": "MIME type (uses configured default if omitted)"
                            },
                            "mode": {
                                "type": "string",
                                "enum": ["append", "replace"],
                                "description": "How to handle existing attachment with same name (uses configured default if omitted)"
                            }
                        },
                        "required": ["name", "content"]
                    }
                }
            }),
            vec!["worker_id", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "delete",
            "Delete a task. Soft deletes by default (sets deleted_at), use obliterate=true to permanently remove. Rejects if task is claimed by another worker unless force=true.",
            json!({
                "worker_id": {
                    "type": "string",
                    "description": "Worker ID attempting to delete"
                },
                "task": {
                    "type": "string",
                    "description": "Task ID"
                },
                "cascade": {
                    "type": "boolean",
                    "description": "Whether to delete children (default: false)"
                },
                "reason": {
                    "type": "string",
                    "description": "Optional reason for deletion"
                },
                "obliterate": {
                    "type": "boolean",
                    "description": "If true, permanently deletes the task from the database. If false (default), soft deletes by setting deleted_at timestamp."
                },
                "force": {
                    "type": "boolean",
                    "description": "Force deletion even if claimed by another worker (default: false)"
                }
            }),
            vec!["worker_id", "task"],
            prompts,
        ),
        make_tool_with_prompts(
            "scan",
            "Scan the task graph from a starting task in multiple directions. Returns related tasks organized by direction: before (predecessors via blocks/follows), after (successors), above (ancestors via contains), below (descendants). Each direction has depth control: 0=none, N=levels, -1=all.",
            json!({
                "task": {
                    "type": "string",
                    "description": "Task ID to scan from"
                },
                "before": {
                    "type": "integer",
                    "description": "Depth for predecessors (tasks that block this one): 0=none, N=levels, -1=all (default: 0)"
                },
                "after": {
                    "type": "integer",
                    "description": "Depth for successors (tasks this one blocks): 0=none, N=levels, -1=all (default: 0)"
                },
                "above": {
                    "type": "integer",
                    "description": "Depth for ancestors (parent chain): 0=none, N=levels, -1=all (default: 0)"
                },
                "below": {
                    "type": "integer",
                    "description": "Depth for descendants (children tree): 0=none, N=levels, -1=all (default: 0)"
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
    ]
}

pub fn create(db: &Database, states_config: &StatesConfig, args: Value) -> Result<Value> {
    let id = get_string(&args, "id");
    let description =
        get_string(&args, "description").ok_or_else(|| ToolError::missing_field("description"))?;
    let parent_id = get_string(&args, "parent");
    // Support both integer and string priority
    let priority = get_i32(&args, "priority")
        .or_else(|| get_string(&args, "priority").map(|s| parse_priority(&s)));
    let points = get_i32(&args, "points");
    let time_estimate_ms = get_i64(&args, "time_estimate_ms");
    let tags = get_string_array(&args, "tags");
    let needed_tags = get_string_array(&args, "needed_tags");
    let wanted_tags = get_string_array(&args, "wanted_tags");

    let task = db.create_task(
        id,
        description,
        parent_id,
        priority,
        points,
        time_estimate_ms,
        needed_tags,
        wanted_tags,
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
    let child_type = get_string(&args, "child_type");
    let sibling_type = get_string(&args, "sibling_type");

    let (root_id, all_ids) =
        db.create_task_tree(tree, parent_id, child_type, sibling_type, states_config)?;

    // Fetch the root task to return full details
    let root_task = db.get_task(&root_id)?.ok_or_else(|| {
        ToolError::new(
            crate::error::ErrorCode::TaskNotFound,
            "Root task not found after creation",
        )
    })?;

    Ok(json!({
        "root": {
            "id": root_task.id,
            "title": root_task.title,
            "description": root_task.description,
            "status": root_task.status,
            "priority": root_task.priority,
            "created_at": root_task.created_at
        },
        "all_ids": all_ids,
        "count": all_ids.len()
    }))
}

pub fn get(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    let task = db
        .get_task(&task_id)?
        .ok_or_else(|| ToolError::new(crate::error::ErrorCode::TaskNotFound, "Task not found"))?;

    let blocked_by = db.get_blockers(&task_id)?;

    // Get attachment metadata
    let attachments = db.get_attachments(&task_id)?;

    // Calculate attachment counts by MIME type
    let mut attachment_counts: std::collections::HashMap<String, i32> =
        std::collections::HashMap::new();
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
                    let file_indicator = if att.file_path.is_some() {
                        " (file)"
                    } else {
                        ""
                    };
                    md.push_str(&format!(
                        "- **{}** [{}]{}\n",
                        att.name, att.mime_type, file_indicator
                    ));
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
                obj.insert(
                    "attachments".to_string(),
                    serde_json::to_value(&attachments)?,
                );
                obj.insert(
                    "attachment_counts".to_string(),
                    serde_json::to_value(&attachment_counts)?,
                );
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
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    let ready = get_bool(&args, "ready").unwrap_or(false);
    let blocked = get_bool(&args, "blocked").unwrap_or(false);
    let claimed = get_bool(&args, "claimed").unwrap_or(false);
    let limit = get_i32(&args, "limit");

    // Extract tag filtering parameters
    let tags_any = get_string_array(&args, "tags_any");
    let tags_all = get_string_array(&args, "tags_all");

    // 'agent' replaces both 'worker_id' and 'qualified_for' - single param for agent-related filtering
    let agent_id = get_string(&args, "agent");

    // Sorting parameters
    let sort_by = get_string(&args, "sort_by");
    let sort_order = get_string(&args, "sort_order");

    // Get tasks based on filters
    let mut tasks = if ready {
        // Ready tasks: in initial state, unclaimed, all deps satisfied
        // If agent is provided, also filter by agent's tag qualifications
        db.get_ready_tasks(
            agent_id.as_deref(),
            states_config,
            deps_config,
            sort_by.as_deref(),
            sort_order.as_deref(),
        )?
    } else if blocked {
        // Blocked tasks: have unsatisfied deps
        db.get_blocked_tasks(
            states_config,
            deps_config,
            sort_by.as_deref(),
            sort_order.as_deref(),
        )?
    } else if claimed {
        // Claimed tasks: currently owned by any agent
        db.get_claimed_tasks(None)?
    } else {
        // General query with filters
        // Handle status which can be string or array
        let status_vec: Option<Vec<String>> = if let Some(status_val) = args.get("status") {
            if let Some(s) = status_val.as_str() {
                Some(vec![s.to_string()])
            } else { status_val.as_array().map(|arr| arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()) }
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

        // Check if tag filtering or agent qualification filtering is needed
        let has_tag_filters = tags_any.is_some() || tags_all.is_some() || agent_id.is_some();

        if has_tag_filters {
            // Use the tag-filtered query
            // When agent is provided without ready=true, filter by agent's qualification
            let qualified_agent_tags = if let Some(aid) = &agent_id {
                Some(db.get_agent_tags(aid)?)
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
                sort_by.as_deref(),
                sort_order.as_deref(),
            )?
        } else {
            // Use list_tasks which returns full Task objects (only supports single status)
            let status = status_vec
                .as_ref()
                .and_then(|v| v.first().map(|s| s.as_str()));
            db.list_tasks(
                status,
                owner.as_deref(),
                parent_id,
                limit,
                sort_by.as_deref(),
                sort_order.as_deref(),
            )?
        }
    };

    // Apply limit (some paths may already have limit applied, but this ensures consistency)
    if let Some(l) = limit {
        tasks.truncate(l as usize);
    }

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

pub fn update(
    db: &Database,
    attachments_config: &AttachmentsConfig,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
    auto_advance: &AutoAdvanceConfig,
    args: Value,
) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let assignee = get_string(&args, "assignee");
    let title = get_string(&args, "title");
    let description = if args.get("description").is_some() {
        Some(get_string(&args, "description"))
    } else {
        None
    };
    let status = get_string(&args, "status");
    // Support both integer and string priority
    let priority = get_i32(&args, "priority")
        .or_else(|| get_string(&args, "priority").map(|s| parse_priority(&s)));
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
    let needed_tags = if args.get("needed_tags").is_some() {
        Some(get_string_array(&args, "needed_tags").unwrap_or_default())
    } else {
        None
    };
    let wanted_tags = if args.get("wanted_tags").is_some() {
        Some(get_string_array(&args, "wanted_tags").unwrap_or_default())
    } else {
        None
    };
    let time_estimate_ms = get_i64(&args, "time_estimate_ms");
    let reason = get_string(&args, "reason");
    let force = get_bool(&args, "force").unwrap_or(false);

    // Process attachments first (before the update)
    let mut attachment_results: Vec<Value> = Vec::new();
    let mut attachment_warnings: Vec<String> = Vec::new();

    if let Some(attachments_arr) = args.get("attachments").and_then(|v| v.as_array()) {
        for att_value in attachments_arr {
            let name = att_value.get("name").and_then(|v| v.as_str());
            let content = att_value.get("content").and_then(|v| v.as_str());
            let mime_override = att_value.get("mime").and_then(|v| v.as_str());
            let mode_override = att_value.get("mode").and_then(|v| v.as_str());

            let name = match name {
                Some(n) => n,
                None => {
                    attachment_warnings
                        .push("Skipped attachment: missing 'name' field".to_string());
                    continue;
                }
            };

            let content = match content {
                Some(c) => c,
                None => {
                    attachment_warnings.push(format!(
                        "Skipped attachment '{}': missing 'content' field",
                        name
                    ));
                    continue;
                }
            };

            // Check unknown key behavior
            if !attachments_config.is_known_key(name) {
                match attachments_config.unknown_key {
                    UnknownKeyBehavior::Reject => {
                        attachment_warnings.push(format!(
                            "Rejected attachment '{}': unknown key (configure in attachments.definitions or set unknown_key to 'allow')",
                            name
                        ));
                        continue;
                    }
                    UnknownKeyBehavior::Warn => {
                        attachment_warnings.push(format!("Unknown attachment key '{}'", name));
                    }
                    UnknownKeyBehavior::Allow => {}
                }
            }

            // Use config defaults for mime/mode, but allow explicit overrides
            let mime_type = mime_override
                .map(String::from)
                .unwrap_or_else(|| attachments_config.get_mime_default(name).to_string());
            let mode = mode_override.unwrap_or_else(|| attachments_config.get_mode_default(name));

            // Validate mode
            if mode != "append" && mode != "replace" {
                attachment_warnings.push(format!(
                    "Skipped attachment '{}': mode must be 'append' or 'replace'",
                    name
                ));
                continue;
            }

            // Handle replace mode - delete existing attachment with same name
            if mode == "replace" {
                let _ = db.delete_attachment_by_name(&task_id, name);
            }

            // Add the attachment
            match db.add_attachment(
                &task_id,
                name.to_string(),
                content.to_string(),
                Some(mime_type.clone()),
                None,
            ) {
                Ok(order_index) => {
                    attachment_results.push(json!({
                        "name": name,
                        "order_index": order_index,
                        "mime_type": mime_type
                    }));
                }
                Err(e) => {
                    attachment_warnings.push(format!("Failed to add attachment '{}': {}", name, e));
                }
            }
        }
    }

    // Perform the task update
    let (task, unblocked, auto_advanced) = db.update_task_unified(
        &task_id,
        &worker_id,
        assignee.as_deref(),
        title,
        description,
        status,
        priority,
        points,
        tags,
        needed_tags,
        wanted_tags,
        time_estimate_ms,
        reason,
        force,
        states_config,
        deps_config,
        auto_advance,
    )?;

    // Build response with task and unblocked/auto_advanced lists
    let mut response = serde_json::to_value(&task)?;
    if let Value::Object(ref mut map) = response {
        // Always include unblocked if non-empty (tasks now ready to claim)
        if !unblocked.is_empty() {
            map.insert("unblocked".to_string(), json!(unblocked));
        }
        // Include auto_advanced if non-empty (tasks that were actually transitioned)
        if !auto_advanced.is_empty() {
            map.insert("auto_advanced".to_string(), json!(auto_advanced));
        }
        // Include attachment results if any were added
        if !attachment_results.is_empty() {
            map.insert("attachments_added".to_string(), json!(attachment_results));
        }
        // Include warnings if any
        if !attachment_warnings.is_empty() {
            map.insert(
                "attachment_warnings".to_string(),
                json!(attachment_warnings),
            );
        }
    }

    Ok(response)
}

pub fn delete(db: &Database, args: Value) -> Result<Value> {
    let worker_id =
        get_string(&args, "worker_id").ok_or_else(|| ToolError::missing_field("worker_id"))?;
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let cascade = get_bool(&args, "cascade").unwrap_or(false);
    let reason = get_string(&args, "reason");
    let obliterate = get_bool(&args, "obliterate").unwrap_or(false);
    let force = get_bool(&args, "force").unwrap_or(false);

    db.delete_task(&task_id, &worker_id, cascade, reason, obliterate, force)?;

    Ok(json!({
        "success": true,
        "soft_deleted": !obliterate
    }))
}

pub fn scan(db: &Database, default_format: OutputFormat, args: Value) -> Result<Value> {
    let task_id = get_string(&args, "task").ok_or_else(|| ToolError::missing_field("task"))?;
    let format = get_string(&args, "format")
        .and_then(|s| OutputFormat::parse(&s))
        .unwrap_or(default_format);

    // Depth parameters: 0=none, N=levels, -1=all
    let before_depth = get_i32(&args, "before").unwrap_or(0);
    let after_depth = get_i32(&args, "after").unwrap_or(0);
    let above_depth = get_i32(&args, "above").unwrap_or(0);
    let below_depth = get_i32(&args, "below").unwrap_or(0);

    // Verify the task exists
    let root_task = db
        .get_task(&task_id)?
        .ok_or_else(|| ToolError::new(crate::error::ErrorCode::TaskNotFound, "Task not found"))?;

    // Traverse in each direction
    let before = db.get_predecessors(&task_id, before_depth)?;
    let after = db.get_successors(&task_id, after_depth)?;
    let above = db.get_ancestors(&task_id, above_depth)?;
    let below = db.get_descendants(&task_id, below_depth)?;

    let result = ScanResult {
        root: root_task,
        before,
        after,
        above,
        below,
    };

    match format {
        OutputFormat::Markdown => Ok(markdown_to_json(format_scan_result_markdown(&result))),
        OutputFormat::Json => Ok(serde_json::to_value(&result)?),
    }
}
