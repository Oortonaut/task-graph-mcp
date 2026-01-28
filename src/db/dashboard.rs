//! Dashboard-specific database queries.
//!
//! These methods provide efficient queries for the web dashboard UI.

use super::Database;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

/// Simple task info for dashboard display.
#[derive(Debug, Clone)]
pub struct DashboardTask {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub priority: i32,
}

/// Extended task info for task list view.
#[derive(Debug, Clone)]
pub struct TaskListItem {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub priority: i32,
    pub worker_id: Option<String>,
    pub tags: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Query parameters for task list.
#[derive(Debug, Clone, Default)]
pub struct TaskListQuery {
    pub status: Option<String>,
    pub phase: Option<String>,
    pub tags: Option<String>,
    pub parent: Option<String>,
    pub owner: Option<String>,
    pub sort_by: String,
    pub sort_order: String,
    pub page: i32,
    pub limit: i32,
    /// Whether to filter to timed states only.
    pub timed_filter: Option<bool>,
    /// List of timed state names to filter on when timed_filter is Some(true).
    pub timed_states: Vec<String>,
}

/// Result of task list query with pagination info.
#[derive(Debug, Clone)]
pub struct TaskListResult {
    pub tasks: Vec<TaskListItem>,
    pub total: i64,
    pub page: i32,
    pub limit: i32,
    pub total_pages: i32,
}

/// Represents a single activity event for the activity feed.
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub id: i64,
    pub event_type: ActivityEventType,
    pub timestamp: i64,
    pub worker_id: Option<String>,
    pub task_id: Option<String>,
    pub file_path: Option<String>,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub reason: Option<String>,
}

/// Type of activity event.
#[derive(Debug, Clone, PartialEq)]
pub enum ActivityEventType {
    TaskTransition,
    FileClaim,
    FileRelease,
}

impl ActivityEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityEventType::TaskTransition => "transition",
            ActivityEventType::FileClaim => "claim",
            ActivityEventType::FileRelease => "release",
        }
    }
}

/// Query parameters for activity list.
#[derive(Debug, Clone, Default)]
pub struct ActivityListQuery {
    pub event_type: Option<String>,
    pub status: Option<String>,
    pub worker: Option<String>,
    pub task: Option<String>,
    pub page: i32,
    pub limit: i32,
}

/// Result of activity list query with pagination.
#[derive(Debug, Clone)]
pub struct ActivityListResult {
    pub events: Vec<ActivityEvent>,
    pub total: i64,
    pub page: i32,
    pub limit: i32,
    pub total_pages: i32,
}

/// Activity statistics for the dashboard.
#[derive(Debug, Clone)]
pub struct ActivityStats {
    pub total_events_24h: i64,
    pub transitions_24h: i64,
    pub file_events_24h: i64,
    pub active_workers: i64,
    pub events_by_status: HashMap<String, i64>,
}

/// Simple worker info for dashboard display.
#[derive(Debug, Clone)]
pub struct DashboardWorker {
    pub id: String,
    pub current_thought: Option<String>,
    pub claim_count: i32,
}

/// Extended task info for worker detail view.
#[derive(Debug, Clone)]
pub struct WorkerClaimedTask {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub current_thought: Option<String>,
}

impl Database {
    /// Get task statistics for the dashboard (total, working, completed).
    pub fn get_task_stats(&self) -> Result<(i64, i64, i64)> {
        self.with_conn(|conn| {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            let working: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'working' AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            let completed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'completed' AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            Ok((total, working, completed))
        })
    }

