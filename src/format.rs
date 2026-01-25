//! Output formatting utilities for markdown and JSON.

use crate::config::StatesConfig;
use crate::types::{ScanResult, Task, TaskTree, WorkerInfo, PRIORITY_DEFAULT};
use serde_json::Value;
use std::collections::HashMap;

/// Output format for query results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Json,
    Markdown,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "json" => Some(OutputFormat::Json),
            "markdown" | "md" => Some(OutputFormat::Markdown),
            _ => None,
        }
    }
}

/// Format a single task as markdown.
pub fn format_task_markdown(task: &Task, blocked_by: &[String]) -> String {
    let mut md = String::new();

    md.push_str(&format!("## Task: {}\n", task.title));
    md.push_str(&format!("- **id**: `{}`\n", task.id));
    md.push_str(&format!("- **status**: {}\n", task.status));
    md.push_str(&format!("- **priority**: {}\n", task.priority));

    if let Some(ref owner) = task.owner_agent {
        md.push_str(&format!("- **owner**: {}\n", owner));
    }

    if !blocked_by.is_empty() {
        let blockers: Vec<String> = blocked_by.iter().map(|id| format!("`{}`", id)).collect();
        md.push_str(&format!("- **blocked_by**: {}\n", blockers.join(", ")));
    }

    if let Some(points) = task.points {
        md.push_str(&format!("- **points**: {}\n", points));
    }

    if let Some(ref thought) = task.current_thought {
        md.push_str(&format!("- **thought**: {}\n", thought));
    }

    if let Some(ref desc) = task.description {
        md.push_str("\n### Description\n");
        md.push_str(desc);
        md.push('\n');
    }

    md
}

/// Format a list of tasks as markdown.
/// Groups tasks by their state dynamically based on the states config.
pub fn format_tasks_markdown(
    tasks: &[(Task, Vec<String>)],
    states_config: &StatesConfig,
) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Tasks ({})\n\n", tasks.len()));

    // Group tasks by status
    let mut by_status: HashMap<String, Vec<&(Task, Vec<String>)>> = HashMap::new();
    for state in states_config.state_names() {
        by_status.insert(state.to_string(), Vec::new());
    }
    for task_entry in tasks {
        by_status
            .entry(task_entry.0.status.clone())
            .or_default()
            .push(task_entry);
    }

    // Output blocking states first (in-progress tasks), then initial state, then others
    // This provides a sensible default ordering

    // First, output blocking states (excluding initial state)
    for state in &states_config.blocking_states {
        if state != &states_config.initial {
            if let Some(state_tasks) = by_status.get(state) {
                if !state_tasks.is_empty() {
                    md.push_str(&format!("## {}\n\n", format_state_name(state)));
                    for (task, blocked_by) in state_tasks {
                        md.push_str(&format_task_short(task, blocked_by));
                    }
                    md.push('\n');
                }
            }
        }
    }

    // Then initial state
    if let Some(state_tasks) = by_status.get(&states_config.initial) {
        if !state_tasks.is_empty() {
            md.push_str(&format!(
                "## {}\n\n",
                format_state_name(&states_config.initial)
            ));
            for (task, blocked_by) in state_tasks {
                md.push_str(&format_task_short(task, blocked_by));
            }
            md.push('\n');
        }
    }

    // Then non-blocking states (terminal states like completed, failed, cancelled)
    for state in states_config.state_names() {
        if !states_config.is_blocking_state(state) && state != &states_config.initial {
            if let Some(state_tasks) = by_status.get(state) {
                if !state_tasks.is_empty() {
                    md.push_str(&format!("## {}\n\n", format_state_name(state)));
                    for (task, blocked_by) in state_tasks {
                        md.push_str(&format_task_short(task, blocked_by));
                    }
                    md.push('\n');
                }
            }
        }
    }

    md
}

