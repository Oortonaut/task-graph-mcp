//! Read-only SQL query tool.
//!
//! Provides a `query` tool for executing read-only SQL queries against the database.
//! This tool is intended for advanced users and debugging purposes.
//!
//! SECURITY: This tool requires user permission before execution and only allows
//! SELECT statements. INSERT, UPDATE, DELETE, DROP, and other modifying statements
//! are rejected.

use super::{get_i32, get_string, get_string_array, make_tool};
use crate::db::Database;
use crate::error::{ErrorCode, ToolError};
use crate::format::{OutputFormat, ToolResult};
use anyhow::Result;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};
use std::time::Duration;

/// Default row limit for query results.
const DEFAULT_ROW_LIMIT: i32 = 100;

/// Maximum allowed row limit.
const MAX_ROW_LIMIT: i32 = 1000;

/// Query execution timeout in seconds.
const QUERY_TIMEOUT_SECS: u64 = 5;

/// Output format for query results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFormat {
    Json,
    Csv,
    Markdown,
}

impl QueryFormat {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "json" => Some(Self::Json),
            "csv" => Some(Self::Csv),
            "markdown" | "md" => Some(Self::Markdown),
            _ => None,
        }
    }
}

/// Get all query-related tools.
pub fn get_tools() -> Vec<Tool> {
    let mut tool = make_tool(
        "query",
        "Execute a read-only SQL query against the task database. REQUIRES USER PERMISSION. \
         Only SELECT statements are allowed. Useful for custom queries, debugging, and \
         advanced reporting. Returns columns, rows, and metadata.",
        json!({
            "sql": {
                "type": "string",
                "description": "SQL SELECT query to execute. Only SELECT statements are allowed."
            },
            "params": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Bind parameters for the query (use ? placeholders in SQL)"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of rows to return (default: 100, max: 1000)"
            },
            "format": {
                "type": "string",
                "enum": ["json", "csv", "markdown"],
                "description": "Output format for results (default: json)"
            }
        }),
        vec!["sql"],
    );

    // Add annotations to indicate this is a read-only but potentially sensitive tool
    // The destructiveHint is false because we only allow SELECT
    // readOnlyHint is true because we don't modify data
    tool.annotations = Some(ToolAnnotations {
        title: Some("SQL Query".into()),
        read_only_hint: Some(true),
        destructive_hint: Some(false),
        idempotent_hint: Some(true),
        open_world_hint: Some(false),
    });

    vec![tool]
}

/// Validate that a SQL query is read-only (SELECT only).
fn validate_readonly_sql(sql: &str) -> Result<(), ToolError> {
    // Normalize whitespace and convert to uppercase for checking
    let normalized = sql.trim().to_uppercase();

    // Check for forbidden statements
    let forbidden_prefixes = [
        "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "REPLACE", "UPSERT",
        "MERGE", "GRANT", "REVOKE", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE",
        "PRAGMA", // Some PRAGMAs can modify settings
    ];

    // Get the first word (statement type)
    let first_word = normalized.split_whitespace().next().unwrap_or("");

    if first_word != "SELECT" && first_word != "WITH" {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            format!(
                "Only SELECT queries are allowed. Got: {}",
                if first_word.len() > 20 {
                    &first_word[..20]
                } else {
                    first_word
                }
            ),
        )
        .with_field("sql"));
    }

    // Additional check for CTEs (WITH ... SELECT is OK, but WITH ... INSERT/UPDATE/DELETE is not)
    if first_word == "WITH" {
        // Look for modification keywords after WITH clause
        for forbidden in &forbidden_prefixes {
            // Check if the forbidden keyword appears as a standalone word (not in quotes or names)
            let pattern = format!(r"\b{}\b", forbidden);
            if let Ok(re) = regex_lite::Regex::new(&pattern)
                && re.is_match(&normalized)
            {
                return Err(ToolError::new(
                    ErrorCode::InvalidFieldValue,
                    format!("{} statements are not allowed in queries", forbidden),
                )
                .with_field("sql"));
            }
        }
    }

    // Check for semicolons that might indicate multiple statements
    // (SQLite allows this but we want to prevent injection)
    let semicolon_count = sql.matches(';').count();
    if semicolon_count > 1 {
        return Err(ToolError::new(
            ErrorCode::InvalidFieldValue,
            "Multiple SQL statements are not allowed",
        )
        .with_field("sql"));
    }

    // Check for forbidden keywords anywhere in the query (for subqueries or injection attempts)
    for forbidden in &forbidden_prefixes {
        // Use word boundary matching to avoid false positives like "DELETED_AT"
        let pattern = format!(r"\b{}\s+", forbidden);
        if let Ok(re) = regex_lite::Regex::new(&pattern)
            && re.is_match(&normalized)
        {
            return Err(ToolError::new(
                ErrorCode::InvalidFieldValue,
                format!("{} statements are not allowed", forbidden),
            )
            .with_field("sql"));
        }
    }

    Ok(())
}

