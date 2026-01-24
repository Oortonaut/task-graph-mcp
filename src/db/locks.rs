//! File lock operations (advisory) and claim tracking.

use super::{now_ms, Database};
use crate::types::{ClaimEvent, ClaimEventType, ClaimUpdates, FileLock};
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

impl Database {
    /// Lock a file (advisory).
    /// Returns Ok with optional warning if already locked by another agent.
    pub fn lock_file(&self, file_path: String, agent_id: &str, reason: Option<String>) -> Result<Option<String>> {
        let now = now_ms();

        self.with_conn(|conn| {
            // Check if already locked
            let existing: Option<String> = conn
                .query_row(
                    "SELECT agent_id FROM file_locks WHERE file_path = ?1",
                    params![&file_path],
                    |row| row.get(0),
                )
                .ok();

            if let Some(existing_agent) = existing {
                if existing_agent != agent_id {
                    // Locked by another agent - return warning
                    return Ok(Some(existing_agent));
                }
                // Already locked by this agent - just update timestamp and reason
                conn.execute(
                    "UPDATE file_locks SET locked_at = ?1, reason = ?2 WHERE file_path = ?3",
                    params![now, &reason, &file_path],
                )?;
            } else {
                // Not locked - create new lock
                conn.execute(
                    "INSERT INTO file_locks (file_path, agent_id, reason, locked_at) VALUES (?1, ?2, ?3, ?4)",
                    params![&file_path, agent_id, &reason, now],
                )?;

                // Record claim event for tracking
                conn.execute(
                    "INSERT INTO claim_sequence (file_path, agent_id, event, reason, timestamp) VALUES (?1, ?2, 'claimed', ?3, ?4)",
                    params![&file_path, agent_id, &reason, now],
                )?;
            }

            Ok(None)
        })
    }

    /// Unlock a file with optional reason for next claimant.
    pub fn unlock_file(&self, file_path: &str, agent_id: &str, reason: Option<String>) -> Result<bool> {
        let now = now_ms();

        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM file_locks WHERE file_path = ?1 AND agent_id = ?2",
                params![file_path, agent_id],
            )?;

            if deleted > 0 {
                // Record release event for tracking
                conn.execute(
                    "INSERT INTO claim_sequence (file_path, agent_id, event, reason, timestamp) VALUES (?1, ?2, 'released', ?3, ?4)",
                    params![file_path, agent_id, &reason, now],
                )?;
            }

