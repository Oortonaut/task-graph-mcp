//! Configuration loading and management.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub states: StatesConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            paths: PathsConfig::default(),
            states: StatesConfig::default(),
        }
    }
}

/// Server-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Path to the SQLite database file.
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,

    /// Path to the media directory for file attachments.
    #[serde(default = "default_media_dir")]
    pub media_dir: PathBuf,

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
            media_dir: default_media_dir(),
            claim_limit: default_claim_limit(),
            stale_timeout_seconds: default_stale_timeout(),
        }
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from(".task-graph/tasks.db")
}

fn default_media_dir() -> PathBuf {
    PathBuf::from(".task-graph/media")
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

/// Task state configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatesConfig {
    /// Default state for new tasks.
    #[serde(default = "default_initial_state")]
    pub initial: String,

    /// States that block dependent tasks (tasks in these states count as "not done").
    #[serde(default = "default_blocking_states")]
    pub blocking_states: Vec<String>,

    /// State definitions with allowed transitions and timing behavior.
    #[serde(default = "default_state_definitions")]
    pub definitions: HashMap<String, StateDefinition>,
}

impl Default for StatesConfig {
    fn default() -> Self {
        Self {
            initial: default_initial_state(),
            blocking_states: default_blocking_states(),
            definitions: default_state_definitions(),
        }
    }
}

fn default_initial_state() -> String {
    "pending".to_string()
}

fn default_blocking_states() -> Vec<String> {
    vec!["pending".to_string(), "in_progress".to_string()]
}

fn default_state_definitions() -> HashMap<String, StateDefinition> {
    let mut defs = HashMap::new();

    defs.insert(
        "pending".to_string(),
        StateDefinition {
            exits: vec!["in_progress".to_string(), "cancelled".to_string()],
            timed: false,
        },
    );

    defs.insert(
        "in_progress".to_string(),
        StateDefinition {
            exits: vec![
                "completed".to_string(),
                "failed".to_string(),
                "pending".to_string(),
            ],
            timed: true,
        },
    );

    defs.insert(
        "completed".to_string(),
        StateDefinition {
            exits: vec![],
            timed: false,
        },
    );

    defs.insert(
        "failed".to_string(),
        StateDefinition {
            exits: vec!["pending".to_string()],
            timed: false,
        },
    );

    defs.insert(
        "cancelled".to_string(),
        StateDefinition {
            exits: vec![],
            timed: false,
        },
    );

    defs
}

/// Definition of a single task state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDefinition {
    /// Allowed states to transition to from this state.
    #[serde(default)]
    pub exits: Vec<String>,

    /// Whether time spent in this state should be tracked (accumulated to time_actual_ms).
    #[serde(default)]
    pub timed: bool,
}

impl StatesConfig {
    /// Check if a state is a valid defined state.
    pub fn is_valid_state(&self, state: &str) -> bool {
        self.definitions.contains_key(state)
    }

    /// Check if a transition from one state to another is allowed.
    pub fn is_valid_transition(&self, from: &str, to: &str) -> bool {
        if let Some(def) = self.definitions.get(from) {
            def.exits.contains(&to.to_string())
        } else {
            false
        }
    }

    /// Check if a state is timed (accumulates duration).
    pub fn is_timed_state(&self, state: &str) -> bool {
        self.definitions
            .get(state)
            .map(|d| d.timed)
            .unwrap_or(false)
    }

    /// Check if a state is terminal (has no exits).
    pub fn is_terminal_state(&self, state: &str) -> bool {
        self.definitions
            .get(state)
            .map(|d| d.exits.is_empty())
            .unwrap_or(false)
    }

    /// Check if a state is a blocking state (blocks dependents).
    pub fn is_blocking_state(&self, state: &str) -> bool {
        self.blocking_states.contains(&state.to_string())
    }

    /// Get all defined state names.
    pub fn state_names(&self) -> Vec<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    /// Get allowed exit states for a given state.
    pub fn get_exits(&self, state: &str) -> Vec<&str> {
        self.definitions
            .get(state)
            .map(|d| d.exits.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Validate the states configuration.
    pub fn validate(&self) -> Result<()> {
        // Check initial state exists
        if !self.definitions.contains_key(&self.initial) {
            return Err(anyhow!(
                "Initial state '{}' is not defined in state definitions",
                self.initial
            ));
        }

        // Check all blocking_states exist
        for state in &self.blocking_states {
            if !self.definitions.contains_key(state) {
                return Err(anyhow!(
                    "Blocking state '{}' is not defined in state definitions",
                    state
                ));
            }
        }

        // Check all exit targets exist
        for (state_name, def) in &self.definitions {
            for exit in &def.exits {
                if !self.definitions.contains_key(exit) {
                    return Err(anyhow!(
                        "State '{}' has exit '{}' which is not defined",
                        state_name,
                        exit
                    ));
                }
            }
        }

        // Check at least one terminal state exists
        let has_terminal = self.definitions.values().any(|d| d.exits.is_empty());
        if !has_terminal {
            return Err(anyhow!(
                "At least one terminal state (with empty exits) must be defined"
            ));
        }

        Ok(())
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

        if let Ok(media_dir) = std::env::var("TASK_GRAPH_MEDIA_DIR") {
            config.server.media_dir = PathBuf::from(media_dir);
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

    /// Ensure the media directory exists.
    pub fn ensure_media_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.server.media_dir)?;
        Ok(())
    }

    /// Get the media directory path.
    pub fn media_dir(&self) -> &Path {
        &self.server.media_dir
    }
}

/// Tool description override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPrompt {
    pub description: String,
}

/// LLM-facing prompts configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Prompts {
    /// Server instructions shown to the LLM.
    pub instructions: Option<String>,

    /// Tool description overrides by tool name.
    #[serde(default)]
    pub tools: HashMap<String, ToolPrompt>,
}

impl Prompts {
    /// Load prompts from file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        // Handle empty or comment-only YAML files (which parse as null)
        let prompts: Option<Prompts> = serde_yaml::from_str(&content)?;
        Ok(prompts.unwrap_or_default())
    }

    /// Load prompts from default location or return defaults.
    pub fn load_or_default() -> Self {
        // Try .task-graph/prompts.yaml
        if let Ok(prompts) = Self::load(".task-graph/prompts.yaml") {
            return prompts;
        }

        Self::default()
    }

    /// Get a tool description override if available.
    pub fn get_tool_description(&self, name: &str) -> Option<&str> {
        self.tools.get(name).map(|t| t.description.as_str())
    }
}
