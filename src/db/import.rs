//! Import functionality for the task-graph database.
//!
//! Provides methods to import data from a JSON snapshot into the database.
//! Supports multiple import modes:
//! - Fresh: Import into an empty database (fails if data exists)
//! - Replace: Clear existing project data and import (default with --force)
//! - Merge: Add missing items, skip or overwrite existing (future)
//!
//! Handles foreign key constraints by:
//! - Deleting tables in reverse order (children first) when clearing
//! - Inserting tables in forward order (parents first) when importing
//!
//! Rebuilds FTS indexes after import.

use crate::config::IdsConfig;
use crate::export::{CURRENT_SCHEMA_VERSION, Snapshot};
use anyhow::{Context, Result, anyhow};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;

use super::Database;

/// Import mode determining how to handle existing data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportMode {
    /// Import into an empty database. Fails if any project data exists.
    #[default]
    Fresh,
    /// Clear all existing project data before importing.
    /// Preserves runtime tables (workers, file_locks).
    Replace,
    /// Merge: Add missing items, skip existing.
    /// - Tasks: skip if ID exists, insert if new
    /// - Dependencies: skip if exact match exists
    /// - Attachments: append (keeps both) or replace by name
    /// - Tags: union existing and imported
    /// - State sequence: skip (preserves existing history)
    Merge,
}

/// Result of a dry-run import preview.
/// Shows what would happen without making any changes.
#[derive(Debug, Clone)]
pub struct DryRunResult {
    /// Import mode that would be used.
    pub mode: ImportMode,
    /// Whether the database is empty (relevant for Fresh mode).
    pub database_is_empty: bool,
    /// Number of existing rows per table (before import).
    pub existing_rows: std::collections::BTreeMap<String, usize>,
    /// Number of rows that would be deleted per table (Replace mode).
    pub would_delete: std::collections::BTreeMap<String, usize>,
    /// Number of rows that would be inserted per table.
    pub would_insert: std::collections::BTreeMap<String, usize>,
    /// Number of rows that would be skipped per table (Merge mode).
    pub would_skip: std::collections::BTreeMap<String, usize>,
    /// Whether the import would succeed with the given mode.
    pub would_succeed: bool,
    /// Reason for failure if would_succeed is false.
    pub failure_reason: Option<String>,
    /// Any warnings that would be generated.
    pub warnings: Vec<String>,
}

impl DryRunResult {
    /// Create a new empty dry-run result.
    fn new(mode: ImportMode) -> Self {
        Self {
            mode,
            database_is_empty: true,
            existing_rows: std::collections::BTreeMap::new(),
            would_delete: std::collections::BTreeMap::new(),
            would_insert: std::collections::BTreeMap::new(),
            would_skip: std::collections::BTreeMap::new(),
            would_succeed: true,
            failure_reason: None,
            warnings: Vec::new(),
        }
    }

    /// Total number of rows that would be deleted.
    pub fn total_would_delete(&self) -> usize {
        self.would_delete.values().sum()
    }

    /// Total number of rows that would be inserted.
    pub fn total_would_insert(&self) -> usize {
        self.would_insert.values().sum()
    }

    /// Total number of rows that would be skipped.
    pub fn total_would_skip(&self) -> usize {
        self.would_skip.values().sum()
    }

    /// Total existing rows in the database.
    pub fn total_existing(&self) -> usize {
        self.existing_rows.values().sum()
    }
}

/// Result of an import operation.
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Number of rows imported per table.
    pub rows_imported: std::collections::BTreeMap<String, usize>,
    /// Number of rows deleted per table (for replace mode).
    pub rows_deleted: std::collections::BTreeMap<String, usize>,
    /// Number of rows skipped per table (for merge mode).
    pub rows_skipped: std::collections::BTreeMap<String, usize>,
    /// Whether FTS indexes were rebuilt.
    pub fts_rebuilt: bool,
    /// Any warnings encountered during import.
    pub warnings: Vec<String>,
    /// ID remapping table (old_id -> new_id), populated when remap_ids is used.
    pub id_remap: Option<HashMap<String, String>>,
    /// Root task IDs that were attached to a parent (when parent_id option was used).
    pub parent_linked_roots: Vec<String>,
}

impl ImportResult {
    /// Create a new empty import result.
    fn new() -> Self {
        Self {
            rows_imported: std::collections::BTreeMap::new(),
            rows_deleted: std::collections::BTreeMap::new(),
            rows_skipped: std::collections::BTreeMap::new(),
            fts_rebuilt: false,
            warnings: Vec::new(),
            id_remap: None,
            parent_linked_roots: Vec::new(),
        }
    }

    /// Total number of rows imported.
    pub fn total_rows(&self) -> usize {
        self.rows_imported.values().sum()
    }

    /// Total number of rows deleted.
    pub fn total_deleted(&self) -> usize {
        self.rows_deleted.values().sum()
    }

    /// Total number of rows skipped (merge mode).
    pub fn total_skipped(&self) -> usize {
        self.rows_skipped.values().sum()
    }
}

/// Options for controlling import behavior.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Import mode (Fresh or Replace).
    pub mode: ImportMode,
    /// Whether to remap all task IDs to fresh petname IDs.
    /// When enabled, generates new IDs for every task and updates
    /// all references (dependencies, attachments, tags, state history).
    pub remap_ids: bool,
    /// Optional parent task ID. When set, root tasks in the imported
    /// snapshot (those with no incoming "contains" dependency) will be
    /// attached to this parent via "contains" dependencies after import.
    pub parent_id: Option<String>,
}

impl ImportOptions {
    /// Create options for fresh import (empty database required).
    pub fn fresh() -> Self {
        Self {
            mode: ImportMode::Fresh,
            remap_ids: false,
            parent_id: None,
        }
    }

    /// Create options for replace import (clear existing data).
    pub fn replace() -> Self {
        Self {
            mode: ImportMode::Replace,
            remap_ids: false,
            parent_id: None,
        }
    }

    /// Create options for merge import (add missing items, skip existing).
    pub fn merge() -> Self {
        Self {
            mode: ImportMode::Merge,
            remap_ids: false,
            parent_id: None,
        }
    }

    /// Enable ID remapping on this options instance (builder pattern).
    pub fn with_remap_ids(mut self) -> Self {
        self.remap_ids = true;
        self
    }

    /// Set parent task ID for attaching imported roots (builder pattern).
    pub fn with_parent(mut self, parent_id: String) -> Self {
        self.parent_id = Some(parent_id);
        self
    }
}

/// Generate a fresh petname ID for use in ID remapping.
/// Uses the same approach as generate_task_id in db/tasks.rs.
fn generate_remap_id(ids_config: &IdsConfig) -> String {
    use petname::{Generator, Petnames};

    let words = ids_config.task_id_words;
    let case = ids_config.id_case;

    let base = Petnames::medium()
        .generate_one(words, "-")
        .unwrap_or_else(|| format!("task-{}", chrono::Utc::now().timestamp_millis()));

    case.convert(&base)
}

