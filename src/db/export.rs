//! Export functionality for the task-graph database.
//!
//! Provides methods to serialize database tables for structured export.
//! Each table is queried with deterministic ordering to produce
//! stable, diffable output.

/// Tables excluded from export (ephemeral/runtime state).
///
/// These tables contain runtime state that should not be version-controlled:
/// - `workers`: Session-based worker registrations
/// - `file_locks`: Active file marks (advisory locks)
/// - `claim_sequence`: File lock audit log (runtime coordination)
/// - `tasks_fts`: Full-text search virtual table (rebuilt on import)
/// - `attachments_fts`: Full-text search virtual table (rebuilt on import)
pub const EPHEMERAL_TABLES: &[&str] = &[
    "workers",
    "file_locks",
    "claim_sequence",
    "tasks_fts",
    "attachments_fts",
];

/// Tables included in export (project data).
///
/// These tables contain project data that should be version-controlled:
pub const PROJECT_TABLES: &[&str] = &[
    "tasks",
    "dependencies",
    "attachments",
    "task_tags",
    "task_needed_tags",
    "task_wanted_tags",
    "task_sequence",
];

use crate::types::{
    Attachment, Dependency, ExportTables, TaskNeededTagRow, TaskSequenceEvent, TaskTagRow,
    TaskWantedTagRow,
};
use anyhow::Result;

use super::Database;
use super::tasks::parse_task_row;

/// Options for controlling export behavior.
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// If true, exclude soft-deleted tasks (where deleted_at is set).
    pub exclude_deleted: bool,
    /// Optional list of specific tables to export. If None, export all tables.
    pub tables: Option<Vec<String>>,
}

impl Database {
    /// Export all project data tables to an ExportTables struct.
    ///
    /// Tables are queried with deterministic ordering per the export spec:
    /// - tasks: ORDER BY id
    /// - dependencies: ORDER BY from_task_id, to_task_id, dep_type
    /// - attachments: ORDER BY task_id, attachment_type, sequence
    /// - task_tags: ORDER BY task_id, tag
    /// - task_needed_tags: ORDER BY task_id, tag
    /// - task_wanted_tags: ORDER BY task_id, tag
    /// - task_sequence: ORDER BY task_id, id
    pub fn export_tables(&self, options: &ExportOptions) -> Result<ExportTables> {
        let tables_to_export = options.tables.as_ref();

        let should_export =
            |table: &str| -> bool { tables_to_export.is_none_or(|t| t.iter().any(|s| s == table)) };

        let mut export = ExportTables::default();

        if should_export("tasks") {
            export.tasks = Some(self.export_tasks(options.exclude_deleted)?);
        }

        if should_export("dependencies") {
            export.dependencies = Some(self.export_dependencies()?);
        }

        if should_export("attachments") {
            export.attachments = Some(self.export_attachments()?);
        }

        if should_export("task_tags") {
            export.task_tags = Some(self.export_task_tags()?);
        }

        if should_export("task_needed_tags") {
            export.task_needed_tags = Some(self.export_task_needed_tags()?);
        }

        if should_export("task_wanted_tags") {
            export.task_wanted_tags = Some(self.export_task_wanted_tags()?);
        }

        if should_export("task_sequence") {
            export.task_sequence = Some(self.export_task_sequence()?);
        }

        Ok(export)
    }

    /// Export all tasks ordered by id.
    fn export_tasks(&self, exclude_deleted: bool) -> Result<Vec<crate::types::Task>> {
        self.with_conn(|conn| {
            let sql = if exclude_deleted {
                "SELECT * FROM tasks WHERE deleted_at IS NULL ORDER BY id"
            } else {
                "SELECT * FROM tasks ORDER BY id"
            };

            let mut stmt = conn.prepare(sql)?;
            let tasks = stmt
                .query_map([], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        })
    }

    /// Export all dependencies ordered by from_task_id, to_task_id, dep_type.
    fn export_dependencies(&self) -> Result<Vec<Dependency>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT from_task_id, to_task_id, dep_type 
                 FROM dependencies 
                 ORDER BY from_task_id, to_task_id, dep_type",
            )?;

