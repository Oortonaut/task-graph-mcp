//! Task CRUD and tree operations.

use super::state_transitions::record_state_transition;
use super::{Database, now_ms};
use crate::config::{
    AutoAdvanceConfig, DependenciesConfig, IdsConfig, PhasesConfig, StatesConfig, TagsConfig,
};
use crate::error::ToolError;
use crate::types::{
    PRIORITY_DEFAULT, Priority, Task, TaskTree, TaskTreeInput, Worker, clamp_priority,
    parse_priority,
};
use anyhow::{Result, anyhow};
use petname::{Generator, Petnames};
use rusqlite::{Connection, Row, params};

/// Options for creating a task tree from nested input.
#[derive(Debug)]
pub struct CreateTreeOptions<'a> {
    pub input: TaskTreeInput,
    pub parent_id: Option<String>,
    pub child_type: Option<String>,
    pub sibling_type: Option<String>,
    pub states_config: &'a StatesConfig,
    pub phases_config: &'a PhasesConfig,
    pub tags_config: &'a TagsConfig,
    pub ids_config: &'a IdsConfig,
}

/// Query parameters for listing tasks with optional filters.
#[derive(Debug, Default)]
pub struct ListTasksQuery<'a> {
    pub status: Option<&'a str>,
    pub phase: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub parent_id: Option<Option<&'a str>>,
    pub limit: Option<i32>,
    pub offset: i32,
    pub sort_by: Option<&'a str>,
    pub sort_order: Option<&'a str>,
}

/// Generate a petname-based task ID using the large wordlist.
/// Uses the configured number of words and case style.
fn generate_task_id(ids_config: &IdsConfig) -> String {
    let words = ids_config.task_id_words;
    let case = ids_config.id_case;

    // Generate with hyphen separator first (petname's default format)
    let base = Petnames::medium()
        .generate_one(words, "-")
        .unwrap_or_else(|| format!("task-{}", super::now_ms()));

    // Convert to desired case
    case.convert(&base)
}

/// Build an ORDER BY clause from sort_by and sort_order parameters.
/// Returns a safe SQL ORDER BY expression.
fn build_order_clause(sort_by: Option<&str>, sort_order: Option<&str>) -> String {
    let field = match sort_by {
        Some("priority") => "CAST(t.priority AS INTEGER)",
        Some("created_at") => "t.created_at",
        Some("updated_at") => "t.updated_at",
        _ => "t.created_at", // default
    };

    let order = match sort_order {
        Some("asc") => "ASC",
        Some("desc") => "DESC",
        _ => {
            // Default: priority is descending (higher number = more important), dates are descending
            "DESC"
        }
    };

    format!("{} {}", field, order)
}

// =============================================================================
// Junction table helpers for tag management
// =============================================================================

/// Sync task tags to the task_tags junction table.
/// Replaces all existing tags for the task.
fn sync_task_tags(conn: &Connection, task_id: &str, tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM task_tags WHERE task_id = ?1", params![task_id])?;
    for tag in tags {
        conn.execute(
            "INSERT INTO task_tags (task_id, tag) VALUES (?1, ?2)",
            params![task_id, tag],
        )?;
    }
    Ok(())
}

/// Sync needed tags (agent must have ALL) to the task_needed_tags junction table.
fn sync_needed_tags(conn: &Connection, task_id: &str, tags: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM task_needed_tags WHERE task_id = ?1",
        params![task_id],
    )?;
    for tag in tags {
        conn.execute(
            "INSERT INTO task_needed_tags (task_id, tag) VALUES (?1, ?2)",
            params![task_id, tag],
        )?;
    }
    Ok(())
}

/// Sync wanted tags (agent must have ANY) to the task_wanted_tags junction table.
fn sync_wanted_tags(conn: &Connection, task_id: &str, tags: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM task_wanted_tags WHERE task_id = ?1",
        params![task_id],
    )?;
    for tag in tags {
        conn.execute(
            "INSERT INTO task_wanted_tags (task_id, tag) VALUES (?1, ?2)",
            params![task_id, tag],
        )?;
    }
    Ok(())
}

pub fn parse_task_row(row: &Row) -> rusqlite::Result<Task> {
    let id: String = row.get("id")?;
    let title: String = row.get("title")?;
    let description: Option<String> = row.get("description")?;
    let status: String = row.get("status")?;
    let phase: Option<String> = row.get("phase")?;
    let priority: String = row.get("priority")?;
    let worker_id: Option<String> = row.get("worker_id")?;
    let claimed_at: Option<i64> = row.get("claimed_at")?;

    let needed_tags_json: Option<String> = row.get("needed_tags")?;
    let wanted_tags_json: Option<String> = row.get("wanted_tags")?;
    let tags_json: Option<String> = row.get("tags")?;

    let points: Option<i32> = row.get("points")?;
    let time_estimate_ms: Option<i64> = row.get("time_estimate_ms")?;
    let time_actual_ms: Option<i64> = row.get("time_actual_ms")?;
    let started_at: Option<i64> = row.get("started_at")?;
    let completed_at: Option<i64> = row.get("completed_at")?;

    let current_thought: Option<String> = row.get("current_thought")?;

    let cost_usd: f64 = row.get("cost_usd")?;
    let metric_0: i64 = row.get("metric_0")?;
    let metric_1: i64 = row.get("metric_1")?;
    let metric_2: i64 = row.get("metric_2")?;
    let metric_3: i64 = row.get("metric_3")?;
    let metric_4: i64 = row.get("metric_4")?;
    let metric_5: i64 = row.get("metric_5")?;
    let metric_6: i64 = row.get("metric_6")?;
    let metric_7: i64 = row.get("metric_7")?;

    let created_at: i64 = row.get("created_at")?;
    let updated_at: i64 = row.get("updated_at")?;

    Ok(Task {
        id,
        title,
        description,
        status,
        phase,
        priority: parse_priority(&priority),
        worker_id,
        claimed_at,
        needed_tags: needed_tags_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default(),
        wanted_tags: wanted_tags_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default(),
        tags: tags_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default(),
        points,
        time_estimate_ms,
        time_actual_ms,
        started_at,
        completed_at,
        current_thought,
        cost_usd,
        metrics: [
            metric_0, metric_1, metric_2, metric_3, metric_4, metric_5, metric_6, metric_7,
        ],
        created_at,
        updated_at,
    })
}

