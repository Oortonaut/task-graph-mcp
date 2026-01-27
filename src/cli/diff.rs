//! Diff subcommand for task-graph CLI
//!
//! Compares snapshot files against the database or each other.

use clap::Args;
use std::path::PathBuf;

/// Arguments for the diff subcommand
#[derive(Args, Debug)]
pub struct DiffArgs {
    /// First snapshot file (or database if comparing two snapshots)
    #[arg(value_name = "FILE")]
    pub source: PathBuf,

    /// Second snapshot file (optional, compares source against database if not provided)
    #[arg(value_name = "FILE")]
    pub target: Option<PathBuf>,

    /// Output format: text (default), json, or summary
    #[arg(short, long, default_value = "text", value_name = "FORMAT")]
    pub format: DiffFormat,

    /// Only show changes for specific tables (comma-separated)
    #[arg(long, value_name = "LIST", value_delimiter = ',')]
    pub tables: Option<Vec<String>>,

    /// Show only summary counts, not individual changes
    #[arg(long)]
    pub summary_only: bool,

    /// Include unchanged tables in output (useful for verification)
    #[arg(long)]
    pub include_unchanged: bool,
}

/// Output format for diff results
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiffFormat {
    #[default]
    Text,
    Json,
    Summary,
}

impl std::str::FromStr for DiffFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(DiffFormat::Text),
            "json" => Ok(DiffFormat::Json),
            "summary" => Ok(DiffFormat::Summary),
            _ => Err(format!(
                "Invalid format '{}'. Valid options: text, json, summary",
                s
            )),
        }
    }
}

impl std::fmt::Display for DiffFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffFormat::Text => write!(f, "text"),
            DiffFormat::Json => write!(f, "json"),
            DiffFormat::Summary => write!(f, "summary"),
        }
    }
}

impl DiffArgs {
    /// Check if we're comparing two snapshots or snapshot vs database
    pub fn is_two_file_diff(&self) -> bool {
        self.target.is_some()
    }

    /// Filter diff tables if --tables is specified
    pub fn should_include_table(&self, table_name: &str) -> bool {
        match &self.tables {
            Some(tables) => tables.iter().any(|t| t == table_name),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_format_parse() {
        assert_eq!("text".parse::<DiffFormat>().unwrap(), DiffFormat::Text);
        assert_eq!("json".parse::<DiffFormat>().unwrap(), DiffFormat::Json);
        assert_eq!("summary".parse::<DiffFormat>().unwrap(), DiffFormat::Summary);
        assert_eq!("JSON".parse::<DiffFormat>().unwrap(), DiffFormat::Json);
        assert!("invalid".parse::<DiffFormat>().is_err());
    }

    #[test]
    fn test_diff_args_table_filter() {
        let args = DiffArgs {
            source: PathBuf::from("test.json"),
            target: None,
            format: DiffFormat::Text,
            tables: Some(vec!["tasks".to_string(), "dependencies".to_string()]),
            summary_only: false,
            include_unchanged: false,
        };

        assert!(args.should_include_table("tasks"));
        assert!(args.should_include_table("dependencies"));
        assert!(!args.should_include_table("attachments"));
    }

    #[test]
    fn test_diff_args_no_filter() {
        let args = DiffArgs {
            source: PathBuf::from("test.json"),
            target: None,
            format: DiffFormat::Text,
            tables: None,
            summary_only: false,
            include_unchanged: false,
        };

        assert!(args.should_include_table("tasks"));
        assert!(args.should_include_table("attachments"));
    }
}
