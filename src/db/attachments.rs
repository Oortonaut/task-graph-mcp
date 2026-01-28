//! Attachment storage operations.

use super::{Database, now_ms};
use crate::types::{Attachment, AttachmentMeta};
use anyhow::{Result, anyhow};
use rusqlite::params;

impl Database {
    /// Add an attachment to a task with auto-increment sequence per type.
    /// Returns the sequence number of the new attachment.
    /// If file_path is provided, content should be empty (stored externally).
    pub fn add_attachment(
        &self,
        task_id: &str,
        attachment_type: String,
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

            // Get next sequence for this (task_id, attachment_type)
            let max_seq: Option<i32> = tx.query_row(
                "SELECT MAX(sequence) FROM attachments WHERE task_id = ?1 AND attachment_type = ?2",
                params![task_id, attachment_type],
                |row| row.get(0),
            )?;
            let sequence = max_seq.unwrap_or(-1) + 1;

            tx.execute(
                "INSERT INTO attachments (task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    task_id,
                    attachment_type,
                    sequence,
                    name,
                    mime_type,
                    content,
                    file_path,
                    now,
                ],
            )?;

            tx.commit()?;
            Ok(sequence)
        })
    }

    /// Get attachments for a task, optionally including content.
    /// Note: For file-based attachments, content is NOT loaded here - use get_attachment for that.
    pub fn get_attachments_full(
        &self,
        task_id: &str,
        include_content: bool,
    ) -> Result<Vec<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at
                 FROM attachments WHERE task_id = ?1 ORDER BY attachment_type, sequence",
            )?;

            let attachments = stmt
                .query_map(params![task_id], |row| {
                    let task_id: String = row.get(0)?;
                    let attachment_type: String = row.get(1)?;
                    let sequence: i32 = row.get(2)?;
                    let name: String = row.get(3)?;
                    let mime_type: String = row.get(4)?;
                    let content: String = row.get(5)?;
                    let file_path: Option<String> = row.get(6)?;
                    let created_at: i64 = row.get(7)?;

                    Ok(Attachment {
                        task_id,
                        attachment_type,
                        sequence,
                        name,
                        mime_type,
                        content: if include_content {
                            content
                        } else {
                            String::new()
                        },
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
    /// - type_pattern: Optional glob pattern (with * wildcard) to filter by attachment_type
    /// - mime_pattern: Optional prefix to filter by MIME type (e.g., "image/" matches "image/png")
    pub fn get_attachments_filtered(
        &self,
        task_id: &str,
        type_pattern: Option<&str>,
        mime_pattern: Option<&str>,
    ) -> Result<Vec<AttachmentMeta>> {
        self.with_conn(|conn| {
            // Build query with optional filters
            let mut sql = String::from(
                "SELECT task_id, attachment_type, sequence, name, mime_type, file_path, created_at
                 FROM attachments WHERE task_id = ?1",
            );

            // For type pattern, convert glob to SQL LIKE pattern
            let type_like = type_pattern.map(|p| {
                // Convert glob wildcards to SQL LIKE: * -> %, ? -> _
                p.replace('*', "%").replace('?', "_")
            });

            if type_like.is_some() {
                sql.push_str(" AND attachment_type LIKE ?2 ESCAPE '\\'");
            }

            if mime_pattern.is_some() {
                let idx = if type_like.is_some() { 3 } else { 2 };
                sql.push_str(&format!(" AND mime_type LIKE ?{} ESCAPE '\\'", idx));
            }

            sql.push_str(" ORDER BY attachment_type, sequence");

            let mut stmt = conn.prepare(&sql)?;

            // Bind parameters based on which filters are present
            let attachments: Vec<AttachmentMeta> = match (&type_like, mime_pattern) {
                (Some(type_pat), Some(mime)) => {
                    let mime_like = format!("{}%", mime);
                    stmt.query_map(params![task_id, type_pat, mime_like], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
                (Some(type_pat), None) => stmt
                    .query_map(params![task_id, type_pat], Self::map_attachment_meta)?
                    .filter_map(|r| r.ok())
                    .collect(),
                (None, Some(mime)) => {
                    let mime_like = format!("{}%", mime);
                    stmt.query_map(params![task_id, mime_like], |row| {
                        Self::map_attachment_meta(row)
                    })?
                    .filter_map(|r| r.ok())
                    .collect()
                }
                (None, None) => stmt
                    .query_map(params![task_id], Self::map_attachment_meta)?
                    .filter_map(|r| r.ok())
                    .collect(),
            };

            Ok(attachments)
        })
    }

    /// Helper to map a row to AttachmentMeta.
    fn map_attachment_meta(row: &rusqlite::Row) -> rusqlite::Result<AttachmentMeta> {
        Ok(AttachmentMeta {
            task_id: row.get(0)?,
            attachment_type: row.get(1)?,
            sequence: row.get(2)?,
            name: row.get(3)?,
            mime_type: row.get(4)?,
            file_path: row.get(5)?,
            created_at: row.get(6)?,
        })
    }

    /// Get a full attachment by (task_id, attachment_type, sequence).
    /// Note: For file-based attachments, content field contains the DB content (empty).
    /// The caller should read from file_path if set.
    pub fn get_attachment(
        &self,
        task_id: &str,
        attachment_type: &str,
        sequence: i32,
    ) -> Result<Option<Attachment>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT task_id, attachment_type, sequence, name, mime_type, content, file_path, created_at
                 FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND sequence = ?3",
            )?;

            let result = stmt.query_row(params![task_id, attachment_type, sequence], |row| {
                let task_id: String = row.get(0)?;
                let attachment_type: String = row.get(1)?;
                let sequence: i32 = row.get(2)?;
                let name: String = row.get(3)?;
                let mime_type: String = row.get(4)?;
                let content: String = row.get(5)?;
                let file_path: Option<String> = row.get(6)?;
                let created_at: i64 = row.get(7)?;

                Ok(Attachment {
                    task_id,
                    attachment_type,
                    sequence,
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

    /// Get file_paths for all attachments of a given type (useful before deletion).
    pub fn get_attachment_file_paths_by_type(
        &self,
        task_id: &str,
        attachment_type: &str,
    ) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT file_path FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND file_path IS NOT NULL",
            )?;

            let paths: Vec<String> = stmt
                .query_map(params![task_id, attachment_type], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(paths)
        })
    }

    /// Delete an attachment by (task_id, attachment_type, sequence).
    pub fn delete_attachment(
        &self,
        task_id: &str,
        attachment_type: &str,
        sequence: i32,
    ) -> Result<bool> {
        self.with_conn(|conn| {
            let deleted = conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND sequence = ?3",
                params![task_id, attachment_type, sequence],
            )?;

            Ok(deleted > 0)
        })
    }

    /// Delete all attachments of a given type (for replace behavior).
    /// Returns the file_paths of deleted attachments (for cleanup).
    pub fn delete_attachments_by_type(
        &self,
        task_id: &str,
        attachment_type: &str,
    ) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            // First get all file_paths
            let file_paths = {
                let mut stmt = conn.prepare(
                    "SELECT file_path FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND file_path IS NOT NULL",
                )?;
                stmt.query_map(params![task_id, attachment_type], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<String>>()
            };

            // Delete all attachments of this type
            conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND attachment_type = ?2",
                params![task_id, attachment_type],
            )?;

            Ok(file_paths)
        })
    }

    /// Delete all attachments of a given type and return count plus file_paths.
    /// Returns (deleted_count, file_paths).
    pub fn delete_attachments_by_type_ex(
        &self,
        task_id: &str,
        attachment_type: &str,
    ) -> Result<(usize, Vec<String>)> {
        self.with_conn(|conn| {
            // First get all file_paths
            let file_paths = {
                let mut stmt = conn.prepare(
                    "SELECT file_path FROM attachments WHERE task_id = ?1 AND attachment_type = ?2 AND file_path IS NOT NULL",
                )?;
                stmt.query_map(params![task_id, attachment_type], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<String>>()
            };

            // Delete all attachments of this type
            let deleted = conn.execute(
                "DELETE FROM attachments WHERE task_id = ?1 AND attachment_type = ?2",
                params![task_id, attachment_type],
            )?;

            Ok((deleted, file_paths))
        })
    }
}
