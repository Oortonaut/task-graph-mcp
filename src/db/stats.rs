//! Aggregation queries for statistics.

use super::Database;
use crate::types::Stats;
use anyhow::Result;
use rusqlite::params;
use uuid::Uuid;

impl Database {
    /// Get aggregate statistics.
    pub fn get_stats(&self, agent_id: Option<&str>, task_id: Option<Uuid>) -> Result<Stats> {
        self.with_conn(|conn| {
            let (sql, params_vec): (String, Vec<String>) = match (agent_id, task_id) {
                (Some(aid), None) => (
                    "SELECT
                        COUNT(*) as total_tasks,
                        SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending_tasks,
                        SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress_tasks,
                        SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_tasks,
                        SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed_tasks,
                        SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks WHERE owner_agent = ?1".to_string(),
                    vec![aid.to_string()],
                ),
                (None, Some(tid)) => {
                    // For a specific task, include the task and all its descendants
                    (
                        "WITH RECURSIVE descendants AS (
                            SELECT id FROM tasks WHERE id = ?1
                            UNION ALL
                            SELECT t.id FROM tasks t
                            INNER JOIN descendants d ON t.parent_id = d.id
                        )
                        SELECT
                            COUNT(*) as total_tasks,
                            SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending_tasks,
                            SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress_tasks,
                            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_tasks,
                            SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed_tasks,
                            SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled_tasks,
                            COALESCE(SUM(points), 0) as total_points,
                            COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points,
                            COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                            COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                            COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                            COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                            COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                            COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                            COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                            COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                            COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                        FROM tasks WHERE id IN (SELECT id FROM descendants)".to_string(),
                        vec![tid.to_string()],
                    )
                },
                (Some(aid), Some(tid)) => (
                    "WITH RECURSIVE descendants AS (
                        SELECT id FROM tasks WHERE id = ?2
                        UNION ALL
                        SELECT t.id FROM tasks t
                        INNER JOIN descendants d ON t.parent_id = d.id
                    )
                    SELECT
                        COUNT(*) as total_tasks,
                        SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending_tasks,
                        SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress_tasks,
                        SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_tasks,
                        SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed_tasks,
                        SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks WHERE id IN (SELECT id FROM descendants) AND owner_agent = ?1".to_string(),
                    vec![aid.to_string(), tid.to_string()],
                ),
                (None, None) => (
                    "SELECT
                        COUNT(*) as total_tasks,
                        SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending_tasks,
                        SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress_tasks,
                        SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed_tasks,
                        SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed_tasks,
                        SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled_tasks,
                        COALESCE(SUM(points), 0) as total_points,
                        COALESCE(SUM(CASE WHEN status = 'completed' THEN points ELSE 0 END), 0) as completed_points,
                        COALESCE(SUM(time_estimate_ms), 0) as total_time_estimate_ms,
                        COALESCE(SUM(time_actual_ms), 0) as total_time_actual_ms,
                        COALESCE(SUM(tokens_in), 0) as total_tokens_in,
                        COALESCE(SUM(tokens_cached), 0) as total_tokens_cached,
                        COALESCE(SUM(tokens_out), 0) as total_tokens_out,
                        COALESCE(SUM(tokens_thinking), 0) as total_tokens_thinking,
                        COALESCE(SUM(tokens_image), 0) as total_tokens_image,
                        COALESCE(SUM(tokens_audio), 0) as total_tokens_audio,
                        COALESCE(SUM(cost_usd), 0.0) as total_cost_usd
                    FROM tasks".to_string(),
                    vec![],
                ),
            };

            let stats = if params_vec.is_empty() {
                conn.query_row(&sql, [], |row| {
                    Ok(Stats {
                        total_tasks: row.get(0)?,
                        pending_tasks: row.get(1)?,
                        in_progress_tasks: row.get(2)?,
                        completed_tasks: row.get(3)?,
                        failed_tasks: row.get(4)?,
                        cancelled_tasks: row.get(5)?,
                        total_points: row.get(6)?,
                        completed_points: row.get(7)?,
                        total_time_estimate_ms: row.get(8)?,
                        total_time_actual_ms: row.get(9)?,
                        total_tokens_in: row.get(10)?,
                        total_tokens_cached: row.get(11)?,
                        total_tokens_out: row.get(12)?,
                        total_tokens_thinking: row.get(13)?,
                        total_tokens_image: row.get(14)?,
                        total_tokens_audio: row.get(15)?,
                        total_cost_usd: row.get(16)?,
                    })
                })?
            } else if params_vec.len() == 1 {
                conn.query_row(&sql, params![params_vec[0]], |row| {
                    Ok(Stats {
                        total_tasks: row.get(0)?,
                        pending_tasks: row.get(1)?,
                        in_progress_tasks: row.get(2)?,
                        completed_tasks: row.get(3)?,
                        failed_tasks: row.get(4)?,
                        cancelled_tasks: row.get(5)?,
                        total_points: row.get(6)?,
                        completed_points: row.get(7)?,
                        total_time_estimate_ms: row.get(8)?,
                        total_time_actual_ms: row.get(9)?,
                        total_tokens_in: row.get(10)?,
                        total_tokens_cached: row.get(11)?,
                        total_tokens_out: row.get(12)?,
                        total_tokens_thinking: row.get(13)?,
                        total_tokens_image: row.get(14)?,
                        total_tokens_audio: row.get(15)?,
                        total_cost_usd: row.get(16)?,
                    })
                })?
            } else {
                conn.query_row(&sql, params![params_vec[0], params_vec[1]], |row| {
                    Ok(Stats {
                        total_tasks: row.get(0)?,
                        pending_tasks: row.get(1)?,
                        in_progress_tasks: row.get(2)?,
                        completed_tasks: row.get(3)?,
                        failed_tasks: row.get(4)?,
                        cancelled_tasks: row.get(5)?,
                        total_points: row.get(6)?,
                        completed_points: row.get(7)?,
                        total_time_estimate_ms: row.get(8)?,
                        total_time_actual_ms: row.get(9)?,
                        total_tokens_in: row.get(10)?,
                        total_tokens_cached: row.get(11)?,
                        total_tokens_out: row.get(12)?,
                        total_tokens_thinking: row.get(13)?,
                        total_tokens_image: row.get(14)?,
                        total_tokens_audio: row.get(15)?,
                        total_cost_usd: row.get(16)?,
                    })
                })?
            };

            Ok(stats)
        })
    }
}
