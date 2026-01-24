//! Task CRUD and tree operations.

use super::{now_ms, Database};
use crate::types::{JoinMode, Priority, Task, TaskStatus, TaskSummary, TaskTree, TaskTreeInput};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;
use uuid::Uuid; // Still needed for generating new IDs

pub fn parse_task_row(row: &Row) -> rusqlite::Result<Task> {
    let id: String = row.get("id")?;
    let parent_id: Option<String> = row.get("parent_id")?;
    let title: String = row.get("title")?;
    let description: Option<String> = row.get("description")?;
    let status: String = row.get("status")?;
    let priority: String = row.get("priority")?;
    let join_mode: String = row.get("join_mode")?;
    let sibling_order: i32 = row.get("sibling_order")?;
    let owner_agent: Option<String> = row.get("owner_agent")?;
    let claimed_at: Option<i64> = row.get("claimed_at")?;

    let needed_tags_json: Option<String> = row.get("needed_tags")?;
    let wanted_tags_json: Option<String> = row.get("wanted_tags")?;

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
        parent_id,
        title,
        description,
        status: TaskStatus::from_str(&status).unwrap_or(TaskStatus::Pending),
        priority: Priority::from_str(&priority).unwrap_or(Priority::Medium),
        join_mode: JoinMode::from_str(&join_mode).unwrap_or(JoinMode::Then),
        sibling_order,
        owner_agent,
        claimed_at,
        needed_tags: needed_tags_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default(),
        wanted_tags: wanted_tags_json
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
        user_metrics: user_metrics_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default()),
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

use crate::types::Agent;