            let deps = stmt
                .query_map([], |row| {
                    Ok(Dependency {
                        from_task_id: row.get(0)?,
                        to_task_id: row.get(1)?,
                        dep_type: row.get(2)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(deps)
        })
    }

    /// Export all attachments ordered by task_id, attachment_type, sequence.
    fn export_attachments(&self) -> Result<Vec<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at
                 FROM attachments
                 ORDER BY task_id, attachment_type, sequence",
            )?;

            let attachments = stmt
                .query_map([], |row| {
                    Ok(Attachment {
                        task_id: row.get(0)?,
                        attachment_type: row.get(1)?,
                        sequence: row.get(2)?,
                        name: row.get(3)?,
                        mime_type: row.get(4)?,
                        content: row.get(5)?,
                        file_path: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(attachments)
        })
    }

    /// Export all task tags ordered by task_id, tag.
    fn export_task_tags(&self) -> Result<Vec<TaskTagRow>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT task_id, tag FROM task_tags ORDER BY task_id, tag")?;

            let tags = stmt
                .query_map([], |row| {
                    Ok(TaskTagRow {
                        task_id: row.get(0)?,
                        tag: row.get(1)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tags)
        })
    }

    /// Export all task needed tags ordered by task_id, tag.
    fn export_task_needed_tags(&self) -> Result<Vec<TaskNeededTagRow>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT task_id, tag FROM task_needed_tags ORDER BY task_id, tag")?;

            let tags = stmt
                .query_map([], |row| {
                    Ok(TaskNeededTagRow {
                        task_id: row.get(0)?,
                        tag: row.get(1)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tags)
        })
    }

    /// Export all task wanted tags ordered by task_id, tag.
    fn export_task_wanted_tags(&self) -> Result<Vec<TaskWantedTagRow>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT task_id, tag FROM task_wanted_tags ORDER BY task_id, tag")?;

            let tags = stmt
                .query_map([], |row| {
                    Ok(TaskWantedTagRow {
                        task_id: row.get(0)?,
                        tag: row.get(1)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tags)
        })
    }

    /// Export all task sequence events ordered by task_id, id.
    fn export_task_sequence(&self) -> Result<Vec<TaskSequenceEvent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, worker_id, status, phase, reason, timestamp, end_timestamp
                 FROM task_sequence
                 ORDER BY task_id, id",
            )?;

