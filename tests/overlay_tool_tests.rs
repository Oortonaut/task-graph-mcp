//! Integration tests for the add_overlay and remove_overlay tools.
//!
//! These tests verify that the overlay management tools properly validate inputs,
//! persist overlay changes to the database, and return correct responses including
//! overlay diffs and active overlay lists.

use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use task_graph_mcp::config::workflows::{StateWorkflow, TransitionPrompts, WorkflowsConfig};
use task_graph_mcp::config::{
    AppConfig, AttachmentsConfig, AutoAdvanceConfig, DependenciesConfig, FeedbackConfig, IdsConfig,
    PhasesConfig, StatesConfig, TagsConfig,
};
use task_graph_mcp::db::Database;
use task_graph_mcp::tools::agents;

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

/// Helper to create a default IdsConfig for testing.
fn default_ids_config() -> IdsConfig {
    IdsConfig::default()
}

/// Build an AppConfig whose workflows have named_overlays populated.
/// This simulates overlay-git.yaml and overlay-troubleshooting.yaml being loaded.
fn app_config_with_overlays() -> AppConfig {
    let mut workflows = WorkflowsConfig::default();

    // Create a "git" overlay that adds a "reviewing" state
    let mut git_overlay = WorkflowsConfig {
        states: HashMap::new(),
        phases: HashMap::new(),
        combos: HashMap::new(),
        gates: HashMap::new(),
        roles: HashMap::new(),
        role_prompts: HashMap::new(),
        ..Default::default()
    };
    git_overlay.states.insert(
        "reviewing".to_string(),
        StateWorkflow {
            exits: vec!["completed".to_string()],
            timed: true,
            prompts: TransitionPrompts {
                enter: Some("Review changes before merging.".to_string()),
                exit: None,
            },
        },
    );

    // Create a "troubleshooting" overlay that modifies the working state prompt
    let mut troubleshoot_overlay = WorkflowsConfig {
        states: HashMap::new(),
        phases: HashMap::new(),
        combos: HashMap::new(),
        gates: HashMap::new(),
        roles: HashMap::new(),
        role_prompts: HashMap::new(),
        ..Default::default()
    };
    troubleshoot_overlay.states.insert(
        "working".to_string(),
        StateWorkflow {
            exits: vec![],
            timed: false,
            prompts: TransitionPrompts {
                enter: Some("Focus on diagnosing the root cause.".to_string()),
                exit: None,
            },
        },
    );

    workflows
        .named_overlays
        .insert("git".to_string(), Arc::new(git_overlay));
    workflows.named_overlays.insert(
        "troubleshooting".to_string(),
        Arc::new(troubleshoot_overlay),
    );

    AppConfig::new(
        Arc::new(StatesConfig::default()),
        Arc::new(PhasesConfig::default()),
        Arc::new(DependenciesConfig::default()),
        Arc::new(AutoAdvanceConfig::default()),
        Arc::new(AttachmentsConfig::default()),
        Arc::new(TagsConfig::default()),
        Arc::new(IdsConfig::default()),
        Arc::new(workflows),
        Arc::new(FeedbackConfig::default()),
    )
}

/// Helper: register a worker by ID directly via the DB, returning the worker_id.
fn register_worker(db: &Database, worker_id: &str) {
    db.register_worker(
        Some(worker_id.to_string()),
        vec![],
        false,
        &default_ids_config(),
        None,
        vec![],
    )
    .expect("Failed to register worker");
}

/// Helper: register a worker with initial overlays.
fn register_worker_with_overlays(db: &Database, worker_id: &str, overlays: Vec<String>) {
    db.register_worker(
        Some(worker_id.to_string()),
        vec![],
        false,
        &default_ids_config(),
        None,
        overlays,
    )
    .expect("Failed to register worker with overlays");
}

// =============================================================================
// add_overlay tool tests
// =============================================================================