/// Format a state name for display (capitalize, replace underscores with spaces).
fn format_state_name(state: &str) -> String {
    state
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format a task in short form for lists.
fn format_task_short(task: &Task, blocked_by: &[String]) -> String {
    let priority_marker = if task.priority > 0 {
        "!!! "
    } else {
        ""
    };

    let blocked = if blocked_by.is_empty() {
        String::new()
    } else {
        format!(" [blocked by {}]", blocked_by.len())
    };

    let owner = task.owner_agent.as_ref()
        .map(|o| format!(" @{}", o))
        .unwrap_or_default();

    let thought = task.current_thought.as_ref()
        .map(|t| format!(" - _{}_", t))
        .unwrap_or_default();

    format!(
        "- {}{} `{}`{}{}{}\n",
        priority_marker,
        task.title,
        &task.id[..8.min(task.id.len())],
        owner,
        blocked,
        thought,
    )
}

/// Format workers as markdown.
pub fn format_workers_markdown(workers: &[WorkerInfo]) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Workers ({})\n\n", workers.len()));

    for worker in workers {
        md.push_str(&format!("## {}\n", worker.id));
        md.push_str(&format!("- **id**: `{}`\n", worker.id));
        
        if !worker.tags.is_empty() {
            md.push_str(&format!("- **tags**: {}\n", worker.tags.join(", ")));
        }

        md.push_str(&format!("- **claims**: {}/{}\n", worker.claim_count, worker.max_claims));

        if let Some(ref thought) = worker.current_thought {
            md.push_str(&format!("- **doing**: {}\n", thought));
        }

        md.push('\n');
    }

    md
}


/// Format attachments as markdown.
pub fn format_attachments_markdown(attachments: &[crate::types::AttachmentMeta]) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Attachments ({})\n\n", attachments.len()));

    if attachments.is_empty() {
        md.push_str("_No attachments found._\n");
        return md;
    }

    for attachment in attachments {
        md.push_str(&format!("## {}\n", attachment.name));
        md.push_str(&format!("- **index**: {}\n", attachment.order_index));
        md.push_str(&format!("- **mime**: {}\n", attachment.mime_type));
        
        if let Some(ref fp) = attachment.file_path {
            md.push_str(&format!("- **file**: `{}`\n", fp));
        }
        
        // Format created_at as relative time if possible
        let created_secs = attachment.created_at / 1000;
        md.push_str(&format!("- **created**: {}\n", created_secs));
        
        md.push('\n');
    }

    md
}

/// Convert markdown to JSON value for uniform response handling.
pub fn markdown_to_json(md: String) -> Value {
    serde_json::json!({
        "format": "markdown",
        "content": md
    })
}

/// Result type for tool handlers - allows returning either JSON or raw text.
#[derive(Debug)]
pub enum ToolResult {
    /// JSON value (will be serialized to JSON string)
    Json(Value),
    /// Raw text (returned as-is, typically markdown)
    Raw(String),
}

impl ToolResult {
    /// Create a JSON result
    pub fn json(value: Value) -> Self {
        ToolResult::Json(value)
    }

    /// Create a raw text result (for markdown)
    pub fn raw(text: String) -> Self {
        ToolResult::Raw(text)
    }

