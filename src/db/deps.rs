//! Dependency operations and cycle detection with typed dependencies.

use super::Database;
use crate::config::{AutoAdvanceConfig, DependenciesConfig, DependencyDisplay, StatesConfig};
use crate::types::{Dependency, Task};
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashSet, VecDeque};

/// Result of a relink operation.
#[derive(Debug)]
pub struct RelinkResult {
    /// Dependencies that were unlinked (from, to).
    pub unlinked: Vec<(String, String)>,
    /// Dependencies that were linked (from, to).
    pub linked: Vec<(String, String)>,
}

/// Check if adding a dependency would create a cycle (transaction-safe version).
fn would_create_cycle_in_tx(
    tx: &rusqlite::Transaction,
    from_task_id: &str,
    to_task_id: &str,
    dep_type: &str,
    deps_config: &DependenciesConfig,
) -> Result<bool> {
    let def = deps_config
        .get_definition(dep_type)
        .ok_or_else(|| anyhow!("Unknown dependency type: {}", dep_type))?;

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
            let mut stmt = tx.prepare(
                "SELECT to_task_id FROM dependencies d
                 JOIN (SELECT value FROM json_each(?1)) types
                 WHERE d.from_task_id = ?2 AND d.dep_type = types.value",
            )?;
            let vertical_types: Vec<&str> = deps_config.vertical_types();
            let types_json = serde_json::to_string(&vertical_types)?;
            stmt.query_map(params![&types_json, &current], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            // For horizontal deps, check all start-blocking relationships
            let mut stmt = tx.prepare(
                "SELECT to_task_id FROM dependencies d
                 JOIN (SELECT value FROM json_each(?1)) types
                 WHERE d.from_task_id = ?2 AND d.dep_type = types.value",
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
        let def = deps_config
        .get_definition(dep_type)
        .ok_or_else(|| anyhow!("Unknown dependency type: {}", dep_type))?;
        if def.display == DependencyDisplay::Vertical
            && let Some(existing_parent) = self.get_parent(to_task_id)?
            && existing_parent != from_task_id
        {
            return Err(anyhow!(
                "Task {} already has parent {}",
                to_task_id,
                existing_parent
            ));
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
        let def = deps_config
        .get_definition(dep_type)
        .ok_or_else(|| anyhow!("Unknown dependency type: {}", dep_type))?;

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
                         WHERE d.from_task_id = ?2 AND d.dep_type = types.value",
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
                         WHERE d.from_task_id = ?2 AND d.dep_type = types.value",
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

    /// Remove a typed dependency. Returns true if a row was deleted.
    pub fn remove_dependency(
        &self,
        from_task_id: &str,
        to_task_id: &str,
        dep_type: &str,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            let rows = conn.execute(
                "DELETE FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2 AND dep_type = ?3",
                params![from_task_id, to_task_id, dep_type],
            )?;
            Ok(rows > 0)
        })
    }

    /// Remove all dependencies of a given type from a task (outgoing edges).
    /// Returns the list of removed dependencies.
    pub fn remove_all_outgoing_dependencies(
        &self,
        from_task_id: &str,
        dep_type: &str,
    ) -> Result<Vec<Dependency>> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // First get the dependencies that will be removed
            let deps: Vec<Dependency> = {
                let mut stmt = tx.prepare(
                    "SELECT from_task_id, to_task_id, dep_type FROM dependencies WHERE from_task_id = ?1 AND dep_type = ?2"
                )?;
                stmt
                    .query_map(params![from_task_id, dep_type], |row| {
                        Ok(Dependency {
                            from_task_id: row.get(0)?,
                            to_task_id: row.get(1)?,
                            dep_type: row.get(2)?,
                        })
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
            };

            // Then delete them
            tx.execute(
                "DELETE FROM dependencies WHERE from_task_id = ?1 AND dep_type = ?2",
                params![from_task_id, dep_type],
            )?;

            tx.commit()?;
            Ok(deps)
        })
    }

    /// Remove all dependencies of a given type to a task (incoming edges).
    /// Returns the list of removed dependencies.
    pub fn remove_all_incoming_dependencies(
        &self,
        to_task_id: &str,
        dep_type: &str,
    ) -> Result<Vec<Dependency>> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // First get the dependencies that will be removed
            let deps: Vec<Dependency> = {
                let mut stmt = tx.prepare(
                    "SELECT from_task_id, to_task_id, dep_type FROM dependencies WHERE to_task_id = ?1 AND dep_type = ?2"
                )?;
                stmt
                    .query_map(params![to_task_id, dep_type], |row| {
                        Ok(Dependency {
                            from_task_id: row.get(0)?,
                            to_task_id: row.get(1)?,
                            dep_type: row.get(2)?,
                        })
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
            };

            // Then delete them
            tx.execute(
                "DELETE FROM dependencies WHERE to_task_id = ?1 AND dep_type = ?2",
                params![to_task_id, dep_type],
            )?;

            tx.commit()?;
            Ok(deps)
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
    /// Excludes soft-deleted tasks.
    pub fn get_blocked_tasks(
        &self,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
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

            // Build ORDER BY clause
            let order_clause = build_order_clause(sort_by, sort_order);

            let sql = format!(
                "SELECT DISTINCT t.*
                 FROM tasks t
                 INNER JOIN dependencies d ON t.id = d.to_task_id
                 INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                 WHERE d.dep_type IN ({})
                 AND blocker.status IN ({})
                 AND t.status = ?1
                 AND t.deleted_at IS NULL
                 ORDER BY {}",
                type_clause, state_clause, order_clause
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
    /// When agent_id is provided, also filters by agent's tag qualifications using junction tables.
    /// Excludes soft-deleted tasks.
    pub fn get_ready_tasks(
        &self,
        agent_id: Option<&str>,
        states_config: &StatesConfig,
        deps_config: &DependenciesConfig,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<Vec<Task>> {
        let start_blocking_types = deps_config.start_blocking_types();

        // Get agent tags if agent_id is provided (for junction table filtering)
        let agent_tags: Option<Vec<String>> = if let Some(aid) = agent_id {
            Some(self.get_agent_tags(aid)?)
        } else {
            None
        };

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

            // Build ORDER BY clause - for ready tasks, default is priority then created_at
            let order_clause = if sort_by.is_some() {
                build_order_clause(sort_by, sort_order)
            } else {
                // Default for ready: priority (high first), then created_at
                "CAST(t.priority AS INTEGER) DESC, t.created_at DESC".to_string()
            };

            // Track param index for agent tag filters
            let mut param_idx = type_start + start_blocking_types.len();

            // Build agent qualification filters using junction tables
            let (agent_needed_clause, agent_wanted_clause) = if let Some(ref tags) = agent_tags {
                // For agent_tags_all (AND): agent must have ALL needed tags
                // Count how many of the task's needed_tags match agent's tags
                // Either the task has no needed_tags, or all must match
                let needed_placeholders: Vec<String> = tags
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", param_idx + i))
                    .collect();
                param_idx += tags.len();

                let needed_clause = if needed_placeholders.is_empty() {
                    // Agent has no tags - only match tasks with no needed_tags
                    "AND NOT EXISTS (SELECT 1 FROM task_needed_tags WHERE task_id = t.id)"
                        .to_string()
                } else {
                    // Task must have no needed_tags OR agent must have all of them
                    format!(
                        "AND (
                            NOT EXISTS (SELECT 1 FROM task_needed_tags WHERE task_id = t.id)
                            OR (
                                SELECT COUNT(*) FROM task_needed_tags WHERE task_id = t.id
                            ) = (
                                SELECT COUNT(*) FROM task_needed_tags 
                                WHERE task_id = t.id AND tag IN ({})
                            )
                        )",
                        needed_placeholders.join(", ")
                    )
                };

                // For agent_tags_any (OR): agent must have at least ONE wanted tag
                let wanted_placeholders: Vec<String> = tags
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", param_idx + i))
                    .collect();

                let wanted_clause = if wanted_placeholders.is_empty() {
                    // Agent has no tags - only match tasks with no wanted_tags
                    "AND NOT EXISTS (SELECT 1 FROM task_wanted_tags WHERE task_id = t.id)"
                        .to_string()
                } else {
                    // Task must have no wanted_tags OR agent must have at least one
                    format!(
                        "AND (
                            NOT EXISTS (SELECT 1 FROM task_wanted_tags WHERE task_id = t.id)
                            OR EXISTS (
                                SELECT 1 FROM task_wanted_tags 
                                WHERE task_id = t.id AND tag IN ({})
                            )
                        )",
                        wanted_placeholders.join(", ")
                    )
                };

                (needed_clause, wanted_clause)
            } else {
                (String::new(), String::new())
            };

            let sql = format!(
                "SELECT t.*
                 FROM tasks t
                 WHERE t.status = ?1
                 AND t.worker_id IS NULL
                 AND t.deleted_at IS NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM dependencies d
                     INNER JOIN tasks blocker ON d.from_task_id = blocker.id
                     WHERE d.to_task_id = t.id 
                     AND d.dep_type IN ({})
                     AND blocker.status IN ({})
                 )
                 {}
                 {}
                 ORDER BY {}",
                type_clause, state_clause, agent_needed_clause, agent_wanted_clause, order_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Build params: initial state + blocking states + types + agent tags (twice if present)
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(states_config.initial.clone()));
            for state in &states_config.blocking_states {
                params_vec.push(Box::new(state.clone()));
            }
            for t in &start_blocking_types {
                params_vec.push(Box::new(t.to_string()));
            }
            // Add agent tags twice (once for needed_tags check, once for wanted_tags check)
            if let Some(ref tags) = agent_tags {
                for tag in tags {
                    params_vec.push(Box::new(tag.clone()));
                }
                for tag in tags {
                    params_vec.push(Box::new(tag.clone()));
                }
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let tasks: Vec<Task> = stmt
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

    /// Get tasks with tag-based filtering using junction tables for indexed lookups.
    /// - `tags_any`: Task must have at least one of these tags (OR)
    /// - `tags_all`: Task must have all of these tags (AND)
    /// - `qualified_for_agent_tags`: If provided, only return tasks where these tags satisfy the task's agent_tags_all/agent_tags_any
    ///
    /// Excludes soft-deleted tasks.
    #[allow(clippy::too_many_arguments)]
    pub fn list_tasks_with_tag_filters(
        &self,
        status: Option<Vec<String>>,
        owner: Option<&str>,
        parent_id: Option<Option<&str>>,
        tags_any: Option<Vec<String>>,
        tags_all: Option<Vec<String>>,
        qualified_for_agent_tags: Option<Vec<String>>,
        limit: Option<i32>,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut sql = String::from("SELECT t.* FROM tasks t WHERE t.deleted_at IS NULL");
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            let mut param_idx = 1;

            // Status filter (can be single or multiple)
            if let Some(ref statuses) = status {
                if statuses.len() == 1 {
                    sql.push_str(&format!(" AND t.status = ?{}", param_idx));
                    params_vec.push(Box::new(statuses[0].clone()));
                    param_idx += 1;
                } else if statuses.len() > 1 {
                    let placeholders: Vec<String> = statuses
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    sql.push_str(&format!(" AND t.status IN ({})", placeholders.join(", ")));
                    for s in statuses {
                        params_vec.push(Box::new(s.clone()));
                    }
                    param_idx += statuses.len();
                }
            }

            // Owner filter
            if let Some(o) = owner {
                sql.push_str(&format!(" AND t.worker_id = ?{}", param_idx));
                params_vec.push(Box::new(o.to_string()));
                param_idx += 1;
            }

            // Parent filter via dependencies table
            if let Some(p) = parent_id {
                match p {
                    Some(pid) => {
                        sql.push_str(&format!(" AND t.id IN (SELECT to_task_id FROM dependencies WHERE from_task_id = ?{} AND dep_type = 'contains')", param_idx));
                        params_vec.push(Box::new(pid.to_string()));
                        param_idx += 1;
                    }
                    None => {
                        // Root tasks: not contained by any other task
                        sql.push_str(" AND t.id NOT IN (SELECT to_task_id FROM dependencies WHERE dep_type = 'contains')");
                    }
                }
            }

            // tags_any: Task must have at least one of these tags (OR) - uses task_tags junction
            if let Some(ref any_tags) = tags_any
                && !any_tags.is_empty() {
                    let placeholders: Vec<String> = any_tags
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    sql.push_str(&format!(
                        " AND EXISTS (SELECT 1 FROM task_tags WHERE task_id = t.id AND tag IN ({}))",
                        placeholders.join(", ")
                    ));
                    for tag in any_tags {
                        params_vec.push(Box::new(tag.clone()));
                    }
                    param_idx += any_tags.len();
                }

            // tags_all: Task must have all of these tags (AND) - uses task_tags junction
            if let Some(ref all_tags) = tags_all
                && !all_tags.is_empty() {
                    let placeholders: Vec<String> = all_tags
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    // Count matching tags must equal total required tags
                    sql.push_str(&format!(
                        " AND (SELECT COUNT(*) FROM task_tags WHERE task_id = t.id AND tag IN ({})) = {}",
                        placeholders.join(", "),
                        all_tags.len()
                    ));
                    for tag in all_tags {
                        params_vec.push(Box::new(tag.clone()));
                    }
                    param_idx += all_tags.len();
                }

            // qualified_for: Agent's tags must satisfy task's requirements - uses junction tables
            if let Some(ref agent_tags) = qualified_for_agent_tags {
                // Agent must have ALL of task's agent_tags_all
                if agent_tags.is_empty() {
                    // Agent has no tags - only match tasks with no needed_tags
                    sql.push_str(" AND NOT EXISTS (SELECT 1 FROM task_needed_tags WHERE task_id = t.id)");
                    // And no wanted_tags
                    sql.push_str(" AND NOT EXISTS (SELECT 1 FROM task_wanted_tags WHERE task_id = t.id)");
                } else {
                    // For needed_tags (AND): task must have no needed_tags OR agent has all
                    let needed_placeholders: Vec<String> = agent_tags
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    sql.push_str(&format!(
                        " AND (
                            NOT EXISTS (SELECT 1 FROM task_needed_tags WHERE task_id = t.id)
                            OR (
                                SELECT COUNT(*) FROM task_needed_tags WHERE task_id = t.id
                            ) = (
                                SELECT COUNT(*) FROM task_needed_tags 
                                WHERE task_id = t.id AND tag IN ({})
                            )
                        )",
                        needed_placeholders.join(", ")
                    ));
                    for tag in agent_tags {
                        params_vec.push(Box::new(tag.clone()));
                    }
                    param_idx += agent_tags.len();

                    // For wanted_tags (OR): task must have no wanted_tags OR agent has at least one
                    let wanted_placeholders: Vec<String> = agent_tags
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", param_idx + i))
                        .collect();
                    sql.push_str(&format!(
                        " AND (
                            NOT EXISTS (SELECT 1 FROM task_wanted_tags WHERE task_id = t.id)
                            OR EXISTS (
                                SELECT 1 FROM task_wanted_tags 
                                WHERE task_id = t.id AND tag IN ({})
                            )
                        )",
                        wanted_placeholders.join(", ")
                    ));
                    for tag in agent_tags {
                        params_vec.push(Box::new(tag.clone()));
                    }
                    // param_idx += agent_tags.len(); // not needed, last param set
                }
            }

            // Build ORDER BY clause
            let order_clause = build_order_clause(sort_by, sort_order);
            sql.push_str(&format!(" ORDER BY {}", order_clause));

            // Apply limit in SQL
            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let tasks: Vec<Task> = stmt
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
                "SELECT tags FROM workers WHERE id = ?1",
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

    /// Atomically relink dependencies: unlink all prev_from→prev_to, then link all from→to.
    /// This is a transaction-safe operation for moving children between parents.
    /// Returns a result with unlinked and linked pairs.
    pub fn relink(
        &self,
        prev_from_ids: &[String],
        prev_to_ids: &[String],
        from_ids: &[String],
        to_ids: &[String],
        dep_type: &str,
        deps_config: &DependenciesConfig,
    ) -> Result<RelinkResult> {
        // Validate dependency type upfront
        if !deps_config.is_valid_dep_type(dep_type) {
            return Err(anyhow!(
                "Invalid dependency type '{}'. Valid types: {:?}",
                dep_type,
                deps_config.dep_type_names()
            ));
        }

        let def = deps_config
        .get_definition(dep_type)
        .ok_or_else(|| anyhow!("Unknown dependency type: {}", dep_type))?;
        let is_vertical = def.display == DependencyDisplay::Vertical;

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            let mut unlinked = Vec::new();
            let mut linked = Vec::new();
            let mut errors = Vec::new();

            // Phase 1: Unlink all prev_from × prev_to
            for prev_from in prev_from_ids {
                for prev_to in prev_to_ids {
                    let rows = tx.execute(
                        "DELETE FROM dependencies WHERE from_task_id = ?1 AND to_task_id = ?2 AND dep_type = ?3",
                        params![prev_from, prev_to, dep_type],
                    )?;
                    if rows > 0 {
                        unlinked.push((prev_from.clone(), prev_to.clone()));
                    }
                }
            }

            // Phase 2: Link all from × to (with validation)
            for from_id in from_ids {
                for to_id in to_ids {
                    // For vertical deps, check single-parent constraint
                    if is_vertical {
                        let existing_parent: Option<String> = tx.query_row(
                            "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1 AND dep_type = 'contains'",
                            params![to_id],
                            |row| row.get(0),
                        ).optional()?;

                        if let Some(ref parent) = existing_parent
                            && parent != from_id {
                                errors.push(format!(
                                    "Task {} already has parent {}",
                                    to_id, parent
                                ));
                                continue;
                            }
                    }

                    // Check for cycles using temporary view within transaction
                    if would_create_cycle_in_tx(&tx, from_id, to_id, dep_type, deps_config)? {
                        errors.push(format!(
                            "Adding dependency {}→{} would create a cycle",
                            from_id, to_id
                        ));
                        continue;
                    }

                    tx.execute(
                        "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id, dep_type) VALUES (?1, ?2, ?3)",
                        params![from_id, to_id, dep_type],
                    )?;
                    linked.push((from_id.clone(), to_id.clone()));
                }
            }

            if !errors.is_empty() {
                // Rollback on validation errors
                tx.rollback()?;
                return Err(anyhow!("Relink failed: {}", errors.join("; ")));
            }

            tx.commit()?;
            Ok(RelinkResult { unlinked, linked })
        })
    }

    // ============================================================================
    // Graph Traversal Methods for scan tool
    // ============================================================================

    /// Get predecessors (tasks that block this task) via blocks/follows dependencies.
    /// depth: 0 = none, N = N levels, -1 = all
    pub fn get_predecessors(&self, task_id: &str, depth: i32) -> Result<Vec<Task>> {
        if depth == 0 {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let mut visited: HashSet<String> = HashSet::new();
            let mut result: Vec<Task> = Vec::new();
            let mut current_level: Vec<String> = vec![task_id.to_string()];
            let mut levels_remaining = if depth < 0 { i32::MAX } else { depth };

            while !current_level.is_empty() && levels_remaining > 0 {
                let mut next_level: Vec<String> = Vec::new();

                for tid in &current_level {
                    // Get tasks that block this one (from_task_id blocks to_task_id)
                    let mut stmt = conn.prepare(
                        "SELECT DISTINCT d.from_task_id FROM dependencies d
                         WHERE d.to_task_id = ?1 AND d.dep_type IN ('blocks', 'follows')",
                    )?;

                    let predecessors: Vec<String> = stmt
                        .query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();

                    for pred_id in predecessors {
                        if !visited.contains(&pred_id) {
                            visited.insert(pred_id.clone());
                            if let Some(task) = get_task_by_id_internal(conn, &pred_id)? {
                                result.push(task);
                            }
                            next_level.push(pred_id);
                        }
                    }
                }

                current_level = next_level;
                levels_remaining -= 1;
            }

            Ok(result)
        })
    }

    /// Get successors (tasks that this task blocks) via blocks/follows dependencies.
    /// depth: 0 = none, N = N levels, -1 = all
    pub fn get_successors(&self, task_id: &str, depth: i32) -> Result<Vec<Task>> {
        if depth == 0 {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let mut visited: HashSet<String> = HashSet::new();
            let mut result: Vec<Task> = Vec::new();
            let mut current_level: Vec<String> = vec![task_id.to_string()];
            let mut levels_remaining = if depth < 0 { i32::MAX } else { depth };

            while !current_level.is_empty() && levels_remaining > 0 {
                let mut next_level: Vec<String> = Vec::new();

                for tid in &current_level {
                    // Get tasks that this one blocks (from_task_id blocks to_task_id)
                    let mut stmt = conn.prepare(
                        "SELECT DISTINCT d.to_task_id FROM dependencies d
                         WHERE d.from_task_id = ?1 AND d.dep_type IN ('blocks', 'follows')",
                    )?;

                    let successors: Vec<String> = stmt
                        .query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();

                    for succ_id in successors {
                        if !visited.contains(&succ_id) {
                            visited.insert(succ_id.clone());
                            if let Some(task) = get_task_by_id_internal(conn, &succ_id)? {
                                result.push(task);
                            }
                            next_level.push(succ_id);
                        }
                    }
                }

                current_level = next_level;
                levels_remaining -= 1;
            }

            Ok(result)
        })
    }

    /// Get ancestors (parent chain) via contains dependency.
    /// depth: 0 = none, N = N levels up, -1 = all
    pub fn get_ancestors(&self, task_id: &str, depth: i32) -> Result<Vec<Task>> {
        if depth == 0 {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let mut result: Vec<Task> = Vec::new();
            let mut current_id = task_id.to_string();
            let mut levels_remaining = if depth < 0 { i32::MAX } else { depth };

            while levels_remaining > 0 {
                // Get parent (from_task_id contains to_task_id)
                let parent_result: Result<String, rusqlite::Error> = conn.query_row(
                    "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1 AND dep_type = 'contains'",
                    params![&current_id],
                    |row| row.get(0),
                );

                match parent_result {
                    Ok(parent_id) => {
                        if let Some(task) = get_task_by_id_internal(conn, &parent_id)? {
                            result.push(task);
                        }
                        current_id = parent_id;
                        levels_remaining -= 1;
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => break,
                    Err(e) => return Err(e.into()),
                }
            }

            Ok(result)
        })
    }

    /// Get descendants (children tree) via contains dependency.
    /// depth: 0 = none, N = N levels down, -1 = all
    pub fn get_descendants(&self, task_id: &str, depth: i32) -> Result<Vec<Task>> {
        if depth == 0 {
            return Ok(vec![]);
        }

        self.with_conn(|conn| {
            let mut visited: HashSet<String> = HashSet::new();
            let mut result: Vec<Task> = Vec::new();
            let mut current_level: Vec<String> = vec![task_id.to_string()];
            let mut levels_remaining = if depth < 0 { i32::MAX } else { depth };

            while !current_level.is_empty() && levels_remaining > 0 {
                let mut next_level: Vec<String> = Vec::new();

                for tid in &current_level {
                    // Get children (from_task_id contains to_task_id)
                    let mut stmt = conn.prepare(
                        "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1 AND dep_type = 'contains'"
                    )?;

                    let children: Vec<String> = stmt
                        .query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();

                    for child_id in children {
                        if !visited.contains(&child_id) {
                            visited.insert(child_id.clone());
                            if let Some(task) = get_task_by_id_internal(conn, &child_id)? {
                                result.push(task);
                            }
                            next_level.push(child_id);
                        }
                    }
                }

                current_level = next_level;
                levels_remaining -= 1;
            }

            Ok(result)
        })
    }
}

