//! Pub/sub inbox operations.

use super::{now_ms, Database};
use crate::types::{EventType, InboxMessage, Subscription, TargetType};
use anyhow::Result;
use rusqlite::params;
use uuid::Uuid;

impl Database {
    /// Subscribe to events for a target.
    pub fn subscribe(
        &self,
        agent_id: &str,
        target_type: TargetType,
        target_id: String,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let now = now_ms();

        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO subscriptions (id, agent_id, target_type, target_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.to_string(),
                    agent_id,
                    target_type.as_str(),
                    &target_id,
                    now,
                ],
            )?;

            Ok(id)
        })
    }

    /// Unsubscribe from events.
    pub fn unsubscribe(&self, subscription_id: Uuid) -> Result<bool> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM subscriptions WHERE id = ?1",
                params![subscription_id.to_string()],
            )?;

            Ok(deleted > 0)
        })
    }

    /// Get subscriptions for an agent.
    pub fn get_subscriptions(&self, agent_id: &str) -> Result<Vec<Subscription>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, agent_id, target_type, target_id, created_at
                 FROM subscriptions WHERE agent_id = ?1",
            )?;

            let subs = stmt
                .query_map(params![agent_id], |row| {
                    let id: String = row.get(0)?;
                    let agent_id: String = row.get(1)?;
                    let target_type: String = row.get(2)?;
                    let target_id: String = row.get(3)?;
                    let created_at: i64 = row.get(4)?;

                    Ok(Subscription {
                        id: Uuid::parse_str(&id).unwrap(),
                        agent_id,
                        target_type: TargetType::from_str(&target_type).unwrap_or(TargetType::Task),
                        target_id,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(subs)
        })
    }

    /// Get agents subscribed to a target.
    pub fn get_subscribers(
        &self,
        target_type: TargetType,
        target_id: &str,
    ) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT agent_id FROM subscriptions WHERE target_type = ?1 AND target_id = ?2",
            )?;

            let agents = stmt
                .query_map(params![target_type.as_str(), target_id], |row| {
                    let id: String = row.get(0)?;
                    Ok(id)
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(agents)
        })
    }

    /// Publish an event to subscribers.
    pub fn publish_event(
        &self,
        target_type: TargetType,
        target_id: &str,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Result<i32> {
        let subscribers = self.get_subscribers(target_type, target_id)?;

        for agent_id in &subscribers {
            self.add_inbox_message(agent_id, event_type, payload.clone())?;
        }

        Ok(subscribers.len() as i32)
    }

    /// Add a message to an agent's inbox.
    pub fn add_inbox_message(
        &self,
        agent_id: &str,
        event_type: EventType,
        payload: serde_json::Value,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let now = now_ms();
        let payload_json = serde_json::to_string(&payload)?;

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO inbox (id, agent_id, event_type, payload, created_at, read)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![
                    id.to_string(),
                    agent_id,
                    event_type.as_str(),
                    payload_json,
                    now,
                ],
            )?;

            Ok(id)
        })
    }

    /// Poll inbox for messages.
    pub fn poll_inbox(
        &self,
        agent_id: &str,
        limit: Option<i32>,
        mark_read: bool,
    ) -> Result<Vec<InboxMessage>> {
        self.with_conn_mut(|conn| {
            let limit_clause = limit.map(|l| format!(" LIMIT {}", l)).unwrap_or_default();
            let sql = format!(
                "SELECT id, agent_id, event_type, payload, created_at, read
                 FROM inbox
                 WHERE agent_id = ?1 AND read = 0
                 ORDER BY created_at ASC{}",
                limit_clause
            );

            let tx = conn.transaction()?;

            let messages: Vec<InboxMessage> = {
                let mut stmt = tx.prepare(&sql)?;
                stmt.query_map(params![agent_id], |row| {
                    let id: String = row.get(0)?;
                    let agent_id: String = row.get(1)?;
                    let event_type: String = row.get(2)?;
                    let payload_json: String = row.get(3)?;
                    let created_at: i64 = row.get(4)?;
                    let read: i32 = row.get(5)?;

                    Ok(InboxMessage {
                        id: Uuid::parse_str(&id).unwrap(),
                        agent_id,
                        event_type: EventType::from_str(&event_type)
                            .unwrap_or(EventType::TaskUpdated),
                        payload: serde_json::from_str(&payload_json).unwrap_or_default(),
                        created_at,
                        read: read != 0,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect()
            };

            if mark_read && !messages.is_empty() {
                let ids: Vec<String> = messages.iter().map(|m| m.id.to_string()).collect();
                let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
                let update_sql = format!(
                    "UPDATE inbox SET read = 1 WHERE id IN ({})",
                    placeholders.join(", ")
                );

                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                for id in &ids {
                    params_vec.push(Box::new(id.clone()));
                }

                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|b| b.as_ref()).collect();

                tx.execute(&update_sql, params_refs.as_slice())?;
            }

            tx.commit()?;

            Ok(messages)
        })
    }

    /// Clear an agent's inbox.
    pub fn clear_inbox(&self, agent_id: &str) -> Result<i32> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM inbox WHERE agent_id = ?1",
                params![agent_id],
            )?;

            Ok(deleted as i32)
        })
    }

    /// Get unread message count for an agent.
    #[allow(dead_code)]
    pub fn get_unread_count(&self, agent_id: &str) -> Result<i32> {
        self.with_conn(|conn| {
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM inbox WHERE agent_id = ?1 AND read = 0",
                params![agent_id],
                |row| row.get(0),
            )?;

            Ok(count)
        })
    }
}
