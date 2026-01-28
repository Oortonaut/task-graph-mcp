//! Diff functionality for comparing snapshots and databases.
//!
//! This module provides:
//! - Comparison between a snapshot file and the current database state
//! - Comparison between two snapshot files
//! - Human-readable diff output suitable for review

use super::{EXPORTED_TABLES, Snapshot, get_table_primary_key};
use crate::db::Database;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fmt;

/// A single field change within a record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub old_value: Value,
    pub new_value: Value,
}

/// A modified record showing which fields changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedRecord {
    /// The primary key value(s) identifying this record
    pub key: Value,
    /// List of field changes
    pub changes: Vec<FieldChange>,
}

/// Diff results for a single table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TableDiff {
    /// Records present in target but not in source (added)
    pub added: Vec<Value>,
    /// Records present in source but not in target (removed)
    pub removed: Vec<Value>,
    /// Records present in both but with different values
    pub modified: Vec<ModifiedRecord>,
}

impl TableDiff {
    /// Check if there are any changes in this table.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Total number of changes.
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }
}

/// Complete diff between two data sources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotDiff {
    /// Source description (e.g., "snapshot.json" or "database")
    pub source_label: String,
    /// Target description
    pub target_label: String,
    /// Diff results per table
    pub tables: BTreeMap<String, TableDiff>,
}

impl SnapshotDiff {
    /// Check if there are any changes.
    pub fn is_empty(&self) -> bool {
        self.tables.values().all(|t| t.is_empty())
    }

    /// Total number of changes across all tables.
    pub fn total_changes(&self) -> usize {
        self.tables.values().map(|t| t.change_count()).sum()
    }

    /// Get a summary of changes per table.
    pub fn summary(&self) -> Vec<(String, usize, usize, usize)> {
        self.tables
            .iter()
            .filter(|(_, diff)| !diff.is_empty())
            .map(|(name, diff)| {
                (
                    name.clone(),
                    diff.added.len(),
                    diff.removed.len(),
                    diff.modified.len(),
                )
            })
            .collect()
    }
}

impl fmt::Display for SnapshotDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            writeln!(f, "No differences found.")?;
            return Ok(());
        }

        writeln!(f, "Diff: {} -> {}", self.source_label, self.target_label)?;
        writeln!(f, "{}", "=".repeat(60))?;

        for (table_name, diff) in &self.tables {
            if diff.is_empty() {
                continue;
            }

            writeln!(f)?;
            writeln!(f, "Table: {}", table_name)?;
            writeln!(f, "{}", "-".repeat(40))?;

            if !diff.added.is_empty() {
                writeln!(f, "  Added ({}):", diff.added.len())?;
                for record in &diff.added {
                    writeln!(f, "    + {}", format_record_brief(record))?;
                }
            }

            if !diff.removed.is_empty() {
                writeln!(f, "  Removed ({}):", diff.removed.len())?;
                for record in &diff.removed {
                    writeln!(f, "    - {}", format_record_brief(record))?;
                }
            }

            if !diff.modified.is_empty() {
                writeln!(f, "  Modified ({}):", diff.modified.len())?;
                for modified in &diff.modified {
                    writeln!(f, "    ~ {}", modified.key)?;
                    for change in &modified.changes {
                        writeln!(
                            f,
                            "        {}: {} -> {}",
                            change.field, change.old_value, change.new_value
                        )?;
                    }
                }
            }
        }

        writeln!(f)?;
        writeln!(f, "Summary: {} total changes", self.total_changes())?;

        Ok(())
    }
}

/// Format a record for brief display (showing key fields).
fn format_record_brief(record: &Value) -> String {
    if let Some(obj) = record.as_object() {
        // Try to show id and title if available
        let id = obj.get("id").map(|v| v.to_string()).unwrap_or_default();
        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 40 {
                    format!("{}...", &s[..37])
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_default();

        if !title.is_empty() {
            format!("{} ({})", id, title)
        } else {
            id
        }
    } else {
        record.to_string()
    }
}

/// Extract the primary key value from a record.
fn extract_key(record: &Value, key_columns: &[&str]) -> Value {
    if key_columns.len() == 1 {
        record.get(key_columns[0]).cloned().unwrap_or(Value::Null)
    } else {
        // Composite key - return as array
        Value::Array(
            key_columns
                .iter()
                .map(|col| record.get(*col).cloned().unwrap_or(Value::Null))
                .collect(),
        )
    }
}

/// Create a string key for hash map lookups.
fn key_to_string(key: &Value) -> String {
    match key {
        Value::Array(arr) => arr
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("|"),
        _ => key.to_string(),
    }
}

/// Compare two values, ignoring floating point precision issues.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => {
            // Compare numbers with tolerance for floats
            if let (Some(fa), Some(fb)) = (na.as_f64(), nb.as_f64()) {
                (fa - fb).abs() < 1e-10
            } else {
                na == nb
            }
        }
        _ => a == b,
    }
}

