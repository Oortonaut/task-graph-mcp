//! Integration tests for the connect tool with workflow parameter.
//!
//! These tests verify that the connect tool properly handles the workflow parameter,
//! returning it in the response and storing it in the database.

use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use task_graph_mcp::config::workflows::WorkflowsConfig;
use task_graph_mcp::config::{
    AppConfig, AttachmentsConfig, AutoAdvanceConfig, DependenciesConfig, FeedbackConfig, IdsConfig,
    PhasesConfig, ServerPaths, StatesConfig, TagsConfig,
};
use task_graph_mcp::db::Database;
use task_graph_mcp::tools::agents::{self, ConnectOptions};

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

/// Helper to create a default AppConfig for testing.
fn default_app_config() -> AppConfig {
    let workflows = Arc::new(WorkflowsConfig::default());
    AppConfig::new(
        Arc::new(StatesConfig::default()),
        Arc::new(PhasesConfig::default()),
        Arc::new(DependenciesConfig::default()),
        Arc::new(AutoAdvanceConfig::default()),
        Arc::new(AttachmentsConfig::default()),
        Arc::new(TagsConfig::default()),
        Arc::new(IdsConfig::default()),
        workflows,
        Arc::new(FeedbackConfig::default()),
    )
}

/// Helper to create a test ServerPaths.
fn test_server_paths() -> ServerPaths {
    ServerPaths {
        db_path: PathBuf::from(":memory:"),
        media_dir: PathBuf::from("test-media"),
        log_dir: PathBuf::from("test-logs"),
        config_path: None,
    }
}

#[test]
fn connect_without_workflow_returns_null_workflow() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "test-worker-no-workflow"
        }),
    )
    .expect("connect should succeed");

    // Workflow should be null when not provided
    assert!(result["workflow"].is_null());
    assert_eq!(result["worker_id"], "test-worker-no-workflow");
}

#[test]
fn connect_with_workflow_returns_workflow_in_response() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "test-worker-with-workflow",
            "workflow": "swarm"
        }),
    )
    .expect("connect should succeed");

    // Workflow should be present in response
    assert_eq!(result["workflow"], "swarm");
    assert_eq!(result["worker_id"], "test-worker-with-workflow");
}

#[test]
fn connect_stores_workflow_in_database() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Connect with workflow
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "db-workflow-worker",
            "workflow": "coordinator"
        }),
    )
    .expect("connect should succeed");

    // Verify the worker has the workflow stored in the database
    let worker = db
        .get_worker("db-workflow-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.workflow, Some("coordinator".to_string()));
}

#[test]
fn connect_stores_null_workflow_when_not_provided() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Connect without workflow
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "no-workflow-worker"
        }),
    )
    .expect("connect should succeed");

    // Verify the worker has no workflow in the database
    let worker = db
        .get_worker("no-workflow-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert!(worker.workflow.is_none());
}

#[test]
fn connect_with_force_updates_workflow() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect with workflow "alpha"
    let result1 = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "force-workflow-worker",
            "workflow": "alpha"
        }),
    )
    .expect("first connect should succeed");

    assert_eq!(result1["workflow"], "alpha");

    // Reconnect with force=true and different workflow
    let result2 = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "force-workflow-worker",
            "workflow": "beta",
            "force": true
        }),
    )
    .expect("force reconnect should succeed");

    assert_eq!(result2["workflow"], "beta");

    // Verify database reflects the updated workflow
    let worker = db
        .get_worker("force-workflow-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.workflow, Some("beta".to_string()));
}

#[test]
fn connect_with_force_can_clear_workflow() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect with workflow
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "clear-workflow-worker",
            "workflow": "initial"
        }),
    )
    .expect("first connect should succeed");

    // Reconnect with force but no workflow (should clear it)
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "clear-workflow-worker",
            "force": true
        }),
    )
    .expect("force reconnect should succeed");

    assert!(result["workflow"].is_null());

    // Verify database shows null workflow
    let worker = db
        .get_worker("clear-workflow-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert!(worker.workflow.is_none());
}

#[test]
fn connect_without_force_fails_for_existing_worker() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "duplicate-worker",
            "workflow": "original"
        }),
    )
    .expect("first connect should succeed");

    // Second connect without force should fail
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "duplicate-worker",
            "workflow": "different"
        }),
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("already registered"));
}

#[test]
fn connect_response_includes_all_expected_fields() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "full-response-worker",
            "workflow": "test-workflow",
            "tags": ["rust", "backend"]
        }),
    )
    .expect("connect should succeed");

    // Verify all expected fields are present
    assert_eq!(result["worker_id"], "full-response-worker");
    assert_eq!(result["workflow"], "test-workflow");
    assert!(result["version"].is_string());
    assert!(
        result["registered_at"].is_string(),
        "registered_at should be ISO 8601 string"
    );
    assert!(result["max_claims"].is_number());
    assert!(result["paths"].is_object());
    assert!(result["config"].is_object());

    // Verify tags are preserved
    let tags = result["tags"].as_array().expect("tags should be array");
    assert_eq!(tags.len(), 2);
    assert!(tags.contains(&json!("rust")));
    assert!(tags.contains(&json!("backend")));
}

