//! Attachment storage operations.

use super::{now_ms, Database};
use crate::types::{Attachment, AttachmentMeta};
use anyhow::{anyhow, Result};
use rusqlite::params;

impl Database {
    /// Add an attachment to a task with auto-increment order_index.
    /// Returns the order_index of the new attachment.
    /// If file_path is provided, content should be empty (stored externally).
    pub fn add_attachment(
        &self,
        task_id: &str,
        name: String,
        content: String,
        mime_type: Option<String>,
        file_path: Option<String>,
    ) -> Result<i32> {
        let now = now_ms();
        let mime_type = mime_type.unwrap_or_else(|| "text/plain".to_string());

        self.with_conn(|conn| {
            // Verify task exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM tasks WHERE id = ?1",
                    params![task_id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if !exists {
                return Err(anyhow!("Task not found"));
            }

            // Get next order_index for this task
            let max_order: Option<i32> = conn.query_row(
                "SELECT MAX(order_index) FROM attachments WHERE task_id = ?1",
                params![task_id],
                |row| row.get(0),
            )?;
            let order_index = max_order.unwrap_or(-1) + 1;

            conn.execute(
                "INSERT INTO attachments (task_id, order_index, name, mime_type, content, file_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    task_id,
                    order_index,
                    name,
                    mime_type,
                    content,
                    file_path,
                    now,
                ],
            )?;

            Ok(order_index)
        })
    }

    /// Get attachments for a task, optionally including content.
    /// Note: For file-based attachments, content is NOT loaded here - use get_attachment for that.
    pub fn get_attachments_full(&self, task_id: &str, include_content: bool) -> Result<Vec<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, order_index, name, mime_type, content, file_path, created_at
                 FROM attachments WHERE task_id = ?1 ORDER BY order_index, created_at",
            )?;

            let attachments = stmt
                .query_map(params![task_id], |row| {
                    let task_id: String = row.get(0)?;
                    let order_index: i32 = row.get(1)?;
                    let name: String = row.get(2)?;
                    let mime_type: String = row.get(3)?;
                    let content: String = row.get(4)?;
                    let file_path: Option<String> = row.get(5)?;
                    let created_at: i64 = row.get(6)?;

                    Ok(Attachment {
                        task_id,
                        order_index,
                        name,
                        mime_type,
                        content: if include_content { content } else { String::new() },
                        file_path,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(attachments)
        })
    }

    /// Get attachments for a task (metadata only).
    pub fn get_attachments(&self, task_id: &str) -> Result<Vec<AttachmentMeta>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, order_index, name, mime_type, file_path, created_at
                 FROM attachments WHERE task_id = ?1 ORDER BY order_index, created_at",
            )?;

            let attachments = stmt
                .query_map(params![task_id], |row| {
                    let task_id: String = row.get(0)?;
                    let order_index: i32 = row.get(1)?;
                    let name: String = row.get(2)?;
                    let mime_type: String = row.get(3)?;
                    let file_path: Option<String> = row.get(4)?;
                    let created_at: i64 = row.get(5)?;

                    Ok(AttachmentMeta {
                        task_id,
                        order_index,
                        name,
                        mime_type,
                        file_path,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(attachments)
        })
    }

    /// Get a full attachment by (task_id, order_index).
    /// Note: For file-based attachments, content field contains the DB content (empty).
    /// The caller should read from file_path if set.
    pub fn get_attachment(&self, task_id: &str, order_index: i32) -> Result<Option<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, order_index, name, mime_type, content, file_path, created_at
                 FROM attachments WHERE task_id = ?1 AND order_index = ?2",
            )?;

            let result = stmt.query_row(params![task_id, order_index], |row| {
                let task_id: String = row.get(0)?;
                let order_index: i32 = row.get(1)?;
                let name: String = row.get(2)?;
                let mime_type: String = row.get(3)?;
                let content: String = row.get(4)?;
                let file_path: Option<String> = row.get(5)?;
                let created_at: i64 = row.get(6)?;

                Ok(Attachment {
                    task_id,
                    order_index,
                    name,
                    mime_type,
                    content,
                    file_path,
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

    /// Get just the file_path for an attachment (useful before deletion).
    pub fn get_attachment_file_path(&self, task_id: &str, order_index: i32) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let result = conn.query_row(
                "SELECT file_path FROM attachments WHERE task_id = ?1 AND order_index = ?2",
                params![task_id, order_index],
                |row| row.get::<_, Option<String>>(0),
            );

            match result {
                Ok(file_path) => Ok(file_path),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Delete an attachment by (task_id, order_index).
    pub fn delete_attachment(&self, task_id: &str, order_index: i32) -> Result<bool> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND order_index = ?2",
                params![task_id, order_index],
            )?;

            Ok(deleted > 0)
        })
    }
}