/// Internal helper to get a task using an existing connection (avoids deadlock).
fn get_task_internal(conn: &Connection, task_id: &str) -> Result<Option<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;

    let result = stmt.query_row(params![task_id], parse_task_row);

    match result {
        Ok(task) => Ok(Some(task)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Internal helper to get a worker using an existing connection (avoids deadlock).
fn get_worker_internal(conn: &Connection, worker_id: &str) -> Result<Option<Worker>> {
    let mut stmt = conn.prepare(
        "SELECT id, tags, max_claims, registered_at, last_heartbeat, last_status, last_phase, workflow
         FROM workers WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![worker_id], |row| {
        let id: String = row.get(0)?;
        let tags_json: String = row.get(1)?;
        let max_claims: i32 = row.get(2)?;
        let registered_at: i64 = row.get(3)?;
        let last_heartbeat: i64 = row.get(4)?;
        let last_status: Option<String> = row.get(5)?;
        let last_phase: Option<String> = row.get(6)?;
        let workflow: Option<String> = row.get(7)?;

        Ok((
            id,
            tags_json,
            max_claims,
            registered_at,
            last_heartbeat,
            last_status,
            last_phase,
            workflow,
        ))
    });

    match result {
        Ok((
            id,
            tags_json,
            max_claims,
            registered_at,
            last_heartbeat,
            last_status,
            last_phase,
            workflow,
        )) => {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(Some(Worker {
                id,
                tags,
                max_claims,
                registered_at,
                last_heartbeat,
                last_status,
                last_phase,
                workflow,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

impl Database {
    /// Create a new task.
    /// If id is provided, uses it as the task ID; otherwise generates a petname ID.
    /// If parent_id is provided, creates a 'contains' dependency from parent to this task.
    /// `title` is the short task title; `description` is optional longer detail.
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        id: Option<String>,
        title: String,
        description: Option<String>,
        parent_id: Option<String>,
        phase: Option<String>,
        priority: Option<Priority>,
        points: Option<i32>,
        time_estimate_ms: Option<i64>,
        agent_tags_all: Option<Vec<String>>,
        agent_tags_any: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        states_config: &StatesConfig,
        ids_config: &IdsConfig,
    ) -> Result<Task> {
        let task_id = id.unwrap_or_else(|| generate_task_id(ids_config));
        let now = now_ms();
        let priority = clamp_priority(priority.unwrap_or(PRIORITY_DEFAULT));
        let initial_status = &states_config.initial;

        let needed_tags = agent_tags_all.unwrap_or_default();
        let wanted_tags = agent_tags_any.unwrap_or_default();
        let tags = tags.unwrap_or_default();
        let needed_tags_json = serde_json::to_string(&needed_tags)?;
        let wanted_tags_json = serde_json::to_string(&wanted_tags)?;
        let tags_json = serde_json::to_string(&tags)?;

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            tx.execute(
                "INSERT INTO tasks (
                    id, title, description, status, phase, priority,
                    needed_tags, wanted_tags, tags, points, time_estimate_ms, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    &task_id,
                    &title,
                    &description,
                    initial_status,
                    &phase,
                    priority.to_string(),
                    needed_tags_json,
                    wanted_tags_json,
                    tags_json,
                    points,
                    time_estimate_ms,
                    now,
                    now,
                ],
            )?;

            // Sync tags to junction tables
            sync_task_tags(&tx, &task_id, &tags)?;
            sync_needed_tags(&tx, &task_id, &needed_tags)?;
            sync_wanted_tags(&tx, &task_id, &wanted_tags)?;

            // Create 'contains' dependency if parent_id is provided
            if let Some(ref pid) = parent_id {
                Database::add_dependency_internal(&tx, pid, &task_id, "contains")?;
            }

            // Record initial state
            record_state_transition(&tx, &task_id, initial_status, None, None, states_config)?;

            tx.commit()?;

            Ok(Task {
                id: task_id,
                title,
                description,
                status: initial_status.clone(),
                phase,
                priority,
                worker_id: None,
                claimed_at: None,
                needed_tags,
                wanted_tags,
                tags,
                points,
                time_estimate_ms,
                time_actual_ms: None,
                started_at: None,
                completed_at: None,
                current_thought: None,
                cost_usd: 0.0,
                metrics: [0; 8],
                created_at: now,
                updated_at: now,
            })
        })
    }

    /// Convenience method to create a task with just a description string.
    /// Uses the description as both title and description. Useful for tests.
    pub fn create_task_simple(
        &self,
        description: impl Into<String>,
        states_config: &StatesConfig,
        ids_config: &IdsConfig,
    ) -> Result<Task> {
        let desc = description.into();
        self.create_task(
            None,
            desc.clone(),
            Some(desc),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            states_config,
            ids_config,
        )
    }

    /// Create a task tree from nested input.
    /// Uses child_type for parent-child dependencies (default: "contains").
    /// Uses sibling_type for sibling dependencies (default: none/parallel).
    #[allow(clippy::type_complexity)]
    pub fn create_task_tree(
        &self,
        opts: CreateTreeOptions<'_>,
    ) -> Result<(String, Vec<String>, Vec<String>, Vec<String>)> {
        let mut all_ids = Vec::new();
        let mut phase_warnings = Vec::new();
        let mut tag_warnings = Vec::new();
        // Default child_type to "contains" if not specified
        let child_type = opts.child_type.or_else(|| Some("contains".to_string()));

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            let root_id = create_tree_recursive(
                &tx,
                &opts.input,
                opts.parent_id.as_deref(),
                None, // no previous sibling for root
                child_type.as_deref(),
                opts.sibling_type.as_deref(),
                &mut all_ids,
                &mut phase_warnings,
                &mut tag_warnings,
                opts.states_config,
                opts.phases_config,
                opts.tags_config,
                opts.ids_config,
            )?;
            tx.commit()?;
            Ok((root_id, all_ids, phase_warnings, tag_warnings))
        })
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;

            let result = stmt.query_row(params![task_id], parse_task_row);

            match result {
                Ok(task) => Ok(Some(task)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Rename a task's ID, updating all references atomically.
    ///
    /// Disables foreign key enforcement, updates every table that references
    /// `tasks.id` inside a transaction, then re-enables and verifies FK
    /// integrity.
    pub fn rename_task(&self, old_id: &str, new_id: &str) -> Result<()> {
        // Validate new_id
        if new_id.is_empty() {
            return Err(anyhow!("new_id must not be empty"));
        }
        if new_id.len() > 64 {
            return Err(anyhow!("new_id must not exceed 64 characters"));
        }

        self.with_conn_mut(|conn| {
            // Pre-check: old_id must exist
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
                params![old_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(anyhow!("Task '{}' not found", old_id));
            }

            // Pre-check: new_id must not already exist
            let conflict: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
                params![new_id],
                |row| row.get(0),
            )?;
            if conflict {
                return Err(anyhow!("Task '{}' already exists", new_id));
            }

            // Disable FK enforcement for the duration of the update
            conn.execute_batch("PRAGMA foreign_keys = OFF")?;

            let result = (|| -> Result<()> {
                let tx = conn.transaction()?;

                // Primary table — triggers tasks_fts_update
                tx.execute(
                    "UPDATE tasks SET id = ?1 WHERE id = ?2",
                    params![new_id, old_id],
                )?;

                // Attachments — triggers attachments_fts_update per row
                tx.execute(
                    "UPDATE attachments SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;

                // Dependencies (both columns)
                tx.execute(
                    "UPDATE dependencies SET from_task_id = ?1 WHERE from_task_id = ?2",
                    params![new_id, old_id],
                )?;
                tx.execute(
                    "UPDATE dependencies SET to_task_id = ?1 WHERE to_task_id = ?2",
                    params![new_id, old_id],
                )?;

                // File locks
                tx.execute(
                    "UPDATE file_locks SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;

                // Tag junction tables
                tx.execute(
                    "UPDATE task_tags SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;
                tx.execute(
                    "UPDATE task_needed_tags SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;
                tx.execute(
                    "UPDATE task_wanted_tags SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;

                // Sequence table
                tx.execute(
                    "UPDATE task_sequence SET task_id = ?1 WHERE task_id = ?2",
                    params![new_id, old_id],
                )?;

                tx.commit()?;
                Ok(())
            })();

            // Re-enable FK enforcement regardless of success
            conn.execute_batch("PRAGMA foreign_keys = ON")?;

            // Propagate any error from the transaction
            result?;

            // Verify FK integrity
            let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
            let violations: Vec<String> = stmt
                .query_map([], |row| {
                    let table: String = row.get(0)?;
                    Ok(table)
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !violations.is_empty() {
                return Err(anyhow!(
                    "Foreign key violations after rename in tables: {:?}",
                    violations
                ));
            }

            Ok(())
        })
    }

    /// Get a task with all its children (tree).
    pub fn get_task_tree(&self, task_id: &str) -> Result<Option<TaskTree>> {
        let task = self.get_task(task_id)?;
        match task {
            None => Ok(None),
            Some(task) => {
                let children = self.get_children_recursive(&task.id)?;
                Ok(Some(TaskTree { task, children }))
            }
        }
    }

    /// Get children recursively.
    fn get_children_recursive(&self, parent_id: &str) -> Result<Vec<TaskTree>> {
        let children = self.get_children(parent_id)?;
        let mut result = Vec::new();

        for child in children {
            let child_children = self.get_children_recursive(&child.id)?;
            result.push(TaskTree {
                task: child,
                children: child_children,
            });
        }

        Ok(result)
    }

    /// Get direct children of a task (via 'contains' dependency).
    pub fn get_children(&self, parent_id: &str) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT t.* FROM tasks t
                 INNER JOIN dependencies d ON t.id = d.to_task_id
                 WHERE d.from_task_id = ?1 AND d.dep_type = 'contains'
                 ORDER BY t.created_at",
            )?;

            let tasks = stmt
                .query_map(params![parent_id], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Update a task.
    #[allow(clippy::too_many_arguments)]
    pub fn update_task(
        &self,
        task_id: &str,
        title: Option<String>,
        description: Option<Option<String>>,
        status: Option<String>,
        priority: Option<Priority>,
        points: Option<Option<i32>>,
        tags: Option<Vec<String>>,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            let new_title = title.unwrap_or(task.title.clone());
            let new_description = description.unwrap_or(task.description.clone());
            let new_status = status.unwrap_or(task.status.clone());
            let new_priority = priority.unwrap_or(task.priority);
            let new_points = points.unwrap_or(task.points);
            let new_tags = tags.unwrap_or(task.tags.clone());

            // Validate the new status exists
            if !states_config.is_valid_state(&new_status) {
                return Err(anyhow!(
                    "Invalid state '{}'. Valid states: {:?}",
                    new_status,
                    states_config.state_names()
                ));
            }

            // Validate state transition if status changed
            if task.status != new_status
                && !states_config.is_valid_transition(&task.status, &new_status)
            {
                let exits = states_config.get_exits(&task.status);
                return Err(anyhow!(
                    "Invalid transition from '{}' to '{}'. Allowed transitions: {:?}",
                    task.status,
                    new_status,
                    exits
                ));
            }

            // Handle status transitions for timestamps
            // Set started_at when first entering a timed state
            let started_at =
                if task.started_at.is_none() && states_config.is_timed_state(&new_status) {
                    Some(now)
                } else {
                    task.started_at
                };

            // Set completed_at when entering a completed state (terminal or with reopen capability)
            let completed_at = if new_status == "completed" {
                Some(now)
            } else {
                task.completed_at
            };

            // Record state transition if status changed (handles time accumulation)
            if task.status != new_status {
                record_state_transition(
                    conn,
                    task_id,
                    &new_status,
                    task.worker_id.as_deref(),
                    None,
                    states_config,
                )?;
            }

            conn.execute(
                "UPDATE tasks SET
                    title = ?1, description = ?2, status = ?3, priority = ?4,
                    points = ?5, started_at = ?6, completed_at = ?7, updated_at = ?8,
                    tags = ?9
                WHERE id = ?10",
                params![
                    new_title,
                    new_description,
                    new_status,
                    new_priority.to_string(),
                    new_points,
                    started_at,
                    completed_at,
                    now,
                    serde_json::to_string(&new_tags)?,
                    task_id,
                ],
            )?;

            Ok(Task {
                id: task_id.to_string(),
                title: new_title,
                description: new_description,
                status: new_status,
                priority: new_priority,
                points: new_points,
                tags: new_tags,
                started_at,
                completed_at,
                updated_at: now,
                ..task
            })
        })
    }

    /// Update a task with unified claim/release logic.
    /// - Transition to timed state = CLAIM (set owner, validate tags, check limit)
    /// - Transition from timed to non-timed = RELEASE (clear owner)
    /// - Transition to terminal = COMPLETE (check children, release file locks)
    /// - With assignee = ASSIGN (set owner to assignee, transition to 'assigned' state)
    /// - Only the owner can update a claimed task (unless force=true)
    ///
    /// Returns (task, unblocked, auto_advanced):
    /// - task: The updated task
    /// - unblocked: Task IDs that are now ready (all dependencies satisfied)
    /// - auto_advanced: Subset of unblocked that were actually transitioned
    #[allow(clippy::too_many_arguments)]
    pub fn update_task_unified(
        &self,
        task_id: &str,
        agent_id: &str,
        assignee: Option<&str>,
        title: Option<String>,
        description: Option<Option<String>>,
        status: Option<String>,
        phase: Option<String>,
        priority: Option<Priority>,
        points: Option<Option<i32>>,
        tags: Option<Vec<String>>,
        needed_tags: Option<Vec<String>>,
        wanted_tags: Option<Vec<String>>,
        time_estimate_ms: Option<i64>,
        reason: Option<String>,
        force: bool,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
        auto_advance: &AutoAdvanceConfig,
    ) -> Result<(Task, Vec<String>, Vec<String>)> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            let task =
                get_task_internal(&tx, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Owner-only validation: if task is claimed, only owner can update (unless force)
            if let Some(ref current_owner) = task.worker_id
                && current_owner != agent_id && !force {
                    return Err(anyhow!(
                        "Task is claimed by agent '{}'. Only the owner can update claimed tasks (use force=true to override)",
                        current_owner
                    ));
                }

            let new_title = title.unwrap_or(task.title.clone());
            let new_description = description.unwrap_or(task.description.clone());
            // If assignee is set but no explicit status, default to 'assigned' state
            let new_status = if assignee.is_some() && status.is_none() {
                "assigned".to_string()
            } else {
                status.unwrap_or(task.status.clone())
            };
            let new_priority = priority.unwrap_or(task.priority);
            let new_points = points.unwrap_or(task.points);
            let new_tags = tags.unwrap_or(task.tags.clone());
            let new_needed_tags = needed_tags.unwrap_or(task.needed_tags.clone());
            let new_wanted_tags = wanted_tags.unwrap_or(task.wanted_tags.clone());
            let new_time_estimate_ms = time_estimate_ms.or(task.time_estimate_ms);
            let new_phase = phase.or(task.phase.clone());

            // Validate the new status exists
            if !states_config.is_valid_state(&new_status) {
                return Err(anyhow!(
                    "Invalid state '{}'. Valid states: {:?}",
                    new_status,
                    states_config.state_names()
                ));
            }

            // Validate state transition if status changed
            if task.status != new_status
                && !states_config.is_valid_transition(&task.status, &new_status) {
                    let exits = states_config.get_exits(&task.status);
                    return Err(anyhow!(
                        "Invalid transition from '{}' to '{}'. Allowed transitions: {:?}",
                        task.status,
                        new_status,
                        exits
                    ));
                }

            // Determine ownership changes based on state transition
            let new_is_timed = states_config.is_timed_state(&new_status);
            let new_is_terminal = states_config.is_terminal_state(&new_status);
            let current_owner = task.worker_id.as_deref();
            let is_owned_by_agent = current_owner == Some(agent_id);
            let is_owned_by_other = current_owner.is_some() && !is_owned_by_agent;

            let mut new_owner: Option<String> = task.worker_id.clone();
            let mut new_claimed_at: Option<i64> = task.claimed_at;

            // ASSIGN: Push coordination - coordinator assigns task to another agent
            // Sets owner without starting the timer (assigned state is untimed)
            if let Some(target_agent) = assignee {
                // Verify task is not already claimed (unless force)
                if is_owned_by_other && !force {
                    return Err(anyhow!(
                        "Task is already claimed by agent '{}'. Use force=true to reassign.",
                        current_owner.unwrap()
                    ));
                }

                // Verify the assignee exists
                let target = get_worker_internal(&tx, target_agent)?
                    .ok_or_else(|| anyhow!("Assignee agent '{}' not found", target_agent))?;

                // Check tag affinity for the assignee
                if !task.needed_tags.is_empty() {
                    for needed in &task.needed_tags {
                        if !target.tags.contains(needed) {
                            return Err(anyhow!(
                                "Assignee '{}' missing required tag: {}",
                                target_agent,
                                needed
                            ));
                        }
                    }
                }

                if !task.wanted_tags.is_empty() {
                    let has_any = task
                        .wanted_tags
                        .iter()
                        .any(|wanted| target.tags.contains(wanted));
                    if !has_any {
                        return Err(anyhow!(
                            "Assignee '{}' has none of the wanted tags: {:?}",
                            target_agent,
                            task.wanted_tags
                        ));
                    }
                }

                // Set ownership to the assignee
                new_owner = Some(target_agent.to_string());
                new_claimed_at = Some(now);
            }

            // CLAIM: Transitioning to a timed state and need to take ownership
            // This handles: non-timed -> timed, OR timed (other owner) -> timed (force claim)
            if new_is_timed && !is_owned_by_agent {
                // Already claimed by someone else?
                if is_owned_by_other && !force {
                    return Err(anyhow!(
                        "Task is already claimed by agent '{}'",
                        current_owner.unwrap()
                    ));
                }

                // Check for unsatisfied blocking dependencies (skip if force)
                if !force {
                    let unsatisfied_blockers = super::deps::get_unsatisfied_start_blockers_in_tx(
                        &tx,
                        task_id,
                        states_config,
                        deps_config,
                    )?;
                    if !unsatisfied_blockers.is_empty() {
                        // Return structured error with blocking task IDs so clients
                        // can monitor them and retry when they complete
                        return Err(ToolError::deps_not_satisfied(&unsatisfied_blockers).into());
                    }
                }

                // Get the agent
                let agent = get_worker_internal(&tx, agent_id)?
                    .ok_or_else(|| anyhow!("Agent not found"))?;

                // Check tag affinity - needed_tags (AND - must have ALL)
                if !task.needed_tags.is_empty() {
                    for needed in &task.needed_tags {
                        if !agent.tags.contains(needed) {
                            return Err(anyhow!("Agent missing required tag: {}", needed));
                        }
                    }
                }

                // Check tag affinity - wanted_tags (OR - must have AT LEAST ONE)
                if !task.wanted_tags.is_empty() {
                    let has_any = task
                        .wanted_tags
                        .iter()
                        .any(|wanted| agent.tags.contains(wanted));
                    if !has_any {
                        return Err(anyhow!("Agent has none of the wanted tags"));
                    }
                }

                // Set ownership
                new_owner = Some(agent_id.to_string());
                new_claimed_at = Some(now);

                // Refresh agent heartbeat
                tx.execute(
                    "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                    params![now, agent_id],
                )?;
            }

            // RELEASE: Transitioning to non-timed state (but not terminal)
            if !new_is_timed && !new_is_terminal && task.worker_id.is_some() {
                // Verify ownership (unless force)
                if is_owned_by_other && !force {
                    return Err(anyhow!("Task is not owned by this agent"));
                }

                // Clear ownership
                new_owner = None;
                new_claimed_at = None;
            }

            // COMPLETE: Transition to terminal state
            if new_is_terminal {
                // Verify ownership if task was claimed (unless force)
                if let Some(ref current_owner) = task.worker_id
                    && current_owner != agent_id && !force {
                        return Err(anyhow!("Task is not owned by this agent"));
                    }

                // Check for incomplete children (via 'contains' dependencies)
                let incomplete_children: i32 = tx.query_row(
                    "SELECT COUNT(*) FROM dependencies d
                     INNER JOIN tasks child ON d.to_task_id = child.id
                     WHERE d.from_task_id = ?1 AND d.dep_type = 'contains'
                     AND child.status IN (SELECT value FROM json_each(?2))",
                    params![
                        task_id,
                        serde_json::to_string(&states_config.blocking_states)?
                    ],
                    |row| row.get(0),
                )?;

                if incomplete_children > 0 {
                    return Err(anyhow!(
                        "Cannot complete task: {} child task(s) are not complete",
                        incomplete_children
                    ));
                }

                // Clear ownership
                new_owner = None;
                new_claimed_at = None;

                // Release file locks associated with this task (for auto-cleanup)
                tx.execute(
                    "DELETE FROM file_locks WHERE task_id = ?1",
                    params![task_id],
                )?;
            }

            // Handle timestamps
            let started_at =
                if task.started_at.is_none() && new_is_timed {
                    Some(now)
                } else {
                    task.started_at
                };

            // Set completed_at when entering completed status (even if it has reopen exits)
            let completed_at = if new_status == "completed" {
                Some(now)
            } else {
                task.completed_at
            };

            // Record state transition if status changed (with reason for audit)
            let status_changed = task.status != new_status;
            if status_changed {
                record_state_transition(
                    &tx,
                    task_id,
                    &new_status,
                    new_owner.as_deref(),
                    reason.as_deref(),
                    states_config,
                )?;
            }

            // Record phase transition if phase changed
            let phase_changed = task.phase != new_phase;
            if phase_changed {
                super::state_transitions::record_phase_transition(
                    &tx,
                    task_id,
                    new_phase.as_deref().unwrap_or(""),
                    Some(agent_id),
                    reason.as_deref(),
                )?;
            }

            tx.execute(
                "UPDATE tasks SET
                    title = ?1, description = ?2, status = ?3, phase = ?4, priority = ?5,
                    points = ?6, started_at = ?7, completed_at = ?8, updated_at = ?9,
                    tags = ?10, worker_id = ?11, claimed_at = ?12,
                    needed_tags = ?13, wanted_tags = ?14, time_estimate_ms = ?15
                WHERE id = ?16",
                params![
                    new_title,
                    new_description,
                    new_status,
                    new_phase,
                    new_priority.to_string(),
                    new_points,
                    started_at,
                    completed_at,
                    now,
                    serde_json::to_string(&new_tags)?,
                    new_owner,
                    new_claimed_at,
                    serde_json::to_string(&new_needed_tags)?,
                    serde_json::to_string(&new_wanted_tags)?,
                    new_time_estimate_ms,
                    task_id,
                ],
            )?;

            // Sync tags to junction tables if changed
            if new_tags != task.tags {
                sync_task_tags(&tx, task_id, &new_tags)?;
            }
            if new_needed_tags != task.needed_tags {
                sync_needed_tags(&tx, task_id, &new_needed_tags)?;
            }
            if new_wanted_tags != task.wanted_tags {
                sync_wanted_tags(&tx, task_id, &new_wanted_tags)?;
            }

            // Check for unblocked tasks if this task transitioned FROM blocking TO non-blocking
            let (unblocked, auto_advanced) = if status_changed {
                let was_blocking = states_config.is_blocking_state(&task.status);
                let is_blocking = states_config.is_blocking_state(&new_status);

                if was_blocking && !is_blocking {
                    super::deps::propagate_unblock_effects(
                        &tx,
                        task_id,
                        Some(agent_id),
                        states_config,
                        deps_config,
                        auto_advance,
                    )?
                } else {
                    (vec![], vec![])
                }
            } else {
                (vec![], vec![])
            };

            tx.commit()?;

            Ok((Task {
                id: task_id.to_string(),
                title: new_title,
                description: new_description,
                status: new_status,
                phase: new_phase,
                priority: new_priority,
                points: new_points,
                tags: new_tags,
                needed_tags: new_needed_tags,
                wanted_tags: new_wanted_tags,
                time_estimate_ms: new_time_estimate_ms,
                started_at,
                completed_at,
                updated_at: now,
                worker_id: new_owner,
                claimed_at: new_claimed_at,
                ..task
            }, unblocked, auto_advanced))
        })
    }

    /// Delete a task (soft delete by default, hard delete with obliterate=true).
    ///
    /// - `worker_id`: The worker attempting to delete (required for ownership check)
    /// - `cascade`: Whether to delete children (default: false)
    /// - `reason`: Optional reason for deletion
    /// - `obliterate`: If true, permanently deletes the task; if false (default), soft deletes
    /// - `force`: If true, allows deletion even if owned by another worker
    pub fn delete_task(
        &self,
        task_id: &str,
        worker_id: &str,
        cascade: bool,
        reason: Option<String>,
        obliterate: bool,
        force: bool,
    ) -> Result<()> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get the task to check ownership
            let task = get_task_internal(&tx, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            // Check ownership - reject if claimed by another worker (unless force)
            if let Some(ref owner) = task.worker_id
                && owner != worker_id && !force {
                    return Err(anyhow!(
                        "Task is claimed by worker '{}'. Use force=true to override.",
                        owner
                    ));
                }

            if obliterate {
                // Hard delete - permanently remove from database
                if cascade {
                    // Find all descendants using recursive CTE and delete them
                    // The CTE finds all tasks reachable via 'contains' dependencies
                    tx.execute(
                        "WITH RECURSIVE descendants AS (
                            SELECT ?1 AS id
                            UNION ALL
                            SELECT dep.to_task_id FROM dependencies dep
                            INNER JOIN descendants d ON dep.from_task_id = d.id
                            WHERE dep.dep_type = 'contains'
                        )
                        DELETE FROM tasks WHERE id IN (SELECT id FROM descendants)",
                        params![task_id],
                    )?;
                } else {
                    // Check for children via dependencies
                    let child_count: i32 = tx.query_row(
                        "SELECT COUNT(*) FROM dependencies WHERE from_task_id = ?1 AND dep_type = 'contains'",
                        params![task_id],
                        |row| row.get(0),
                    )?;

                    if child_count > 0 {
                        return Err(anyhow!("Task has children; use cascade=true to delete"));
                    }

                    tx.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
                }
            } else {
                // Soft delete - set deleted_at, deleted_by, deleted_reason
                if cascade {
                    // Soft delete all descendants
                    tx.execute(
                        "WITH RECURSIVE descendants AS (
                            SELECT ?1 AS id
                            UNION ALL
                            SELECT dep.to_task_id FROM dependencies dep
                            INNER JOIN descendants d ON dep.from_task_id = d.id
                            WHERE dep.dep_type = 'contains'
                        )
                        UPDATE tasks SET deleted_at = ?2, deleted_by = ?3, deleted_reason = ?4, updated_at = ?2
                        WHERE id IN (SELECT id FROM descendants) AND deleted_at IS NULL",
                        params![task_id, now, worker_id, reason],
                    )?;
                } else {
                    // Check for children via dependencies
                    let child_count: i32 = tx.query_row(
                        "SELECT COUNT(*) FROM dependencies WHERE from_task_id = ?1 AND dep_type = 'contains'",
                        params![task_id],
                        |row| row.get(0),
                    )?;

                    if child_count > 0 {
                        return Err(anyhow!("Task has children; use cascade=true to delete"));
                    }

                    tx.execute(
                        "UPDATE tasks SET deleted_at = ?1, deleted_by = ?2, deleted_reason = ?3, updated_at = ?1 WHERE id = ?4",
                        params![now, worker_id, reason, task_id],
                    )?;
                }
            }

            tx.commit()?;
            Ok(())
        })
    }

    /// List tasks with optional filters.
    /// Returns full Task objects. Excludes soft-deleted tasks.
    pub fn list_tasks(&self, query: ListTasksQuery<'_>) -> Result<Vec<Task>> {
        let ListTasksQuery {
            status,
            phase,
            owner,
            parent_id,
            limit,
            offset,
            sort_by,
            sort_order,
        } = query;
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT t.* FROM tasks t WHERE t.deleted_at IS NULL",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(s) = status {
                sql.push_str(" AND t.status = ?");
                params_vec.push(Box::new(s.to_string()));
            }

            if let Some(p) = phase {
                sql.push_str(" AND t.phase = ?");
                params_vec.push(Box::new(p.to_string()));
            }

            if let Some(o) = owner {
                sql.push_str(" AND t.worker_id = ?");
                params_vec.push(Box::new(o.to_string()));
            }

            // Handle parent filtering via dependencies table
            if let Some(p) = parent_id {
                match p {
                    Some(pid) => {
                        sql.push_str(" AND t.id IN (SELECT to_task_id FROM dependencies WHERE from_task_id = ? AND dep_type = 'contains')");
                        params_vec.push(Box::new(pid.to_string()));
                    }
                    None => {
                        // Root tasks: not contained by any other task
                        sql.push_str(" AND t.id NOT IN (SELECT to_task_id FROM dependencies WHERE dep_type = 'contains')");
                    }
                }
            }

            // Build ORDER BY clause
            let order_clause = build_order_clause(sort_by, sort_order);
            sql.push_str(&format!(" ORDER BY {}", order_clause));

            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }

            if offset > 0 {
                sql.push_str(&format!(" OFFSET {}", offset));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let tasks = stmt
                .query_map(params_refs.as_slice(), parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Set the current thought for tasks owned by an agent.
    pub fn set_thought(
        &self,
        agent_id: &str,
        thought: Option<String>,
        task_ids: Option<Vec<String>>,
    ) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            let updated = if let Some(ids) = task_ids {
                let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "UPDATE tasks SET current_thought = ?, updated_at = ?
                     WHERE worker_id = ? AND id IN ({})",
                    placeholders.join(", ")
                );

                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                params_vec.push(Box::new(thought.clone()));
                params_vec.push(Box::new(now));
                params_vec.push(Box::new(agent_id.to_string()));
                for id in &ids {
                    params_vec.push(Box::new(id.clone()));
                }

                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();
                conn.execute(&sql, params_refs.as_slice())?
            } else {
                conn.execute(
                    "UPDATE tasks SET current_thought = ?, updated_at = ? WHERE worker_id = ?",
                    params![thought, now, agent_id],
                )?
            };

            Ok(updated as i32)
        })
    }

    /// Log time for a task.
    pub fn log_time(&self, task_id: &str, duration_ms: i64) -> Result<i64> {
        let now = now_ms();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE tasks SET time_actual_ms = COALESCE(time_actual_ms, 0) + ?1, updated_at = ?2
                 WHERE id = ?3",
                params![duration_ms, now, task_id],
            )?;

            let total: i64 = conn.query_row(
                "SELECT COALESCE(time_actual_ms, 0) FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )?;

            Ok(total)
        })
    }

    /// Log metrics and cost for a task.
    /// Values in the metrics array are aggregated (added) to existing values.
    pub fn log_metrics(
        &self,
        task_id: &str,
        cost_usd: Option<f64>,
        values: &[i64],
    ) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Aggregate metrics (add new values to existing)
            let mut new_metrics = task.metrics;
            for (i, &val) in values.iter().take(8).enumerate() {
                new_metrics[i] += val;
            }

            let new_cost_usd = task.cost_usd + cost_usd.unwrap_or(0.0);

            conn.execute(
                "UPDATE tasks SET
                    metric_0 = ?1, metric_1 = ?2, metric_2 = ?3, metric_3 = ?4,
                    metric_4 = ?5, metric_5 = ?6, metric_6 = ?7, metric_7 = ?8,
                    cost_usd = ?9, updated_at = ?10
                WHERE id = ?11",
                params![
                    new_metrics[0],
                    new_metrics[1],
                    new_metrics[2],
                    new_metrics[3],
                    new_metrics[4],
                    new_metrics[5],
                    new_metrics[6],
                    new_metrics[7],
                    new_cost_usd,
                    now,
                    task_id,
                ],
            )?;

            Ok(Task {
                cost_usd: new_cost_usd,
                metrics: new_metrics,
                updated_at: now,
                ..task
            })
        })
    }

    /// Claim a task for an agent.
    /// Uses the first timed state (typically "working") as the claiming state.
    pub fn claim_task(
        &self,
        task_id: &str,
        agent_id: &str,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        // Find the first timed state to use for claiming (typically "working")
        let claim_status = states_config
            .definitions
            .iter()
            .find(|(_, def)| def.timed)
            .map(|(name, _)| name.as_str())
            .unwrap_or("working");

        self.with_conn(|conn| {
            // Get the task (using internal helper to avoid deadlock)
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Check if already claimed
            if task.worker_id.is_some() {
                return Err(anyhow!("Task is already claimed"));
            }

            // Validate state transition
            if !states_config.is_valid_transition(&task.status, claim_status) {
                let exits = states_config.get_exits(&task.status);
                return Err(anyhow!(
                    "Cannot claim task in state '{}'. Allowed transitions: {:?}",
                    task.status,
                    exits
                ));
            }

            // Get the agent (using internal helper to avoid deadlock)
            let agent =
                get_worker_internal(conn, agent_id)?.ok_or_else(|| anyhow!("Agent not found"))?;

            // Check tag affinity - needed_tags (AND - must have ALL)
            if !task.needed_tags.is_empty() {
                for needed in &task.needed_tags {
                    if !agent.tags.contains(needed) {
                        return Err(anyhow!("Agent missing required tag: {}", needed));
                    }
                }
            }

            // Check tag affinity - wanted_tags (OR - must have AT LEAST ONE)
            if !task.wanted_tags.is_empty() {
                let has_any = task
                    .wanted_tags
                    .iter()
                    .any(|wanted| agent.tags.contains(wanted));
                if !has_any {
                    return Err(anyhow!("Agent has none of the wanted tags"));
                }
            }

            conn.execute(
                "UPDATE tasks SET worker_id = ?1, claimed_at = ?2, status = ?3, started_at = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![agent_id, now, claim_status, now, now, task_id,],
            )?;

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(
                conn,
                task_id,
                claim_status,
                Some(agent_id),
                None,
                states_config,
            )?;

            // Refresh agent heartbeat
            conn.execute(
                "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            Ok(Task {
                worker_id: Some(agent_id.to_string()),
                claimed_at: Some(now),
                status: claim_status.to_string(),
                started_at: Some(now),
                updated_at: now,
                ..task
            })
        })
    }

    /// Release a task claim.
    pub fn release_task(
        &self,
        task_id: &str,
        agent_id: &str,
        states_config: &StatesConfig,
    ) -> Result<()> {
        let now = now_ms();
        let release_status = &states_config.initial;

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            if task.worker_id.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(
                conn,
                task_id,
                release_status,
                Some(agent_id),
                None,
                states_config,
            )?;

            conn.execute(
                "UPDATE tasks SET worker_id = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![release_status, now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force release a task regardless of owner.
    pub fn force_release(&self, task_id: &str, states_config: &StatesConfig) -> Result<()> {
        let now = now_ms();
        let release_status = &states_config.initial;

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(
                conn,
                task_id,
                release_status,
                task.worker_id.as_deref(),
                None,
                states_config,
            )?;

            conn.execute(
                "UPDATE tasks SET worker_id = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![release_status, now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force claim a task even if owned by another agent.
    pub fn force_claim_task(
        &self,
        task_id: &str,
        agent_id: &str,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        // Find the first timed state to use for claiming (typically "working")
        let claim_status = states_config
            .definitions
            .iter()
            .find(|(_, def)| def.timed)
            .map(|(name, _)| name.as_str())
            .unwrap_or("working");

        self.with_conn(|conn| {
            // Get the task
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Get the agent
            let agent =
                get_worker_internal(conn, agent_id)?.ok_or_else(|| anyhow!("Agent not found"))?;

            // Check tag affinity - needed_tags (AND)
            if !task.needed_tags.is_empty() {
                for needed in &task.needed_tags {
                    if !agent.tags.contains(needed) {
                        return Err(anyhow!("Agent missing required tag: {}", needed));
                    }
                }
            }

            // Check tag affinity - wanted_tags (OR)
            if !task.wanted_tags.is_empty() {
                let has_any = task
                    .wanted_tags
                    .iter()
                    .any(|wanted| agent.tags.contains(wanted));
                if !has_any {
                    return Err(anyhow!("Agent has none of the wanted tags"));
                }
            }

            conn.execute(
                "UPDATE tasks SET worker_id = ?1, claimed_at = ?2, status = ?3, started_at = COALESCE(started_at, ?4), updated_at = ?5
                 WHERE id = ?6",
                params![agent_id, now, claim_status, now, now, task_id,],
            )?;

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(
                conn,
                task_id,
                claim_status,
                Some(agent_id),
                None,
                states_config,
            )?;

            // Refresh agent heartbeat
            conn.execute(
                "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            Ok(Task {
                worker_id: Some(agent_id.to_string()),
                claimed_at: Some(now),
                status: claim_status.to_string(),
                started_at: task.started_at.or(Some(now)),
                updated_at: now,
                ..task
            })
        })
    }

    /// Release a task claim with a specified state.
    pub fn release_task_with_state(
        &self,
        task_id: &str,
        agent_id: &str,
        state: &str,
        states_config: &StatesConfig,
    ) -> Result<()> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            if task.worker_id.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            // Validate state exists
            if !states_config.is_valid_state(state) {
                return Err(anyhow!(
                    "Invalid state '{}'. Valid states: {:?}",
                    state,
                    states_config.state_names()
                ));
            }

            // Validate transition
            if !states_config.is_valid_transition(&task.status, state) {
                let exits = states_config.get_exits(&task.status);
                return Err(anyhow!(
                    "Invalid transition from '{}' to '{}'. Allowed transitions: {:?}",
                    task.status,
                    state,
                    exits
                ));
            }

            // Set completed_at when entering completed status (even if it has reopen exits)
            let completed_at = if state == "completed" {
                Some(now)
            } else {
                None
            };

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(conn, task_id, state, Some(agent_id), None, states_config)?;

            conn.execute(
                "UPDATE tasks SET worker_id = NULL, claimed_at = NULL, status = ?1, completed_at = COALESCE(?2, completed_at), updated_at = ?3
                 WHERE id = ?4",
                params![state, completed_at, now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force release stale claims.
    pub fn force_release_stale(
        &self,
        timeout_seconds: i64,
        states_config: &StatesConfig,
    ) -> Result<i32> {
        let now = now_ms();
        let cutoff = now - (timeout_seconds * 1000);
        let release_status = &states_config.initial;

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE tasks SET worker_id = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE claimed_at < ?3 AND worker_id IS NOT NULL",
                params![release_status, now, cutoff],
            )?;

            Ok(updated as i32)
        })
    }

    /// Complete a task and release file locks held by the agent.
    /// Uses "completed" state by default, which should be a terminal state.
    /// Checks that all children (via 'contains' dependencies) are complete.
    pub fn complete_task(
        &self,
        task_id: &str,
        agent_id: &str,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        // Find a terminal state to use (prefer "completed" if it exists)
        let complete_status = if states_config.definitions.contains_key("completed") {
            "completed"
        } else {
            // Find any terminal state
            states_config
                .definitions
                .iter()
                .find(|(_, def)| def.exits.is_empty())
                .map(|(name, _)| name.as_str())
                .unwrap_or("completed")
        };

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get the task
            let mut stmt = tx.prepare("SELECT * FROM tasks WHERE id = ?1")?;
            let task = stmt
                .query_row(params![task_id], parse_task_row)
                .map_err(|_| anyhow!("Task not found"))?;
            drop(stmt);

            // Verify ownership
            if task.worker_id.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            // Check for incomplete children (blocking completion)
            let incomplete_children: i32 = tx.query_row(
                "SELECT COUNT(*) FROM dependencies d
                 INNER JOIN tasks child ON d.to_task_id = child.id
                 WHERE d.from_task_id = ?1 AND d.dep_type = 'contains'
                 AND child.status IN (SELECT value FROM json_each(?2))",
                params![
                    task_id,
                    serde_json::to_string(&states_config.blocking_states)?
                ],
                |row| row.get(0),
            )?;

            if incomplete_children > 0 {
                return Err(anyhow!(
                    "Cannot complete task: {} child task(s) are not complete",
                    incomplete_children
                ));
            }

            // Validate transition
            if !states_config.is_valid_transition(&task.status, complete_status) {
                let exits = states_config.get_exits(&task.status);
                return Err(anyhow!(
                    "Cannot complete task in state '{}'. Allowed transitions: {:?}",
                    task.status,
                    exits
                ));
            }

            // Record state transition (accumulates time from timed state)
            record_state_transition(
                &tx,
                task_id,
                complete_status,
                Some(agent_id),
                None,
                states_config,
            )?;

            // Update task to completed
            tx.execute(
                "UPDATE tasks SET status = ?1, completed_at = ?2, updated_at = ?3,
                 worker_id = NULL, claimed_at = NULL
                 WHERE id = ?4",
                params![complete_status, now, now, task_id],
            )?;

            // Release file locks associated with this task (for auto-cleanup)
            tx.execute(
                "DELETE FROM file_locks WHERE task_id = ?1",
                params![task_id],
            )?;

            // Refresh agent heartbeat
            tx.execute(
                "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            tx.commit()?;

            Ok(Task {
                status: complete_status.to_string(),
                completed_at: Some(now),
                updated_at: now,
                worker_id: None,
                claimed_at: None,
                ..task
            })
        })
    }

    /// Get all tasks. Excludes soft-deleted tasks.
    pub fn get_all_tasks(&self) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT * FROM tasks WHERE deleted_at IS NULL ORDER BY created_at")?;
            let tasks = stmt
                .query_map([], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        })
    }

    /// Get tasks by status.
    #[allow(dead_code)]
    pub fn get_tasks_by_status(&self, status: &str) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT * FROM tasks WHERE status = ?1 ORDER BY created_at")?;
            let tasks = stmt
                .query_map(params![status], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        })
    }

    /// Get claimed tasks. Excludes soft-deleted tasks.
    pub fn get_claimed_tasks(&self, agent_id: Option<&str>) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let tasks = if let Some(aid) = agent_id {
                let mut stmt = conn
                    .prepare("SELECT * FROM tasks WHERE worker_id = ?1 AND deleted_at IS NULL ORDER BY claimed_at")?;
                stmt.query_map(params![aid], parse_task_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE worker_id IS NOT NULL AND deleted_at IS NULL ORDER BY claimed_at",
                )?;
                stmt.query_map([], parse_task_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            };

            Ok(tasks)
        })
    }
}

