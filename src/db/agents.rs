//! Agent CRUD operations.

use super::{now_ms, Database};
use crate::types::Agent;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Maximum length for agent IDs.
pub const MAX_AGENT_ID_LEN: usize = 36;

/// Internal helper to get an agent using an existing connection (avoids deadlock).
fn get_agent_internal(conn: &Connection, agent_id: &str) -> Result<Option<Agent>> {
    let mut stmt = conn.prepare(
        "SELECT id, tags, max_claims, registered_at, last_heartbeat
         FROM agents WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![agent_id], |row| {
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
            Ok(Some(Agent {
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
    /// Register a new agent.
    ///
    /// If `agent_id` is provided, it must be at most 36 characters.
    /// If not provided, a UUID7 (time-sortable) will be generated.
    /// If `force` is true and the agent already exists, it will be re-registered
    /// (useful for stuck agent recovery).
    pub fn register_agent(
        &self,
        agent_id: Option<String>,
        tags: Vec<String>,
        force: bool,
    ) -> Result<Agent> {
        let id = match agent_id {
            Some(id) => {
                if id.len() > MAX_AGENT_ID_LEN {
                    return Err(anyhow!(
                        "Agent ID must be at most {} characters, got {}",
                        MAX_AGENT_ID_LEN,
                        id.len()
                    ));
                }
                if id.is_empty() {
                    return Err(anyhow!("Agent ID cannot be empty"));
                }
                id
            }
            None => Uuid::now_v7().to_string(),
        };
        let now = now_ms();
        let max_claims = 5; // Default, TODO: make configurable
        let tags_json = serde_json::to_string(&tags)?;

        self.with_conn(|conn| {
            // Check if agent ID already exists
            let exists: bool = conn
                .query_row("SELECT 1 FROM agents WHERE id = ?1", params![&id], |_| Ok(true))
                .unwrap_or(false);

            if exists {
                if force {
                    // Force reconnection: update existing agent
                    conn.execute(
                        "UPDATE agents SET tags = ?1, max_claims = ?2, last_heartbeat = ?3 WHERE id = ?4",
                        params![tags_json, max_claims, now, &id],
                    )?;
                } else {
                    return Err(anyhow!("Agent ID '{}' already registered. Use force=true to reconnect.", id));
                }
            } else {
                conn.execute(
                    "INSERT INTO agents (id, tags, max_claims, registered_at, last_heartbeat)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![&id, tags_json, max_claims, now, now],
                )?;
            }

            Ok(Agent {
                id,
                tags,
                max_claims,
                registered_at: now,
                last_heartbeat: now,
            })
        })
    }

    /// Get an agent by ID.
    pub fn get_agent(&self, agent_id: &str) -> Result<Option<Agent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tags, max_claims, registered_at, last_heartbeat
                 FROM agents WHERE id = ?1",
            )?;

            let result = stmt.query_row(params![agent_id], |row| {
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
                    Ok(Some(Agent {
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

    /// Check if an agent exists. Returns error if not found.
    pub fn require_agent(&self, agent_id: &str) -> Result<Agent> {
        self.get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Agent {} not found", agent_id))
    }

    /// Update an agent.
    pub fn update_agent(
        &self,
        agent_id: &str,
        tags: Option<Vec<String>>,
        max_claims: Option<i32>,
    ) -> Result<Agent> {
        self.with_conn(|conn| {
            let agent = get_agent_internal(conn, agent_id)?
                .ok_or_else(|| anyhow!("Agent not found"))?;

            let new_tags = tags.unwrap_or(agent.tags.clone());
            let new_max_claims = max_claims.unwrap_or(agent.max_claims);
            let tags_json = serde_json::to_string(&new_tags)?;

            conn.execute(
                "UPDATE agents SET tags = ?1, max_claims = ?2 WHERE id = ?3",
                params![tags_json, new_max_claims, agent_id],
            )?;

            Ok(Agent {
                id: agent_id.to_string(),
                tags: new_tags,
                max_claims: new_max_claims,
                registered_at: agent.registered_at,
                last_heartbeat: agent.last_heartbeat,
            })
        })
    }

    /// Update agent heartbeat.
    pub fn heartbeat(&self, agent_id: &str) -> Result<i32> {
        let now = now_ms();

        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE agents SET last_heartbeat = ?1 WHERE id = ?2",
                params![now, agent_id],
            )?;

            if updated == 0 {
                return Err(anyhow!("Agent not found"));
            }

            // Return current claim count
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE owner_agent = ?1 AND status = 'in_progress'",
                params![agent_id],
                |row| row.get(0),
            )?;

            Ok(count)
        })
    }

    /// Unregister an agent (releases all claims).
    pub fn unregister_agent(&self, agent_id: &str) -> Result<()> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Release all task claims
            tx.execute(
                "UPDATE tasks SET owner_agent = NULL, claimed_at = NULL
                 WHERE owner_agent = ?1",
                params![agent_id],
            )?;

            // Remove all file locks
            tx.execute(
                "DELETE FROM file_locks WHERE agent_id = ?1",
                params![agent_id],
            )?;

            // Remove agent
            tx.execute(
                "DELETE FROM agents WHERE id = ?1",
                params![agent_id],
            )?;

            tx.commit()?;
            Ok(())
        })
    }

    /// List all agents.
    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tags, max_claims, registered_at, last_heartbeat
                 FROM agents ORDER BY registered_at DESC",
            )?;

            let agents = stmt.query_map([], |row| {
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
                Agent {
                    id,
                    tags,
                    max_claims,
                    registered_at,
                    last_heartbeat,
                }
            })
            .collect();

            Ok(agents)
        })
    }

    /// List all agents with extended info (claim count, current thought).
    pub fn list_agents_info(&self) -> Result<Vec<crate::types::AgentInfo>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT a.id, a.tags, a.max_claims, a.registered_at, a.last_heartbeat,
                        (SELECT COUNT(*) FROM tasks WHERE owner_agent = a.id AND status = 'in_progress') as claim_count,
                        (SELECT current_thought FROM tasks WHERE owner_agent = a.id AND status = 'in_progress' AND current_thought IS NOT NULL LIMIT 1) as current_thought
                 FROM agents a ORDER BY a.registered_at DESC",
            )?;

            let agents = stmt.query_map([], |row| {
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
                crate::types::AgentInfo {
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

            Ok(agents)
        })
    }

    /// Get agents with stale heartbeats.
    #[allow(dead_code)]
    pub fn get_stale_agents(&self, timeout_seconds: i64) -> Result<Vec<Agent>> {
        let cutoff = now_ms() - (timeout_seconds * 1000);

        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tags, max_claims, registered_at, last_heartbeat
                 FROM agents WHERE last_heartbeat < ?1",
            )?;

            let agents = stmt.query_map(params![cutoff], |row| {
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
                Agent {
                    id,
                    tags,
                    max_claims,
                    registered_at,
                    last_heartbeat,
                }
            })
            .collect();

            Ok(agents)
        })
    }

    /// Get claim count for an agent.
    pub fn get_claim_count(&self, agent_id: &str) -> Result<i32> {
        self.with_conn(|conn| {
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE owner_agent = ?1 AND status = 'in_progress'",
                params![agent_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
    }
}
