//! Advisory tools for on-demand governance guidance.
//!
//! Agents request advisories via `get_advisory` to get contextual guidance
//! on decomposition, domain injection, approval patterns, and more.

use super::{get_string, make_tool};
use crate::config::workflows::WorkflowsConfig;
use crate::db::Database;
use crate::prompts::{PromptContext, expand_prompt};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Get the advisory-related tools.
pub fn get_tools() -> Vec<Tool> {
    vec![make_tool(
        "get_advisory",
        "Get governance advisory guidance. Without topic: lists all advisory topics with filters. With topic: returns the full advisory content (with template expansion if task context provided). Use to get guidance on decomposition, domain injection, approval gates, and more.",
        json!({
            "topic": {
                "type": "string",
                "description": "Advisory topic name (e.g., 'decompose-epic', 'inject-legal'). Omit to list all topics."
            },
            "task": {
                "type": "string",
                "description": "Task ID for context-sensitive filtering and template expansion. Optional."
            },
            "worker_id": {
                "type": "string",
                "description": "Worker ID for role-based filtering and template expansion. Optional."
            }
        }),
        vec![],
    )]
}

/// Get advisory guidance.
///
/// Two modes:
/// 1. **List mode** (no topic): Returns all advisory topics with their filters and a
///    relevance indicator based on task/worker context.
/// 2. **Detail mode** (with topic): Returns the full advisory content with template
///    variables expanded using the task/worker context.
pub fn get_advisory(db: &Database, workflows: &WorkflowsConfig, args: Value) -> Result<Value> {
    let topic = get_string(&args, "topic");
    let task_id = get_string(&args, "task");
    let worker_id = get_string(&args, "worker_id");

    // Extract context from task if provided
    let task = task_id
        .as_deref()
        .and_then(|id| db.get_task(id).ok().flatten());
    let task_tags: Vec<String> = task.as_ref().map(|t| t.tags.clone()).unwrap_or_default();

    // Extract level and domain from task tags
    let task_level: Option<String> = task_tags
        .iter()
        .find(|t| t.starts_with("level:"))
        .map(|t| t.strip_prefix("level:").unwrap_or(t).to_string());
    let task_domain: Option<String> = task_tags
        .iter()
        .find(|t| t.starts_with("domain:"))
        .map(|t| t.strip_prefix("domain:").unwrap_or(t).to_string());

    // Get worker info for role matching
    let worker = worker_id
        .as_deref()
        .and_then(|id| db.get_worker(id).ok().flatten());
    let worker_role = worker.as_ref().and_then(|w| workflows.match_role(&w.tags));

    match topic {
        None => {
            // List mode: return all advisory topics with filters and relevance
            let mut topics: Vec<Value> = Vec::new();
            let mut sorted_keys: Vec<&String> = workflows.advisories.keys().collect();
            sorted_keys.sort();

            for key in sorted_keys {
                let advisory = &workflows.advisories[key];
                let relevant = is_relevant(
                    advisory,
                    task_level.as_deref(),
                    task_domain.as_deref(),
                    task.as_ref().and_then(|t| t.phase.as_deref()),
                    worker_role.as_deref(),
                );

                let mut entry = json!({
                    "topic": key,
                    "relevant": relevant,
                });
                if !advisory.level.is_empty() {
                    entry["level"] = json!(advisory.level);
                }
                if !advisory.phase.is_empty() {
                    entry["phase"] = json!(advisory.phase);
                }
                if !advisory.role.is_empty() {
                    entry["role"] = json!(advisory.role);
                }
                if !advisory.domain.is_empty() {
                    entry["domain"] = json!(advisory.domain);
                }
                topics.push(entry);
            }

            Ok(json!({
                "advisories": topics,
                "count": topics.len(),
                "hint": "Call get_advisory(topic=\"<name>\") to read a specific advisory."
            }))
        }
        Some(topic_name) => {
            // Detail mode: return full content with template expansion
            let advisory = workflows.advisories.get(&topic_name).ok_or_else(|| {
                let available: Vec<&String> = workflows.advisories.keys().collect();
                crate::error::ToolError::new(
                    crate::error::ErrorCode::InvalidFieldValue,
                    format!(
                        "Advisory topic '{}' not found. Available: {:?}",
                        topic_name, available
                    ),
                )
            })?;

            // Build prompt context for template expansion
            let states_config = workflows.into();
            let phases_config = workflows.into();
            let status = task
                .as_ref()
                .map(|t| t.status.as_str())
                .unwrap_or("pending");
            let phase = task.as_ref().and_then(|t| t.phase.as_deref());
            let mut ctx = PromptContext::new(status, phase, &states_config, &phases_config);

            if let Some(ref t) = task {
                ctx = ctx.with_task(&t.id, &t.title, t.priority, &t.tags);
            }
            if let Some(ref w) = worker {
                ctx = ctx.with_agent(&w.id, worker_role.as_deref(), &w.tags);
            }

            let content = expand_prompt(&advisory.content, &ctx);

            let relevant = is_relevant(
                advisory,
                task_level.as_deref(),
                task_domain.as_deref(),
                phase,
                worker_role.as_deref(),
            );

            let mut result = json!({
                "topic": topic_name,
                "content": content,
                "relevant": relevant,
            });
            if !advisory.level.is_empty() {
                result["level"] = json!(advisory.level);
            }
            if !advisory.phase.is_empty() {
                result["phase"] = json!(advisory.phase);
            }
            if !advisory.role.is_empty() {
                result["role"] = json!(advisory.role);
            }
            if !advisory.domain.is_empty() {
                result["domain"] = json!(advisory.domain);
            }

            Ok(result)
        }
    }
}

