//! Dependency operations and cycle detection.

use super::Database;
use crate::config::StatesConfig;
use crate::types::{Dependency, Task};
use anyhow::{anyhow, Result};
use rusqlite::params;
use std::collections::{HashSet, VecDeque};

impl Database {
    /// Add a dependency (from blocks to).
    pub fn add_dependency(&self, from_task_id: &str, to_task_id: &str) -> Result<()> {
        // Check for cycle
        if self.would_create_cycle(from_task_id, to_task_id)? {
            return Err(anyhow!("Adding this dependency would create a cycle"));
        }

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id) VALUES (?1, ?2)",
                params![from_task_id, to_task_id],
            )?;
            Ok(())
        })
    }

    /// Check if adding a dependency would create a cycle.
    fn would_create_cycle(&self, from_task_id: &str, to_task_id: &str) -> Result<bool> {
        // If from can reach to already, adding to -> from would create a cycle
        // Actually, we're adding from -> to (from blocks to)
        // A cycle would occur if to can already reach from

        self.with_conn(|conn| {
            let mut visited: HashSet<String> = HashSet::new();
            let mut queue: VecDeque<String> = VecDeque::new();
            queue.push_back(to_task_id.to_string());

            while let Some(current) = queue.pop_front() {
                if current == from_task_id {
                    return Ok(true); // Would create a cycle
                }

                if visited.contains(&current) {
                    continue;
                }
                visited.insert(current.clone());

                // Get all tasks that current blocks
                let mut stmt = conn.prepare(
                    "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1"
                )?;

                let deps: Vec<String> = stmt.query_map(params![&current], |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

                for dep in deps {
                    if !visited.contains(&dep) {
                        queue.push_back(dep);
                    }
                }
            }

            Ok(false)
        })
    }

    /// Remove a dependency.
    pub fn remove_dependency(&self, from_task_id: &str, to_task_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2",
                params![from_task_id, to_task_id],
            )?;
            Ok(())
        })
    }

    /// Get all dependencies.
    pub fn get_all_dependencies(&self) -> Result<Vec<Dependency>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT from_task_id, to_task_id FROM dependencies")?;

            let deps = stmt.query_map([], |row| {
                let from: String = row.get(0)?;
                let to: String = row.get(1)?;
                Ok(Dependency {
                    from_task_id: from,
                    to_task_id: to,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

            Ok(deps)
        })
    }

    /// Get tasks that block a given task.
    pub fn get_blockers(&self, task_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1"
            )?;

            let blockers = stmt.query_map(params![task_id], |row| {
                let id: String = row.get(0)?;
                Ok(id)
            })?
            .filter_map(|r| r.ok())
            .collect();

            Ok(blockers)
        })
    }

    /// Get tasks that a given task blocks.
    #[allow(dead_code)]
    pub fn get_blocking(&self, task_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1"
            )?;

            let blocking = stmt.query_map(params![task_id], |row| {
                let id: String = row.get(0)?;
                Ok(id)
            })?
            .filter_map(|r| r.ok())
            .collect();

            Ok(blocking)
        })
    }

    /// Get tasks that are blocked by incomplete dependencies.
    /// A task is blocked if any of its dependencies are in a blocking state.
    pub fn get_blocked_tasks(&self, states_config: &StatesConfig) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let blocking_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let blocking_clause = blocking_placeholders.join(", ");

            let sql = format!(
                "SELECT DISTINCT t.*
                 FROM tasks t
                 INNER JOIN dependencies d ON t.id = d.to_task_id
                 INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                 WHERE blocker.status IN ({})
                 AND t.status = ?1
                 ORDER BY t.created_at",
                blocking_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Build params: initial state + all blocking states
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(states_config.initial.clone()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let tasks = stmt
                .query_map(params_refs.as_slice(), super::tasks::parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Get tasks that are ready to be claimed (all dependencies satisfied).
    /// A task is ready if it's in the initial state, unclaimed, and all dependencies are not blocking.
    pub fn get_ready_tasks(
        &self,
        exclude_agent: Option<&str>,
        states_config: &StatesConfig,
    ) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            // Build IN clause from blocking_states for both dependency check and sibling check
            let blocking_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2)) // Start at ?2 (after initial state)
                .collect();
            let blocking_clause = blocking_placeholders.join(", ");

            // Build the SQL with dynamic blocking states check
            let sql = if exclude_agent.is_some() {
                format!(
                    "SELECT t.*
                     FROM tasks t
                     WHERE t.status = ?1
                     AND t.owner_agent IS NULL
                     AND NOT EXISTS (
                         SELECT 1 FROM dependencies d
                         INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                         WHERE d.to_task_id = t.id AND blocker.status IN ({})
                     )
                     AND NOT EXISTS (
                         SELECT 1 FROM tasks prev
                         WHERE prev.parent_id = t.parent_id
                         AND prev.sibling_order < t.sibling_order
                         AND t.join_mode = 'then'
                         AND prev.status IN ({})
                     )
                     AND (t.owner_agent IS NULL OR t.owner_agent != ?{})
                     ORDER BY
                         CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                         t.created_at",
                    blocking_clause,
                    blocking_clause,
                    states_config.blocking_states.len() + 2 // Position for exclude_agent param
                )
            } else {
                format!(
                    "SELECT t.*
                     FROM tasks t
                     WHERE t.status = ?1
                     AND t.owner_agent IS NULL
                     AND NOT EXISTS (
                         SELECT 1 FROM dependencies d
                         INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                         WHERE d.to_task_id = t.id AND blocker.status IN ({})
                     )
                     AND NOT EXISTS (
                         SELECT 1 FROM tasks prev
                         WHERE prev.parent_id = t.parent_id
                         AND prev.sibling_order < t.sibling_order
                         AND t.join_mode = 'then'
                         AND prev.status IN ({})
                     )
                     ORDER BY
                         CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                         t.created_at",
                    blocking_clause,
                    blocking_clause
                )
            };

            let mut stmt = conn.prepare(&sql)?;

            // Build params: initial state + blocking states (+ optionally exclude_agent)
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(states_config.initial.clone()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            if let Some(aid) = exclude_agent {
                params_vec.push(Box::new(aid.to_string()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let tasks = stmt
                .query_map(params_refs.as_slice(), super::tasks::parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Check if a task has unmet dependencies.
    #[allow(dead_code)]
    pub fn has_unmet_dependencies(
        &self,
        task_id: &str,
        states_config: &StatesConfig,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let blocking_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let blocking_clause = blocking_placeholders.join(", ");

            let sql = format!(
                "SELECT COUNT(*) FROM dependencies d
                 INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                 WHERE d.to_task_id = ?1 AND blocker.status IN ({})",
                blocking_clause
            );

            // Build params: task_id + blocking states
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(task_id.to_string()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let count: i32 = conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0))?;

            Ok(count > 0)
        })
    }
}


