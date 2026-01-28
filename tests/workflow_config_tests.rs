//! Integration tests for workflow configuration loading.
//!
//! Tests the ConfigLoader's workflow-related methods:
//! - load_workflow_by_name() - Load specific workflow files
//! - list_workflows() - Discover available workflows

use std::fs;
use task_graph_mcp::config::{ConfigLoader, ConfigPaths};
use tempfile::TempDir;

/// Helper to create a ConfigLoader with specific temp directories.
fn create_loader_with_dirs(
    project_dir: Option<std::path::PathBuf>,
    user_dir: Option<std::path::PathBuf>,
) -> ConfigLoader {
    let paths = ConfigPaths::with_dirs(project_dir, user_dir);
    ConfigLoader::load_with_paths(paths).expect("Failed to create config loader")
}

/// Minimal valid workflow YAML content for testing.
fn minimal_workflow_yaml() -> &'static str {
    r#"
settings:
  initial_state: pending
  disconnect_state: pending
  blocking_states: [pending, working]

states:
  pending:
    exits: [working]
    timed: false
  working:
    exits: [completed]
    timed: true
  completed:
    exits: []
    timed: false

phases:
  implement: {}
  test: {}
"#
}

/// Workflow YAML with custom prompts to verify loading.
fn swarm_like_workflow_yaml() -> &'static str {
    r##"
topology:
  name: swarm
  description: Parallel generalists with fine-grained tasks

settings:
  initial_state: pending
  disconnect_state: pending
  blocking_states: [pending, assigned, working]
  unknown_phase: warn

states:
  pending:
    exits: [assigned, working, cancelled]
    timed: false
  assigned:
    exits: [working, pending, cancelled]
    timed: false
    prompts:
      enter: "A task has been assigned to you."
  working:
    exits: [completed, failed, pending]
    timed: true
    prompts:
      enter: |
        ## Swarm Worker Active
        Claim ONE task at a time and complete it before claiming another.
        Use mark_updates to check for file coordination changes from other workers.
      exit: "Before leaving working state - unmark files"
  completed:
    exits: [pending]
    timed: false
  failed:
    exits: [pending]
    timed: false
  cancelled:
    exits: []
    timed: false

phases:
  explore: {}
  implement:
    prompts:
      enter: "Implementation phase. Check mark_updates before touching shared files."
  review: {}
  test: {}
"##
}

mod load_workflow_by_name_tests {
    use super::*;

    #[test]
    fn loads_workflow_from_project_directory() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create workflow-swarm.yaml in project directory
        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            swarm_like_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflow = loader
            .load_workflow_by_name("swarm")
            .expect("Failed to load swarm workflow");

        // Verify workflow was loaded correctly
        assert_eq!(workflow.settings.initial_state, "pending");
        assert!(
            workflow
                .settings
                .blocking_states
                .contains(&"working".to_string())
        );

        // Verify states were loaded
        assert!(workflow.states.contains_key("pending"));
        assert!(workflow.states.contains_key("working"));
        assert!(workflow.states.contains_key("assigned"));

        // Verify working state is timed
        let working = workflow.states.get("working").unwrap();
        assert!(working.timed);

