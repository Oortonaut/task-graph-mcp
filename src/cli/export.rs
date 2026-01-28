//! Export subcommand for task-graph CLI
//!
//! Exports the task database to a structured JSON format that can be
//! version-controlled, diffed, and re-imported.

use clap::Args;
use std::path::PathBuf;

/// Arguments for the export subcommand
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Output file path (default: stdout)
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Force gzip compression (auto-detected from .gz extension otherwise)
    #[arg(long)]
    pub gzip: bool,

    /// Comma-separated list of tables to export
    ///
    /// Available tables: tasks, dependencies, attachments, task_tags,
    /// task_needed_tags, task_wanted_tags, task_sequence
    #[arg(long, value_name = "LIST", value_delimiter = ',')]
    pub tables: Option<Vec<String>>,

    /// Exclude task_sequence table (audit history)
    #[arg(long)]
    pub no_history: bool,

    /// Filter out soft-deleted tasks (where deleted_at is set)
    #[arg(long)]
    pub exclude_deleted: bool,

    /// Automatically compress if output exceeds this size
    ///
    /// Accepts human-readable sizes: 100KB, 1MB, etc.
    /// If the uncompressed output exceeds this threshold, the output
    /// will be gzip compressed (and .gz appended to filename if needed).
    #[arg(long, value_name = "SIZE")]
    pub compress_threshold: Option<String>,
}

impl ExportArgs {
    /// Get the list of tables to export, or None for all tables
    pub fn tables_to_export(&self) -> Option<Vec<String>> {
        if self.no_history {
            // If --no-history is set but --tables is also set, filter out history
            if let Some(ref tables) = self.tables {
                Some(
                    tables
                        .iter()
                        .filter(|t| *t != "task_sequence")
                        .cloned()
                        .collect(),
                )
            } else {
                // Return all tables except task_sequence
                Some(vec![
                    "tasks".to_string(),
                    "dependencies".to_string(),
                    "attachments".to_string(),
                    "task_tags".to_string(),
                    "task_needed_tags".to_string(),
                    "task_wanted_tags".to_string(),
                ])
            }
        } else {
            self.tables.clone()
        }
    }

    /// Parse the compress threshold into bytes
    pub fn compress_threshold_bytes(&self) -> Option<u64> {
        self.compress_threshold.as_ref().and_then(|s| parse_size(s))
    }

    /// Determine if output should be compressed based on args and filename
    pub fn should_compress(&self, output_size: Option<u64>) -> bool {
        // Explicit --gzip flag always wins
        if self.gzip {
            return true;
        }

        // Check if output filename ends with .gz
        if let Some(ref path) = self.output
            && path.extension().is_some_and(|ext| ext == "gz")
        {
            return true;
        }

        // Check against threshold if provided
        if let (Some(threshold), Some(size)) = (self.compress_threshold_bytes(), output_size) {
            return size > threshold;
        }

        false
    }
}

/// Parse a human-readable size string into bytes
///
/// Supports: B, KB, MB, GB (case-insensitive)
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();

    if let Some(num) = s.strip_suffix("GB") {
        num.trim()
            .parse::<u64>()
            .ok()
            .map(|n| n * 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("MB") {
        num.trim().parse::<u64>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("KB") {
        num.trim().parse::<u64>().ok().map(|n| n * 1024)
    } else if let Some(num) = s.strip_suffix('B') {
        num.trim().parse::<u64>().ok()
    } else {
        // Try parsing as plain number (bytes)
        s.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("100"), Some(100));
        assert_eq!(parse_size("100B"), Some(100));
        assert_eq!(parse_size("100KB"), Some(100 * 1024));
        assert_eq!(parse_size("100kb"), Some(100 * 1024));
        assert_eq!(parse_size("1MB"), Some(1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("invalid"), None);
    }

    #[test]
    fn test_tables_to_export_no_history() {
        let args = ExportArgs {
            output: None,
            gzip: false,
            tables: None,
            no_history: true,
            exclude_deleted: false,
            compress_threshold: None,
        };

        let tables = args.tables_to_export().unwrap();
        assert!(!tables.contains(&"task_sequence".to_string()));
        assert!(tables.contains(&"tasks".to_string()));
    }

    #[test]
    fn test_should_compress() {
        // Test explicit gzip flag
        let args = ExportArgs {
            output: None,
            gzip: true,
            tables: None,
            no_history: false,
            exclude_deleted: false,
            compress_threshold: None,
        };
        assert!(args.should_compress(None));

        // Test .gz extension detection
        let args = ExportArgs {
            output: Some(PathBuf::from("snapshot.json.gz")),
            gzip: false,
            tables: None,
            no_history: false,
            exclude_deleted: false,
            compress_threshold: None,
        };
        assert!(args.should_compress(None));

        // Test threshold
        let args = ExportArgs {
            output: None,
            gzip: false,
            tables: None,
            no_history: false,
            exclude_deleted: false,
            compress_threshold: Some("100KB".to_string()),
        };
        assert!(!args.should_compress(Some(50 * 1024))); // Under threshold
        assert!(args.should_compress(Some(150 * 1024))); // Over threshold
    }
}
