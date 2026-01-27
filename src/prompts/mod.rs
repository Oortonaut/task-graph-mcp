//! Transition prompts system.
//!
//! Loads and delivers prompts when tasks transition between states/phases.
//! Prompts are markdown files with naming convention:
//! - `enter~{status}.md` - entering a status (any phase)
//! - `exit~{status}.md` - exiting a status (any phase)
//! - `enter%{phase}.md` - entering a phase (any status)
//! - `exit%{phase}.md` - exiting a phase (any status)
//! - `enter~{status}%{phase}.md` - entering specific status+phase combo
//! - `exit~{status}%{phase}.md` - exiting specific status+phase combo
//!
//! Files are loaded from layered directories (user overrides project overrides defaults):
//! 1. ~/.task-graph/prompts/
//! 2. .task-graph/prompts/
//! 3. [install]/defaults/prompts/

use std::path::PathBuf;

/// Default prompts embedded at compile time.
pub mod defaults {
    pub const ENTER_WORKING: &str = include_str!("../defaults/prompts/enter~working.md");
}

/// Configuration for prompt directories.
#[derive(Debug, Clone)]
pub struct PromptsConfig {
    /// User-level prompts directory (~/.task-graph/prompts/)
    pub user_dir: Option<PathBuf>,
    /// Project-level prompts directory (.task-graph/prompts/)
    pub project_dir: Option<PathBuf>,
}

impl PromptsConfig {
    /// Create a new PromptsConfig with the given base directories.
    pub fn new(user_home: Option<PathBuf>, project_root: Option<PathBuf>) -> Self {
        Self {
            user_dir: user_home.map(|h| h.join(".task-graph").join("prompts")),
            project_dir: project_root.map(|p| p.join(".task-graph").join("prompts")),
        }
    }
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            user_dir: std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .ok()
                .map(|h| PathBuf::from(h).join(".task-graph").join("prompts")),
            project_dir: Some(PathBuf::from(".task-graph/prompts")),
        }
    }
}

/// Load a prompt file by trigger name.
///
/// Checks directories in order: user, project, then embedded defaults.
/// Returns None if no prompt file exists for this trigger.
pub fn load_prompt(trigger: &str, config: &PromptsConfig) -> Option<String> {
    let filename = format!("{}.md", trigger);

    // Check user directory first
    if let Some(ref user_dir) = config.user_dir {
        let path = user_dir.join(&filename);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Some(content);
            }
        }
    }

    // Check project directory
    if let Some(ref project_dir) = config.project_dir {
        let path = project_dir.join(&filename);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Some(content);
            }
        }
    }

    // Check embedded defaults
    load_embedded_prompt(trigger)
}

/// Load an embedded default prompt.
fn load_embedded_prompt(trigger: &str) -> Option<String> {
    match trigger {
        "enter~working" => Some(defaults::ENTER_WORKING.to_string()),
        _ => None,
    }
}

/// Get the list of triggers that should fire for a state transition.
///
/// Order: exits (specific → general), then enters (general → specific)
pub fn get_transition_triggers(
    old_status: &str,
    old_phase: Option<&str>,
    new_status: &str,
    new_phase: Option<&str>,
) -> Vec<String> {
    let mut triggers = Vec::new();

    let status_changed = old_status != new_status;
    let phase_changed = old_phase != new_phase;

    // === EXITS (specific → general) ===

    // Exit combo (if either changed and had a phase)
    if (status_changed || phase_changed) && old_phase.is_some() {
        if let Some(op) = old_phase {
            triggers.push(format!("exit~{}%{}", old_status, op));
        }
    }

    // Exit phase (if phase changed)
    if phase_changed {
        if let Some(op) = old_phase {
            triggers.push(format!("exit%{}", op));
        }
    }

    // Exit status (if status changed)
    if status_changed {
        triggers.push(format!("exit~{}", old_status));
    }

    // === ENTERS (general → specific) ===

    // Enter status (if status changed)
    if status_changed {
        triggers.push(format!("enter~{}", new_status));
    }

    // Enter phase (if phase changed)
    if phase_changed {
        if let Some(np) = new_phase {
            triggers.push(format!("enter%{}", np));
        }
    }

    // Enter combo (if either changed and has a phase)
    if (status_changed || phase_changed) && new_phase.is_some() {
        if let Some(np) = new_phase {
            triggers.push(format!("enter~{}%{}", new_status, np));
        }
    }

    triggers
}

/// Get all prompts that should be delivered for a state transition.
///
/// Returns a vector of prompt strings (caller concatenates as needed).
pub fn get_transition_prompts(
    old_status: &str,
    old_phase: Option<&str>,
    new_status: &str,
    new_phase: Option<&str>,
    config: &PromptsConfig,
) -> Vec<String> {
    get_transition_triggers(old_status, old_phase, new_status, new_phase)
        .iter()
        .filter_map(|trigger| load_prompt(trigger, config))
        .collect()
}

/// List all available prompt files across all directories.
pub fn list_available_prompts(config: &PromptsConfig) -> Vec<String> {
    let mut prompts = Vec::new();

    // Embedded defaults
    prompts.push("enter~working".to_string());

    // Scan directories
    for dir in [&config.user_dir, &config.project_dir].into_iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    if let Some(name_str) = name.to_str() {
                        if !prompts.contains(&name_str.to_string()) {
                            prompts.push(name_str.to_string());
                        }
                    }
                }
            }
        }
    }

    prompts.sort();
    prompts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triggers_status_change_only() {
        let triggers = get_transition_triggers("pending", None, "working", None);
        assert_eq!(triggers, vec!["exit~pending", "enter~working"]);
    }

    #[test]
    fn test_triggers_phase_change_only() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "working", Some("review"));
        assert_eq!(
            triggers,
            vec![
                "exit~working%diagnose",
                "exit%diagnose",
                "enter%review",
                "enter~working%review"
            ]
        );
    }

    #[test]
    fn test_triggers_both_change() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "finished", Some("review"));
        assert_eq!(
            triggers,
            vec![
                "exit~working%diagnose",
                "exit%diagnose",
                "exit~working",
                "enter~finished",
                "enter%review",
                "enter~finished%review"
            ]
        );
    }

    #[test]
    fn test_triggers_enter_phase_from_none() {
        let triggers = get_transition_triggers("working", None, "working", Some("diagnose"));
        assert_eq!(
            triggers,
            vec!["enter%diagnose", "enter~working%diagnose"]
        );
    }

    #[test]
    fn test_triggers_exit_phase_to_none() {
        let triggers = get_transition_triggers("working", Some("diagnose"), "working", None);
        assert_eq!(
            triggers,
            vec!["exit~working%diagnose", "exit%diagnose"]
        );
    }

    #[test]
    fn test_no_triggers_when_unchanged() {
        let triggers =
            get_transition_triggers("working", Some("diagnose"), "working", Some("diagnose"));
        assert!(triggers.is_empty());
    }
}
