//! File lock operations (advisory) and claim tracking.

use super::{now_ms, Database};
use crate::types::{ClaimEvent, ClaimEventType, ClaimUpdates, FileLock};
use anyhow::Result;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

impl Database {
    /// Lock a file (advisory).
    /// Returns Ok with optional warning if already locked by another worker.
    pub fn lock_file(&self, file_path: String, worker_id: &str, reason: Option<String>, task_id: Option<String>) -> Result<Option<String>> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            // Check if already locked
            let existing: Option<String> = tx
                .query_row(
                    "SELECT worker_id FROM file_locks WHERE file_path = ?1",
                    params![&file_path],
                    |row| row.get(0),
                )
                .ok();

            let result = if let Some(existing_worker) = existing {
                if existing_worker != worker_id {
                    // Locked by another worker - return warning
                    Some(existing_worker)
                } else {
                    // Already locked by this worker - just update timestamp, reason, and task_id
                    tx.execute(
                        "UPDATE file_locks SET locked_at = ?1, reason = ?2, task_id = ?3 WHERE file_path = ?4",
                        params![now, &reason, &task_id, &file_path],
                    )?;
                    None
                }
            } else {
                // Not locked - create new lock
                tx.execute(
                    "INSERT INTO file_locks (file_path, worker_id, reason, locked_at, task_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![&file_path, worker_id, &reason, now, &task_id],
                )?;

                // Record claim event for tracking
                tx.execute(
                    "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp) VALUES (?1, ?2, 'claimed', ?3, ?4)",
                    params![&file_path, worker_id, &reason, now],
                )?;
                None
            };

