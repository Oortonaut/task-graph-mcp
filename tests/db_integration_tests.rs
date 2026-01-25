//! Integration tests for the database layer.
//!
//! These tests verify the core database operations using an in-memory SQLite database.
//! Tests are organized by module and functionality.

use task_graph_mcp::config::{DependenciesConfig, StatesConfig};
use task_graph_mcp::db::Database;
use task_graph_mcp::types::Priority;

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

mod agent_tests {
    use super::*;

    #[test]
    fn register_agent_creates_agent_with_defaults() {
        let db = setup_db();

        let agent = db
            .register_agent(None, vec![], false)
            .expect("Failed to register agent");

        assert!(agent.tags.is_empty());
        assert_eq!(agent.max_claims, 5); // default
        assert!(agent.registered_at > 0);
        assert!(agent.last_heartbeat > 0);
    }

    #[test]
    fn register_agent_with_custom_tags() {
        let db = setup_db();

        let agent = db
            .register_agent(
                None,
                vec!["rust".to_string(), "backend".to_string()],
                false,
            )
            .expect("Failed to register agent");

        assert_eq!(agent.tags, vec!["rust", "backend"]);
        assert_eq!(agent.max_claims, 5); // default
    }

    #[test]
    fn register_agent_with_custom_id() {
        let db = setup_db();

        let agent = db
            .register_agent(
                Some("my-custom-agent".to_string()),
                vec![],
                false,
            )
            .expect("Failed to register agent with custom ID");

        assert_eq!(agent.id, "my-custom-agent");
    }

    #[test]
    fn register_agent_rejects_id_over_36_chars() {
        let db = setup_db();

        let result = db.register_agent(
            Some("this-id-is-way-too-long-and-should-be-rejected-by-the-system".to_string()),
            vec![],
            false,
        );

        assert!(result.is_err());
    }

    #[test]
    fn register_agent_rejects_empty_id() {
        let db = setup_db();

        let result = db.register_agent(Some("".to_string()), vec![], false);

        assert!(result.is_err());
    }

    #[test]
    fn register_agent_rejects_duplicate_id() {
        let db = setup_db();

        // First registration should succeed
        let result = db.register_agent(
            Some("duplicate-agent".to_string()),
            vec![],
            false,
        );
        assert!(result.is_ok());

        // Second registration with same ID should fail
        let result = db.register_agent(
            Some("duplicate-agent".to_string()),
            vec![],
            false,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already registered"));
    }

    #[test]
    fn register_agent_with_force_allows_reconnection() {
        let db = setup_db();

        // First registration
        let agent1 = db.register_agent(
            Some("force-agent".to_string()),
            vec!["old-tag".to_string()],
            false,
        ).unwrap();
        assert_eq!(agent1.tags, vec!["old-tag"]);

        // Second registration without force should fail
        let result = db.register_agent(
            Some("force-agent".to_string()),
            vec!["new-tag".to_string()],
            false,
        );
        assert!(result.is_err());

        // Third registration with force=true should succeed and update tags
        let agent2 = db.register_agent(
            Some("force-agent".to_string()),
            vec!["new-tag".to_string()],
            true,
        ).unwrap();
        assert_eq!(agent2.tags, vec!["new-tag"]);
    }

    #[test]
    fn get_agent_returns_registered_agent() {
        let db = setup_db();
        let agent = db
            .register_agent(None, vec!["finder".to_string()], false)
            .unwrap();

        let found = db.get_agent(&agent.id).unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().tags, vec!["finder"]);
    }

    #[test]
    fn get_agent_returns_none_for_unknown_id() {
        let db = setup_db();

        let result = db.get_agent("unknown-agent-id").unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn update_agent_modifies_properties() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();

        let updated = db
            .update_agent(
                &agent.id,
                Some(vec!["new-tag".to_string()]),
                Some(3),
            )
            .unwrap();

        assert_eq!(updated.tags, vec!["new-tag"]);
        assert_eq!(updated.max_claims, 3);
    }

    #[test]
    fn heartbeat_updates_last_heartbeat_and_returns_claim_count() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let original_heartbeat = agent.last_heartbeat;

        // Small delay to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        let claim_count = db.heartbeat(&agent.id).unwrap();

