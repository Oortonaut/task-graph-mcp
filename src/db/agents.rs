//! Worker CRUD operations.

use super::{now_ms, Database};
use crate::types::{CleanupSummary, DisconnectSummary, Worker};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};

/// Maximum length for worker IDs.
pub const MAX_WORKER_ID_LEN: usize = 36;

/// Maximum attempts to generate a unique petname before falling back.
const MAX_PETNAME_ATTEMPTS: u32 = 100;

/// Generate a unique petname-based worker ID.
/// Tries base petname first, then appends numbers (e.g., "happy-turtle-2").
fn generate_unique_petname(conn: &Connection) -> String {
    let base = petname::petname(2, "-").unwrap_or_else(|| "worker".to_string());
    
    // Check if base name is available
    let exists: bool = conn
        .query_row("SELECT 1 FROM workers WHERE id = ?1", params![&base], |_| Ok(true))
        .unwrap_or(false);
    
    if !exists {
        return base;
    }
    
    // Try appending numbers: happy-turtle-2, happy-turtle-3, etc.
    for i in 2..=MAX_PETNAME_ATTEMPTS {
        let candidate = format!("{}-{}", base, i);
        let exists: bool = conn
            .query_row("SELECT 1 FROM workers WHERE id = ?1", params![&candidate], |_| Ok(true))
            .unwrap_or(false);
        if !exists {
            return candidate;
        }
    }
    
    // Fallback: generate a completely new petname with 3 words for uniqueness
    petname::petname(3, "-").unwrap_or_else(|| format!("worker-{}", now_ms()))
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

impl Database {
    /// Register a new worker.
    ///
    /// If `worker_id` is provided, it must be at most 36 characters.
    /// If not provided, a human-readable petname will be generated (e.g., "happy-turtle").
    /// If `force` is true and the worker already exists, it will be re-registered
    /// (useful for stuck worker recovery).
    pub fn register_worker(
        &self,
        worker_id: Option<String>,
        tags: Vec<String>,
        force: bool,
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
        let max_claims = 5; // Default, TODO: make configurable
        let tags_json = serde_json::to_string(&tags)?;

        self.with_conn(|conn| {
            // Generate ID inside connection to avoid race conditions
            let id = match provided_id {
                Some(id) => id,
                None => generate_unique_petname(conn),
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
                    // Force reconnection: update existing worker and reset poll position
                    conn.execute(
                        "UPDATE workers SET tags = ?1, max_claims = ?2, last_heartbeat = ?3, last_claim_sequence = ?4 WHERE id = ?5",
                        params![tags_json, max_claims, now, initial_sequence, &id],
                    )?;
                } else {
                    return Err(anyhow!("Worker ID '{}' already registered. Use force=true to reconnect.", id));
                }
            } else {
                conn.execute(
                    "INSERT INTO workers (id, tags, max_claims, registered_at, last_heartbeat, last_claim_sequence)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![&id, tags_json, max_claims, now, now, initial_sequence],
                )?;
            }

            Ok(Worker {
                id,
                tags,
                max_claims,
                registered_at: now,
                last_heartbeat: now,
            })
        })
    }

    /// Get a worker by ID.
    pub fn get_worker(&self, worker_id: &str) -> Result<Option<Worker>> {
        self.with_conn(|conn| {
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
        })
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
            let worker = get_worker_internal(conn, worker_id)?
                .ok_or_else(|| anyhow!("Worker not found"))?;

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
            })
        })
    }

    /// Update worker heartbeat.
    pub fn heartbeat(&self, worker_id: &str) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE workers SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, worker_id],
            )?;

            if updated == 0 {
                return Err(anyhow!("Worker not found"));
            }

            // Return current claim count
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE worker_id = ?1 AND status = 'in_progress'",
                params![worker_id],
                |row| row.get(0),
            )?;

            Ok(count)
        })
    }

    /// Unregister a worker (releases all claims).
    /// Returns a summary of released tasks and files.
    pub fn unregister_worker(&self, worker_id: &str, final_status: &str) -> Result<DisconnectSummary> {
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
            tx.execute(
                "DELETE FROM workers WHERE id = ?1",
                params![worker_id],
            )?;

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
                "SELECT id, tags, max_claims, registered_at, last_heartbeat
                 FROM workers ORDER BY registered_at DESC",
            )?;

            let workers = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let tags_json: String = row.get(1)?;
                let max_claims: i32 = row.get(2)?;
                let registered_at: i64 = row.get(3)?;
                let last_heartbeat: i64 = row.get(4)?;

                Ok((id, tags_json, max_claims, registered_at, last_heartbeat))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, tags_json, max_claims, registered_at, last_heartbeat)| {
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Worker {
                    id,
                    tags,
                    max_claims,
                    registered_at,
                    last_heartbeat,
                }
            })
            .collect();

            Ok(workers)
        })
    }

    /// List all workers with extended info (claim count, current thought).
    pub fn list_workers_info(&self) -> Result<Vec<crate::types::WorkerInfo>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT w.id, w.tags, w.max_claims, w.registered_at, w.last_heartbeat,
                        (SELECT COUNT(*) FROM tasks WHERE worker_id = w.id AND status = 'in_progress') as claim_count,
                        (SELECT current_thought FROM tasks WHERE worker_id = w.id AND status = 'in_progress' AND current_thought IS NOT NULL LIMIT 1) as current_thought
                 FROM workers w ORDER BY w.registered_at DESC",
            )?;

            let workers = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let tags_json: String = row.get(1)?;
                let max_claims: i32 = row.get(2)?;
                let registered_at: i64 = row.get(3)?;
                let last_heartbeat: i64 = row.get(4)?;
                let claim_count: i32 = row.get(5)?;
                let current_thought: Option<String> = row.get(6)?;

                Ok((id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought)| {
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                crate::types::WorkerInfo {
                    id,
                    tags,
                    max_claims,
                    claim_count,
                    current_thought,
                    registered_at,
                    last_heartbeat,
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
    ) -> Result<Vec<crate::types::WorkerInfo>> {
        self.with_conn(|conn| {
            // Start with base query
            let mut sql = String::from(
                "SELECT DISTINCT w.id, w.tags, w.max_claims, w.registered_at, w.last_heartbeat,
                        (SELECT COUNT(*) FROM tasks WHERE worker_id = w.id AND status = 'in_progress') as claim_count,
                        (SELECT current_thought FROM tasks WHERE worker_id = w.id AND status = 'in_progress' AND current_thought IS NOT NULL LIMIT 1) as current_thought
                 FROM workers w WHERE 1=1",
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

                    Ok((id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought))
                })?
                .filter_map(|r| r.ok())
                .map(|(id, tags_json, max_claims, registered_at, last_heartbeat, claim_count, current_thought)| {
                    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                    crate::types::WorkerInfo {
                        id,
                        tags,
                        max_claims,
                        claim_count,
                        current_thought,
                        registered_at,
                        last_heartbeat,
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
    fn get_related_task_ids_internal(conn: &Connection, task_id: &str, depth: i32) -> Result<Vec<String>> {
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
                    let mut stmt = conn.prepare(
                        "SELECT to_task_id FROM dependencies WHERE from_task_id = ?1"
                    )?;
                    stmt.query_map(params![tid], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect()
                } else {
                    // Ancestors: tasks where this task is the to_task_id (parents, blockers)
                    let mut stmt = conn.prepare(
                        "SELECT from_task_id FROM dependencies WHERE to_task_id = ?1"
                    )?;
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
                "SELECT id, tags, max_claims, registered_at, last_heartbeat
                 FROM workers WHERE last_heartbeat < ?1",
            )?;

            let workers = stmt.query_map(params![cutoff], |row| {
                let id: String = row.get(0)?;
                let tags_json: String = row.get(1)?;
                let max_claims: i32 = row.get(2)?;
                let registered_at: i64 = row.get(3)?;
                let last_heartbeat: i64 = row.get(4)?;

                Ok((id, tags_json, max_claims, registered_at, last_heartbeat))
            })?
            .filter_map(|r| r.ok())
            .map(|(id, tags_json, max_claims, registered_at, last_heartbeat)| {
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Worker {
                    id,
                    tags,
                    max_claims,
                    registered_at,
                    last_heartbeat,
                }
            })
            .collect();

            Ok(workers)
        })
    }

    /// Cleanup stale workers by evicting them and releasing their claims.
    /// Returns a summary of the cleanup operation.
    pub fn cleanup_stale_workers(&self, timeout_seconds: i64, final_status: &str) -> Result<CleanupSummary> {
        let stale_workers = self.get_stale_workers(timeout_seconds)?;
        
        let mut total_tasks_released = 0;
        let mut total_files_released = 0;
        let mut evicted_worker_ids = Vec::new();
        
        for worker in &stale_workers {
            // Release file locks first
            let _ = self.release_worker_locks(&worker.id);
            
            // Unregister the worker
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

    /// Get claim count for a worker.
    pub fn get_claim_count(&self, worker_id: &str) -> Result<i32> {
        self.with_conn(|conn| {
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE worker_id = ?1 AND status = 'in_progress'",
                params![worker_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
    }
}
