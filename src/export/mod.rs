//! Export/Import module for task-graph databases.
//!
//! This module provides structured export functionality enabling:
//! - Version control of project task data
//! - Database reconstruction from exports
//! - Migration between schema versions
//! - Human-readable diffs in git

pub mod diff;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Schema version of the current database.
/// This should be updated when the database schema changes.
pub const CURRENT_SCHEMA_VERSION: i32 = 3;

/// Export format version (semver).
pub const EXPORT_VERSION: &str = "1.0.0";

/// Tables that are exported (project data).
pub const EXPORTED_TABLES: &[&str] = &[
    "tasks",
    "dependencies",
    "attachments",
    "task_tags",
    "task_needed_tags",
    "task_wanted_tags",
    "task_sequence",
];

/// Tables excluded from export (ephemeral/runtime).
pub const EXCLUDED_TABLES: &[&str] = &[
    "workers",
    "file_locks",
    "claim_sequence",
    // FTS virtual tables are also excluded (they end with _fts*)
];

/// A structured export snapshot of the task-graph database.
/// 
/// This is a flexible format that can load exports created by either
/// the Database::export_tables (strongly-typed) or any JSON conforming
/// to the export spec. The `tables` field uses generic JSON values
/// to support comparison across schema versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Database schema version (from task-graph internals)
    /// Uses i32 for compatibility with existing exports.
    pub schema_version: i32,

    /// Export format version (semver)
    pub export_version: String,

    /// ISO 8601 timestamp of export
    pub exported_at: String,

    /// Tool name and version that created this export
    pub exported_by: String,

    /// Table data, keyed by table name.
    /// Each table is an array of row objects with column names as keys.
    pub tables: BTreeMap<String, Vec<Value>>,
}

impl Snapshot {
    /// Create a new empty snapshot with current metadata.
    pub fn new() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            export_version: EXPORT_VERSION.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            exported_by: format!("task-graph-mcp v{}", env!("CARGO_PKG_VERSION")),
            tables: BTreeMap::new(),
        }
    }

    /// Load a snapshot from JSON data.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Load a snapshot from a file (supports both plain JSON and gzip).
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        use std::fs::File;
        use std::io::{BufReader, Read};

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Check for gzip magic bytes
        let mut magic = [0u8; 2];
        reader.read_exact(&mut magic)?;

        // Reset to start
        drop(reader);
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        if magic == [0x1f, 0x8b] {
            // Gzip compressed
            let decoder = flate2::read::GzDecoder::new(reader);
            let snapshot: Snapshot = serde_json::from_reader(decoder)?;
            Ok(snapshot)
        } else {
            // Plain JSON
            let snapshot: Snapshot = serde_json::from_reader(reader)?;
            Ok(snapshot)
        }
    }

    /// Serialize to JSON with pretty formatting.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Get rows for a specific table.
    pub fn get_table(&self, name: &str) -> Option<&Vec<Value>> {
        self.tables.get(name)
    }

    /// Check if this snapshot's schema is compatible with the current version.
    pub fn is_schema_compatible(&self) -> bool {
        self.schema_version == CURRENT_SCHEMA_VERSION
    }

    /// Get the list of tables present in this snapshot.
    pub fn table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Row ordering specifications for each exported table.
/// These ensure deterministic ordering for git diffs.
pub fn get_table_ordering(table: &str) -> &'static str {
    match table {
        "tasks" => "ORDER BY id",
        "dependencies" => "ORDER BY from_task_id, to_task_id, dep_type",
        "attachments" => "ORDER BY task_id, order_index",
        "task_tags" => "ORDER BY task_id, tag",
        "task_needed_tags" => "ORDER BY task_id, tag",
        "task_wanted_tags" => "ORDER BY task_id, tag",
        "task_sequence" => "ORDER BY task_id, id",
        _ => "ORDER BY rowid",
    }
}

/// Get the primary key column(s) for a table.
/// Used for identifying records during diff operations.
pub fn get_table_primary_key(table: &str) -> &'static [&'static str] {
    match table {
        "tasks" => &["id"],
        "dependencies" => &["from_task_id", "to_task_id", "dep_type"],
        "attachments" => &["task_id", "order_index"],
        "task_tags" => &["task_id", "tag"],
        "task_needed_tags" => &["task_id", "tag"],
        "task_wanted_tags" => &["task_id", "tag"],
        "task_sequence" => &["id"],
        _ => &["rowid"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_new() {
        let snapshot = Snapshot::new();
        assert_eq!(snapshot.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(snapshot.export_version, EXPORT_VERSION);
        assert!(snapshot.tables.is_empty());
    }

    #[test]
    fn test_snapshot_json_roundtrip() {
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![serde_json::json!({
                "id": "test-1",
                "title": "Test Task"
            })],
        );

        let json = snapshot.to_json_pretty().unwrap();
        let loaded = Snapshot::from_json(&json).unwrap();

        assert_eq!(loaded.schema_version, snapshot.schema_version);
        assert_eq!(loaded.tables.len(), 1);
    }

    #[test]
    fn test_table_ordering() {
        assert_eq!(get_table_ordering("tasks"), "ORDER BY id");
        assert_eq!(
            get_table_ordering("dependencies"),
            "ORDER BY from_task_id, to_task_id, dep_type"
        );
    }

    #[test]
    fn test_table_primary_key() {
        assert_eq!(get_table_primary_key("tasks"), &["id"]);
        assert_eq!(
            get_table_primary_key("dependencies"),
            &["from_task_id", "to_task_id", "dep_type"]
        );
        assert_eq!(get_table_primary_key("attachments"), &["task_id", "order_index"]);
    }
}
