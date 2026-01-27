//! Integration tests for the database layer.
//!
//! These tests verify the core database operations using an in-memory SQLite database.
//! Tests are organized by module and functionality.

use task_graph_mcp::config::{AutoAdvanceConfig, DependenciesConfig, PhasesConfig, StatesConfig};
use task_graph_mcp::db::Database;
use task_graph_mcp::types::PRIORITY_DEFAULT;

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

/// Helper to create a default StatesConfig for testing.
fn default_states_config() -> StatesConfig {
    StatesConfig::default()
}

/// Helper to create a default DependenciesConfig for testing.
fn default_deps_config() -> DependenciesConfig {
    DependenciesConfig::default()
}

/// Helper to create a default AutoAdvanceConfig for testing (disabled).
fn default_auto_advance() -> AutoAdvanceConfig {
    AutoAdvanceConfig::default()
}

/// Helper to create a default PhasesConfig for testing.
fn default_phases_config() -> PhasesConfig {
    PhasesConfig::default()
}

mod agent_tests {
    use super::*;

    #[test]
    fn register_worker_creates_agent_with_defaults() {
        let db = setup_db();

        let agent = db
            .register_worker(None, vec![], false)
            .expect("Failed to register agent");

        assert!(agent.tags.is_empty());
        assert_eq!(agent.max_claims, i32::MAX); // unlimited by default
        assert!(agent.registered_at > 0);
        assert!(agent.last_heartbeat > 0);
    }

    #[test]
    fn register_worker_with_custom_tags() {
        let db = setup_db();

        let agent = db
            .register_worker(None, vec!["rust".to_string(), "backend".to_string()], false)
            .expect("Failed to register agent");

        assert_eq!(agent.tags, vec!["rust", "backend"]);
        assert_eq!(agent.max_claims, i32::MAX); // unlimited by default
    }

    #[test]
    fn register_worker_with_custom_id() {
        let db = setup_db();

        let agent = db
            .register_worker(Some("my-custom-agent".to_string()), vec![], false)
            .expect("Failed to register agent with custom ID");

        assert_eq!(agent.id, "my-custom-agent");
    }

    #[test]
    fn register_worker_rejects_id_over_36_chars() {
        let db = setup_db();

        let result = db.register_worker(
            Some("this-id-is-way-too-long-and-should-be-rejected-by-the-system".to_string()),
            vec![],
            false,
        );

        assert!(result.is_err());
    }

    #[test]
    fn register_worker_rejects_empty_id() {
        let db = setup_db();

        let result = db.register_worker(Some("".to_string()), vec![], false);

        assert!(result.is_err());
    }

    #[test]
    fn register_worker_rejects_duplicate_id() {
        let db = setup_db();

        // First registration should succeed
        let result = db.register_worker(Some("duplicate-agent".to_string()), vec![], false);
        assert!(result.is_ok());

        // Second registration with same ID should fail
        let result = db.register_worker(Some("duplicate-agent".to_string()), vec![], false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
    }

    #[test]
    fn register_worker_with_force_allows_reconnection() {
        let db = setup_db();

        // First registration
        let agent1 = db
            .register_worker(
                Some("force-agent".to_string()),
                vec!["old-tag".to_string()],
                false,
            )
            .unwrap();
        assert_eq!(agent1.tags, vec!["old-tag"]);

        // Second registration without force should fail
        let result = db.register_worker(
            Some("force-agent".to_string()),
            vec!["new-tag".to_string()],
            false,
        );
        assert!(result.is_err());

        // Third registration with force=true should succeed and update tags
        let agent2 = db
            .register_worker(
                Some("force-agent".to_string()),
                vec!["new-tag".to_string()],
                true,
            )
            .unwrap();
        assert_eq!(agent2.tags, vec!["new-tag"]);
    }

    #[test]
    fn get_worker_returns_registered_agent() {
        let db = setup_db();
        let agent = db
            .register_worker(None, vec!["finder".to_string()], false)
            .unwrap();

        let found = db.get_worker(&agent.id).unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().tags, vec!["finder"]);
    }

    #[test]
    fn get_worker_returns_none_for_unknown_id() {
        let db = setup_db();

        let result = db.get_worker("unknown-agent-id").unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn update_worker_modifies_properties() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        let updated = db
            .update_worker(&agent.id, Some(vec!["new-tag".to_string()]), Some(3))
            .unwrap();

        assert_eq!(updated.tags, vec!["new-tag"]);
        assert_eq!(updated.max_claims, 3);
    }

    #[test]
    fn heartbeat_updates_last_heartbeat_and_returns_claim_count() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let original_heartbeat = agent.last_heartbeat;

        // Small delay to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        let claim_count = db.heartbeat(&agent.id).unwrap();

        assert_eq!(claim_count, 0);
        let updated = db.get_worker(&agent.id).unwrap().unwrap();
        assert!(updated.last_heartbeat >= original_heartbeat);
    }

    #[test]
    fn heartbeat_fails_for_unknown_agent() {
        let db = setup_db();

        let result = db.heartbeat("unknown-agent");

        assert!(result.is_err());
    }

    #[test]
    fn unregister_worker_removes_agent() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        db.unregister_worker(&agent.id, "pending").unwrap();

        let found = db.get_worker(&agent.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn list_workers_returns_all_registered_agents() {
        let db = setup_db();
        db.register_worker(None, vec!["agent1".to_string()], false)
            .unwrap();
        db.register_worker(None, vec!["agent2".to_string()], false)
            .unwrap();

        let agents = db.list_workers().unwrap();

        assert_eq!(agents.len(), 2);
    }

    #[test]
    fn auto_generated_worker_ids_are_unique_petnames() {
        let db = setup_db();

        // Register multiple workers without specifying IDs
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        let agent3 = db.register_worker(None, vec![], false).unwrap();

        // All IDs should be unique
        assert_ne!(agent1.id, agent2.id);
        assert_ne!(agent2.id, agent3.id);
        assert_ne!(agent1.id, agent3.id);

        // IDs should be human-readable (contain hyphens, not UUID format)
        assert!(
            agent1.id.contains('-'),
            "Expected petname format with hyphens, got: {}",
            agent1.id
        );
        assert!(
            !agent1.id.contains("0000"),
            "ID looks like UUID, expected petname: {}",
            agent1.id
        );

        // IDs should be reasonably short (petnames are typically < 30 chars)
        assert!(
            agent1.id.len() < 36,
            "ID too long, expected petname: {}",
            agent1.id
        );
    }
}

mod task_tests {
    use super::*;

