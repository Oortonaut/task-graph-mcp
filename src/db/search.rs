//! Full-text search operations using FTS5.

use super::Database;
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// A search result from full-text search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Task ID
    pub task_id: String,
    /// Task title
    pub title: String,
    /// Task description
    pub description: Option<String>,
    /// Task status
    pub status: String,
    /// BM25 relevance score (lower is more relevant)
    pub score: f64,
    /// Highlighted snippet from title
    pub title_snippet: String,
    /// Highlighted snippet from description
    pub description_snippet: Option<String>,
    /// Attachment matches if include_attachments is true
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachment_matches: Vec<AttachmentMatch>,
}

/// A matching attachment from full-text search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMatch {
    /// Attachment type/category
    pub attachment_type: String,
    /// Sequence within type
    pub sequence: i32,
    /// Attachment name/label
    pub name: String,
    /// Highlighted content snippet
    pub content_snippet: String,
}

impl Database {
    /// Search tasks using FTS5 full-text search.
    ///
    /// The query supports FTS5 MATCH syntax:
    /// - Simple words: `error handling`
    /// - Phrases: `"error handling"`
    /// - Prefix: `error*`
    /// - Boolean: `error AND NOT warning`
    /// - Column-specific: `title:error` or `description:handling`
    ///
    /// Results are ranked by BM25 relevance score.
    pub fn search_tasks(
        &self,
        query: &str,
        limit: Option<i32>,
        include_attachments: bool,
        status_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let limit = limit.unwrap_or(20).min(100);

        self.with_conn(|conn| {
            // First, search tasks_fts
            let mut sql = String::from(
                "SELECT
                    fts.task_id,
                    t.title,
                    t.description,
                    t.status,
                    bm25(tasks_fts) as score,
                    snippet(tasks_fts, 1, '<mark>', '</mark>', '...', 32) as title_snippet,
                    snippet(tasks_fts, 2, '<mark>', '</mark>', '...', 64) as description_snippet
                FROM tasks_fts fts
                INNER JOIN tasks t ON fts.task_id = t.id
                WHERE tasks_fts MATCH ?1",
            );

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(query.to_string()));

            if let Some(status) = status_filter {
                sql.push_str(" AND t.status = ?2");
                params_vec.push(Box::new(status.to_string()));
            }

            sql.push_str(" ORDER BY score LIMIT ?");
            params_vec.push(Box::new(limit));

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql)?;
            let mut results: Vec<SearchResult> = stmt
                .query_map(params_refs.as_slice(), |row| {
                    Ok(SearchResult {
                        task_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        status: row.get(3)?,
                        score: row.get(4)?,
                        title_snippet: row.get(5)?,
                        description_snippet: row.get(6)?,
                        attachment_matches: Vec::new(),
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            // If include_attachments, also search attachments_fts
            if include_attachments {
                // Search attachments
                let attachment_sql = "SELECT
                    afts.task_id,
                    afts.attachment_type,
                    afts.sequence,
                    afts.name,
                    snippet(attachments_fts, 4, '<mark>', '</mark>', '...', 64) as content_snippet
                FROM attachments_fts afts
                WHERE attachments_fts MATCH ?1
                ORDER BY bm25(attachments_fts)
                LIMIT ?2";

                let mut att_stmt = conn.prepare(attachment_sql)?;
                let att_matches: Vec<(String, String, i32, String, String)> = att_stmt
                    .query_map(params![query, limit * 3], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i32>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                // Group attachment matches by task_id and merge with task results
                for (task_id, attachment_type, sequence, name, content_snippet) in att_matches {
                    // Check if task already in results
                    if let Some(result) = results.iter_mut().find(|r| r.task_id == task_id) {
                        result.attachment_matches.push(AttachmentMatch {
                            attachment_type,
                            sequence,
                            name,
                            content_snippet,
                        });
                    } else {
                        // Add task to results if not already present (attachment-only match)
                        // Apply status filter if needed
                        let task_sql = if status_filter.is_some() {
                            "SELECT id, title, description, status FROM tasks WHERE id = ?1 AND status = ?2"
                        } else {
                            "SELECT id, title, description, status FROM tasks WHERE id = ?1"
                        };

                        let task_result: Option<(String, String, Option<String>, String)> =
                            if let Some(status) = status_filter {
                                conn.query_row(task_sql, params![&task_id, status], |row| {
                                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                                })
                                .ok()
                            } else {
                                conn.query_row(task_sql, params![&task_id], |row| {
                                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                                })
                                .ok()
                            };

                        if let Some((id, title, description, status)) = task_result {
                            results.push(SearchResult {
                                task_id: id.clone(),
                                title: title.clone(),
                                description: description.clone(),
                                status,
                                score: 999.0, // Attachment-only matches get lower priority
                                title_snippet: title,
                                description_snippet: description,
                                attachment_matches: vec![AttachmentMatch {
                                    attachment_type,
                                    sequence,
                                    name,
                                    content_snippet,
                                }],
                            });
                        }
                    }
                }
            }

            // Sort by score and apply limit
            results.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
            results.truncate(limit as usize);

            Ok(results)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StatesConfig;

    fn states() -> StatesConfig {
        StatesConfig::default()
    }

    #[test]
    fn test_search_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let results = db.search_tasks("test", None, false, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_insert_trigger_indexes_new_tasks() {
        let db = Database::open_in_memory().unwrap();

        // Create a task - trigger should automatically add to FTS
        let task = db
            .create_task(
                None,
                "Test FTS indexing with keywords".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states(),
            )
            .unwrap();

        // Search should find it immediately
        let results = db.search_tasks("indexing", None, false, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, task.id);
    }

    #[test]
    fn test_fts_update_trigger_reindexes_modified_tasks() {
        let db = Database::open_in_memory().unwrap();

        // Create a task with initial content
        let task = db
            .create_task(
                None,
                "Original title original".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states(),
            )
            .unwrap();

        // Verify initial content is indexed
        let results = db.search_tasks("Original", None, false, None).unwrap();
        assert_eq!(results.len(), 1);

        // Update the task - trigger should reindex
        db.update_task(
            &task.id,
            Some("Updated title with newkeyword".to_string()),
            Some(Some("Updated description".to_string())),
            None,
            None,
            None,
            None,
            &states(),
        )
        .unwrap();

        // Search should find new content
        let results = db.search_tasks("newkeyword", None, false, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, task.id);

        // Verify updated title is searchable
        let results = db.search_tasks("Updated", None, false, None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_fts_delete_trigger_removes_from_index() {
        let db = Database::open_in_memory().unwrap();

        // Create a task
        let task = db
            .create_task(
                None,
                "Deletable task content".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states(),
            )
            .unwrap();

        // Verify it's indexed
        let results = db.search_tasks("Deletable", None, false, None).unwrap();
        assert_eq!(results.len(), 1);

        // Delete the task
        db.delete_task(&task.id, "test-worker", false, None, true, true)
            .unwrap();

        // Search should find nothing
        let results = db.search_tasks("Deletable", None, false, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_search_with_bm25_ranking() {
        let db = Database::open_in_memory().unwrap();

        // Create tasks with varying relevance
        db.create_task(
            None,
            "Bug fix for minor bug".to_string(),
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states(),
        )
        .unwrap();
        db.create_task(
            None,
            "Bug bug bug multiple bugs".to_string(),
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states(),
        )
        .unwrap();
        db.create_task(
            None,
            "Feature implementation".to_string(),
            None,
            None, // phase
            None,
            None,
            None,
            None,
            None,
            None,
            &states(),
        )
        .unwrap();

        // Search for "bug" - higher frequency should rank better
        let results = db.search_tasks("bug", None, false, None).unwrap();
        assert_eq!(results.len(), 2);
        // The task with more "bug" occurrences should have a better (lower) score
        assert!(results[0].score <= results[1].score);
    }

    #[test]
    fn test_fts_attachment_trigger_indexes_text_content() {
        let db = Database::open_in_memory().unwrap();

        // Create a task
        let task = db
            .create_task(
                None,
                "Task with attachment".to_string(),
                None,
                None, // phase
                None,
                None,
                None,
                None,
                None,
                None,
                &states(),
            )
            .unwrap();

        // Add a text attachment
        db.add_attachment(
            &task.id,
            "notes".to_string(),
            String::new(),
            "Important searchable content here".to_string(),
            Some("text/plain".to_string()),
            None,
        )
        .unwrap();

        // Search with include_attachments should find it
        let results = db.search_tasks("searchable", None, true, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, task.id);
        assert_eq!(results[0].attachment_matches.len(), 1);
        assert_eq!(results[0].attachment_matches[0].attachment_type, "notes");
    }
}
