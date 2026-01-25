//! Dependency operations and cycle detection with typed dependencies.

use super::Database;
use crate::config::{DependenciesConfig, DependencyDisplay, StatesConfig};
use crate::types::{Dependency, Task};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use std::collections::{HashSet, VecDeque};

impl Database {
    /// Add a typed dependency (from blocks/contains to).
    pub fn add_dependency(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        dep_type: &str,
        deps_config: &DependenciesConfig,
    ) -> Result<()> {
        // Validate dependency type
        if !deps_config.is_valid_dep_type(dep_type) {
            return Err(anyhow!(
                "Invalid dependency type '{}'. Valid types: {:?}",
                dep_type,
                deps_config.dep_type_names()
            ));
        }

        // For vertical (contains) dependencies, check single-parent constraint
        let def = deps_config.get_definition(dep_type).unwrap();
        if def.display == DependencyDisplay::Vertical {
            if let Some(existing_parent) = self.get_parent(to_task_id)? {
                if existing_parent != from_task_id {
                    return Err(anyhow!(
                        "Task {} already has parent {}",
                        to_task_id,
                        existing_parent
                    ));
                }
            }
        }

        // Check for cycle in the appropriate graph
        if self.would_create_cycle(from_task_id, to_task_id, dep_type, deps_config)? {
            return Err(anyhow!("Adding this dependency would create a cycle"));
        }

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type) VALUES (?1, ?2, ?3)",
                params![from_task_id, to_task_id, dep_type],
            )?;
            Ok(())
        })
    }

    /// Check if adding a dependency would create a cycle.
    /// For horizontal deps: check cycle in the start-blocking graph.
    /// For vertical deps: check containment cycle.
    pub fn would_create_cycle(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        dep_type: &str,
        deps_config: &DependenciesConfig,
    ) -> Result<bool> {
        let def = deps_config.get_definition(dep_type).unwrap();

        self.with_conn(|conn| {
            // A cycle would occur if to_task can already reach from_task
            // through the same "graph" (horizontal or vertical)
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

                // Get all tasks that current points to (in the relevant graph)
                let deps: Vec<String> = if def.display == DependencyDisplay::Vertical {
                    // For vertical deps, only check containment relationships
                    let mut stmt = conn.prepare(
                        "SELECT to_task_id FROM dependencies d
                         JOIN (SELECT value FROM json_each(?1)) types
                         WHERE d.from_task_id = ?2 AND d.dep_type = types.value"
                    )?;
                    let vertical_types: Vec<&str> = deps_config.vertical_types();
                    let types_json = serde_json::to_string(&vertical_types)?;
                    stmt.query_map(params![&types_json, &current], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect()
                } else {
                    // For horizontal deps, check all start-blocking relationships
                    let mut stmt = conn.prepare(
                        "SELECT to_task_id FROM dependencies d
                         JOIN (SELECT value FROM json_each(?1)) types
                         WHERE d.from_task_id = ?2 AND d.dep_type = types.value"
                    )?;
                    let start_blocking: Vec<&str> = deps_config.start_blocking_types();
                    let types_json = serde_json::to_string(&start_blocking)?;
                    stmt.query_map(params![&types_json, &current], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect()
                };

                for dep in deps {
                    if !visited.contains(&dep) {
                        queue.push_back(dep);
                    }
                }
            }

            Ok(false)
        })
    }

    /// Remove a typed dependency.
    pub fn remove_dependency(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        dep_type: &str,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2 AND dep_type = ?3",
                params![from_task_id, to_task_id, dep_type],
            )?;
            Ok(())
        })
    }

    /// Get all dependencies.
    pub fn get_all_dependencies(&self) -> Result<Vec<Dependency>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT from_task_id, to_task_id, dep_type FROM dependencies")?;

            let deps = stmt
                .query_map([], |row| {
                    let from: String = row.get(0)?;
                    let to: String = row.get(1)?;
                    let dep_type: String = row.get(2)?;
                    Ok(Dependency {
                        from_task_id: from,
                        to_task_id: to,
                        dep_type,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(deps)
        })
    }

    /// Get dependencies of a specific type for a task.
    pub fn get_dependencies_by_type(
        &self,
        task_id: &str,
        dep_type: &str,
        direction: &str,
    ) -> Result<Vec<Dependency>> {
        self.with_conn(|conn| {
            let sql = if direction == "incoming" {
                "SELECT from_task_id, to_task_id, dep_type FROM dependencies WHERE to_task_id = ?1 AND dep_type = ?2"
            } else {
                "SELECT from_task_id, to_task_id, dep_type FROM dependencies WHERE from_task_id = ?1 AND dep_type = ?2"
            };

            let mut stmt = conn.prepare(sql)?;

            let deps = stmt
                .query_map(params![task_id, dep_type], |row| {
                    let from: String = row.get(0)?;
                    let to: String = row.get(1)?;
                    let dep_type: String = row.get(2)?;
                    Ok(Dependency {
                        from_task_id: from,
                        to_task_id: to,
                        dep_type,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(deps)
        })
    }

    /// Get tasks that block a given task from starting (dep_type with blocks: start).
    pub fn get_start_blockers(
        &self,
        task_id: &str,
        deps_config: &DependenciesConfig,
    ) -> Result<Vec<String>> {
        let start_blocking_types = deps_config.start_blocking_types();
        if start_blocking_types.is_empty() {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let placeholders: String = start_blocking_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1 AND dep_type IN ({})",
                placeholders
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(task_id.to_string()));
            for t in &start_blocking_types {
                params_vec.push(Box::new(t.to_string()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let blockers = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(blockers)
        })
    }

    /// Get tasks that block a given task from completing (dep_type with blocks: completion).
    /// For a parent task, this returns children that must complete first.
    pub fn get_completion_blockers(
        &self,
        task_id: &str,
        deps_config: &DependenciesConfig,
    ) -> Result<Vec<String>> {
        let completion_blocking_types = deps_config.completion_blocking_types();
        if completion_blocking_types.is_empty() {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let placeholders: String = completion_blocking_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");

            // For completion blockers, we look at outgoing edges (from_task_id = parent)
            // because "contains" means parent -> child, and child blocks parent completion
            let sql = format!(
                "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1 AND dep_type IN ({})",
                placeholders
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(task_id.to_string()));
            for t in &completion_blocking_types {
                params_vec.push(Box::new(t.to_string()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let blockers = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(blockers)
        })
    }

    /// Get the parent of a task (via 'contains' dependency).
    pub fn get_parent(&self, task_id: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let result: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1 AND dep_type = 'contains'",
                params![task_id],
                |row| row.get(0),
            );

            match result {
                Ok(parent_id) => Ok(Some(parent_id)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Get children of a task (via 'contains' dependency).
    pub fn get_children_ids(&self, task_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1 AND dep_type = 'contains'"
            )?;

            let children = stmt
                .query_map(params![task_id], |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(children)
        })
    }

    /// Get all tasks that block a given task (backwards compatible).
    /// Returns tasks from both 'blocks' and 'follows' dependencies.
    pub fn get_blockers(&self, task_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT from_task_id FROM dependencies 
                 WHERE to_task_id = ?1 AND dep_type IN ('blocks', 'follows')",
            )?;

            let blockers = stmt
                .query_map(params![task_id], |row| {
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
                "SELECT to_task_id FROM dependencies 
                 WHERE from_task_id = ?1 AND dep_type IN ('blocks', 'follows')",
            )?;

            let blocking = stmt
                .query_map(params![task_id], |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(blocking)
        })
    }

    /// Get tasks that are blocked by incomplete start dependencies.
    /// A task is blocked if any of its start-blocking dependencies are in a blocking state.
    pub fn get_blocked_tasks(
        &self,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
    ) -> Result<Vec<Task>> {
        let start_blocking_types = deps_config.start_blocking_types();
        if start_blocking_types.is_empty() {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let state_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let state_clause = state_placeholders.join(", ");

            // Build IN clause from start_blocking_types
            let type_start = states_config.blocking_states.len() + 2;
            let type_placeholders: Vec<String> = start_blocking_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", type_start + i))
                .collect();
            let type_clause = type_placeholders.join(", ");

            let sql = format!(
                "SELECT DISTINCT t.*
                 FROM tasks t
                 INNER JOIN dependencies d ON t.id = d.to_task_id
                 INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                 WHERE d.dep_type IN ({})
                 AND blocker.status IN ({})
                 AND t.status = ?1
                 ORDER BY t.created_at",
                type_clause, state_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Build params: initial state + blocking states + start_blocking_types
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(states_config.initial.clone()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            for t in &start_blocking_types {
                params_vec.push(Box::new(t.to_string()));
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

    /// Get tasks that are ready to be claimed (all start dependencies satisfied).
    /// A task is ready if it's in the initial state, unclaimed, and all start-blocking deps are not blocking.
    pub fn get_ready_tasks(
        &self,
        exclude_agent: Option<&str>,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
    ) -> Result<Vec<Task>> {
        let start_blocking_types = deps_config.start_blocking_types();

        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let state_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let state_clause = state_placeholders.join(", ");

            // Build IN clause from start_blocking_types
            let type_start = states_config.blocking_states.len() + 2;
            let type_placeholders: Vec<String> = start_blocking_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", type_start + i))
                .collect();
            let type_clause = type_placeholders.join(", ");

            let exclude_param_pos = type_start + start_blocking_types.len();

            // Build the SQL with dynamic blocking states and dependency types
            let sql = if exclude_agent.is_some() {
                format!(
                    "SELECT t.*
                     FROM tasks t
                     WHERE t.status = ?1
                     AND t.owner_agent IS NULL
                     AND NOT EXISTS (
                         SELECT 1 FROM dependencies d
                         INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                         WHERE d.to_task_id = t.id 
                         AND d.dep_type IN ({})
                         AND blocker.status IN ({})
                     )
                     AND (t.owner_agent IS NULL OR t.owner_agent != ?{})
                     ORDER BY
                         CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                         t.created_at",
                    type_clause, state_clause, exclude_param_pos
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
                         WHERE d.to_task_id = t.id 
                         AND d.dep_type IN ({})
                         AND blocker.status IN ({})
                     )
                     ORDER BY
                         CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                         t.created_at",
                    type_clause, state_clause
                )
            };

            let mut stmt = conn.prepare(&sql)?;

            // Build params: initial state + blocking states + types + exclude_agent
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(states_config.initial.clone()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            for t in &start_blocking_types {
                params_vec.push(Box::new(t.to_string()));
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

    /// Check if a task has unmet start dependencies.
    #[allow(dead_code)]
    pub fn has_unmet_start_dependencies(
        &self,
        task_id: &str,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
    ) -> Result<bool> {
        let start_blocking_types = deps_config.start_blocking_types();
        if start_blocking_types.is_empty() {
            return Ok(false);
        }

        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let state_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let state_clause = state_placeholders.join(", ");

            // Build IN clause from types
            let type_start = states_config.blocking_states.len() + 2;
            let type_placeholders: Vec<String> = start_blocking_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", type_start + i))
                .collect();
            let type_clause = type_placeholders.join(", ");

            let sql = format!(
                "SELECT COUNT(*) FROM dependencies d
                 INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                 WHERE d.to_task_id = ?1 
                 AND d.dep_type IN ({})
                 AND blocker.status IN ({})",
                type_clause, state_clause
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(task_id.to_string()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            for t in &start_blocking_types {
                params_vec.push(Box::new(t.to_string()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let count: i32 = conn.query_row(&sql, params_refs.as_slice(), |row| row.get(0))?;

            Ok(count > 0)
        })
    }

    /// Check if a task has incomplete children (blocking completion).
    pub fn has_incomplete_children(
        &self,
        task_id: &str,
        states_config: &StatesConfig,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            // Build IN clause from blocking_states
            let state_placeholders: Vec<String> = states_config
                .blocking_states
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let state_clause = state_placeholders.join(", ");

            let sql = format!(
                "SELECT COUNT(*) FROM dependencies d
                 INNER JOIN tasks child ON d.to_task_id = child.id
                 WHERE d.from_task_id = ?1 
                 AND d.dep_type = 'contains'
                 AND child.status IN ({})",
                state_clause
            );

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

    /// Get tasks with tag-based filtering.
    /// - `tags_any`: Task must have at least one of these tags (OR)
    /// - `tags_all`: Task must have all of these tags (AND)
    /// - `qualified_for_agent_tags`: If provided, only return tasks where these tags satisfy the task's needed_tags/wanted_tags
    pub fn list_tasks_with_tag_filters(
        &self,
        status: Option<Vec<String>>,
        owner: Option<&str>,
        parent_id: Option<Option<&str>>,
        tags_any: Option<Vec<String>>,
        tags_all: Option<Vec<String>>,
        qualified_for_agent_tags: Option<Vec<String>>,
        limit: Option<i32>,
    ) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut param_idx = 1;

            // Status filter (can be single or multiple)
            if let Some(ref statuses) = status {
                if statuses.len() == 1 {
                    sql.push_str(&format!(" AND status = ?{}", param_idx));
                    params_vec.push(Box::new(statuses[0].clone()));
                    param_idx += 1;
                } else if statuses.len() > 1 {
                    let placeholders: Vec<String> = statuses
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    sql.push_str(&format!(" AND status IN ({})", placeholders.join(", ")));
                    for s in statuses {
                        params_vec.push(Box::new(s.clone()));
                    }
                    param_idx += statuses.len();
                }
            }

            // Owner filter
            if let Some(o) = owner {
                sql.push_str(&format!(" AND owner_agent = ?{}", param_idx));
                params_vec.push(Box::new(o.to_string()));
                param_idx += 1;
            }

            // Parent filter via dependencies table
            if let Some(p) = parent_id {
                match p {
                    Some(pid) => {
                        sql.push_str(&format!(" AND id IN (SELECT to_task_id FROM dependencies WHERE from_task_id = ?{} AND dep_type = 'contains')", param_idx));
                        params_vec.push(Box::new(pid.to_string()));
                        param_idx += 1;
                    }
                    None => {
                        // Root tasks: not contained by any other task
                        sql.push_str(" AND id NOT IN (SELECT to_task_id FROM dependencies WHERE dep_type = 'contains')");
                    }
                }
            }

            // tags_any: Task has at least one of these tags
            if let Some(ref tags) = tags_any {
                if !tags.is_empty() {
                    let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
                    sql.push_str(&format!(
                        " AND EXISTS (
                            SELECT 1 FROM json_each(tags) AS task_tag
                            WHERE task_tag.value IN (SELECT value FROM json_each(?{}))
                        )", param_idx
                    ));
                    params_vec.push(Box::new(tags_json));
                    param_idx += 1;
                }
            }

            // tags_all: Task has all of these tags
            if let Some(ref tags) = tags_all {
                if !tags.is_empty() {
                    let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
                    let count = tags.len();
                    sql.push_str(&format!(
                        " AND (
                            SELECT COUNT(DISTINCT task_tag.value) FROM json_each(tags) AS task_tag
                            WHERE task_tag.value IN (SELECT value FROM json_each(?{}))
                        ) = {}", param_idx, count
                    ));
                    params_vec.push(Box::new(tags_json));
                    param_idx += 1;
                }
            }

            // qualified_for: Agent's tags satisfy task's needed_tags/wanted_tags
            if let Some(ref agent_tags) = qualified_for_agent_tags {
                let agent_tags_json = serde_json::to_string(agent_tags).unwrap_or_else(|_| "[]".to_string());
                sql.push_str(&format!(
                    " AND (
                        -- Agent has ALL needed_tags (or task has no needed_tags)
                        (needed_tags IS NULL OR needed_tags = '[]' OR (
                            SELECT COUNT(DISTINCT needed.value) FROM json_each(needed_tags) AS needed
                            WHERE needed.value IN (SELECT value FROM json_each(?{}))
                        ) = json_array_length(needed_tags))
                        AND
                        -- Agent has at least ONE wanted_tag (or task has no wanted_tags)
                        (wanted_tags IS NULL OR wanted_tags = '[]' OR EXISTS (
                            SELECT 1 FROM json_each(?{}) AS agent_tag
                            WHERE agent_tag.value IN (SELECT value FROM json_each(wanted_tags))
                        ))
                    )", param_idx, param_idx + 1
                ));
                params_vec.push(Box::new(agent_tags_json.clone()));
                params_vec.push(Box::new(agent_tags_json));
            }

            sql.push_str(" ORDER BY created_at DESC");

            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let tasks = stmt
                .query_map(params_refs.as_slice(), super::tasks::parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tasks)
        })
    }

    /// Get agent tags by agent ID.
    pub fn get_agent_tags(&self, agent_id: &str) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let result: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT tags FROM agents WHERE id = ?1",
                params![agent_id],
                |row| row.get(0),
            );

            match result {
                Ok(tags_json) => {
                    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                    Ok(tags)
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(vec![]),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Internal helper: add dependency within a transaction (used by tasks.rs).
    pub(super) fn add_dependency_internal(
        conn: &Connection,
        from_task_id: &str,
        to_task_id: &str,
        dep_type: &str,
    ) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type) VALUES (?1, ?2, ?3)",
            params![from_task_id, to_task_id, dep_type],
        )?;
        Ok(())
    }
}
