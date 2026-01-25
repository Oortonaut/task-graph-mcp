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
}