/// Execute a read-only SQL query.
pub fn query(db: &Database, default_format: OutputFormat, args: Value) -> Result<ToolResult> {
    let sql = get_string(&args, "sql").ok_or_else(|| ToolError::missing_field("sql"))?;

    let params = get_string_array(&args, "params").unwrap_or_default();

    let limit = get_i32(&args, "limit")
        .map(|l| l.clamp(1, MAX_ROW_LIMIT))
        .unwrap_or(DEFAULT_ROW_LIMIT);

    // Use explicit format if provided, otherwise use config default
    let format = get_string(&args, "format")
        .and_then(|f| QueryFormat::from_str(&f))
        .unwrap_or(match default_format {
            OutputFormat::Json => QueryFormat::Json,
            OutputFormat::Markdown => QueryFormat::Markdown,
        });

    // Validate the query is read-only
    validate_readonly_sql(&sql)?;

    // Execute the query with timeout
    let result = db.with_conn(|conn| {
        // Set a busy timeout for this connection
        conn.busy_timeout(Duration::from_secs(QUERY_TIMEOUT_SECS))?;

        // Prepare the statement
        let mut stmt = conn.prepare(&sql)?;

        // Get column names
        let column_count = stmt.column_count();
        let columns: Vec<String> = (0..column_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        // Bind parameters
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        // Execute and collect rows
        let mut rows_data: Vec<Vec<Value>> = Vec::new();
        let mut row_iter = stmt.query(params_refs.as_slice())?;

        let mut count = 0;
        while let Some(row) = row_iter.next()? {
            if count >= limit {
                break;
            }

            let mut row_values: Vec<Value> = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let value: Value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(i) => json!(i),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(s) => {
                        json!(String::from_utf8_lossy(s).to_string())
                    }
                    rusqlite::types::ValueRef::Blob(b) => {
                        json!(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            b
                        ))
                    }
                };
                row_values.push(value);
            }
            rows_data.push(row_values);
            count += 1;
        }

        // Check if there are more rows (for truncated flag)
        let has_more = row_iter.next()?.is_some();

        Ok((columns, rows_data, count, has_more))
    })?;

    let (columns, rows_data, row_count, truncated) = result;

    // Format the output based on requested format
    match format {
        QueryFormat::Json => {
            // Convert rows to objects with column names as keys
            let rows: Vec<Value> = rows_data
                .iter()
                .map(|row| {
                    let obj: serde_json::Map<String, Value> = columns
                        .iter()
                        .zip(row.iter())
                        .map(|(col, val)| (col.clone(), val.clone()))
                        .collect();
                    Value::Object(obj)
                })
                .collect();

            Ok(ToolResult::Json(json!({
                "columns": columns,
                "rows": rows,
                "row_count": row_count,
                "truncated": truncated,
                "limit": limit
            })))
        }
        QueryFormat::Csv => {
            let mut csv = String::new();
            // Header
            csv.push_str(&columns.join(","));
            csv.push('\n');
            // Rows
            for row in &rows_data {
                let values: Vec<String> = row
                    .iter()
                    .map(|v| match v {
                        Value::Null => String::new(),
                        Value::String(s) => {
                            // Escape quotes and wrap in quotes if contains comma or quotes
                            if s.contains(',') || s.contains('"') || s.contains('\n') {
                                format!("\"{}\"", s.replace('"', "\"\""))
                            } else {
                                s.clone()
                            }
                        }
                        _ => v.to_string(),
                    })
                    .collect();
                csv.push_str(&values.join(","));
                csv.push('\n');
            }

            // CSV is raw text output
            if truncated {
                csv.push_str(&format!("\n# Results truncated at {} rows\n", limit));
            }
            Ok(ToolResult::Raw(csv))
        }
        QueryFormat::Markdown => {
            let mut md = String::new();

            if columns.is_empty() {
                md.push_str("*No columns*\n");
            } else {
                // Header
                md.push_str("| ");
                md.push_str(&columns.join(" | "));
                md.push_str(" |\n");

                // Separator
                md.push_str("| ");
                md.push_str(
                    &columns
                        .iter()
                        .map(|_| "---")
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                md.push_str(" |\n");

                // Rows
                for row in &rows_data {
                    md.push_str("| ");
                    let values: Vec<String> = row
                        .iter()
                        .map(|v| match v {
                            Value::Null => String::from("*null*"),
                            Value::String(s) => s.replace('|', "\\|"),
                            _ => v.to_string(),
                        })
                        .collect();
                    md.push_str(&values.join(" | "));
                    md.push_str(" |\n");
                }
            }

            if truncated {
                md.push_str(&format!("\n*Results truncated at {} rows*\n", limit));
            }

            Ok(ToolResult::Raw(md))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_readonly_select() {
        assert!(validate_readonly_sql("SELECT * FROM tasks").is_ok());
        assert!(validate_readonly_sql("  SELECT id FROM tasks WHERE status = 'pending'  ").is_ok());
        assert!(validate_readonly_sql("select count(*) from tasks").is_ok());
    }

    #[test]
    fn test_validate_readonly_with_cte() {
        assert!(validate_readonly_sql(
            "WITH task_counts AS (SELECT status, COUNT(*) as cnt FROM tasks GROUP BY status) SELECT * FROM task_counts"
        ).is_ok());
    }

    #[test]
    fn test_validate_readonly_rejects_insert() {
        let result = validate_readonly_sql("INSERT INTO tasks (title) VALUES ('test')");
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("INSERT"));
    }

    #[test]
    fn test_validate_readonly_rejects_update() {
        let result = validate_readonly_sql("UPDATE tasks SET status = 'done'");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_readonly_rejects_delete() {
        let result = validate_readonly_sql("DELETE FROM tasks WHERE id = 'xxx'");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_readonly_rejects_drop() {
        let result = validate_readonly_sql("DROP TABLE tasks");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_readonly_rejects_multiple_statements() {
        let result = validate_readonly_sql("SELECT 1; DROP TABLE tasks;");
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Multiple"));
    }

    #[test]
    fn test_validate_readonly_allows_column_names_with_keywords() {
        // Column names like "deleted_at" or "updated_at" should be allowed
        assert!(validate_readonly_sql("SELECT deleted_at FROM tasks").is_ok());
        assert!(validate_readonly_sql("SELECT updated_at, created_at FROM tasks").is_ok());
    }

    #[test]
    fn test_query_format_parsing() {
        assert_eq!(QueryFormat::from_str("json"), Some(QueryFormat::Json));
        assert_eq!(QueryFormat::from_str("JSON"), Some(QueryFormat::Json));
        assert_eq!(QueryFormat::from_str("csv"), Some(QueryFormat::Csv));
        assert_eq!(
            QueryFormat::from_str("markdown"),
            Some(QueryFormat::Markdown)
        );
        assert_eq!(QueryFormat::from_str("md"), Some(QueryFormat::Markdown));
        assert_eq!(QueryFormat::from_str("invalid"), None);
    }
}