            tx.commit()?;
            Ok(result)
        })
    }

    /// Unlock a file with optional reason for next claimant.
    pub fn unlock_file(&self, file_path: &str, worker_id: &str, reason: Option<String>) -> Result<bool> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            let deleted = tx.execute(
                "DELETE FROM file_locks WHERE file_path = ?1 AND worker_id = ?2",
                params![file_path, worker_id],
            )?;

            if deleted > 0 {
                // Find the claim_id for this file+worker (most recent claim)
                let claim_id: Option<i64> = tx.query_row(
                    "SELECT MAX(id) FROM claim_sequence
                     WHERE file_path = ?1 AND worker_id = ?2 AND event = 'claimed'",
                    params![file_path, worker_id],
                    |row| row.get(0),
                ).ok().flatten();

                // Close any open claim for this file+worker
                tx.execute(
                    "UPDATE claim_sequence SET end_timestamp = ?1
                     WHERE file_path = ?2 AND worker_id = ?3 AND end_timestamp IS NULL",
                    params![now, file_path, worker_id],
                )?;

                // Record release event with claim_id reference
                tx.execute(
                    "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp, claim_id)
                     VALUES (?1, ?2, 'released', ?3, ?4, ?5)",
                    params![file_path, worker_id, &reason, now, claim_id],
                )?;
            }

            tx.commit()?;
            Ok(deleted > 0)
        })
    }

    /// Unlock multiple files with verbose return.
    /// Returns a list of (file_path, worker_id) pairs for files that were actually released.
    pub fn unlock_files_verbose(
        &self,
        file_paths: Vec<String>,
        worker_id: &str,
        reason: Option<String>,
    ) -> Result<Vec<(String, String)>> {
        let now = now_ms();
        let mut released = Vec::new();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            for file_path in file_paths {
                let deleted = tx.execute(
                    "DELETE FROM file_locks WHERE file_path = ?1 AND worker_id = ?2",
                    params![&file_path, worker_id],
                )?;

                if deleted > 0 {
                    // Find the claim_id for this file+worker (most recent claim)
                    let claim_id: Option<i64> = tx.query_row(
                        "SELECT MAX(id) FROM claim_sequence
                         WHERE file_path = ?1 AND worker_id = ?2 AND event = 'claimed'",
                        params![&file_path, worker_id],
                        |row| row.get(0),
                    ).ok().flatten();

                    // Close any open claim for this file+worker
                    tx.execute(
                        "UPDATE claim_sequence SET end_timestamp = ?1
                         WHERE file_path = ?2 AND worker_id = ?3 AND end_timestamp IS NULL",
                        params![now, &file_path, worker_id],
                    )?;

                    // Record release event with claim_id reference
                    tx.execute(
                        "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp, claim_id)
                         VALUES (?1, ?2, 'released', ?3, ?4, ?5)",
                        params![&file_path, worker_id, &reason, now, claim_id],
                    )?;

                    released.push((file_path, worker_id.to_string()));
                }
            }

            tx.commit()?;
            Ok(released)
        })
    }

    /// Release all files held by a worker with verbose return.
    /// Returns a list of (file_path, worker_id) pairs for files that were released.
    pub fn release_worker_locks_verbose(&self, worker_id: &str, reason: Option<String>) -> Result<Vec<(String, String)>> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get files locked by this worker before deleting
            let files_to_release: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT file_path FROM file_locks WHERE worker_id = ?1"
                )?;
                stmt.query_map(params![worker_id], |row| row.get::<_, String>(0))?
                    .filter_map(|r| r.ok())
                    .collect()
            };

            if files_to_release.is_empty() {
                tx.commit()?;
                return Ok(Vec::new());
            }

            // Close any open claims for this worker
            tx.execute(
                "UPDATE claim_sequence SET end_timestamp = ?1
                 WHERE worker_id = ?2 AND end_timestamp IS NULL",
                params![now, worker_id],
            )?;

            // Record release events for each file
            for file_path in &files_to_release {
                tx.execute(
                    "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp)
                     VALUES (?1, ?2, 'released', ?3, ?4)",
                    params![file_path, worker_id, &reason, now],
                )?;
            }

            // Delete the locks
            tx.execute(
                "DELETE FROM file_locks WHERE worker_id = ?1",
                params![worker_id],
            )?;

            tx.commit()?;

            let released: Vec<(String, String)> = files_to_release
                .into_iter()
                .map(|f| (f, worker_id.to_string()))
                .collect();

            Ok(released)
        })
    }

    /// Release all files associated with a task with verbose return.
    /// Returns a list of (file_path, worker_id) pairs for files that were released.
    pub fn release_task_locks_verbose(&self, task_id: &str, reason: Option<String>) -> Result<Vec<(String, String)>> {
        let now = now_ms();

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Get files locked by this task before deleting
            let files_to_release: Vec<(String, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT file_path, worker_id FROM file_locks WHERE task_id = ?1"
                )?;
                stmt.query_map(params![task_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            if files_to_release.is_empty() {
                tx.commit()?;
                return Ok(Vec::new());
            }

            // Close any open claims for these files
            for (file_path, worker_id) in &files_to_release {
                tx.execute(
                    "UPDATE claim_sequence SET end_timestamp = ?1
                     WHERE file_path = ?2 AND worker_id = ?3 AND end_timestamp IS NULL",
                    params![now, file_path, worker_id],
                )?;

                // Record release event
                let reason_str = reason.as_deref().unwrap_or("task release");
                tx.execute(
                    "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp)
                     VALUES (?1, ?2, 'released', ?3, ?4)",
                    params![file_path, worker_id, reason_str, now],
                )?;
            }

            // Delete the locks
            tx.execute(
                "DELETE FROM file_locks WHERE task_id = ?1",
                params![task_id],
            )?;

            tx.commit()?;
            Ok(files_to_release)
        })
    }

    /// Get claim updates since worker's last poll.
    /// Returns all claim/release events since the agent's last poll position.
    pub fn claim_updates(&self, worker_id: &str) -> Result<ClaimUpdates> {
        self.with_conn(|conn| {
            // Get worker's last sequence
            let last_seq: i64 = conn
                .query_row(
                    "SELECT last_claim_sequence FROM workers WHERE id = ?1",
                    params![worker_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Get all new events since last sequence.
            // We use >= because last_seq now represents "next event to fetch" (set to max+1 after each poll).
            let mut stmt = conn.prepare(
                "SELECT id, file_path, worker_id, event, reason, timestamp, end_timestamp, claim_id
                 FROM claim_sequence
                 WHERE id >= ?1
                 ORDER BY id"
            )?;
            let events: Vec<ClaimEvent> = stmt.query_map(params![last_seq], |row| {
                Ok(ClaimEvent {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    worker_id: row.get(2)?,
                    event: ClaimEventType::from_str(&row.get::<_, String>(3)?).unwrap_or(ClaimEventType::Claimed),
                    reason: row.get(4)?,
                    timestamp: row.get(5)?,
                    end_timestamp: row.get(6)?,
                    claim_id: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

            // Find max sequence from events. After polling, we set last_seq = max + 1
            // so that claims we just saw have claim_id < last_seq (for release filtering).
            let max_seen = events.iter().map(|e| e.id).max();
            let new_seq = match max_seen {
                Some(max) => max + 1,  // +1 so claims just polled satisfy claim_id < new_seq
                None => last_seq,       // No events, keep current sequence
            };

            // Update worker's last sequence
            if new_seq > last_seq {
                conn.execute(
                    "UPDATE workers SET last_claim_sequence = ?1 WHERE id = ?2",
                    params![new_seq, worker_id],
                )?;
            }

            // Separate into claims and releases
            let new_claims: Vec<ClaimEvent> = events.iter()
                .filter(|e| e.event == ClaimEventType::Claimed)
                .cloned()
                .collect();

            // For releases, only include if agent has polled and received the original claim.
            // Include if claim_id < last_seq (strictly less - was in a previous poll, and
            // last_seq is max+1 after each poll) OR claim_id is in current batch.
            let new_claim_ids: HashSet<i64> = new_claims.iter()
                .map(|c| c.id)
                .collect();

            let dropped_claims: Vec<ClaimEvent> = events.iter()
                .filter(|e| e.event == ClaimEventType::Released)
                .filter(|release| {
                    match release.claim_id {
                        Some(cid) => cid < last_seq || new_claim_ids.contains(&cid),
                        None => true, // Legacy releases without claim_id - include them
                    }
                })
                .cloned()
                .collect();

            Ok(ClaimUpdates {
                new_claims,
                dropped_claims,
                sequence: new_seq,
            })
        })
    }

    /// Get file locks with full details.
    pub fn get_file_locks(
        &self,
        file_paths: Option<Vec<String>>,
        agent_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<HashMap<String, FileLock>> {
        self.with_conn(|conn| {
            let locks = if let Some(paths) = file_paths {
                if paths.is_empty() {
                    return Ok(HashMap::new());
                }

                let placeholders: Vec<String> = paths.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "SELECT file_path, worker_id, reason, locked_at, task_id FROM file_locks WHERE file_path IN ({})",
                    placeholders.join(", ")
                );

                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                for path in &paths {
                    params_vec.push(Box::new(path.clone()));
                }

                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();

                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params_refs.as_slice(), |row| {
                    let file_path: String = row.get(0)?;
                    Ok((file_path.clone(), FileLock {
                        file_path,
                        worker_id: row.get(1)?,
                        reason: row.get(2)?,
                        locked_at: row.get(3)?,
                        task_id: row.get(4)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else if let Some(aid) = agent_id {
                let mut stmt = conn.prepare(
                    "SELECT file_path, worker_id, reason, locked_at, task_id FROM file_locks WHERE worker_id = ?1",
                )?;
                stmt.query_map(params![aid], |row| {
                    let file_path: String = row.get(0)?;
                    Ok((file_path.clone(), FileLock {
                        file_path,
                        worker_id: row.get(1)?,
                        reason: row.get(2)?,
                        locked_at: row.get(3)?,
                        task_id: row.get(4)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else if let Some(tid) = task_id {
                let mut stmt = conn.prepare(
                    "SELECT file_path, worker_id, reason, locked_at, task_id FROM file_locks WHERE task_id = ?1",
                )?;
                stmt.query_map(params![tid], |row| {
                    let file_path: String = row.get(0)?;
                    Ok((file_path.clone(), FileLock {
                        file_path,
                        worker_id: row.get(1)?,
                        reason: row.get(2)?,
                        locked_at: row.get(3)?,
                        task_id: row.get(4)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                // Return empty - we now require at least one filter
                HashMap::new()
            };

            Ok(locks)
        })
    }

    /// Get all file locks as FileLock objects.
    pub fn get_all_file_locks(&self) -> Result<Vec<FileLock>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT file_path, worker_id, reason, locked_at, task_id FROM file_locks")?;

            let locks = stmt
                .query_map([], |row| {
                    let file_path: String = row.get(0)?;
                    let worker_id: String = row.get(1)?;
                    let reason: Option<String> = row.get(2)?;
                    let locked_at: i64 = row.get(3)?;
                    let task_id: Option<String> = row.get(4)?;
                    Ok(FileLock {
                        file_path,
                        worker_id,
                        reason,
                        locked_at,
                        task_id,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(locks)
        })
    }

    /// Release all locks held by a worker.
    pub fn release_worker_locks(&self, worker_id: &str) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            // Close any open claims for this worker
            conn.execute(
                "UPDATE claim_sequence SET end_timestamp = ?1
                 WHERE worker_id = ?2 AND end_timestamp IS NULL",
                params![now, worker_id],
            )?;

            let deleted = conn.execute(
                "DELETE FROM file_locks WHERE worker_id = ?1",
                params![worker_id],
            )?;

            Ok(deleted as i32)
        })
    }


    /// Release all locks associated with a task.
    /// Called automatically when a task completes.
    pub fn release_task_locks(&self, task_id: &str) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            // Get files locked by this task before deleting
            let files_to_release: Vec<(String, String)> = {
                let mut stmt = conn.prepare(
                    "SELECT file_path, worker_id FROM file_locks WHERE task_id = ?1"
                )?;
                stmt.query_map(params![task_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            // Close any open claims for these files
            for (file_path, worker_id) in &files_to_release {
                conn.execute(
                    "UPDATE claim_sequence SET end_timestamp = ?1
                     WHERE file_path = ?2 AND worker_id = ?3 AND end_timestamp IS NULL",
                    params![now, file_path, worker_id],
                )?;

                // Record release event
                conn.execute(
                    "INSERT INTO claim_sequence (file_path, worker_id, event, reason, timestamp)
                     VALUES (?1, ?2, 'released', 'task completed', ?3)",
                    params![file_path, worker_id, now],
                )?;
            }

            let deleted = conn.execute(
                "DELETE FROM file_locks WHERE task_id = ?1",
                params![task_id],
            )?;

            Ok(deleted as i32)
        })
    }
}