/// Compare two records and return field differences.
fn diff_records(source: &Value, target: &Value, key_columns: &[&str]) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    let source_obj = source.as_object();
    let target_obj = target.as_object();

    if let (Some(src), Some(tgt)) = (source_obj, target_obj) {
        // Get all field names from both records
        let mut all_fields: HashSet<&str> = src.keys().map(|s| s.as_str()).collect();
        all_fields.extend(tgt.keys().map(|s| s.as_str()));

        for field in all_fields {
            // Skip primary key columns
            if key_columns.contains(&field) {
                continue;
            }

            let src_val = src.get(field).unwrap_or(&Value::Null);
            let tgt_val = tgt.get(field).unwrap_or(&Value::Null);

            if !values_equal(src_val, tgt_val) {
                changes.push(FieldChange {
                    field: field.to_string(),
                    old_value: src_val.clone(),
                    new_value: tgt_val.clone(),
                });
            }
        }
    }

    changes
}

/// Diff a single table's data.
fn diff_table(source_rows: &[Value], target_rows: &[Value], key_columns: &[&str]) -> TableDiff {
    // Build lookup maps by key
    let source_by_key: BTreeMap<String, &Value> = source_rows
        .iter()
        .map(|row| (key_to_string(&extract_key(row, key_columns)), row))
        .collect();

    let target_by_key: BTreeMap<String, &Value> = target_rows
        .iter()
        .map(|row| (key_to_string(&extract_key(row, key_columns)), row))
        .collect();

    let mut diff = TableDiff::default();

    // Find added records (in target but not in source)
    for (key, row) in &target_by_key {
        if !source_by_key.contains_key(key) {
            diff.added.push((*row).clone());
        }
    }

    // Find removed records (in source but not in target)
    for (key, row) in &source_by_key {
        if !target_by_key.contains_key(key) {
            diff.removed.push((*row).clone());
        }
    }

    // Find modified records (present in both but different)
    for (key, source_row) in &source_by_key {
        if let Some(target_row) = target_by_key.get(key) {
            let changes = diff_records(source_row, target_row, key_columns);
            if !changes.is_empty() {
                diff.modified.push(ModifiedRecord {
                    key: extract_key(source_row, key_columns),
                    changes,
                });
            }
        }
    }

    diff
}

/// Compare a snapshot against the current database state.
///
/// Returns a diff where:
/// - "added" = records in DB but not in snapshot
/// - "removed" = records in snapshot but not in DB
/// - "modified" = records with same key but different values
pub fn diff_snapshot_vs_database(snapshot: &Snapshot, db: &Database) -> Result<SnapshotDiff> {
    let mut result = SnapshotDiff {
        source_label: "snapshot".to_string(),
        target_label: "database".to_string(),
        tables: BTreeMap::new(),
    };

    // Get tables to compare
    let tables: Vec<&str> = EXPORTED_TABLES
        .iter()
        .filter(|t| snapshot.tables.contains_key(**t))
        .copied()
        .collect();

    let empty_vec: Vec<Value> = Vec::new();
    for table_name in tables {
        let key_columns = get_table_primary_key(table_name);
        let snapshot_rows = snapshot.get_table(table_name).unwrap_or(&empty_vec);

        // Query database for current state
        let db_rows = query_table_as_json(db, table_name)?;

        let table_diff = diff_table(snapshot_rows, &db_rows, key_columns);

        if !table_diff.is_empty() {
            result.tables.insert(table_name.to_string(), table_diff);
        }
    }

    Ok(result)
}

