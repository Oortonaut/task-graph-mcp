//! File lock operations (advisory).

use super::{now_ms, Database};
use crate::types::FileLock;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

impl Database {
    /// Lock a file (advisory).
    /// Returns Ok with optional warning if already locked by another agent.
    pub fn lock_file(&self, file_path: String, agent_id: &str) -> Result<Option<String>> {
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
                // Already locked by this agent - just update timestamp
                conn.execute(
                    "UPDATE file_locks SET locked_at = ?1 WHERE file_path = ?2",
                    params![now, &file_path],
                )?;
            } else {
                // Not locked - create new lock
                conn.execute(
                    "INSERT INTO file_locks (file_path, agent_id, locked_at) VALUES (?1, ?2, ?3)",
                    params![&file_path, agent_id, now],
                )?;
            }

            Ok(None)
        })
    }

    /// Unlock a file.
    pub fn unlock_file(&self, file_path: &str, agent_id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM file_locks WHERE file_path = ?1 AND agent_id = ?2",
                params![file_path, agent_id],
            )?;

            Ok(deleted > 0)
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
                conn.prepare("SELECT file_path, agent_id, locked_at FROM file_locks")?;

            let locks = stmt
                .query_map([], |row| {
                    let file_path: String = row.get(0)?;
                    let agent_id: String = row.get(1)?;
                    let locked_at: i64 = row.get(2)?;
                    Ok(FileLock {
                        file_path,
                        agent_id,
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
