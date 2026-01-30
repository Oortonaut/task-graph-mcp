//! Template instantiation system for the task-graph database.
//!
//! Templates are Snapshot-format JSON files that define reusable task structures.
//! Instantiation creates fresh copies with new IDs while preserving the internal
//! dependency graph, optionally attaching the template root to a parent task.
//!
//! # Entry and Exit Points
//!
//! - **Entry points**: Root tasks within the template (tasks that have no parent
//!   within the template itself). These are the "top" of the template hierarchy.
//! - **Exit points**: Tasks with external dependency targets (references to task IDs
//!   not present in the template). These represent integration boundaries.
//!
//! # Instantiation Flow
//!
//! 1. Load the template from a Snapshot JSON file
//! 2. Validate schema compatibility
//! 3. Detect entry/exit points from the template structure
//! 4. Remap all IDs to fresh petname-based IDs
//! 5. Record template metadata (source, original IDs, mapping)
//! 6. Optionally attach entry points to a parent task
//! 7. Import into the database via merge mode

use crate::config::IdsConfig;
use crate::export::Snapshot;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::Database;
use super::import::{ImportMode, ImportOptions, ImportResult, remap_snapshot};

/// Metadata about a template, extracted during analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMetadata {
    /// Name of the template (derived from filename or explicit).
    pub name: String,

    /// Source file path the template was loaded from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,

    /// Entry point task IDs (root tasks with no parent in the template).
    /// These are the original IDs from the template file.
    pub entry_points: Vec<String>,

    /// Exit point task IDs (tasks with external dependency references).
    /// These are the original IDs from the template file.
    pub exit_points: Vec<String>,

    /// Total number of tasks in the template.
    pub task_count: usize,

    /// Total number of dependencies in the template.
    pub dependency_count: usize,

    /// Tags found across all tasks in the template.
    pub all_tags: Vec<String>,
}

/// Options for controlling template instantiation.
#[derive(Debug, Clone, Default)]
pub struct InstantiateOptions {
    /// Parent task ID to attach the template's entry points to.
    /// A "contains" dependency will be created from parent to each entry point.
    pub parent_task_id: Option<String>,

    /// Dependency type to use when attaching to parent (default: "contains").
    pub attach_dep_type: String,

    /// Optional prefix to add to task titles for disambiguation.
    pub title_prefix: Option<String>,

    /// Additional tags to add to all instantiated tasks.
    pub extra_tags: Vec<String>,

    /// Whether to reset all task statuses to the initial state.
    /// Default: true (templates are instantiated as fresh work).
    pub reset_status: bool,

    /// Override the initial status for instantiated tasks.
    /// If None, uses the config's initial state.
    pub initial_status: Option<String>,
}

impl InstantiateOptions {
    /// Create default instantiation options.
    pub fn new() -> Self {
        Self {
            attach_dep_type: "contains".to_string(),
            reset_status: true,
            ..Default::default()
        }
    }

    /// Set the parent task ID (builder pattern).
    pub fn with_parent(mut self, parent_id: &str) -> Self {
        self.parent_task_id = Some(parent_id.to_string());
        self
    }

    /// Set a title prefix (builder pattern).
    pub fn with_title_prefix(mut self, prefix: &str) -> Self {
        self.title_prefix = Some(prefix.to_string());
        self
    }

    /// Add extra tags to all instantiated tasks (builder pattern).
    pub fn with_extra_tags(mut self, tags: Vec<String>) -> Self {
        self.extra_tags = tags;
        self
    }
}

/// Result of a template instantiation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantiateResult {
    /// Template metadata (from the source template).
    pub metadata: TemplateMetadata,

    /// ID remapping table: original template ID -> new database ID.
    pub id_map: HashMap<String, String>,

    /// New IDs of the entry point tasks (after remapping).
    pub entry_point_ids: Vec<String>,

    /// New IDs of the exit point tasks (after remapping).
    pub exit_point_ids: Vec<String>,

    /// Import statistics from the database insertion.
    pub import_stats: ImportStats,

    /// Parent task ID if attachment was performed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_to_parent: Option<String>,
}

/// Simplified import stats for the instantiation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStats {
    pub tasks_imported: usize,
    pub dependencies_imported: usize,
    pub tags_imported: usize,
    pub total_rows: usize,
}

