//! Embedded workflow and overlay files.
//!
//! These files are compiled into the binary so they're always available,
//! even when the install directory doesn't exist (e.g., new projects).

/// Embedded workflow YAML files.
pub mod workflows {
    pub const HIERARCHICAL: &str = include_str!("../../config/workflow-hierarchical.yaml");
    pub const KANBAN: &str = include_str!("../../config/workflow-kanban.yaml");
    pub const PUSH: &str = include_str!("../../config/workflow-push.yaml");
    pub const RELAY: &str = include_str!("../../config/workflow-relay.yaml");
    pub const SOLO: &str = include_str!("../../config/workflow-solo.yaml");
    pub const SPRINT: &str = include_str!("../../config/workflow-sprint.yaml");
    pub const SWARM: &str = include_str!("../../config/workflow-swarm.yaml");

    /// Get all embedded workflow names and their content.
    pub fn all() -> Vec<(&'static str, &'static str)> {
        vec![
            ("hierarchical", HIERARCHICAL),
            ("kanban", KANBAN),
            ("push", PUSH),
            ("relay", RELAY),
            ("solo", SOLO),
            ("sprint", SPRINT),
            ("swarm", SWARM),
        ]
    }

    /// Get embedded workflow by name.
    pub fn get(name: &str) -> Option<&'static str> {
        match name {
            "hierarchical" => Some(HIERARCHICAL),
            "kanban" => Some(KANBAN),
            "push" => Some(PUSH),
            "relay" => Some(RELAY),
            "solo" => Some(SOLO),
            "sprint" => Some(SPRINT),
            "swarm" => Some(SWARM),
            _ => None,
        }
    }

    /// List all embedded workflow names.
    pub fn names() -> Vec<&'static str> {
        vec![
            "hierarchical",
            "kanban",
            "push",
            "relay",
            "solo",
            "sprint",
            "swarm",
        ]
    }
}

/// Embedded overlay YAML files.
pub mod overlays {
    pub const GIT: &str = include_str!("../../config/overlay-git.yaml");
    pub const GIT_WORKTREE: &str = include_str!("../../config/overlay-git-worktree.yaml");
    pub const GOVERNANCE: &str = include_str!("../../config/overlay-governance.yaml");
    pub const REASONING: &str = include_str!("../../config/overlay-reasoning.yaml");
    pub const TROUBLESHOOTING: &str = include_str!("../../config/overlay-troubleshooting.yaml");

    /// Get all embedded overlay names and their content.
    pub fn all() -> Vec<(&'static str, &'static str)> {
        vec![
            ("git", GIT),
            ("git-worktree", GIT_WORKTREE),
            ("governance", GOVERNANCE),
            ("reasoning", REASONING),
            ("troubleshooting", TROUBLESHOOTING),
        ]
    }

    /// Get embedded overlay by name.
    pub fn get(name: &str) -> Option<&'static str> {
        match name {
            "git" => Some(GIT),
            "git-worktree" => Some(GIT_WORKTREE),
            "governance" => Some(GOVERNANCE),
            "reasoning" => Some(REASONING),
            "troubleshooting" => Some(TROUBLESHOOTING),
            _ => None,
        }
    }

    /// List all embedded overlay names.
    pub fn names() -> Vec<&'static str> {
        vec![
            "git",
            "git-worktree",
            "governance",
            "reasoning",
            "troubleshooting",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_workflows_parse() {
        for (name, content) in workflows::all() {
            let result: Result<serde_yaml::Value, _> = serde_yaml::from_str(content);
            assert!(
                result.is_ok(),
                "Failed to parse workflow '{}': {:?}",
                name,
                result.err()
            );
        }
    }

    #[test]
    fn test_embedded_overlays_parse() {
        for (name, content) in overlays::all() {
            let result: Result<serde_yaml::Value, _> = serde_yaml::from_str(content);
            assert!(
                result.is_ok(),
                "Failed to parse overlay '{}': {:?}",
                name,
                result.err()
            );
        }
    }

    #[test]
    fn test_workflow_get() {
        assert!(workflows::get("solo").is_some());
        assert!(workflows::get("swarm").is_some());
        assert!(workflows::get("nonexistent").is_none());
    }

    #[test]
    fn test_overlay_get() {
        assert!(overlays::get("git").is_some());
        assert!(overlays::get("troubleshooting").is_some());
        assert!(overlays::get("nonexistent").is_none());
    }
}
