//! CLI command definitions for task-graph-mcp
//!
//! This module defines the CLI structure using clap's derive macros.
//! The main entry point is the `Cli` struct which contains subcommands.

pub mod diff;
pub mod export;
pub mod import;

use clap::{Parser, Subcommand, ValueEnum};
use diff::DiffArgs;
use export::ExportArgs;
use import::ImportArgs;

/// UI mode for the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum UiMode {
    /// No UI, MCP server only (default)
    #[default]
    None,
    /// Enable web dashboard UI
    Web,
}

/// Default port for the web dashboard.
pub const DEFAULT_UI_PORT: u16 = 31994;

/// Task Graph MCP Server and CLI tools
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Path to database file (overrides config)
    #[arg(short, long, global = true)]
    pub database: Option<String>,

    /// Path to media directory (overrides config)
    #[arg(short, long, global = true)]
    pub media_dir: Option<String>,

    /// Path to log directory (overrides config)
    #[arg(long, global = true)]
    pub log_dir: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Logging output: 0/off, 1/stdout, 2/stderr (default), or filename
    #[arg(short, long, default_value = "2", global = true)]
    pub log: String,

    /// UI mode: none (MCP only) or web (enable dashboard)
    #[arg(long, value_enum, global = true)]
    pub ui: Option<UiMode>,

    /// Port for the web dashboard (default: 31994)
    #[arg(long, global = true)]
    pub ui_port: Option<u16>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the MCP server (default if no subcommand given)
    Serve,

    /// Export task database to structured JSON format
    Export(ExportArgs),

    /// Import task data from a structured JSON export file
    Import(ImportArgs),

    /// Compare snapshot files or snapshot against database
    Diff(DiffArgs),
}