/// Helper function to create task tree recursively within a transaction.
/// Creates dependencies from parent to children using child_type.
/// Creates dependencies between siblings using sibling_type.
/// Supports referencing existing tasks via ref_id.
#[allow(clippy::too_many_arguments)]
fn create_tree_recursive(
    conn: &Connection,
    input: &TaskTreeInput,
    parent_id: Option<&str>,
    prev_sibling_id: Option<&str>,
    child_type: Option<&str>,
    sibling_type: Option<&str>,
    all_ids: &mut Vec<String>,
    phase_warnings: &mut Vec<String>,
    tag_warnings: &mut Vec<String>,
    states_config: &StatesConfig,
    phases_config: &PhasesConfig,
    tags_config: &TagsConfig,
    ids_config: &IdsConfig,
) -> Result<String> {
    // Check if this node references an existing task
    let task_id = if let Some(ref ref_id) = input.ref_id {
        // Verify the referenced task exists
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            params![ref_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(anyhow::anyhow!("Referenced task '{}' not found", ref_id));
        }
        ref_id.clone()
    } else {
        // Create a new task
        let task_id = input
            .id
            .clone()
            .unwrap_or_else(|| generate_task_id(ids_config));
        let now = now_ms();
        let priority = clamp_priority(input.priority.unwrap_or(PRIORITY_DEFAULT));
        let initial_status = &states_config.initial;

        // Derive title: use explicit title, or derive from description, or empty
        let title = input.title.clone().unwrap_or_else(|| {
            input
                .description
                .as_deref()
                .map(|d| crate::format::truncate_title(d).into_owned())
                .unwrap_or_default()
        });

        // Check phase validity
        if let Some(ref phase) = input.phase
            && let Some(warning) = phases_config.check_phase(phase)?
        {
            phase_warnings.push(format!("Task '{}': {}", task_id, warning));
        }

        let needed_tags = input.needed_tags.clone().unwrap_or_default();
        let wanted_tags = input.wanted_tags.clone().unwrap_or_default();
        let tags = input.tags.clone().unwrap_or_default();

        // Check tag validity for all tag types
        for warning in tags_config.validate_tags(&tags)? {
            tag_warnings.push(format!("Task '{}': {}", task_id, warning));
        }
        for warning in tags_config.validate_tags(&needed_tags)? {
            tag_warnings.push(format!("Task '{}' needed_tags: {}", task_id, warning));
        }
        for warning in tags_config.validate_tags(&wanted_tags)? {
            tag_warnings.push(format!("Task '{}' wanted_tags: {}", task_id, warning));
        }

        let needed_tags_json = serde_json::to_string(&needed_tags)?;
        let wanted_tags_json = serde_json::to_string(&wanted_tags)?;
        let tags_json = serde_json::to_string(&tags)?;

        conn.execute(
            "INSERT INTO tasks (
                id, title, description, status, phase, priority,
                needed_tags, wanted_tags, tags, points, time_estimate_ms, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                &task_id,
                &title,
                &input.description,
                initial_status,
                &input.phase,
                priority.to_string(),
                needed_tags_json,
                wanted_tags_json,
                tags_json,
                input.points,
                input.time_estimate_ms,
                now,
                now,
            ],
        )?;

        // Record initial state transition
        record_state_transition(conn, &task_id, initial_status, None, None, states_config)?;

        // Sync tags to junction tables for indexed lookups
        sync_task_tags(conn, &task_id, &tags)?;
        sync_needed_tags(conn, &task_id, &needed_tags)?;
        sync_wanted_tags(conn, &task_id, &wanted_tags)?;

        task_id
    };

    // Create dependency from parent if child_type is specified
    if let (Some(pid), Some(ct)) = (parent_id, child_type) {
        Database::add_dependency_internal(conn, pid, &task_id, ct)?;
    }

    // Create dependency from previous sibling if sibling_type is specified
    if let (Some(prev_id), Some(st)) = (prev_sibling_id, sibling_type) {
        Database::add_dependency_internal(conn, prev_id, &task_id, st)?;
    }

    all_ids.push(task_id.clone());

    // Create children with dependencies based on child_type and sibling_type
    let mut prev_child_id: Option<String> = None;
    for child in input.children.iter() {
        let child_id = create_tree_recursive(
            conn,
            child,
            Some(&task_id),
            prev_child_id.as_deref(),
            child_type,
            sibling_type,
            all_ids,
            phase_warnings,
            tag_warnings,
            states_config,
            phases_config,
            tags_config,
            ids_config,
        )?;
        prev_child_id = Some(child_id);
    }

    Ok(task_id)
}