impl From<&ImportResult> for ImportStats {
    fn from(result: &ImportResult) -> Self {
        Self {
            tasks_imported: *result.rows_imported.get("tasks").unwrap_or(&0),
            dependencies_imported: *result.rows_imported.get("dependencies").unwrap_or(&0),
            tags_imported: result.rows_imported.get("task_tags").unwrap_or(&0)
                + result.rows_imported.get("task_needed_tags").unwrap_or(&0)
                + result.rows_imported.get("task_wanted_tags").unwrap_or(&0),
            total_rows: result.total_rows(),
        }
    }
}

/// Analyze a template snapshot to extract metadata without modifying it.
///
/// This identifies entry points (root tasks), exit points (external references),
/// and collects summary statistics about the template.
///
/// # Arguments
/// * `snapshot` - The template snapshot to analyze
/// * `name` - Template name (for metadata)
/// * `source_path` - Optional source file path
pub fn analyze_template(
    snapshot: &Snapshot,
    name: &str,
    source_path: Option<&str>,
) -> Result<TemplateMetadata> {
    // Collect all task IDs in the template
    let task_ids: HashSet<String> = snapshot
        .tables
        .get("tasks")
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|t| t.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if task_ids.is_empty() {
        return Err(anyhow!("Template contains no tasks"));
    }

    // Find which tasks are children (have a parent within the template)
    // A task is a child if it appears as to_task_id in a vertical (contains) dependency
    let mut child_task_ids: HashSet<String> = HashSet::new();
    let mut exit_point_ids: HashSet<String> = HashSet::new();

    if let Some(deps) = snapshot.tables.get("dependencies") {
        for dep in deps {
            let from_id = dep
                .get("from_task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let to_id = dep.get("to_task_id").and_then(|v| v.as_str()).unwrap_or("");
            let dep_type = dep.get("dep_type").and_then(|v| v.as_str()).unwrap_or("");

            // Track children (tasks contained by other template tasks)
            if dep_type == "contains" && task_ids.contains(from_id) && task_ids.contains(to_id) {
                child_task_ids.insert(to_id.to_string());
            }

            // Track exit points (dependencies that reference tasks outside the template)
            if !task_ids.contains(from_id) || !task_ids.contains(to_id) {
                // The task that IS in the template is an exit point
                if task_ids.contains(from_id) {
                    exit_point_ids.insert(from_id.to_string());
                }
                if task_ids.contains(to_id) {
                    exit_point_ids.insert(to_id.to_string());
                }
            }
        }
    }

    // Entry points are tasks that are not children of any other template task
    let entry_points: Vec<String> = task_ids
        .iter()
        .filter(|id| !child_task_ids.contains(*id))
        .cloned()
        .collect();

    let exit_points: Vec<String> = exit_point_ids.into_iter().collect();

    // Collect all unique tags
    let mut all_tags: HashSet<String> = HashSet::new();
    if let Some(tags) = snapshot.tables.get("task_tags") {
        for tag_row in tags {
            if let Some(tag) = tag_row.get("tag").and_then(|v| v.as_str()) {
                all_tags.insert(tag.to_string());
            }
        }
    }
    let mut all_tags: Vec<String> = all_tags.into_iter().collect();
    all_tags.sort();

    let dependency_count = snapshot
        .tables
        .get("dependencies")
        .map(|d| d.len())
        .unwrap_or(0);

    Ok(TemplateMetadata {
        name: name.to_string(),
        source_path: source_path.map(String::from),
        entry_points,
        exit_points,
        task_count: task_ids.len(),
        dependency_count,
        all_tags,
    })
}

/// Prepare a template snapshot for instantiation by remapping IDs and optionally
/// modifying task properties. Returns the prepared snapshot and ID mapping.
///
/// This is the core transformation step:
/// 1. Remap all task IDs to fresh petname IDs
/// 2. Optionally reset task statuses to initial state
/// 3. Optionally prefix task titles
/// 4. Optionally add extra tags
/// 5. Reset timestamps to current time
/// 6. Clear runtime fields (worker_id, claimed_at, thoughts, etc.)
fn prepare_snapshot(
    snapshot: &Snapshot,
    ids_config: &IdsConfig,
    options: &InstantiateOptions,
) -> Result<(Snapshot, HashMap<String, String>)> {
    // Phase 1: Remap all IDs using the existing remap_snapshot function
    let (mut prepared, id_map) =
        remap_snapshot(snapshot, ids_config).context("Failed to remap template IDs")?;

    let now_ms = chrono::Utc::now().timestamp_millis();

    // Phase 2: Apply template instantiation transformations
    if let Some(tasks) = prepared.tables.get_mut("tasks") {
        for task_row in tasks.iter_mut() {
            if let Some(obj) = task_row.as_object_mut() {
                // Reset status if requested
                if options.reset_status {
                    let status = options.initial_status.as_deref().unwrap_or("pending");
                    obj.insert("status".to_string(), Value::String(status.to_string()));
                }

                // Prefix titles if requested
                if let Some(ref prefix) = options.title_prefix
                    && let Some(title) = obj.get("title").and_then(|v| v.as_str())
                {
                    obj.insert(
                        "title".to_string(),
                        Value::String(format!("{}: {}", prefix, title)),
                    );
                }

                // Clear runtime fields
                obj.insert("worker_id".to_string(), Value::Null);
                obj.insert("claimed_at".to_string(), Value::Null);
                obj.insert("current_thought".to_string(), Value::Null);
                obj.insert("started_at".to_string(), Value::Null);
                obj.insert("completed_at".to_string(), Value::Null);
                obj.insert("time_actual_ms".to_string(), Value::Null);
                obj.insert("cost_usd".to_string(), serde_json::json!(0.0));
                obj.insert(
                    "metrics".to_string(),
                    serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0]),
                );

                // Update timestamps to now
                obj.insert("created_at".to_string(), serde_json::json!(now_ms));
                obj.insert("updated_at".to_string(), serde_json::json!(now_ms));
            }
        }
    }

    // Phase 3: Add extra tags if requested
    if !options.extra_tags.is_empty() {
        // Get all task IDs from the prepared (remapped) snapshot
        let task_ids: Vec<String> = prepared
            .tables
            .get("tasks")
            .map(|tasks| {
                tasks
                    .iter()
                    .filter_map(|t| t.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Add extra tags for each task
        let tag_rows = prepared
            .tables
            .entry("task_tags".to_string())
            .or_insert_with(Vec::new);

        for task_id in &task_ids {
            for tag in &options.extra_tags {
                tag_rows.push(serde_json::json!({
                    "task_id": task_id,
                    "tag": tag,
                }));
            }
        }
    }

    // Phase 4: Clear task_sequence (state history is not relevant for instantiated templates)
    prepared
        .tables
        .insert("task_sequence".to_string(), Vec::new());

    Ok((prepared, id_map))
}

/// List available templates from a directory.
///
/// Scans the given directory for .json files that are valid Snapshot-format templates.
/// Returns metadata for each discovered template.
pub fn list_templates(templates_dir: &Path) -> Result<Vec<TemplateMetadata>> {
    let mut templates = Vec::new();

    if !templates_dir.exists() {
        return Ok(templates);
    }

    let entries = std::fs::read_dir(templates_dir)
        .with_context(|| format!("Failed to read templates directory: {:?}", templates_dir))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Only process .json files
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        // Try to load and analyze the template
        match Snapshot::from_file(&path) {
            Ok(snapshot) => {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                match analyze_template(&snapshot, &name, path.to_str()) {
                    Ok(metadata) => templates.push(metadata),
                    Err(e) => {
                        // Skip invalid templates but log a warning
                        eprintln!("Warning: Template {:?} has invalid structure: {}", path, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to load template {:?}: {}", path, e);
            }
        }
    }

    // Sort by name for deterministic ordering
    templates.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(templates)
}

impl Database {
    /// Instantiate a template from a Snapshot into the database.
    ///
    /// This is the main entry point for template instantiation:
    /// 1. Analyzes the template to detect entry/exit points
    /// 2. Remaps all IDs to fresh petname-based IDs
    /// 3. Applies instantiation transformations (status reset, title prefix, etc.)
    /// 4. Imports the prepared snapshot via merge mode
    /// 5. Optionally attaches entry points to a parent task
    ///
    /// # Arguments
    /// * `snapshot` - The template snapshot to instantiate
    /// * `name` - Template name (for metadata/tracking)
    /// * `source_path` - Optional source file path
    /// * `ids_config` - ID generation configuration
    /// * `options` - Instantiation options
    ///
    /// # Returns
    /// * `Ok(InstantiateResult)` - Instantiation results including ID mapping
    /// * `Err` - If instantiation fails
    pub fn instantiate_template(
        &self,
        snapshot: &Snapshot,
        name: &str,
        source_path: Option<&str>,
        ids_config: &IdsConfig,
        options: &InstantiateOptions,
    ) -> Result<InstantiateResult> {
        // Step 1: Analyze the template to find entry/exit points
        let metadata = analyze_template(snapshot, name, source_path)?;

        // Step 2: Validate parent task exists if specified
        if let Some(ref parent_id) = options.parent_task_id
            && !self.task_exists(parent_id)?
        {
            return Err(anyhow!(
                "Parent task '{}' not found. Cannot attach template.",
                parent_id
            ));
        }

        // Step 3: Prepare the snapshot (remap IDs, apply transformations)
        let (prepared_snapshot, id_map) = prepare_snapshot(snapshot, ids_config, options)?;

        // Step 4: Map entry/exit points to new IDs
        let entry_point_ids: Vec<String> = metadata
            .entry_points
            .iter()
            .filter_map(|old_id| id_map.get(old_id).cloned())
            .collect();

        let exit_point_ids: Vec<String> = metadata
            .exit_points
            .iter()
            .filter_map(|old_id| id_map.get(old_id).cloned())
            .collect();

        // Step 5: Import the prepared snapshot using merge mode
        let import_options = ImportOptions {
            mode: ImportMode::Merge,
            remap_ids: false,
            parent_id: None,
        };
        let import_result = self
            .import_snapshot(&prepared_snapshot, &import_options)
            .context("Failed to import instantiated template")?;

        let import_stats = ImportStats::from(&import_result);

        // Step 6: Attach entry points to parent task if specified
        if let Some(ref parent_id) = options.parent_task_id {
            self.attach_template_to_parent(parent_id, &entry_point_ids, &options.attach_dep_type)?;
        }

        Ok(InstantiateResult {
            metadata,
            id_map,
            entry_point_ids,
            exit_point_ids,
            import_stats,
            attached_to_parent: options.parent_task_id.clone(),
        })
    }

    /// Instantiate a template from a file path.
    ///
    /// Convenience method that loads the snapshot from a file and delegates
    /// to `instantiate_template`.
    pub fn instantiate_template_file(
        &self,
        template_path: &Path,
        ids_config: &IdsConfig,
        options: &InstantiateOptions,
    ) -> Result<InstantiateResult> {
        let snapshot = Snapshot::from_file(template_path)
            .with_context(|| format!("Failed to load template from {:?}", template_path))?;

        let name = template_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        self.instantiate_template(
            &snapshot,
            &name,
            template_path.to_str(),
            ids_config,
            options,
        )
    }

    /// Attach template entry points to a parent task via dependency links.
    ///
    /// Creates a dependency of the specified type from the parent to each
    /// entry point task.
    fn attach_template_to_parent(
        &self,
        parent_id: &str,
        entry_point_ids: &[String],
        dep_type: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            for entry_id in entry_point_ids {
                conn.execute(
                    "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type) VALUES (?1, ?2, ?3)",
                    rusqlite::params![parent_id, entry_id, dep_type],
                )?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IdsConfig;
    use crate::export::{CURRENT_SCHEMA_VERSION, EXPORT_VERSION, Snapshot};
    use std::collections::BTreeMap;

    /// Create a minimal test template snapshot.
    fn make_test_template() -> Snapshot {
        let mut tables = BTreeMap::new();

        // Two tasks: a root and a child
        tables.insert(
            "tasks".to_string(),
            vec![
                serde_json::json!({
                    "id": "tpl-root",
                    "title": "Root Task",
                    "description": "The root of the template",
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": [],
                    "wanted_tags": [],
                    "tags": ["template"],
                    "points": null,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "cost_usd": 0.0,
                    "metrics": [0,0,0,0,0,0,0,0],
                    "created_at": 1000000,
                    "updated_at": 1000000
                }),
                serde_json::json!({
                    "id": "tpl-child-1",
                    "title": "Child Task 1",
                    "description": "First child",
                    "status": "pending",
                    "priority": "5",
                    "worker_id": null,
                    "claimed_at": null,
                    "needed_tags": [],
                    "wanted_tags": [],
                    "tags": ["child"],
                    "points": 3,
                    "time_estimate_ms": null,
                    "time_actual_ms": null,
                    "started_at": null,
                    "completed_at": null,
                    "current_thought": null,
                    "cost_usd": 0.0,
                    "metrics": [0,0,0,0,0,0,0,0],
                    "created_at": 1000001,
                    "updated_at": 1000001
                }),
            ],
        );

        // Root contains child
        tables.insert(
            "dependencies".to_string(),
            vec![serde_json::json!({
                "from_task_id": "tpl-root",
                "to_task_id": "tpl-child-1",
                "dep_type": "contains"
            })],
        );

        tables.insert(
            "task_tags".to_string(),
            vec![
                serde_json::json!({"task_id": "tpl-root", "tag": "template"}),
                serde_json::json!({"task_id": "tpl-child-1", "tag": "child"}),
            ],
        );

        tables.insert("attachments".to_string(), Vec::new());
        tables.insert("task_needed_tags".to_string(), Vec::new());
        tables.insert("task_wanted_tags".to_string(), Vec::new());
        tables.insert("task_sequence".to_string(), Vec::new());

        Snapshot {
            schema_version: CURRENT_SCHEMA_VERSION,
            export_version: EXPORT_VERSION.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            exported_by: "test-template".to_string(),
            tables,
        }
    }

    #[test]
    fn test_analyze_template_entry_points() {
        let snapshot = make_test_template();
        let metadata = analyze_template(&snapshot, "test-template", None).unwrap();

        // Root task should be the only entry point (child is contained)
        assert_eq!(metadata.entry_points.len(), 1);
        assert!(metadata.entry_points.contains(&"tpl-root".to_string()));
        assert_eq!(metadata.task_count, 2);
        assert_eq!(metadata.dependency_count, 1);
    }

    #[test]
    fn test_analyze_template_exit_points() {
        let mut snapshot = make_test_template();

        // Add an external dependency (child blocks an external task)
        if let Some(deps) = snapshot.tables.get_mut("dependencies") {
            deps.push(serde_json::json!({
                "from_task_id": "tpl-child-1",
                "to_task_id": "external-task-123",
                "dep_type": "blocks"
            }));
        }

        let metadata = analyze_template(&snapshot, "test-template", None).unwrap();

        // child-1 should be an exit point because it references an external task
        assert!(metadata.exit_points.contains(&"tpl-child-1".to_string()));
    }

    #[test]
    fn test_analyze_empty_template() {
        let snapshot = Snapshot {
            schema_version: CURRENT_SCHEMA_VERSION,
            export_version: EXPORT_VERSION.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            exported_by: "test".to_string(),
            tables: BTreeMap::new(),
        };

        let result = analyze_template(&snapshot, "empty", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no tasks"));
    }

    #[test]
    fn test_prepare_snapshot_remaps_ids() {
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new();

        let (prepared, id_map) = prepare_snapshot(&snapshot, &ids_config, &options).unwrap();

        // All original IDs should be remapped
        assert!(id_map.contains_key("tpl-root"));
        assert!(id_map.contains_key("tpl-child-1"));

        // New IDs should be different from originals
        assert_ne!(id_map["tpl-root"], "tpl-root");
        assert_ne!(id_map["tpl-child-1"], "tpl-child-1");

        // Prepared snapshot should use new IDs
        let tasks = prepared.tables.get("tasks").unwrap();
        let task_ids: Vec<&str> = tasks
            .iter()
            .filter_map(|t| t.get("id").and_then(|v| v.as_str()))
            .collect();
        assert!(!task_ids.contains(&"tpl-root"));
        assert!(task_ids.contains(&id_map["tpl-root"].as_str()));
    }

    #[test]
    fn test_prepare_snapshot_resets_status() {
        let mut snapshot = make_test_template();

        // Set tasks to non-pending status
        if let Some(tasks) = snapshot.tables.get_mut("tasks") {
            for task in tasks.iter_mut() {
                if let Some(obj) = task.as_object_mut() {
                    obj.insert("status".to_string(), Value::String("completed".to_string()));
                    obj.insert(
                        "worker_id".to_string(),
                        Value::String("old-worker".to_string()),
                    );
                }
            }
        }

        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new(); // reset_status = true by default

        let (prepared, _) = prepare_snapshot(&snapshot, &ids_config, &options).unwrap();

        // All tasks should be reset to pending
        let tasks = prepared.tables.get("tasks").unwrap();
        for task in tasks {
            assert_eq!(task.get("status").and_then(|v| v.as_str()), Some("pending"));
            // Runtime fields should be cleared
            assert!(task.get("worker_id").unwrap().is_null());
            assert!(task.get("claimed_at").unwrap().is_null());
        }
    }

    #[test]
    fn test_prepare_snapshot_title_prefix() {
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new().with_title_prefix("Sprint-1");

        let (prepared, _) = prepare_snapshot(&snapshot, &ids_config, &options).unwrap();

        let tasks = prepared.tables.get("tasks").unwrap();
        for task in tasks {
            let title = task.get("title").and_then(|v| v.as_str()).unwrap();
            assert!(
                title.starts_with("Sprint-1: "),
                "Title should be prefixed: {}",
                title
            );
        }
    }

    #[test]
    fn test_prepare_snapshot_extra_tags() {
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options =
            InstantiateOptions::new().with_extra_tags(vec!["sprint-1".into(), "team-a".into()]);

        let (prepared, _) = prepare_snapshot(&snapshot, &ids_config, &options).unwrap();

        // Should have original tags + extra tags for each task
        let tags = prepared.tables.get("task_tags").unwrap();
        // Original: 2 tags + extra: 2 tasks * 2 tags = 4 new tag rows
        assert!(
            tags.len() >= 6,
            "Expected at least 6 tag rows, got {}",
            tags.len()
        );
    }

    #[test]
    fn test_prepare_snapshot_clears_sequence() {
        let mut snapshot = make_test_template();

        // Add some state history
        snapshot.tables.insert(
            "task_sequence".to_string(),
            vec![serde_json::json!({
                "id": 1,
                "task_id": "tpl-root",
                "worker_id": "old-worker",
                "status": "working",
                "phase": null,
                "reason": "started",
                "timestamp": 1000000,
                "end_timestamp": 1000100
            })],
        );

        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new();

        let (prepared, _) = prepare_snapshot(&snapshot, &ids_config, &options).unwrap();

        // State history should be cleared
        let sequence = prepared.tables.get("task_sequence").unwrap();
        assert!(
            sequence.is_empty(),
            "task_sequence should be empty after instantiation"
        );
    }

    #[test]
    fn test_instantiate_template_integration() {
        let db = Database::open_in_memory().unwrap();
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new();

        let result = db
            .instantiate_template(&snapshot, "test-template", None, &ids_config, &options)
            .unwrap();

        // Verify metadata
        assert_eq!(result.metadata.name, "test-template");
        assert_eq!(result.metadata.task_count, 2);

        // Verify entry points were mapped
        assert_eq!(result.entry_point_ids.len(), 1);

        // Verify tasks were imported
        assert_eq!(result.import_stats.tasks_imported, 2);
        assert_eq!(result.import_stats.dependencies_imported, 1);

        // Verify ID mapping is complete
        assert_eq!(result.id_map.len(), 2);

        // Verify tasks exist in the database
        for new_id in result.id_map.values() {
            assert!(
                db.task_exists(new_id).unwrap(),
                "Task {} should exist",
                new_id
            );
        }
    }

    #[test]
    fn test_instantiate_with_parent() {
        let db = Database::open_in_memory().unwrap();

        // Create a parent task first
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, title, status, priority, cost_usd, created_at, updated_at)
                 VALUES ('parent-task', 'Parent', 'pending', 5, 0.0, 1000000, 1000000)",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new().with_parent("parent-task");

        let result = db
            .instantiate_template(&snapshot, "test-template", None, &ids_config, &options)
            .unwrap();

        // Verify parent attachment
        assert_eq!(result.attached_to_parent, Some("parent-task".to_string()));

        // Verify the dependency was created
        let has_dep: bool = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT 1 FROM dependencies WHERE from_task_id = 'parent-task' AND dep_type = 'contains'",
                    [],
                    |_| Ok(true),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .unwrap_or(false);
        assert!(
            has_dep,
            "Parent should have a contains dependency to entry point"
        );
    }

    #[test]
    fn test_instantiate_with_invalid_parent() {
        let db = Database::open_in_memory().unwrap();
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new().with_parent("nonexistent-parent");

        let result =
            db.instantiate_template(&snapshot, "test-template", None, &ids_config, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_multiple_instantiations_unique_ids() {
        let db = Database::open_in_memory().unwrap();
        let snapshot = make_test_template();
        let ids_config = IdsConfig::default();
        let options = InstantiateOptions::new();

        // Instantiate the same template twice
        let result1 = db
            .instantiate_template(&snapshot, "test-1", None, &ids_config, &options)
            .unwrap();
        let result2 = db
            .instantiate_template(&snapshot, "test-2", None, &ids_config, &options)
            .unwrap();

        // IDs should be different between instantiations
        let ids1: HashSet<&String> = result1.id_map.values().collect();
        let ids2: HashSet<&String> = result2.id_map.values().collect();

        assert!(
            ids1.is_disjoint(&ids2),
            "Multiple instantiations should produce unique IDs"
        );

        // Both should have all tasks imported
        assert_eq!(result1.import_stats.tasks_imported, 2);
        assert_eq!(result2.import_stats.tasks_imported, 2);
    }
}
