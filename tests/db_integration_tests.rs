//! Integration tests for the database layer.
//!
//! These tests verify the core database operations using an in-memory SQLite database.
//! Tests are organized by module and functionality.

use task_graph_mcp::db::Database;
use task_graph_mcp::types::{EventType, Priority, TargetType, TaskStatus};
use uuid::Uuid;

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

mod agent_tests {
    use super::*;

    #[test]
    fn register_agent_creates_agent_with_defaults() {
        let db = setup_db();

        let agent = db
            .register_agent(None, None, vec![], None)
            .expect("Failed to register agent");

        assert!(agent.name.is_none());
        assert!(agent.tags.is_empty());
        assert_eq!(agent.max_claims, 5); // default
        assert!(agent.registered_at > 0);
        assert!(agent.last_heartbeat > 0);
    }

    #[test]
    fn register_agent_with_custom_values() {
        let db = setup_db();

        let agent = db
            .register_agent(
                None,
                Some("test-agent".to_string()),
                vec!["rust".to_string(), "backend".to_string()],
                Some(10),
            )
            .expect("Failed to register agent");

        assert_eq!(agent.name, Some("test-agent".to_string()));
        assert_eq!(agent.tags, vec!["rust", "backend"]);
        assert_eq!(agent.max_claims, 10);
    }

    #[test]
    fn register_agent_with_custom_id() {
        let db = setup_db();

        let agent = db
            .register_agent(
                Some("my-custom-agent".to_string()),
                Some("Custom Agent".to_string()),
                vec![],
                None,
            )
            .expect("Failed to register agent with custom ID");

        assert_eq!(agent.id, "my-custom-agent");
        assert_eq!(agent.name, Some("Custom Agent".to_string()));
    }

    #[test]
    fn register_agent_rejects_id_over_36_chars() {
        let db = setup_db();

        let result = db.register_agent(
            Some("this-id-is-way-too-long-and-should-be-rejected-by-the-system".to_string()),
            None,
            vec![],
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn register_agent_rejects_empty_id() {
        let db = setup_db();

        let result = db.register_agent(Some("".to_string()), None, vec![], None);

        assert!(result.is_err());
    }

    #[test]
    fn get_agent_returns_registered_agent() {
        let db = setup_db();
        let agent = db
            .register_agent(None, Some("finder".to_string()), vec![], None)
            .unwrap();

        let found = db.get_agent(&agent.id).unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().name, Some("finder".to_string()));
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
        let agent = db.register_agent(None, None, vec![], None).unwrap();

        let updated = db
            .update_agent(
                &agent.id,
                Some(Some("updated-name".to_string())),
                Some(vec!["new-tag".to_string()]),
                Some(3),
            )
            .unwrap();

        assert_eq!(updated.name, Some("updated-name".to_string()));
        assert_eq!(updated.tags, vec!["new-tag"]);
        assert_eq!(updated.max_claims, 3);
    }

    #[test]
    fn heartbeat_updates_last_heartbeat_and_returns_claim_count() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
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
        let agent = db.register_agent(None, None, vec![], None).unwrap();

        db.unregister_agent(&agent.id).unwrap();

        let found = db.get_agent(&agent.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn list_agents_returns_all_registered_agents() {
        let db = setup_db();
        db.register_agent(None, Some("agent1".to_string()), vec![], None)
            .unwrap();
        db.register_agent(None, Some("agent2".to_string()), vec![], None)
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
            )
            .unwrap();

        assert_eq!(task.title, "Test Task");
        assert!(task.description.is_none());
        assert!(task.parent_id.is_none());
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, Priority::Medium);
        assert!(task.owner_agent.is_none());
    }

    #[test]
    fn create_task_with_all_fields() {
        let db = setup_db();

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
                Some(serde_json::json!({"key": "value"})),
            )
            .unwrap();

