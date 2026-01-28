//! Import subcommand for task-graph CLI
//!
//! Imports task data from a structured JSON export file back into
//! the database.

use clap::Args;
use std::path::PathBuf;

/// Arguments for the import subcommand
#[derive(Args, Debug)]
pub struct ImportArgs {
    /// Path to the export file to import
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Validate import without modifying database
    ///
    /// Parses the file, validates schema compatibility, and reports
    /// what would be imported without making any changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Merge mode: add missing items, skip existing
    ///
    /// By default, import replaces all project data. With --merge:
    /// - Tasks: skip if ID already exists, insert if new
    /// - Dependencies: skip if exact match exists
    /// - Attachments: configurable via --attachment-mode
    #[arg(long)]
    pub merge: bool,

    /// Force overwrite of existing data without prompting
    ///
    /// In replace mode (default): skip confirmation prompt
    /// In merge mode: overwrite conflicts instead of skipping
    #[arg(long)]
    pub force: bool,

    /// Enable strict validation mode
    ///
    /// Rejects imports with:
    /// - Circular dependencies (normally just warned)
    /// - Missing referenced tasks
    /// - Invalid status values
    #[arg(long)]
    pub strict: bool,
}

impl ImportArgs {
    /// Check if this is a gzipped file based on extension
    pub fn is_gzipped(&self) -> bool {
        self.file.extension().is_some_and(|ext| ext == "gz")
    }

    /// Describe the import mode for logging
    pub fn import_mode(&self) -> &'static str {
        if self.dry_run {
            "dry-run"
        } else if self.merge {
            if self.force {
                "merge-overwrite"
            } else {
                "merge-skip"
            }
        } else {
            "replace"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_gzipped() {
        let args = ImportArgs {
            file: PathBuf::from("snapshot.json"),
            dry_run: false,
            merge: false,
            force: false,
            strict: false,
        };
        assert!(!args.is_gzipped());

        let args = ImportArgs {
            file: PathBuf::from("snapshot.json.gz"),
            dry_run: false,
            merge: false,
            force: false,
            strict: false,
        };
        assert!(args.is_gzipped());
    }

    #[test]
    fn test_import_mode() {
        // Dry run
        let args = ImportArgs {
            file: PathBuf::from("test.json"),
            dry_run: true,
            merge: false,
            force: false,
            strict: false,
        };
        assert_eq!(args.import_mode(), "dry-run");

        // Replace (default)
        let args = ImportArgs {
            file: PathBuf::from("test.json"),
            dry_run: false,
            merge: false,
            force: false,
            strict: false,
        };
        assert_eq!(args.import_mode(), "replace");

        // Merge skip
        let args = ImportArgs {
            file: PathBuf::from("test.json"),
            dry_run: false,
            merge: true,
            force: false,
            strict: false,
        };
        assert_eq!(args.import_mode(), "merge-skip");

        // Merge overwrite
        let args = ImportArgs {
            file: PathBuf::from("test.json"),
            dry_run: false,
            merge: true,
            force: true,
            strict: false,
        };
        assert_eq!(args.import_mode(), "merge-overwrite");
    }
}