#[test]
fn add_overlay_succeeds_and_persists() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "add-overlay-worker");

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "add-overlay-worker",
            "overlay": "git"
        }),
    )
    .expect("add_overlay should succeed");

    // Check response fields
    assert_eq!(result["success"], true);
    assert_eq!(result["worker_id"], "add-overlay-worker");
    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be array");
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0], "git");

    // Verify persistence in DB
    let worker = db
        .get_worker("add-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert_eq!(worker.overlays, vec!["git".to_string()]);
}

#[test]
fn add_overlay_appends_to_existing_overlays() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(&db, "append-overlay-worker", vec!["git".to_string()]);

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "append-overlay-worker",
            "overlay": "troubleshooting"
        }),
    )
    .expect("add_overlay should succeed");

    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be array");
    assert_eq!(overlays.len(), 2);
    assert_eq!(overlays[0], "git");
    assert_eq!(overlays[1], "troubleshooting");

    // Verify DB state
    let worker = db
        .get_worker("append-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert_eq!(
        worker.overlays,
        vec!["git".to_string(), "troubleshooting".to_string()]
    );
}

#[test]
fn add_overlay_returns_overlay_diff() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "diff-overlay-worker");

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "diff-overlay-worker",
            "overlay": "git"
        }),
    )
    .expect("add_overlay should succeed");

    // The git overlay adds a "reviewing" state, so diff should mention it
    let diff = &result["overlay_diff"];
    assert!(diff.is_object(), "overlay_diff should be an object");

    let states_added = diff["states_added"]
        .as_array()
        .expect("states_added should be array");
    assert!(
        states_added.iter().any(|v| v.as_str() == Some("reviewing")),
        "overlay_diff should show 'reviewing' as added state"
    );
}

#[test]
fn add_overlay_rejects_unknown_overlay() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "unknown-overlay-worker");

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "unknown-overlay-worker",
            "overlay": "nonexistent"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("unknown overlay"),
        "Error should mention unknown overlay, got: {}",
        err_msg
    );
}

#[test]
fn add_overlay_rejects_duplicate_overlay() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(&db, "dup-overlay-worker", vec!["git".to_string()]);

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "dup-overlay-worker",
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("already active"),
        "Error should mention already active, got: {}",
        err_msg
    );
}

#[test]
fn add_overlay_rejects_unknown_worker() {
    let db = setup_db();
    let config = app_config_with_overlays();

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "ghost-worker",
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found"),
        "Error should mention not found, got: {}",
        err_msg
    );
}

#[test]
fn add_overlay_rejects_missing_worker_id() {
    let db = setup_db();
    let config = app_config_with_overlays();

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("worker_id"),
        "Error should mention worker_id, got: {}",
        err_msg
    );
}

#[test]
fn add_overlay_rejects_missing_overlay() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "missing-overlay-field-worker");

    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "missing-overlay-field-worker"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("overlay"),
        "Error should mention overlay, got: {}",
        err_msg
    );
}

// =============================================================================
// remove_overlay tool tests
// =============================================================================

#[test]
fn remove_overlay_succeeds_and_persists() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(&db, "remove-overlay-worker", vec!["git".to_string()]);

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "remove-overlay-worker",
            "overlay": "git"
        }),
    )
    .expect("remove_overlay should succeed");

    assert_eq!(result["success"], true);
    assert_eq!(result["worker_id"], "remove-overlay-worker");
    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be array");
    assert!(overlays.is_empty());

    // Verify persistence
    let worker = db
        .get_worker("remove-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert!(worker.overlays.is_empty());
}

#[test]
fn remove_overlay_leaves_other_overlays_intact() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(
        &db,
        "partial-remove-worker",
        vec!["git".to_string(), "troubleshooting".to_string()],
    );

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "partial-remove-worker",
            "overlay": "git"
        }),
    )
    .expect("remove_overlay should succeed");

    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be array");
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0], "troubleshooting");

    // Verify DB state
    let worker = db
        .get_worker("partial-remove-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert_eq!(worker.overlays, vec!["troubleshooting".to_string()]);
}

