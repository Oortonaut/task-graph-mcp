//! Attachment storage operations.

use super::{now_ms, Database};
use crate::types::{Attachment, AttachmentMeta};
use anyhow::{anyhow, Result};
use rusqlite::params;
use uuid::Uuid;

impl Database {
    /// Add an attachment to a task.
    pub fn add_attachment(
        &self,
        task_id: Uuid,
        name: String,
        content: String,
        mime_type: Option<String>,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let now = now_ms();
        let mime_type = mime_type.unwrap_or_else(|| "text/plain".to_string());

        self.with_conn(|conn| {
            // Verify task exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM tasks WHERE id = ?1",
                    params![task_id.to_string()],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if !exists {
                return Err(anyhow!("Task not found"));
            }

            conn.execute(
                "INSERT INTO attachments (id, task_id, name, mime_type, content, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.to_string(),
                    task_id.to_string(),
                    name,
                    mime_type,
                    content,
                    now,
                ],
            )?;

            Ok(id)
        })
    }

    /// Get attachments for a task (metadata only).
    pub fn get_attachments(&self, task_id: Uuid) -> Result<Vec<AttachmentMeta>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, name, mime_type, created_at
                 FROM attachments WHERE task_id = ?1 ORDER BY created_at",
            )?;

            let attachments = stmt
                .query_map(params![task_id.to_string()], |row| {
                    let id: String = row.get(0)?;
                    let task_id: String = row.get(1)?;
                    let name: String = row.get(2)?;
                    let mime_type: String = row.get(3)?;
                    let created_at: i64 = row.get(4)?;

                    Ok(AttachmentMeta {
                        id: Uuid::parse_str(&id).unwrap(),
                        task_id: Uuid::parse_str(&task_id).unwrap(),
                        name,
                        mime_type,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(attachments)
        })
    }

    /// Get a full attachment with content.
    pub fn get_attachment(&self, attachment_id: Uuid) -> Result<Option<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, name, mime_type, content, created_at
                 FROM attachments WHERE id = ?1",
            )?;

            let result = stmt.query_row(params![attachment_id.to_string()], |row| {
                let id: String = row.get(0)?;
                let task_id: String = row.get(1)?;
                let name: String = row.get(2)?;
                let mime_type: String = row.get(3)?;
                let content: String = row.get(4)?;
                let created_at: i64 = row.get(5)?;

                Ok(Attachment {
                    id: Uuid::parse_str(&id).unwrap(),
                    task_id: Uuid::parse_str(&task_id).unwrap(),
                    name,
                    mime_type,
                    content,
                    created_at,
                })
            });

            match result {
                Ok(attachment) => Ok(Some(attachment)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Delete an attachment.
    pub fn delete_attachment(&self, attachment_id: Uuid) -> Result<bool> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM attachments WHERE id = ?1",
                params![attachment_id.to_string()],
            )?;

            Ok(deleted > 0)
        })
    }
}
