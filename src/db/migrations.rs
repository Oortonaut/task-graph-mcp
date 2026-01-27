//! Migration registry for structured export/import data transformations.
//!
//! This module handles transforming exported JSON data between different schema versions.
//! Unlike SQL migrations (handled by refinery), these migrations transform the in-memory
//! JSON representation of task data during import operations.
//!
//! # Example
//!
//! ```ignore
//! use task_graph::db::migrations::{MigrationRegistry, SchemaVersion};
//!
//! let registry = MigrationRegistry::new()
//!     .register(2, 3, migrate_v2_to_v3)
//!     .register(3, 4, migrate_v3_to_v4);
//!
//! // Find migration path from v2 to v4
//! let path = registry.find_path(2, 4)?;
//! assert_eq!(path, vec![(2, 3), (3, 4)]);
//!
//! // Apply migrations to transform data
//! let migrated = registry.migrate(data, 2, 4)?;
//! ```

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

/// Schema version identifier.
///
/// Versions are positive integers that increase monotonically.
/// The current schema version should match the database migration version.
pub type SchemaVersion = u32;

/// A migration function that transforms JSON export data from one version to another.
///
/// # Arguments
/// * `data` - Mutable reference to the JSON Value representing the export data.
///   The function should modify this in place.
///
/// # Returns
/// * `Ok(())` on successful migration
/// * `Err` with details if the migration fails (e.g., missing required fields)
///
/// # Example
///
/// ```ignore
/// fn migrate_v2_to_v3(data: &mut Value) -> Result<()> {
///     // Transform data from v2 format to v3 format
///     if let Some(tasks) = data.get_mut("tables").and_then(|t| t.get_mut("tasks")) {
///         // Add new field with default value
///         if let Some(arr) = tasks.as_array_mut() {
///             for task in arr {
///                 task["new_field"] = Value::Null;
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
pub type MigrationFn = fn(&mut Value) -> Result<()>;

/// A migration step with metadata.
#[derive(Clone)]
pub struct Migration {
    /// Source schema version.
    pub from: SchemaVersion,
    /// Target schema version.
    pub to: SchemaVersion,
    /// Description of what this migration does.
    pub description: &'static str,
    /// The migration function.
    pub migrate: MigrationFn,
}

impl fmt::Debug for Migration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Migration")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("description", &self.description)
            .finish()
    }
}