    /// Get count of active workers (those with recent heartbeats).
    pub fn get_active_worker_count(&self) -> Result<i64> {
        self.with_conn(|conn| {
            // Consider workers active if heartbeat within last 5 minutes
            let cutoff = super::now_ms() - (5 * 60 * 1000);
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM workers WHERE last_heartbeat > ?1",
                params![cutoff],
                |row| row.get(0),
            )?;
            Ok(count)
        })
    }

    /// Get recent tasks for dashboard display.
    pub fn get_recent_tasks(&self, limit: i32) -> Result<Vec<DashboardTask>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, status, priority
                 FROM tasks
                 WHERE deleted_at IS NULL
                 ORDER BY updated_at DESC
                 LIMIT ?1",
            )?;

            let tasks = stmt
                .query_map(params![limit], |row| {
                    let id: String = row.get(0)?;
                    let title: Option<String> = row.get(1)?;
                    let status: String = row.get(2)?;
                    let priority: i32 = row.get(3)?;
                    Ok(DashboardTask {
                        id,
                        title,
                        status,
                        priority,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Get active workers for dashboard display.
    pub fn get_active_workers(&self) -> Result<Vec<DashboardWorker>> {
        self.with_conn(|conn| {
            // Consider workers active if heartbeat within last 5 minutes
            let cutoff = super::now_ms() - (5 * 60 * 1000);

            let mut stmt = conn.prepare(
                "SELECT w.id,
                        (SELECT current_thought FROM tasks WHERE worker_id = w.id AND status = 'working' AND current_thought IS NOT NULL LIMIT 1),
                        (SELECT COUNT(*) FROM tasks WHERE worker_id = w.id AND status = 'working')
                 FROM workers w
                 WHERE w.last_heartbeat > ?1
                 ORDER BY w.last_heartbeat DESC"
            )?;

            let workers = stmt
                .query_map(params![cutoff], |row| {
                    let id: String = row.get(0)?;
                    let current_thought: Option<String> = row.get(1)?;
                    let claim_count: i32 = row.get(2)?;
                    Ok(DashboardWorker { id, current_thought, claim_count })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(workers)
        })
    }

    /// Query tasks with filters for the task list view.
    pub fn query_tasks(&self, query: &TaskListQuery) -> Result<TaskListResult> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT t.id, t.title, t.status, t.priority, t.worker_id, t.tags, t.created_at, t.updated_at
                 FROM tasks t
                 WHERE t.deleted_at IS NULL"
            );

            let mut count_sql = String::from(
                "SELECT COUNT(*) FROM tasks t WHERE t.deleted_at IS NULL"
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut param_idx = 1;

            // Status filter
            if let Some(ref status) = query.status {
                if !status.is_empty() {
                    let clause = format!(" AND t.status = ?{}", param_idx);
                    sql.push_str(&clause);
                    count_sql.push_str(&clause);
                    params_vec.push(Box::new(status.clone()));
                    param_idx += 1;
                }
            }

            // Owner filter
            if let Some(ref owner) = query.owner {
                if !owner.is_empty() {
                    let clause = format!(" AND t.worker_id = ?{}", param_idx);
                    sql.push_str(&clause);
                    count_sql.push_str(&clause);
                    params_vec.push(Box::new(owner.clone()));
                    param_idx += 1;
                }
            }

            // Parent filter
            if let Some(ref parent) = query.parent {
                if !parent.is_empty() {
                    let clause = format!(
                        " AND t.id IN (SELECT to_task_id FROM dependencies WHERE from_task_id = ?{} AND dep_type = 'contains')",
                        param_idx
                    );
                    sql.push_str(&clause);
                    count_sql.push_str(&clause);
                    params_vec.push(Box::new(parent.clone()));
                    param_idx += 1;
                }
            }

            // Tags filter (comma-separated, any match)
            if let Some(ref tags) = query.tags {
                if !tags.is_empty() {
                    let tag_list: Vec<&str> = tags.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
                    if !tag_list.is_empty() {
                        let mut tag_conditions = Vec::new();
                        for tag in tag_list {
                            tag_conditions.push(format!("t.tags LIKE '%' || ?{} || '%'", param_idx));
                            params_vec.push(Box::new(tag.to_string()));
                            param_idx += 1;
                        }
                        let clause = format!(" AND ({})", tag_conditions.join(" OR "));
                        sql.push_str(&clause);
                        count_sql.push_str(&clause);
                    }
                }
            }

            // Sorting
            let order_clause = match (query.sort_by.as_str(), query.sort_order.as_str()) {
                ("priority", "asc") => " ORDER BY t.priority ASC, t.created_at DESC",
                ("priority", "desc") | ("priority", _) => " ORDER BY t.priority DESC, t.created_at DESC",
                ("created", "asc") | ("created_at", "asc") => " ORDER BY t.created_at ASC",
                ("created", "desc") | ("created_at", "desc") => " ORDER BY t.created_at DESC",
                ("updated", "asc") | ("updated_at", "asc") => " ORDER BY t.updated_at ASC",
                ("updated", "desc") | ("updated_at", "desc") => " ORDER BY t.updated_at DESC",
                _ => " ORDER BY t.priority DESC, t.created_at DESC",
            };
            sql.push_str(order_clause);

            // Pagination
            let offset = (query.page - 1) * query.limit;
            sql.push_str(&format!(" LIMIT {} OFFSET {}", query.limit, offset));

            // Get total count
            let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let total: i64 = conn.query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))?;

            // Get tasks
            let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;

            let tasks = stmt
                .query_map(params_refs.as_slice(), |row| {
                    Ok(TaskListItem {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        status: row.get(2)?,
                        priority: row.get(3)?,
                        worker_id: row.get(4)?,
                        tags: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            let total_pages = ((total as f64) / (query.limit as f64)).ceil() as i32;

            Ok(TaskListResult {
                tasks,
                total,
                page: query.page,
                limit: query.limit,
                total_pages,
            })
        })
    }

    /// Get tasks claimed by a specific worker for the detail view.
    pub fn get_worker_claimed_tasks(&self, worker_id: &str) -> Result<Vec<WorkerClaimedTask>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, status, current_thought
                 FROM tasks
                 WHERE worker_id = ?1 AND status = 'working' AND deleted_at IS NULL
                 ORDER BY claimed_at DESC",
            )?;

            let tasks = stmt
                .query_map(params![worker_id], |row| {
                    let id: String = row.get(0)?;
                    let title: Option<String> = row.get(1)?;
                    let status: String = row.get(2)?;
                    let current_thought: Option<String> = row.get(3)?;
                    Ok(WorkerClaimedTask {
                        id,
                        title,
                        status,
                        current_thought,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Simple task update for dashboard (bypasses state machine validation).
    /// This is an admin-level operation that allows direct field updates.
    pub fn dashboard_update_task(
        &self,
        task_id: &str,
        status: Option<&str>,
        priority: Option<i32>,
        description: Option<&str>,
        tags: Option<Vec<String>>,
    ) -> Result<()> {
        let now = super::now_ms();

        self.with_conn(|conn| {
            // Build dynamic update query
            let mut updates = vec!["updated_at = ?1".to_string()];
            let mut param_idx = 2;

            if status.is_some() {
                updates.push(format!("status = ?{}", param_idx));
                param_idx += 1;
            }
            if priority.is_some() {
                updates.push(format!("priority = ?{}", param_idx));
                param_idx += 1;
            }
            if description.is_some() {
                updates.push(format!("description = ?{}", param_idx));
                param_idx += 1;
            }
            if tags.is_some() {
                updates.push(format!("tags = ?{}", param_idx));
                param_idx += 1;
            }

            let sql = format!(
                "UPDATE tasks SET {} WHERE id = ?{}",
                updates.join(", "),
                param_idx
            );

            // Build params list dynamically
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(now));

            if let Some(s) = status {
                params_vec.push(Box::new(s.to_string()));
            }
            if let Some(p) = priority {
                params_vec.push(Box::new(p));
            }
            if let Some(d) = description {
                params_vec.push(Box::new(d.to_string()));
            }
            if let Some(t) = tags {
                params_vec.push(Box::new(serde_json::to_string(&t)?));
            }
            params_vec.push(Box::new(task_id.to_string()));

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();
            let rows_affected = conn.execute(&sql, params_refs.as_slice())?;

            if rows_affected == 0 {
                return Err(anyhow::anyhow!("Task not found"));
            }

            Ok(())
        })
    }

    /// Simple task deletion for dashboard (soft delete).
    pub fn dashboard_delete_task(&self, task_id: &str) -> Result<()> {
        let now = super::now_ms();

        self.with_conn(|conn| {
            // Check for children
            let child_count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM dependencies WHERE from_task_id = ?1 AND dep_type = 'contains'",
                params![task_id],
                |row| row.get(0),
            )?;

            if child_count > 0 {
                return Err(anyhow::anyhow!("Task has children; delete them first"));
            }

            let rows_affected = conn.execute(
                "UPDATE tasks SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
                params![now, task_id],
            )?;

            if rows_affected == 0 {
                return Err(anyhow::anyhow!("Task not found or already deleted"));
            }

            Ok(())
        })
    }

    /// Force release a task claim (admin operation for dashboard).
    /// Sets status to pending and clears worker_id, claimed_at.
    pub fn dashboard_force_release_task(&self, task_id: &str) -> Result<()> {
        let now = super::now_ms();

        self.with_conn(|conn| {
            let rows_affected = conn.execute(
                "UPDATE tasks SET
                    status = 'pending',
                    worker_id = NULL,
                    claimed_at = NULL,
                    current_thought = NULL,
                    updated_at = ?1
                WHERE id = ?2 AND deleted_at IS NULL",
                params![now, task_id],
            )?;

            if rows_affected == 0 {
                return Err(anyhow::anyhow!("Task not found or already deleted"));
            }

            Ok(())
        })
    }

    /// Get activity statistics for the last 24 hours.
    pub fn get_activity_stats(&self) -> Result<ActivityStats> {
        let now = super::now_ms();
        let cutoff_24h = now - (24 * 60 * 60 * 1000);

        self.with_conn(|conn| {
            // Count task state transitions in last 24h
            let transitions_24h: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_sequence WHERE timestamp >= ?1",
                params![cutoff_24h],
                |row| row.get(0),
            )?;

            // Count file claim/release events in last 24h
            let file_events_24h: i64 = conn.query_row(
                "SELECT COUNT(*) FROM claim_sequence WHERE timestamp >= ?1",
                params![cutoff_24h],
                |row| row.get(0),
            )?;

            let total_events_24h = transitions_24h + file_events_24h;

            // Count active workers (heartbeat in last 5 minutes)
            let worker_cutoff = now - (5 * 60 * 1000);
            let active_workers: i64 = conn.query_row(
                "SELECT COUNT(*) FROM workers WHERE last_heartbeat >= ?1",
                params![worker_cutoff],
                |row| row.get(0),
            )?;

            // Get transition counts by status in last 24h
            let mut events_by_status = HashMap::new();
            let mut stmt = conn.prepare(
                "SELECT status, COUNT(*) FROM task_sequence
                 WHERE timestamp >= ?1 AND status IS NOT NULL GROUP BY status",
            )?;
            let mut rows = stmt.query(params![cutoff_24h])?;
            while let Some(row) = rows.next()? {
                let status: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                events_by_status.insert(status, count);
            }

            Ok(ActivityStats {
                total_events_24h,
                transitions_24h,
                file_events_24h,
                active_workers,
                events_by_status,
            })
        })
    }

    /// Query activity events with filters and pagination.
    pub fn query_activity(&self, query: &ActivityListQuery) -> Result<ActivityListResult> {
        self.with_conn(|conn| {
            // We need to combine task_sequence and claim_sequence into a unified view
            // Use UNION ALL for efficiency

            let mut events = Vec::new();
            let mut total: i64 = 0;

            // Determine which event types to query
            let include_transitions =
                query.event_type.is_none() || query.event_type.as_deref() == Some("transition");
            let include_files =
                query.event_type.is_none() || query.event_type.as_deref() == Some("file");

            // Query task state transitions
            if include_transitions {
                let mut sql = String::from(
                    "SELECT id, task_id, worker_id, status, reason, timestamp
                     FROM task_sequence WHERE status IS NOT NULL",
                );
                let mut count_sql =
                    String::from("SELECT COUNT(*) FROM task_sequence WHERE status IS NOT NULL");
                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                let mut param_idx = 1;

                // Status filter
                if let Some(ref status) = query.status {
                    if !status.is_empty() {
                        sql.push_str(&format!(" AND status = ?{}", param_idx));
                        count_sql.push_str(&format!(" AND status = ?{}", param_idx));
                        params_vec.push(Box::new(status.clone()));
                        param_idx += 1;
                    }
                }

                // Worker filter
                if let Some(ref worker) = query.worker {
                    if !worker.is_empty() {
                        sql.push_str(&format!(" AND worker_id = ?{}", param_idx));
                        count_sql.push_str(&format!(" AND worker_id = ?{}", param_idx));
                        params_vec.push(Box::new(worker.clone()));
                        param_idx += 1;
                    }
                }

                // Task filter
                if let Some(ref task) = query.task {
                    if !task.is_empty() {
                        sql.push_str(&format!(" AND task_id LIKE '%' || ?{} || '%'", param_idx));
                        count_sql
                            .push_str(&format!(" AND task_id LIKE '%' || ?{} || '%'", param_idx));
                        params_vec.push(Box::new(task.clone()));
                        let _ = param_idx; // Consumed
                    }
                }

                sql.push_str(" ORDER BY timestamp DESC");

                // Get count for transitions
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();
                let trans_count: i64 =
                    conn.query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))?;
                total += trans_count;

                // Pagination for transitions only if not querying file events too
                if !include_files {
                    let offset = (query.page - 1) * query.limit;
                    sql.push_str(&format!(" LIMIT {} OFFSET {}", query.limit, offset));
                }

                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();
                let mut stmt = conn.prepare(&sql)?;
                let mut rows = stmt.query(params_refs.as_slice())?;

                while let Some(row) = rows.next()? {
                    let id: i64 = row.get(0)?;
                    let task_id: String = row.get(1)?;
                    let worker_id: Option<String> = row.get(2)?;
                    let event: String = row.get(3)?;
                    let reason: Option<String> = row.get(4)?;
                    let timestamp: i64 = row.get(5)?;

                    events.push(ActivityEvent {
                        id,
                        event_type: ActivityEventType::TaskTransition,
                        timestamp,
                        worker_id,
                        task_id: Some(task_id),
                        file_path: None,
                        from_status: None,
                        to_status: Some(event),
                        reason,
                    });
                }
            }

            // Query file claim/release events
            if include_files && query.status.is_none() {
                let mut sql = String::from(
                    "SELECT id, file_path, worker_id, event, reason, timestamp
                     FROM claim_sequence WHERE 1=1",
                );
                let mut count_sql = String::from("SELECT COUNT(*) FROM claim_sequence WHERE 1=1");
                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                let param_idx = 1;

                // Worker filter
                if let Some(ref worker) = query.worker {
                    if !worker.is_empty() {
                        sql.push_str(&format!(" AND worker_id = ?{}", param_idx));
                        count_sql.push_str(&format!(" AND worker_id = ?{}", param_idx));
                        params_vec.push(Box::new(worker.clone()));
                    }
                }

                // Note: Task filter for file events is not implemented - file events
                // are less relevant when filtering by task

                sql.push_str(" ORDER BY timestamp DESC");

                // Get count for file events
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();
                let file_count: i64 =
                    conn.query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))?;
                total += file_count;

                // Skip file events if task filter is active
                if query.task.is_none() || query.task.as_ref().map(|t| t.is_empty()).unwrap_or(true)
                {
                    let params_refs: Vec<&dyn rusqlite::ToSql> =
                        params_vec.iter().map(|b| b.as_ref()).collect();
                    let mut stmt = conn.prepare(&sql)?;
                    let mut rows = stmt.query(params_refs.as_slice())?;

                    while let Some(row) = rows.next()? {
                        let id: i64 = row.get(0)?;
                        let file_path: String = row.get(1)?;
                        let worker_id: String = row.get(2)?;
                        let event: String = row.get(3)?;
                        let reason: Option<String> = row.get(4)?;
                        let timestamp: i64 = row.get(5)?;

                        let event_type = if event == "claimed" {
                            ActivityEventType::FileClaim
                        } else {
                            ActivityEventType::FileRelease
                        };

                        events.push(ActivityEvent {
                            id: id + 1_000_000_000, // Offset to avoid ID collision
                            event_type,
                            timestamp,
                            worker_id: Some(worker_id),
                            task_id: None,
                            file_path: Some(file_path),
                            from_status: None,
                            to_status: None,
                            reason,
                        });
                    }
                }
            }

            // Sort all events by timestamp descending
            events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

            // Apply pagination to combined results
            let offset = ((query.page - 1) * query.limit) as usize;
            let limit = query.limit as usize;
            let paginated_events: Vec<ActivityEvent> =
                events.into_iter().skip(offset).take(limit).collect();

            let total_pages = ((total as f64) / (query.limit as f64)).ceil() as i32;

            Ok(ActivityListResult {
                events: paginated_events,
                total,
                page: query.page,
                limit: query.limit,
                total_pages: total_pages.max(1),
            })
        })
    }

    /// Get all file marks with full details for the dashboard.
    pub fn get_all_file_marks(&self) -> Result<Vec<DashboardFileMark>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT file_path, worker_id, reason, locked_at, task_id
                 FROM file_locks
                 ORDER BY locked_at DESC",
            )?;

            let marks = stmt
                .query_map([], |row| {
                    Ok(DashboardFileMark {
                        file_path: row.get(0)?,
                        worker_id: row.get(1)?,
                        reason: row.get(2)?,
                        locked_at: row.get(3)?,
                        task_id: row.get(4)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(marks)
        })
    }

    /// Get file marks statistics for the dashboard.
    pub fn get_file_marks_stats(&self) -> Result<FileMarksStats> {
        self.with_conn(|conn| {
            let total_marks: i64 =
                conn.query_row("SELECT COUNT(*) FROM file_locks", [], |row| row.get(0))?;

            let unique_agents: i64 = conn.query_row(
                "SELECT COUNT(DISTINCT worker_id) FROM file_locks",
                [],
                |row| row.get(0),
            )?;

            let with_tasks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM file_locks WHERE task_id IS NOT NULL",
                [],
                |row| row.get(0),
            )?;

            // Count stale marks (older than 1 hour)
            let now = super::now_ms();
            let stale_cutoff = now - (60 * 60 * 1000); // 1 hour
            let stale_marks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM file_locks WHERE locked_at < ?1",
                params![stale_cutoff],
                |row| row.get(0),
            )?;

            Ok(FileMarksStats {
                total_marks,
                unique_agents,
                with_tasks,
                stale_marks,
            })
        })
    }

    /// Force-remove a file mark (admin operation for dashboard).
    /// Unlike normal unlock, this doesn't require the worker_id to match.
    pub fn force_unmark_file(&self, file_path: &str) -> Result<bool> {
        let now = super::now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get the current owner before deleting
            let owner: Option<String> = tx.query_row(
                "SELECT worker_id FROM file_locks WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            ).ok();

            let deleted = tx.execute(
                "DELETE FROM file_locks WHERE file_path = ?1",
                params![file_path],
            )?;

            if deleted > 0 {
                if let Some(worker_id) = owner {
                    // Find the claim_id for this file+worker (most recent claim)
                    let claim_id: Option<i64> = tx.query_row(
                        "SELECT MAX(id) FROM claim_sequence
                         WHERE file_path = ?1 AND worker_id = ?2 AND event = 'claimed'",
                        params![file_path, &worker_id],
                        |row| row.get(0),
                    ).ok().flatten();

                    // Close any open claim for this file+worker
                    tx.execute(
                        "UPDATE claim_sequence SET end_timestamp = ?1
                         WHERE file_path = ?2 AND worker_id = ?3 AND end_timestamp IS NULL",
                        params![now, file_path, &worker_id],
                    )?;

                    // Record force-release event
                    tx.execute(
                        "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp, claim_id)
                         VALUES (?1, ?2, 'released', 'Force-unmarked via dashboard', ?3, ?4)",
                        params![file_path, &worker_id, now, claim_id],
                    )?;
                }
            }

            tx.commit()?;
            Ok(deleted > 0)
        })
    }

    // ========== METRICS DASHBOARD METHODS ==========

    /// Get metrics overview statistics.
    pub fn get_metrics_overview(&self) -> Result<MetricsOverview> {
        self.with_conn(|conn| {
            let row: (i64, i64, f64, i64, i64, i64) = conn.query_row(
                "SELECT
                    COUNT(*) as total_tasks,
                    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_tasks,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost,
                    COALESCE(SUM(time_actual_ms), 0) as total_time,
                    COALESCE(SUM(points), 0) as total_points,
                    COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points
                FROM tasks
                WHERE deleted_at IS NULL",
                [],
                |row| Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                )),
            )?;

            Ok(MetricsOverview {
                total_tasks: row.0,
                completed_tasks: row.1,
                total_cost_usd: row.2,
                total_time_ms: row.3,
                total_points: row.4,
                completed_points: row.5,
            })
        })
    }

    /// Get task counts by status for distribution chart.
    pub fn get_status_distribution(&self) -> Result<HashMap<String, i64>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT status, COUNT(*) as count
                 FROM tasks
                 WHERE deleted_at IS NULL
                 GROUP BY status",
            )?;

            let mut distribution = HashMap::new();
            let rows = stmt.query_map([], |row| {
                let status: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((status, count))
            })?;

            for row in rows {
                let (status, count) = row?;
                distribution.insert(status, count);
            }

            Ok(distribution)
        })
    }

    /// Get velocity data (completed tasks per period).
    /// Period can be "day" or "week".
    pub fn get_velocity(&self, period: &str, num_periods: i32) -> Result<Vec<VelocityDataPoint>> {
        self.with_conn(|conn| {
            let now = super::now_ms();
            let period_ms: i64 = match period {
                "week" => 7 * 24 * 60 * 60 * 1000,
                _ => 24 * 60 * 60 * 1000, // day
            };

            let mut data_points = Vec::new();

            for i in 0..num_periods {
                let period_end = now - (i as i64 * period_ms);
                let period_start = period_end - period_ms;

                let (count, points): (i64, i64) = conn.query_row(
                    "SELECT COUNT(*), COALESCE(SUM(points), 0)
                     FROM tasks
                     WHERE deleted_at IS NULL
                     AND status = 'completed'
                     AND completed_at >= ?1
                     AND completed_at < ?2",
                    params![period_start, period_end],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;

                // Generate label (e.g., "Jan 15" or "Week 3")
                let label = if period == "week" {
                    if i == 0 {
                        "This week".to_string()
                    } else if i == 1 {
                        "Last week".to_string()
                    } else {
                        format!("{} weeks ago", i)
                    }
                } else {
                    if i == 0 {
                        "Today".to_string()
                    } else if i == 1 {
                        "Yesterday".to_string()
                    } else {
                        format!("{} days ago", i)
                    }
                };

                data_points.push(VelocityDataPoint {
                    period_label: label,
                    completed_count: count,
                    total_points: points,
                });
            }

            // Reverse to show oldest first
            data_points.reverse();
            Ok(data_points)
        })
    }

    /// Get average time spent in each status.
    pub fn get_time_in_status(&self) -> Result<Vec<TimeInStatusStats>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    status,
                    AVG(COALESCE(end_timestamp, ?1) - timestamp) as avg_duration,
                    SUM(COALESCE(end_timestamp, ?1) - timestamp) as total_duration,
                    COUNT(*) as transition_count
                FROM task_sequence
                WHERE status IS NOT NULL
                GROUP BY status
                ORDER BY avg_duration DESC",
            )?;

            let now = super::now_ms();
            let stats = stmt
                .query_map(params![now], |row| {
                    Ok(TimeInStatusStats {
                        status: row.get(0)?,
                        avg_duration_ms: row.get::<_, f64>(1)? as i64,
                        total_duration_ms: row.get::<_, f64>(2)? as i64,
                        transition_count: row.get(3)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(stats)
        })
    }

    /// Get cost breakdown by agent/worker.
    pub fn get_cost_by_agent(&self) -> Result<Vec<AgentCostStats>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    worker_id,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost,
                    COUNT(*) as task_count,
                    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_count,
                    COALESCE(SUM(time_actual_ms), 0) as total_time
                FROM tasks
                WHERE deleted_at IS NULL AND worker_id IS NOT NULL
                GROUP BY worker_id
                ORDER BY total_cost DESC",
            )?;

            let stats = stmt
                .query_map([], |row| {
                    Ok(AgentCostStats {
                        worker_id: row.get(0)?,
                        total_cost_usd: row.get(1)?,
                        task_count: row.get(2)?,
                        completed_count: row.get(3)?,
                        total_time_ms: row.get(4)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(stats)
        })
    }

    /// Get aggregate of custom metrics (metric_0 through metric_7).
    pub fn get_custom_metrics(&self) -> Result<CustomMetricsAggregate> {
        self.with_conn(|conn| {
            let row: (i64, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    COALESCE(SUM(metric_0), 0),
                    COALESCE(SUM(metric_1), 0),
                    COALESCE(SUM(metric_2), 0),
                    COALESCE(SUM(metric_3), 0),
                    COALESCE(SUM(metric_4), 0),
                    COALESCE(SUM(metric_5), 0),
                    COALESCE(SUM(metric_6), 0),
                    COALESCE(SUM(metric_7), 0)
                FROM tasks
                WHERE deleted_at IS NULL",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )?;

            Ok(CustomMetricsAggregate {
                metrics: [row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7],
            })
        })
    }

    // ========== DEPENDENCY GRAPH METHODS ==========

    /// Get dependency graph data for visualization.
    /// Returns nodes (tasks) and edges (dependencies) for rendering as a DAG.
    pub fn get_dependency_graph(
        &self,
        dep_type: Option<&str>,
        focus_task: Option<&str>,
        depth: i32,
    ) -> Result<DependencyGraph> {
        self.with_conn(|conn| {
            let mut nodes: Vec<GraphNode> = Vec::new();
            let mut edges: Vec<GraphEdge> = Vec::new();
            let mut seen_tasks: std::collections::HashSet<String> = std::collections::HashSet::new();

            // Build type filter clause
            let type_clause = match dep_type {
                Some("blocks") => "AND d.dep_type = 'blocks'",
                Some("follows") => "AND d.dep_type = 'follows'",
                Some("contains") => "AND d.dep_type = 'contains'",
                _ => "", // All types
            };

            if let Some(focus_id) = focus_task {
                // Focus mode: get dependencies around a specific task
                let actual_depth = if depth < 0 { 100 } else { depth };

                // Get the focus task first
                if let Ok(task) = conn.query_row(
                    "SELECT id, title, status, priority FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
                    params![focus_id],
                    |row| Ok(GraphNode {
                        id: row.get(0)?,
                        title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        status: row.get(2)?,
                        priority: row.get(3)?,
                    }),
                ) {
                    seen_tasks.insert(task.id.clone());
                    nodes.push(task);
                }

                // Get predecessors (tasks that block this one)
                let mut current_level: Vec<String> = vec![focus_id.to_string()];
                for _ in 0..actual_depth {
                    if current_level.is_empty() { break; }
                    let mut next_level: Vec<String> = Vec::new();

                    for tid in &current_level {
                        let sql = format!(
                            "SELECT d.from_task_id, d.dep_type, t.id, t.title, t.status, t.priority
                             FROM dependencies d
                             JOIN tasks t ON d.from_task_id = t.id
                             WHERE d.to_task_id = ?1 AND t.deleted_at IS NULL {}",
                            type_clause
                        );

                        let mut stmt = conn.prepare(&sql)?;
                        let rows = stmt.query_map(params![tid], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, String>(4)?,
                                row.get::<_, i32>(5)?,
                            ))
                        })?;

                        for row in rows.flatten() {
                            let (from_id, dep_type_str, task_id, title, status, priority) = row;

                            edges.push(GraphEdge {
                                from_id: from_id.clone(),
                                to_id: tid.clone(),
                                dep_type: dep_type_str,
                            });

                            if !seen_tasks.contains(&task_id) {
                                seen_tasks.insert(task_id.clone());
                                nodes.push(GraphNode {
                                    id: task_id.clone(),
                                    title: title.unwrap_or_default(),
                                    status,
                                    priority,
                                });
                                next_level.push(task_id);
                            }
                        }
                    }
                    current_level = next_level;
                }

                // Get successors (tasks this one blocks)
                let mut current_level: Vec<String> = vec![focus_id.to_string()];
                for _ in 0..actual_depth {
                    if current_level.is_empty() { break; }
                    let mut next_level: Vec<String> = Vec::new();

                    for tid in &current_level {
                        let sql = format!(
                            "SELECT d.to_task_id, d.dep_type, t.id, t.title, t.status, t.priority
                             FROM dependencies d
                             JOIN tasks t ON d.to_task_id = t.id
                             WHERE d.from_task_id = ?1 AND t.deleted_at IS NULL {}",
                            type_clause
                        );

                        let mut stmt = conn.prepare(&sql)?;
                        let rows = stmt.query_map(params![tid], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, String>(4)?,
                                row.get::<_, i32>(5)?,
                            ))
                        })?;

                        for row in rows.flatten() {
                            let (to_id, dep_type_str, task_id, title, status, priority) = row;

                            edges.push(GraphEdge {
                                from_id: tid.clone(),
                                to_id: to_id.clone(),
                                dep_type: dep_type_str,
                            });

                            if !seen_tasks.contains(&task_id) {
                                seen_tasks.insert(task_id.clone());
                                nodes.push(GraphNode {
                                    id: task_id.clone(),
                                    title: title.unwrap_or_default(),
                                    status,
                                    priority,
                                });
                                next_level.push(task_id);
                            }
                        }
                    }
                    current_level = next_level;
                }
            } else {
                // Full graph mode: get all dependencies
                let sql = format!(
                    "SELECT d.from_task_id, d.to_task_id, d.dep_type
                     FROM dependencies d
                     JOIN tasks t1 ON d.from_task_id = t1.id
                     JOIN tasks t2 ON d.to_task_id = t2.id
                     WHERE t1.deleted_at IS NULL AND t2.deleted_at IS NULL {}",
                    type_clause
                );

                let mut stmt = conn.prepare(&sql)?;
                let edge_rows = stmt.query_map([], |row| {
                    Ok(GraphEdge {
                        from_id: row.get(0)?,
                        to_id: row.get(1)?,
                        dep_type: row.get(2)?,
                    })
                })?;

                for edge in edge_rows.flatten() {
                    seen_tasks.insert(edge.from_id.clone());
                    seen_tasks.insert(edge.to_id.clone());
                    edges.push(edge);
                }

                // Now get node details for all seen tasks
                if !seen_tasks.is_empty() {
                    let placeholders: String = seen_tasks.iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", i + 1))
                        .collect::<Vec<_>>()
                        .join(", ");

                    let node_sql = format!(
                        "SELECT id, title, status, priority FROM tasks
                         WHERE id IN ({}) AND deleted_at IS NULL",
                        placeholders
                    );

                    let mut stmt = conn.prepare(&node_sql)?;
                    let params_vec: Vec<String> = seen_tasks.iter().cloned().collect();
                    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec
                        .iter()
                        .map(|s| s as &dyn rusqlite::ToSql)
                        .collect();

                    let node_rows = stmt.query_map(params_refs.as_slice(), |row| {
                        Ok(GraphNode {
                            id: row.get(0)?,
                            title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            status: row.get(2)?,
                            priority: row.get(3)?,
                        })
                    })?;

                    for node in node_rows.flatten() {
                        nodes.push(node);
                    }
                }
            }

            Ok(DependencyGraph { nodes, edges })
        })
    }

    /// Get dependency graph statistics.
    pub fn get_dependency_graph_stats(&self) -> Result<DependencyGraphStats> {
        self.with_conn(|conn| {
            let total_tasks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            let total_deps: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dependencies d
                 JOIN tasks t1 ON d.from_task_id = t1.id
                 JOIN tasks t2 ON d.to_task_id = t2.id
                 WHERE t1.deleted_at IS NULL AND t2.deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;

            let blocks_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dependencies d
                 JOIN tasks t1 ON d.from_task_id = t1.id
                 JOIN tasks t2 ON d.to_task_id = t2.id
                 WHERE t1.deleted_at IS NULL AND t2.deleted_at IS NULL AND d.dep_type = 'blocks'",
                [],
                |row| row.get(0),
            )?;

            let follows_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dependencies d
                 JOIN tasks t1 ON d.from_task_id = t1.id
                 JOIN tasks t2 ON d.to_task_id = t2.id
                 WHERE t1.deleted_at IS NULL AND t2.deleted_at IS NULL AND d.dep_type = 'follows'",
                [],
                |row| row.get(0),
            )?;

            let contains_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM dependencies d
                 JOIN tasks t1 ON d.from_task_id = t1.id
                 JOIN tasks t2 ON d.to_task_id = t2.id
                 WHERE t1.deleted_at IS NULL AND t2.deleted_at IS NULL AND d.dep_type = 'contains'",
                [],
                |row| row.get(0),
            )?;

            Ok(DependencyGraphStats {
                total_tasks,
                total_deps,
                blocks_count,
                follows_count,
                contains_count,
            })
        })
    }

    /// Get all unique phases used by tasks.
    pub fn get_available_phases(&self) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT phase FROM tasks WHERE phase IS NOT NULL AND deleted_at IS NULL ORDER BY phase"
            )?;

            let phases = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(phases)
        })
    }
}

