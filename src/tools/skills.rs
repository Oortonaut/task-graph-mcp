//! Skill management tools - list and get skills.

use super::make_tool;
use crate::resources::skills::{get_skill_resource, list_skills};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};
use std::path::Path;

/// Get all skill-related tools.
pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "list_skills",
            "List all available skills. Shows built-in and custom skills with their source information.",
            json!({}),
            vec![],
        ),
        make_tool(
            "get_skill",
            "Get a skill's full content by name.",
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

            get_skill_resource(Some(skills_dir), skill_name)
        }

        _ => Err(anyhow::anyhow!("Unknown skill tool: {}", name)),
    }
}

/// Check if a tool name is a skill tool.
pub fn is_skill_tool(name: &str) -> bool {
    matches!(name, "list_skills" | "get_skill")
}