/// Internal helper to get an agent using an existing connection (avoids deadlock).
fn get_agent_internal(conn: &Connection, agent_id: &str) -> Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, tags, max_claims, registered_at, last_heartbeat
         FROM agents WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![agent_id], |row| {
        let id: String = row.get(0)?;
        let name: Option<String> = row.get(1)?;
        let tags_json: String = row.get(2)?;
        let max_claims: i32 = row.get(3)?;
        let registered_at: i64 = row.get(4)?;
        let last_heartbeat: i64 = row.get(5)?;

        Ok((id, name, tags_json, max_claims, registered_at, last_heartbeat))
    });

    match result {
        Ok((id, name, tags_json, max_claims, registered_at, last_heartbeat)) => {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(Some(Agent {
                id,
                name,
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
    pub fn create_task(
        &self,
        title: String,
        description: Option<String>,
        parent_id: Option<String>,
        priority: Option<Priority>,
        points: Option<i32>,
        time_estimate_ms: Option<i64>,
        needed_tags: Option<Vec<String>>,
        wanted_tags: Option<Vec<String>>,
        blocked_by: Option<Vec<String>>,
    ) -> Result<Task> {
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let priority = priority.unwrap_or(Priority::Medium);

        // Calculate sibling order
        let sibling_order = self.get_next_sibling_order(parent_id.as_deref())?;

        let needed_tags = needed_tags.unwrap_or_default();
        let wanted_tags = wanted_tags.unwrap_or_default();
        let needed_tags_json = serde_json::to_string(&needed_tags)?;
        let wanted_tags_json = serde_json::to_string(&wanted_tags)?;

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            tx.execute(
                "INSERT INTO tasks (
                    id, parent_id, title, description, status, priority, join_mode, sibling_order,
                    needed_tags, wanted_tags, points, time_estimate_ms, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    &id,
                    &parent_id,
                    title,
                    description,
                    TaskStatus::Pending.as_str(),
                    priority.as_str(),
                    JoinMode::Then.as_str(),
                    sibling_order,
                    needed_tags_json,
                    wanted_tags_json,
                    points,
                    time_estimate_ms,
                    now,
                    now,
                ],
            )?;

            // Add dependencies if specified
            if let Some(blockers) = &blocked_by {
                for blocker_id in blockers {
                    tx.execute(
                        "INSERT OR IGNORE INTO dependencies (from_task_id, to_task_id) VALUES (?1, ?2)",
                        params![blocker_id, &id],
                    )?;
                }
            }

            tx.commit()?;

            Ok(Task {
                id,
                parent_id,
                title,
                description,
                status: TaskStatus::Pending,
                priority,
                join_mode: JoinMode::Then,
                sibling_order,
                owner_agent: None,
                claimed_at: None,
                needed_tags,
                wanted_tags,
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

    /// Get the next sibling order for a given parent.
    fn get_next_sibling_order(&self, parent_id: Option<&str>) -> Result<i32> {
        self.with_conn(|conn| {
            let max_order: Option<i32> = if let Some(pid) = parent_id {
                conn.query_row(
                    "SELECT MAX(sibling_order) FROM tasks WHERE parent_id = ?1",
                    params![pid],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT MAX(sibling_order) FROM tasks WHERE parent_id IS NULL",
                    [],
                    |row| row.get(0),
                )?
            };

            Ok(max_order.unwrap_or(-1) + 1)
        })
    }

    /// Create a task tree from nested input.
    pub fn create_task_tree(
        &self,
        input: TaskTreeInput,
        parent_id: Option<String>,
    ) -> Result<(String, Vec<String>)> {
        let mut all_ids = Vec::new();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            let root_id = create_tree_recursive(&tx, &input, parent_id.as_deref(), 0, &mut all_ids)?;
            tx.commit()?;
            Ok((root_id, all_ids))
        })
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM tasks WHERE id = ?1",
            )?;

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

    /// Get direct children of a task.
    pub fn get_children(&self, parent_id: &str) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM tasks WHERE parent_id = ?1 ORDER BY sibling_order",
            )?;

            let tasks = stmt.query_map(params![parent_id], parse_task_row)?
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
        status: Option<TaskStatus>,
        priority: Option<Priority>,
        points: Option<Option<i32>>,
    ) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            let new_title = title.unwrap_or(task.title.clone());
            let new_description = description.unwrap_or(task.description.clone());
            let new_status = status.unwrap_or(task.status);
            let new_priority = priority.unwrap_or(task.priority);
            let new_points = points.unwrap_or(task.points);

            // Handle status transitions
            let (started_at, completed_at) = match (task.status, new_status) {
                (TaskStatus::Pending, TaskStatus::InProgress) => (Some(now), task.completed_at),
                (_, TaskStatus::Completed) | (_, TaskStatus::Failed) | (_, TaskStatus::Cancelled) => {
                    (task.started_at, Some(now))
                }
                _ => (task.started_at, task.completed_at),
            };

            conn.execute(
                "UPDATE tasks SET
                    title = ?1, description = ?2, status = ?3, priority = ?4,
                    points = ?5, started_at = ?6, completed_at = ?7, updated_at = ?8
                WHERE id = ?9",
                params![
                    new_title,
                    new_description,
                    new_status.as_str(),
                    new_priority.as_str(),
                    new_points,
                    started_at,
                    completed_at,
                    now,
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
                started_at,
                completed_at,
                updated_at: now,
                ..task
            })
        })
    }

    /// Delete a task.
    pub fn delete_task(&self, task_id: &str, cascade: bool) -> Result<()> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            if cascade {
                // Delete all descendants (CASCADE in foreign key handles this)
                tx.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
            } else {
                // Check for children
                let child_count: i32 = tx.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE parent_id = ?1",
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
        status: Option<TaskStatus>,
        owner: Option<&str>,
        parent_id: Option<Option<&str>>,
        limit: Option<i32>,
    ) -> Result<Vec<TaskSummary>> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT id, parent_id, title, status, priority, owner_agent, points, current_thought
                 FROM tasks WHERE 1=1"
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(s) = status {
                sql.push_str(" AND status = ?");
                params_vec.push(Box::new(s.as_str().to_string()));
            }

            if let Some(o) = owner {
                sql.push_str(" AND owner_agent = ?");
                params_vec.push(Box::new(o.to_string()));
            }

            if let Some(p) = parent_id {
                match p {
                    Some(pid) => {
                        sql.push_str(" AND parent_id = ?");
                        params_vec.push(Box::new(pid.to_string()));
                    }
                    None => {
                        sql.push_str(" AND parent_id IS NULL");
                    }
                }
            }

            sql.push_str(" ORDER BY created_at DESC");

            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }

            let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let tasks = stmt.query_map(params_refs.as_slice(), |row| {
                let id: String = row.get(0)?;
                let parent_id: Option<String> = row.get(1)?;
                let title: String = row.get(2)?;
                let status: String = row.get(3)?;
                let priority: String = row.get(4)?;
                let owner_agent: Option<String> = row.get(5)?;
                let points: Option<i32> = row.get(6)?;
                let current_thought: Option<String> = row.get(7)?;

                Ok(TaskSummary {
                    id,
                    parent_id,
                    title,
                    status: TaskStatus::from_str(&status).unwrap_or(TaskStatus::Pending),
                    priority: Priority::from_str(&priority).unwrap_or(Priority::Medium),
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

                let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
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
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

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

            let user_metrics_json = new_user_metrics.as_ref().map(|m| serde_json::to_string(m).unwrap());

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
    pub fn claim_task(&self, task_id: &str, agent_id: &str) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            // Get the task (using internal helper to avoid deadlock)
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            // Check if already claimed
            if task.owner_agent.is_some() {
                return Err(anyhow!("Task is already claimed"));
            }

            // Get the agent (using internal helper to avoid deadlock)
            let agent = get_agent_internal(conn, agent_id)?
                .ok_or_else(|| anyhow!("Agent not found"))?;

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
                let has_any = task.wanted_tags.iter().any(|wanted| agent.tags.contains(wanted));
                if !has_any {
                    return Err(anyhow!("Agent has none of the wanted tags"));
                }
            }

            conn.execute(
                "UPDATE tasks SET owner_agent = ?1, claimed_at = ?2, status = ?3, started_at = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![
                    agent_id,
                    now,
                    TaskStatus::InProgress.as_str(),
                    now,
                    now,
                    task_id,
                ],
            )?;

            // Refresh agent heartbeat
            conn.execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            Ok(Task {
                owner_agent: Some(agent_id.to_string()),
                claimed_at: Some(now),
                status: TaskStatus::InProgress,
                started_at: Some(now),
                updated_at: now,
                ..task
            })
        })
    }

    /// Release a task claim.
    pub fn release_task(&self, task_id: &str, agent_id: &str) -> Result<()> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            if task.owner_agent.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![TaskStatus::Pending.as_str(), now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force release a task regardless of owner.
    pub fn force_release(&self, task_id: &str) -> Result<()> {
        let now = now_ms();

        self.with_conn(|conn| {
            conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![TaskStatus::Pending.as_str(), now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force claim a task even if owned by another agent.
    pub fn force_claim_task(&self, task_id: &str, agent_id: &str) -> Result<Task> {
        let now = now_ms();

        self.with_conn(|conn| {
            // Get the task
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            // Get the agent
            let agent = get_agent_internal(conn, agent_id)?
                .ok_or_else(|| anyhow!("Agent not found"))?;

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
                let has_any = task.wanted_tags.iter().any(|wanted| agent.tags.contains(wanted));
                if !has_any {
                    return Err(anyhow!("Agent has none of the wanted tags"));
                }
            }

            conn.execute(
                "UPDATE tasks SET owner_agent = ?1, claimed_at = ?2, status = ?3, started_at = COALESCE(started_at, ?4), updated_at = ?5
                 WHERE id = ?6",
                params![
                    agent_id,
                    now,
                    TaskStatus::InProgress.as_str(),
                    now,
                    now,
                    task_id,
                ],
            )?;

            // Refresh agent heartbeat
            conn.execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            Ok(Task {
                owner_agent: Some(agent_id.to_string()),
                claimed_at: Some(now),
                status: TaskStatus::InProgress,
                started_at: task.started_at.or(Some(now)),
                updated_at: now,
                ..task
            })
        })
    }

    /// Release a task claim with a specified state.
    pub fn release_task_with_state(&self, task_id: &str, agent_id: &str, state: TaskStatus) -> Result<()> {
        let now = now_ms();

        self.with_conn(|conn| {
            let task = get_task_internal(conn, task_id)?
                .ok_or_else(|| anyhow!("Task not found"))?;

            if task.owner_agent.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            // Set completed_at only for terminal states
            let completed_at = match state {
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => Some(now),
                _ => None,
            };

            conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, completed_at = COALESCE(?2, completed_at), updated_at = ?3
                 WHERE id = ?4",
                params![state.as_str(), completed_at, now, task_id],
            )?;

            Ok(())
        })
    }

    /// Force release stale claims.
    pub fn force_release_stale(&self, timeout_seconds: i64) -> Result<i32> {
        let now = now_ms();
        let cutoff = now - (timeout_seconds * 1000);

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL, status = ?1, updated_at = ?2
                 WHERE claimed_at < ?3 AND owner_agent IS NOT NULL",
                params![TaskStatus::Pending.as_str(), now, cutoff],
            )?;

            Ok(updated as i32)
        })
    }


    /// Complete a task and release file locks held by the agent.
    pub fn complete_task(&self, task_id: &str, agent_id: &str) -> Result<Task> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get the task
            let mut stmt = tx.prepare("SELECT * FROM tasks WHERE id = ?1")?;
            let task = stmt.query_row(params![task_id], parse_task_row)
                .map_err(|_| anyhow!("Task not found"))?;
            drop(stmt);

            // Verify ownership
            if task.owner_agent.as_deref() != Some(agent_id) {
                return Err(anyhow!("Task is not owned by this agent"));
            }

            // Update task to completed
            tx.execute(
                "UPDATE tasks SET status = ?1, completed_at = ?2, updated_at = ?3,
                 owner_agent = NULL, claimed_at = NULL
                 WHERE id = ?4",
                params![
                    TaskStatus::Completed.as_str(),
                    now,
                    now,
                    task_id,
                ],
            )?;

            // Release all file locks held by this agent
            tx.execute(
                "DELETE FROM file_locks WHERE agent_id = ?1",
                params![agent_id],
            )?;

            // Refresh agent heartbeat
            tx.execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            tx.commit()?;

            Ok(Task {
                status: TaskStatus::Completed,
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
            let tasks = stmt.query_map([], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        })
    }

    /// Get tasks by status.
    #[allow(dead_code)]
    pub fn get_tasks_by_status(&self, status: TaskStatus) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE status = ?1 ORDER BY created_at")?;
            let tasks = stmt.query_map(params![status.as_str()], parse_task_row)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        })
    }

    /// Get claimed tasks.
    pub fn get_claimed_tasks(&self, agent_id: Option<&str>) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let tasks = if let Some(aid) = agent_id {
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE owner_agent = ?1 ORDER BY claimed_at"
                )?;
                stmt.query_map(params![aid], parse_task_row)?
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE owner_agent IS NOT NULL ORDER BY claimed_at"
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
fn create_tree_recursive(
    conn: &Connection,
    input: &TaskTreeInput,
    parent_id: Option<&str>,
    sibling_order: i32,
    all_ids: &mut Vec<String>,
) -> Result<String> {
    let id = Uuid::now_v7().to_string();
    let now = now_ms();
    let priority = input.priority.unwrap_or(Priority::Medium);
    let join_mode = input.join_mode.unwrap_or(JoinMode::Then);

    let needed_tags = input.needed_tags.clone().unwrap_or_default();
    let wanted_tags = input.wanted_tags.clone().unwrap_or_default();
    let needed_tags_json = serde_json::to_string(&needed_tags)?;
    let wanted_tags_json = serde_json::to_string(&wanted_tags)?;

    conn.execute(
        "INSERT INTO tasks (
            id, parent_id, title, description, status, priority, join_mode, sibling_order,
            needed_tags, wanted_tags, points, time_estimate_ms, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            &id,
            parent_id,
            input.title,
            input.description,
            TaskStatus::Pending.as_str(),
            priority.as_str(),
            join_mode.as_str(),
            sibling_order,
            needed_tags_json,
            wanted_tags_json,
            input.points,
            input.time_estimate_ms,
            now,
            now,
        ],
    )?;

    all_ids.push(id.clone());

    // Create children
    for (i, child) in input.children.iter().enumerate() {
        create_tree_recursive(conn, child, Some(&id), i as i32, all_ids)?;
    }

    Ok(id)
}