/// Remap all task IDs in a snapshot, generating fresh petname IDs for each task
/// and updating all references (dependencies, attachments, tags, state history).
///
/// Returns a new snapshot with remapped IDs and the old->new ID mapping table.
///
/// # Arguments
/// * `snapshot` - The original snapshot to remap
/// * `ids_config` - ID generation configuration (word count, case style)
///
/// # Returns
/// * `(Snapshot, HashMap<String, String>)` - The remapped snapshot and the old->new mapping
pub fn remap_snapshot(
    snapshot: &Snapshot,
    ids_config: &IdsConfig,
) -> Result<(Snapshot, HashMap<String, String>)> {
    let mut remapped = snapshot.clone();
    let mut id_map: HashMap<String, String> = HashMap::new();

    // Phase 1: Build the old->new ID mapping from the tasks table.
    // Generate a unique new ID for each task. If a collision occurs
    // (extremely unlikely with petnames), retry.
    if let Some(tasks) = snapshot.tables.get("tasks") {
        let mut used_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for task_row in tasks {
            if let Some(old_id) = task_row.get("id").and_then(|v| v.as_str()) {
                let mut new_id = generate_remap_id(ids_config);
                // Ensure uniqueness (retry on collision)
                let mut attempts = 0;
                while used_ids.contains(&new_id) {
                    new_id = generate_remap_id(ids_config);
                    attempts += 1;
                    if attempts > 100 {
                        return Err(anyhow!(
                            "Failed to generate unique ID after 100 attempts. \
                             Consider increasing ids.task_id_words in config."
                        ));
                    }
                }
                used_ids.insert(new_id.clone());
                id_map.insert(old_id.to_string(), new_id);
            }
        }
    }

    // Helper closure: remap an ID field in a JSON object, returning the object unchanged
    // if the old ID is not in the map (external reference).
    let remap_field = |obj: &mut serde_json::Map<String, Value>, field: &str| {
        if let Some(val) = obj.get(field) {
            if let Some(old_id) = val.as_str() {
                if let Some(new_id) = id_map.get(old_id) {
                    obj.insert(field.to_string(), Value::String(new_id.clone()));
                }
                // If not in map, it's an external reference -- leave it unchanged
            }
        }
    };

    // Phase 2: Remap IDs in all tables.

    // tasks: remap "id" field
    if let Some(tasks) = remapped.tables.get_mut("tasks") {
        for task_row in tasks.iter_mut() {
            if let Some(obj) = task_row.as_object_mut() {
                remap_field(obj, "id");
            }
        }
    }

    // dependencies: remap "from_task_id" and "to_task_id"
    if let Some(deps) = remapped.tables.get_mut("dependencies") {
        for dep_row in deps.iter_mut() {
            if let Some(obj) = dep_row.as_object_mut() {
                remap_field(obj, "from_task_id");
                remap_field(obj, "to_task_id");
            }
        }
    }

    // attachments: remap "task_id"
    if let Some(attachments) = remapped.tables.get_mut("attachments") {
        for att_row in attachments.iter_mut() {
            if let Some(obj) = att_row.as_object_mut() {
                remap_field(obj, "task_id");
            }
        }
    }

    // task_tags: remap "task_id"
    if let Some(tags) = remapped.tables.get_mut("task_tags") {
        for tag_row in tags.iter_mut() {
            if let Some(obj) = tag_row.as_object_mut() {
                remap_field(obj, "task_id");
            }
        }
    }

    // task_needed_tags: remap "task_id"
    if let Some(tags) = remapped.tables.get_mut("task_needed_tags") {
        for tag_row in tags.iter_mut() {
            if let Some(obj) = tag_row.as_object_mut() {
                remap_field(obj, "task_id");
            }
        }
    }

    // task_wanted_tags: remap "task_id"
    if let Some(tags) = remapped.tables.get_mut("task_wanted_tags") {
        for tag_row in tags.iter_mut() {
            if let Some(obj) = tag_row.as_object_mut() {
                remap_field(obj, "task_id");
            }
        }
    }

    // task_sequence: remap "task_id"
    if let Some(events) = remapped.tables.get_mut("task_sequence") {
        for event_row in events.iter_mut() {
            if let Some(obj) = event_row.as_object_mut() {
                remap_field(obj, "task_id");
            }
        }
    }

    Ok((remapped, id_map))
}

