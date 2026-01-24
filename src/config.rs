//! Configuration loading and management.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub paths: PathsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            paths: PathsConfig::default(),
        }
    }
}

/// Server-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Path to the SQLite database file.
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,

    /// Maximum claims per agent.
    #[serde(default = "default_claim_limit")]
    pub claim_limit: i32,

    /// Timeout for stale claims in seconds.
    #[serde(default = "default_stale_timeout")]
    pub stale_timeout_seconds: i64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            claim_limit: default_claim_limit(),
            stale_timeout_seconds: default_stale_timeout(),
        }
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from(".task-graph/tasks.db")
}

fn default_claim_limit() -> i32 {
    5
}

fn default_stale_timeout() -> i64 {
    900 // 15 minutes
}

/// Path handling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Style for representing file paths.
    #[serde(default)]
    pub style: PathStyle,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            style: PathStyle::Relative,
        }
    }
}

/// Path style for file locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathStyle {
    /// Relative paths (e.g., src/main.rs)
    Relative,
    /// Project-prefixed paths (e.g., ${project}/src/main.rs)
    ProjectPrefixed,
}

impl Default for PathStyle {
    fn default() -> Self {
        PathStyle::Relative
    }
}

impl Config {
    /// Load configuration from file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from default locations or return defaults.
    pub fn load_or_default() -> Self {
        // Try .task-graph/config.yaml
        if let Ok(config) = Self::load(".task-graph/config.yaml") {
            return config;
        }

        // Try environment variables
        let mut config = Self::default();

        if let Ok(db_path) = std::env::var("TASK_GRAPH_DB_PATH") {
            config.server.db_path = PathBuf::from(db_path);
        }

        if let Ok(limit) = std::env::var("TASK_GRAPH_CLAIM_LIMIT") {
            if let Ok(limit) = limit.parse() {
                config.server.claim_limit = limit;
            }
        }

        if let Ok(timeout) = std::env::var("TASK_GRAPH_STALE_TIMEOUT") {
            if let Ok(timeout) = timeout.parse() {
                config.server.stale_timeout_seconds = timeout;
            }
        }

        config
    }

    /// Ensure the database directory exists.
    pub fn ensure_db_dir(&self) -> Result<()> {
        if let Some(parent) = self.server.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}
