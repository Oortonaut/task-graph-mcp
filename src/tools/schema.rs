//! Schema introspection tool for exposing database structure.

use super::{get_bool, get_string, make_tool};
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::{Value, json};

/// Get the schema introspection tools.
pub fn get_tools() -> Vec<Tool> {
    vec![make_tool(
        "get_schema",
        "Get the task-graph database schema. Returns table names, columns (with types), indexes, foreign keys, and optionally the SQL definitions. Useful for agents writing reports or queries.",
        json!({
            "table": {
                "type": "string",
                "description": "Filter to a specific table name. If not provided, returns all tables."
            },
            "include_sql": {
                "type": "boolean",
                "description": "Include the SQL CREATE statements (default: false)"
            }
        }),
        vec![],
    )]
}

/// Get database schema information.
pub fn get_schema(db: &Database, args: Value) -> Result<Value> {
    let table_filter = get_string(&args, "table");
    let include_sql = get_bool(&args, "include_sql").unwrap_or(false);

    let schema = db.get_schema(include_sql)?;

    // If a specific table is requested, filter the results
    let tables = if let Some(ref table_name) = table_filter {
        schema
            .tables
            .into_iter()
            .filter(|t| t.name.eq_ignore_ascii_case(table_name))
            .collect()
    } else {
        schema.tables
    };

    if let Some(table_name) = table_filter
        && tables.is_empty()
    {
        return Ok(json!({
            "error": format!("Table '{}' not found", table_name),
            "available_tables": db.get_table_names()?
        }));
    }

    Ok(json!({
        "sqlite_version": schema.sqlite_version,
        "table_count": tables.len(),
        "tables": tables.iter().map(|t| {
            let mut table_obj = json!({
                "name": t.name,
                "type": t.table_type,
                "columns": t.columns.iter().map(|c| {
                    json!({
                        "name": c.name,
                        "type": c.data_type,
                        "nullable": c.nullable,
                        "primary_key": c.primary_key,
                        "default": c.default_value
                    })
                }).collect::<Vec<_>>()
            });

            // Only include indexes if there are any
            if !t.indexes.is_empty() {
                table_obj["indexes"] = json!(t.indexes.iter().map(|i| {
                    json!({
                        "name": i.name,
                        "unique": i.unique,
                        "columns": i.columns
                    })
                }).collect::<Vec<_>>());
            }

            // Only include foreign keys if there are any
            if !t.foreign_keys.is_empty() {
                table_obj["foreign_keys"] = json!(t.foreign_keys.iter().map(|fk| {
                    json!({
                        "from": fk.from_column,
                        "references": format!("{}.{}", fk.to_table, fk.to_column),
                        "on_delete": fk.on_delete,
                        "on_update": fk.on_update
                    })
                }).collect::<Vec<_>>());
            }

            // Include SQL if requested
            if let Some(ref sql) = t.sql {
                table_obj["sql"] = json!(sql);
            }

            table_obj
        }).collect::<Vec<_>>()
    }))
}