/// Compare two snapshots.
///
/// Returns a diff where:
/// - "added" = records in target but not in source
/// - "removed" = records in source but not in target
/// - "modified" = records with same key but different values
pub fn diff_snapshots(source: &Snapshot, target: &Snapshot) -> SnapshotDiff {
    let mut result = SnapshotDiff {
        source_label: "source".to_string(),
        target_label: "target".to_string(),
        tables: BTreeMap::new(),
    };

    // Get all tables present in either snapshot
    let mut all_tables: HashSet<&str> = source.tables.keys().map(|s| s.as_str()).collect();
    all_tables.extend(target.tables.keys().map(|s| s.as_str()));

    for table_name in all_tables {
        let key_columns = get_table_primary_key(table_name);
        let source_rows = source
            .get_table(table_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let target_rows = target
            .get_table(table_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let table_diff = diff_table(source_rows, target_rows, key_columns);

        if !table_diff.is_empty() {
            result.tables.insert(table_name.to_string(), table_diff);
        }
    }

    result
}

/// Query a table and return rows as JSON values.
///
/// This is a generic query that returns all columns as a JSON object per row.
fn query_table_as_json(db: &Database, table_name: &str) -> Result<Vec<Value>> {
    use super::get_table_ordering;

    let ordering = get_table_ordering(table_name);
    let query = format!("SELECT * FROM {} {}", table_name, ordering);

    db.with_conn(|conn| {
        let mut stmt = conn.prepare(&query)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows: Vec<Value> = stmt
            .query_map([], |row| {
                let mut obj = serde_json::Map::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let value = row_value_to_json(row, i)?;
                    obj.insert(col_name.clone(), value);
                }
                Ok(Value::Object(obj))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    })
}

/// Convert a SQLite row value to JSON.
fn row_value_to_json(row: &rusqlite::Row, idx: usize) -> rusqlite::Result<Value> {
    use rusqlite::types::ValueRef;

    match row.get_ref(idx)? {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(i) => Ok(Value::Number(i.into())),
        ValueRef::Real(f) => Ok(serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null)),
        ValueRef::Text(s) => {
            let text = String::from_utf8_lossy(s).to_string();
            Ok(Value::String(text))
        }
        ValueRef::Blob(b) => {
            // Encode blob as base64
            use base64::{Engine, engine::general_purpose::STANDARD};
            Ok(Value::String(STANDARD.encode(b)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_key_single() {
        let record = json!({"id": "task-1", "title": "Test"});
        let key = extract_key(&record, &["id"]);
        assert_eq!(key, json!("task-1"));
    }

    #[test]
    fn test_extract_key_composite() {
        let record = json!({
            "from_task_id": "a",
            "to_task_id": "b",
            "dep_type": "blocks"
        });
        let key = extract_key(&record, &["from_task_id", "to_task_id", "dep_type"]);
        assert_eq!(key, json!(["a", "b", "blocks"]));
    }

    #[test]
    fn test_diff_records() {
        let source = json!({
            "id": "task-1",
            "title": "Old Title",
            "status": "pending"
        });
        let target = json!({
            "id": "task-1",
            "title": "New Title",
            "status": "pending"
        });

        let changes = diff_records(&source, &target, &["id"]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "title");
        assert_eq!(changes[0].old_value, json!("Old Title"));
        assert_eq!(changes[0].new_value, json!("New Title"));
    }

    #[test]
    fn test_diff_table() {
        let source = vec![
            json!({"id": "1", "title": "Keep"}),
            json!({"id": "2", "title": "Remove"}),
            json!({"id": "3", "title": "Old"}),
        ];
        let target = vec![
            json!({"id": "1", "title": "Keep"}),
            json!({"id": "3", "title": "New"}),
            json!({"id": "4", "title": "Added"}),
        ];

        let diff = diff_table(&source, &target, &["id"]);

        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.modified.len(), 1);

        assert_eq!(diff.added[0]["id"], json!("4"));
        assert_eq!(diff.removed[0]["id"], json!("2"));
        assert_eq!(diff.modified[0].key, json!("3"));
    }

    #[test]
    fn test_diff_snapshots() {
        let mut source = Snapshot::new();
        source.tables.insert(
            "tasks".to_string(),
            vec![
                json!({"id": "1", "title": "Task 1"}),
                json!({"id": "2", "title": "Task 2"}),
            ],
        );

        let mut target = Snapshot::new();
        target.tables.insert(
            "tasks".to_string(),
            vec![
                json!({"id": "1", "title": "Task 1 Updated"}),
                json!({"id": "3", "title": "Task 3"}),
            ],
        );

        let diff = diff_snapshots(&source, &target);

        assert!(!diff.is_empty());
        let tasks_diff = diff.tables.get("tasks").unwrap();
        assert_eq!(tasks_diff.added.len(), 1);
        assert_eq!(tasks_diff.removed.len(), 1);
        assert_eq!(tasks_diff.modified.len(), 1);
    }

    #[test]
    fn test_values_equal() {
        assert!(values_equal(&json!(1), &json!(1)));
        assert!(values_equal(&json!(1.0), &json!(1.0)));
        assert!(values_equal(&json!("a"), &json!("a")));
        assert!(!values_equal(&json!(1), &json!(2)));
        assert!(!values_equal(&json!("a"), &json!("b")));
    }

    #[test]
    fn test_snapshot_diff_display() {
        let mut diff = SnapshotDiff {
            source_label: "old.json".to_string(),
            target_label: "new.json".to_string(),
            tables: BTreeMap::new(),
        };

        diff.tables.insert(
            "tasks".to_string(),
            TableDiff {
                added: vec![json!({"id": "new-task", "title": "New Task"})],
                removed: vec![],
                modified: vec![],
            },
        );

        let output = format!("{}", diff);
        assert!(output.contains("old.json -> new.json"));
        assert!(output.contains("Table: tasks"));
        assert!(output.contains("Added (1)"));
    }
}
