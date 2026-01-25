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

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            // Verify task exists
            let exists: bool = tx
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
            let max_order: Option<i32> = tx.query_row(
                "SELECT MAX(order_index) FROM attachments WHERE task_id = ?1",
                params![task_id],
                |row| row.get(0),
            )?;
            let order_index = max_order.unwrap_or(-1) + 1;

            tx.execute(
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

            tx.commit()?;
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
        self.get_attachments_filtered(task_id, None, None)
    }


    /// Get attachments for a task with optional filtering (metadata only).
    /// - name_pattern: Optional glob pattern (with * wildcard) to filter by attachment name
    /// - mime_pattern: Optional prefix to filter by MIME type (e.g., "image/" matches "image/png")
    pub fn get_attachments_filtered(
        &self,
        task_id: &str,
        name_pattern: Option<&str>,
        mime_pattern: Option<&str>,
    ) -> Result<Vec<AttachmentMeta>> {
        self.with_conn(|conn| {
            // Build query with optional filters
            let mut sql = String::from(
                "SELECT task_id, order_index, name, mime_type, file_path, created_at
                 FROM attachments WHERE task_id = ?1"
            );

            // For name pattern, convert glob to SQL LIKE pattern
            let name_like = name_pattern.map(|p| {
                // Convert glob wildcards to SQL LIKE: * -> %, ? -> _
                p.replace('*', "%").replace('?', "_")
            });

            if name_like.is_some() {
                sql.push_str(" AND name LIKE ?2 ESCAPE '\\'");
            }

            if mime_pattern.is_some() {
                let idx = if name_like.is_some() { 3 } else { 2 };
                sql.push_str(&format!(" AND mime_type LIKE ?{} ESCAPE '\\\\'", idx));
            }

            sql.push_str(" ORDER BY order_index, created_at");

            let mut stmt = conn.prepare(&sql)?;

            // Bind parameters based on which filters are present
            let attachments: Vec<AttachmentMeta> = match (&name_like, mime_pattern) {
                (Some(name), Some(mime)) => {
                    let mime_like = format!("{}%", mime);
                    stmt.query_map(params![task_id, name, mime_like], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
                (Some(name), None) => {
                    stmt.query_map(params![task_id, name], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
                (None, Some(mime)) => {
                    let mime_like = format!("{}%", mime);
                    stmt.query_map(params![task_id, mime_like], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
                (None, None) => {
                    stmt.query_map(params![task_id], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
            };

            Ok(attachments)
        })
    }

    /// Helper to map a row to AttachmentMeta.
    fn map_attachment_meta(row: &rusqlite::Row) -> rusqlite::Result<AttachmentMeta> {
        Ok(AttachmentMeta {
            task_id: row.get(0)?,
            order_index: row.get(1)?,
            name: row.get(2)?,
            mime_type: row.get(3)?,
            file_path: row.get(4)?,
            created_at: row.get(5)?,
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


    /// Delete an attachment by name (for replace behavior).
    /// Returns the file_path if one was set (for cleanup).
    pub fn delete_attachment_by_name(&self, task_id: &str, name: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            // First get the file_path if any
            let file_path: Option<String> = conn
                .query_row(
                    "SELECT file_path FROM attachments WHERE task_id = ?1 AND name = ?2",
                    params![task_id, name],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            // Delete the attachment
            conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND name = ?2",
                params![task_id, name],
            )?;

            Ok(file_path)
        })
    }

    /// Delete an attachment by name and return whether it was deleted plus the file_path.
    /// Returns (was_deleted, file_path).
    pub fn delete_attachment_by_name_ex(&self, task_id: &str, name: &str) -> Result<(bool, Option<String>)> {
        self.with_conn(|conn| {
            // First get the file_path if any
            let file_path: Option<String> = conn
                .query_row(
                    "SELECT file_path FROM attachments WHERE task_id = ?1 AND name = ?2",
                    params![task_id, name],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            // Delete the attachment
            let rows_affected = conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND name = ?2",
                params![task_id, name],
            )?;

            Ok((rows_affected > 0, file_path))
        })
    }
}