    #[test]
    fn create_task_with_minimal_fields() {
        let db = setup_db();
        let states_config = default_states_config();

        let task = db
            .create_task(
                None,                    // id
                "Test Task".to_string(), // description
                None,                    // parent_id
                None, // phase
                None,                    // priority
                None,                    // points
                None,                    // time_estimate
                None,                    // needed_tags
                None,                    // wanted_tags
                None,                    // tags
                &states_config,
            )
            .unwrap();

        assert_eq!(task.title, "Test Task");
        assert_eq!(task.description, Some("Test Task".to_string()));
        assert_eq!(task.status, "pending");
        assert_eq!(task.priority, PRIORITY_DEFAULT);
        assert!(task.worker_id.is_none());
    }

    #[test]
    fn create_task_with_all_fields() {
        let db = setup_db();
        let states_config = default_states_config();

        let task = db
            .create_task(
                None,                                  // id
                "Full Task - Description".to_string(), // description
                None,                                  // parent_id
                None, // phase
                Some(8),
                Some(5),
                Some(3600000),
                Some(vec!["rust".to_string()]),    // needed_tags
                Some(vec!["backend".to_string()]), // wanted_tags
                None,                              // tags
                &states_config,
            )
            .unwrap();

        assert_eq!(task.title, "Full Task - Description");
        assert_eq!(
            task.description,
            Some("Full Task - Description".to_string())
        );
        assert_eq!(task.priority, 8);
        assert_eq!(task.points, Some(5));
        assert_eq!(task.time_estimate_ms, Some(3600000));
        assert_eq!(task.needed_tags, vec!["rust"]);
        assert_eq!(task.wanted_tags, vec!["backend"]);
    }

    #[test]
    fn create_task_with_parent_creates_contains_dependency() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task(
                None,
                "Parent".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        let child1 = db
            .create_task(
                None,
                "Child 1".to_string(),
                Some(parent.id.clone()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let child2 = db
            .create_task(
                None,
                "Child 2".to_string(),
                Some(parent.id.clone()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Verify parent-child relationships via contains dependency
        let children = db.get_children(&parent.id).unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|c| c.id == child1.id));
        assert!(children.iter().any(|c| c.id == child2.id));
    }

    #[test]
    fn get_task_returns_existing_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Find Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        let found = db.get_task(&task.id).unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Find Me");
    }

    #[test]
    fn get_task_returns_none_for_unknown_id() {
        let db = setup_db();
        let unknown_id = "non-existent-task-id";

        let result = db.get_task(unknown_id).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn update_task_modifies_properties() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Original".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        let updated = db
            .update_task(
                &task.id,
                Some("Updated".to_string()),
                Some(Some("New Description".to_string())),
                Some("in_progress".to_string()),
                Some(8),
                None,
                None,
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.description, Some("New Description".to_string()));
        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.priority, 8);
    }

    #[test]
    fn update_task_to_completed_sets_completed_at() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Complete Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        assert!(task.completed_at.is_none());

