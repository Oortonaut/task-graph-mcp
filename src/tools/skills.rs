//! Skill management tools - list and get skills.
//!
//! Custom skills require pre-approval (via .approved file) or user approval
//! through evocation before their content is accessible.

use super::make_tool;
use crate::resources::skills::{get_skill_resource, is_skill_approved, list_skills};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::Path;

/// Get all skill-related tools.
pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "list_skills",
            "List all available skills with their approval status. Built-in skills are always approved. Custom skills show whether they require approval.",
            json!({}),
            vec![],
        ),
        make_tool(
            "get_skill",
            "Get a skill's content. Custom skills must be pre-approved (in .task-graph/skills/.approved) or user-approved via evocation. Returns error if skill is not approved.",
            json!({
                "name": {
                    "type": "string",
                    "description": "The skill name (e.g., 'basics', 'coordinator', 'worker')"
                }
            }),
            vec!["name"],
        ),
    ]
}

/// Handle skill tool calls.
pub fn call_tool(skills_dir: &Path, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list_skills" => list_skills(Some(skills_dir)),

        "get_skill" => {
            let skill_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

            // Check if skill is approved before returning content
            if !is_skill_approved(Some(skills_dir), skill_name) {
                return Err(anyhow::anyhow!(
                    "Skill '{}' is not approved. Add to .task-graph/skills/.approved or approve via evocation.",
                    skill_name
                ));
            }

            get_skill_resource(Some(skills_dir), skill_name)
        }

        _ => Err(anyhow::anyhow!("Unknown skill tool: {}", name)),
    }
}

/// Check if a tool name is a skill tool.
pub fn is_skill_tool(name: &str) -> bool {
    matches!(name, "list_skills" | "get_skill")
}