/// Extract root task IDs from a snapshot.
///
/// Root tasks are those whose IDs do NOT appear as the `to_task_id` of any
/// dependency with `dep_type = "contains"` within the snapshot. These are the
/// top-level tasks that have no parent in the imported tree.
///
/// # Arguments
/// * `snapshot` - The snapshot to analyze
///
/// # Returns
/// A vector of task IDs that are root tasks in the snapshot.
pub fn snapshot_root_task_ids(snapshot: &Snapshot) -> Vec<String> {
    use std::collections::HashSet;

    // Collect all task IDs from the snapshot
    let all_task_ids: HashSet<String> = snapshot
        .tables
        .get("tasks")
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|row| row.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Collect IDs that are children (targets of "contains" dependencies)
    let child_ids: HashSet<String> = snapshot
        .tables
        .get("dependencies")
        .map(|deps| {
            deps.iter()
                .filter_map(|row| {
                    let dep_type = row.get("dep_type").and_then(|v| v.as_str())?;
                    if dep_type == "contains" {
                        row.get("to_task_id")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Root tasks = all tasks minus children
    all_task_ids
        .into_iter()
        .filter(|id| !child_ids.contains(id))
        .collect()
}

/// Tables in the order they should be imported (respecting foreign key constraints).
/// Tasks must be imported first since other tables reference it.
const IMPORT_ORDER: &[&str] = &[
    "tasks",
    "dependencies",
    "attachments",
    "task_tags",
    "task_needed_tags",
    "task_wanted_tags",
    "task_sequence",
];

impl Database {
    /// Import data from a snapshot into the database.
    ///
    /// This function:
    /// 1. Validates schema version compatibility
    /// 2. Based on mode:
    ///    - Fresh: Validates the database is empty
    ///    - Replace: Clears existing project data (preserves runtime tables)
    ///    - Merge: Keeps existing data, adds only new items
    /// 3. Inserts all rows in the correct order (respecting foreign keys)
    /// 4. Rebuilds FTS indexes
    ///
    /// # Arguments
    /// * `snapshot` - The snapshot to import
    /// * `options` - Import options
    ///
    /// # Returns
    /// * `Ok(ImportResult)` - Import statistics
    /// * `Err` - If import fails
    pub fn import_snapshot(
        &self,
        snapshot: &Snapshot,
        options: &ImportOptions,
    ) -> Result<ImportResult> {
        // Validate schema version
        if snapshot.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(anyhow!(
                "Schema version mismatch: snapshot is v{}, database is v{}. Migration required.",
                snapshot.schema_version,
                CURRENT_SCHEMA_VERSION
            ));
        }

        let mut result = ImportResult::new();

        // Handle mode-specific pre-import actions
        match options.mode {
            ImportMode::Fresh => {
                // Validate database is empty
                self.validate_empty_database()?;
            }
            ImportMode::Replace => {
                // Clear existing project data
                result.rows_deleted = self.clear_project_data()?;
            }
            ImportMode::Merge => {
                // No pre-import action needed for merge mode
                // Existing data is kept, new data is added selectively
            }
        }

        // Import tables in order
        self.with_conn_mut(|conn| {
            // Disable foreign key checks during import for performance
            // (we're importing in the correct order anyway)
            conn.execute("PRAGMA foreign_keys = OFF", [])?;

            // Use a transaction for atomicity
            let tx = conn.transaction()?;

            for table_name in IMPORT_ORDER {
                if let Some(rows) = snapshot.tables.get(*table_name) {
                    let (imported, skipped) = if options.mode == ImportMode::Merge {
                        merge_table(&tx, table_name, rows)?
                    } else {
                        let count = import_table(&tx, table_name, rows)?;
                        (count, 0)
                    };
                    result
                        .rows_imported
                        .insert(table_name.to_string(), imported);
                    if skipped > 0 {
                        result.rows_skipped.insert(table_name.to_string(), skipped);
                    }
                }
            }

            tx.commit()?;

            // Re-enable foreign keys
            conn.execute("PRAGMA foreign_keys = ON", [])?;

            Ok(())
        })?;

        // Rebuild FTS indexes
        self.rebuild_fts_indexes()?;
        result.fts_rebuilt = true;

        // If a parent task ID is specified, attach root tasks from the snapshot
        // under the parent with "contains" dependencies.
        if let Some(ref parent_id) = options.parent_id {
            // Verify parent task exists in the database
            if !self.task_exists(parent_id)? {
                return Err(anyhow!(
                    "Parent task '{}' not found in database. Cannot attach imported roots.",
                    parent_id
                ));
            }

            let root_ids = snapshot_root_task_ids(snapshot);
            if !root_ids.is_empty() {
                self.with_conn(|conn| {
                    for root_id in &root_ids {
                        conn.execute(
                            "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type) VALUES (?1, ?2, ?3)",
                            params![parent_id, root_id, "contains"],
                        )?;
                    }
                    Ok(())
                })?;
                result.parent_linked_roots = root_ids;
            }
        }

        Ok(result)
    }

    /// Preview what an import would do without making any changes.
    ///
    /// This is the dry-run mode: it analyzes the snapshot against the current
    /// database state and reports what would be inserted, deleted, or skipped.
    ///
    /// # Arguments
    /// * `snapshot` - The snapshot to preview importing
    /// * `options` - Import options (determines mode)
    ///
    /// # Returns
    /// * `DryRunResult` - Preview of what would happen
    pub fn preview_import(&self, snapshot: &Snapshot, options: &ImportOptions) -> DryRunResult {
        let mut result = DryRunResult::new(options.mode);

        // Check schema compatibility
        if snapshot.schema_version != CURRENT_SCHEMA_VERSION {
            result.would_succeed = false;
            result.failure_reason = Some(format!(
                "Schema version mismatch: snapshot is v{}, database is v{}. Migration required.",
                snapshot.schema_version, CURRENT_SCHEMA_VERSION
            ));
            return result;
        }

        // Get current row counts for all tables
        let existing = self.get_table_row_counts();
        if let Err(e) = existing {
            result.would_succeed = false;
            result.failure_reason = Some(format!("Failed to query database: {}", e));
            return result;
        }
        let existing = existing.unwrap();
        result.existing_rows = existing.clone();
        result.database_is_empty = existing.values().all(|&count| count == 0);

        // Check mode-specific conditions
        match options.mode {
            ImportMode::Fresh => {
                if !result.database_is_empty {
                    result.would_succeed = false;
                    let non_empty: Vec<_> = existing
                        .iter()
                        .filter(|&(_, count)| *count > 0)
                        .map(|(table, count)| format!("{}: {} rows", table, count))
                        .collect();
                    result.failure_reason = Some(format!(
                        "Database is not empty. Use --force to overwrite or --merge to add. Non-empty tables: {}",
                        non_empty.join(", ")
                    ));
                    return result;
                }
                // In fresh mode, all rows from snapshot would be inserted
                for table_name in IMPORT_ORDER {
                    let count = snapshot.tables.get(*table_name).map_or(0, |v| v.len());
                    result.would_insert.insert(table_name.to_string(), count);
                }
            }
            ImportMode::Replace => {
                // All existing rows would be deleted
                for (table, count) in &existing {
                    if *count > 0 {
                        result.would_delete.insert(table.clone(), *count);
                    }
                }
                // All rows from snapshot would be inserted
                for table_name in IMPORT_ORDER {
                    let count = snapshot.tables.get(*table_name).map_or(0, |v| v.len());
                    result.would_insert.insert(table_name.to_string(), count);
                }
            }
            ImportMode::Merge => {
                // Need to analyze each table to see what would be inserted vs skipped
                if let Err(e) = self.preview_merge(snapshot, &mut result) {
                    result.would_succeed = false;
                    result.failure_reason = Some(format!("Failed to analyze merge: {}", e));
                    return result;
                }
            }
        }

        result
    }

    /// Preview what a merge import would do.
    fn preview_merge(&self, snapshot: &Snapshot, result: &mut DryRunResult) -> Result<()> {
        self.with_conn(|conn| {
            for table_name in IMPORT_ORDER {
                if let Some(rows) = snapshot.tables.get(*table_name) {
                    let (would_insert, would_skip) = preview_merge_table(conn, table_name, rows)?;
                    result
                        .would_insert
                        .insert(table_name.to_string(), would_insert);
                    if would_skip > 0 {
                        result.would_skip.insert(table_name.to_string(), would_skip);
                    }
                } else {
                    result.would_insert.insert(table_name.to_string(), 0);
                }
            }
            Ok(())
        })
    }

    /// Get the row count for each project data table.
    fn get_table_row_counts(&self) -> Result<std::collections::BTreeMap<String, usize>> {
        self.with_conn(|conn| {
            let mut counts = std::collections::BTreeMap::new();
            for table in IMPORT_ORDER {
                let count: i64 =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                        row.get(0)
                    })?;
                counts.insert(table.to_string(), count as usize);
            }
            Ok(counts)
        })
    }

    /// Validate that the database is empty (no project data).
    fn validate_empty_database(&self) -> Result<()> {
        self.with_conn(|conn| {
            for table in IMPORT_ORDER {
                let count: i64 = conn.query_row(
                    &format!("SELECT COUNT(*) FROM {}", table),
                    [],
                    |row| row.get(0),
                )?;
                if count > 0 {
                    return Err(anyhow!(
                        "Database is not empty: table '{}' contains {} rows. Use --force to overwrite.",
                        table,
                        count
                    ));
                }
            }
            Ok(())
        })
    }

    /// Clear all project data tables, preserving runtime tables.
    ///
    /// Tables are deleted in reverse order to respect foreign key constraints
    /// (children deleted before parents).
    ///
    /// Runtime tables preserved:
    /// - workers: Session-based worker registrations
    /// - file_locks: Active file marks (advisory locks)
    /// - claim_sequence: File lock audit log
    ///
    /// # Returns
    /// A map of table names to number of rows deleted.
    pub fn clear_project_data(&self) -> Result<std::collections::BTreeMap<String, usize>> {
        let mut deleted = std::collections::BTreeMap::new();

        self.with_conn_mut(|conn| {
            // Disable foreign key checks during deletion for performance
            conn.execute("PRAGMA foreign_keys = OFF", [])?;

            // Use a transaction for atomicity
            let tx = conn.transaction()?;

            // Delete in reverse order to respect foreign key constraints
            // (children first, then parents)
            for table_name in IMPORT_ORDER.iter().rev() {
                let count: i64 =
                    tx.query_row(&format!("SELECT COUNT(*) FROM {}", table_name), [], |row| {
                        row.get(0)
                    })?;

                if count > 0 {
                    tx.execute(&format!("DELETE FROM {}", table_name), [])?;
                    deleted.insert(table_name.to_string(), count as usize);
                }
            }

            // Also clear FTS tables (they'll be rebuilt after import)
            tx.execute("DELETE FROM tasks_fts", [])?;
            tx.execute("DELETE FROM attachments_fts", [])?;

            // Reset auto-increment counter for task_sequence
            // This ensures imported IDs don't conflict with auto-generated ones
            tx.execute(
                "DELETE FROM sqlite_sequence WHERE name = 'task_sequence'",
                [],
            )?;

            tx.commit()?;

            // Re-enable foreign keys
            conn.execute("PRAGMA foreign_keys = ON", [])?;

            Ok(())
        })?;

        Ok(deleted)
    }

    /// Rebuild FTS indexes from the base tables.
    ///
    /// This is called after import to populate the FTS virtual tables
    /// since triggers don't fire during bulk import.
    pub fn rebuild_fts_indexes(&self) -> Result<()> {
        self.with_conn(|conn| {
            // Rebuild tasks_fts
            conn.execute("DELETE FROM tasks_fts", [])?;
            conn.execute(
                "INSERT INTO tasks_fts(task_id, title, description)
                 SELECT id, title, COALESCE(description, '')
                 FROM tasks",
                [],
            )?;

            // Rebuild attachments_fts (only text content)
            conn.execute("DELETE FROM attachments_fts", [])?;
            conn.execute(
                "INSERT INTO attachments_fts(task_id, attachment_type, sequence, name, content)
                 SELECT task_id, attachment_type, sequence, name, content
                 FROM attachments
                 WHERE mime_type LIKE 'text/%'",
                [],
            )?;

            Ok(())
        })
    }
}

/// Import rows into a specific table.
fn import_table(conn: &rusqlite::Connection, table_name: &str, rows: &[Value]) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    match table_name {
        "tasks" => import_tasks(conn, rows),
        "dependencies" => import_dependencies(conn, rows),
        "attachments" => import_attachments(conn, rows),
        "task_tags" => import_task_tags(conn, rows),
        "task_needed_tags" => import_task_needed_tags(conn, rows),
        "task_wanted_tags" => import_task_wanted_tags(conn, rows),
        "task_sequence" => import_task_sequence(conn, rows),
        _ => Err(anyhow!("Unknown table: {}", table_name)),
    }
}

/// Merge rows into a specific table (skip existing, insert new).
/// Returns (imported_count, skipped_count).
fn merge_table(
    conn: &rusqlite::Connection,
    table_name: &str,
    rows: &[Value],
) -> Result<(usize, usize)> {
    if rows.is_empty() {
        return Ok((0, 0));
    }

    match table_name {
        "tasks" => merge_tasks(conn, rows),
        "dependencies" => merge_dependencies(conn, rows),
        "attachments" => merge_attachments(conn, rows),
        "task_tags" => merge_task_tags(conn, rows),
        "task_needed_tags" => merge_task_needed_tags(conn, rows),
        "task_wanted_tags" => merge_task_wanted_tags(conn, rows),
        "task_sequence" => merge_task_sequence(conn, rows),
        _ => Err(anyhow!("Unknown table: {}", table_name)),
    }
}

/// Preview what a merge would do for a specific table (no modifications).
/// Returns (would_insert_count, would_skip_count).
fn preview_merge_table(
    conn: &rusqlite::Connection,
    table_name: &str,
    rows: &[Value],
) -> Result<(usize, usize)> {
    if rows.is_empty() {
        return Ok((0, 0));
    }

    match table_name {
        "tasks" => preview_merge_tasks(conn, rows),
        "dependencies" => preview_merge_dependencies(conn, rows),
        "attachments" => preview_merge_attachments(conn, rows),
        "task_tags" => preview_merge_task_tags(conn, rows),
        "task_needed_tags" => preview_merge_task_needed_tags(conn, rows),
        "task_wanted_tags" => preview_merge_task_wanted_tags(conn, rows),
        "task_sequence" => Ok((0, rows.len())), // Always skip in merge mode
        _ => Err(anyhow!("Unknown table: {}", table_name)),
    }
}

/// Preview merge for tasks - count how many would be inserted vs skipped.
fn preview_merge_tasks(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row.as_object().context("Task row must be an object")?;
        let task_id = get_string(obj, "id")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM tasks WHERE id = ?1",
                params![&task_id],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Preview merge for dependencies - count how many would be inserted vs skipped.
fn preview_merge_dependencies(
    conn: &rusqlite::Connection,
    rows: &[Value],
) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("Dependency row must be an object")?;
        let from_id = get_string(obj, "from_task_id")?;
        let to_id = get_string(obj, "to_task_id")?;
        let dep_type = get_string(obj, "dep_type")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2 AND dep_type = ?3",
                params![&from_id, &to_id, &dep_type],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Preview merge for attachments - count how many would be inserted vs skipped.
fn preview_merge_attachments(
    conn: &rusqlite::Connection,
    rows: &[Value],
) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("Attachment row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let attachment_type = get_string(obj, "attachment_type")?;
        let sequence = get_i32(obj, "sequence")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND sequence = ?3",
                params![&task_id, &attachment_type, sequence],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Preview merge for task_tags - count how many would be inserted vs skipped.
fn preview_merge_task_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row.as_object().context("TaskTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Preview merge for task_needed_tags - count how many would be inserted vs skipped.
fn preview_merge_task_needed_tags(
    conn: &rusqlite::Connection,
    rows: &[Value],
) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("TaskNeededTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_needed_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Preview merge for task_wanted_tags - count how many would be inserted vs skipped.
fn preview_merge_task_wanted_tags(
    conn: &rusqlite::Connection,
    rows: &[Value],
) -> Result<(usize, usize)> {
    let mut would_insert = 0;
    let mut would_skip = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("TaskWantedTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_wanted_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            would_skip += 1;
        } else {
            would_insert += 1;
        }
    }

    Ok((would_insert, would_skip))
}

/// Merge tasks - skip if ID exists, insert if new.
fn merge_tasks(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt = conn.prepare(
        "INSERT INTO tasks (
            id, title, description, status, priority, worker_id, claimed_at,
            needed_tags, wanted_tags, tags,
            points, time_estimate_ms, time_actual_ms, started_at, completed_at,
            current_thought,
            metric_0, metric_1, metric_2, metric_3, metric_4, metric_5, metric_6, metric_7,
            cost_usd,
            deleted_at, deleted_by, deleted_reason,
            created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15,
            ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
            ?25,
            ?26, ?27, ?28,
            ?29, ?30
        )",
    )?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row.as_object().context("Task row must be an object")?;
        let task_id = get_string(obj, "id")?;

        // Check if task already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM tasks WHERE id = ?1",
                params![&task_id],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![
            task_id,
            get_string(obj, "title")?,
            get_opt_string(obj, "description"),
            get_string(obj, "status")?,
            get_string(obj, "priority")?,
            get_opt_string(obj, "worker_id"),
            get_opt_i64(obj, "claimed_at"),
            get_opt_string(obj, "needed_tags"),
            get_opt_string(obj, "wanted_tags"),
            get_opt_string(obj, "tags"),
            get_opt_i32(obj, "points"),
            get_opt_i64(obj, "time_estimate_ms"),
            get_opt_i64(obj, "time_actual_ms"),
            get_opt_i64(obj, "started_at"),
            get_opt_i64(obj, "completed_at"),
            get_opt_string(obj, "current_thought"),
            get_i64_or_default(obj, "metric_0"),
            get_i64_or_default(obj, "metric_1"),
            get_i64_or_default(obj, "metric_2"),
            get_i64_or_default(obj, "metric_3"),
            get_i64_or_default(obj, "metric_4"),
            get_i64_or_default(obj, "metric_5"),
            get_i64_or_default(obj, "metric_6"),
            get_i64_or_default(obj, "metric_7"),
            get_f64_or_default(obj, "cost_usd"),
            get_opt_i64(obj, "deleted_at"),
            get_opt_string(obj, "deleted_by"),
            get_opt_string(obj, "deleted_reason"),
            get_i64(obj, "created_at")?,
            get_i64(obj, "updated_at")?,
        ])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge dependencies - skip if exact match exists.
fn merge_dependencies(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt = conn.prepare(
        "INSERT INTO dependencies (from_task_id, to_task_id, dep_type)
         VALUES (?1, ?2, ?3)",
    )?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("Dependency row must be an object")?;
        let from_id = get_string(obj, "from_task_id")?;
        let to_id = get_string(obj, "to_task_id")?;
        let dep_type = get_string(obj, "dep_type")?;

        // Check if exact dependency already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2 AND dep_type = ?3",
                params![&from_id, &to_id, &dep_type],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![from_id, to_id, dep_type])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge attachments - skip if exact match (task_id + attachment_type + sequence) exists.
fn merge_attachments(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt = conn.prepare(
        "INSERT INTO attachments (task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("Attachment row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let attachment_type = get_string(obj, "attachment_type")?;
        let sequence = get_i32(obj, "sequence")?;

        // Check if attachment already exists (by task_id + attachment_type + sequence)
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND sequence = ?3",
                params![&task_id, &attachment_type, sequence],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![
            task_id,
            attachment_type,
            sequence,
            get_string(obj, "name")?,
            get_string(obj, "mime_type")?,
            get_string(obj, "content")?,
            get_opt_string(obj, "file_path"),
            get_i64(obj, "created_at")?,
        ])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge task_tags - skip if exact match exists.
fn merge_task_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt = conn.prepare("INSERT INTO task_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row.as_object().context("TaskTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        // Check if tag already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![task_id, tag])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge task_needed_tags - skip if exact match exists.
fn merge_task_needed_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt =
        conn.prepare("INSERT INTO task_needed_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("TaskNeededTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        // Check if tag already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_needed_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![task_id, tag])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge task_wanted_tags - skip if exact match exists.
fn merge_task_wanted_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    let mut insert_stmt =
        conn.prepare("INSERT INTO task_wanted_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut imported = 0;
    let mut skipped = 0;

    for row in rows {
        let obj = row
            .as_object()
            .context("TaskWantedTag row must be an object")?;
        let task_id = get_string(obj, "task_id")?;
        let tag = get_string(obj, "tag")?;

        // Check if tag already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM task_wanted_tags WHERE task_id = ?1 AND tag = ?2",
                params![&task_id, &tag],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        insert_stmt.execute(params![task_id, tag])?;
        imported += 1;
    }

    Ok((imported, skipped))
}

/// Merge task_sequence - skip all in merge mode to preserve existing history.
/// State history from the snapshot is not imported to avoid conflicts with existing history.
fn merge_task_sequence(conn: &rusqlite::Connection, rows: &[Value]) -> Result<(usize, usize)> {
    // In merge mode, we skip all state sequence imports to preserve existing history.
    // The rationale is that state history reflects what actually happened in this database,
    // and importing history from another database could create inconsistencies.
    let _ = conn; // silence unused variable warning
    Ok((0, rows.len()))
}

/// Import tasks table.
fn import_tasks(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "INSERT INTO tasks (
            id, title, description, status, priority, worker_id, claimed_at,
            needed_tags, wanted_tags, tags,
            points, time_estimate_ms, time_actual_ms, started_at, completed_at,
            current_thought,
            metric_0, metric_1, metric_2, metric_3, metric_4, metric_5, metric_6, metric_7,
            cost_usd,
            deleted_at, deleted_by, deleted_reason,
            created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15,
            ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
            ?25,
            ?26, ?27, ?28,
            ?29, ?30
        )",
    )?;

    let mut count = 0;
    for row in rows {
        let obj = row.as_object().context("Task row must be an object")?;

        stmt.execute(params![
            get_string(obj, "id")?,
            get_string(obj, "title")?,
            get_opt_string(obj, "description"),
            get_string(obj, "status")?,
            get_string(obj, "priority")?,
            get_opt_string(obj, "worker_id"),
            get_opt_i64(obj, "claimed_at"),
            get_opt_string(obj, "needed_tags"),
            get_opt_string(obj, "wanted_tags"),
            get_opt_string(obj, "tags"),
            get_opt_i32(obj, "points"),
            get_opt_i64(obj, "time_estimate_ms"),
            get_opt_i64(obj, "time_actual_ms"),
            get_opt_i64(obj, "started_at"),
            get_opt_i64(obj, "completed_at"),
            get_opt_string(obj, "current_thought"),
            get_i64_or_default(obj, "metric_0"),
            get_i64_or_default(obj, "metric_1"),
            get_i64_or_default(obj, "metric_2"),
            get_i64_or_default(obj, "metric_3"),
            get_i64_or_default(obj, "metric_4"),
            get_i64_or_default(obj, "metric_5"),
            get_i64_or_default(obj, "metric_6"),
            get_i64_or_default(obj, "metric_7"),
            get_f64_or_default(obj, "cost_usd"),
            get_opt_i64(obj, "deleted_at"),
            get_opt_string(obj, "deleted_by"),
            get_opt_string(obj, "deleted_reason"),
            get_i64(obj, "created_at")?,
            get_i64(obj, "updated_at")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import dependencies table.
fn import_dependencies(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "INSERT INTO dependencies (from_task_id, to_task_id, dep_type)
         VALUES (?1, ?2, ?3)",
    )?;

    let mut count = 0;
    for row in rows {
        let obj = row
            .as_object()
            .context("Dependency row must be an object")?;

        stmt.execute(params![
            get_string(obj, "from_task_id")?,
            get_string(obj, "to_task_id")?,
            get_string(obj, "dep_type")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import attachments table.
fn import_attachments(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "INSERT INTO attachments (task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut count = 0;
    for row in rows {
        let obj = row
            .as_object()
            .context("Attachment row must be an object")?;

        stmt.execute(params![
            get_string(obj, "task_id")?,
            get_string(obj, "attachment_type")?,
            get_i32(obj, "sequence")?,
            get_string(obj, "name")?,
            get_string(obj, "mime_type")?,
            get_string(obj, "content")?,
            get_opt_string(obj, "file_path"),
            get_i64(obj, "created_at")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import task_tags table.
fn import_task_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare("INSERT INTO task_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut count = 0;
    for row in rows {
        let obj = row.as_object().context("TaskTag row must be an object")?;

        stmt.execute(params![
            get_string(obj, "task_id")?,
            get_string(obj, "tag")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import task_needed_tags table.
fn import_task_needed_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare("INSERT INTO task_needed_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut count = 0;
    for row in rows {
        let obj = row
            .as_object()
            .context("TaskNeededTag row must be an object")?;

        stmt.execute(params![
            get_string(obj, "task_id")?,
            get_string(obj, "tag")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import task_wanted_tags table.
fn import_task_wanted_tags(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare("INSERT INTO task_wanted_tags (task_id, tag) VALUES (?1, ?2)")?;

    let mut count = 0;
    for row in rows {
        let obj = row
            .as_object()
            .context("TaskWantedTag row must be an object")?;

        stmt.execute(params![
            get_string(obj, "task_id")?,
            get_string(obj, "tag")?,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Import task_sequence table.
fn import_task_sequence(conn: &rusqlite::Connection, rows: &[Value]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "INSERT INTO task_sequence (id, task_id, worker_id, status, phase, reason, timestamp, end_timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut count = 0;
    for row in rows {
        let obj = row
            .as_object()
            .context("TaskSequenceEvent row must be an object")?;

        stmt.execute(params![
            get_i64(obj, "id")?,
            get_string(obj, "task_id")?,
            get_opt_string(obj, "worker_id"),
            get_opt_string(obj, "status"),
            get_opt_string(obj, "phase"),
            get_opt_string(obj, "reason"),
            get_i64(obj, "timestamp")?,
            get_opt_i64(obj, "end_timestamp"),
        ])?;
        count += 1;
    }

    Ok(count)
}

// ============================================================================
// JSON value extraction helpers
// ============================================================================

/// Get a required string value from a JSON object.
fn get_string(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Missing or invalid string field: {}", key))
}

/// Get an optional string value from a JSON object.
fn get_opt_string(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_str().map(|s| s.to_string())
        }
    })
}

/// Get a required i64 value from a JSON object.
fn get_i64(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i64> {
    obj.get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Missing or invalid i64 field: {}", key))
}

/// Get an optional i64 value from a JSON object.
fn get_opt_i64(obj: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    obj.get(key)
        .and_then(|v| if v.is_null() { None } else { v.as_i64() })
}

/// Get an i64 value with a default of 0.
fn get_i64_or_default(obj: &serde_json::Map<String, Value>, key: &str) -> i64 {
    get_opt_i64(obj, key).unwrap_or(0)
}

/// Get a required i32 value from a JSON object.
fn get_i32(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i32> {
    obj.get(key)
        .and_then(|v| v.as_i64())
        .map(|i| i as i32)
        .ok_or_else(|| anyhow!("Missing or invalid i32 field: {}", key))
}

/// Get an optional i32 value from a JSON object.
#[allow(dead_code)]
fn get_opt_i32(obj: &serde_json::Map<String, Value>, key: &str) -> Option<i32> {
    obj.get(key).and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_i64().map(|i| i as i32)
        }
    })
}

/// Get an f64 value with a default of 0.0.
fn get_f64_or_default(obj: &serde_json::Map<String, Value>, key: &str) -> f64 {
    obj.get(key)
        .and_then(|v| if v.is_null() { None } else { v.as_f64() })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IdsConfig;
    use crate::export::Snapshot;
    use serde_json::json;

    #[test]
    fn test_import_empty_snapshot() {
        let db = Database::open_in_memory().unwrap();
        let snapshot = Snapshot::new();
        let options = ImportOptions::default();

        let result = db.import_snapshot(&snapshot, &options).unwrap();
        assert_eq!(result.total_rows(), 0);
        assert!(result.fts_rebuilt);
    }

    #[test]
    fn test_import_tasks() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "task-1",
                "title": "Test Task",
                "description": "A test task",
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert_eq!(result.rows_imported.get("tasks"), Some(&1));
        assert!(result.fts_rebuilt);

        // Verify FTS was populated
        let results = db.search_tasks("Test", None, 0, false, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "task-1");
    }

    #[test]
    fn test_import_with_dependencies() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        // Add tasks
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "task-a",
                    "title": "Task A",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "task-b",
                    "title": "Task B",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );

        // Add dependency
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![json!({
                "from_task_id": "task-a",
                "to_task_id": "task-b",
                "dep_type": "blocks"
            })],
        );

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert_eq!(result.rows_imported.get("tasks"), Some(&2));
        assert_eq!(result.rows_imported.get("dependencies"), Some(&1));
    }

    #[test]
    fn test_import_fails_on_non_empty_database() {
        let db = Database::open_in_memory().unwrap();

        // Create a task first
        use crate::config::StatesConfig;
        db.create_task(
            None,
            "Existing task".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        let snapshot = Snapshot::new();
        let options = ImportOptions::fresh(); // Explicitly use fresh mode

        let result = db.import_snapshot(&snapshot, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not empty"));
    }

    #[test]
    fn test_import_replace_mode() {
        let db = Database::open_in_memory().unwrap();

        // Create existing task
        use crate::config::StatesConfig;
        let existing_id = db
            .create_task(
                None,
                "Existing task".to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None, // tags
                &StatesConfig::default(),
                &IdsConfig::default(),
            )
            .unwrap();

        // Verify task exists
        let task = db.get_task(&existing_id.id).unwrap();
        assert!(task.is_some());
        assert_eq!(task.unwrap().title, "Existing task");

        // Create snapshot with different task
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "imported-task",
                "title": "Imported Task",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        // Import in replace mode
        let options = ImportOptions::replace();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // Verify old task was deleted and new task imported
        assert_eq!(result.rows_deleted.get("tasks"), Some(&1));
        assert_eq!(result.rows_imported.get("tasks"), Some(&1));

        // Old task should be gone
        let old_task = db.get_task(&existing_id.id).unwrap();
        assert!(old_task.is_none());

        // New task should exist
        let new_task = db.get_task("imported-task").unwrap();
        assert!(new_task.is_some());
        assert_eq!(new_task.unwrap().title, "Imported Task");
    }

    #[test]
    fn test_replace_mode_preserves_workers() {
        let db = Database::open_in_memory().unwrap();

        // Register a worker
        db.register_worker(
            Some("test-worker".to_string()),
            vec!["rust".to_string(), "test".to_string()],
            false,
            &IdsConfig::default(),
            None,
        )
        .unwrap();

        // Verify worker exists
        let workers = db.list_workers().unwrap();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].id, "test-worker");

        // Create a task
        use crate::config::StatesConfig;
        db.create_task(
            None,
            "Task to replace".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Import empty snapshot in replace mode
        let snapshot = Snapshot::new();
        let options = ImportOptions::replace();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // Task should be deleted
        assert_eq!(result.rows_deleted.get("tasks"), Some(&1));

        // Worker should still exist (preserved)
        let workers = db.list_workers().unwrap();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].id, "test-worker");
    }

    #[test]
    fn test_clear_project_data() {
        let db = Database::open_in_memory().unwrap();

        // Create tasks with dependencies and tags
        use crate::config::{DependenciesConfig, StatesConfig};
        let task_a = db
            .create_task(
                None,
                "Task A".to_string(),
                None,
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                Some(vec!["rust".to_string(), "test".to_string()]), // tags
                &StatesConfig::default(),
                &IdsConfig::default(),
            )
            .unwrap();

        let task_b = db
            .create_task(
                None,
                "Task B".to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None, // tags
                &StatesConfig::default(),
                &IdsConfig::default(),
            )
            .unwrap();

        // Add dependency
        db.add_dependency(
            &task_a.id,
            &task_b.id,
            "blocks",
            &DependenciesConfig::default(),
        )
        .unwrap();

        // Clear all project data
        let deleted = db.clear_project_data().unwrap();

        // Verify counts
        assert_eq!(deleted.get("tasks"), Some(&2));
        assert_eq!(deleted.get("dependencies"), Some(&1));
        assert_eq!(deleted.get("task_tags"), Some(&2));

        // Verify tables are empty
        db.with_conn(|conn| {
            for table in IMPORT_ORDER {
                let count: i64 =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(count, 0, "Table {} should be empty", table);
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_import_schema_version_mismatch() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();
        snapshot.schema_version = 999; // Invalid version

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Schema version mismatch")
        );
    }

    #[test]
    fn test_import_with_attachments() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        // Add task
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "task-1",
                "title": "Task with attachment",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        // Add attachment
        snapshot.tables.insert(
            "attachments".to_string(),
            vec![json!({
                "task_id": "task-1",
                "attachment_type": "notes",
                "sequence": 0,
                "name": "",
                "mime_type": "text/plain",
                "content": "Some searchable notes content",
                "file_path": null,
                "created_at": 1700000000000_i64
            })],
        );

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert_eq!(result.rows_imported.get("tasks"), Some(&1));
        assert_eq!(result.rows_imported.get("attachments"), Some(&1));

        // Verify attachment FTS was populated
        let results = db.search_tasks("searchable", None, 0, true, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].attachment_matches.len(), 1);
    }

    #[test]
    fn test_import_with_tags() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        // Add task
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "task-1",
                "title": "Tagged Task",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        // Add tags
        snapshot.tables.insert(
            "task_tags".to_string(),
            vec![
                json!({"task_id": "task-1", "tag": "rust"}),
                json!({"task_id": "task-1", "tag": "backend"}),
            ],
        );

        snapshot.tables.insert(
            "task_needed_tags".to_string(),
            vec![json!({"task_id": "task-1", "tag": "senior"})],
        );

        snapshot.tables.insert(
            "task_wanted_tags".to_string(),
            vec![json!({"task_id": "task-1", "tag": "rust-expert"})],
        );

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert_eq!(result.rows_imported.get("task_tags"), Some(&2));
        assert_eq!(result.rows_imported.get("task_needed_tags"), Some(&1));
        assert_eq!(result.rows_imported.get("task_wanted_tags"), Some(&1));
    }

    #[test]
    fn test_import_task_sequence() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        // Add task
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "task-1",
                "title": "Task with history",
                "description": null,
                "status": "completed",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": 1700000001000_i64,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000001000_i64
            })],
        );

        // Add state history
        snapshot.tables.insert(
            "task_sequence".to_string(),
            vec![
                json!({
                    "id": 1,
                    "task_id": "task-1",
                    "worker_id": null,
                    "event": "pending",
                    "reason": "Task created",
                    "timestamp": 1700000000000_i64,
                    "end_timestamp": 1700000000500_i64
                }),
                json!({
                    "id": 2,
                    "task_id": "task-1",
                    "worker_id": "worker-1",
                    "event": "working",
                    "reason": "Started work",
                    "timestamp": 1700000000500_i64,
                    "end_timestamp": 1700000001000_i64
                }),
                json!({
                    "id": 3,
                    "task_id": "task-1",
                    "worker_id": "worker-1",
                    "event": "completed",
                    "reason": "Done",
                    "timestamp": 1700000001000_i64,
                    "end_timestamp": null
                }),
            ],
        );

        let options = ImportOptions::default();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert_eq!(result.rows_imported.get("task_sequence"), Some(&3));
    }

    #[test]
    fn test_rebuild_fts_indexes() {
        let db = Database::open_in_memory().unwrap();

        // First, insert a task normally (trigger will fire)
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, title, description, status, priority, created_at, updated_at)
                 VALUES ('test-task', 'Manual Insert Test', 'Bypass trigger', 'pending', '5', 1700000000000, 1700000000000)",
                [],
            )?;
            Ok(())
        }).unwrap();

        // FTS should have the task due to triggers
        let results = db.search_tasks("Manual", None, 0, false, None).unwrap();
        assert_eq!(results.len(), 1);

        // Now delete from FTS to simulate a corrupted/empty FTS state
        db.with_conn(|conn| {
            conn.execute("DELETE FROM tasks_fts", [])?;
            Ok(())
        })
        .unwrap();

        // Search should now find nothing
        let results = db.search_tasks("Manual", None, 0, false, None).unwrap();
        assert!(results.is_empty());

        // Rebuild FTS
        db.rebuild_fts_indexes().unwrap();

        // Now search should work again
        let results = db.search_tasks("Manual", None, 0, false, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "test-task");
    }

    #[test]
    fn test_import_mode_default() {
        // Default mode should be Fresh
        let options = ImportOptions::default();
        assert_eq!(options.mode, ImportMode::Fresh);
    }

    #[test]
    fn test_import_result_total_deleted() {
        let mut result = ImportResult::new();
        result.rows_deleted.insert("tasks".to_string(), 5);
        result.rows_deleted.insert("dependencies".to_string(), 3);
        assert_eq!(result.total_deleted(), 8);
    }

    #[test]
    fn test_import_result_total_skipped() {
        let mut result = ImportResult::new();
        result.rows_skipped.insert("tasks".to_string(), 3);
        result.rows_skipped.insert("dependencies".to_string(), 2);
        assert_eq!(result.total_skipped(), 5);
    }

    #[test]
    fn test_merge_mode_skips_existing_tasks() {
        let db = Database::open_in_memory().unwrap();

        // Create existing task with specific ID
        use crate::config::StatesConfig;
        db.create_task(
            Some("existing-task".to_string()),
            "Existing task".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Create snapshot with same ID task and a new task
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "existing-task", // This should be skipped
                    "title": "Should Be Skipped",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "new-task", // This should be imported
                    "title": "New Task",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );

        // Import in merge mode
        let options = ImportOptions::merge();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // 1 imported (new-task), 1 skipped (existing-task)
        assert_eq!(result.rows_imported.get("tasks"), Some(&1));
        assert_eq!(result.rows_skipped.get("tasks"), Some(&1));

        // Existing task should still have original title
        let existing = db.get_task("existing-task").unwrap().unwrap();
        assert_eq!(existing.title, "Existing task");

        // New task should be imported
        let new_task = db.get_task("new-task").unwrap();
        assert!(new_task.is_some());
        assert_eq!(new_task.unwrap().title, "New Task");
    }

    #[test]
    fn test_merge_mode_skips_existing_dependencies() {
        let db = Database::open_in_memory().unwrap();

        // Create tasks and dependency
        use crate::config::{DependenciesConfig, StatesConfig};
        db.create_task(
            Some("task-a".to_string()),
            "Task A".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();
        db.create_task(
            Some("task-b".to_string()),
            "Task B".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();
        db.create_task(
            Some("task-c".to_string()),
            "Task C".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();
        db.add_dependency("task-a", "task-b", "blocks", &DependenciesConfig::default())
            .unwrap();

        // Create snapshot with existing and new dependencies
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![
                json!({
                    "from_task_id": "task-a",
                    "to_task_id": "task-b",
                    "dep_type": "blocks" // Existing - should be skipped
                }),
                json!({
                    "from_task_id": "task-b",
                    "to_task_id": "task-c",
                    "dep_type": "blocks" // New - should be imported
                }),
            ],
        );

        // Import in merge mode
        let options = ImportOptions::merge();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // 1 imported (b->c), 1 skipped (a->b)
        assert_eq!(result.rows_imported.get("dependencies"), Some(&1));
        assert_eq!(result.rows_skipped.get("dependencies"), Some(&1));
    }

    #[test]
    fn test_merge_mode_skips_state_sequence() {
        let db = Database::open_in_memory().unwrap();

        // Create a task (will have initial state history)
        use crate::config::StatesConfig;
        db.create_task(
            Some("task-1".to_string()),
            "Task 1".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Snapshot with state history
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "task_sequence".to_string(),
            vec![json!({
                "id": 999,
                "task_id": "task-1",
                "worker_id": null,
                "event": "pending",
                "reason": "Imported history",
                "timestamp": 1700000000000_i64,
                "end_timestamp": null
            })],
        );

        // Import in merge mode
        let options = ImportOptions::merge();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // State sequence should be all skipped in merge mode
        assert_eq!(result.rows_imported.get("task_sequence"), Some(&0));
        assert_eq!(result.rows_skipped.get("task_sequence"), Some(&1));
    }

    #[test]
    fn test_merge_mode_adds_new_tags() {
        let db = Database::open_in_memory().unwrap();

        // Create task with tags
        use crate::config::StatesConfig;
        db.create_task(
            Some("task-1".to_string()),
            "Task 1".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec!["existing-tag".to_string()]), // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Snapshot with existing and new tags
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "task_tags".to_string(),
            vec![
                json!({"task_id": "task-1", "tag": "existing-tag"}), // Existing - skip
                json!({"task_id": "task-1", "tag": "new-tag"}),      // New - import
            ],
        );

        // Import in merge mode
        let options = ImportOptions::merge();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // 1 imported (new-tag), 1 skipped (existing-tag)
        assert_eq!(result.rows_imported.get("task_tags"), Some(&1));
        assert_eq!(result.rows_skipped.get("task_tags"), Some(&1));
    }

    #[test]
    fn test_import_options_merge() {
        let options = ImportOptions::merge();
        assert_eq!(options.mode, ImportMode::Merge);
    }

    // ============================================================================
    // Dry-run (preview_import) tests
    // ============================================================================

    #[test]
    fn test_preview_fresh_mode_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        // Add a task to the snapshot
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "task-1",
                "title": "Test Task",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        let options = ImportOptions::fresh();
        let preview = db.preview_import(&snapshot, &options);

        assert!(preview.would_succeed);
        assert!(preview.database_is_empty);
        assert_eq!(preview.mode, ImportMode::Fresh);
        assert_eq!(preview.total_would_insert(), 1);
        assert_eq!(preview.total_would_delete(), 0);
        assert_eq!(preview.total_would_skip(), 0);
    }

    #[test]
    fn test_preview_fresh_mode_non_empty_db() {
        let db = Database::open_in_memory().unwrap();

        // Create existing task
        use crate::config::StatesConfig;
        db.create_task(
            None,
            "Existing task".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        let snapshot = Snapshot::new();
        let options = ImportOptions::fresh();
        let preview = db.preview_import(&snapshot, &options);

        // Should fail because database is not empty
        assert!(!preview.would_succeed);
        assert!(!preview.database_is_empty);
        assert!(preview.failure_reason.is_some());
        assert!(preview.failure_reason.unwrap().contains("not empty"));
    }

    #[test]
    fn test_preview_replace_mode() {
        let db = Database::open_in_memory().unwrap();

        // Create existing tasks
        use crate::config::StatesConfig;
        db.create_task(
            Some("existing-1".to_string()),
            "Existing 1".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();
        db.create_task(
            Some("existing-2".to_string()),
            "Existing 2".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Create snapshot with different task
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "new-task",
                "title": "New Task",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0,
                "metric_1": 0,
                "metric_2": 0,
                "metric_3": 0,
                "metric_4": 0,
                "metric_5": 0,
                "metric_6": 0,
                "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null,
                "deleted_by": null,
                "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );

        let options = ImportOptions::replace();
        let preview = db.preview_import(&snapshot, &options);

        assert!(preview.would_succeed);
        assert!(!preview.database_is_empty);
        assert_eq!(preview.mode, ImportMode::Replace);
        // Would delete 2 existing tasks
        assert_eq!(preview.would_delete.get("tasks"), Some(&2));
        // Would insert 1 new task
        assert_eq!(preview.would_insert.get("tasks"), Some(&1));
        assert_eq!(preview.total_would_skip(), 0);
    }

    #[test]
    fn test_preview_merge_mode() {
        let db = Database::open_in_memory().unwrap();

        // Create existing task
        use crate::config::StatesConfig;
        db.create_task(
            Some("existing-task".to_string()),
            "Existing Task".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None, // tags
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Create snapshot with existing and new tasks
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "existing-task", // Will be skipped
                    "title": "Should Skip",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "new-task", // Will be inserted
                    "title": "New Task",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0,
                    "metric_1": 0,
                    "metric_2": 0,
                    "metric_3": 0,
                    "metric_4": 0,
                    "metric_5": 0,
                    "metric_6": 0,
                    "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null,
                    "deleted_by": null,
                    "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );

        let options = ImportOptions::merge();
        let preview = db.preview_import(&snapshot, &options);

        assert!(preview.would_succeed);
        assert!(!preview.database_is_empty);
        assert_eq!(preview.mode, ImportMode::Merge);
        // Would skip 1 existing task
        assert_eq!(preview.would_skip.get("tasks"), Some(&1));
        // Would insert 1 new task
        assert_eq!(preview.would_insert.get("tasks"), Some(&1));
        // No deletions in merge mode
        assert_eq!(preview.total_would_delete(), 0);
    }

    #[test]
    fn test_preview_schema_version_mismatch() {
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();
        snapshot.schema_version = 999; // Invalid version

        let options = ImportOptions::fresh();
        let preview = db.preview_import(&snapshot, &options);

        assert!(!preview.would_succeed);
        assert!(preview.failure_reason.is_some());
        assert!(
            preview
                .failure_reason
                .unwrap()
                .contains("Schema version mismatch")
        );
    }

    #[test]
    fn test_dry_run_result_totals() {
        let mut result = DryRunResult::new(ImportMode::Replace);
        result.existing_rows.insert("tasks".to_string(), 5);
        result.existing_rows.insert("dependencies".to_string(), 3);
        result.would_delete.insert("tasks".to_string(), 5);
        result.would_delete.insert("dependencies".to_string(), 3);
        result.would_insert.insert("tasks".to_string(), 2);
        result.would_skip.insert("attachments".to_string(), 1);

        assert_eq!(result.total_existing(), 8);
        assert_eq!(result.total_would_delete(), 8);
        assert_eq!(result.total_would_insert(), 2);
        assert_eq!(result.total_would_skip(), 1);
    }

    // ============================================================================
    // ID remapping tests
    // ============================================================================

    #[test]
    fn test_remap_snapshot_generates_new_ids() {
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "old-task-1",
                    "title": "Task 1",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "old-task-2",
                    "title": "Task 2",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );

        let ids_config = IdsConfig::default();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config).unwrap();

        // Should have 2 entries in the mapping
        assert_eq!(id_map.len(), 2);
        assert!(id_map.contains_key("old-task-1"));
        assert!(id_map.contains_key("old-task-2"));

        // New IDs should be different from old IDs
        assert_ne!(id_map["old-task-1"], "old-task-1");
        assert_ne!(id_map["old-task-2"], "old-task-2");

        // New IDs should be unique
        assert_ne!(id_map["old-task-1"], id_map["old-task-2"]);

        // Remapped snapshot tasks should have the new IDs
        let tasks = remapped.tables.get("tasks").unwrap();
        let task1_id = tasks[0].get("id").unwrap().as_str().unwrap();
        let task2_id = tasks[1].get("id").unwrap().as_str().unwrap();
        assert_eq!(task1_id, id_map["old-task-1"]);
        assert_eq!(task2_id, id_map["old-task-2"]);
    }

    #[test]
    fn test_remap_snapshot_remaps_dependencies() {
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "parent",
                    "title": "Parent",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "child",
                    "title": "Child",
                    "description": null,
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );

        // Add a contains dependency (parent -> child) and a blocks dependency
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![
                json!({
                    "from_task_id": "parent",
                    "to_task_id": "child",
                    "dep_type": "contains"
                }),
                json!({
                    "from_task_id": "child",
                    "to_task_id": "parent",
                    "dep_type": "blocks"
                }),
            ],
        );

        let ids_config = IdsConfig::default();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config).unwrap();

        let new_parent = &id_map["parent"];
        let new_child = &id_map["child"];

        // Verify dependencies reference the new IDs
        let deps = remapped.tables.get("dependencies").unwrap();
        assert_eq!(deps.len(), 2);

        let dep0 = deps[0].as_object().unwrap();
        assert_eq!(dep0["from_task_id"].as_str().unwrap(), new_parent.as_str());
        assert_eq!(dep0["to_task_id"].as_str().unwrap(), new_child.as_str());
        assert_eq!(dep0["dep_type"].as_str().unwrap(), "contains");

        let dep1 = deps[1].as_object().unwrap();
        assert_eq!(dep1["from_task_id"].as_str().unwrap(), new_child.as_str());
        assert_eq!(dep1["to_task_id"].as_str().unwrap(), new_parent.as_str());
        assert_eq!(dep1["dep_type"].as_str().unwrap(), "blocks");
    }

    #[test]
    fn test_remap_snapshot_remaps_attachments_and_tags() {
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![json!({
                "id": "my-task",
                "title": "My Task",
                "description": null,
                "status": "pending",
                "priority": "5",
                "worker_id": null,
                "claimed_at": null,
                "needed_tags": null,
                "wanted_tags": null,
                "tags": "[]",
                "points": null,
                "time_estimate_ms": null,
                "time_actual_ms": null,
                "started_at": null,
                "completed_at": null,
                "current_thought": null,
                "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                "cost_usd": 0.0,
                "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                "created_at": 1700000000000_i64,
                "updated_at": 1700000000000_i64
            })],
        );
        snapshot.tables.insert(
            "attachments".to_string(),
            vec![json!({
                "task_id": "my-task",
                "attachment_type": "note",
                "sequence": 1,
                "name": "test-note",
                "mime_type": "text/plain",
                "content": "Hello world",
                "file_path": null,
                "created_at": 1700000000000_i64
            })],
        );
        snapshot.tables.insert(
            "task_tags".to_string(),
            vec![json!({
                "task_id": "my-task",
                "tag": "rust"
            })],
        );
        snapshot.tables.insert(
            "task_needed_tags".to_string(),
            vec![json!({
                "task_id": "my-task",
                "tag": "implementer"
            })],
        );
        snapshot.tables.insert(
            "task_wanted_tags".to_string(),
            vec![json!({
                "task_id": "my-task",
                "tag": "code"
            })],
        );
        snapshot.tables.insert(
            "task_sequence".to_string(),
            vec![json!({
                "id": 1,
                "task_id": "my-task",
                "worker_id": null,
                "status": "pending",
                "phase": null,
                "reason": null,
                "timestamp": 1700000000000_i64,
                "end_timestamp": null
            })],
        );

        let ids_config = IdsConfig::default();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config).unwrap();
        let new_id = &id_map["my-task"];

        // Attachments should use new task_id
        let atts = remapped.tables.get("attachments").unwrap();
        assert_eq!(atts[0]["task_id"].as_str().unwrap(), new_id.as_str());

        // Tags should use new task_id
        let tags = remapped.tables.get("task_tags").unwrap();
        assert_eq!(tags[0]["task_id"].as_str().unwrap(), new_id.as_str());

        let needed = remapped.tables.get("task_needed_tags").unwrap();
        assert_eq!(needed[0]["task_id"].as_str().unwrap(), new_id.as_str());

        let wanted = remapped.tables.get("task_wanted_tags").unwrap();
        assert_eq!(wanted[0]["task_id"].as_str().unwrap(), new_id.as_str());

        // State history should use new task_id
        let events = remapped.tables.get("task_sequence").unwrap();
        assert_eq!(events[0]["task_id"].as_str().unwrap(), new_id.as_str());
    }

    #[test]
    fn test_remap_snapshot_empty() {
        // Empty snapshot should produce empty mapping
        let snapshot = Snapshot::new();
        let ids_config = IdsConfig::default();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config).unwrap();

        assert!(id_map.is_empty());
        assert!(remapped.tables.is_empty());
    }

    #[test]
    fn test_remap_import_round_trip() {
        // Test that a remapped snapshot can be imported successfully
        let db = Database::open_in_memory().unwrap();
        let mut snapshot = Snapshot::new();

        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({
                    "id": "task-alpha",
                    "title": "Alpha Task",
                    "description": "First task",
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
                json!({
                    "id": "task-beta",
                    "title": "Beta Task",
                    "description": "Second task",
                    "status": "pending",
                    "priority": "3",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": null,
                    "wanted_tags": null,
                    "tags": "[]",
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
                    "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
                    "cost_usd": 0.0,
                    "deleted_at": null, "deleted_by": null, "deleted_reason": null,
                    "created_at": 1700000000000_i64,
                    "updated_at": 1700000000000_i64
                }),
            ],
        );
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![json!({
                "from_task_id": "task-alpha",
                "to_task_id": "task-beta",
                "dep_type": "contains"
            })],
        );

        // Remap IDs
        let ids_config = IdsConfig::default();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config).unwrap();

        // Import the remapped snapshot
        let options = ImportOptions::fresh();
        let result = db.import_snapshot(&remapped, &options).unwrap();

        assert_eq!(result.rows_imported.get("tasks"), Some(&2));
        assert_eq!(result.rows_imported.get("dependencies"), Some(&1));

        // Verify tasks exist with new IDs
        let new_alpha = &id_map["task-alpha"];
        let new_beta = &id_map["task-beta"];

        // Search for the tasks in the database
        let alpha_results = db.search_tasks("Alpha", None, 0, false, None).unwrap();
        assert_eq!(alpha_results.len(), 1);
        assert_eq!(alpha_results[0].task_id, *new_alpha);

        let beta_results = db.search_tasks("Beta", None, 0, false, None).unwrap();
        assert_eq!(beta_results.len(), 1);
        assert_eq!(beta_results[0].task_id, *new_beta);
    }

    // ============================================================================
    // Parent attachment tests
    // ============================================================================

    #[test]
    fn test_snapshot_root_task_ids_all_roots() {
        // Snapshot with two tasks and no "contains" deps: both are roots
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({"id": "a", "title": "A"}),
                json!({"id": "b", "title": "B"}),
            ],
        );
        let roots = snapshot_root_task_ids(&snapshot);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&"a".to_string()));
        assert!(roots.contains(&"b".to_string()));
    }

    #[test]
    fn test_snapshot_root_task_ids_with_contains() {
        // "a" contains "b" -> only "a" is a root
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({"id": "a", "title": "A"}),
                json!({"id": "b", "title": "B"}),
            ],
        );
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![json!({"from_task_id": "a", "to_task_id": "b", "dep_type": "contains"})],
        );
        let roots = snapshot_root_task_ids(&snapshot);
        assert_eq!(roots.len(), 1);
        assert!(roots.contains(&"a".to_string()));
    }

    #[test]
    fn test_snapshot_root_task_ids_non_contains_dep_ignored() {
        // "a" blocks "b" -> both are roots (only "contains" matters)
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                json!({"id": "a", "title": "A"}),
                json!({"id": "b", "title": "B"}),
            ],
        );
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![json!({"from_task_id": "a", "to_task_id": "b", "dep_type": "blocks"})],
        );
        let roots = snapshot_root_task_ids(&snapshot);
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn test_snapshot_root_task_ids_empty_snapshot() {
        let snapshot = Snapshot::new();
        let roots = snapshot_root_task_ids(&snapshot);
        assert!(roots.is_empty());
    }

    /// Helper to create a full task JSON value for import tests.
    fn make_task_json(id: &str, title: &str) -> serde_json::Value {
        json!({
            "id": id,
            "title": title,
            "description": "",
            "status": "pending",
            "priority": "5",
            "worker_id": null,
            "claimed_at": null,
            "needed_tags": null,
            "wanted_tags": null,
            "tags": "[]",
            "points": null,
            "time_estimate_ms": null,
            "time_actual_ms": null,
            "started_at": null,
            "completed_at": null,
            "current_thought": null,
            "metric_0": 0, "metric_1": 0, "metric_2": 0, "metric_3": 0,
            "metric_4": 0, "metric_5": 0, "metric_6": 0, "metric_7": 0,
            "cost_usd": 0.0,
            "deleted_at": null, "deleted_by": null, "deleted_reason": null,
            "created_at": 1700000000000_i64,
            "updated_at": 1700000000000_i64
        })
    }

    #[test]
    fn test_import_with_parent_attaches_root_tasks() {
        use crate::config::StatesConfig;

        let db = Database::open_in_memory().unwrap();

        // Pre-create the parent task in the database
        db.create_task(
            Some("parent-task".to_string()),
            "Parent".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Create snapshot with two root tasks and one child
        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![
                make_task_json("root-a", "Root A"),
                make_task_json("root-b", "Root B"),
                make_task_json("child-c", "Child C"),
            ],
        );
        snapshot.tables.insert(
            "dependencies".to_string(),
            vec![
                json!({"from_task_id": "root-a", "to_task_id": "child-c", "dep_type": "contains"}),
            ],
        );

        // Import with parent -- use merge mode since parent task already exists in DB
        let options = ImportOptions::merge().with_parent("parent-task".to_string());
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // Verify root tasks were linked
        assert_eq!(result.parent_linked_roots.len(), 2);
        assert!(result.parent_linked_roots.contains(&"root-a".to_string()));
        assert!(result.parent_linked_roots.contains(&"root-b".to_string()));
        // child-c should NOT be in roots (it has a contains parent)
        assert!(!result.parent_linked_roots.contains(&"child-c".to_string()));

        // Verify "contains" dependencies exist in DB
        let parent_a = db.get_parent("root-a").unwrap();
        assert_eq!(parent_a, Some("parent-task".to_string()));

        let parent_b = db.get_parent("root-b").unwrap();
        assert_eq!(parent_b, Some("parent-task".to_string()));

        // child-c should have root-a as parent (from the snapshot)
        let parent_c = db.get_parent("child-c").unwrap();
        assert_eq!(parent_c, Some("root-a".to_string()));
    }

    #[test]
    fn test_import_with_parent_not_found_fails() {
        let db = Database::open_in_memory().unwrap();

        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![make_task_json("task-x", "Task X")],
        );

        // Import with nonexistent parent
        let options = ImportOptions::fresh().with_parent("nonexistent".to_string());
        let result = db.import_snapshot(&snapshot, &options);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found"),
            "Expected 'not found' in: {}",
            err_msg
        );
    }

    #[test]
    fn test_import_without_parent_does_not_link() {
        let db = Database::open_in_memory().unwrap();

        let mut snapshot = Snapshot::new();
        snapshot.tables.insert(
            "tasks".to_string(),
            vec![make_task_json("task-y", "Task Y")],
        );

        // Import without parent
        let options = ImportOptions::fresh();
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        assert!(result.parent_linked_roots.is_empty());

        // Verify no parent exists
        let parent = db.get_parent("task-y").unwrap();
        assert_eq!(parent, None);
    }

    #[test]
    fn test_import_with_parent_and_empty_snapshot() {
        use crate::config::StatesConfig;

        let db = Database::open_in_memory().unwrap();

        // Pre-create the parent task
        db.create_task(
            Some("parent-task".to_string()),
            "Parent".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &StatesConfig::default(),
            &IdsConfig::default(),
        )
        .unwrap();

        // Import empty snapshot with parent -- use merge since parent already exists
        let snapshot = Snapshot::new();
        let options = ImportOptions::merge().with_parent("parent-task".to_string());
        let result = db.import_snapshot(&snapshot, &options).unwrap();

        // No roots to link
        assert!(result.parent_linked_roots.is_empty());
    }

    #[test]
    fn test_import_options_with_parent_builder() {
        let options = ImportOptions::merge().with_parent("my-parent".to_string());
        assert_eq!(options.mode, ImportMode::Merge);
        assert_eq!(options.parent_id, Some("my-parent".to_string()));
        assert!(!options.remap_ids);
    }
}
