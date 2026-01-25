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
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd,
                        COALESCE(SUM(metric_0), 0) as total_metric_0,
                        COALESCE(SUM(metric_1), 0) as total_metric_1,
                        COALESCE(SUM(metric_2), 0) as total_metric_2,
                        COALESCE(SUM(metric_3), 0) as total_metric_3,
                        COALESCE(SUM(metric_4), 0) as total_metric_4,
                        COALESCE(SUM(metric_5), 0) as total_metric_5,
                        COALESCE(SUM(metric_6), 0) as total_metric_6,
                        COALESCE(SUM(metric_7), 0) as total_metric_7
                    FROM tasks WHERE worker_id = ?1"
                        .to_string(),
                    vec![aid.to_string()],
                ),
                (None, Some(tid)) => (
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd,
                        COALESCE(SUM(metric_0), 0) as total_metric_0,
                        COALESCE(SUM(metric_1), 0) as total_metric_1,
                        COALESCE(SUM(metric_2), 0) as total_metric_2,
                        COALESCE(SUM(metric_3), 0) as total_metric_3,
                        COALESCE(SUM(metric_4), 0) as total_metric_4,
                        COALESCE(SUM(metric_5), 0) as total_metric_5,
                        COALESCE(SUM(metric_6), 0) as total_metric_6,
                        COALESCE(SUM(metric_7), 0) as total_metric_7
                    FROM tasks WHERE id IN (SELECT id FROM descendants)"
                        .to_string(),
                    vec![tid.to_string()],
                ),
                (Some(aid), Some(tid)) => (
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT
                        COUNT(*) as total_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        0 as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd,
                        COALESCE(SUM(metric_0), 0) as total_metric_0,
                        COALESCE(SUM(metric_1), 0) as total_metric_1,
                        COALESCE(SUM(metric_2), 0) as total_metric_2,
                        COALESCE(SUM(metric_3), 0) as total_metric_3,
                        COALESCE(SUM(metric_4), 0) as total_metric_4,
                        COALESCE(SUM(metric_5), 0) as total_metric_5,
                        COALESCE(SUM(metric_6), 0) as total_metric_6,
                        COALESCE(SUM(metric_7), 0) as total_metric_7
                    FROM tasks WHERE id IN (SELECT id FROM descendants) AND worker_id = ?1"
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
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd,
                        COALESCE(SUM(metric_0), 0) as total_metric_0,
                        COALESCE(SUM(metric_1), 0) as total_metric_1,
                        COALESCE(SUM(metric_2), 0) as total_metric_2,
                        COALESCE(SUM(metric_3), 0) as total_metric_3,
                        COALESCE(SUM(metric_4), 0) as total_metric_4,
                        COALESCE(SUM(metric_5), 0) as total_metric_5,
                        COALESCE(SUM(metric_6), 0) as total_metric_6,
                        COALESCE(SUM(metric_7), 0) as total_metric_7
                    FROM tasks"
                        .to_string(),
                    vec![],
                ),
            };

            // Query base stats - returns 14 columns now
            #[allow(clippy::type_complexity)]
            let (
                total_tasks,
                total_points,
                _completed_points_placeholder,
                total_time_estimate_ms,
                total_time_actual_ms,
                total_cost_usd,
                m0,
                m1,
                m2,
                m3,
                m4,
                m5,
                m6,
                m7,
            ): (
                i64,
                i64,
                i64,
                i64,
                i64,
                f64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
                i64,
            ) = if params_vec.is_empty() {
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
                        row.get(12)?,
                        row.get(13)?,
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
                        row.get(12)?,
                        row.get(13)?,
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
                        row.get(12)?,
                        row.get(13)?,
                    ))
                })?
            };

            // Now query task counts by state
            let count_sql = match (agent_id, task_id) {
                (Some(_aid), None) => {
                    "SELECT status, COUNT(*) as cnt FROM tasks WHERE worker_id = ?1 GROUP BY status"
                }
                (None, Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT status, COUNT(*) as cnt FROM tasks
                    WHERE id IN (SELECT id FROM descendants) GROUP BY status"
                }
                (Some(_aid), Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT status, COUNT(*) as cnt FROM tasks
                    WHERE id IN (SELECT id FROM descendants) AND worker_id = ?1 GROUP BY status"
                }
                (None, None) => "SELECT status, COUNT(*) as cnt FROM tasks GROUP BY status",
            };

            let mut tasks_by_status: HashMap<String, i64> = HashMap::new();

            // Initialize all defined states to 0
            for state in states_config.state_names() {
                tasks_by_status.insert(state.to_string(), 0);
            }

            // Query and fill in actual counts
            let mut stmt = conn.prepare(count_sql)?;
            let status_counts: Vec<(String, i64)> = if params_vec.is_empty() {
                stmt.query_map([], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else if params_vec.len() == 1 {
                stmt.query_map(params![params_vec[0].clone()], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                stmt.query_map(
                    params![params_vec[0].clone(), params_vec[1].clone()],
                    |row| {
                        let status: String = row.get(0)?;
                        let count: i64 = row.get(1)?;
                        Ok((status, count))
                    },
                )?
                .filter_map(|r| r.ok())
                .collect()
            };

            for (status, count) in status_counts {
                tasks_by_status.insert(status, count);
            }

            // Calculate completed_points (points for tasks in non-blocking states)
            let completed_points_sql = match (agent_id, task_id) {
                (Some(_aid), None) => {
                    "SELECT COALESCE(SUM(points), 0) FROM tasks 
                     WHERE worker_id = ?1 AND status NOT IN (SELECT value FROM json_each(?2))"
                }
                (None, Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?1
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT COALESCE(SUM(points), 0) FROM tasks
                    WHERE id IN (SELECT id FROM descendants)
                    AND status NOT IN (SELECT value FROM json_each(?2))"
                }
                (Some(_aid), Some(_tid)) => {
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT dep.to_task_id FROM dependencies dep
                        INNER JOIN descendants d ON dep.from_task_id = d.id
                        WHERE dep.dep_type = 'contains'
                    )
                    SELECT COALESCE(SUM(points), 0) FROM tasks
                    WHERE id IN (SELECT id FROM descendants) AND worker_id = ?1
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
                tasks_by_status,
                total_points,
                completed_points,
                total_time_estimate_ms,
                total_time_actual_ms,
                total_cost_usd,
                total_metrics: [m0, m1, m2, m3, m4, m5, m6, m7],
            })
        })
    }
}