        // Need to transition through in_progress first (pending -> in_progress -> completed)
        db.update_task(
            &task.id,
            None,
            None,
            Some("in_progress".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        let updated = db
            .update_task(
                &task.id,
                None,
                None,
                Some("completed".to_string()),
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn delete_task_removes_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Delete Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Hard delete with obliterate=true
        db.delete_task(&task.id, "test-worker", false, None, true, false)
            .unwrap();

        let found = db.get_task(&task.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn delete_task_without_cascade_fails_if_has_children() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task(
                None,
                "Parent".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.create_task(
            None,
            "Child".to_string(),
            Some(parent.id.clone()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        // Try to delete parent without cascade - should fail
        let result = db.delete_task(&parent.id, "test-worker", false, None, true, false);

        assert!(result.is_err());
    }

    #[test]
    fn delete_task_with_cascade_removes_children() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task(
                None,
                "Parent".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let child = db
            .create_task(
                None,
                "Child".to_string(),
                Some(parent.id.clone()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Delete parent with cascade - should delete both parent and child
        db.delete_task(&parent.id, "test-worker", true, None, true, false)
            .unwrap();

        assert!(db.get_task(&parent.id).unwrap().is_none());
        assert!(db.get_task(&child.id).unwrap().is_none());
    }

    #[test]
    fn get_children_returns_direct_children_in_order() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task(
                None,
                "Parent".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.create_task(
            None,
            "Child 1".to_string(),
            Some(parent.id.clone()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.create_task(
            None,
            "Child 2".to_string(),
            Some(parent.id.clone()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let children = db.get_children(&parent.id).unwrap();

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].title, "Child 1");
        assert_eq!(children[1].title, "Child 2");
    }

    #[test]
    fn list_tasks_filters_by_status() {
        let db = setup_db();
        let states_config = default_states_config();
        db.create_task(
            None,
            "Pending".to_string(),
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        let task2 = db
            .create_task(
                None,
                "Completed".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        // Transition through in_progress to completed
        db.update_task(
            &task2.id,
            None,
            None,
            Some("in_progress".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.update_task(
            &task2.id,
            None,
            None,
            Some("completed".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let pending = db
            .list_tasks(Some("pending"), None, None, None, None, None, None)
            .unwrap();
        let completed = db
            .list_tasks(Some("completed"), None, None, None, None, None, None)
            .unwrap();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "Pending");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].title, "Completed");
    }

    /// Test that the tool-level create function properly handles needed_tags and wanted_tags.
    /// This is a regression test for BUG-001 where these parameters were silently ignored.
    #[test]
    fn create_tool_stores_needed_and_wanted_tags() {
        use serde_json::json;
        use task_graph_mcp::tools::tasks::create;

        let db = setup_db();
        let states_config = default_states_config();
        let phases_config = default_phases_config();

        // Call the tool-level create function with needed_tags and wanted_tags
        let args = json!({
            "description": "Task with tags",
            "needed_tags": ["backend", "admin"],
            "wanted_tags": ["testing", "senior"]
        });

        let result = create(&db, &states_config, &phases_config, args).expect("create should succeed");

        // Extract the task ID from the result
        let task_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .expect("result should have id");

        // Fetch the task and verify the tags were stored
        let task = db.get_task(task_id).unwrap().expect("task should exist");

        assert_eq!(task.needed_tags, vec!["backend", "admin"]);
        assert_eq!(task.wanted_tags, vec!["testing", "senior"]);
    }
}

mod task_claiming_tests {
    use super::*;

    #[test]
    fn claim_task_assigns_owner_and_updates_status() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Claim Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        let claimed = db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        assert_eq!(claimed.worker_id, Some(agent.id.clone()));
        assert_eq!(claimed.status, "in_progress");
        assert!(claimed.claimed_at.is_some());
        assert!(claimed.started_at.is_some());
    }

    #[test]
    fn claim_task_fails_if_already_claimed() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Claimed".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();
        let result = db.claim_task(&task.id, &agent2.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_fails_if_agent_missing_needed_tag() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db
            .register_worker(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                None,
                "Rust Task".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                Some(vec!["rust".to_string()]), // needs rust tag
                None,
                None,
                &states_config,
            )
            .unwrap();

        let result = db.claim_task(&task.id, &agent.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_succeeds_if_agent_has_needed_tags() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db
            .register_worker(None, vec!["rust".to_string(), "backend".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                None,
                "Rust Task".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                Some(vec!["rust".to_string()]),
                None,
                None,
                &states_config,
            )
            .unwrap();

        let result = db.claim_task(&task.id, &agent.id, &states_config);

        assert!(result.is_ok());
    }

    #[test]
    fn claim_task_fails_if_agent_has_none_of_wanted_tags() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db
            .register_worker(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                None,
                "Flexible Task".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                Some(vec!["rust".to_string(), "go".to_string()]), // wants rust OR go
                None,
                &states_config,
            )
            .unwrap();

        let result = db.claim_task(&task.id, &agent.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn release_task_clears_owner_and_resets_status() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Release Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        db.release_task(&task.id, &agent.id, &states_config)
            .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.worker_id.is_none());
        assert_eq!(updated.status, "pending");
    }

    #[test]
    fn release_task_fails_if_not_owner() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Owned".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        let result = db.release_task(&task.id, &agent2.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn force_release_clears_owner_regardless() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Force".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        db.force_release(&task.id, &states_config).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.worker_id.is_none());
    }

    // Tests for unified update with claim/release behavior
    #[test]
    fn update_to_timed_state_claims_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Update Claim".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Update to in_progress (timed state) should claim the task
        let (updated, _unblocked, auto_advanced) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("in_progress".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None, // needed_tags, wanted_tags, time_estimate_ms, reason
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.worker_id, Some(agent.id.clone()));
        assert!(updated.claimed_at.is_some());
        assert!(auto_advanced.is_empty()); // No auto-advance with default config
    }

    #[test]
    fn update_from_timed_to_non_timed_releases_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Update Release".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // First claim via update
        db.update_task_unified(
            &task.id,
            &agent.id,
            None, // assignee
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None, // needed_tags, wanted_tags, time_estimate_ms, reason
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        )
        .unwrap();

        // Update back to pending (non-timed) should release
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("pending".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None, // needed_tags, wanted_tags, time_estimate_ms, reason
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "pending");
        assert!(updated.worker_id.is_none());
        assert!(updated.claimed_at.is_none());
    }

    #[test]
    fn update_with_force_claims_from_another_agent() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Force Update".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Agent1 claims the task
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        // Agent2 force claims via update
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent2.id,
                None, // assignee
                None,
                None,
                Some("in_progress".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None, // needed_tags, wanted_tags, time_estimate_ms, reason
                true, // force
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.worker_id, Some(agent2.id.clone()));
    }

    #[test]
    fn update_without_force_fails_if_claimed_by_another() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "No Force".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Agent1 claims the task
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        // Agent2 tries to claim without force - should fail
        let result = db.update_task_unified(
            &task.id,
            &agent2.id,
            None, // assignee
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,  // needed_tags, wanted_tags, time_estimate_ms, reason
            false, // no force
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_validates_tag_affinity_on_claim() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db
            .register_worker(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                None,
                "Needs Rust".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                Some(vec!["rust".to_string()]), // needed_tags
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Update to claim should fail due to missing tag
        let result = db.update_task_unified(
            &task.id,
            &agent.id,
            None, // assignee
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None, // needed_tags, wanted_tags, time_estimate_ms, reason
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_to_completed_clears_ownership() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Complete Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Claim the task
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        // Complete via update
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None, // needed_tags, wanted_tags, time_estimate_ms, reason
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "completed");
        assert!(updated.worker_id.is_none());
        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn update_between_two_timed_states_preserves_ownership() {
        use std::collections::HashMap;
        use task_graph_mcp::config::StateDefinition;

        let db = setup_db();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();

        // Create a custom StatesConfig with two timed states
        let mut definitions = HashMap::new();
        definitions.insert(
            "pending".to_string(),
            StateDefinition {
                exits: vec!["in_progress".to_string(), "cancelled".to_string()],
                timed: false,
            },
        );
        definitions.insert(
            "in_progress".to_string(),
            StateDefinition {
                exits: vec![
                    "reviewing".to_string(),
                    "completed".to_string(),
                    "failed".to_string(),
                    "pending".to_string(),
                ],
                timed: true,
            },
        );
        definitions.insert(
            "reviewing".to_string(),
            StateDefinition {
                exits: vec![
                    "in_progress".to_string(),
                    "completed".to_string(),
                    "failed".to_string(),
                ],
                timed: true, // Second timed state
            },
        );
        definitions.insert(
            "completed".to_string(),
            StateDefinition {
                exits: vec![],
                timed: false,
            },
        );
        definitions.insert(
            "failed".to_string(),
            StateDefinition {
                exits: vec!["pending".to_string()],
                timed: false,
            },
        );
        definitions.insert(
            "cancelled".to_string(),
            StateDefinition {
                exits: vec![],
                timed: false,
            },
        );

        let states_config = task_graph_mcp::config::StatesConfig {
            initial: "pending".to_string(),
            disconnect_state: "pending".to_string(),
            blocking_states: vec![
                "pending".to_string(),
                "in_progress".to_string(),
                "reviewing".to_string(),
            ],
            definitions,
        };

        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Timed to Timed".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Claim via transition to first timed state
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("in_progress".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.worker_id, Some(agent.id.clone()));

        // Transition to second timed state - should preserve ownership
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("reviewing".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "reviewing");
        assert_eq!(updated.worker_id, Some(agent.id.clone())); // Still owned
        assert!(updated.claimed_at.is_some()); // Still has claimed_at

        // Verify state history was recorded for both transitions
        let history = db.get_task_state_history(&task.id).unwrap();
        // Should have: pending (initial), in_progress, reviewing
        assert!(
            history.len() >= 3,
            "Expected at least 3 history entries, got {}",
            history.len()
        );

        let states: Vec<&str> = history.iter().map(|e| e.status.as_deref().unwrap_or("")).collect();
        assert!(
            states.contains(&"pending"),
            "History should contain 'pending'"
        );
        assert!(
            states.contains(&"in_progress"),
            "History should contain 'in_progress'"
        );
        assert!(
            states.contains(&"reviewing"),
            "History should contain 'reviewing'"
        );
    }

    #[test]
    fn update_to_same_state_succeeds() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Same State".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Claim the task first
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        let claimed = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(claimed.status, "in_progress");

        // Update to the same state (in_progress -> in_progress)
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("in_progress".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.worker_id, Some(agent.id.clone()));

        // Verify no additional history was recorded (status didn't change)
        let history = db.get_task_state_history(&task.id).unwrap();
        // Should have: pending (initial), in_progress (from claim) - but NOT another in_progress
        let in_progress_count = history.iter().filter(|e| e.status.as_deref() == Some("in_progress")).count();
        assert_eq!(
            in_progress_count, 1,
            "Should only have one in_progress entry, not duplicates"
        );
    }

    #[test]
    fn update_between_two_untimed_states() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Untimed to Untimed".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Claim and then fail the task to get to 'failed' state
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        let (failed_task, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("failed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(failed_task.status, "failed");
        assert!(failed_task.worker_id.is_none()); // Released on transition to terminal-ish state

        // Now transition from failed (untimed) to pending (untimed)
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("pending".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "pending");
        assert!(updated.worker_id.is_none()); // Still no owner
        assert!(updated.claimed_at.is_none());

        // Verify state history was recorded for all transitions
        let history = db.get_task_state_history(&task.id).unwrap();
        // Should have: pending (initial), in_progress (claim), failed, pending
        assert!(
            history.len() >= 4,
            "Expected at least 4 history entries, got {}",
            history.len()
        );

        let states: Vec<&str> = history.iter().map(|e| e.status.as_deref().unwrap_or("")).collect();
        assert!(
            states.contains(&"failed"),
            "History should contain 'failed'"
        );
        // Check we have pending twice (initial and after failed)
        let pending_count = states.iter().filter(|&&s| s == "pending").count();
        assert!(
            pending_count >= 2,
            "Should have at least 2 pending entries (initial + after failed)"
        );
    }

    #[test]
    fn claim_fails_if_blocked_by_single_task() {
        // BUG-002 regression: Claim should fail if task has unsatisfied blocking dependencies
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks: A blocks B
        let task_a = db
            .create_task(
                None,
                "Task A".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task_b = db
            .create_task(
                None,
                "Task B".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task_a.id, &task_b.id, "blocks", &deps_config)
            .unwrap();

        // Attempt to claim B (which is blocked by A)
        let result = db.update_task_unified(
            &task_b.id,
            &agent.id,
            None,
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(
            result.is_err(),
            "Claim should fail when task has unsatisfied dependencies"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsatisfied dependencies") && err_msg.contains(&task_a.id),
            "Error should mention unsatisfied dependencies and the blocking task ID. Got: {}",
            err_msg
        );
    }

    #[test]
    fn claim_fails_if_blocked_by_chain() {
        // BUG-002 regression: A blocks B, B blocks C - claim on C should fail
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create chain: A blocks B, B blocks C
        let task_a = db
            .create_task(
                None,
                "Task A".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task_b = db
            .create_task(
                None,
                "Task B".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task_c = db
            .create_task(
                None,
                "Task C".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task_a.id, &task_b.id, "blocks", &deps_config)
            .unwrap();
        db.add_dependency(&task_b.id, &task_c.id, "blocks", &deps_config)
            .unwrap();

        // Attempt to claim C (which is blocked by B which is blocked by A)
        let result = db.update_task_unified(
            &task_c.id,
            &agent.id,
            None,
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(
            result.is_err(),
            "Claim on C should fail when B is still in blocking state"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsatisfied dependencies") && err_msg.contains(&task_b.id),
            "Error should mention unsatisfied dependencies and the blocking task B ID. Got: {}",
            err_msg
        );
    }

    #[test]
    fn claim_succeeds_after_blocker_completes() {
        // BUG-002 regression: Claim should succeed once blocking task is completed
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks: A blocks B
        let task_a = db
            .create_task(
                None,
                "Task A".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task_b = db
            .create_task(
                None,
                "Task B".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task_a.id, &task_b.id, "blocks", &deps_config)
            .unwrap();

        // Complete task A
        db.claim_task(&task_a.id, &agent.id, &states_config)
            .unwrap();
        db.update_task_unified(
            &task_a.id,
            &agent.id,
            None,
            None,
            None,
            Some("completed".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        )
        .unwrap();

        // Now claim B should succeed
        let result = db.update_task_unified(
            &task_b.id,
            &agent.id,
            None,
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(
            result.is_ok(),
            "Claim should succeed after blocking task is completed"
        );
        let (task, _, _) = result.unwrap();
        assert_eq!(task.status, "in_progress");
        assert_eq!(task.worker_id.as_deref(), Some(agent.id.as_str()));
    }

    #[test]
    fn claim_with_force_bypasses_dependency_check() {
        // BUG-002 regression: force=true should bypass the dependency check
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks: A blocks B
        let task_a = db
            .create_task(
                None,
                "Task A".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task_b = db
            .create_task(
                None,
                "Task B".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task_a.id, &task_b.id, "blocks", &deps_config)
            .unwrap();

        // Claim B with force=true should succeed despite being blocked
        let result = db.update_task_unified(
            &task_b.id,
            &agent.id,
            None,
            None,
            None,
            Some("in_progress".to_string()),
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            true, // force=true
            &states_config,
            &deps_config,
            &auto_advance,
        );

        assert!(
            result.is_ok(),
            "Claim with force=true should succeed even when task has unsatisfied dependencies"
        );
        let (task, _, _) = result.unwrap();
        assert_eq!(task.status, "in_progress");
    }
}

mod dependency_tests {
    use super::*;

    #[test]
    fn add_dependency_creates_relationship() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Task 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        let blockers = db.get_blockers(&task2.id).unwrap();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0], task1.id);
    }

    #[test]
    fn add_dependency_fails_if_would_create_cycle() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Task 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap(); // task1 blocks task2

        let result = db.add_dependency(&task2.id, &task1.id, "blocks", &deps_config); // task2 blocks task1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn add_dependency_fails_for_longer_cycles() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Task 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task3 = db
            .create_task(
                None,
                "Task 3".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap(); // 1 -> 2
        db.add_dependency(&task2.id, &task3.id, "blocks", &deps_config)
            .unwrap(); // 2 -> 3

        let result = db.add_dependency(&task3.id, &task1.id, "blocks", &deps_config); // 3 -> 1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn remove_dependency_removes_relationship() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Task 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        db.remove_dependency(&task1.id, &task2.id, "blocks")
            .unwrap();

        let blockers = db.get_blockers(&task2.id).unwrap();
        assert!(blockers.is_empty());
    }

    #[test]
    fn get_ready_tasks_excludes_blocked_tasks() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Blocker".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        let ready = db
            .get_ready_tasks(None, &states_config, &deps_config, None, None)
            .unwrap();

        // task1 is ready, task2 is blocked
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, task1.id);
    }

    #[test]
    fn get_ready_tasks_includes_unblocked_after_completion() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task(
                None,
                "Blocker".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        // Complete blocker (need to transition through in_progress first)
        db.update_task(
            &task1.id,
            None,
            None,
            Some("in_progress".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.update_task(
            &task1.id,
            None,
            None,
            Some("completed".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let ready = db
            .get_ready_tasks(None, &states_config, &deps_config, None, None)
            .unwrap();

        // Now task2 is ready
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, task2.id);
    }
}

mod file_lock_tests {
    use super::*;

    #[test]
    fn lock_file_creates_lock() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        let warning = db
            .lock_file("src/main.rs".to_string(), &agent.id, None, None)
            .unwrap();

        assert!(warning.is_none());
        // Query by worker_id since get_file_locks requires at least one filter
        let locks = db.get_file_locks(None, Some(&agent.id), None).unwrap();
        assert_eq!(locks.len(), 1);
        assert!(locks.contains_key("src/main.rs"));
    }

    #[test]
    fn lock_file_returns_warning_if_locked_by_another() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent1.id, None, None)
            .unwrap();
        let warning = db
            .lock_file("src/main.rs".to_string(), &agent2.id, None, None)
            .unwrap();

        assert!(warning.is_some());
        assert_eq!(warning.unwrap(), agent1.id);
    }

    #[test]
    fn lock_file_updates_timestamp_if_same_agent() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent.id, None, None)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let warning = db
            .lock_file("src/main.rs".to_string(), &agent.id, None, None)
            .unwrap();

        assert!(warning.is_none()); // No warning for same agent
    }

    #[test]
    fn unlock_file_removes_lock() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent.id, None, None)
            .unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent.id, None).unwrap();

        assert!(unlocked);
        let locks = db.get_file_locks(None, None, None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn unlock_file_fails_for_wrong_agent() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent1.id, None, None)
            .unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent2.id, None).unwrap();

        assert!(!unlocked);
    }

    #[test]
    fn get_file_locks_filters_by_agent() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();
        db.lock_file("file1.rs".to_string(), &agent1.id, None, None)
            .unwrap();
        db.lock_file("file2.rs".to_string(), &agent2.id, None, None)
            .unwrap();

        let agent1_locks = db.get_file_locks(None, Some(&agent1.id), None).unwrap();

        assert_eq!(agent1_locks.len(), 1);
        assert!(agent1_locks.contains_key("file1.rs"));
    }

    #[test]
    fn release_worker_locks_removes_all_agent_locks() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();
        db.lock_file("file1.rs".to_string(), &agent.id, None, None)
            .unwrap();
        db.lock_file("file2.rs".to_string(), &agent.id, None, None)
            .unwrap();

        let released = db.release_worker_locks(&agent.id).unwrap();

        assert_eq!(released, 2);
        let locks = db.get_file_locks(None, None, None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn claim_updates_returns_immediately() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        let start = std::time::Instant::now();
        let updates = db.claim_updates(&agent.id).unwrap();
        let elapsed = start.elapsed();

        // Should return immediately (within 100ms)
        assert!(elapsed.as_millis() < 100);
        assert!(updates.new_claims.is_empty());
        assert!(updates.dropped_claims.is_empty());
    }

    #[test]
    fn claim_updates_returns_new_claims() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims a file
        db.lock_file("test.rs".to_string(), &agent1.id, None, None)
            .unwrap();

        let start = std::time::Instant::now();
        let updates = db.claim_updates(&agent2.id).unwrap();
        let elapsed = start.elapsed();

        // Should return immediately
        assert!(
            elapsed.as_millis() < 100,
            "Expected immediate return, but elapsed: {:?}",
            elapsed
        );
        assert_eq!(updates.new_claims.len(), 1);
        assert_eq!(updates.new_claims[0].file_path, "test.rs");
    }

    #[test]
    fn claim_updates_shows_release_for_claim_before_registration() {
        // When an agent registers after a claim, they should still see the release.
        // This allows agents to track when files become available, even for
        // claims that happened before they registered.
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims a file
        db.lock_file("edge.rs".to_string(), &agent1.id, None, None)
            .unwrap();

        // Agent2 registers AFTER the claim
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 releases the file
        db.unlock_file("edge.rs", &agent1.id, None).unwrap();

        // Agent2 polls - should see the release (file is now available)
        let updates = db.claim_updates(&agent2.id).unwrap();

        // Agent2 should NOT see the claim (it was before their registration)
        assert!(
            updates.new_claims.is_empty(),
            "Agent2 should not see the claim"
        );

        // Agent2 SHOULD see the release - the file became available
        assert_eq!(
            updates.dropped_claims.len(),
            1,
            "Agent2 should see release so they know file is available"
        );
        assert_eq!(updates.dropped_claims[0].file_path, "edge.rs");
    }

    #[test]
    fn claim_updates_includes_release_for_previously_polled_claim() {
        // Verify that after an agent polls and sees a claim, they DO see the release
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims a file
        db.lock_file("polled.rs".to_string(), &agent1.id, None, None)
            .unwrap();

        // Agent2 polls and sees the claim
        let updates1 = db.claim_updates(&agent2.id).unwrap();
        assert_eq!(updates1.new_claims.len(), 1);
        assert_eq!(updates1.new_claims[0].file_path, "polled.rs");

        // Agent1 releases the file
        db.unlock_file("polled.rs", &agent1.id, None).unwrap();

        // Agent2 polls again - should see the release because they polled and saw the claim
        let updates2 = db.claim_updates(&agent2.id).unwrap();
        assert!(updates2.new_claims.is_empty());
        assert_eq!(
            updates2.dropped_claims.len(),
            1,
            "Should see release for previously polled claim"
        );
        assert_eq!(updates2.dropped_claims[0].file_path, "polled.rs");
    }

    #[test]
    fn claim_updates_includes_release_when_claim_in_same_batch() {
        // When claim and release both happen before a poll, both should be visible
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims and releases a file before agent2 polls
        db.lock_file("batch.rs".to_string(), &agent1.id, None, None)
            .unwrap();
        db.unlock_file("batch.rs", &agent1.id, None).unwrap();

        // Agent2 polls - should see both claim and release in same batch
        let updates = db.claim_updates(&agent2.id).unwrap();

        assert_eq!(updates.new_claims.len(), 1, "Should see the claim");
        assert_eq!(updates.new_claims[0].file_path, "batch.rs");
        assert_eq!(
            updates.dropped_claims.len(),
            1,
            "Should see the release (claim in same batch)"
        );
        assert_eq!(updates.dropped_claims[0].file_path, "batch.rs");
    }

    #[test]
    fn claim_updates_new_agent_only_sees_future_events() {
        // New agents should only see events that happen after they register
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims and releases a file
        db.lock_file("old.rs".to_string(), &agent1.id, None, None)
            .unwrap();
        db.unlock_file("old.rs", &agent1.id, None).unwrap();

        // Agent2 registers AFTER the claim+release cycle
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 claims a new file AFTER agent2 registered
        db.lock_file("new.rs".to_string(), &agent1.id, None, None)
            .unwrap();

        // Agent2 polls - should only see new.rs
        let updates = db.claim_updates(&agent2.id).unwrap();

        assert_eq!(updates.new_claims.len(), 1, "Should only see new.rs");
        assert_eq!(updates.new_claims[0].file_path, "new.rs");
        assert!(
            updates.dropped_claims.is_empty(),
            "Should not see old.rs release"
        );
    }

    /// Regression test for BUG-003/BUG-004: unmark_file and mark_updates failed
    /// with "no such column: end_timestamp" because claim_sequence table was
    /// missing the end_timestamp column.
    #[test]
    fn regression_unmark_file_after_mark_file() {
        let db = setup_db();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Mark (lock) a file
        let lock_result = db.lock_file(
            "test.rs".to_string(),
            &agent.id,
            Some("testing".to_string()),
            None,
        );
        assert!(lock_result.is_ok(), "lock_file should succeed");

        // Unmark (unlock) the file - this was failing with end_timestamp error
        let unlock_result = db.unlock_file("test.rs", &agent.id, None);
        assert!(
            unlock_result.is_ok(),
            "unlock_file should succeed (was failing with end_timestamp column error)"
        );
        assert!(unlock_result.unwrap(), "unlock should return true");
    }

    /// Regression test for BUG-003/BUG-004: mark_updates failed with
    /// "no such column: end_timestamp" because claim_sequence table was
    /// missing the end_timestamp column.
    #[test]
    fn regression_mark_updates_after_mark_file() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 marks a file
        db.lock_file(
            "test.rs".to_string(),
            &agent1.id,
            Some("testing".to_string()),
            None,
        )
        .unwrap();

        // Agent2 polls for updates - this was failing with end_timestamp error
        let updates = db.claim_updates(&agent2.id);
        assert!(
            updates.is_ok(),
            "claim_updates should succeed (was failing with end_timestamp column error)"
        );

        let updates = updates.unwrap();
        assert_eq!(updates.new_claims.len(), 1, "Should see the new claim");
        assert_eq!(updates.new_claims[0].file_path, "test.rs");
    }

    /// Regression test: Verify end_timestamp is properly set when unlocking
    #[test]
    fn regression_end_timestamp_populated_on_unlock() {
        let db = setup_db();
        let agent1 = db.register_worker(None, vec![], false).unwrap();
        let agent2 = db.register_worker(None, vec![], false).unwrap();

        // Agent1 marks and unmarks a file
        db.lock_file("test.rs".to_string(), &agent1.id, None, None)
            .unwrap();
        db.unlock_file("test.rs", &agent1.id, None).unwrap();

        // Agent2 polls for updates
        let updates = db.claim_updates(&agent2.id).unwrap();

        // Both claim and release should be visible
        assert_eq!(updates.new_claims.len(), 1, "Should see the claim");
        assert_eq!(updates.dropped_claims.len(), 1, "Should see the release");

        // The claim event should have an end_timestamp populated
        let claim_event = &updates.new_claims[0];
        assert!(
            claim_event.end_timestamp.is_some(),
            "Claim event should have end_timestamp set after release"
        );
    }
}

mod tracking_tests {
    use super::*;

    #[test]
    fn set_thought_updates_current_thought() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Think".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        db.set_thought(&agent.id, Some("Thinking...".to_string()), None)
            .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(updated.current_thought, Some("Thinking...".to_string()));
    }

    #[test]
    fn log_time_accumulates_duration() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Time Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.log_time(&task.id, 1000).unwrap();
        db.log_time(&task.id, 2000).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(updated.time_actual_ms, Some(3000));
    }

    #[test]
    fn log_cost_accumulates_tokens_and_cost() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Cost Me".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // log_metrics(task_id, cost_usd, values)
        // values: [metric_0, metric_1, ...]
        db.log_metrics(
            &task.id,
            Some(0.001),
            &[100, 0, 50], // metric_0=100, metric_2=50
        )
        .unwrap();
        db.log_metrics(
            &task.id,
            Some(0.002),
            &[200, 0, 100], // metric_0=200, metric_2=100
        )
        .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(updated.metrics[0], 300); // metric_0 aggregated
        assert_eq!(updated.metrics[2], 150); // metric_2 aggregated
        assert!((updated.cost_usd - 0.003).abs() < 0.0001);
    }
}

mod stats_tests {
    use super::*;

    #[test]
    fn get_stats_returns_aggregate_statistics() {
        let db = setup_db();
        let states_config = default_states_config();
        db.create_task(
            None,
            "Task 1".to_string(),
            None,
            None, // phase
            None,
            Some(3),
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                Some(5),
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        // Transition through in_progress to completed
        db.update_task(
            &task2.id,
            None,
            None,
            Some("in_progress".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.update_task(
            &task2.id,
            None,
            None,
            Some("completed".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let stats = db.get_stats(None, None, &states_config).unwrap();

        assert_eq!(stats.total_tasks, 2);
        assert_eq!(*stats.tasks_by_status.get("pending").unwrap_or(&0), 1);
        assert_eq!(*stats.tasks_by_status.get("completed").unwrap_or(&0), 1);
        assert_eq!(stats.total_points, 8);
        assert_eq!(stats.completed_points, 5);
    }

    #[test]
    fn get_stats_filters_by_agent() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Agent Task".to_string(),
                None,
                None, // phase
                None,
                Some(3),
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        db.create_task(
            None,
            "Other Task".to_string(),
            None,
            None, // phase
            None,
            Some(5),
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let stats = db.get_stats(Some(&agent.id), None, &states_config).unwrap();

        assert_eq!(stats.total_tasks, 1);
        assert_eq!(stats.total_points, 3);
    }

    #[test]
    fn get_stats_filters_by_task_tree() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task(
                None,
                "Parent".to_string(),
                None,
                None, // phase
                None,
                Some(2),
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.create_task(
            None,
            "Child".to_string(),
            Some(parent.id.clone()),
            None, // phase
            None,
            Some(3),
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.create_task(
            None,
            "Other".to_string(),
            None,
            None, // phase
            None,
            Some(10),
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let stats = db
            .get_stats(None, Some(&parent.id), &states_config)
            .unwrap();

        assert_eq!(stats.total_tasks, 2); // parent + child
        assert_eq!(stats.total_points, 5); // 2 + 3
    }
}

mod state_transition_tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn create_task_records_initial_pending_state() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status.as_deref().unwrap(), "pending");
        assert!(history[0].end_timestamp.is_none()); // Still open
    }

    #[test]
    fn claim_task_records_in_progress_transition() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].status.as_deref().unwrap(), "pending");
        assert!(history[0].end_timestamp.is_some()); // Closed by claim
        assert_eq!(history[1].status.as_deref().unwrap(), "in_progress");
        assert!(history[1].worker_id.is_some());
    }

    #[test]
    fn complete_task_accumulates_time_from_in_progress() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(100));
        db.complete_task(&task.id, &agent.id, &states_config)
            .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.time_actual_ms.unwrap() >= 100);

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[2].status.as_deref().unwrap(), "completed");
    }

    #[test]
    fn multiple_claim_cycles_accumulate_time() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // First claim cycle
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(50));
        db.release_task_with_state(&task.id, &agent.id, "pending", &states_config)
            .unwrap();

        // Second claim cycle
        db.force_claim_task(&task.id, &agent.id, &states_config)
            .unwrap();
        sleep(Duration::from_millis(50));
        db.complete_task(&task.id, &agent.id, &states_config)
            .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        // Should have accumulated time from both in_progress periods
        assert!(updated.time_actual_ms.unwrap() >= 100);

        let history = db.get_task_state_history(&task.id).unwrap();
        // pending -> in_progress -> pending -> in_progress -> completed
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn release_to_non_working_state_accumulates_time() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(100));
        db.release_task_with_state(&task.id, &agent.id, "failed", &states_config)
            .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.time_actual_ms.unwrap() >= 100);
    }

    #[test]
    fn current_state_duration_returns_elapsed_time_for_working_state() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_worker(None, vec![], false).unwrap();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        // Before claim, should be None (pending is not a working state)
        let duration = db
            .get_current_state_duration(&task.id, &states_config)
            .unwrap();
        assert!(duration.is_none());

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(50));

        // After claim, should return elapsed time
        let duration = db
            .get_current_state_duration(&task.id, &states_config)
            .unwrap();
        assert!(duration.is_some());
        assert!(duration.unwrap() >= 50);
    }

    #[test]
    fn update_task_status_records_transition() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task(
                None,
                "Test".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.update_task(
            &task.id,
            None,
            None,
            Some("cancelled".to_string()),
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].status.as_deref().unwrap(), "pending");
        assert_eq!(history[1].status.as_deref().unwrap(), "cancelled");
    }

    #[test]
    fn reopen_completed_task_to_pending() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance();
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create and complete a task
        let task = db
            .create_task(
                None,
                "Test reopen".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        db.complete_task(&task.id, &agent.id, &states_config)
            .unwrap();

        // Verify it's completed
        let completed_task = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(completed_task.status, "completed");

        // Now reopen it to pending
        let (updated, _, _) = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("pending".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                Some("Task needs rework".to_string()), // reason
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(updated.status, "pending");
        assert!(updated.worker_id.is_none()); // Released when going to pending

        // Verify state history includes the reopen transition
        let history = db.get_task_state_history(&task.id).unwrap();
        let states: Vec<&str> = history.iter().map(|e| e.status.as_deref().unwrap_or("")).collect();
        assert!(
            states.contains(&"pending")
                && states.contains(&"in_progress")
                && states.contains(&"completed"),
            "Expected pending, in_progress, and completed in history, got {:?}",
            states
        );

        // The last transition should be back to pending
        let last_event = history.last().unwrap();
        assert_eq!(last_event.status.as_deref().unwrap(), "pending");
        assert_eq!(
            last_event.reason.as_deref(),
            Some("Task needs rework")
        );
    }
}