/// Error type for migration operations.
#[derive(Debug, Clone)]
pub enum MigrationError {
    /// No migration path exists between the versions.
    NoPath {
        from: SchemaVersion,
        to: SchemaVersion,
        available: Vec<(SchemaVersion, SchemaVersion)>,
    },
    /// A migration function failed.
    MigrationFailed {
        from: SchemaVersion,
        to: SchemaVersion,
        reason: String,
    },
    /// The source version is greater than or equal to the target.
    InvalidVersionRange {
        from: SchemaVersion,
        to: SchemaVersion,
    },
    /// Circular dependency detected in migration graph.
    CycleDetected {
        versions: Vec<SchemaVersion>,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::NoPath { from, to, available } => {
                write!(
                    f,
                    "Cannot migrate from schema v{} to v{}. ",
                    from, to
                )?;
                if available.is_empty() {
                    write!(f, "No migrations registered.")
                } else {
                    let paths: Vec<String> = available
                        .iter()
                        .map(|(a, b)| format!("v{}->v{}", a, b))
                        .collect();
                    write!(f, "Supported migrations: {}", paths.join(", "))
                }
            }
            MigrationError::MigrationFailed { from, to, reason } => {
                write!(
                    f,
                    "Migration v{}->v{} failed: {}",
                    from, to, reason
                )
            }
            MigrationError::InvalidVersionRange { from, to } => {
                write!(
                    f,
                    "Invalid version range: source ({}) must be less than target ({})",
                    from, to
                )
            }
            MigrationError::CycleDetected { versions } => {
                let cycle: Vec<String> = versions.iter().map(|v| format!("v{}", v)).collect();
                write!(f, "Cycle detected in migration graph: {}", cycle.join(" -> "))
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// Registry for data migrations between schema versions.
///
/// The registry maintains a directed graph of migrations and can find paths
/// to migrate data through multiple versions (e.g., v2 -> v3 -> v4).
#[derive(Default)]
pub struct MigrationRegistry {
    /// Migrations indexed by (from_version, to_version).
    migrations: HashMap<(SchemaVersion, SchemaVersion), Migration>,
    /// Adjacency list for path finding: from_version -> [(to_version, ...)].
    adjacency: HashMap<SchemaVersion, Vec<SchemaVersion>>,
}

impl MigrationRegistry {
    /// Create an empty migration registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a migration between two schema versions.
    ///
    /// # Arguments
    /// * `from` - Source schema version
    /// * `to` - Target schema version
    /// * `description` - Human-readable description of what this migration does
    /// * `migrate` - Function that transforms data from `from` to `to`
    ///
    /// # Panics
    /// Panics if `from >= to` (migrations must go forward).
    pub fn register(
        mut self,
        from: SchemaVersion,
        to: SchemaVersion,
        description: &'static str,
        migrate: MigrationFn,
    ) -> Self {
        assert!(
            from < to,
            "Migration must go forward: {} -> {}",
            from,
            to
        );

        let migration = Migration {
            from,
            to,
            description,
            migrate,
        };

        self.migrations.insert((from, to), migration);
        self.adjacency.entry(from).or_default().push(to);

        self
    }

    /// Get all registered migration steps.
    pub fn migrations(&self) -> impl Iterator<Item = &Migration> {
        self.migrations.values()
    }

    /// Get available direct migrations as (from, to) pairs.
    pub fn available_migrations(&self) -> Vec<(SchemaVersion, SchemaVersion)> {
        self.migrations.keys().copied().collect()
    }

    /// Check if a direct migration exists.
    pub fn has_direct_migration(&self, from: SchemaVersion, to: SchemaVersion) -> bool {
        self.migrations.contains_key(&(from, to))
    }

    /// Get a direct migration if it exists.
    pub fn get_migration(&self, from: SchemaVersion, to: SchemaVersion) -> Option<&Migration> {
        self.migrations.get(&(from, to))
    }

    /// Find the shortest migration path between two versions.
    ///
    /// Uses BFS to find the path with the fewest migration steps.
    ///
    /// # Arguments
    /// * `from` - Source schema version
    /// * `to` - Target schema version
    ///
    /// # Returns
    /// * `Ok(path)` - Vector of (from, to) pairs representing the migration path
    /// * `Err(MigrationError::NoPath)` - If no path exists
    /// * `Err(MigrationError::InvalidVersionRange)` - If from >= to
    pub fn find_path(
        &self,
        from: SchemaVersion,
        to: SchemaVersion,
    ) -> Result<Vec<(SchemaVersion, SchemaVersion)>, MigrationError> {
        if from >= to {
            return Err(MigrationError::InvalidVersionRange { from, to });
        }

        // Special case: direct migration exists
        if self.migrations.contains_key(&(from, to)) {
            return Ok(vec![(from, to)]);
        }

        // BFS to find shortest path
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent: HashMap<SchemaVersion, SchemaVersion> = HashMap::new();

        queue.push_back(from);
        visited.insert(from);

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstruct path
                let mut path = Vec::new();
                let mut node = to;
                while let Some(&prev) = parent.get(&node) {
                    path.push((prev, node));
                    node = prev;
                }
                path.reverse();
                return Ok(path);
            }

            if let Some(neighbors) = self.adjacency.get(&current) {
                for &next in neighbors {
                    if !visited.contains(&next) && next <= to {
                        visited.insert(next);
                        parent.insert(next, current);
                        queue.push_back(next);
                    }
                }
            }
        }

        Err(MigrationError::NoPath {
            from,
            to,
            available: self.available_migrations(),
        })
    }

    /// Migrate JSON data from one schema version to another.
    ///
    /// This finds the migration path and applies each migration in sequence.
    ///
    /// # Arguments
    /// * `data` - Mutable reference to the JSON export data
    /// * `from` - Current schema version of the data
    /// * `to` - Target schema version
    ///
    /// # Returns
    /// * `Ok(())` - If migration succeeded
    /// * `Err` - If path not found or a migration step failed
    pub fn migrate(
        &self,
        data: &mut Value,
        from: SchemaVersion,
        to: SchemaVersion,
    ) -> Result<(), MigrationError> {
        if from == to {
            return Ok(());
        }

        let path = self.find_path(from, to)?;

        for (step_from, step_to) in path {
            let migration = self
                .migrations
                .get(&(step_from, step_to))
                .expect("path contains only valid migrations");

            (migration.migrate)(data).map_err(|e| MigrationError::MigrationFailed {
                from: step_from,
                to: step_to,
                reason: e.to_string(),
            })?;

            // Update schema_version in the data
            if let Some(obj) = data.as_object_mut() {
                obj.insert("schema_version".to_string(), Value::Number(step_to.into()));
            }
        }

        Ok(())
    }

    /// Check if migration is possible between two versions.
    pub fn can_migrate(&self, from: SchemaVersion, to: SchemaVersion) -> bool {
        if from >= to {
            return false;
        }
        self.find_path(from, to).is_ok()
    }

    /// Get the description of migration steps from one version to another.
    ///
    /// Useful for explaining what will happen during migration.
    pub fn describe_path(
        &self,
        from: SchemaVersion,
        to: SchemaVersion,
    ) -> Result<Vec<String>, MigrationError> {
        let path = self.find_path(from, to)?;
        Ok(path
            .iter()
            .map(|(f, t)| {
                let migration = self.migrations.get(&(*f, *t)).unwrap();
                format!("v{} -> v{}: {}", f, t, migration.description)
            })
            .collect())
    }

    /// Get the highest schema version known to the registry.
    pub fn max_version(&self) -> Option<SchemaVersion> {
        self.migrations
            .values()
            .map(|m| m.to)
            .max()
    }

    /// Get all known schema versions (both source and target).
    pub fn all_versions(&self) -> Vec<SchemaVersion> {
        let mut versions: HashSet<SchemaVersion> = HashSet::new();
        for migration in self.migrations.values() {
            versions.insert(migration.from);
            versions.insert(migration.to);
        }
        let mut sorted: Vec<_> = versions.into_iter().collect();
        sorted.sort();
        sorted
    }
}

impl fmt::Debug for MigrationRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MigrationRegistry")
            .field("migrations", &self.migrations.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Builder for creating the default migration registry with all known migrations.
///
/// This is where actual migration functions should be registered.
/// Call `build_default_registry()` to get a registry with all migrations.
pub fn build_default_registry() -> MigrationRegistry {
    MigrationRegistry::new()
        // Register migrations as they are implemented:
        // .register(1, 2, "Add foo field to tasks", migrate_v1_to_v2)
        // .register(2, 3, "Rename bar to baz", migrate_v2_to_v3)
}

// ============================================================================
// Import Migration Execution
// ============================================================================

/// Result of checking schema compatibility.
#[derive(Debug, Clone)]
pub enum SchemaCompatibility {
    /// Versions match, no migration needed.
    Compatible,
    /// Migration is required and a path exists.
    NeedsMigration {
        from: SchemaVersion,
        to: SchemaVersion,
        path: Vec<(SchemaVersion, SchemaVersion)>,
    },
    /// Migration is required but no path exists.
    Incompatible {
        from: SchemaVersion,
        to: SchemaVersion,
        error: MigrationError,
    },
    /// Export is newer than database schema.
    ExportNewer {
        export_version: SchemaVersion,
        database_version: SchemaVersion,
    },
}

impl SchemaCompatibility {
    /// Check if migration can proceed (either compatible or migratable).
    pub fn can_proceed(&self) -> bool {
        matches!(self, SchemaCompatibility::Compatible | SchemaCompatibility::NeedsMigration { .. })
    }

    /// Get a human-readable description of the compatibility status.
    pub fn describe(&self) -> String {
        match self {
            SchemaCompatibility::Compatible => {
                "Schema versions match, no migration needed.".to_string()
            }
            SchemaCompatibility::NeedsMigration { from, to, path } => {
                let steps: Vec<String> = path
                    .iter()
                    .map(|(f, t)| format!("v{} -> v{}", f, t))
                    .collect();
                format!(
                    "Migration required from v{} to v{}: {}",
                    from, to, steps.join(" -> ")
                )
            }
            SchemaCompatibility::Incompatible { from, to, error } => {
                format!(
                    "Cannot migrate from v{} to v{}: {}",
                    from, to, error
                )
            }
            SchemaCompatibility::ExportNewer { export_version, database_version } => {
                format!(
                    "Export schema v{} is newer than database schema v{}. \
                    Please upgrade the database first.",
                    export_version, database_version
                )
            }
        }
    }
}

/// Check schema compatibility between an export and the target database.
///
/// # Arguments
/// * `registry` - The migration registry to use for path finding
/// * `export_version` - Schema version of the export data
/// * `database_version` - Schema version of the target database
///
/// # Returns
/// A `SchemaCompatibility` variant indicating whether migration is needed/possible.
pub fn check_schema_compatibility(
    registry: &MigrationRegistry,
    export_version: SchemaVersion,
    database_version: SchemaVersion,
) -> SchemaCompatibility {
    if export_version == database_version {
        return SchemaCompatibility::Compatible;
    }

    if export_version > database_version {
        return SchemaCompatibility::ExportNewer {
            export_version,
            database_version,
        };
    }

    // export_version < database_version: need to migrate data forward
    match registry.find_path(export_version, database_version) {
        Ok(path) => SchemaCompatibility::NeedsMigration {
            from: export_version,
            to: database_version,
            path,
        },
        Err(error) => SchemaCompatibility::Incompatible {
            from: export_version,
            to: database_version,
            error,
        },
    }
}

/// Migrate export data in place from one schema version to another.
///
/// This is a convenience function that:
/// 1. Checks schema compatibility
/// 2. Applies all necessary migrations
/// 3. Updates the schema_version field in the data
///
/// # Arguments
/// * `registry` - The migration registry to use
/// * `data` - Mutable reference to the JSON export data (Snapshot as Value)
/// * `target_version` - Target schema version
///
/// # Returns
/// * `Ok(MigrationReport)` with details of what was done
/// * `Err(MigrationError)` if migration fails
pub fn migrate_export_data(
    registry: &MigrationRegistry,
    data: &mut Value,
    target_version: SchemaVersion,
) -> Result<MigrationReport, MigrationError> {
    let export_version = data
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as SchemaVersion)
        .unwrap_or(1); // Default to v1 if not specified

    let compatibility = check_schema_compatibility(registry, export_version, target_version);

    match compatibility {
        SchemaCompatibility::Compatible => {
            Ok(MigrationReport {
                from_version: export_version,
                to_version: target_version,
                steps_applied: vec![],
                was_migrated: false,
            })
        }
        SchemaCompatibility::NeedsMigration { from, to, path } => {
            let mut steps_applied = Vec::new();

            for (step_from, step_to) in &path {
                let migration = registry
                    .get_migration(*step_from, *step_to)
                    .expect("path contains only valid migrations");

                (migration.migrate)(data).map_err(|e| MigrationError::MigrationFailed {
                    from: *step_from,
                    to: *step_to,
                    reason: e.to_string(),
                })?;

                steps_applied.push(MigrationStep {
                    from: *step_from,
                    to: *step_to,
                    description: migration.description.to_string(),
                });

                // Update schema_version in the data
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("schema_version".to_string(), Value::Number((*step_to).into()));
                }
            }

            Ok(MigrationReport {
                from_version: from,
                to_version: to,
                steps_applied,
                was_migrated: true,
            })
        }
        SchemaCompatibility::Incompatible { error, .. } => Err(error),
        SchemaCompatibility::ExportNewer { export_version, database_version } => {
            Err(MigrationError::InvalidVersionRange {
                from: export_version,
                to: database_version,
            })
        }
    }
}