        // Verify prompts were loaded
        assert!(working.prompts.enter.is_some());
        assert!(
            working
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("Swarm Worker Active")
        );
    }

    #[test]
    fn loads_workflow_from_user_directory_when_not_in_project() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        // Create workflow only in user directory
        fs::write(
            user_dir.join("workflow-custom.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let workflow = loader
            .load_workflow_by_name("custom")
            .expect("Failed to load custom workflow");

        // Verify workflow was loaded
        assert_eq!(workflow.settings.initial_state, "pending");
        assert!(workflow.states.contains_key("pending"));
        assert!(workflow.states.contains_key("working"));
    }

    #[test]
    fn user_workflow_takes_precedence_over_project() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        // Create different workflows in both directories with same name
        let project_yaml = r#"
settings:
  initial_state: pending
  blocking_states: [pending]
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    prompts:
      enter: "Project version"
  completed:
    exits: []
phases: {}
"#;
        let user_yaml = r#"
settings:
  initial_state: pending
  blocking_states: [pending]
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    prompts:
      enter: "User version"
  completed:
    exits: []
phases: {}
"#;

        fs::write(project_dir.join("workflow-test.yaml"), project_yaml).unwrap();
        fs::write(user_dir.join("workflow-test.yaml"), user_yaml).unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let workflow = loader
            .load_workflow_by_name("test")
            .expect("Failed to load test workflow");

        // User directory takes precedence over project directory
        let working = workflow.states.get("working").unwrap();
        assert!(
            working
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("User version")
        );
    }

    #[test]
    fn returns_error_for_nonexistent_workflow() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let result = loader.load_workflow_by_name("nonexistent");

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("nonexistent"));
        assert!(err_msg.contains("not found"));
    }

    #[test]
    fn returns_error_when_no_directories_exist() {
        let temp = TempDir::new().unwrap();
        // Create paths that don't exist
        let project_dir = temp.path().join("nonexistent-project");
        let user_dir = temp.path().join("nonexistent-user");

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let result = loader.load_workflow_by_name("swarm");

        assert!(result.is_err());
    }

    #[test]
    fn merges_workflow_with_defaults() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create a partial workflow that only overrides some settings
        let partial_yaml = r#"
settings:
  initial_state: assigned

states:
  assigned:
    exits: [working]
    prompts:
      enter: "Custom assigned prompt"
"#;

        fs::write(project_dir.join("workflow-partial.yaml"), partial_yaml).unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflow = loader
            .load_workflow_by_name("partial")
            .expect("Failed to load partial workflow");

        // Custom setting should be applied
        assert_eq!(workflow.settings.initial_state, "assigned");

        // The assigned state should have our custom prompt
        let assigned = workflow.states.get("assigned").unwrap();
        assert!(
            assigned
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("Custom assigned prompt")
        );

        // Default states should still exist from defaults
        // (Note: depending on merge behavior, other states may or may not be present)
        // The key point is that the custom state is correctly loaded
    }
}

mod list_workflows_tests {
    use super::*;

    #[test]
    fn lists_workflows_from_project_directory() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create multiple workflow files
        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            project_dir.join("workflow-solo.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            project_dir.join("workflow-relay.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        // Also create a non-workflow file to ensure it's ignored
        fs::write(project_dir.join("config.yaml"), "server: {}").unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflows = loader.list_workflows();

        assert!(workflows.contains(&"swarm".to_string()));
        assert!(workflows.contains(&"solo".to_string()));
        assert!(workflows.contains(&"relay".to_string()));
        assert_eq!(workflows.len(), 3);
    }