        assert_eq!(task.title, "Full Task");
        assert_eq!(task.description, Some("Description".to_string()));
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.points, Some(5));
        assert_eq!(task.time_estimate_ms, Some(3600000));
        assert_eq!(task.needed_tags, vec!["rust"]);
        assert_eq!(task.wanted_tags, vec!["backend"]);
        assert!(task.metadata.is_some());
    }

    #[test]
    fn create_task_with_parent_assigns_correct_sibling_order() {
        let db = setup_db();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        let child1 = db
            .create_task(
                "Child 1".to_string(),
                None,
                Some(parent.id),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let child2 = db
            .create_task(
                "Child 2".to_string(),
                None,
                Some(parent.id),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(child1.sibling_order, 0);
        assert_eq!(child2.sibling_order, 1);
    }

    #[test]
    fn get_task_returns_existing_task() {
        let db = setup_db();
        let task = db
            .create_task("Find Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        let found = db.get_task(task.id).unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Find Me");
    }

    #[test]
    fn get_task_returns_none_for_unknown_id() {
        let db = setup_db();
        let unknown_id = Uuid::new_v4();

        let result = db.get_task(unknown_id).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn update_task_modifies_properties() {
        let db = setup_db();
        let task = db
            .create_task("Original".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        let updated = db
            .update_task(
                task.id,
                Some("Updated".to_string()),
                Some(Some("New Description".to_string())),
                Some(TaskStatus::InProgress),
                Some(Priority::High),
                None,
                None,
            )
            .unwrap();

        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.description, Some("New Description".to_string()));
        assert_eq!(updated.status, TaskStatus::InProgress);
        assert_eq!(updated.priority, Priority::High);
    }

    #[test]
    fn update_task_to_completed_sets_completed_at() {
        let db = setup_db();
        let task = db
            .create_task("Complete Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        assert!(task.completed_at.is_none());

        let updated = db
            .update_task(task.id, None, None, Some(TaskStatus::Completed), None, None, None)
            .unwrap();

        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn delete_task_removes_task() {
        let db = setup_db();
        let task = db
            .create_task("Delete Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.delete_task(task.id, false).unwrap();

        let found = db.get_task(task.id).unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn delete_task_without_cascade_fails_if_has_children() {
        let db = setup_db();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.create_task(
            "Child".to_string(),
            None,
            Some(parent.id),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result = db.delete_task(parent.id, false);

        assert!(result.is_err());
    }

    #[test]
    fn delete_task_with_cascade_removes_children() {
        let db = setup_db();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let child = db
            .create_task(
                "Child".to_string(),
                None,
                Some(parent.id),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        db.delete_task(parent.id, true).unwrap();

        assert!(db.get_task(parent.id).unwrap().is_none());
        assert!(db.get_task(child.id).unwrap().is_none());
    }

    #[test]
    fn get_children_returns_direct_children_in_order() {
        let db = setup_db();
        let parent = db
            .create_task("Parent".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.create_task(
            "Child 1".to_string(),
            None,
            Some(parent.id),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db.create_task(
            "Child 2".to_string(),
            None,
            Some(parent.id),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let children = db.get_children(parent.id).unwrap();

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].title, "Child 1");
        assert_eq!(children[1].title, "Child 2");
    }

    #[test]
    fn list_tasks_filters_by_status() {
        let db = setup_db();
        db.create_task("Pending".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Completed".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.update_task(task2.id, None, None, Some(TaskStatus::Completed), None, None, None)
            .unwrap();

        let pending = db
            .list_tasks(Some(TaskStatus::Pending), None, None, None)
            .unwrap();
        let completed = db
            .list_tasks(Some(TaskStatus::Completed), None, None, None)
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
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Claim Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        let claimed = db.claim_task(task.id, &agent.id).unwrap();

        assert_eq!(claimed.owner_agent, Some(agent.id.clone()));
        assert_eq!(claimed.status, TaskStatus::InProgress);
        assert!(claimed.claimed_at.is_some());
        assert!(claimed.started_at.is_some());
    }

    #[test]
    fn claim_task_fails_if_already_claimed() {
        let db = setup_db();
        let agent1 = db.register_agent(None, None, vec![], None).unwrap();
        let agent2 = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Claimed".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.claim_task(task.id, &agent1.id).unwrap();
        let result = db.claim_task(task.id, &agent2.id);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_fails_if_agent_at_claim_limit() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], Some(1)).unwrap(); // max 1 claim
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.claim_task(task1.id, &agent.id).unwrap();
        let result = db.claim_task(task2.id, &agent.id);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_fails_if_agent_missing_needed_tag() {
        let db = setup_db();
        let agent = db
            .register_agent(None, None, vec!["python".to_string()], None)
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
            )
            .unwrap();

        let result = db.claim_task(task.id, &agent.id);

        assert!(result.is_err());
    }

    #[test]
    fn claim_task_succeeds_if_agent_has_needed_tags() {
        let db = setup_db();
        let agent = db
            .register_agent(None, None, vec!["rust".to_string(), "backend".to_string()], None)
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
            )
            .unwrap();

        let result = db.claim_task(task.id, &agent.id);

        assert!(result.is_ok());
    }

    #[test]
    fn claim_task_fails_if_agent_has_none_of_wanted_tags() {
        let db = setup_db();
        let agent = db
            .register_agent(None, None, vec!["python".to_string()], None)
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
            )
            .unwrap();

        let result = db.claim_task(task.id, &agent.id);

        assert!(result.is_err());
    }

    #[test]
    fn release_task_clears_owner_and_resets_status() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Release Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.claim_task(task.id, &agent.id).unwrap();

        db.release_task(task.id, &agent.id).unwrap();

        let updated = db.get_task(task.id).unwrap().unwrap();
        assert!(updated.owner_agent.is_none());
        assert_eq!(updated.status, TaskStatus::Pending);
    }

    #[test]
    fn release_task_fails_if_not_owner() {
        let db = setup_db();
        let agent1 = db.register_agent(None, None, vec![], None).unwrap();
        let agent2 = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Owned".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.claim_task(task.id, &agent1.id).unwrap();

        let result = db.release_task(task.id, &agent2.id);

        assert!(result.is_err());
    }

    #[test]
    fn force_release_clears_owner_regardless() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Force".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.claim_task(task.id, &agent.id).unwrap();

        db.force_release(task.id).unwrap();

        let updated = db.get_task(task.id).unwrap().unwrap();
        assert!(updated.owner_agent.is_none());
    }
}

mod dependency_tests {
    use super::*;

    #[test]
    fn add_dependency_creates_relationship() {
        let db = setup_db();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.add_dependency(task1.id, task2.id).unwrap();

        let blockers = db.get_blockers(task2.id).unwrap();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0], task1.id);
    }

    #[test]
    fn add_dependency_fails_if_would_create_cycle() {
        let db = setup_db();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.add_dependency(task1.id, task2.id).unwrap(); // task1 blocks task2

        let result = db.add_dependency(task2.id, task1.id); // task2 blocks task1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn add_dependency_fails_for_longer_cycles() {
        let db = setup_db();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task3 = db
            .create_task("Task 3".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.add_dependency(task1.id, task2.id).unwrap(); // 1 -> 2
        db.add_dependency(task2.id, task3.id).unwrap(); // 2 -> 3

        let result = db.add_dependency(task3.id, task1.id); // 3 -> 1 - cycle!

        assert!(result.is_err());
    }

    #[test]
    fn remove_dependency_removes_relationship() {
        let db = setup_db();
        let task1 = db
            .create_task("Task 1".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Task 2".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.add_dependency(task1.id, task2.id).unwrap();

        db.remove_dependency(task1.id, task2.id).unwrap();

        let blockers = db.get_blockers(task2.id).unwrap();
        assert!(blockers.is_empty());
    }

    #[test]
    fn get_ready_tasks_excludes_blocked_tasks() {
        let db = setup_db();
        let task1 = db
            .create_task("Blocker".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Blocked".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.add_dependency(task1.id, task2.id).unwrap();

        let ready = db.get_ready_tasks(None).unwrap();

        // task1 is ready, task2 is blocked
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, task1.id);
    }

    #[test]
    fn get_ready_tasks_includes_unblocked_after_completion() {
        let db = setup_db();
        let task1 = db
            .create_task("Blocker".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        let task2 = db
            .create_task("Blocked".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.add_dependency(task1.id, task2.id).unwrap();

        // Complete blocker
        db.update_task(task1.id, None, None, Some(TaskStatus::Completed), None, None, None)
            .unwrap();

        let ready = db.get_ready_tasks(None).unwrap();

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
        let agent = db.register_agent(None, None, vec![], None).unwrap();

        let warning = db.lock_file("src/main.rs".to_string(), &agent.id).unwrap();

        assert!(warning.is_none());
        let locks = db.get_file_locks(None, None).unwrap();
        assert_eq!(locks.len(), 1);
        assert!(locks.contains_key("src/main.rs"));
    }

    #[test]
    fn lock_file_returns_warning_if_locked_by_another() {
        let db = setup_db();
        let agent1 = db.register_agent(None, None, vec![], None).unwrap();
        let agent2 = db.register_agent(None, None, vec![], None).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent1.id).unwrap();
        let warning = db.lock_file("src/main.rs".to_string(), &agent2.id).unwrap();

        assert!(warning.is_some());
        assert_eq!(warning.unwrap(), agent1.id);
    }

    #[test]
    fn lock_file_updates_timestamp_if_same_agent() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();

        db.lock_file("src/main.rs".to_string(), &agent.id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let warning = db.lock_file("src/main.rs".to_string(), &agent.id).unwrap();

        assert!(warning.is_none()); // No warning for same agent
    }

    #[test]
    fn unlock_file_removes_lock() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent.id).unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent.id).unwrap();

        assert!(unlocked);
        let locks = db.get_file_locks(None, None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn unlock_file_fails_for_wrong_agent() {
        let db = setup_db();
        let agent1 = db.register_agent(None, None, vec![], None).unwrap();
        let agent2 = db.register_agent(None, None, vec![], None).unwrap();
        db.lock_file("src/main.rs".to_string(), &agent1.id).unwrap();

        let unlocked = db.unlock_file("src/main.rs", &agent2.id).unwrap();

        assert!(!unlocked);
    }

    #[test]
    fn get_file_locks_filters_by_agent() {
        let db = setup_db();
        let agent1 = db.register_agent(None, None, vec![], None).unwrap();
        let agent2 = db.register_agent(None, None, vec![], None).unwrap();
        db.lock_file("file1.rs".to_string(), &agent1.id).unwrap();
        db.lock_file("file2.rs".to_string(), &agent2.id).unwrap();

        let agent1_locks = db.get_file_locks(None, Some(&agent1.id)).unwrap();

        assert_eq!(agent1_locks.len(), 1);
        assert!(agent1_locks.contains_key("file1.rs"));
    }

    #[test]
    fn release_agent_locks_removes_all_agent_locks() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.lock_file("file1.rs".to_string(), &agent.id).unwrap();
        db.lock_file("file2.rs".to_string(), &agent.id).unwrap();

        let released = db.release_agent_locks(&agent.id).unwrap();

        assert_eq!(released, 2);
        let locks = db.get_file_locks(None, None).unwrap();
        assert!(locks.is_empty());
    }
}

mod inbox_tests {
    use super::*;

    #[test]
    fn subscribe_creates_subscription() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Task".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        let sub_id = db
            .subscribe(&agent.id, TargetType::Task, task.id.to_string())
            .unwrap();

        assert!(!sub_id.is_nil());
    }

    #[test]
    fn unsubscribe_removes_subscription() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let sub_id = db
            .subscribe(&agent.id, TargetType::Task, "some-task".to_string())
            .unwrap();

        let removed = db.unsubscribe(sub_id).unwrap();

        assert!(removed);
    }

    #[test]
    fn publish_event_adds_messages_to_subscribers() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Task".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.subscribe(&agent.id, TargetType::Task, task.id.to_string())
            .unwrap();

        let count = db
            .publish_event(
                TargetType::Task,
                &task.id.to_string(),
                EventType::TaskUpdated,
                serde_json::json!({"status": "completed"}),
            )
            .unwrap();

        assert_eq!(count, 1);
    }

    #[test]
    fn poll_inbox_returns_unread_messages() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskCreated, serde_json::json!({}))
            .unwrap();

        let messages = db.poll_inbox(&agent.id, None, false).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].event_type, EventType::TaskCreated);
    }

    #[test]
    fn poll_inbox_marks_messages_as_read() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskCreated, serde_json::json!({}))
            .unwrap();

        db.poll_inbox(&agent.id, None, true).unwrap(); // mark_read = true
        let messages = db.poll_inbox(&agent.id, None, false).unwrap();

        assert!(messages.is_empty()); // Already read
    }

    #[test]
    fn poll_inbox_respects_limit() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskCreated, serde_json::json!({}))
            .unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskUpdated, serde_json::json!({}))
            .unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskDeleted, serde_json::json!({}))
            .unwrap();

        let messages = db.poll_inbox(&agent.id, Some(2), false).unwrap();

        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn clear_inbox_removes_all_messages() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskCreated, serde_json::json!({}))
            .unwrap();
        db.add_inbox_message(&agent.id, EventType::TaskUpdated, serde_json::json!({}))
            .unwrap();

        let cleared = db.clear_inbox(&agent.id).unwrap();

        assert_eq!(cleared, 2);
        let messages = db.poll_inbox(&agent.id, None, false).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn get_subscriptions_returns_agent_subscriptions() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        db.subscribe(&agent.id, TargetType::Task, "task-1".to_string())
            .unwrap();
        db.subscribe(&agent.id, TargetType::File, "file.rs".to_string())
            .unwrap();

        let subs = db.get_subscriptions(&agent.id).unwrap();

        assert_eq!(subs.len(), 2);
    }
}