/// Report of a completed migration operation.
#[derive(Debug, Clone)]
pub struct MigrationReport {
    /// Original schema version of the data.
    pub from_version: SchemaVersion,
    /// Target schema version (same as from if no migration needed).
    pub to_version: SchemaVersion,
    /// List of migration steps that were applied.
    pub steps_applied: Vec<MigrationStep>,
    /// Whether any migrations were actually applied.
    pub was_migrated: bool,
}

impl MigrationReport {
    /// Get a human-readable summary of the migration.
    pub fn summary(&self) -> String {
        if !self.was_migrated {
            format!("No migration needed (schema v{})", self.from_version)
        } else {
            let steps: Vec<String> = self
                .steps_applied
                .iter()
                .map(|s| format!("v{} -> v{}", s.from, s.to))
                .collect();
            format!(
                "Migrated from v{} to v{} ({} steps: {})",
                self.from_version,
                self.to_version,
                self.steps_applied.len(),
                steps.join(", ")
            )
        }
    }
}

/// A single migration step that was applied.
#[derive(Debug, Clone)]
pub struct MigrationStep {
    /// Source version.
    pub from: SchemaVersion,
    /// Target version.
    pub to: SchemaVersion,
    /// Description of what the migration did.
    pub description: String,
}

/// Validate that an export can be imported into the current database schema.
///
/// This is a convenience function for the import workflow that:
/// 1. Extracts the schema_version from the export
/// 2. Compares it with the current database schema version
/// 3. Checks if a migration path exists
/// 4. Returns a user-friendly validation result
///
/// # Arguments
/// * `export_schema_version` - Schema version from the export file
/// * `current_schema_version` - Current database schema version
///
/// # Returns
/// A `SchemaValidationResult` with the validation outcome and user-friendly message
pub fn validate_import_schema(
    export_schema_version: SchemaVersion,
    current_schema_version: SchemaVersion,
) -> SchemaValidationResult {
    let registry = build_default_registry();
    let compatibility = check_schema_compatibility(&registry, export_schema_version, current_schema_version);

    SchemaValidationResult {
        export_version: export_schema_version,
        database_version: current_schema_version,
        compatibility,
    }
}

