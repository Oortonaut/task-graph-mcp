//! Tests for schema introspection functionality.

use task_graph_mcp::db::Database;

/// Helper to create a fresh in-memory database for testing.
fn setup_db() -> Database {
    Database::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn get_schema_returns_all_tables() {
    let db = setup_db();

    let schema = db.get_schema(false).expect("Failed to get schema");

    // Should have SQLite version
    assert!(!schema.sqlite_version.is_empty());

    // Should have multiple tables
    assert!(
        schema.tables.len() > 0,
        "Schema should have at least one table"
    );

    // Find the tasks table
    let tasks_table = schema.tables.iter().find(|t| t.name == "tasks");
    assert!(tasks_table.is_some(), "Schema should include 'tasks' table");

    // tasks table should have columns
    let tasks_table = tasks_table.unwrap();
    assert!(
        tasks_table.columns.len() > 0,
        "tasks table should have columns"
    );

    // Verify some expected columns exist
    let column_names: Vec<&str> = tasks_table
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        column_names.contains(&"id"),
        "tasks table should have 'id' column"
    );
    assert!(
        column_names.contains(&"title"),
        "tasks table should have 'title' column"
    );
    assert!(
        column_names.contains(&"status"),
        "tasks table should have 'status' column"
    );
}

#[test]
fn get_schema_includes_column_details() {
    let db = setup_db();

    let schema = db.get_schema(false).expect("Failed to get schema");

    let tasks_table = schema.tables.iter().find(|t| t.name == "tasks").unwrap();

    // Find the id column
    let id_column = tasks_table.columns.iter().find(|c| c.name == "id").unwrap();
    assert!(id_column.primary_key, "id column should be primary key");
    assert!(
        !id_column.data_type.is_empty(),
        "id column should have a data type"
    );
}

#[test]
fn get_schema_includes_foreign_keys() {
    let db = setup_db();

    let schema = db.get_schema(false).expect("Failed to get schema");

    // Find the dependencies table which should have foreign keys
    let deps_table = schema.tables.iter().find(|t| t.name == "dependencies");
    assert!(
        deps_table.is_some(),
        "Schema should include 'dependencies' table"
    );

    let deps_table = deps_table.unwrap();
    // Dependencies table should have foreign keys to tasks
    assert!(
        deps_table.foreign_keys.len() > 0,
        "dependencies table should have foreign keys"
    );
}

#[test]
fn get_schema_with_sql_includes_create_statements() {
    let db = setup_db();

    let schema = db.get_schema(true).expect("Failed to get schema with SQL");

    // At least one table should have SQL
    let has_sql = schema.tables.iter().any(|t| t.sql.is_some());
    assert!(
        has_sql,
        "Schema with include_sql=true should have SQL statements"
    );
}

#[test]
fn get_schema_without_sql_excludes_create_statements() {
    let db = setup_db();

    let schema = db.get_schema(false).expect("Failed to get schema");

    // No table should have SQL
    let has_sql = schema.tables.iter().any(|t| t.sql.is_some());
    assert!(
        !has_sql,
        "Schema with include_sql=false should not have SQL statements"
    );
}

#[test]
fn get_table_names_returns_only_names() {
    let db = setup_db();

    let names = db.get_table_names().expect("Failed to get table names");

    // Should have multiple tables
    assert!(names.len() > 0, "Should have at least one table name");

    // Should include expected tables
    assert!(
        names.contains(&"tasks".to_string()),
        "Should include 'tasks' table"
    );
    assert!(
        names.contains(&"workers".to_string()),
        "Should include 'workers' table"
    );
    assert!(
        names.contains(&"dependencies".to_string()),
        "Should include 'dependencies' table"
    );
}

#[test]
fn get_schema_excludes_internal_tables() {
    let db = setup_db();

    let schema = db.get_schema(false).expect("Failed to get schema");

    // Should not include sqlite internal tables
    let has_sqlite_internal = schema.tables.iter().any(|t| t.name.starts_with("sqlite_"));
    assert!(
        !has_sqlite_internal,
        "Schema should not include sqlite_ internal tables"
    );

    // Should not include refinery migration tables
    let has_refinery = schema
        .tables
        .iter()
        .any(|t| t.name.starts_with("refinery_"));
    assert!(
        !has_refinery,
        "Schema should not include refinery_ migration tables"
    );
}
