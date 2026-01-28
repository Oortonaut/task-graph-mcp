//! Integration tests for the connect tool with workflow parameter.
//!
//! These tests verify that the connect tool properly handles the workflow parameter,
//! returning it in the response and storing it in the database.

use serde_json::json;
use std::path::PathBuf;
use task_graph_mcp::config::{
    DependenciesConfig, IdsConfig, PhasesConfig, ServerPaths, StatesConfig, TagsConfig,
};
use task_graph_mcp::db::Database;
use task_graph_mcp::tools::agents;

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

/// Helper to create default configs for testing.
fn default_configs() -> (
    StatesConfig,
    PhasesConfig,
    DependenciesConfig,
    TagsConfig,
    IdsConfig,
) {
    (
        StatesConfig::default(),
        PhasesConfig::default(),
        DependenciesConfig::default(),
        TagsConfig::default(),
        IdsConfig::default(),
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
    let (states, phases, deps, tags, ids) = default_configs();

    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    // Connect with workflow
    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    // Connect without workflow
    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    // First connect with workflow "alpha"
    let result1 = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
        json!({
            "worker_id": "force-workflow-worker",
            "workflow": "alpha"
        }),
    )
    .expect("first connect should succeed");

    assert_eq!(result1["workflow"], "alpha");

    // Reconnect with force=true and different workflow
    let result2 = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    // First connect with workflow
    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
        json!({
            "worker_id": "clear-workflow-worker",
            "workflow": "initial"
        }),
    )
    .expect("first connect should succeed");

    // Reconnect with force but no workflow (should clear it)
    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    // First connect
    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
        json!({
            "worker_id": "duplicate-worker",
            "workflow": "original"
        }),
    )
    .expect("first connect should succeed");

    // Second connect without force should fail
    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    let (states, phases, deps, tags, ids) = default_configs();

    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
    assert!(result["registered_at"].is_number());
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
    let (states, phases, deps, tags, ids) = default_configs();

    // Connect with empty string workflow (not null/None)
    let result = agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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

#[test]
fn list_workers_includes_workflow() {
    let db = setup_db();
    let server_paths = test_server_paths();
    let (states, phases, deps, tags, ids) = default_configs();

    // Register workers with different workflows
    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
        json!({
            "worker_id": "worker-a",
            "workflow": "swarm"
        }),
    )
    .expect("connect should succeed");

    agents::connect(
        &db,
        &server_paths,
        &states,
        &phases,
        &deps,
        &tags,
        &ids,
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