            let events = stmt
                .query_map([], |row| {
                    Ok(TaskSequenceEvent {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        worker_id: row.get(2)?,
                        status: row.get(3)?,
                        phase: row.get(4)?,
                        reason: row.get(5)?,
                        timestamp: row.get(6)?,
                        end_timestamp: row.get(7)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(events)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DependenciesConfig, IdsConfig, StatesConfig};

    fn default_states_config() -> StatesConfig {
        StatesConfig::default()
    }

    fn default_deps_config() -> DependenciesConfig {
        DependenciesConfig::default()
    }

    #[test]
    fn test_export_empty_database() {
        let db = Database::open_in_memory().unwrap();
        let options = ExportOptions::default();
        let export = db.export_tables(&options).unwrap();

        assert!(export.tasks.as_ref().unwrap().is_empty());
        assert!(export.dependencies.as_ref().unwrap().is_empty());
        assert!(export.attachments.as_ref().unwrap().is_empty());
        assert!(export.task_tags.as_ref().unwrap().is_empty());
        assert!(export.task_needed_tags.as_ref().unwrap().is_empty());
        assert!(export.task_wanted_tags.as_ref().unwrap().is_empty());
        assert!(export.task_sequence.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_export_selective_tables() {
        let db = Database::open_in_memory().unwrap();
        let options = ExportOptions {
            exclude_deleted: false,
            tables: Some(vec!["tasks".to_string(), "dependencies".to_string()]),
        };
        let export = db.export_tables(&options).unwrap();

        // Selected tables should be Some
        assert!(export.tasks.is_some());
        assert!(export.dependencies.is_some());

        // Non-selected tables should be None
        assert!(export.attachments.is_none());
        assert!(export.task_tags.is_none());
        assert!(export.task_needed_tags.is_none());
        assert!(export.task_wanted_tags.is_none());
        assert!(export.task_sequence.is_none());
    }

    #[test]
    fn test_export_tasks_ordered_by_id() {
        let db = Database::open_in_memory().unwrap();
        let states_config = default_states_config();

        // Create tasks with IDs that would be out of order alphabetically if not sorted
        db.create_task(
            Some("z-task".to_string()),
            "Z Task".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();
        db.create_task(
            Some("a-task".to_string()),
            "A Task".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();
        db.create_task(
            Some("m-task".to_string()),
            "M Task".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();

        let options = ExportOptions::default();
        let export = db.export_tables(&options).unwrap();
        let tasks = export.tasks.unwrap();

        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "a-task");
        assert_eq!(tasks[1].id, "m-task");
        assert_eq!(tasks[2].id, "z-task");
    }

    #[test]
    fn test_export_excludes_deleted_tasks_when_requested() {
        let db = Database::open_in_memory().unwrap();
        let states_config = default_states_config();

        // Create a normal task
        db.create_task(
            Some("task-1".to_string()),
            "Task 1".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();

        // Create and delete a task
        db.create_task(
            Some("task-2".to_string()),
            "Task 2".to_string(),
            None,
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();
        // Soft delete: task_id, worker_id, cascade, reason, obliterate, force
        db.delete_task("task-2", "test-worker", false, None, false, true)
            .unwrap();

        // Export without excluding deleted
        let options = ExportOptions {
            exclude_deleted: false,
            tables: None,
        };
        let export = db.export_tables(&options).unwrap();
        assert_eq!(export.tasks.as_ref().unwrap().len(), 2);

        // Export with excluding deleted
        let options = ExportOptions {
            exclude_deleted: true,
            tables: None,
        };
        let export = db.export_tables(&options).unwrap();
        assert_eq!(export.tasks.as_ref().unwrap().len(), 1);
        assert_eq!(export.tasks.as_ref().unwrap()[0].id, "task-1");
    }

    #[test]
    fn test_export_dependencies_ordered() {
        let db = Database::open_in_memory().unwrap();
        let states_config = default_states_config();
        let deps_config = default_deps_config();

        // Create tasks first
        for id in ["a", "b", "c"] {
            db.create_task(
                Some(id.to_string()),
                format!("Task {}", id),
                None,
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states_config,
                &IdsConfig::default(),
            )
            .unwrap();
        }

        // Add dependencies in non-sorted order
        db.add_dependency("c", "a", "blocks", &deps_config).unwrap();
        db.add_dependency("a", "b", "follows", &deps_config)
            .unwrap();
        db.add_dependency("a", "b", "blocks", &deps_config).unwrap();

        let options = ExportOptions::default();
        let export = db.export_tables(&options).unwrap();
        let deps = export.dependencies.unwrap();

        assert_eq!(deps.len(), 3);
        // Should be ordered by from_task_id, to_task_id, dep_type
        assert_eq!(
            (
                deps[0].from_task_id.as_str(),
                deps[0].to_task_id.as_str(),
                deps[0].dep_type.as_str()
            ),
            ("a", "b", "blocks")
        );
        assert_eq!(
            (
                deps[1].from_task_id.as_str(),
                deps[1].to_task_id.as_str(),
                deps[1].dep_type.as_str()
            ),
            ("a", "b", "follows")
        );
        assert_eq!(
            (
                deps[2].from_task_id.as_str(),
                deps[2].to_task_id.as_str(),
                deps[2].dep_type.as_str()
            ),
            ("c", "a", "blocks")
        );
    }

    #[test]
    fn test_export_task_tags_ordered() {
        let db = Database::open_in_memory().unwrap();
        let states_config = default_states_config();

        // Create tasks with tags in various orders
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
            None,                                                 // wanted_tags
            Some(vec!["zebra".to_string(), "apple".to_string()]), // tags
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();
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
            None,                            // wanted_tags
            Some(vec!["mango".to_string()]), // tags
            &states_config,
            &IdsConfig::default(),
        )
        .unwrap();

        let options = ExportOptions::default();
        let export = db.export_tables(&options).unwrap();
        let tags = export.task_tags.unwrap();

        assert_eq!(tags.len(), 3);
        // Should be ordered by task_id, then tag
        assert_eq!(
            (tags[0].task_id.as_str(), tags[0].tag.as_str()),
            ("task-a", "mango")
        );
        assert_eq!(
            (tags[1].task_id.as_str(), tags[1].tag.as_str()),
            ("task-b", "apple")
        );
        assert_eq!(
            (tags[2].task_id.as_str(), tags[2].tag.as_str()),
            ("task-b", "zebra")
        );
    }
}