/// Result of validating an import's schema version.
#[derive(Debug, Clone)]
pub struct SchemaValidationResult {
    /// Schema version of the export file.
    pub export_version: SchemaVersion,
    /// Current database schema version.
    pub database_version: SchemaVersion,
    /// Compatibility status.
    pub compatibility: SchemaCompatibility,
}

impl SchemaValidationResult {
    /// Check if the import can proceed.
    pub fn is_valid(&self) -> bool {
        self.compatibility.can_proceed()
    }

    /// Get a user-friendly message about the validation result.
    pub fn message(&self) -> String {
        match &self.compatibility {
            SchemaCompatibility::Compatible => {
                format!(
                    "Schema compatible: export v{} matches database v{}",
                    self.export_version, self.database_version
                )
            }
            SchemaCompatibility::NeedsMigration { path, .. } => {
                format!(
                    "Schema migration required: export v{} will be migrated to v{} ({} steps)",
                    self.export_version, self.database_version, path.len()
                )
            }
            SchemaCompatibility::Incompatible { error, .. } => {
                format!("Schema incompatible: {}", error)
            }
            SchemaCompatibility::ExportNewer { .. } => {
                format!(
                    "Schema incompatible: export v{} is newer than database v{}. \
                    Upgrade the database or use a compatible export.",
                    self.export_version, self.database_version
                )
            }
        }
    }