/// Get advisory topic names relevant to a task+worker context.
///
/// Extracts `level:*` and `domain:*` from task tags, uses phase and role
/// for matching. Returns sorted topic names that agents can fetch with
/// `get_advisory(topic="...")`.
pub fn relevant_advisory_topics(
    workflows: &WorkflowsConfig,
    task_tags: &[String],
    phase: Option<&str>,
    role: Option<&str>,
) -> Vec<String> {
    if workflows.advisories.is_empty() {
        return Vec::new();
    }

    let task_level: Option<&str> = task_tags
        .iter()
        .find(|t| t.starts_with("level:"))
        .map(|t| t.strip_prefix("level:").unwrap_or(t));
    let task_domain: Option<&str> = task_tags
        .iter()
        .find(|t| t.starts_with("domain:"))
        .map(|t| t.strip_prefix("domain:").unwrap_or(t));

    let mut topics: Vec<String> = workflows
        .advisories
        .iter()
        .filter(|(_, adv)| is_relevant(adv, task_level, task_domain, phase, role))
        .map(|(name, _)| name.clone())
        .collect();
    topics.sort();
    topics
}

/// Check if an advisory is relevant to the current context.
/// An advisory is relevant if all non-empty filter lists match the context.
fn is_relevant(
    advisory: &crate::config::workflows::AdvisoryDefinition,
    level: Option<&str>,
    domain: Option<&str>,
    phase: Option<&str>,
    role: Option<&str>,
) -> bool {
    let level_match = advisory.level.is_empty()
        || level.map_or(false, |l| advisory.level.iter().any(|al| al == l));
    let domain_match = advisory.domain.is_empty()
        || domain.map_or(false, |d| advisory.domain.iter().any(|ad| ad == d));
    let phase_match = advisory.phase.is_empty()
        || phase.map_or(false, |p| advisory.phase.iter().any(|ap| ap == p));
    let role_match =
        advisory.role.is_empty() || role.map_or(false, |r| advisory.role.iter().any(|ar| ar == r));

    level_match && domain_match && phase_match && role_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::workflows::AdvisoryDefinition;

    #[test]
    fn test_is_relevant_empty_filters() {
        let advisory = AdvisoryDefinition::default();
        assert!(is_relevant(&advisory, None, None, None, None));
        assert!(is_relevant(
            &advisory,
            Some("epic"),
            Some("engineering"),
            Some("implement"),
            Some("worker")
        ));
    }

    #[test]
    fn test_is_relevant_level_filter() {
        let advisory = AdvisoryDefinition {
            level: vec!["epic".to_string(), "story".to_string()],
            ..Default::default()
        };
        assert!(is_relevant(&advisory, Some("epic"), None, None, None));
        assert!(is_relevant(&advisory, Some("story"), None, None, None));
        assert!(!is_relevant(&advisory, Some("task"), None, None, None));
        assert!(!is_relevant(&advisory, None, None, None, None));
    }

    #[test]
    fn test_is_relevant_domain_filter() {
        let advisory = AdvisoryDefinition {
            domain: vec!["legal".to_string()],
            ..Default::default()
        };
        assert!(is_relevant(&advisory, None, Some("legal"), None, None));
        assert!(!is_relevant(
            &advisory,
            None,
            Some("engineering"),
            None,
            None
        ));
    }

    #[test]
    fn test_is_relevant_combined_filters() {
        let advisory = AdvisoryDefinition {
            level: vec!["epic".to_string()],
            domain: vec!["engineering".to_string()],
            phase: vec!["implement".to_string()],
            ..Default::default()
        };
        // All match
        assert!(is_relevant(
            &advisory,
            Some("epic"),
            Some("engineering"),
            Some("implement"),
            None
        ));
        // One doesn't match
        assert!(!is_relevant(
            &advisory,
            Some("story"),
            Some("engineering"),
            Some("implement"),
            None
        ));
        assert!(!is_relevant(
            &advisory,
            Some("epic"),
            Some("legal"),
            Some("implement"),
            None
        ));
    }
}
