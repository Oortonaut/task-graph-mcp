//! Worker CRUD operations.

use super::{Database, now_ms};
use crate::config::IdsConfig;
use crate::types::{CleanupSummary, DisconnectSummary, Worker};
use anyhow::{Result, anyhow};
use petname::{Generator, Petnames};
use rusqlite::{Connection, params};

/// Maximum length for worker IDs (4-word petnames can be ~50 chars).
pub const MAX_WORKER_ID_LEN: usize = 64;

/// Generate a petname-based agent ID using the large wordlist with configured case style.
/// With 4 words from a large wordlist, collisions are extremely unlikely.
fn generate_agent_id(ids_config: &IdsConfig) -> String {
    let words = ids_config.agent_id_words;
    let case = ids_config.agent_id_case;

    // Generate with hyphen separator first (petname's default format)
    let base = Petnames::medium()
        .generate_one(words, "-")
        .unwrap_or_else(|| format!("worker-{}", now_ms()));

    // Convert to desired case
    case.convert(&base)
}

/// Parse overlays JSON from DB (nullable TEXT column) into Vec<String>.
fn parse_overlays(overlays_json: &Option<String>) -> Vec<String> {
    overlays_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// Internal helper to get a worker using an existing connection (avoids deadlock).
fn get_worker_internal(conn: &Connection, worker_id: &str) -> Result<Option<Worker>> {
    let mut stmt = conn.prepare(
        "SELECT id, tags, max_claims, registered_at, last_heartbeat, last_status, last_phase, last_task_id, workflow, overlays
         FROM workers WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![worker_id], |row| {
        let id: String = row.get(0)?;
        let tags_json: String = row.get(1)?;
        let max_claims: i32 = row.get(2)?;
        let registered_at: i64 = row.get(3)?;
        let last_heartbeat: i64 = row.get(4)?;
        let last_status: Option<String> = row.get(5)?;
        let last_phase: Option<String> = row.get(6)?;
        let last_task_id: Option<String> = row.get(7)?;
        let workflow: Option<String> = row.get(8)?;
        let overlays_json: Option<String> = row.get(9)?;

        Ok((
            id,
            tags_json,
            max_claims,
            registered_at,
            last_heartbeat,
            last_status,
            last_phase,
            last_task_id,
            workflow,
            overlays_json,
        ))
    });

    match result {
        Ok((
            id,
            tags_json,
            max_claims,
            registered_at,
            last_heartbeat,
            last_status,
            last_phase,
            last_task_id,
            workflow,
            overlays_json,
        )) => {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let overlays = parse_overlays(&overlays_json);
            Ok(Some(Worker {
                id,
                tags,
                max_claims,
                registered_at,
                last_heartbeat,
                last_status,
                last_phase,
                last_task_id,
                workflow,
                overlays,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

impl Database {
    /// Register a new worker.
    ///
    /// If `worker_id` is provided, it must be at most 36 characters.
    /// If not provided, a human-readable petname will be generated (e.g., "happy-turtle").
    /// If `force` is true and the worker already exists, it will be re-registered
    /// (useful for stuck worker recovery).
    /// If `workflow` is provided, the worker will use that named workflow (e.g., "swarm" for workflow-swarm.yaml).
    /// If `overlays` is provided, those overlays will be applied on top of the workflow.
    #[allow(clippy::too_many_arguments)]
    pub fn register_worker(
        &self,
        worker_id: Option<String>,
        tags: Vec<String>,
        force: bool,
        ids_config: &IdsConfig,
        workflow: Option<String>,
        overlays: Vec<String>,
        max_claims: Option<i32>,
    ) -> Result<Worker> {
        // Validate user-provided ID upfront (before acquiring connection)
        let provided_id = match worker_id {
            Some(id) => {
                if id.len() > MAX_WORKER_ID_LEN {
                    return Err(anyhow!(
                        "Worker ID must be at most {} characters, got {}",
                        MAX_WORKER_ID_LEN,
                        id.len()
                    ));
                }
                if id.is_empty() {
                    return Err(anyhow!("Worker ID cannot be empty"));
                }
                Some(id)
            }
            None => None,
        };
        let now = now_ms();
        let max_claims = match max_claims {
            Some(0) => i32::MAX, // 0 means unlimited
            Some(n) => n,
            None => 1, // Default to 1 concurrent claim
        };
        let tags_json = serde_json::to_string(&tags)?;
        let overlays_json = if overlays.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&overlays)?)
        };

        self.with_conn(|conn| {
            // Generate ID (with 4+ words from a large wordlist, collisions are extremely unlikely)
            let id = match provided_id {
                Some(id) => id,
                None => generate_agent_id(ids_config),
            };

            // Check if worker ID already exists
            let exists: bool = conn
                .query_row("SELECT 1 FROM workers WHERE id = ?1", params![&id], |_| Ok(true))
                .unwrap_or(false);

            // Get current max claim sequence + 1 to initialize poll position.
            // This ensures first poll returns empty (no events since registration).
            // The +1 is needed because we now query with `id >= last_seq`.
            let current_max_sequence: i64 = conn
                .query_row("SELECT COALESCE(MAX(id), 0) FROM claim_sequence", [], |row| row.get(0))
                .unwrap_or(0);
            let initial_sequence = current_max_sequence + 1;

            if exists {
                if force {
                    // Force reconnection: update existing worker and reset poll position, including workflow and overlays
                    conn.execute(
                        "UPDATE workers SET tags = ?1, max_claims = ?2, last_heartbeat = ?3, last_claim_sequence = ?4, workflow = ?5, overlays = ?6 WHERE id = ?7",
                        params![tags_json, max_claims, now, initial_sequence, &workflow, &overlays_json, &id],
                    )?;
                } else {
                    return Err(anyhow!("Worker ID '{}' already registered. Use force=true to reconnect.", id));
                }
            } else {
                conn.execute(
                    "INSERT INTO workers (id, tags, max_claims, registered_at, last_heartbeat, last_claim_sequence, workflow, overlays)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![&id, tags_json, max_claims, now, now, initial_sequence, &workflow, &overlays_json],
                )?;
            }

            Ok(Worker {
                id,
                tags,
                max_claims,
                registered_at: now,
                last_heartbeat: now,
                last_status: None,
                last_phase: None,
                last_task_id: None,
                workflow,
                overlays,
            })
        })
    }

    /// Get a worker by ID.
    pub fn get_worker(&self, worker_id: &str) -> Result<Option<Worker>> {
        self.with_conn(|conn| get_worker_internal(conn, worker_id))
    }

    /// Check if a worker exists. Returns error if not found.
    pub fn require_worker(&self, worker_id: &str) -> Result<Worker> {
        self.get_worker(worker_id)?
            .ok_or_else(|| anyhow::anyhow!("Worker {} not found", worker_id))
    }

    /// Update a worker.
    pub fn update_worker(
        &self,
        worker_id: &str,
        tags: Option<Vec<String>>,
        max_claims: Option<i32>,
    ) -> Result<Worker> {
        self.with_conn(|conn| {
            let worker =
                get_worker_internal(conn, worker_id)?.ok_or_else(|| anyhow!("Worker not found"))?;

            let new_tags = tags.unwrap_or(worker.tags.clone());
            let new_max_claims = max_claims.unwrap_or(worker.max_claims);
            let tags_json = serde_json::to_string(&new_tags)?;

            conn.execute(
                "UPDATE workers SET tags = ?1, max_claims = ?2 WHERE id = ?3",
                params![tags_json, new_max_claims, worker_id],
            )?;

            Ok(Worker {
                id: worker_id.to_string(),
                tags: new_tags,
                max_claims: new_max_claims,
                registered_at: worker.registered_at,
                last_heartbeat: worker.last_heartbeat,
                last_status: worker.last_status,
                last_phase: worker.last_phase,
                last_task_id: worker.last_task_id,
                workflow: worker.workflow,
                overlays: worker.overlays,
            })
        })
    }

    /// Update only the overlays for a worker.
    pub fn update_worker_overlays(&self, worker_id: &str, overlays: Vec<String>) -> Result<Worker> {
        let overlays_json = if overlays.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&overlays)?)
        };

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE workers SET overlays = ?1 WHERE id = ?2",
                params![overlays_json, worker_id],
            )?;

            if updated == 0 {
                return Err(anyhow!("Worker not found"));
            }

            get_worker_internal(conn, worker_id)?
                .ok_or_else(|| anyhow!("Worker not found after update"))
        })
    }

    /// Update worker's last seen state (status and phase) for transition prompt tracking.
    /// Returns the previous state (old_status, old_phase) for prompt calculation.
    pub fn update_worker_state(
        &self,
        worker_id: &str,
        new_status: Option<&str>,
        new_phase: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<(Option<String>, Option<String>)> {
        self.with_conn(|conn| {
            // Get current state
            let (old_status, old_phase): (Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT last_status, last_phase FROM workers WHERE id = ?1",
                    params![worker_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => anyhow!("Worker not found"),
                    e => e.into(),
                })?;

            // Update to new state (including last_task_id for per-task tracking)
            conn.execute(
                "UPDATE workers SET last_status = ?1, last_phase = ?2, last_task_id = ?3 WHERE id = ?4",
                params![new_status, new_phase, task_id, worker_id],
            )?;

            Ok((old_status, old_phase))
        })
    }

    /// Update worker heartbeat.
    pub fn heartbeat(
        &self,
        worker_id: &str,
        states_config: &crate::config::StatesConfig,
    ) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, worker_id],
            )?;

            if updated == 0 {
                return Err(anyhow!("Worker not found"));
            }

            // Return current claim count (all timed states)
            get_claim_count_internal(conn, worker_id, states_config)
        })
    }

    /// Unregister a worker (releases all claims).
    /// Returns a summary of released tasks and files.
    pub fn unregister_worker(
        &self,
        worker_id: &str,
        final_status: &str,
    ) -> Result<DisconnectSummary> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Release all task claims, setting them to final_status
            let tasks_released = tx.execute(
                "UPDATE tasks SET worker_id = NULL, claimed_at = NULL, status = ?2
                 WHERE worker_id = ?1",
                params![worker_id, final_status],
            )? as i32;

            // Remove all file locks
            let files_released = tx.execute(
                "DELETE FROM file_locks WHERE worker_id = ?1",
                params![worker_id],
            )? as i32;

            // Remove worker
            tx.execute("DELETE FROM workers WHERE id = ?1", params![worker_id])?;

            tx.commit()?;
            Ok(DisconnectSummary {
                tasks_released,
                files_released,
                final_status: final_status.to_string(),
            })
        })
    }

    /// List all workers.
    pub fn list_workers(&self) -> Result<Vec<Worker>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tags, max_claims, registered_at, last_heartbeat, last_status, last_phase, last_task_id, workflow, overlays
                 FROM workers ORDER BY registered_at DESC",
            )?;

            let workers = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let tags_json: String = row.get(1)?;
                    let max_claims: i32 = row.get(2)?;
                    let registered_at: i64 = row.get(3)?;
                    let last_heartbeat: i64 = row.get(4)?;
                    let last_status: Option<String> = row.get(5)?;
                    let last_phase: Option<String> = row.get(6)?;
                    let last_task_id: Option<String> = row.get(7)?;
                    let workflow: Option<String> = row.get(8)?;
                    let overlays_json: Option<String> = row.get(9)?;

                    Ok((
                        id,
                        tags_json,
                        max_claims,
                        registered_at,
                        last_heartbeat,
                        last_status,
                        last_phase,
                        last_task_id,
                        workflow,
                        overlays_json,
                    ))
                })?
                .filter_map(|r| r.ok())
                .map(
                    |(
                        id,
                        tags_json,
                        max_claims,
                        registered_at,
                        last_heartbeat,
                        last_status,
                        last_phase,
                        last_task_id,
                        workflow,
                        overlays_json,
                    )| {
                        let tags: Vec<String> =
                            serde_json::from_str(&tags_json).unwrap_or_default();
                        let overlays = parse_overlays(&overlays_json);
                        Worker {
                            id,
                            tags,
                            max_claims,
                            registered_at,
                            last_heartbeat,
                            last_status,
                            last_phase,
                            last_task_id,
                            workflow,
                            overlays,
                        }
                    },
                )
                .collect();

            Ok(workers)
        })
    }

    /// List all workers with extended info (claim count, current thought).
    pub fn list_workers_info(
        &self,
        states_config: &crate::config::StatesConfig,
    ) -> Result<Vec<crate::types::WorkerInfo>> {
        self.with_conn(|conn| {
            let timed_states = states_config.timed_state_names();
            let (status_in, status_in_thought) = if timed_states.is_empty() {
                ("status = '__none__'".to_string(), "status = '__none__'".to_string())
            } else {
                let quoted: Vec<String> = timed_states.iter().map(|s| format!("'{}'", s)).collect();
                let clause = format!("status IN ({})", quoted.join(", "));
                (clause.clone(), clause)
            };

            let sql = format!(
                "SELECT w.id, w.tags, w.max_claims, w.registered_at, w.last_heartbeat,
                        (SELECT COUNT(*) FROM tasks WHERE worker_id = w.id AND {}) as claim_count,
                        (SELECT current_thought FROM tasks WHERE worker_id = w.id AND {} AND current_thought IS NOT NULL LIMIT 1) as current_thought,
                        w.last_status, w.last_phase, w.last_task_id, w.workflow, w.overlays
                 FROM workers w ORDER BY w.registered_at DESC",
                status_in, status_in_thought
            );

            let mut stmt = conn.prepare(&sql)?;

            let workers = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let tags_json: String = row.get(1)?;
                let max_claims: i32 = row.get(2)?;
                let registered_at: i64 = row.get(3)?;
                let last_heartbeat: i64 = row.get(4)?;
                let claim_count: i32 = row.get(5)?;
                let current_thought: Option<String> = row.get(6)?;
                let last_status: Option<String> = row.get(7)?;
                let last_phase: Option<String> = row.get(8)?;
                let last_task_id: Option<String> = row.get(9)?;
                let workflow: Option<String> = row.get(10)?;
                let overlays_json: Option<String> = row.get(11)?;

                Ok((id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought, last_status, last_phase, last_task_id, workflow, overlays_json))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought, last_status, last_phase, last_task_id, workflow, overlays_json)| {
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                let overlays = parse_overlays(&overlays_json);
                crate::types::WorkerInfo {
                    id,
                    tags,
                    max_claims,
                    claim_count,
                    current_thought,
                    registered_at,
                    last_heartbeat,
                    last_status,
                    last_phase,
                    last_task_id,
                    workflow,
                    overlays,
                }
            })
            .collect();

            Ok(workers)
        })
    }

    /// List workers with optional filters by tags, file claimed, or related task.
    ///
    /// - `tags`: Workers must have ALL of these tags
    /// - `file`: Workers that have claimed this file
    /// - `task_id`: Workers working on tasks related to this task
    /// - `depth`: Task relationship depth (-3 to 3). Negative: ancestors, positive: descendants
    pub fn list_workers_filtered(
        &self,
        tags: Option<&Vec<String>>,
        file: Option<&str>,
        task_id: Option<&str>,
        depth: i32,
        states_config: &crate::config::StatesConfig,
    ) -> Result<Vec<crate::types::WorkerInfo>> {
        self.with_conn(|conn| {
            let timed_states = states_config.timed_state_names();
            let status_clause = if timed_states.is_empty() {
                "status = '__none__'".to_string()
            } else {
                let quoted: Vec<String> = timed_states.iter().map(|s| format!("'{}'", s)).collect();
                format!("status IN ({})", quoted.join(", "))
            };

            // Start with base query
            let mut sql = format!(
                "SELECT DISTINCT w.id, w.tags, w.max_claims, w.registered_at, w.last_heartbeat,
                        (SELECT COUNT(*) FROM tasks WHERE worker_id = w.id AND {}) as claim_count,
                        (SELECT current_thought FROM tasks WHERE worker_id = w.id AND {} AND current_thought IS NOT NULL LIMIT 1) as current_thought,
                        w.last_status, w.last_phase, w.last_task_id, w.workflow, w.overlays
                 FROM workers w WHERE 1=1",
                status_clause, status_clause
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            // Filter by file claim
            if let Some(f) = file {
                sql.push_str(" AND w.id IN (SELECT worker_id FROM file_locks WHERE file_path = ?)");
                params_vec.push(Box::new(f.to_string()));
            }

            // Filter by related task (with depth traversal)
            if let Some(tid) = task_id {
                // Get all related task IDs at the given depth
                let related_task_ids = Self::get_related_task_ids_internal(conn, tid, depth)?;
                if !related_task_ids.is_empty() {
                    let placeholders: Vec<String> = related_task_ids.iter().map(|_| "?".to_string()).collect();
                    sql.push_str(&format!(
                        " AND w.id IN (SELECT DISTINCT worker_id FROM tasks WHERE id IN ({}) AND worker_id IS NOT NULL)",
                        placeholders.join(", ")
                    ));
                    for task in related_task_ids {
                        params_vec.push(Box::new(task));
                    }
                } else {
                    // No related tasks found, return empty result
                    return Ok(Vec::new());
                }
            }

            sql.push_str(" ORDER BY w.registered_at DESC");

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let workers: Vec<crate::types::WorkerInfo> = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    let tags_json: String = row.get(1)?;
                    let max_claims: i32 = row.get(2)?;
                    let registered_at: i64 = row.get(3)?;
                    let last_heartbeat: i64 = row.get(4)?;
                    let claim_count: i32 = row.get(5)?;
                    let current_thought: Option<String> = row.get(6)?;
                    let last_status: Option<String> = row.get(7)?;
                    let last_phase: Option<String> = row.get(8)?;
                    let last_task_id: Option<String> = row.get(9)?;
                    let workflow: Option<String> = row.get(10)?;
                    let overlays_json: Option<String> = row.get(11)?;

                    Ok((id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought, last_status, last_phase, last_task_id, workflow, overlays_json))
                })?
                .filter_map(|r| r.ok())
                .map(|(id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought, last_status, last_phase, last_task_id, workflow, overlays_json)| {
                    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                    let overlays = parse_overlays(&overlays_json);
                    crate::types::WorkerInfo {
                        id,
                        tags,
                        max_claims,
                        claim_count,
                        current_thought,
                        registered_at,
                        last_heartbeat,
                        last_status,
                        last_phase,
                        last_task_id,
                        workflow,
                        overlays,
                    }
                })
                .collect();

            // Post-filter by tags (need to check ALL tags match)
            let workers = if let Some(required_tags) = tags {
                workers
                    .into_iter()
                    .filter(|w| required_tags.iter().all(|t| w.tags.contains(t)))
                    .collect()
            } else {
                workers
            };

            Ok(workers)
        })
    }

    /// Internal helper to get related task IDs at a given depth.
    /// Negative depth: ancestors (parents/blockers), positive depth: descendants (children/blocked).
    fn get_related_task_ids_internal(
        conn: &Connection,
        task_id: &str,
        depth: i32,
    ) -> Result<Vec<String>> {
        use std::collections::HashSet;

        let mut result = HashSet::new();
        result.insert(task_id.to_string());

        if depth == 0 {
            return Ok(result.into_iter().collect());
        }

        let abs_depth = depth.abs();
        let mut current_level: HashSet<String> = [task_id.to_string()].into_iter().collect();

        for _ in 0..abs_depth {
            if current_level.is_empty() {
                break;
            }

            let mut next_level = HashSet::new();

            for tid in &current_level {
                let related: Vec<String> = if depth > 0 {
                    // Descendants: tasks where this task is the from_task_id (children, blocked tasks)
                    let mut stmt = conn
                        .prepare("SELECT to_task_id FROM dependencies WHERE from_task_id = ?1")?;
                    stmt.query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect()
                } else {
                    // Ancestors: tasks where this task is the to_task_id (parents, blockers)
                    let mut stmt = conn
                        .prepare("SELECT from_task_id FROM dependencies WHERE to_task_id = ?1")?;
                    stmt.query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect()
                };

                for related_id in related {
                    if !result.contains(&related_id) {
                        next_level.insert(related_id.clone());
                        result.insert(related_id);
                    }
                }
            }

            current_level = next_level;
        }

        Ok(result.into_iter().collect())
    }

    /// Get workers with stale heartbeats.
    pub fn get_stale_workers(&self, timeout_seconds: i64) -> Result<Vec<Worker>> {
        let cutoff = now_ms() - (timeout_seconds * 1000);

        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tags, max_claims, registered_at, last_heartbeat, last_status, last_phase, last_task_id, workflow, overlays
                 FROM workers WHERE last_heartbeat < ?1",
            )?;

            let workers = stmt
                .query_map(params![cutoff], |row| {
                    let id: String = row.get(0)?;
                    let tags_json: String = row.get(1)?;
                    let max_claims: i32 = row.get(2)?;
                    let registered_at: i64 = row.get(3)?;
                    let last_heartbeat: i64 = row.get(4)?;
                    let last_status: Option<String> = row.get(5)?;
                    let last_phase: Option<String> = row.get(6)?;
                    let last_task_id: Option<String> = row.get(7)?;
                    let workflow: Option<String> = row.get(8)?;
                    let overlays_json: Option<String> = row.get(9)?;

                    Ok((
                        id,
                        tags_json,
                        max_claims,
                        registered_at,
                        last_heartbeat,
                        last_status,
                        last_phase,
                        last_task_id,
                        workflow,
                        overlays_json,
                    ))
                })?
                .filter_map(|r| r.ok())
                .map(
                    |(
                        id,
                        tags_json,
                        max_claims,
                        registered_at,
                        last_heartbeat,
                        last_status,
                        last_phase,
                        last_task_id,
                        workflow,
                        overlays_json,
                    )| {
                        let tags: Vec<String> =
                            serde_json::from_str(&tags_json).unwrap_or_default();
                        let overlays = parse_overlays(&overlays_json);
                        Worker {
                            id,
                            tags,
                            max_claims,
                            registered_at,
                            last_heartbeat,
                            last_status,
                            last_phase,
                            last_task_id,
                            workflow,
                            overlays,
                        }
                    },
                )
                .collect();

            Ok(workers)
        })
    }

    /// Cleanup stale workers by evicting them and releasing their claims.
    ///
    /// For each stale worker, individual `task_sequence` entries are inserted
    /// for every released task before calling `unregister_worker()`. This allows
    /// polling agents to discover released tasks gradually via the sequence table
    /// rather than all tasks becoming available simultaneously (which would cause
    /// scheduling storms when an agent holding many tasks times out).
    ///
    /// Returns a summary of the cleanup operation.
    pub fn cleanup_stale_workers(
        &self,
        timeout_seconds: i64,
        final_status: &str,
    ) -> Result<CleanupSummary> {
        let stale_workers = self.get_stale_workers(timeout_seconds)?;

        let mut total_tasks_released = 0;
        let mut total_files_released = 0;
        let mut evicted_worker_ids = Vec::new();

        for worker in &stale_workers {
            // Record individual task_sequence entries BEFORE bulk-releasing
            let released_task_ids =
                self.record_stale_release_transitions(&worker.id, final_status)?;

            if released_task_ids.len() > 5 {
                eprintln!(
                    "[cleanup] Bulk-releasing {} task claims from stale agent '{}' (last heartbeat: {})",
                    released_task_ids.len(),
                    worker.id,
                    worker.last_heartbeat,
                );
            }

            // Release file locks first
            let _ = self.release_worker_locks(&worker.id);

            // Unregister the worker (releases task claims and removes worker)
            if let Ok(summary) = self.unregister_worker(&worker.id, final_status) {
                total_tasks_released += summary.tasks_released;
                total_files_released += summary.files_released;
                evicted_worker_ids.push(worker.id.clone());
            }
        }

        Ok(CleanupSummary {
            workers_evicted: evicted_worker_ids.len() as i32,
            tasks_released: total_tasks_released,
            files_released: total_files_released,
            final_status: final_status.to_string(),
            evicted_worker_ids,
        })
    }

    /// Record individual task_sequence entries for each task claimed by a stale
    /// worker before it is unregistered. Provides gradual discovery and audit trail.
    fn record_stale_release_transitions(
        &self,
        worker_id: &str,
        final_status: &str,
    ) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let now = now_ms();

            let mut stmt = conn.prepare("SELECT id FROM tasks WHERE worker_id = ?1")?;
            let released_task_ids: Vec<String> = stmt
                .query_map(params![worker_id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            for task_id in &released_task_ids {
                // Close any open status transition
                conn.execute(
                    "UPDATE task_sequence SET end_timestamp = ?1
                     WHERE task_id = ?2 AND end_timestamp IS NULL AND status IS NOT NULL",
                    params![now, task_id],
                )?;

                // Insert stale_release transition
                conn.execute(
                    "INSERT INTO task_sequence (task_id, worker_id, status, reason, timestamp)
                     VALUES (?1, ?2, ?3, 'stale_release', ?4)",
                    params![task_id, worker_id, final_status, now],
                )?;
            }

            Ok(released_task_ids)
        })
    }

    /// Force-expire a specific worker, releasing all its claimed tasks and file locks,
    /// then unregistering it. Unlike cleanup_stale_workers, this does not check
    /// heartbeat staleness -- it unconditionally expires the worker.
    pub fn expire_worker(&self, worker_id: &str, final_status: &str) -> Result<DisconnectSummary> {
        let files_from_locks = self.release_worker_locks(worker_id).unwrap_or(0);
        let mut summary = self.unregister_worker(worker_id, final_status)?;
        summary.files_released += files_from_locks;
        Ok(summary)
    }

    /// Get claim count for a worker (counts tasks in any timed state).
    pub fn get_claim_count(
        &self,
        worker_id: &str,
        states_config: &crate::config::StatesConfig,
    ) -> Result<i32> {
        self.with_conn(|conn| get_claim_count_internal(conn, worker_id, states_config))
    }
}

/// Internal helper to get claim count using an existing connection (avoids deadlock in transactions).
/// Counts tasks in any timed state, not just 'working'.
pub(crate) fn get_claim_count_internal(
    conn: &Connection,
    worker_id: &str,
    states_config: &crate::config::StatesConfig,
) -> Result<i32> {
    let timed_states = states_config.timed_state_names();
    if timed_states.is_empty() {
        return Ok(0);
    }
    let placeholders: Vec<String> = (0..timed_states.len())
        .map(|i| format!("?{}", i + 2))
        .collect();
    let sql = format!(
        "SELECT COUNT(*) FROM tasks WHERE worker_id = ?1 AND status IN ({})",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    param_values.push(Box::new(worker_id.to_string()));
    for state in &timed_states {
        param_values.push(Box::new(state.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
    let count: i32 = stmt.query_row(param_refs.as_slice(), |row| row.get(0))?;
    Ok(count)
}
