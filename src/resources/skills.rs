//! Skill resources - expose bundled skills via MCP resources with override support.
//!
//! Skills can be overridden by placing custom SKILL.md files in:
//! - `.task-graph/skills/{name}/SKILL.md` (project-local)
//!
//! The lookup order is:
//! 1. Project-local override (`.task-graph/skills/`)
//! 2. Embedded default (compiled into binary)
//!
//! ## Security
//!
//! Custom (non-override) skills require explicit approval before content is served.
//! Approvals are stored in `.task-graph/skills/.approved` (one skill name per line).
//! Built-in skills and overrides of built-in skills are trusted by default.

use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Embedded skill content (compiled into binary).
/// These are the SKILL.md files from the skills/ directory.
pub mod embedded {
    pub const BASICS: &str = include_str!("../../skills/task-graph-basics/SKILL.md");
    pub const COORDINATOR: &str = include_str!("../../skills/task-graph-coordinator/SKILL.md");
    pub const WORKER: &str = include_str!("../../skills/task-graph-worker/SKILL.md");
    pub const REPORTING: &str = include_str!("../../skills/task-graph-reporting/SKILL.md");
    pub const MIGRATION: &str = include_str!("../../skills/task-graph-migration/SKILL.md");
    pub const REPAIR: &str = include_str!("../../skills/task-graph-repair/SKILL.md");
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
        description: "Foundation - tool reference, connection workflow, shared patterns",
        role: "foundation",
    },
    SkillInfo {
        name: "coordinator",
        full_name: "task-graph-coordinator",
        description: "Orchestrator - create task trees, assign work, monitor progress",
        role: "coordinator",
    },
    SkillInfo {
        name: "worker",
        full_name: "task-graph-worker",
        description: "Executor - claim tasks, report progress, complete work",
        role: "worker",
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
        "coordinator" | "task-graph-coordinator" => Some(embedded::COORDINATOR),
        "worker" | "task-graph-worker" => Some(embedded::WORKER),
        "reporting" | "task-graph-reporting" => Some(embedded::REPORTING),
        "migration" | "task-graph-migration" => Some(embedded::MIGRATION),
        "repair" | "task-graph-repair" => Some(embedded::REPAIR),
        _ => None,
    }
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

/// Get the path to the approvals file.
fn get_approvals_path(skills_dir: &Path) -> PathBuf {
    skills_dir.join(".approved")
}

/// Load the set of approved custom skill names.
pub fn load_approved_skills(skills_dir: Option<&Path>) -> HashSet<String> {
    let mut approved = HashSet::new();

    if let Some(dir) = skills_dir {
        let approvals_path = get_approvals_path(dir);
        if approvals_path.exists()
            && let Ok(content) = std::fs::read_to_string(&approvals_path) {
                for line in content.lines() {
                    let name = line.trim();
                    if !name.is_empty() && !name.starts_with('#') {
                        approved.insert(name.to_string());
                    }
                }
            }
    }

    approved
}

/// Check if a custom skill is approved.
pub fn is_skill_approved(skills_dir: Option<&Path>, name: &str) -> bool {
    // Built-in skills are always approved
    if is_builtin_skill(name) {
        return true;
    }

    let approved = load_approved_skills(skills_dir);
    let normalized = normalize_name(name);
    approved.contains(normalized) || approved.contains(name)
}

/// Approve a custom skill by adding it to the approvals file.
pub fn approve_skill(skills_dir: &Path, name: &str) -> Result<()> {
    validate_skill_name(name)?;

    // Don't allow approving built-in skills (they're already trusted)
    if is_builtin_skill(name) {
        return Ok(());
    }

    let approvals_path = get_approvals_path(skills_dir);

    // Load existing approvals
    let mut approved = load_approved_skills(Some(skills_dir));
    let normalized = normalize_name(name).to_string();

    if approved.contains(&normalized) {
        return Ok(()); // Already approved
    }

    approved.insert(normalized.clone());

    // Write back
    let content: Vec<String> = approved.into_iter().collect();
    std::fs::write(&approvals_path, content.join("\n") + "\n")?;

    Ok(())
}

