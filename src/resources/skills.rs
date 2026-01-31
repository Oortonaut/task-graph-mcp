//! Skill resources - expose bundled skills via MCP resources with override support.
//!
//! Skills can be overridden by placing custom SKILL.md files in:
//! - `~/.task-graph/skills/{name}/SKILL.md` (user-level, highest priority)
//! - `task-graph/skills/{name}/SKILL.md` (project-level)
//! - `.task-graph/skills/{name}/SKILL.md` (project-level, deprecated)
//!
//! The lookup order is:
//! 1. User override (`~/.task-graph/skills/`)
//! 2. Project override (`task-graph/skills/` or `.task-graph/skills/`)
//! 3. Embedded default (compiled into binary from `config/skills/`)

use anyhow::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Embedded skill content (compiled into binary).
/// These are the SKILL.md files from the config/skills/ directory.
pub mod embedded {
    pub const BASICS: &str = include_str!("../../config/skills/task-graph-basics/SKILL.md");
    pub const REPORTING: &str = include_str!("../../config/skills/task-graph-reporting/SKILL.md");
    pub const MIGRATION: &str = include_str!("../../config/skills/task-graph-migration/SKILL.md");
    pub const REPAIR: &str = include_str!("../../config/skills/task-graph-repair/SKILL.md");
}

/// Skill metadata for listing.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: &'static str,
    pub full_name: &'static str,
    pub description: &'static str,
    pub role: &'static str,
}

/// All available skills.
pub const SKILLS: &[SkillInfo] = &[
    SkillInfo {
        name: "basics",
        full_name: "task-graph-basics",
        description: "Foundation - tool reference, connection workflow, task trees, search, shared patterns",
        role: "foundation",
    },
    SkillInfo {
        name: "reporting",
        full_name: "task-graph-reporting",
        description: "Analytics - generate reports, track costs and velocity",
        role: "reporting",
    },
    SkillInfo {
        name: "migration",
        full_name: "task-graph-migration",
        description: "Import - migrate from GitHub Issues, Linear, Jira, markdown",
        role: "migration",
    },
    SkillInfo {
        name: "repair",
        full_name: "task-graph-repair",
        description: "Maintenance - fix orphaned tasks, broken deps, stale claims",
        role: "repair",
    },
];

/// Get the embedded skill content by name.
fn get_embedded_skill(name: &str) -> Option<&'static str> {
    match name {
        "basics" | "task-graph-basics" => Some(embedded::BASICS),
        "reporting" | "task-graph-reporting" => Some(embedded::REPORTING),
        "migration" | "task-graph-migration" => Some(embedded::MIGRATION),
        "repair" | "task-graph-repair" => Some(embedded::REPAIR),
        _ => None,
    }
}

/// Extract the `description` field from YAML frontmatter in a SKILL.md file.
/// Returns None if frontmatter is missing or has no description.
fn parse_frontmatter_description(content: &str) -> Option<String> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    let after_open = &content[3..];
    let close = after_open.find("\n---")?;
    let yaml_block = &after_open[..close];
    let mapping: serde_yaml::Value = serde_yaml::from_str(yaml_block).ok()?;
    mapping
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Normalize skill name (strip "task-graph-" prefix if present).
fn normalize_name(name: &str) -> &str {
    name.strip_prefix("task-graph-").unwrap_or(name)
}

/// Check if a skill name is a built-in skill (embedded or override of embedded).
fn is_builtin_skill(name: &str) -> bool {
    let normalized = normalize_name(name);
    SKILLS.iter().any(|s| s.name == normalized)
}

/// Validate skill name to prevent path traversal attacks.
/// Only allows alphanumeric characters, hyphens, and underscores.
fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("Skill name cannot be empty"));
    }

    if name.len() > 64 {
        return Err(anyhow::anyhow!("Skill name too long (max 64 chars)"));
    }

    // Check for path traversal attempts
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(anyhow::anyhow!(
            "Invalid skill name: path traversal not allowed"
        ));
    }

    // Only allow safe characters: alphanumeric, hyphen, underscore
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow::anyhow!(
            "Invalid skill name: only alphanumeric, hyphen, and underscore allowed"
        ));
    }

    Ok(())
}

/// Get the path to a project-local skill override.
fn get_override_path(skills_dir: &Path, name: &str) -> PathBuf {
    let normalized = normalize_name(name);
    // Try both naming conventions
    let full_name = format!("task-graph-{}", normalized);

    let short_path = skills_dir.join(normalized).join("SKILL.md");
    let full_path = skills_dir.join(&full_name).join("SKILL.md");

    if full_path.exists() {
        full_path
    } else {
        short_path
    }
}

/// Get skill content, checking for overrides first.
/// Validates skill name to prevent path traversal attacks.
pub fn get_skill(skills_dir: Option<&Path>, name: &str) -> Result<String> {
    // Validate name before any file operations
    validate_skill_name(name)?;

    let normalized = normalize_name(name);

    // Check for override first
    if let Some(dir) = skills_dir {
        let override_path = get_override_path(dir, name);

        // Additional safety: ensure the resolved path is within skills_dir
        if let Ok(canonical_override) = override_path.canonicalize()
            && let Ok(canonical_dir) = dir.canonicalize()
            && canonical_override.starts_with(&canonical_dir)
            && override_path.exists()
        {
            return std::fs::read_to_string(&override_path)
                .map_err(|e| anyhow::anyhow!("Failed to read skill override: {}", e));
        }
    }

    // Fall back to embedded
    get_embedded_skill(normalized)
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("Unknown skill: {}", name))
}