            Ok(deleted > 0)
        })
    }

    /// Get claim updates since agent's last poll.
    pub fn claim_updates(&self, agent_id: &str, files: Option<Vec<String>>) -> Result<ClaimUpdates> {
        self.with_conn(|conn| {
            // Get agent's last sequence
            let last_seq: i64 = conn
                .query_row(
                    "SELECT last_claim_sequence FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Get new events since last sequence
            let events: Vec<ClaimEvent> = if let Some(ref paths) = files {
                if paths.is_empty() {
                    Vec::new()
                } else {
                    let placeholders: Vec<String> = paths.iter().map(|_| "?".to_string()).collect();
                    let sql = format!(
                        "SELECT id, file_path, agent_id, event, reason, timestamp 
                         FROM claim_sequence 
                         WHERE id > ?1 AND file_path IN ({})
                         ORDER BY id",
                        placeholders.join(", ")
                    );

                    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                    params_vec.push(Box::new(last_seq));
                    for path in paths {
                        params_vec.push(Box::new(path.clone()));
                    }

                    let params_refs: Vec<&dyn rusqlite::ToSql> =
                        params_vec.iter().map(|b| b.as_ref()).collect();

                    let mut stmt = conn.prepare(&sql)?;
                    stmt.query_map(params_refs.as_slice(), |row| {
                        let id: i64 = row.get(0)?;
                        let file_path: String = row.get(1)?;
                        let agent_id: String = row.get(2)?;
                        let event_str: String = row.get(3)?;
                        let reason: Option<String> = row.get(4)?;
                        let timestamp: i64 = row.get(5)?;
                        Ok(ClaimEvent {
                            id,
                            file_path,
                            agent_id,
                            event: ClaimEventType::from_str(&event_str).unwrap_or(ClaimEventType::Claimed),
                            reason,
                            timestamp,
                        })
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, file_path, agent_id, event, reason, timestamp 
                     FROM claim_sequence 
                     WHERE id > ?1
                     ORDER BY id"
                )?;
                stmt.query_map(params![last_seq], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let agent_id: String = row.get(2)?;
                    let event_str: String = row.get(3)?;
                    let reason: Option<String> = row.get(4)?;
                    let timestamp: i64 = row.get(5)?;
                    Ok(ClaimEvent {
                        id,
                        file_path,
                        agent_id,
                        event: ClaimEventType::from_str(&event_str).unwrap_or(ClaimEventType::Claimed),
                        reason,
                        timestamp,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            // Find max sequence from events
            let new_seq = events.iter().map(|e| e.id).max().unwrap_or(last_seq);

            // Update agent's last sequence
            if new_seq > last_seq {
                conn.execute(
                    "UPDATE agents SET last_claim_sequence = ?1 WHERE id = ?2",
                    params![new_seq, agent_id],
                )?;
            }

            // Separate into claims and releases
            let new_claims: Vec<ClaimEvent> = events.iter()
                .filter(|e| e.event == ClaimEventType::Claimed)
                .cloned()
                .collect();
            let dropped_claims: Vec<ClaimEvent> = events.iter()
                .filter(|e| e.event == ClaimEventType::Released)
                .cloned()
                .collect();

            Ok(ClaimUpdates {
                new_claims,
                dropped_claims,
                sequence: new_seq,
            })
        })
    }

    /// Get file locks.
    pub fn get_file_locks(
        &self,
        file_paths: Option<Vec<String>>,
        agent_id: Option<&str>,
    ) -> Result<HashMap<String, String>> {
        self.with_conn(|conn| {
            let locks = if let Some(paths) = file_paths {
                if paths.is_empty() {
                    return Ok(HashMap::new());
                }

                let placeholders: Vec<String> = paths.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "SELECT file_path, agent_id FROM file_locks WHERE file_path IN ({})",
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
                    let path: String = row.get(0)?;
                    let agent: String = row.get(1)?;
                    Ok((path, agent))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else if let Some(aid) = agent_id {
                let mut stmt = conn.prepare(
                    "SELECT file_path, agent_id FROM file_locks WHERE agent_id = ?1",
                )?;
                stmt.query_map(params![aid], |row| {
                    let path: String = row.get(0)?;
                    let agent: String = row.get(1)?;
                    Ok((path, agent))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                let mut stmt = conn.prepare("SELECT file_path, agent_id FROM file_locks")?;
                stmt.query_map([], |row| {
                    let path: String = row.get(0)?;
                    let agent: String = row.get(1)?;
                    Ok((path, agent))
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            Ok(locks)
        })
    }

    /// Get all file locks as FileLock objects.
    pub fn get_all_file_locks(&self) -> Result<Vec<FileLock>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT file_path, agent_id, reason, locked_at FROM file_locks")?;

            let locks = stmt
                .query_map([], |row| {
                    let file_path: String = row.get(0)?;
                    let agent_id: String = row.get(1)?;
                    let reason: Option<String> = row.get(2)?;
                    let locked_at: i64 = row.get(3)?;
                    Ok(FileLock {
                        file_path,
                        agent_id,
                        reason,
                        locked_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(locks)
        })
    }

    /// Release all locks held by an agent.
    pub fn release_agent_locks(&self, agent_id: &str) -> Result<i32> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM file_locks WHERE agent_id = ?1",
                params![agent_id],
            )?;

            Ok(deleted as i32)
        })
    }
}