        assert_eq!(claim_count, 0);
        let updated = db.get_agent(&agent.id).unwrap().unwrap();
        assert!(updated.last_heartbeat >= original_heartbeat);
    }

    #[test]
    fn heartbeat_fails_for_unknown_agent() {
        let db = setup_db();

        let result = db.heartbeat("unknown-agent");

        assert!(result.is_err());
    }

    #[test]
    fn unregister_agent_removes_agent() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();

        db.unregister_agent(&agent.id).unwrap();

        let found = db.get_agent(&agent.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn list_agents_returns_all_registered_agents() {
        let db = setup_db();
        db.register_agent(None, vec!["agent1".to_string()], false)
            .unwrap();
        db.register_agent(None, vec!["agent2".to_string()], false)
            .unwrap();

        let agents = db.list_agents().unwrap();

        assert_eq!(agents.len(), 2);
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
                "Test Task".to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        assert_eq!(task.title, "Test Task");
        assert!(task.description.is_none());
        assert_eq!(task.status, "pending");
        assert_eq!(task.priority, Priority::Medium);
        assert!(task.owner_agent.is_none());
    }

    #[test]
    fn create_task_with_all_fields() {
        let db = setup_db();
        let states_config = default_states_config();

        let task = db
            .create_task(
                "Full Task".to_string(),
                Some("Description".to_string()),
                None,
                Some(Priority::High),
                Some(5),
                Some(3600000),
                Some(vec!["rust".to_string()]),
                Some(vec!["backend".to_string()]),
                None, // tags
                None, // blocked_by
                &states_config,
            )
            .unwrap();

        assert_eq!(task.title, "Full Task");
        assert_eq!(task.description, Some("Description".to_string()));
        assert_eq!(task.priority, Priority::High);
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
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        let child1 = db
            .create_task(
                "Child 1".to_string(),
                None,
                Some(parent.id.clone()),
                None,
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
                "Child 2".to_string(),
                None,
                Some(parent.id.clone()),
                None,
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
            .create_task("Find Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
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
            .create_task("Original".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        let updated = db
            .update_task(
                &task.id,
                Some("Updated".to_string()),
                Some(Some("New Description".to_string())),
                Some("in_progress".to_string()),
                Some(Priority::High),
                None,
                None,
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.description, Some("New Description".to_string()));
        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.priority, Priority::High);
    }

    #[test]
    fn update_task_to_completed_sets_completed_at() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task("Complete Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        assert!(task.completed_at.is_none());

        // Need to transition through in_progress first (pending -> in_progress -> completed)
        db.update_task(&task.id, None, None, Some("in_progress".to_string()), None, None, None, &states_config)
            .unwrap();
        let updated = db
            .update_task(&task.id, None, None, Some("completed".to_string()), None, None, None, &states_config)
            .unwrap();

        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn delete_task_removes_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task("Delete Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.delete_task(&task.id, false).unwrap();

        let found = db.get_task(&task.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn delete_task_without_cascade_fails_if_has_children() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.create_task(
            "Child".to_string(),
            None,
            Some(parent.id.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let result = db.delete_task(&parent.id, false);

        assert!(result.is_err());
    }

    #[test]
    fn delete_task_with_cascade_removes_children() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let child = db
            .create_task(
                "Child".to_string(),
                None,
                Some(parent.id.clone()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();

        db.delete_task(&parent.id, true).unwrap();

        assert!(db.get_task(&parent.id).unwrap().is_none());
        assert!(db.get_task(&child.id).unwrap().is_none());
    }

    #[test]
    fn get_children_returns_direct_children_in_order() {
        let db = setup_db();
        let states_config = default_states_config();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.create_task(
            "Child 1".to_string(),
            None,
            Some(parent.id.clone()),
            None,
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
            "Child 2".to_string(),
            None,
            Some(parent.id.clone()),
            None,
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
        db.create_task("Pending".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Completed".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        // Transition through in_progress to completed
        db.update_task(&task2.id, None, None, Some("in_progress".to_string()), None, None, None, &states_config)
            .unwrap();
        db.update_task(&task2.id, None, None, Some("completed".to_string()), None, None, None, &states_config)
            .unwrap();

        let pending = db
            .list_tasks(Some("pending"), None, None, None)
            .unwrap();
        let completed = db
            .list_tasks(Some("completed"), None, None, None)
            .unwrap();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title, "Pending");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].title, "Completed");
    }
}

mod task_claiming_tests {
    use super::*;

    #[test]
    fn claim_task_assigns_owner_and_updates_status() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Claim Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        let claimed = db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        assert_eq!(claimed.owner_agent, Some(agent.id.clone()));
        assert_eq!(claimed.status, "in_progress");
        assert!(claimed.claimed_at.is_some());
        assert!(claimed.started_at.is_some());
    }

    #[test]
    fn claim_task_fails_if_already_claimed() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Claimed".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();
        let result = db.claim_task(&task.id, &agent2.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_fails_if_agent_at_claim_limit() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        // Set max_claims to 1 via update_agent
        let agent = db.update_agent(&agent.id, None, Some(1)).unwrap();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.claim_task(&task1.id, &agent.id, &states_config).unwrap();
        let result = db.claim_task(&task2.id, &agent.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_fails_if_agent_missing_needed_tag() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db
            .register_agent(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                "Rust Task".to_string(),
                None,
                None,
                None,
                None,
                None,
                Some(vec!["rust".to_string()]), // needs rust tag
                None,
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
            .register_agent(None, vec!["rust".to_string(), "backend".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                "Rust Task".to_string(),
                None,
                None,
                None,
                None,
                None,
                Some(vec!["rust".to_string()]),
                None,
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
            .register_agent(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                "Flexible Task".to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(vec!["rust".to_string(), "go".to_string()]), // wants rust OR go
                None,
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
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Release Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        db.release_task(&task.id, &agent.id, &states_config).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.owner_agent.is_none());
        assert_eq!(updated.status, "pending");
    }

    #[test]
    fn release_task_fails_if_not_owner() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Owned".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        let result = db.release_task(&task.id, &agent2.id, &states_config);

        assert!(result.is_err());
    }

    #[test]
    fn force_release_clears_owner_regardless() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Force".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        db.force_release(&task.id, &states_config).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.owner_agent.is_none());
    }

    // Tests for unified update with claim/release behavior
    #[test]
    fn update_to_timed_state_claims_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Update Claim".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // Update to in_progress (timed state) should claim the task
        let updated = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, None,
                Some("in_progress".to_string()),
                None, None, None,
                false,
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.owner_agent, Some(agent.id.clone()));
        assert!(updated.claimed_at.is_some());
    }

    #[test]
    fn update_from_timed_to_non_timed_releases_task() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Update Release".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // First claim via update
        db.update_task_unified(
            &task.id,
            &agent.id,
            None, None,
            Some("in_progress".to_string()),
            None, None, None,
            false,
            &states_config,
        )
        .unwrap();

        // Update back to pending (non-timed) should release
        let updated = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, None,
                Some("pending".to_string()),
                None, None, None,
                false,
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.status, "pending");
        assert!(updated.owner_agent.is_none());
        assert!(updated.claimed_at.is_none());
    }

    #[test]
    fn update_with_force_claims_from_another_agent() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Force Update".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // Agent1 claims the task
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        // Agent2 force claims via update
        let updated = db
            .update_task_unified(
                &task.id,
                &agent2.id,
                None, None,
                Some("in_progress".to_string()),
                None, None, None,
                true, // force
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.owner_agent, Some(agent2.id.clone()));
    }

    #[test]
    fn update_without_force_fails_if_claimed_by_another() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("No Force".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // Agent1 claims the task
        db.claim_task(&task.id, &agent1.id, &states_config).unwrap();

        // Agent2 tries to claim without force - should fail
        let result = db.update_task_unified(
            &task.id,
            &agent2.id,
            None, None,
            Some("in_progress".to_string()),
            None, None, None,
            false, // no force
            &states_config,
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_validates_tag_affinity_on_claim() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db
            .register_agent(None, vec!["python".to_string()], false)
            .unwrap();
        let task = db
            .create_task(
                "Needs Rust".to_string(),
                None, None, None, None, None,
                Some(vec!["rust".to_string()]), // needed_tags
                None, None, None,
                &states_config,
            )
            .unwrap();

        // Update to claim should fail due to missing tag
        let result = db.update_task_unified(
            &task.id,
            &agent.id,
            None, None,
            Some("in_progress".to_string()),
            None, None, None,
            false,
            &states_config,
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_to_completed_clears_ownership() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Complete Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // Claim the task
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        // Complete via update
        let updated = db
            .update_task_unified(
                &task.id,
                &agent.id,
                None, None,
                Some("completed".to_string()),
                None, None, None,
                false,
                &states_config,
            )
            .unwrap();

        assert_eq!(updated.status, "completed");
        assert!(updated.owner_agent.is_none());
        assert!(updated.completed_at.is_some());
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
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap();

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
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap(); // task1 blocks task2

        let result = db.add_dependency(&task2.id, &task1.id, "blocks", &deps_config); // task2 blocks task1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn add_dependency_fails_for_longer_cycles() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task3 = db
            .create_task("Task 3".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap(); // 1 -> 2
        db.add_dependency(&task2.id, &task3.id, "blocks", &deps_config).unwrap(); // 2 -> 3

        let result = db.add_dependency(&task3.id, &task1.id, "blocks", &deps_config); // 3 -> 1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn remove_dependency_removes_relationship() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap();

        db.remove_dependency(&task1.id, &task2.id, "blocks").unwrap();

        let blockers = db.get_blockers(&task2.id).unwrap();
        assert!(blockers.is_empty());
    }

    #[test]
    fn get_ready_tasks_excludes_blocked_tasks() {
        let db = setup_db();
        let states_config = default_states_config();
        let deps_config = default_deps_config();
        let task1 = db
            .create_task("Blocker".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Blocked".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap();

        let ready = db.get_ready_tasks(None, &states_config, &deps_config).unwrap();

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
            .create_task("Blocker".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        let task2 = db
            .create_task("Blocked".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();
        db.add_dependency(&task1.id, &task2.id, "blocks", &deps_config).unwrap();

        // Complete blocker (need to transition through in_progress first)
        db.update_task(&task1.id, None, None, Some("in_progress".to_string()), None, None, None, &states_config)
            .unwrap();
        db.update_task(&task1.id, None, None, Some("completed".to_string()), None, None, None, &states_config)
            .unwrap();

        let ready = db.get_ready_tasks(None, &states_config, &deps_config).unwrap();

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
        let agent = db.register_agent(None, vec![], false).unwrap();

        let warning = db.lock_file("src/main.rs".to_string(), &agent.id, None).unwrap();

        assert!(warning.is_none());
        let locks = db.get_file_locks(None, None).unwrap();
        assert_eq!(locks.len(), 1);
        assert!(locks.contains_key("src/main.rs"));
    }

    #[test]
    fn lock_file_returns_warning_if_locked_by_another() {
        let db = setup_db();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent1.id, None).unwrap();
        let warning = db.lock_file("src/main.rs".to_string(), &agent2.id, None).unwrap();

        assert!(warning.is_some());
        assert_eq!(warning.unwrap(), agent1.id);
    }

    #[test]
    fn lock_file_updates_timestamp_if_same_agent() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent.id, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let warning = db.lock_file("src/main.rs".to_string(), &agent.id, None).unwrap();

        assert!(warning.is_none()); // No warning for same agent
    }

    #[test]
    fn unlock_file_removes_lock() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent.id, None).unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent.id, None).unwrap();

        assert!(unlocked);
        let locks = db.get_file_locks(None, None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn unlock_file_fails_for_wrong_agent() {
        let db = setup_db();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent1.id, None).unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent2.id, None).unwrap();

        assert!(!unlocked);
    }

    #[test]
    fn get_file_locks_filters_by_agent() {
        let db = setup_db();
        let agent1 = db.register_agent(None, vec![], false).unwrap();
        let agent2 = db.register_agent(None, vec![], false).unwrap();
        db.lock_file("file1.rs".to_string(), &agent1.id, None).unwrap();
        db.lock_file("file2.rs".to_string(), &agent2.id, None).unwrap();

        let agent1_locks = db.get_file_locks(None, Some(&agent1.id)).unwrap();

        assert_eq!(agent1_locks.len(), 1);
        assert!(agent1_locks.contains_key("file1.rs"));
    }

    #[test]
    fn release_agent_locks_removes_all_agent_locks() {
        let db = setup_db();
        let agent = db.register_agent(None, vec![], false).unwrap();
        db.lock_file("file1.rs".to_string(), &agent.id, None).unwrap();
        db.lock_file("file2.rs".to_string(), &agent.id, None).unwrap();

        let released = db.release_agent_locks(&agent.id).unwrap();

        assert_eq!(released, 2);
        let locks = db.get_file_locks(None, None).unwrap();
        assert!(locks.is_empty());
    }
}

mod tracking_tests {
    use super::*;

    #[test]
    fn set_thought_updates_current_thought() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Think".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
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
            .create_task("Time Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
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
            .create_task("Cost Me".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.log_cost(
            &task.id,
            Some(100),
            None,
            Some(50),
            None,
            None,
            None,
            Some(0.001),
            None,
        )
        .unwrap();
        db.log_cost(
            &task.id,
            Some(200),
            None,
            Some(100),
            None,
            None,
            None,
            Some(0.002),
            None,
        )
        .unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert_eq!(updated.tokens_in, 300);
        assert_eq!(updated.tokens_out, 150);
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
            "Task 1".to_string(),
            None,
            None,
            None,
            Some(3),
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
                "Task 2".to_string(),
                None,
                None,
                None,
                Some(5),
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        // Transition through in_progress to completed
        db.update_task(&task2.id, None, None, Some("in_progress".to_string()), None, None, None, &states_config)
            .unwrap();
        db.update_task(&task2.id, None, None, Some("completed".to_string()), None, None, None, &states_config)
            .unwrap();

        let stats = db.get_stats(None, None, &states_config).unwrap();

        assert_eq!(stats.total_tasks, 2);
        assert_eq!(*stats.tasks_by_state.get("pending").unwrap_or(&0), 1);
        assert_eq!(*stats.tasks_by_state.get("completed").unwrap_or(&0), 1);
        assert_eq!(stats.total_points, 8);
        assert_eq!(stats.completed_points, 5);
    }

    #[test]
    fn get_stats_filters_by_agent() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task(
                "Agent Task".to_string(),
                None,
                None,
                None,
                Some(3),
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        db.create_task(
            "Other Task".to_string(),
            None,
            None,
            None,
            Some(5),
            None,
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
                "Parent".to_string(),
                None,
                None,
                None,
                Some(2),
                None,
                None,
                None,
                None,
                None,
                &states_config,
            )
            .unwrap();
        db.create_task(
            "Child".to_string(),
            None,
            Some(parent.id.clone()),
            None,
            Some(3),
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();
        db.create_task(
            "Other".to_string(),
            None,
            None,
            None,
            Some(10),
            None,
            None,
            None,
            None,
            None,
            &states_config,
        )
        .unwrap();

        let stats = db.get_stats(None, Some(&parent.id), &states_config).unwrap();

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
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].event, "pending");
        assert!(history[0].end_timestamp.is_none()); // Still open
    }

    #[test]
    fn claim_task_records_in_progress_transition() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].event, "pending");
        assert!(history[0].end_timestamp.is_some()); // Closed by claim
        assert_eq!(history[1].event, "in_progress");
        assert!(history[1].agent_id.is_some());
    }

    #[test]
    fn complete_task_accumulates_time_from_in_progress() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(100));
        db.complete_task(&task.id, &agent.id, &states_config).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.time_actual_ms.unwrap() >= 100);

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[2].event, "completed");
    }

    #[test]
    fn multiple_claim_cycles_accumulate_time() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // First claim cycle
        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(50));
        db.release_task_with_state(&task.id, &agent.id, "pending", &states_config).unwrap();

        // Second claim cycle
        db.force_claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(50));
        db.complete_task(&task.id, &agent.id, &states_config).unwrap();

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
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(100));
        db.release_task_with_state(&task.id, &agent.id, "failed", &states_config).unwrap();

        let updated = db.get_task(&task.id).unwrap().unwrap();
        assert!(updated.time_actual_ms.unwrap() >= 100);
    }

    #[test]
    fn current_state_duration_returns_elapsed_time_for_working_state() {
        let db = setup_db();
        let states_config = default_states_config();
        let agent = db.register_agent(None, vec![], false).unwrap();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        // Before claim, should be None (pending is not a working state)
        let duration = db.get_current_state_duration(&task.id, &states_config).unwrap();
        assert!(duration.is_none());

        db.claim_task(&task.id, &agent.id, &states_config).unwrap();
        sleep(Duration::from_millis(50));

        // After claim, should return elapsed time
        let duration = db.get_current_state_duration(&task.id, &states_config).unwrap();
        assert!(duration.is_some());
        assert!(duration.unwrap() >= 50);
    }

    #[test]
    fn update_task_status_records_transition() {
        let db = setup_db();
        let states_config = default_states_config();
        let task = db
            .create_task("Test".to_string(), None, None, None, None, None, None, None, None, None, &states_config)
            .unwrap();

        db.update_task(&task.id, None, None, Some("cancelled".to_string()), None, None, None, &states_config).unwrap();

        let history = db.get_task_state_history(&task.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].event, "pending");
        assert_eq!(history[1].event, "cancelled");
    }
}