    /// Convert to the appropriate string representation
    pub fn into_string(self) -> String {
        match self {
            ToolResult::Json(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            ToolResult::Raw(s) => s,
        }
    }
}

/// Format a task tree as markdown with visual tree structure.
pub fn format_task_tree_markdown(tree: &TaskTree) -> String {
    let mut md = String::new();

    // Format root task as heading
    md.push_str(&format!("# {}\n", tree.task.title));

    // Add root task metadata
    let mut meta_parts = Vec::new();
    meta_parts.push(tree.task.status.to_uppercase());
    if tree.task.priority != PRIORITY_DEFAULT {
        meta_parts.push(format!("P{}", tree.task.priority));
    }
    if let Some(points) = tree.task.points {
        meta_parts.push(format!("{} pts", points));
    }
    if let Some(ref owner) = tree.task.owner_agent {
        meta_parts.push(format!("@{}", owner));
    }

    if !meta_parts.is_empty() {
        md.push_str(&format!("_{}_\n", meta_parts.join(", ")));
    }

    if let Some(ref desc) = tree.task.description {
        md.push_str(&format!("\n{}\n", desc));
    }

    // Format children with tree characters
    if !tree.children.is_empty() {
        md.push('\n');
        format_tree_children(&tree.children, "", &mut md);
    }

    md
}

/// Recursively format children with tree structure characters.
fn format_tree_children(children: &[TaskTree], prefix: &str, md: &mut String) {
    let count = children.len();

    for (i, child) in children.iter().enumerate() {
        let is_last = i == count - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        // Build the task line with metadata
        let mut meta_parts = Vec::new();
        meta_parts.push(child.task.status.clone());
        if child.task.priority != PRIORITY_DEFAULT {
            meta_parts.push(format!("P{}", child.task.priority));
        }
        if let Some(points) = child.task.points {
            meta_parts.push(format!("{} pts", points));
        }
        if let Some(ref owner) = child.task.owner_agent {
            meta_parts.push(format!("@{}", owner));
        }

        let meta_str = if !meta_parts.is_empty() {
            format!(" [{}]", meta_parts.join(", "))
        } else {
            String::new()
        };

        md.push_str(&format!("{}{}{}{}\n", prefix, connector, child.task.title, meta_str));

        // Recursively format grandchildren
        if !child.children.is_empty() {
            format_tree_children(&child.children, &format!("{}{}", prefix, child_prefix), md);
        }
    }
}

/// Format a scan result as markdown.
pub fn format_scan_result_markdown(result: &ScanResult) -> String {
    let mut md = String::new();

    // Root task header
    md.push_str(&format!("# Scan: {}\\n", result.root.title));
    md.push_str(&format!("- **id**: `{}`\\n", result.root.id));
    md.push_str(&format!("- **status**: {}\\n", result.root.status));
    md.push_str(&format!("- **priority**: {}\\n", result.root.priority));
    
    if let Some(ref owner) = result.root.owner_agent {
        md.push_str(&format!("- **owner**: {}\\n", owner));
    }
    
    if let Some(ref desc) = result.root.description {
        md.push_str(&format!("\\n{}\\n", desc));
    }

    // Before (predecessors)
    if !result.before.is_empty() {
        md.push_str(&format!("\\n## Before ({} tasks)\\n", result.before.len()));
        md.push_str("_Tasks that block this task via blocks/follows dependencies_\\n\\n");
        for task in &result.before {
            md.push_str(&format_scan_task_short(task));
        }
    }

    // After (successors)
    if !result.after.is_empty() {
        md.push_str(&format!("\\n## After ({} tasks)\\n", result.after.len()));
        md.push_str("_Tasks that this task blocks via blocks/follows dependencies_\\n\\n");
        for task in &result.after {
            md.push_str(&format_scan_task_short(task));
        }
    }

    // Above (ancestors)
    if !result.above.is_empty() {
        md.push_str(&format!("\\n## Above ({} tasks)\\n", result.above.len()));
        md.push_str("_Parent chain via contains dependency_\\n\\n");
        for task in &result.above {
            md.push_str(&format_scan_task_short(task));
        }
    }

    // Below (descendants)
    if !result.below.is_empty() {
        md.push_str(&format!("\\n## Below ({} tasks)\\n", result.below.len()));
        md.push_str("_Descendants via contains dependency_\\n\\n");
        for task in &result.below {
            md.push_str(&format_scan_task_short(task));
        }
    }

    // Summary
    let total = result.before.len() + result.after.len() + result.above.len() + result.below.len();
    md.push_str(&format!("\\n---\\n**Total related tasks**: {}\\n", total));

    md
}

/// Format a task in short form for scan results.
fn format_scan_task_short(task: &Task) -> String {
    let priority_marker = if task.priority > 0 {
        "!!! "
    } else {
        ""
    };

    let owner = task.owner_agent.as_ref()
        .map(|o| format!(" @{}", o))
        .unwrap_or_default();

    let points = task.points
        .map(|p| format!(" ({} pts)", p))
        .unwrap_or_default();

    format!(
        "- {}{} `{}` [{}]{}{}\\n",
        priority_marker,
        task.title,
        &task.id[..8.min(task.id.len())],
        task.status,
        owner,
        points,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Priority, Task, TaskTree, PRIORITY_DEFAULT};

    fn make_test_task(id: &str, title: &str, status: &str, priority: Priority, points: Option<i32>) -> Task {
        Task {
            id: id.to_string(),
            title: title.to_string(),
            description: None,
            status: status.to_string(),
            priority,
            owner_agent: None,
            claimed_at: None,
            agent_tags_all: vec![],
            agent_tags_any: vec![],
            tags: vec![],
            points,
            time_estimate_ms: None,
            time_actual_ms: None,
            started_at: None,
            completed_at: None,
            current_thought: None,
            cost_usd: 0.0,
            metrics: [0; 8],
            user_metrics: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_format_task_tree_markdown_root_only() {
        let tree = TaskTree {
            task: make_test_task("root-1", "Root Task", "pending", 8, Some(5)),
            children: vec![],
        };

        let result = format_task_tree_markdown(&tree);
        assert!(result.contains("# Root Task"));
        assert!(result.contains("PENDING"));
        assert!(result.contains("P8"));
        assert!(result.contains("5 pts"));
    }

    #[test]
    fn test_format_task_tree_markdown_with_children() {
        let tree = TaskTree {
            task: make_test_task("root-1", "API Refactoring Sprint", "in_progress", 8, Some(16)),
            children: vec![
                TaskTree {
                    task: make_test_task("child-1", "Tier 1: Prerequisites", "pending", 8, Some(9)),
                    children: vec![
                        TaskTree {
                            task: make_test_task("grandchild-1", "Refactor connect", "completed", PRIORITY_DEFAULT, Some(3)),
                            children: vec![],
                        },
                        TaskTree {
                            task: make_test_task("grandchild-2", "Merge claim/release", "pending", PRIORITY_DEFAULT, Some(5)),
                            children: vec![],
                        },
                    ],
                },
                TaskTree {
                    task: make_test_task("child-2", "Tier 2: Navigation", "pending", PRIORITY_DEFAULT, Some(7)),
                    children: vec![],
                },
            ],
        };

        let result = format_task_tree_markdown(&tree);

        // Check root formatting
        assert!(result.contains("# API Refactoring Sprint"));
        assert!(result.contains("IN_PROGRESS"));

        // Check tree structure characters
        assert!(result.contains("├── Tier 1: Prerequisites"));
        assert!(result.contains("└── Tier 2: Navigation"));

        // Check grandchildren have proper indentation
        assert!(result.contains("│   ├── Refactor connect"));
        assert!(result.contains("│   └── Merge claim/release"));
    }

    #[test]
    fn test_format_task_tree_markdown_deep_nesting() {
        let tree = TaskTree {
            task: make_test_task("root", "Root", "pending", PRIORITY_DEFAULT, None),
            children: vec![
                TaskTree {
                    task: make_test_task("l1", "Level 1", "pending", PRIORITY_DEFAULT, None),
                    children: vec![
                        TaskTree {
                            task: make_test_task("l2", "Level 2", "pending", PRIORITY_DEFAULT, None),
                            children: vec![
                                TaskTree {
                                    task: make_test_task("l3", "Level 3", "pending", PRIORITY_DEFAULT, None),
                                    children: vec![],
                                },
                            ],
                        },
                    ],
                },
            ],
        };

        let result = format_task_tree_markdown(&tree);

        // Check deep nesting with proper prefix
        assert!(result.contains("└── Level 1"));
        assert!(result.contains("    └── Level 2"));
        assert!(result.contains("        └── Level 3"));
    }
}