#[test]
fn remove_overlay_returns_overlay_diff() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(&db, "diff-remove-worker", vec!["git".to_string()]);

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "diff-remove-worker",
            "overlay": "git"
        }),
    )
    .expect("remove_overlay should succeed");

    let diff = &result["overlay_diff"];
    assert!(diff.is_object(), "overlay_diff should be an object");
}

#[test]
fn remove_overlay_rejects_inactive_overlay() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "inactive-overlay-worker");

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "inactive-overlay-worker",
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not active"),
        "Error should mention not active, got: {}",
        err_msg
    );
}

#[test]
fn remove_overlay_rejects_unknown_worker() {
    let db = setup_db();
    let config = app_config_with_overlays();

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "ghost-worker",
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found"),
        "Error should mention not found, got: {}",
        err_msg
    );
}

#[test]
fn remove_overlay_rejects_missing_worker_id() {
    let db = setup_db();
    let config = app_config_with_overlays();

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "overlay": "git"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("worker_id"),
        "Error should mention worker_id, got: {}",
        err_msg
    );
}

#[test]
fn remove_overlay_rejects_missing_overlay() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "missing-overlay-remove-worker");

    let result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "missing-overlay-remove-worker"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("overlay"),
        "Error should mention overlay, got: {}",
        err_msg
    );
}

// =============================================================================
// add + remove round-trip tests
// =============================================================================

#[test]
fn add_then_remove_overlay_round_trip() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "round-trip-worker");

    // Add overlay
    let add_result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "round-trip-worker",
            "overlay": "git"
        }),
    )
    .expect("add_overlay should succeed");

    assert_eq!(add_result["overlays"].as_array().unwrap().len(), 1);

    // Remove overlay
    let remove_result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "round-trip-worker",
            "overlay": "git"
        }),
    )
    .expect("remove_overlay should succeed");

    assert!(remove_result["overlays"].as_array().unwrap().is_empty());

    // Verify DB state
    let worker = db
        .get_worker("round-trip-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert!(worker.overlays.is_empty());
}

#[test]
fn add_multiple_overlays_then_remove_one() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker(&db, "multi-round-trip-worker");

    // Add git overlay
    agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "multi-round-trip-worker",
            "overlay": "git"
        }),
    )
    .expect("add git overlay should succeed");

    // Add troubleshooting overlay
    let add_result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "multi-round-trip-worker",
            "overlay": "troubleshooting"
        }),
    )
    .expect("add troubleshooting overlay should succeed");

    let overlays = add_result["overlays"].as_array().unwrap();
    assert_eq!(overlays.len(), 2);
    assert_eq!(overlays[0], "git");
    assert_eq!(overlays[1], "troubleshooting");

    // Remove git overlay (leaves troubleshooting)
    let remove_result = agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "multi-round-trip-worker",
            "overlay": "git"
        }),
    )
    .expect("remove git overlay should succeed");

    let remaining = remove_result["overlays"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0], "troubleshooting");

    // Verify final DB state
    let worker = db
        .get_worker("multi-round-trip-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert_eq!(worker.overlays, vec!["troubleshooting".to_string()]);
}

#[test]
fn add_overlay_after_remove_succeeds() {
    let db = setup_db();
    let config = app_config_with_overlays();
    register_worker_with_overlays(&db, "readd-overlay-worker", vec!["git".to_string()]);

    // Remove git
    agents::remove_overlay(
        &db,
        &config,
        json!({
            "worker_id": "readd-overlay-worker",
            "overlay": "git"
        }),
    )
    .expect("remove_overlay should succeed");

    // Re-add git (should not fail -- it was removed)
    let result = agents::add_overlay(
        &db,
        &config,
        json!({
            "worker_id": "readd-overlay-worker",
            "overlay": "git"
        }),
    )
    .expect("re-adding removed overlay should succeed");

    let overlays = result["overlays"].as_array().unwrap();
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0], "git");
}