/// File mark info for dashboard display.
#[derive(Debug, Clone)]
pub struct DashboardFileMark {
    pub file_path: String,
    pub worker_id: String,
    pub reason: Option<String>,
    pub locked_at: i64,
    pub task_id: Option<String>,
}

/// File marks statistics for the dashboard.
#[derive(Debug, Clone)]
pub struct FileMarksStats {
    pub total_marks: i64,
    pub unique_agents: i64,
    pub with_tasks: i64,
    pub stale_marks: i64,
}

// ========== METRICS STRUCTS ==========

/// Metrics overview data for the dashboard.
#[derive(Debug, Clone)]
pub struct MetricsOverview {
    pub total_tasks: i64,
    pub completed_tasks: i64,
    pub total_cost_usd: f64,
    pub total_time_ms: i64,
    pub total_points: i64,
    pub completed_points: i64,
}

/// Velocity data point for the chart.
#[derive(Debug, Clone)]
pub struct VelocityDataPoint {
    pub period_label: String,
    pub completed_count: i64,
    pub total_points: i64,
}

/// Time in status statistics.
#[derive(Debug, Clone)]
pub struct TimeInStatusStats {
    pub status: String,
    pub avg_duration_ms: i64,
    pub total_duration_ms: i64,
    pub transition_count: i64,
}

/// Cost and task stats by agent/worker.
#[derive(Debug, Clone)]
pub struct AgentCostStats {
    pub worker_id: String,
    pub total_cost_usd: f64,
    pub task_count: i64,
    pub completed_count: i64,
    pub total_time_ms: i64,
}

/// Custom metrics aggregate.
#[derive(Debug, Clone)]
pub struct CustomMetricsAggregate {
    pub metrics: [i64; 8],
}

// ========== DEPENDENCY GRAPH STRUCTS ==========

/// A node in the dependency graph representing a task.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: i32,
}

/// An edge in the dependency graph representing a dependency.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphEdge {
    pub from_id: String,
    pub to_id: String,
    pub dep_type: String,
}

/// The full dependency graph data.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DependencyGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Statistics about the dependency graph.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DependencyGraphStats {
    pub total_tasks: i64,
    pub total_deps: i64,
    pub blocks_count: i64,
    pub follows_count: i64,
    pub contains_count: i64,
}
