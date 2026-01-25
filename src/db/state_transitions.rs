//! State transition tracking for automatic time accumulation.

use crate::config::StatesConfig;
use crate::db::{now_ms, Database};
use crate::types::TaskStateEvent;
use anyhow::Result;
use rusqlite::{params, Connection};

/// Record a state transition and accumulate time if transitioning from a timed state.
///
/// Returns the elapsed time added to time_actual_ms (0 if previous state was not timed).
pub(crate) fn record_state_transition(
    conn: &Connection,
    task_id: &str,
    to_status: &str,
    worker_id: Option<&str>,
    reason: Option<&str>,
    states_config: &StatesConfig,
) -> Result<i64> {
    let now = now_ms();
    let mut elapsed_added = 0i64;

    // Find and close any open transition for this task
    let open_transition: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT id, event, timestamp FROM task_state_sequence
             WHERE task_id = ?1 AND end_timestamp IS NULL
             ORDER BY id DESC LIMIT 1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    if let Some((open_id, prev_event_str, start_timestamp)) = open_transition {
        // Close the previous transition
        conn.execute(
            "UPDATE task_state_sequence SET end_timestamp = ?1 WHERE id = ?2",
            params![now, open_id],
        )?;

        // If previous state was a timed state, accumulate elapsed time
        if states_config.is_timed_state(&prev_event_str) {
            elapsed_added = now - start_timestamp;

            // Add elapsed time to task's time_actual_ms
            conn.execute(
                "UPDATE tasks SET time_actual_ms = COALESCE(time_actual_ms, 0) + ?1, updated_at = ?2
                 WHERE id = ?3",
                params![elapsed_added, now, task_id],
            )?;
        }
    }

    // Insert the new transition
    conn.execute(
        "INSERT INTO task_state_sequence (task_id, worker_id, event, reason, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![task_id, worker_id, to_status, reason, now],
    )?;

    Ok(elapsed_added)
}

/// Statistics for project-wide state transitions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectStateStats {
    pub total_transitions: i64,
    pub total_time_ms: i64,
    pub tasks_affected: i64,
    pub transitions_by_status: std::collections::HashMap<String, i64>,
    pub time_by_status_ms: std::collections::HashMap<String, i64>,
    pub transitions_by_agent: std::collections::HashMap<String, i64>,
    pub time_by_agent_ms: std::collections::HashMap<String, i64>,
}