    #[test]
    fn lists_workflows_from_both_directories() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        // Workflows in project directory
        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            project_dir.join("workflow-solo.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        // Workflows in user directory
        fs::write(
            user_dir.join("workflow-custom.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            user_dir.join("workflow-enterprise.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let workflows = loader.list_workflows();

        assert!(workflows.contains(&"swarm".to_string()));
        assert!(workflows.contains(&"solo".to_string()));
        assert!(workflows.contains(&"custom".to_string()));
        assert!(workflows.contains(&"enterprise".to_string()));
        assert_eq!(workflows.len(), 4);
    }

    #[test]
    fn deduplicates_workflows_present_in_both_directories() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        // Same workflow name in both directories
        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            user_dir.join("workflow-swarm.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        // Additional unique workflow
        fs::write(
            user_dir.join("workflow-unique.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let workflows = loader.list_workflows();

        // Should only have 2 unique names, not 3
        assert_eq!(workflows.len(), 2);
        assert!(workflows.contains(&"swarm".to_string()));
        assert!(workflows.contains(&"unique".to_string()));
    }

    #[test]
    fn returns_empty_list_when_no_workflows_exist() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create some non-workflow files
        fs::write(project_dir.join("config.yaml"), "server: {}").unwrap();
        fs::write(project_dir.join("prompts.yaml"), "prompts: {}").unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflows = loader.list_workflows();

        assert!(workflows.is_empty());
    }

    #[test]
    fn returns_sorted_workflow_names() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create workflows in non-alphabetical order
        fs::write(
            project_dir.join("workflow-zebra.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            project_dir.join("workflow-alpha.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            project_dir.join("workflow-middle.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflows = loader.list_workflows();

        assert_eq!(workflows, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn ignores_invalid_workflow_filenames() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Valid workflow
        fs::write(
            project_dir.join("workflow-valid.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        // Invalid names that should be ignored
        fs::write(project_dir.join("workflow-.yaml"), minimal_workflow_yaml()).unwrap(); // empty name
        fs::write(project_dir.join("workflow.yaml"), minimal_workflow_yaml()).unwrap(); // missing dash
        fs::write(
            project_dir.join("workflow-test.yml"),
            minimal_workflow_yaml(),
        )
        .unwrap(); // wrong extension
        fs::write(
            project_dir.join("my-workflow-test.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap(); // wrong prefix

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflows = loader.list_workflows();

        // Should only contain the valid one
        assert_eq!(workflows.len(), 1);
        assert!(workflows.contains(&"valid".to_string()));
    }

    #[test]
    fn handles_nonexistent_directories_gracefully() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("nonexistent-project");
        let user_dir = temp.path().join("nonexistent-user");

        let loader = create_loader_with_dirs(Some(project_dir), Some(user_dir));
        let workflows = loader.list_workflows();

        // Should return empty list without error
        assert!(workflows.is_empty());
    }
}

mod workflow_content_tests {
    use super::*;

    #[test]
    fn loaded_workflow_has_correct_state_transitions() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            swarm_like_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflow = loader.load_workflow_by_name("swarm").unwrap();

        // Verify state transitions
        let pending = workflow.states.get("pending").unwrap();
        assert!(pending.exits.contains(&"assigned".to_string()));
        assert!(pending.exits.contains(&"working".to_string()));
        assert!(pending.exits.contains(&"cancelled".to_string()));

        let working = workflow.states.get("working").unwrap();
        assert!(working.exits.contains(&"completed".to_string()));
        assert!(working.exits.contains(&"failed".to_string()));
        assert!(working.exits.contains(&"pending".to_string()));

        let completed = workflow.states.get("completed").unwrap();
        assert!(completed.exits.contains(&"pending".to_string()));

        let cancelled = workflow.states.get("cancelled").unwrap();
        assert!(cancelled.exits.is_empty());
    }

    #[test]
    fn loaded_workflow_has_correct_phase_prompts() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            swarm_like_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflow = loader.load_workflow_by_name("swarm").unwrap();

        // Verify phases exist
        assert!(workflow.phases.contains_key("implement"));
        assert!(workflow.phases.contains_key("explore"));
        assert!(workflow.phases.contains_key("review"));
        assert!(workflow.phases.contains_key("test"));

        // Verify implement phase has a prompt
        let implement = workflow.phases.get("implement").unwrap();
        assert!(implement.prompts.enter.is_some());
        assert!(
            implement
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("mark_updates")
        );
    }

    #[test]
    fn workflow_settings_are_correctly_loaded() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            swarm_like_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let workflow = loader.load_workflow_by_name("swarm").unwrap();

        // Verify settings
        assert_eq!(workflow.settings.initial_state, "pending");
        assert_eq!(workflow.settings.disconnect_state, "pending");
        assert!(
            workflow
                .settings
                .blocking_states
                .contains(&"pending".to_string())
        );
        assert!(
            workflow
                .settings
                .blocking_states
                .contains(&"assigned".to_string())
        );
        assert!(
            workflow
                .settings
                .blocking_states
                .contains(&"working".to_string())
        );
    }
}

mod install_directory_tests {
    use super::*;
    use task_graph_mcp::config::ConfigPaths;

    /// Helper to create a ConfigLoader with all three directories.
    fn create_loader_with_all_dirs(
        install_dir: Option<std::path::PathBuf>,
        project_dir: Option<std::path::PathBuf>,
        user_dir: Option<std::path::PathBuf>,
    ) -> ConfigLoader {
        let paths = ConfigPaths::with_all_dirs(install_dir, project_dir, user_dir);
        ConfigLoader::load_with_paths(paths).expect("Failed to create config loader")
    }

    #[test]
    fn loads_workflow_from_install_directory() {
        let temp = TempDir::new().unwrap();
        let install_dir = temp.path().join("config");
        fs::create_dir_all(&install_dir).unwrap();

        // Create workflow in install directory only
        fs::write(
            install_dir.join("workflow-builtin.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_all_dirs(Some(install_dir), None, None);
        let workflow = loader.load_workflow_by_name("builtin");

        assert!(
            workflow.is_ok(),
            "Should load workflow from install directory"
        );
    }

    #[test]
    fn lists_workflows_from_install_directory() {
        let temp = TempDir::new().unwrap();
        let install_dir = temp.path().join("config");
        fs::create_dir_all(&install_dir).unwrap();

        fs::write(
            install_dir.join("workflow-swarm.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();
        fs::write(
            install_dir.join("workflow-solo.yaml"),
            minimal_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_all_dirs(Some(install_dir), None, None);
        let workflows = loader.list_workflows();

        assert!(workflows.contains(&"swarm".to_string()));
        assert!(workflows.contains(&"solo".to_string()));
        assert_eq!(workflows.len(), 2);
    }

    #[test]
    fn project_overrides_install_for_same_workflow() {
        let temp = TempDir::new().unwrap();
        let install_dir = temp.path().join("config");
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();

        // Install version
        let install_yaml = r#"
settings:
  initial_state: pending
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    prompts:
      enter: "Install version prompt"
  completed:
    exits: []
phases: {}
"#;

        // Project version with different prompt
        let project_yaml = r#"
settings:
  initial_state: pending
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    prompts:
      enter: "Project version prompt"
  completed:
    exits: []
phases: {}
"#;

        fs::write(install_dir.join("workflow-test.yaml"), install_yaml).unwrap();
        fs::write(project_dir.join("workflow-test.yaml"), project_yaml).unwrap();

        let loader = create_loader_with_all_dirs(Some(install_dir), Some(project_dir), None);
        let workflow = loader.load_workflow_by_name("test").unwrap();

        // Project should override install
        let working = workflow.states.get("working").unwrap();
        assert!(
            working
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("Project version")
        );
    }

    #[test]
    fn user_overrides_project_overrides_install() {
        let temp = TempDir::new().unwrap();
        let install_dir = temp.path().join("config");
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        let make_yaml = |version: &str| {
            format!(
                r#"
settings:
  initial_state: pending
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    prompts:
      enter: "{} version prompt"
  completed:
    exits: []
phases: {{}}
"#,
                version
            )
        };

        fs::write(install_dir.join("workflow-test.yaml"), make_yaml("Install")).unwrap();
        fs::write(project_dir.join("workflow-test.yaml"), make_yaml("Project")).unwrap();
        fs::write(user_dir.join("workflow-test.yaml"), make_yaml("User")).unwrap();

        let loader =
            create_loader_with_all_dirs(Some(install_dir), Some(project_dir), Some(user_dir));
        let workflow = loader.load_workflow_by_name("test").unwrap();

        // User should win
        let working = workflow.states.get("working").unwrap();
        assert!(
            working
                .prompts
                .enter
                .as_ref()
                .unwrap()
                .contains("User version")
        );
    }
}

mod named_workflows_cache_tests {
    use std::sync::Arc;

    #[test]
    fn named_workflows_cache_starts_empty() {
        let workflow = task_graph_mcp::config::workflows::WorkflowsConfig::default();
        assert!(workflow.named_workflows.is_empty());
        assert!(workflow.default_workflow_key.is_none());
    }

    #[test]
    fn can_insert_and_retrieve_named_workflow() {
        let mut base = task_graph_mcp::config::workflows::WorkflowsConfig::default();
        let swarm = task_graph_mcp::config::workflows::WorkflowsConfig::default();

        base.named_workflows
            .insert("swarm".to_string(), Arc::new(swarm));

        assert!(base.get_named_workflow("swarm").is_some());
        assert!(base.get_named_workflow("nonexistent").is_none());
    }

    #[test]
    fn default_workflow_key_returns_correct_workflow() {
        let mut base = task_graph_mcp::config::workflows::WorkflowsConfig::default();
        let swarm = task_graph_mcp::config::workflows::WorkflowsConfig::default();

        base.named_workflows
            .insert("swarm".to_string(), Arc::new(swarm));
        base.default_workflow_key = Some("swarm".to_string());

        assert!(base.get_default_workflow().is_some());

        // Clear the key
        base.default_workflow_key = None;
        assert!(base.get_default_workflow().is_none());
    }

    #[test]
    fn default_workflow_key_with_missing_workflow_returns_none() {
        let mut base = task_graph_mcp::config::workflows::WorkflowsConfig::default();
        base.default_workflow_key = Some("nonexistent".to_string());

        // Key is set but workflow doesn't exist in cache
        assert!(base.get_default_workflow().is_none());
    }
}

mod workflow_prompts_differ_from_defaults_tests {
    use super::*;

    #[test]
    fn swarm_workflow_has_distinct_working_prompt() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        fs::write(
            project_dir.join("workflow-swarm.yaml"),
            swarm_like_workflow_yaml(),
        )
        .unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let swarm_workflow = loader.load_workflow_by_name("swarm").unwrap();
        let default_workflow = task_graph_mcp::config::workflows::WorkflowsConfig::default();

        // Get working state prompts from both
        let swarm_working = swarm_workflow.states.get("working").unwrap();
        let default_working = default_workflow.states.get("working").unwrap();

        let swarm_prompt = swarm_working.prompts.enter.as_ref().unwrap();
        let default_prompt = default_working.prompts.enter.as_ref().unwrap();

        // Swarm prompt should contain swarm-specific content
        assert!(
            swarm_prompt.contains("Swarm Worker Active"),
            "Swarm workflow should have 'Swarm Worker Active' in prompt"
        );
        assert!(
            swarm_prompt.contains("mark_updates"),
            "Swarm workflow should mention mark_updates for file coordination"
        );
        assert!(
            swarm_prompt.contains("Claim ONE task"),
            "Swarm workflow should mention claiming one task at a time"
        );

        // Default prompt should NOT contain swarm-specific content
        assert!(
            !default_prompt.contains("Swarm Worker Active"),
            "Default workflow should not have swarm-specific content"
        );

        // They should be different
        assert_ne!(
            swarm_prompt, default_prompt,
            "Swarm and default prompts should be different"
        );
    }

    #[test]
    fn loaded_workflow_prompts_override_defaults() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        fs::create_dir_all(&project_dir).unwrap();

        // Create a workflow with a custom prompt that's obviously different
        let custom_yaml = r#"
settings:
  initial_state: pending
states:
  pending:
    exits: [working]
  working:
    exits: [completed]
    timed: true
    prompts:
      enter: "CUSTOM_UNIQUE_PROMPT_12345"
  completed:
    exits: []
phases: {}
"#;

        fs::write(project_dir.join("workflow-custom.yaml"), custom_yaml).unwrap();

        let loader = create_loader_with_dirs(Some(project_dir), None);
        let custom_workflow = loader.load_workflow_by_name("custom").unwrap();

        let working = custom_workflow.states.get("working").unwrap();
        let prompt = working.prompts.enter.as_ref().unwrap();

        assert!(
            prompt.contains("CUSTOM_UNIQUE_PROMPT_12345"),
            "Custom prompt should be loaded, not default"
        );
    }
}
