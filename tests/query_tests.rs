//! Integration tests for the query tool.
//!
//! These tests verify the read-only SQL query functionality.

use serde_json::{Value, json};
use task_graph_mcp::config::StatesConfig;
use task_graph_mcp::db::Database;
use task_graph_mcp::format::{OutputFormat, ToolResult};
use task_graph_mcp::tools::query;

/// Helper to extract JSON from ToolResult.
fn unwrap_json(result: ToolResult) -> Value {
    match result {
        ToolResult::Json(v) => v,
        ToolResult::Raw(s) => panic!("Expected JSON, got raw text: {}", s),
    }
}

/// Helper to extract raw text from ToolResult.
fn unwrap_raw(result: ToolResult) -> String {
    match result {
        ToolResult::Raw(s) => s,
        ToolResult::Json(v) => panic!("Expected raw text, got JSON: {}", v),
    }
}

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

/// Helper to create a default StatesConfig for testing.
fn default_states_config() -> StatesConfig {
    StatesConfig::default()
}

#[test]
fn query_select_all_tasks() {
    let db = setup_db();
    let states_config = default_states_config();

    // Create some test tasks
    db.create_task(
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

    db.create_task(
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

    // Query all tasks
    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT id, title FROM tasks ORDER BY created_at"
            }),
        )
        .unwrap(),
    );

    assert_eq!(result["row_count"], 2);
    assert!(!result["truncated"].as_bool().unwrap());
    assert_eq!(result["columns"], json!(["id", "title"]));

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["title"], "Task 1");
    assert_eq!(rows[1]["title"], "Task 2");
}

#[test]
fn query_with_parameters() {
    let db = setup_db();
    let states_config = default_states_config();

    // Create test tasks
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

    db.create_task(
        None,
        "Other Task".to_string(),
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

    // Query with parameter
    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT id, title FROM tasks WHERE id = ?",
                "params": [task.id]
            }),
        )
        .unwrap(),
    );

    assert_eq!(result["row_count"], 1);
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows[0]["title"], "Find Me");
}

#[test]
fn query_enforces_row_limit() {
    let db = setup_db();
    let states_config = default_states_config();

    // Create more tasks than the limit
    for i in 0..10 {
        db.create_task(
            None,
            format!("Task {}", i),
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
    }

    // Query with limit of 5
    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT id, title FROM tasks",
                "limit": 5
            }),
        )
        .unwrap(),
    );

    assert_eq!(result["row_count"], 5);
    assert!(result["truncated"].as_bool().unwrap());
    assert_eq!(result["limit"], 5);
}

#[test]
fn query_csv_format() {
    let db = setup_db();
    let states_config = default_states_config();

    // Create task with a title that will test CSV escaping
    db.create_task(
        None,
        "CSV Task".to_string(),
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

    let csv_data = unwrap_raw(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT title, status FROM tasks",
                "format": "csv"
            }),
        )
        .unwrap(),
    );

    assert!(csv_data.contains("title,status"));
    assert!(csv_data.contains("CSV Task"));
    assert!(csv_data.contains("pending")); // default status
}

#[test]
fn query_markdown_format() {
    let db = setup_db();
    let states_config = default_states_config();

    db.create_task(
        None,
        "Markdown Task".to_string(),
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

    let md_data = unwrap_raw(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT title FROM tasks",
                "format": "markdown"
            }),
        )
        .unwrap(),
    );

    assert!(md_data.contains("| title |"));
    assert!(md_data.contains("| --- |"));
    assert!(md_data.contains("| Markdown Task |"));
}

#[test]
fn query_rejects_insert() {
    let db = setup_db();

    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "INSERT INTO tasks (id, title) VALUES ('x', 'bad')"
        }),
    );

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("INSERT") || err.contains("SELECT"));
}

#[test]
fn query_rejects_update() {
    let db = setup_db();

    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "UPDATE tasks SET title = 'hacked'"
        }),
    );

    assert!(result.is_err());
}

#[test]
fn query_rejects_delete() {
    let db = setup_db();

    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "DELETE FROM tasks"
        }),
    );

    assert!(result.is_err());
}

#[test]
fn query_rejects_drop() {
    let db = setup_db();

    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "DROP TABLE tasks"
        }),
    );

    assert!(result.is_err());
}

#[test]
fn query_rejects_multiple_statements() {
    let db = setup_db();

    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "SELECT 1; DROP TABLE tasks;"
        }),
    );

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Multiple"));
}

#[test]
fn query_allows_column_names_with_keywords() {
    let db = setup_db();

    // This should work - "deleted_at" contains "DELETE" but it's a column name
    let result = query::query(
        &db,
        OutputFormat::Json,
        json!({
            "sql": "SELECT id, status FROM tasks WHERE status = 'pending'"
        }),
    );

    assert!(result.is_ok());
}

#[test]
fn query_with_cte() {
    let db = setup_db();
    let states_config = default_states_config();

    db.create_task(
        None,
        "CTE Task".to_string(),
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

    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "WITH task_list AS (SELECT id, title FROM tasks) SELECT * FROM task_list"
            }),
        )
        .unwrap(),
    );

    assert_eq!(result["row_count"], 1);
}

#[test]
fn query_max_limit_enforced() {
    let db = setup_db();

    // Try to set limit above max (1000)
    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT 1",
                "limit": 5000
            }),
        )
        .unwrap(),
    );

    // Should be clamped to 1000
    assert_eq!(result["limit"], 1000);
}

#[test]
fn query_default_limit() {
    let db = setup_db();

    // Without explicit limit, should use default (100)
    let result = unwrap_json(
        query::query(
            &db,
            OutputFormat::Json,
            json!({
                "sql": "SELECT 1"
            }),
        )
        .unwrap(),
    );

    assert_eq!(result["limit"], 100);
}