    /// Get a detailed description suitable for verbose output.
    pub fn details(&self) -> String {
        self.compatibility.describe()
    }
}

// ============================================================================
// Migration Functions
// ============================================================================
// Each migration function below transforms JSON data from one version to the next.
// Add new migrations here as schema evolves.

/// Example migration function (not used, serves as template).
#[allow(dead_code)]
fn migrate_example(data: &mut Value) -> Result<()> {
    // Access tables
    let tables = data
        .get_mut("tables")
        .ok_or_else(|| anyhow!("Missing 'tables' object"))?;

    // Transform tasks
    if let Some(tasks) = tables.get_mut("tasks").and_then(|t| t.as_array_mut()) {
        for task in tasks {
            // Add new field with default value
            if task.get("new_field").is_none() {
                task["new_field"] = Value::Null;
            }

            // Rename field
            if let Some(old_value) = task.get("old_name").cloned() {
                task["new_name"] = old_value;
                task.as_object_mut().unwrap().remove("old_name");
            }

            // Transform field value
            if task.get("status").and_then(|s| s.as_str()) == Some("old_status") {
                task["status"] = Value::String("new_status".to_string());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn noop_migration(_data: &mut Value) -> Result<()> {
        Ok(())
    }

    fn add_field_migration(data: &mut Value) -> Result<()> {
        if let Some(tables) = data.get_mut("tables") {
            if let Some(tasks) = tables.get_mut("tasks").and_then(|t| t.as_array_mut()) {
                for task in tasks {
                    task["new_field"] = Value::String("default".to_string());
                }
            }
        }
        Ok(())
    }

    fn failing_migration(_data: &mut Value) -> Result<()> {
        Err(anyhow!("Migration failed intentionally"))
    }

    #[test]
    fn test_register_migration() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "Test migration", noop_migration);

        assert!(registry.has_direct_migration(1, 2));
        assert!(!registry.has_direct_migration(2, 3));
    }

    #[test]
    fn test_find_direct_path() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration);

        let path = registry.find_path(1, 2).unwrap();
        assert_eq!(path, vec![(1, 2)]);
    }

    #[test]
    fn test_find_chained_path() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(2, 3, "v2 to v3", noop_migration)
            .register(3, 4, "v3 to v4", noop_migration);

        let path = registry.find_path(1, 4).unwrap();
        assert_eq!(path, vec![(1, 2), (2, 3), (3, 4)]);
    }

    #[test]
    fn test_find_shortest_path() {
        // Create graph with multiple paths: 1->2->4 and 1->2->3->4
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(2, 3, "v2 to v3", noop_migration)
            .register(2, 4, "v2 to v4 (direct)", noop_migration)
            .register(3, 4, "v3 to v4", noop_migration);

        // Should find shorter path 1->2->4
        let path = registry.find_path(1, 4).unwrap();
        assert_eq!(path, vec![(1, 2), (2, 4)]);
    }

    #[test]
    fn test_no_path() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(3, 4, "v3 to v4", noop_migration);

        let result = registry.find_path(1, 4);
        assert!(matches!(result, Err(MigrationError::NoPath { .. })));
    }

    #[test]
    fn test_no_path_error_message() {
        // Verify error message matches spec: "Cannot import schema v1 into v4. Supported: v2->v3, v3->v4"
        let registry = MigrationRegistry::new()
            .register(2, 3, "v2 to v3", noop_migration)
            .register(3, 4, "v3 to v4", noop_migration);

        let result = registry.find_path(1, 4);
        match result {
            Err(MigrationError::NoPath { from, to, available }) => {
                assert_eq!(from, 1);
                assert_eq!(to, 4);
                // Check available contains our migrations
                assert!(available.contains(&(2, 3)));
                assert!(available.contains(&(3, 4)));
                
                // Verify Display formatting
                let error = MigrationError::NoPath { from, to, available };
                let message = error.to_string();
                assert!(message.contains("Cannot migrate from schema v1 to v4"));
                assert!(message.contains("Supported migrations:"));
                assert!(message.contains("v2->v3"));
                assert!(message.contains("v3->v4"));
            }
            _ => panic!("Expected NoPath error"),
        }
    }

    #[test]
    fn test_no_migrations_registered_error_message() {
        let registry = MigrationRegistry::new();

        let result = registry.find_path(1, 4);
        match result {
            Err(MigrationError::NoPath { from, to, available }) => {
                assert_eq!(from, 1);
                assert_eq!(to, 4);
                assert!(available.is_empty());
                
                let message = MigrationError::NoPath { from, to, available }.to_string();
                assert!(message.contains("No migrations registered"));
            }
            _ => panic!("Expected NoPath error"),
        }
    }

    #[test]
    fn test_invalid_version_range() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration);

        // from >= to
        assert!(matches!(
            registry.find_path(2, 1),
            Err(MigrationError::InvalidVersionRange { .. })
        ));
        assert!(matches!(
            registry.find_path(2, 2),
            Err(MigrationError::InvalidVersionRange { .. })
        ));
    }

    #[test]
    fn test_migrate_data() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "Add new_field to tasks", add_field_migration);

        let mut data = json!({
            "schema_version": 1,
            "tables": {
                "tasks": [
                    {"id": "1", "title": "Task 1"},
                    {"id": "2", "title": "Task 2"}
                ]
            }
        });

        registry.migrate(&mut data, 1, 2).unwrap();

        // Check schema_version updated
        assert_eq!(data["schema_version"], 2);

        // Check field added to tasks
        let tasks = data["tables"]["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["new_field"], "default");
        assert_eq!(tasks[1]["new_field"], "default");
    }

    #[test]
    fn test_migrate_same_version() {
        let registry = MigrationRegistry::new();

        let mut data = json!({"schema_version": 2});

        // Should be a no-op
        registry.migrate(&mut data, 2, 2).unwrap();
        assert_eq!(data["schema_version"], 2);
    }

    #[test]
    fn test_migration_failure() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "Failing migration", failing_migration);

        let mut data = json!({"schema_version": 1, "tables": {}});

        let result = registry.migrate(&mut data, 1, 2);
        assert!(matches!(
            result,
            Err(MigrationError::MigrationFailed { from: 1, to: 2, .. })
        ));
    }

    #[test]
    fn test_describe_path() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "Add foo field", noop_migration)
            .register(2, 3, "Rename bar to baz", noop_migration);

        let descriptions = registry.describe_path(1, 3).unwrap();
        assert_eq!(descriptions.len(), 2);
        assert!(descriptions[0].contains("Add foo field"));
        assert!(descriptions[1].contains("Rename bar to baz"));
    }

    #[test]
    fn test_can_migrate() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(2, 3, "v2 to v3", noop_migration);

        assert!(registry.can_migrate(1, 2));
        assert!(registry.can_migrate(1, 3));
        assert!(registry.can_migrate(2, 3));
        assert!(!registry.can_migrate(1, 4)); // no path
        assert!(!registry.can_migrate(3, 1)); // backwards
    }

    #[test]
    fn test_max_version() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(2, 5, "v2 to v5", noop_migration);

        assert_eq!(registry.max_version(), Some(5));
    }

    #[test]
    fn test_all_versions() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(3, 4, "v3 to v4", noop_migration);

        let versions = registry.all_versions();
        assert_eq!(versions, vec![1, 2, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "Migration must go forward")]
    fn test_register_backwards_panics() {
        MigrationRegistry::new().register(3, 2, "Invalid", noop_migration);
    }

    // =========================================================================
    // Migration Execution Tests
    // =========================================================================

    #[test]
    fn test_schema_compatibility_compatible() {
        let registry = MigrationRegistry::new();
        let result = check_schema_compatibility(&registry, 3, 3);
        assert!(matches!(result, SchemaCompatibility::Compatible));
        assert!(result.can_proceed());
    }

    #[test]
    fn test_schema_compatibility_needs_migration() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration)
            .register(2, 3, "v2 to v3", noop_migration);

        let result = check_schema_compatibility(&registry, 1, 3);
        match &result {
            SchemaCompatibility::NeedsMigration { from, to, path } => {
                assert_eq!(*from, 1);
                assert_eq!(*to, 3);
                assert_eq!(*path, vec![(1, 2), (2, 3)]);
            }
            _ => panic!("Expected NeedsMigration"),
        }
        assert!(result.can_proceed());
    }

    #[test]
    fn test_schema_compatibility_incompatible() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration);

        // No path from 1 to 5
        let result = check_schema_compatibility(&registry, 1, 5);
        assert!(matches!(result, SchemaCompatibility::Incompatible { .. }));
        assert!(!result.can_proceed());
    }

    #[test]
    fn test_schema_compatibility_export_newer() {
        let registry = MigrationRegistry::new();
        let result = check_schema_compatibility(&registry, 5, 3);
        match result {
            SchemaCompatibility::ExportNewer { export_version, database_version } => {
                assert_eq!(export_version, 5);
                assert_eq!(database_version, 3);
            }
            _ => panic!("Expected ExportNewer"),
        }
        assert!(!result.can_proceed());
    }

    #[test]
    fn test_migrate_export_data_no_migration() {
        let registry = MigrationRegistry::new();
        let mut data = json!({
            "schema_version": 3,
            "tables": {}
        });

        let report = migrate_export_data(&registry, &mut data, 3).unwrap();
        assert!(!report.was_migrated);
        assert_eq!(report.from_version, 3);
        assert_eq!(report.to_version, 3);
        assert!(report.steps_applied.is_empty());
    }

    #[test]
    fn test_migrate_export_data_with_migration() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "Add new field", add_field_migration)
            .register(2, 3, "No-op migration", noop_migration);

        let mut data = json!({
            "schema_version": 1,
            "tables": {
                "tasks": [
                    {"id": "1", "title": "Task 1"}
                ]
            }
        });

        let report = migrate_export_data(&registry, &mut data, 3).unwrap();
        
        assert!(report.was_migrated);
        assert_eq!(report.from_version, 1);
        assert_eq!(report.to_version, 3);
        assert_eq!(report.steps_applied.len(), 2);
        assert_eq!(report.steps_applied[0].from, 1);
        assert_eq!(report.steps_applied[0].to, 2);
        assert_eq!(report.steps_applied[1].from, 2);
        assert_eq!(report.steps_applied[1].to, 3);

        // Check data was migrated
        assert_eq!(data["schema_version"], 3);
        assert_eq!(data["tables"]["tasks"][0]["new_field"], "default");
    }

    #[test]
    fn test_migrate_export_data_missing_version() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration);

        // No schema_version field - should default to 1
        let mut data = json!({
            "tables": {}
        });

        let report = migrate_export_data(&registry, &mut data, 2).unwrap();
        assert!(report.was_migrated);
        assert_eq!(report.from_version, 1);
    }

    #[test]
    fn test_migrate_export_data_export_newer_fails() {
        let registry = MigrationRegistry::new();
        let mut data = json!({
            "schema_version": 5,
            "tables": {}
        });

        let result = migrate_export_data(&registry, &mut data, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_migration_report_summary() {
        let report = MigrationReport {
            from_version: 1,
            to_version: 3,
            steps_applied: vec![
                MigrationStep {
                    from: 1,
                    to: 2,
                    description: "Add foo".to_string(),
                },
                MigrationStep {
                    from: 2,
                    to: 3,
                    description: "Add bar".to_string(),
                },
            ],
            was_migrated: true,
        };

        let summary = report.summary();
        assert!(summary.contains("v1 to v3"));
        assert!(summary.contains("2 steps"));
    }

    #[test]
    fn test_schema_compatibility_describe() {
        let registry = MigrationRegistry::new()
            .register(1, 2, "v1 to v2", noop_migration);

        let compatible = check_schema_compatibility(&registry, 2, 2);
        assert!(compatible.describe().contains("no migration needed"));

        let needs_migration = check_schema_compatibility(&registry, 1, 2);
        assert!(needs_migration.describe().contains("Migration required"));

        let export_newer = check_schema_compatibility(&registry, 5, 3);
        assert!(export_newer.describe().contains("newer than database"));
    }

    // =========================================================================
    // Schema Validation Tests
    // =========================================================================

    #[test]
    fn test_validate_import_schema_compatible() {
        // When versions match, should be valid
        let result = validate_import_schema(3, 3);
        assert!(result.is_valid());
        assert!(result.message().contains("compatible"));
    }

    #[test]
    fn test_validate_import_schema_export_newer() {
        // Export newer than database should fail
        let result = validate_import_schema(5, 3);
        assert!(!result.is_valid());
        assert!(result.message().contains("newer"));
    }

    #[test]
    fn test_schema_validation_result_messages() {
        // Compatible case
        let result = validate_import_schema(3, 3);
        let msg = result.message();
        assert!(msg.contains("export v3"));
        assert!(msg.contains("database v3"));
        assert!(msg.contains("compatible"));

        // Export newer case
        let result = validate_import_schema(5, 3);
        let msg = result.message();
        assert!(msg.contains("export v5"));
        assert!(msg.contains("database v3"));
        assert!(msg.contains("newer") || msg.contains("incompatible"));
    }
}