/// Helper to get a task by ID within a connection context.
fn get_task_by_id_internal(conn: &Connection, task_id: &str) -> Result<Option<Task>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")?;
    let task = stmt
        .query_row(params![task_id], super::tasks::parse_task_row)
        .optional()?;
    Ok(task)
}

/// Get the IDs of tasks that block a given task from starting (unsatisfied dependencies).
/// A task blocks starting if it has a start-blocking dependency type and is in a blocking state.
/// This is a transaction-safe version for use within existing transactions.
pub(crate) fn get_unsatisfied_start_blockers_in_tx(
    conn: &Connection,
    task_id: &str,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
) -> Result<Vec<String>> {
    let start_blocking_types = deps_config.start_blocking_types();
    if start_blocking_types.is_empty() {
        return Ok(vec![]);
    }

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
        "SELECT blocker.id FROM dependencies d
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
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let blockers = stmt
        .query_map(params_refs.as_slice(), |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(blockers)
}

/// Propagate unblock effects when a task transitions out of a blocking state.
/// This is called after a task completes to find newly unblocked tasks and
/// optionally auto-advance them.
///
/// Returns (unblocked, auto_advanced):
/// - unblocked: All task IDs that are now ready (all dependencies satisfied)
/// - auto_advanced: Subset of unblocked that were actually transitioned (when enabled)
///
/// Algorithm:
/// 1. Find all tasks that have a start-blocking dependency on the completed task
/// 2. For each candidate:
///    - Skip if not in initial state or already claimed
///    - Check if ALL other start-blockers are also satisfied
///    - If fully unblocked → add to unblocked list
///    - If auto_advance enabled → also transition to target_state
/// 3. Return both lists
pub(crate) fn propagate_unblock_effects(
    conn: &Connection,
    completed_task_id: &str,
    agent_id: Option<&str>,
    states_config: &StatesConfig,
    deps_config: &DependenciesConfig,
    auto_advance: &AutoAdvanceConfig,
) -> Result<(Vec<String>, Vec<String>)> {
    // Get start-blocking dependency types
    let start_blocking_types = deps_config.start_blocking_types();
    if start_blocking_types.is_empty() {
        return Ok((vec![], vec![]));
    }

    // Find all tasks that depend on the completed task via start-blocking dependencies
    let type_placeholders: Vec<String> = start_blocking_types
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 2))
        .collect();
    let type_clause = type_placeholders.join(", ");

    let sql = format!(
        "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1 AND dep_type IN ({})",
        type_clause
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params_vec.push(Box::new(completed_task_id.to_string()));
    for t in &start_blocking_types {
        params_vec.push(Box::new(t.to_string()));
    }
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let dependent_task_ids: Vec<String> = stmt
        .query_map(params_refs.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut unblocked = Vec::new();
    let mut auto_advanced = Vec::new();
    let now = super::now_ms();

    // Determine if we should auto-advance
    let should_auto_advance = auto_advance.enabled && auto_advance.target_state.is_some();
    let target_state = auto_advance.target_state.clone();

    // Validate target state if auto-advance is enabled
    if should_auto_advance {
        let ts = target_state.as_ref().unwrap();
        if !states_config.is_valid_state(ts) {
            return Err(anyhow!(
                "Auto-advance target state '{}' is not a valid state",
                ts
            ));
        }
    }

    for task_id in dependent_task_ids {
        // Get the task
        let task = match get_task_by_id_internal(conn, &task_id)? {
            Some(t) => t,
            None => continue,
        };

        // Skip if not in initial state
        if task.status != states_config.initial {
            continue;
        }

        // Skip if task is already claimed
        if task.worker_id.is_some() {
            continue;
        }

        // Check if ALL start-blockers are now satisfied (not in blocking states)
        // Build query to count remaining blockers that are still blocking
        let state_placeholders: Vec<String> = states_config
            .blocking_states
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 3))
            .collect();
        let state_clause = state_placeholders.join(", ");

        // Reuse type_placeholders from above
        let type_start = states_config.blocking_states.len() + 3;
        let type_placeholders2: Vec<String> = start_blocking_types
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", type_start + i))
            .collect();
        let type_clause2 = type_placeholders2.join(", ");

        let blocker_sql = format!(
            "SELECT COUNT(*) FROM dependencies d
             INNER JOIN tasks blocker ON d.from_task_id = blocker.id
             WHERE d.to_task_id = ?1
             AND d.from_task_id != ?2
             AND d.dep_type IN ({})
             AND blocker.status IN ({})",
            type_clause2, state_clause
        );

        let mut blocker_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        blocker_params.push(Box::new(task_id.clone()));
        blocker_params.push(Box::new(completed_task_id.to_string()));
        for state in &states_config.blocking_states {
            blocker_params.push(Box::new(state.clone()));
        }
        for t in &start_blocking_types {
            blocker_params.push(Box::new(t.to_string()));
        }
        let blocker_refs: Vec<&dyn rusqlite::ToSql> =
            blocker_params.iter().map(|b| b.as_ref()).collect();

        let remaining_blockers: i32 =
            conn.query_row(&blocker_sql, blocker_refs.as_slice(), |row| row.get(0))?;

        if remaining_blockers > 0 {
            continue; // Still blocked by other tasks
        }

        // Task is now fully unblocked - add to unblocked list
        unblocked.push(task_id.clone());

        // Auto-advance if enabled and transition is valid
        if should_auto_advance {
            let ts = target_state.as_ref().unwrap();

            // Validate transition from initial to target_state
            if !states_config.is_valid_transition(&states_config.initial, ts) {
                // Skip auto-advance for this task - transition not allowed
                continue;
            }

            // Auto-advance: update the task's status
            conn.execute(
                "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![ts, now, &task_id],
            )?;

            // Record state transition
            let reason = format!("auto-advanced: blocker '{}' completed", completed_task_id);
            super::state_transitions::record_state_transition(
                conn,
                &task_id,
                ts,
                agent_id,
                Some(&reason),
                states_config,
            )?;

            auto_advanced.push(task_id);
        }
    }

    Ok((unblocked, auto_advanced))
}