/// Revoke approval for a custom skill.
pub fn revoke_skill(skills_dir: &Path, name: &str) -> Result<()> {
    validate_skill_name(name)?;

    let approvals_path = get_approvals_path(skills_dir);
    let mut approved = load_approved_skills(Some(skills_dir));
    let normalized = normalize_name(name).to_string();

    approved.remove(&normalized);
    approved.remove(name);

    // Write back
    let content: Vec<String> = approved.into_iter().collect();
    if content.is_empty() {
        // Remove file if empty
        let _ = std::fs::remove_file(&approvals_path);
    } else {
        std::fs::write(&approvals_path, content.join("\n") + "\n")?;
    }

    Ok(())
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
                && canonical_override.starts_with(&canonical_dir) && override_path.exists() {
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

/// List all skills as JSON, indicating which are overridden and approved.
pub fn list_skills(skills_dir: Option<&Path>) -> Result<Value> {
    let approved_set = load_approved_skills(skills_dir);

    let mut skills_list: Vec<Value> = SKILLS
        .iter()
        .map(|s| {
            let overridden = is_overridden(skills_dir, s.name);
            json!({
                "name": s.name,
                "full_name": s.full_name,
                "description": s.description,
                "role": s.role,
                "uri": format!("skills://{}", s.name),
                "overridden": overridden,
                "source": if overridden { "local" } else { "embedded" },
                "approved": true,  // Built-in skills are always approved
                "trusted": true,
            })
        })
        .collect();

    // Also check for custom skills in the override directory
    if let Some(dir) = skills_dir
        && dir.exists()
            && let Ok(entries) = std::fs::read_dir(dir) {
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

                            // It's a custom skill - check if approved
                            let is_approved =
                                approved_set.contains(normalized) || approved_set.contains(&name);

                            skills_list.push(json!({
                                "name": normalized,
                                "full_name": name,
                                "description": "Custom skill (requires approval)",
                                "role": "custom",
                                "uri": format!("skills://{}", normalized),
                                "overridden": false,
                                "source": "local",
                                "approved": is_approved,
                                "trusted": false,
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
/// For custom (non-built-in) skills, checks approval status.
/// Unapproved custom skills return a preview instead of full content.
pub fn get_skill_resource(skills_dir: Option<&Path>, name: &str) -> Result<Value> {
    validate_skill_name(name)?;

    let normalized = normalize_name(name);
    let is_builtin = is_builtin_skill(name);
    let overridden = is_overridden(skills_dir, name);
    let approved = is_skill_approved(skills_dir, name);

    let info = SKILLS.iter().find(|s| s.name == normalized);

    // For custom unapproved skills, don't return full content
    if !is_builtin && !approved {
        // Try to get a preview (first 500 chars)
        let preview = match get_skill(skills_dir, name) {
            Ok(content) => {
                let preview_len = content.len().min(500);
                let mut preview: String = content.chars().take(preview_len).collect();
                if content.len() > 500 {
                    preview.push_str("\n\n[... content truncated - skill requires approval ...]");
                }
                Some(preview)
            }
            Err(_) => None,
        };

        return Ok(json!({
            "name": normalized,
            "full_name": name,
            "role": "custom",
            "description": "Custom skill (requires approval)",
            "content": null,
            "preview": preview,
            "mime_type": "text/markdown",
            "overridden": false,
            "source": "local",
            "approved": false,
            "trusted": false,
            "error": "Skill requires approval. Use approve_skill tool or add to .task-graph/skills/.approved",
        }));
    }

    // Approved or built-in - return full content
    let content = get_skill(skills_dir, name)?;

    Ok(json!({
        "name": info.map(|i| i.name).unwrap_or(normalized),
        "full_name": info.map(|i| i.full_name).unwrap_or(name),
        "role": info.map(|i| i.role).unwrap_or("custom"),
        "description": info.map(|i| i.description).unwrap_or("Custom skill"),
        "content": content,
        "preview": null,
        "mime_type": "text/markdown",
        "overridden": overridden,
        "source": if is_builtin && !overridden { "embedded" } else { "local" },
        "approved": true,
        "trusted": is_builtin,
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
        assert_eq!(normalize_name("task-graph-coordinator"), "coordinator");
    }

    #[test]
    fn test_get_skill_embedded() {
        let content = get_skill(None, "basics").unwrap();
        assert!(!content.is_empty());
        assert!(content.starts_with("---"));
    }

    #[test]
    fn test_list_skills() {
        let result = list_skills(None).unwrap();
        assert_eq!(result["count"], 6);
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