#[test]
fn connect_with_empty_workflow_string_stores_empty() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Connect with empty string workflow (not null/None)
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "empty-workflow-worker",
            "workflow": ""
        }),
    )
    .expect("connect should succeed");

    // Empty string is still stored
    assert_eq!(result["workflow"], "");

    // Verify database stores empty string
    let worker = db
        .get_worker("empty-workflow-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.workflow, Some("".to_string()));
}

// ========================================================================
// Overlay tests
// ========================================================================

#[test]
fn connect_with_overlays_stores_overlays_in_database() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "overlay-db-worker",
            "overlays": ["git", "user-request"]
        }),
    )
    .expect("connect should succeed");

    // Verify the worker has overlays stored in the database
    let worker = db
        .get_worker("overlay-db-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(
        worker.overlays,
        vec!["git".to_string(), "user-request".to_string()]
    );
}

#[test]
fn connect_response_includes_overlays_when_non_empty() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "overlay-response-worker",
            "overlays": ["git", "troubleshooting"]
        }),
    )
    .expect("connect should succeed");

    // Overlays should be present in response
    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(overlays.len(), 2);
    assert_eq!(overlays[0], "git");
    assert_eq!(overlays[1], "troubleshooting");
    assert_eq!(result["worker_id"], "overlay-response-worker");
}

#[test]
fn connect_without_overlays_returns_no_overlays_field() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "no-overlay-worker"
        }),
    )
    .expect("connect should succeed");

    // Overlays field should be absent (null) when not provided
    assert!(result["overlays"].is_null());
    assert_eq!(result["worker_id"], "no-overlay-worker");

    // Verify database stores empty overlays
    let worker = db
        .get_worker("no-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert!(worker.overlays.is_empty());
}

#[test]
fn connect_with_force_updates_overlays() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect with overlays ["alpha"]
    let result1 = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "force-overlay-worker",
            "overlays": ["alpha"]
        }),
    )
    .expect("first connect should succeed");

    let overlays1 = result1["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(overlays1.len(), 1);
    assert_eq!(overlays1[0], "alpha");

    // Reconnect with force=true and different overlays
    let result2 = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "force-overlay-worker",
            "overlays": ["beta", "gamma"],
            "force": true
        }),
    )
    .expect("force reconnect should succeed");

    let overlays2 = result2["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(overlays2.len(), 2);
    assert_eq!(overlays2[0], "beta");
    assert_eq!(overlays2[1], "gamma");

    // Verify database reflects the updated overlays
    let worker = db
        .get_worker("force-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(
        worker.overlays,
        vec!["beta".to_string(), "gamma".to_string()]
    );
}

#[test]
fn connect_with_force_can_clear_overlays() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect with overlays
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "clear-overlay-worker",
            "overlays": ["initial-overlay"]
        }),
    )
    .expect("first connect should succeed");

    // Reconnect with force but no overlays (should clear them)
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "clear-overlay-worker",
            "force": true
        }),
    )
    .expect("force reconnect should succeed");

    assert!(result["overlays"].is_null());

    // Verify database shows empty overlays
    let worker = db
        .get_worker("clear-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert!(worker.overlays.is_empty());
}

#[test]
fn list_workers_includes_overlays() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Register a worker with overlays
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "overlay-list-a",
            "overlays": ["git", "review"]
        }),
    )
    .expect("connect should succeed");

    // Register a worker without overlays
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "overlay-list-b"
        }),
    )
    .expect("connect should succeed");

    // List workers and verify overlays
    let workers = db.list_workers().expect("list should succeed");

    let worker_a = workers.iter().find(|w| w.id == "overlay-list-a");
    let worker_b = workers.iter().find(|w| w.id == "overlay-list-b");

    assert!(worker_a.is_some());
    assert!(worker_b.is_some());

    assert_eq!(
        worker_a.unwrap().overlays,
        vec!["git".to_string(), "review".to_string()]
    );
    assert!(worker_b.unwrap().overlays.is_empty());
}

#[test]
fn list_workers_includes_workflow() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Register workers with different workflows
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "worker-a",
            "workflow": "swarm"
        }),
    )
    .expect("connect should succeed");

    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "worker-b"
        }),
    )
    .expect("connect should succeed");

    // List workers and verify workflows
    let workers = db.list_workers().expect("list should succeed");

    let worker_a = workers.iter().find(|w| w.id == "worker-a");
    let worker_b = workers.iter().find(|w| w.id == "worker-b");

    assert!(worker_a.is_some());
    assert!(worker_b.is_some());

    assert_eq!(worker_a.unwrap().workflow, Some("swarm".to_string()));
    assert!(worker_b.unwrap().workflow.is_none());
}

