//! Output formatting utilities for markdown and JSON.

use crate::types::{AgentInfo, Task, TaskStatus, Priority};
use serde_json::Value;

/// Output format for query results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
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
    md.push_str(&format!("- **status**: {}\n", task.status.as_str()));
    md.push_str(&format!("- **priority**: {}\n", task.priority.as_str()));

    if let Some(ref owner) = task.owner_agent {
        md.push_str(&format!("- **owner**: {}\n", owner));
    }

    if let Some(ref parent_id) = task.parent_id {
        md.push_str(&format!("- **parent_id**: `{}`\n", parent_id));
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
pub fn format_tasks_markdown(tasks: &[(Task, Vec<String>)]) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Tasks ({})\n\n", tasks.len()));

    // Group by status
    let pending: Vec<_> = tasks.iter().filter(|(t, _)| t.status == TaskStatus::Pending).collect();
    let in_progress: Vec<_> = tasks.iter().filter(|(t, _)| t.status == TaskStatus::InProgress).collect();
    let completed: Vec<_> = tasks.iter().filter(|(t, _)| t.status == TaskStatus::Completed).collect();
    let failed: Vec<_> = tasks.iter().filter(|(t, _)| t.status == TaskStatus::Failed).collect();
    let cancelled: Vec<_> = tasks.iter().filter(|(t, _)| t.status == TaskStatus::Cancelled).collect();

    if !in_progress.is_empty() {
        md.push_str("## In Progress\n\n");
        for (task, blocked_by) in &in_progress {
            md.push_str(&format_task_short(task, blocked_by));
        }
        md.push('\n');
    }

    if !pending.is_empty() {
        md.push_str("## Pending\n\n");
        for (task, blocked_by) in &pending {
            md.push_str(&format_task_short(task, blocked_by));
        }
        md.push('\n');
    }

    if !completed.is_empty() {
        md.push_str("## Completed\n\n");
        for (task, blocked_by) in &completed {
            md.push_str(&format_task_short(task, blocked_by));
        }
        md.push('\n');
    }

    if !failed.is_empty() {
        md.push_str("## Failed\n\n");
        for (task, blocked_by) in &failed {
            md.push_str(&format_task_short(task, blocked_by));
        }
        md.push('\n');
    }

    if !cancelled.is_empty() {
        md.push_str("## Cancelled\n\n");
        for (task, blocked_by) in &cancelled {
            md.push_str(&format_task_short(task, blocked_by));
        }
        md.push('\n');
    }

    md
}

/// Format a task in short form for lists.
fn format_task_short(task: &Task, blocked_by: &[String]) -> String {
    let priority_marker = match task.priority {
        Priority::High => "!!! ",
        Priority::Medium => "",
        Priority::Low => "",
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

/// Format agents as markdown.
pub fn format_agents_markdown(agents: &[AgentInfo]) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Agents ({})\n\n", agents.len()));

    for agent in agents {
        md.push_str(&format!("## {}\n", agent.name.as_deref().unwrap_or(&agent.id)));
        md.push_str(&format!("- **id**: `{}`\n", agent.id));
        
        if !agent.tags.is_empty() {
            md.push_str(&format!("- **tags**: {}\n", agent.tags.join(", ")));
        }

        md.push_str(&format!("- **claims**: {}/{}\n", agent.claim_count, agent.max_claims));

        if let Some(ref thought) = agent.current_thought {
            md.push_str(&format!("- **doing**: {}\n", thought));
        }

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