mod tracking_tests {
    use super::*;

    #[test]
    fn set_thought_updates_current_thought() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
        let task = db
            .create_task("Think".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();
        db.claim_task(task.id, &agent.id).unwrap();

        db.set_thought(&agent.id, Some("Thinking...".to_string()), None)
            .unwrap();

        let updated = db.get_task(task.id).unwrap().unwrap();
        assert_eq!(updated.current_thought, Some("Thinking...".to_string()));
    }

    #[test]
    fn log_time_accumulates_duration() {
        let db = setup_db();
        let task = db
            .create_task("Time Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.log_time(task.id, 1000).unwrap();
        db.log_time(task.id, 2000).unwrap();

        let updated = db.get_task(task.id).unwrap().unwrap();
        assert_eq!(updated.time_actual_ms, Some(3000));
    }

    #[test]
    fn log_cost_accumulates_tokens_and_cost() {
        let db = setup_db();
        let task = db
            .create_task("Cost Me".to_string(), None, None, None, None, None, None, None, None)
            .unwrap();

        db.log_cost(
            task.id,
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
            task.id,
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

        let updated = db.get_task(task.id).unwrap().unwrap();
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
            )
            .unwrap();
        db.update_task(task2.id, None, None, Some(TaskStatus::Completed), None, None, None)
            .unwrap();

        let stats = db.get_stats(None, None).unwrap();

        assert_eq!(stats.total_tasks, 2);
        assert_eq!(stats.pending_tasks, 1);
        assert_eq!(stats.completed_tasks, 1);
        assert_eq!(stats.total_points, 8);
        assert_eq!(stats.completed_points, 5);
    }

    #[test]
    fn get_stats_filters_by_agent() {
        let db = setup_db();
        let agent = db.register_agent(None, None, vec![], None).unwrap();
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
            )
            .unwrap();
        db.claim_task(task.id, &agent.id).unwrap();
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
        )
        .unwrap();

        let stats = db.get_stats(Some(&agent.id), None).unwrap();

        assert_eq!(stats.total_tasks, 1);
        assert_eq!(stats.total_points, 3);
    }

    #[test]
    fn get_stats_filters_by_task_tree() {
        let db = setup_db();
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
            )
            .unwrap();
        db.create_task(
            "Child".to_string(),
            None,
            Some(parent.id),
            None,
            Some(3),
            None,
            None,
            None,
            None,
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
        )
        .unwrap();

        let stats = db.get_stats(None, Some(parent.id)).unwrap();

        assert_eq!(stats.total_tasks, 2); // parent + child
        assert_eq!(stats.total_points, 5); // 2 + 3
    }
}