// ========================================================================
// Additional overlay edge-case tests
// ========================================================================

/// Verify that passing an empty overlays array [] is stored as NULL in the DB,
/// not as a JSON "[]" string. This ensures empty overlays are treated the same
/// as absent overlays.
#[test]
fn connect_with_empty_overlays_array_stored_as_null() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "empty-array-overlay-worker",
            "overlays": []
        }),
    )
    .expect("connect should succeed");

    // Response should not include overlays when array is empty
    assert!(result["overlays"].is_null());

    // Database should store empty vec (parsed from NULL)
    let worker = db
        .get_worker("empty-array-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert!(worker.overlays.is_empty());
}

/// Round-trip test: connect with overlays, then retrieve the worker from
/// the DB and verify the overlays are correctly parsed back from JSON.
#[test]
fn connect_overlays_round_trip_through_db() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let overlays_input = vec!["git", "user-request", "troubleshooting"];

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "round-trip-overlay-worker",
            "overlays": overlays_input
        }),
    )
    .expect("connect should succeed");

    // Verify response
    let response_overlays = result["overlays"]
        .as_array()
        .expect("overlays should be an array in response");
    assert_eq!(response_overlays.len(), 3);
    assert_eq!(response_overlays[0], "git");
    assert_eq!(response_overlays[1], "user-request");
    assert_eq!(response_overlays[2], "troubleshooting");

    // Verify DB round-trip via get_worker (parses JSON from DB)
    let worker = db
        .get_worker("round-trip-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.overlays.len(), 3);
    assert_eq!(worker.overlays[0], "git");
    assert_eq!(worker.overlays[1], "user-request");
    assert_eq!(worker.overlays[2], "troubleshooting");
}

/// Test that connect with both overlays and workflow stores both independently.
#[test]
fn connect_with_overlays_and_workflow_combined() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "combo-worker",
            "workflow": "swarm",
            "overlays": ["git", "review"]
        }),
    )
    .expect("connect should succeed");

    // Verify both are in response
    assert_eq!(result["workflow"], "swarm");
    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(overlays.len(), 2);
    assert_eq!(overlays[0], "git");
    assert_eq!(overlays[1], "review");

    // Verify both stored in DB
    let worker = db
        .get_worker("combo-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.workflow, Some("swarm".to_string()));
    assert_eq!(
        worker.overlays,
        vec!["git".to_string(), "review".to_string()]
    );
}

/// Test that force reconnection with overlays completely replaces
/// the previous overlay set (not appending).
#[test]
fn connect_with_overlays_force_reconnection_replaces_overlays() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // First connect with 3 overlays
    agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "replace-overlay-worker",
            "overlays": ["alpha", "beta", "gamma"]
        }),
    )
    .expect("first connect should succeed");

    // Verify initial state
    let worker1 = db
        .get_worker("replace-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");
    assert_eq!(worker1.overlays.len(), 3);

    // Force reconnect with just 1 overlay - should completely replace, not append
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "replace-overlay-worker",
            "overlays": ["delta"],
            "force": true
        }),
    )
    .expect("force reconnect should succeed");

    let overlays = result["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0], "delta");

    // Verify DB reflects the replacement, not a merge
    let worker2 = db
        .get_worker("replace-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker2.overlays, vec!["delta".to_string()]);
}

/// Test that overlays order is preserved through the connect -> DB -> read cycle.
#[test]
fn connect_overlays_preserve_order() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let app_config = default_app_config();

    // Overlay order matters for precedence - verify it's preserved
    let result = agents::connect(
        ConnectOptions {
            db: &db,
            server_paths: &server_paths,
            config: &app_config,
            workflows: &WorkflowsConfig::default(),
        },
        json!({
            "worker_id": "order-overlay-worker",
            "overlays": ["first", "second", "third"]
        }),
    )
    .expect("connect should succeed");

    // Verify order in response
    let resp_overlays = result["overlays"]
        .as_array()
        .expect("overlays should be an array");
    assert_eq!(resp_overlays[0], "first");
    assert_eq!(resp_overlays[1], "second");
    assert_eq!(resp_overlays[2], "third");

    // Verify order from DB
    let worker = db
        .get_worker("order-overlay-worker")
        .expect("get_worker should succeed")
        .expect("worker should exist");

    assert_eq!(worker.overlays[0], "first");
    assert_eq!(worker.overlays[1], "second");
    assert_eq!(worker.overlays[2], "third");
}
