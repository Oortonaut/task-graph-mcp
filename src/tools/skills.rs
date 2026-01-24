//! Skill management tools - approve and revoke custom skills.

use super::make_tool;
use crate::resources::skills::{
    approve_skill, get_skill_resource, list_skills, revoke_skill,
};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::Path;

/// Get all skill-related tools.
pub fn get_tools() -> Vec<Tool> {
    vec![
        make_tool(
            "approve_skill",
            "Approve a custom skill for use. Custom skills (not built-in) require explicit approval before their full content is accessible. This is a security measure to prevent loading arbitrary instructions.",
            json!({
                "name": {
                    "type": "string",
                    "description": "The skill name to approve (e.g., 'my-custom-skill')"
                }
            }),
            vec!["name"],
        ),
        make_tool(
            "revoke_skill",
            "Revoke approval for a custom skill. The skill will no longer be accessible until re-approved.",
            json!({
                "name": {
                    "type": "string",
                    "description": "The skill name to revoke (e.g., 'my-custom-skill')"
                }
            }),
            vec!["name"],
        ),
        make_tool(
            "list_skills",
            "List all available skills with their approval status. Built-in skills are always approved. Custom skills show whether they require approval.",
            json!({}),
            vec![],
        ),
        make_tool(
            "get_skill",
            "Get a skill's content. For unapproved custom skills, returns only a preview. Use approve_skill first to get full content.",
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
        "approve_skill" => {
            let skill_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

            // Check if skill exists first
            if get_skill_resource(Some(skills_dir), skill_name).is_err() {
                return Err(anyhow::anyhow!("Skill not found: {}", skill_name));
            }

            approve_skill(skills_dir, skill_name)?;

            Ok(json!({
                "success": true,
                "message": format!("Skill '{}' has been approved", skill_name),
                "skill": skill_name,
            }))
        }

        "revoke_skill" => {
            let skill_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

            revoke_skill(skills_dir, skill_name)?;

            Ok(json!({
                "success": true,
                "message": format!("Skill '{}' approval has been revoked", skill_name),
                "skill": skill_name,
            }))
        }

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
    matches!(
        name,
        "approve_skill" | "revoke_skill" | "list_skills" | "get_skill"
    )
}
