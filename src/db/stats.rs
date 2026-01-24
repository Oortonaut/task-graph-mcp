//! Aggregation queries for statistics.

use super::Database;
use crate::config::StatesConfig;
use crate::types::Stats;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

impl Database {
    /// Get aggregate statistics with dynamic state counting.
    pub fn get_stats(
        &self,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        states_config: &StatesConfig,
    ) -> Result<Stats> {
        self.with_conn(|conn| {
            // First, get the base aggregate stats
            let (base_sql, params_vec): (String, Vec<String>) = match (agent_id, task_id) {
                (Some(aid), None) => (
                    "SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks WHERE owner_agent = ?1"
                        .to_string(),
                    vec![aid.to_string()],
                ),
                (None, Some(tid)) => (
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks WHERE id IN (SELECT id FROM descendants)"
                        .to_string(),
                    vec![tid.to_string()],
                ),
                (Some(aid), Some(tid)) => (
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks WHERE id IN (SELECT id FROM descendants) AND owner_agent = ?1"
                        .to_string(),
                    vec![aid.to_string(), tid.to_string()],
                ),
                (None, None) => (
                    "SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks"
                        .to_string(),
                    vec![],
                ),
            };

            // Query base stats
            let (
                total_tasks,
                total_points,
                _completed_points_placeholder,
                total_time_estimate_ms,
                total_time_actual_ms,
                total_tokens_in,
                total_tokens_cached,
                total_tokens_out,
                total_tokens_thinking,
                total_tokens_image,
                total_tokens_audio,
                total_cost_usd,
            ): (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, f64) = if params_vec.is_empty()
            {
                conn.query_row(&base_sql, [], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ))
                })?
            } else if params_vec.len() == 1 {
                conn.query_row(&base_sql, params![params_vec[0]], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ))
                })?
            } else {
                conn.query_row(&base_sql, params![params_vec[0], params_vec[1]], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ))
                })?
            };

            // Now query task counts by state
            let count_sql = match (agent_id, task_id) {
                (Some(_aid), None) => {
                    "SELECT status, COUNT(*) as cnt FROM tasks WHERE owner_agent = ?1 GROUP BY status"
                }
                (None, Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT status, COUNT(*) as cnt FROM tasks 
                    WHERE id IN (SELECT id FROM descendants) GROUP BY status"
                }
                (Some(_aid), Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT status, COUNT(*) as cnt FROM tasks 
                    WHERE id IN (SELECT id FROM descendants) AND owner_agent = ?1 GROUP BY status"
                }
                (None, None) => "SELECT status, COUNT(*) as cnt FROM tasks GROUP BY status",
            };

            let mut tasks_by_state: HashMap<String, i64> = HashMap::new();

            // Initialize all defined states to 0
            for state in states_config.state_names() {
                tasks_by_state.insert(state.to_string(), 0);
            }

            // Query and fill in actual counts
            let mut stmt = conn.prepare(count_sql)?;
            let status_counts: Vec<(String, i64)> = if params_vec.is_empty() {
                stmt.query_map([], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count))
                })?.filter_map(|r| r.ok()).collect()
            } else if params_vec.len() == 1 {
                stmt.query_map(params![params_vec[0].clone()], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count))
                })?.filter_map(|r| r.ok()).collect()
            } else {
                stmt.query_map(params![params_vec[0].clone(), params_vec[1].clone()], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count))
                })?.filter_map(|r| r.ok()).collect()
            };

            for (status, count) in status_counts {
                tasks_by_state.insert(status, count);
            }

            // Calculate completed_points (points for tasks in non-blocking states)
            let completed_points_sql = match (agent_id, task_id) {
                (Some(_aid), None) => {
                    "SELECT COALESCE(SUM(points), 0) FROM tasks 
                     WHERE owner_agent = ?1 AND status NOT IN (SELECT value FROM json_each(?2))"
                }
                (None, Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT COALESCE(SUM(points), 0) FROM tasks 
                    WHERE id IN (SELECT id FROM descendants) 
                    AND status NOT IN (SELECT value FROM json_each(?2))"
                }
                (Some(_aid), Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT COALESCE(SUM(points), 0) FROM tasks 
                    WHERE id IN (SELECT id FROM descendants) AND owner_agent = ?1
                    AND status NOT IN (SELECT value FROM json_each(?3))"
                }
                (None, None) => {
                    "SELECT COALESCE(SUM(points), 0) FROM tasks 
                     WHERE status NOT IN (SELECT value FROM json_each(?1))"
                }
            };

            let blocking_states_json = serde_json::to_string(&states_config.blocking_states)?;

            let completed_points: i64 = match (agent_id, task_id) {
                (Some(aid), None) => conn.query_row(
                    completed_points_sql,
                    params![aid, blocking_states_json],
                    |row| row.get(0),
                )?,
                (None, Some(tid)) => conn.query_row(
                    completed_points_sql,
                    params![tid, blocking_states_json],
                    |row| row.get(0),
                )?,
                (Some(aid), Some(tid)) => conn.query_row(
                    completed_points_sql,
                    params![aid, tid, blocking_states_json],
                    |row| row.get(0),
                )?,
                (None, None) => {
                    conn.query_row(completed_points_sql, params![blocking_states_json], |row| {
                        row.get(0)
                    })?
                }
            };

            Ok(Stats {
                total_tasks,
                tasks_by_state,
                total_points,
                completed_points,
                total_time_estimate_ms,
                total_time_actual_ms,
                total_tokens_in,
                total_tokens_cached,
                total_tokens_out,
                total_tokens_thinking,
                total_tokens_image,
                total_tokens_audio,
                total_cost_usd,
            })
        })
    }
}