impl Database {
    /// Get the state transition history for a task.
    pub fn get_task_state_history(&self, task_id: &str) -> Result<Vec<TaskStateEvent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, worker_id, event, reason, timestamp, end_timestamp
                 FROM task_state_sequence
                 WHERE task_id = ?1
                 ORDER BY id ASC",
            )?;

            let events = stmt
                .query_map(params![task_id], |row| {
                    Ok(TaskStateEvent {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        worker_id: row.get(2)?,
                        event: row.get(3)?,
                        reason: row.get(4)?,
                        timestamp: row.get(5)?,
                        end_timestamp: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(events)
        })
    }

    /// Get the current duration in the current state (for active time tracking).
    /// Only returns a duration if the current state is a timed state.
    pub fn get_current_state_duration(
        &self,
        task_id: &str,
        states_config: &StatesConfig,
    ) -> Result<Option<i64>> {
        self.with_conn(|conn| {
            let result: Option<(String, i64)> = conn
                .query_row(
                    "SELECT event, timestamp FROM task_state_sequence
                     WHERE task_id = ?1 AND end_timestamp IS NULL
                     ORDER BY id DESC LIMIT 1",
                    params![task_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();

            match result {
                Some((event_str, start_timestamp)) => {
                    if states_config.is_timed_state(&event_str) {
                        return Ok(Some(now_ms() - start_timestamp));
                    }
                    Ok(None)
                }
                None => Ok(None),
            }
        })
    }

    /// Get project-wide state transition history with optional time range filter.
    /// Returns all state transitions across all tasks within the specified time range.
    pub fn get_project_state_history(
        &self,
        from_timestamp: Option<i64>,
        to_timestamp: Option<i64>,
        state_filter: Option<&[String]>,
        limit: Option<i64>,
    ) -> Result<Vec<TaskStateEvent>> {
        self.with_conn(|conn| {
            // Build query dynamically based on filters
            let mut sql = String::from(
                "SELECT id, task_id, worker_id, event, reason, timestamp, end_timestamp
                 FROM task_state_sequence WHERE 1=1"
            );
            let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(from_ts) = from_timestamp {
                sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len() + 1));
                param_values.push(Box::new(from_ts));
            }

            if let Some(to_ts) = to_timestamp {
                sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len() + 1));
                param_values.push(Box::new(to_ts));
            }

            if let Some(states) = state_filter {
                if !states.is_empty() {
                    let placeholders: Vec<String> = states.iter().enumerate()
                        .map(|(i, _)| format!("?{}", param_values.len() + i + 1))
                        .collect();
                    sql.push_str(&format!(" AND event IN ({})", placeholders.join(", ")));
                    for state in states {
                        param_values.push(Box::new(state.clone()));
                    }
                }
            }

            sql.push_str(" ORDER BY timestamp DESC, id DESC");

            if let Some(lim) = limit {
                sql.push_str(&format!(" LIMIT ?{}", param_values.len() + 1));
                param_values.push(Box::new(lim));
            }

            let mut stmt = conn.prepare(&sql)?;

            // Convert Vec<Box<dyn ToSql>> to slice of references
            let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

            let events = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(TaskStateEvent {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        worker_id: row.get(2)?,
                        event: row.get(3)?,
                        reason: row.get(4)?,
                        timestamp: row.get(5)?,
                        end_timestamp: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(events)
        })
    }

    /// Get aggregate project statistics for state transitions within a time range.
    /// Returns counts of transitions per state and per agent.
    pub fn get_project_state_stats(
        &self,
        from_timestamp: Option<i64>,
        to_timestamp: Option<i64>,
    ) -> Result<ProjectStateStats> {
        self.with_conn(|conn| {
            let mut transitions_by_status = std::collections::HashMap::new();
            let mut time_by_status = std::collections::HashMap::new();
            let mut transitions_by_agent = std::collections::HashMap::new();
            let mut time_by_agent = std::collections::HashMap::new();
            let mut tasks_touched = std::collections::HashSet::new();
            let mut total_transitions = 0i64;
            let mut total_time_ms = 0i64;

            // Build base query
            let mut sql = String::from(
                "SELECT event, worker_id, task_id, timestamp, end_timestamp FROM task_state_sequence WHERE 1=1"
            );
            let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(from_ts) = from_timestamp {
                sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len() + 1));
                param_values.push(Box::new(from_ts));
            }

            if let Some(to_ts) = to_timestamp {
                sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len() + 1));
                param_values.push(Box::new(to_ts));
            }

            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

            let mut rows = stmt.query(param_refs.as_slice())?;

            while let Some(row) = rows.next()? {
                let event: String = row.get(0)?;
                let worker_id: Option<String> = row.get(1)?;
                let task_id: String = row.get(2)?;
                let timestamp: i64 = row.get(3)?;
                let end_timestamp: Option<i64> = row.get(4)?;

                total_transitions += 1;
                tasks_touched.insert(task_id);

                *transitions_by_status.entry(event.clone()).or_insert(0i64) += 1;

                if let Some(ref agent) = worker_id {
                    *transitions_by_agent.entry(agent.clone()).or_insert(0i64) += 1;
                }

                // Calculate duration if we have an end timestamp
                if let Some(end_ts) = end_timestamp {
                    let duration = end_ts - timestamp;
                    total_time_ms += duration;
                    *time_by_status.entry(event).or_insert(0i64) += duration;

                    if let Some(agent) = worker_id {
                        *time_by_agent.entry(agent).or_insert(0i64) += duration;
                    }
                }
            }

            Ok(ProjectStateStats {
                total_transitions,
                total_time_ms,
                tasks_affected: tasks_touched.len() as i64,
                transitions_by_status,
                time_by_status_ms: time_by_status,
                transitions_by_agent,
                time_by_agent_ms: time_by_agent,
            })
        })
    }
}