/// Check if a skill is overridden.
fn is_overridden(skills_dir: Option<&Path>, name: &str) -> bool {
    if let Some(dir) = skills_dir {
        get_override_path(dir, name).exists()
    } else {
        false
    }
}

/// List all skills as JSON, indicating which are overridden.
pub fn list_skills(skills_dir: Option<&Path>) -> Result<Value> {
    let mut skills_list: Vec<Value> = SKILLS
        .iter()
        .map(|s| {
            let overridden = is_overridden(skills_dir, s.name);
            let description = get_skill(skills_dir, s.name)
                .ok()
                .and_then(|content| parse_frontmatter_description(&content))
                .unwrap_or_else(|| s.description.to_string());
            json!({
                "name": s.name,
                "full_name": s.full_name,
                "description": description,
                "role": s.role,
                "uri": format!("skills://{}", s.name),
                "overridden": overridden,
                "source": if overridden { "local" } else { "embedded" },
            })
        })
        .collect();

    // Also check for custom skills in the override directory
    if let Some(dir) = skills_dir
        && dir.exists()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let normalized = normalize_name(&name);

                    // Skip if it's an override of a known skill
                    if SKILLS
                        .iter()
                        .any(|s| s.name == normalized || s.full_name == name)
                    {
                        continue;
                    }

                    let description = std::fs::read_to_string(&skill_md)
                        .ok()
                        .and_then(|content| parse_frontmatter_description(&content))
                        .unwrap_or_else(|| "Custom skill".to_string());

                    skills_list.push(json!({
                        "name": normalized,
                        "full_name": name,
                        "description": description,
                        "role": "custom",
                        "uri": format!("skills://{}", normalized),
                        "overridden": false,
                        "source": "local",
                    }));
                }
            }
        }
    }

    Ok(json!({
        "skills": skills_list,
        "count": skills_list.len(),
        "override_dir": skills_dir.map(|p| p.display().to_string()),
    }))
}

/// Get a skill's content as JSON.
pub fn get_skill_resource(skills_dir: Option<&Path>, name: &str) -> Result<Value> {
    validate_skill_name(name)?;

    let normalized = normalize_name(name);
    let is_builtin = is_builtin_skill(name);
    let overridden = is_overridden(skills_dir, name);

    let info = SKILLS.iter().find(|s| s.name == normalized);
    let content = get_skill(skills_dir, name)?;

    Ok(json!({
        "name": info.map(|i| i.name).unwrap_or(normalized),
        "full_name": info.map(|i| i.full_name).unwrap_or(name),
        "role": info.map(|i| i.role).unwrap_or("custom"),
        "description": info.map(|i| i.description).unwrap_or("Custom skill"),
        "content": content,
        "mime_type": "text/markdown",
        "overridden": overridden,
        "source": if is_builtin && !overridden { "embedded" } else { "local" },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_embedded_skill() {
        assert!(get_embedded_skill("basics").is_some());
        assert!(get_embedded_skill("task-graph-basics").is_some());
        assert!(get_embedded_skill("unknown").is_none());
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("basics"), "basics");
        assert_eq!(normalize_name("task-graph-basics"), "basics");
        assert_eq!(normalize_name("task-graph-reporting"), "reporting");
    }

    #[test]
    fn test_get_skill_embedded() {
        let content = get_skill(None, "basics").unwrap();
        assert!(!content.is_empty());
        assert!(content.starts_with("---"));
    }

    #[test]
    fn test_parse_frontmatter_description() {
        let md = "---\nname: foo\ndescription: A great skill\n---\n# Heading\n";
        assert_eq!(
            parse_frontmatter_description(md),
            Some("A great skill".to_string())
        );
    }

    #[test]
    fn test_parse_frontmatter_no_description() {
        let md = "---\nname: foo\n---\n# Heading\n";
        assert_eq!(parse_frontmatter_description(md), None);
    }

    #[test]
    fn test_parse_frontmatter_missing() {
        assert_eq!(parse_frontmatter_description("# No frontmatter"), None);
    }

    #[test]
    fn test_list_skills() {
        let result = list_skills(None).unwrap();
        assert_eq!(result["count"], 4);
    }

    #[test]
    fn test_list_skills_has_frontmatter_descriptions() {
        let result = list_skills(None).unwrap();
        let skills = result["skills"].as_array().unwrap();
        for skill in skills {
            let desc = skill["description"].as_str().unwrap();
            // Embedded skills should have real descriptions from frontmatter, not the fallback
            assert!(
                !desc.is_empty(),
                "Skill {} has empty description",
                skill["name"]
            );
            assert_ne!(
                desc, "Custom skill",
                "Skill {} still using fallback description",
                skill["name"]
            );
        }
    }

    #[test]
    fn test_skill_content_not_empty() {
        for skill in SKILLS {
            let content = get_skill(None, skill.name).unwrap();
            assert!(!content.is_empty(), "Skill {} is empty", skill.name);
            assert!(
                content.starts_with("---"),
                "Skill {} missing frontmatter",
                skill.name
            );
        }
    }
}
