//! Output formatting utilities for markdown and JSON.

use crate::config::StatesConfig;
use crate::types::{AgentInfo, Priority, Task};
use serde_json::Value;
use std::collections::HashMap;

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
    md.push_str(&format!("- **status**: {}\n", task.status));
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