mod auto_advance_tests {
    use super::*;

    /// Helper to create an auto-advance config with a specific target state.
    fn auto_advance_enabled(target_state: &str) -> AutoAdvanceConfig {
        AutoAdvanceConfig {
            enabled: true,
            target_state: Some(target_state.to_string()),
        }
    }

    #[test]
    fn unblocked_reported_even_when_auto_advance_disabled() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = default_auto_advance(); // disabled by default
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks where task1 blocks task2
        let task1 = db
            .create_task(
                None,
                "Blocker".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        // Claim and complete task1
        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let (_, unblocked, auto_advanced) = db
            .update_task_unified(
                &task1.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        // unblocked should contain task2 (now ready to claim)
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0], task2.id);

        // auto_advanced should be empty because auto_advance is disabled
        assert!(auto_advanced.is_empty());

        // task2 should still be in pending state (not transitioned)
        let task2_updated = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_updated.status, "pending");
    }

    #[test]
    fn auto_advance_single_blocker_completes() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        // Note: We need a "ready" state that tasks can transition to.
        // By default, StatesConfig may not have "ready" as a valid state.
        // Let's use the initial state transitions. We'll need to check what's valid.
        // Actually, for this test, let's just use in_progress since it should be a valid transition.
        // But in_progress is timed, which would require claiming.
        // Let's check if pending can go to cancelled (non-timed) for testing.
        // Actually the plan suggests adding a "ready" state, but we should test with existing states.
        // We can test by checking if auto_advance returns the list even if the state is the same.
        // Let's create a custom config for this test.

        // For now, let's test that when enabled with no valid target state, nothing happens.
        // In a real scenario, you'd configure states to have a "ready" state.
        // Let's just verify the list is returned when a dependency is satisfied.

        // Test with completed as target - but that's terminal and not a valid transition from pending
        // Let's use in_progress as target - but that would claim the task
        // The issue is the default state machine doesn't have a non-timed intermediate state

        // For this test, let's just verify the auto_advanced list is populated
        // by using cancelled as target (since pending -> cancelled is valid)
        let auto_advance = auto_advance_enabled("cancelled");
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks where task1 blocks task2
        let task1 = db
            .create_task(
                None,
                "Blocker".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        // Claim and complete task1
        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let (_, unblocked, auto_advanced) = db
            .update_task_unified(
                &task1.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        // unblocked should contain task2
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0], task2.id);

        // auto_advanced should also contain task2 (it was transitioned)
        assert_eq!(auto_advanced.len(), 1);
        assert_eq!(auto_advanced[0], task2.id);

        let task2_updated = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_updated.status, "cancelled");
    }

    #[test]
    fn auto_advance_multiple_blockers_waits_for_all() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = auto_advance_enabled("cancelled");
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create three tasks: task1 and task3 both block task2
        let task1 = db
            .create_task(
                None,
                "Blocker 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task3 = db
            .create_task(
                None,
                "Blocker 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();
        db.add_dependency(&task3.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        // Complete task1 - task2 should NOT advance yet (task3 still blocking)
        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let (_, _, auto_advanced_1) = db
            .update_task_unified(
                &task1.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert!(auto_advanced_1.is_empty());
        let task2_status = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_status.status, "pending");

        // Complete task3 - NOW task2 should advance
        db.claim_task(&task3.id, &agent.id, &states_config).unwrap();
        let (_, _, auto_advanced_2) = db
            .update_task_unified(
                &task3.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        assert_eq!(auto_advanced_2.len(), 1);
        assert_eq!(auto_advanced_2[0], task2.id);
        let task2_updated = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_updated.status, "cancelled");
    }

    #[test]
    fn auto_advance_skips_non_initial_state_tasks() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = auto_advance_enabled("cancelled");
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create two tasks where task1 blocks task2
        let task1 = db
            .create_task(
                None,
                "Blocker".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Blocked".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();

        // Manually move task2 to in_progress (not initial state)
        db.claim_task(&task2.id, &agent.id, &states_config).unwrap();

        // Complete task1
        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let (_, _, auto_advanced) = db
            .update_task_unified(
                &task1.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        // task2 should NOT be auto-advanced because it's not in initial state
        assert!(auto_advanced.is_empty());
        let task2_updated = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_updated.status, "in_progress"); // Unchanged
    }

    #[test]
    fn auto_advance_cascading_chain() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let auto_advance = auto_advance_enabled("cancelled");
        let agent = db.register_worker(None, vec![], false).unwrap();

        // Create a chain: task1 -> task2 -> task3
        let task1 = db
            .create_task(
                None,
                "Task 1".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task2 = db
            .create_task(
                None,
                "Task 2".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        let task3 = db
            .create_task(
                None,
                "Task 3".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config)
            .unwrap();
        db.add_dependency(&task2.id, &task3.id, "blocks", &deps_config)
            .unwrap();

        // Complete task1 - task2 should auto-advance (but not task3 since task2 just changed)
        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let (_, _, auto_advanced) = db
            .update_task_unified(
                &task1.id,
                &agent.id,
                None, // assignee
                None,
                None,
                Some("completed".to_string()),
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                &states_config,
                &deps_config,
                &auto_advance,
            )
            .unwrap();

        // task2 should be auto-advanced to cancelled
        // task3 should NOT be auto-advanced because "cancelled" is not a blocking state
        // Wait, cancelled is not in blocking_states (which is [pending, in_progress] by default)
        // So task2 transitioning to cancelled should NOT trigger task3 to auto-advance
        // in the same transaction - it would need a separate update
        assert_eq!(auto_advanced.len(), 1);
        assert_eq!(auto_advanced[0], task2.id);

        // Verify states
        let task2_updated = db.get_task(&task2.id).unwrap().unwrap();
        assert_eq!(task2_updated.status, "cancelled");

        // task3 should still be pending since task2's transition to cancelled
        // doesn't count as "completing" in the cascade - it's in a separate scope
        // Actually task2 -> cancelled is a non-blocking state, so task3 might advance too
        // Let me check - "cancelled" is not in blocking_states, so task2 is no longer blocking task3
        // But the cascade only happens in the same update_task_unified call
        // So task3 should still be pending
        let task3_updated = db.get_task(&task3.id).unwrap().unwrap();
        assert_eq!(task3_updated.status, "pending"); // Still pending - cascade doesn't happen recursively
    }
}

mod attachment_tests {
    use super::*;

    /// Helper to create a task for attachment tests.
    fn create_test_task(db: &Database) -> task_graph_mcp::types::Task {
        db.create_task(
            None,
            "Attachment Test".to_string(),
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &default_states_config(),
        )
        .unwrap()
    }

    #[test]
    fn get_attachments_filtered_by_mime_prefix() {
        let db = setup_db();
        let task = create_test_task(&db);

        // Add attachments with different MIME types
        db.add_attachment(
            &task.id,
            "data.json".to_string(),
            r#"{"key": "value"}"#.to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "readme.txt".to_string(),
            "This is a text file".to_string(),
            Some("text/plain".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "notes.md".to_string(),
            "# Notes\nSome markdown".to_string(),
            Some("text/markdown".to_string()),
            None,
        )
        .unwrap();

        // Test filtering by exact MIME type prefix
        let json_attachments = db
            .get_attachments_filtered(&task.id, None, Some("application/json"))
            .unwrap();
        assert_eq!(json_attachments.len(), 1);
        assert_eq!(json_attachments[0].name, "data.json");

        // Test filtering by MIME type prefix (text/)
        let text_attachments = db
            .get_attachments_filtered(&task.id, None, Some("text/"))
            .unwrap();
        assert_eq!(text_attachments.len(), 2);
        let names: Vec<&str> = text_attachments.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"readme.txt"));
        assert!(names.contains(&"notes.md"));
    }

    #[test]
    fn get_attachments_filtered_by_name_pattern() {
        let db = setup_db();
        let task = create_test_task(&db);

        db.add_attachment(
            &task.id,
            "data.json".to_string(),
            r#"{"key": "value"}"#.to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "config.json".to_string(),
            r#"{"setting": true}"#.to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "readme.txt".to_string(),
            "Text content".to_string(),
            Some("text/plain".to_string()),
            None,
        )
        .unwrap();

        // Filter by glob pattern
        let json_files = db
            .get_attachments_filtered(&task.id, Some("*.json"), None)
            .unwrap();
        assert_eq!(json_files.len(), 2);

        // Filter by specific name
        let data_file = db
            .get_attachments_filtered(&task.id, Some("data.json"), None)
            .unwrap();
        assert_eq!(data_file.len(), 1);
        assert_eq!(data_file[0].name, "data.json");
    }

    #[test]
    fn get_attachments_filtered_by_both_name_and_mime() {
        let db = setup_db();
        let task = create_test_task(&db);

        db.add_attachment(
            &task.id,
            "data.json".to_string(),
            r#"{"key": "value"}"#.to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "schema.json".to_string(),
            r#"{"type": "object"}"#.to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "data.txt".to_string(),
            "Plain text".to_string(),
            Some("text/plain".to_string()),
            None,
        )
        .unwrap();

        // Filter by both name pattern and MIME type
        let result = db
            .get_attachments_filtered(&task.id, Some("data.*"), Some("application/json"))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "data.json");
    }

    #[test]
    fn get_attachments_no_filter_returns_all() {
        let db = setup_db();
        let task = create_test_task(&db);

        db.add_attachment(
            &task.id,
            "file1.txt".to_string(),
            "Content 1".to_string(),
            Some("text/plain".to_string()),
            None,
        )
        .unwrap();
        db.add_attachment(
            &task.id,
            "file2.json".to_string(),
            "{}".to_string(),
            Some("application/json".to_string()),
            None,
        )
        .unwrap();

        let all = db.get_attachments_filtered(&task.id, None, None).unwrap();
        assert_eq!(all.len(), 2);
    }
}
