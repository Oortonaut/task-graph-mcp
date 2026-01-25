//! Task CRUD and tree operations.

use super::state_transitions::record_state_transition;
use super::{now_ms, Database};
use crate::config::StatesConfig;
use crate::types::{parse_priority, priority_to_str, Priority, Task, TaskSummary, TaskTree, TaskTreeInput, Worker, PRIORITY_MEDIUM};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;
use uuid::Uuid;

pub fn parse_task_row(row: &Row) -> rusqlite::Result<Task> {
    let id: String = row.get("id")?;
    let title: String = row.get("title")?;
    let description: Option<String> = row.get("description")?;
    let status: String = row.get("status")?;
    let priority: String = row.get("priority")?;
    let owner_agent: Option<String> = row.get("owner_agent")?;
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

    let tokens_in: i64 = row.get("tokens_in")?;
    let tokens_cached: i64 = row.get("tokens_cached")?;
    let tokens_out: i64 = row.get("tokens_out")?;
    let tokens_thinking: i64 = row.get("tokens_thinking")?;
    let tokens_image: i64 = row.get("tokens_image")?;
    let tokens_audio: i64 = row.get("tokens_audio")?;
    let cost_usd: f64 = row.get("cost_usd")?;
    let user_metrics_json: Option<String> = row.get("user_metrics")?;

    let created_at: i64 = row.get("created_at")?;
    let updated_at: i64 = row.get("updated_at")?;

    Ok(Task {
        id,
        title,
        description,
        status,
        priority: parse_priority(&priority),
        owner_agent,
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
        tokens_in,
        tokens_cached,
        tokens_out,
        tokens_thinking,
        tokens_image,
        tokens_audio,
        cost_usd,
        user_metrics: user_metrics_json.map(|s| serde_json::from_str(&s).unwrap_or_default()),
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
        "SELECT id, tags, max_claims, registered_at, last_heartbeat
         FROM workers WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![worker_id], |row| {
        let id: String = row.get(0)?;
        let tags_json: String = row.get(1)?;
        let max_claims: i32 = row.get(2)?;
        let registered_at: i64 = row.get(3)?;
        let last_heartbeat: i64 = row.get(4)?;

        Ok((id, tags_json, max_claims, registered_at, last_heartbeat))
    });

    match result {
        Ok((id, tags_json, max_claims, registered_at, last_heartbeat)) => {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(Some(Worker {
                id,
                tags,
                max_claims,
                registered_at,
                last_heartbeat,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Internal helper to get claim count using an existing connection (avoids deadlock).
fn get_claim_count_internal(conn: &Connection, agent_id: &str) -> Result<i32> {
    let count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE owner_agent = ?1 AND status = 'in_progress'",
        params![agent_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

impl Database {
    /// Create a new task.
    /// If id is provided, uses it as the task ID; otherwise generates UUID7.
    /// If parent_id is provided, creates a 'contains' dependency from parent to this task.
    pub fn create_task(
        &self,
        id: Option<String>,
        description: String,
        parent_id: Option<String>,
        priority: Option<Priority>,
        points: Option<i32>,
        time_estimate_ms: Option<i64>,
        needed_tags: Option<Vec<String>>,
        wanted_tags: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let task_id = id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let now = now_ms();
        let priority = priority.unwrap_or(PRIORITY_MEDIUM);
        let initial_status = &states_config.initial;

        let needed_tags = needed_tags.unwrap_or_default();
        let wanted_tags = wanted_tags.unwrap_or_default();
        let tags = tags.unwrap_or_default();
        let needed_tags_json = serde_json::to_string(&needed_tags)?;
        let wanted_tags_json = serde_json::to_string(&wanted_tags)?;
        let tags_json = serde_json::to_string(&tags)?;

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            tx.execute(
                "INSERT INTO tasks (
                    id, title, description, status, priority,
                    needed_tags, wanted_tags, tags, points, time_estimate_ms, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    &task_id,
                    &description,  // Use description as title
                    &description,  // Also store as description
                    initial_status,
                    priority_to_str(priority),
                    needed_tags_json,
                    wanted_tags_json,
                    tags_json,
                    points,
                    time_estimate_ms,
                    now,
                    now,
                ],
            )?;

            // Create 'contains' dependency if parent_id is provided
            if let Some(ref pid) = parent_id {
                Database::add_dependency_internal(&tx, pid, &task_id, "contains")?;
            }

            // Record initial state
            record_state_transition(&tx, &task_id, initial_status, None, None, states_config)?;

            tx.commit()?;

            Ok(Task {
                id: task_id,
                title: description.clone(),
                description: Some(description),
                status: initial_status.clone(),
                priority,
                owner_agent: None,
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
                tokens_in: 0,
                tokens_cached: 0,
                tokens_out: 0,
                tokens_thinking: 0,
                tokens_image: 0,
                tokens_audio: 0,
                cost_usd: 0.0,
                user_metrics: None,
                created_at: now,
                updated_at: now,
            })
        })
    }

    /// Create a task tree from nested input.
    /// Uses 'contains' dependencies for parent-child relationships
    /// and 'follows' dependencies for sequential children (when parallel=false).
    pub fn create_task_tree(
        &self,
        input: TaskTreeInput,
        parent_id: Option<String>,
        states_config: &StatesConfig,
    ) -> Result<(String, Vec<String>)> {
        let mut all_ids = Vec::new();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            let root_id = create_tree_recursive(
                &tx,
                &input,
                parent_id.as_deref(),
                None, // no previous sibling for root
                &mut all_ids,
                states_config,
            )?;
            tx.commit()?;
            Ok((root_id, all_ids))
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
            if task.status != new_status {
                if !states_config.is_valid_transition(&task.status, &new_status) {
                    let exits = states_config.get_exits(&task.status);
                    return Err(anyhow!(
                        "Invalid transition from '{}' to '{}'. Allowed transitions: {:?}",
                        task.status,
                        new_status,
                        exits
                    ));
                }
            }

            // Handle status transitions for timestamps
            // Set started_at when first entering a timed state
            let started_at =
                if task.started_at.is_none() && states_config.is_timed_state(&new_status) {
                    Some(now)
                } else {
                    task.started_at
                };

            // Set completed_at when entering a terminal state
            let completed_at = if states_config.is_terminal_state(&new_status) {
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
                    task.owner_agent.as_deref(),
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
                    priority_to_str(new_priority),
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
    pub fn update_task_unified(
        &self,
        task_id: &str,
        agent_id: &str,
        title: Option<String>,
        description: Option<Option<String>>,
        status: Option<String>,
        priority: Option<Priority>,
        points: Option<Option<i32>>,
        tags: Option<Vec<String>>,
        force: bool,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            let task =
                get_task_internal(&tx, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

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
            if task.status != new_status {
                if !states_config.is_valid_transition(&task.status, &new_status) {
                    let exits = states_config.get_exits(&task.status);
                    return Err(anyhow!(
                        "Invalid transition from '{}' to '{}'. Allowed transitions: {:?}",
                        task.status,
                        new_status,
                        exits
                    ));
                }
            }

            // Determine ownership changes based on state transition
            let new_is_timed = states_config.is_timed_state(&new_status);
            let new_is_terminal = states_config.is_terminal_state(&new_status);
            let current_owner = task.owner_agent.as_deref();
            let is_owned_by_agent = current_owner == Some(agent_id);
            let is_owned_by_other = current_owner.is_some() && !is_owned_by_agent;

            let mut new_owner: Option<String> = task.owner_agent.clone();
            let mut new_claimed_at: Option<i64> = task.claimed_at;

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

                // Get the agent
                let agent = get_worker_internal(&tx, agent_id)?
                    .ok_or_else(|| anyhow!("Agent not found"))?;

                // Check claim limit (skip if force)
                let current_claims = get_claim_count_internal(&tx, agent_id)?;
                if !force && current_claims >= agent.max_claims {
                    return Err(anyhow!("Agent has reached claim limit"));
                }

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
            if !new_is_timed && !new_is_terminal && task.owner_agent.is_some() {
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
                if let Some(ref current_owner) = task.owner_agent {
                    if current_owner != agent_id && !force {
                        return Err(anyhow!("Task is not owned by this agent"));
                    }
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

            let completed_at = if new_is_terminal {
                Some(now)
            } else {
                task.completed_at
            };

            // Record state transition if status changed
            if task.status != new_status {
                record_state_transition(
                    &tx,
                    task_id,
                    &new_status,
                    new_owner.as_deref(),
                    None,
                    states_config,
                )?;
            }

            tx.execute(
                "UPDATE tasks SET
                    title = ?1, description = ?2, status = ?3, priority = ?4,
                    points = ?5, started_at = ?6, completed_at = ?7, updated_at = ?8,
                    tags = ?9, owner_agent = ?10, claimed_at = ?11
                WHERE id = ?12",
                params![
                    new_title,
                    new_description,
                    new_status,
                    priority_to_str(new_priority),
                    new_points,
                    started_at,
                    completed_at,
                    now,
                    serde_json::to_string(&new_tags)?,
                    new_owner,
                    new_claimed_at,
                    task_id,
                ],
            )?;

            tx.commit()?;

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
                owner_agent: new_owner,
                claimed_at: new_claimed_at,
                ..task
            })
        })
    }

    /// Delete a task.
    pub fn delete_task(&self, task_id: &str, cascade: bool) -> Result<()> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

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

            tx.commit()?;
            Ok(())
        })
    }

    /// List tasks with optional filters.
    pub fn list_tasks(
        &self,
        status: Option<&str>,
        owner: Option<&str>,
        parent_id: Option<Option<&str>>,
        limit: Option<i32>,
    ) -> Result<Vec<TaskSummary>> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT id, title, status, priority, owner_agent, points, current_thought
                 FROM tasks WHERE 1=1",
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(s) = status {
                sql.push_str(" AND status = ?");
                params_vec.push(Box::new(s.to_string()));
            }

            if let Some(o) = owner {
                sql.push_str(" AND owner_agent = ?");
                params_vec.push(Box::new(o.to_string()));
            }

            // Handle parent filtering via dependencies table
            if let Some(p) = parent_id {
                match p {
                    Some(pid) => {
                        sql.push_str(" AND id IN (SELECT to_task_id FROM dependencies WHERE from_task_id = ? AND dep_type = 'contains')");
                        params_vec.push(Box::new(pid.to_string()));
                    }
                    None => {
                        // Root tasks: not contained by any other task
                        sql.push_str(" AND id NOT IN (SELECT to_task_id FROM dependencies WHERE dep_type = 'contains')");
                    }
                }
            }

            sql.push_str(" ORDER BY created_at DESC");

            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let tasks = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let status: String = row.get(2)?;
                    let priority: String = row.get(3)?;
                    let owner_agent: Option<String> = row.get(4)?;
                    let points: Option<i32> = row.get(5)?;
                    let current_thought: Option<String> = row.get(6)?;

                    Ok(TaskSummary {
                        id,
                        title,
                        status,
                        priority: parse_priority(&priority),
                        owner_agent,
                        points,
                        current_thought,
                    })
                })?
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
                     WHERE owner_agent = ? AND id IN ({})",
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
                    "UPDATE tasks SET current_thought = ?, updated_at = ? WHERE owner_agent = ?",
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

    /// Log cost and token usage for a task.
    pub fn log_cost(
        &self,
        task_id: &str,
        tokens_in: Option<i64>,
        tokens_cached: Option<i64>,
        tokens_out: Option<i64>,
        tokens_thinking: Option<i64>,
        tokens_image: Option<i64>,
        tokens_audio: Option<i64>,
        cost_usd: Option<f64>,
        user_metrics: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            let new_tokens_in = task.tokens_in + tokens_in.unwrap_or(0);
            let new_tokens_cached = task.tokens_cached + tokens_cached.unwrap_or(0);
            let new_tokens_out = task.tokens_out + tokens_out.unwrap_or(0);
            let new_tokens_thinking = task.tokens_thinking + tokens_thinking.unwrap_or(0);
            let new_tokens_image = task.tokens_image + tokens_image.unwrap_or(0);
            let new_tokens_audio = task.tokens_audio + tokens_audio.unwrap_or(0);
            let new_cost_usd = task.cost_usd + cost_usd.unwrap_or(0.0);

            // Merge user_metrics
            let new_user_metrics = if let Some(new_metrics) = user_metrics {
                let mut merged = task.user_metrics.clone().unwrap_or_default();
                for (k, v) in new_metrics {
                    merged.insert(k, v);
                }
                Some(merged)
            } else {
                task.user_metrics.clone()
            };

            let user_metrics_json = new_user_metrics
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap());

            conn.execute(
                "UPDATE tasks SET
                    tokens_in = ?1, tokens_cached = ?2, tokens_out = ?3,
                    tokens_thinking = ?4, tokens_image = ?5, tokens_audio = ?6,
                    cost_usd = ?7, user_metrics = ?8, updated_at = ?9
                WHERE id = ?10",
                params![
                    new_tokens_in,
                    new_tokens_cached,
                    new_tokens_out,
                    new_tokens_thinking,
                    new_tokens_image,
                    new_tokens_audio,
                    new_cost_usd,
                    user_metrics_json,
                    now,
                    task_id,
                ],
            )?;

            Ok(Task {
                tokens_in: new_tokens_in,
                tokens_cached: new_tokens_cached,
                tokens_out: new_tokens_out,
                tokens_thinking: new_tokens_thinking,
                tokens_image: new_tokens_image,
                tokens_audio: new_tokens_audio,
                cost_usd: new_cost_usd,
                user_metrics: new_user_metrics,
                updated_at: now,
                ..task
            })
        })
    }

    /// Claim a task for an agent.
    /// Uses the first timed state (typically "in_progress") as the claiming state.
    pub fn claim_task(
        &self,
        task_id: &str,
        agent_id: &str,
        states_config: &StatesConfig,
    ) -> Result<Task> {
        let now = now_ms();

        // Find the first timed state to use for claiming (typically "in_progress")
        let claim_status = states_config
            .definitions
            .iter()
            .find(|(_, def)| def.timed)
            .map(|(name, _)| name.as_str())
            .unwrap_or("in_progress");

        self.with_conn(|conn| {
            // Get the task (using internal helper to avoid deadlock)
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Check if already claimed
            if task.owner_agent.is_some() {
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

            // Check claim limit (using internal helper to avoid deadlock)
            let current_claims = get_claim_count_internal(conn, agent_id)?;
            if current_claims >= agent.max_claims {
                return Err(anyhow!("Agent has reached claim limit"));
            }

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
                "UPDATE tasks SET owner_agent = ?1, claimed_at = ?2, status = ?3, started_at = ?4, updated_at = ?5
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
                owner_agent: Some(agent_id.to_string()),
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

            if task.owner_agent.as_deref() != Some(agent_id) {
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
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
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
                task.owner_agent.as_deref(),
                None,
                states_config,
            )?;

            conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
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

        // Find the first timed state to use for claiming (typically "in_progress")
        let claim_status = states_config
            .definitions
            .iter()
            .find(|(_, def)| def.timed)
            .map(|(name, _)| name.as_str())
            .unwrap_or("in_progress");

        self.with_conn(|conn| {
            // Get the task
            let task =
                get_task_internal(conn, task_id)?.ok_or_else(|| anyhow!("Task not found"))?;

            // Get the agent
            let agent =
                get_worker_internal(conn, agent_id)?.ok_or_else(|| anyhow!("Agent not found"))?;

            // Check claim limit
            let current_claims = get_claim_count_internal(conn, agent_id)?;
            if current_claims >= agent.max_claims {
                return Err(anyhow!("Agent has reached claim limit"));
            }

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
                "UPDATE tasks SET owner_agent = ?1, claimed_at = ?2, status = ?3, started_at = COALESCE(started_at, ?4), updated_at = ?5
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
                owner_agent: Some(agent_id.to_string()),
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

            if task.owner_agent.as_deref() != Some(agent_id) {
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

            // Set completed_at for terminal states
            let completed_at = if states_config.is_terminal_state(state) {
                Some(now)
            } else {
                None
            };

            // Record state transition (accumulates time if coming from timed state)
            record_state_transition(conn, task_id, state, Some(agent_id), None, states_config)?;

            conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, completed_at = COALESCE(?2, completed_at), updated_at = ?3
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
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE claimed_at < ?3 AND owner_agent IS NOT NULL",
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
            if task.owner_agent.as_deref() != Some(agent_id) {
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
                 owner_agent = NULL, claimed_at = NULL
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
                owner_agent: None,
                claimed_at: None,
                ..task
            })
        })
    }

    /// Get all tasks.
    pub fn get_all_tasks(&self) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY created_at")?;
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

    /// Get claimed tasks.
    pub fn get_claimed_tasks(&self, agent_id: Option<&str>) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let tasks = if let Some(aid) = agent_id {
                let mut stmt = conn
                    .prepare("SELECT * FROM tasks WHERE owner_agent = ?1 ORDER BY claimed_at")?;
                stmt.query_map(params![aid], parse_task_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE owner_agent IS NOT NULL ORDER BY claimed_at",
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
/// Creates 'contains' dependencies from parent to children.
/// Creates 'follows' dependencies between siblings when parallel=false.
fn create_tree_recursive(
    conn: &Connection,
    input: &TaskTreeInput,
    parent_id: Option<&str>,
    prev_sibling_id: Option<&str>,
    all_ids: &mut Vec<String>,
    states_config: &StatesConfig,
) -> Result<String> {
    // Use custom id if provided, otherwise generate UUID7
    let generated_id = Uuid::now_v7().to_string();
    let task_id = input.id.clone().unwrap_or(generated_id);
    let now = now_ms();
    let priority = input.priority.unwrap_or(PRIORITY_MEDIUM);
    let initial_status = &states_config.initial;

    let needed_tags = input.needed_tags.clone().unwrap_or_default();
    let wanted_tags = input.wanted_tags.clone().unwrap_or_default();
    let tags = input.tags.clone().unwrap_or_default();
    let needed_tags_json = serde_json::to_string(&needed_tags)?;
    let wanted_tags_json = serde_json::to_string(&wanted_tags)?;
    let tags_json = serde_json::to_string(&tags)?;

    conn.execute(
        "INSERT INTO tasks (
            id, title, description, status, priority,
            needed_tags, wanted_tags, tags, points, time_estimate_ms, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            &task_id,
            &input.description,  // Use description as title
            &input.description,  // Also store as description
            initial_status,
            priority_to_str(priority),
            needed_tags_json,
            wanted_tags_json,
            tags_json,
            input.points,
            input.time_estimate_ms,
            now,
            now,
        ],
    )?;

    // Create 'contains' dependency from parent if present
    if let Some(pid) = parent_id {
        Database::add_dependency_internal(conn, pid, &task_id, "contains")?;
    }

    // Create 'follows' dependency from previous sibling if sequential (parallel=false)
    if !input.parallel {
        if let Some(prev_id) = prev_sibling_id {
            Database::add_dependency_internal(conn, prev_id, &task_id, "follows")?;
        }
    }

    // Record initial state transition
    record_state_transition(conn, &task_id, initial_status, None, None, states_config)?;

    all_ids.push(task_id.clone());

    // Create children with 'follows' dependencies if not parallel
    let mut prev_child_id: Option<String> = None;
    for child in input.children.iter() {
        let child_id = create_tree_recursive(
            conn,
            child,
            Some(&task_id),
            if child.parallel {
                None
            } else {
                prev_child_id.as_deref()
            },
            all_ids,
            states_config,
        )?;
        prev_child_id = Some(child_id);
    }

    Ok(task_id)
}
