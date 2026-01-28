//! Configuration types and structures.
//!
//! This module contains all the configuration types used throughout the application.

use crate::format::OutputFormat;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Default port for the web dashboard.
pub const DEFAULT_UI_PORT: u16 = 31994;

/// UI mode for the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiMode {
    /// No UI, MCP server only (default)
    #[default]
    None,
    /// Enable web dashboard UI
    Web,
}

/// UI configuration for the web dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// UI mode: none (MCP only) or web (enable dashboard).
    #[serde(default)]
    pub mode: UiMode,

    /// Port for the web dashboard (default: 31994).
    #[serde(default = "default_ui_port")]
    pub port: u16,

    /// Initial retry delay in milliseconds when dashboard fails to start (default: 15000).
    #[serde(default = "default_retry_initial_ms")]
    pub retry_initial_ms: u64,

    /// Jitter range in milliseconds for retry delay (default: 5000, meaning ±5s).
    #[serde(default = "default_retry_jitter_ms")]
    pub retry_jitter_ms: u64,

    /// Maximum retry interval in milliseconds (default: 240000 = 4 minutes).
    #[serde(default = "default_retry_max_ms")]
    pub retry_max_ms: u64,

    /// Exponential backoff multiplier (default: 2.0).
    #[serde(default = "default_retry_multiplier")]
    pub retry_multiplier: f64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            mode: UiMode::default(),
            port: default_ui_port(),
            retry_initial_ms: default_retry_initial_ms(),
            retry_jitter_ms: default_retry_jitter_ms(),
            retry_max_ms: default_retry_max_ms(),
            retry_multiplier: default_retry_multiplier(),
        }
    }
}

fn default_ui_port() -> u16 {
    DEFAULT_UI_PORT
}

fn default_retry_initial_ms() -> u64 {
    15_000 // 15 seconds
}

fn default_retry_jitter_ms() -> u64 {
    5_000 // ±5 seconds
}

fn default_retry_max_ms() -> u64 {
    240_000 // 4 minutes
}

fn default_retry_multiplier() -> f64 {
    2.0
}

/// Auto-advance configuration for automatically transitioning tasks when dependencies are satisfied.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoAdvanceConfig {
    /// Enable auto-advance when dependencies are satisfied (default: false).
    #[serde(default)]
    pub enabled: bool,

    /// Target state for auto-advanced tasks (e.g., "ready").
    /// If None, tasks remain in their current state even when unblocked.
    #[serde(default)]
    pub target_state: Option<String>,
}

/// Behavior for unknown attachment keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UnknownKeyBehavior {
    /// Silently use default mime/mode.
    Allow,
    /// Use defaults but return a warning in the response (default).
    #[default]
    Warn,
    /// Reject unknown keys with an error.
    Reject,
}

/// Definition of a preconfigured attachment key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentKeyDefinition {
    /// Default MIME type for this key.
    pub mime: String,
    /// Default mode: "append" or "replace".
    #[serde(default = "default_append_mode")]
    pub mode: String,
}

fn default_append_mode() -> String {
    "append".to_string()
}

/// Attachments configuration with preconfigured key definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentsConfig {
    /// Behavior for unknown attachment keys (allow, warn, reject).
    #[serde(default)]
    pub unknown_key: UnknownKeyBehavior,
    /// Preconfigured attachment key definitions.
    #[serde(default = "AttachmentsConfig::default_definitions")]
    pub definitions: HashMap<String, AttachmentKeyDefinition>,
}

impl Default for AttachmentsConfig {
    fn default() -> Self {
        Self {
            unknown_key: UnknownKeyBehavior::default(),
            definitions: Self::default_definitions(),
        }
    }
}

impl AttachmentsConfig {
    /// Default attachment key definitions.
    pub fn default_definitions() -> HashMap<String, AttachmentKeyDefinition> {
        let mut defs = HashMap::new();

        defs.insert(
            "commit".to_string(),
            AttachmentKeyDefinition {
                mime: "text/git.hash".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "checkin".to_string(),
            AttachmentKeyDefinition {
                mime: "text/p4.changelist".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "meta".to_string(),
            AttachmentKeyDefinition {
                mime: "application/json".to_string(),
                mode: "replace".to_string(),
            },
        );

        defs.insert(
            "note".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "log".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "error".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "output".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "diff".to_string(),
            AttachmentKeyDefinition {
                mime: "text/x-diff".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "changelist".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "append".to_string(),
            },
        );

        defs.insert(
            "plan".to_string(),
            AttachmentKeyDefinition {
                mime: "text/markdown".to_string(),
                mode: "replace".to_string(),
            },
        );

        defs.insert(
            "result".to_string(),
            AttachmentKeyDefinition {
                mime: "application/json".to_string(),
                mode: "replace".to_string(),
            },
        );

        defs.insert(
            "context".to_string(),
            AttachmentKeyDefinition {
                mime: "text/plain".to_string(),
                mode: "replace".to_string(),
            },
        );

        defs
    }

    /// Get the definition for a key, if it exists.
    pub fn get_definition(&self, key: &str) -> Option<&AttachmentKeyDefinition> {
        self.definitions.get(key)
    }

    /// Check if a key is a known/configured key.
    pub fn is_known_key(&self, key: &str) -> bool {
        self.definitions.contains_key(key)
    }

    /// Get the default MIME type for a key, or fallback to text/plain.
    pub fn get_mime_default(&self, key: &str) -> &str {
        self.definitions
            .get(key)
            .map(|d| d.mime.as_str())
            .unwrap_or("text/plain")
    }

    /// Get the default mode for a key, or fallback to "append".
    pub fn get_mode_default(&self, key: &str) -> &str {
        self.definitions
            .get(key)
            .map(|d| d.mode.as_str())
            .unwrap_or("append")
    }
}

/// Definition of a preconfigured tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDefinition {
    /// Category for grouping (e.g., "language", "domain", "type").
    #[serde(default)]
    pub category: Option<String>,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Tags configuration with preconfigured tag definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagsConfig {
    /// Behavior for unknown tags (allow, warn, reject).
    #[serde(default)]
    pub unknown_tag: UnknownKeyBehavior,
    /// Preconfigured tag definitions.
    #[serde(default)]
    pub definitions: HashMap<String, TagDefinition>,
}

impl Default for TagsConfig {
    fn default() -> Self {
        Self {
            unknown_tag: UnknownKeyBehavior::default(),
            definitions: HashMap::new(),
        }
    }
}

impl TagsConfig {
    /// Check if a tag is a known/defined tag.
    pub fn is_known_tag(&self, tag: &str) -> bool {
        self.definitions.contains_key(tag)
    }

    /// Get all defined tag names.
    pub fn tag_names(&self) -> Vec<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    /// Get all tags in a specific category.
    pub fn tags_in_category(&self, category: &str) -> Vec<&str> {
        self.definitions
            .iter()
            .filter(|(_, def)| def.category.as_deref() == Some(category))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Get all unique categories.
    pub fn categories(&self) -> Vec<&str> {
        let mut cats: Vec<&str> = self
            .definitions
            .values()
            .filter_map(|def| def.category.as_deref())
            .collect();
        cats.sort();
        cats.dedup();
        cats
    }

    /// Validate a single tag, returning Ok(None) if valid, Ok(Some(warning)) for warn mode, or Err for reject.
    pub fn validate_tag(&self, tag: &str) -> Result<Option<String>> {
        if self.is_known_tag(tag) {
            return Ok(None);
        }

        match self.unknown_tag {
            UnknownKeyBehavior::Allow => Ok(None),
            UnknownKeyBehavior::Warn => Ok(Some(format!(
                "Unknown tag '{}'. Known tags: {:?}",
                tag,
                self.tag_names()
            ))),
            UnknownKeyBehavior::Reject => Err(anyhow!(
                "Unknown tag '{}'. Configure in tags.definitions or set unknown_tag to 'allow' or 'warn'. Known tags: {:?}",
                tag,
                self.tag_names()
            )),
        }
    }

    /// Validate multiple tags, collecting warnings and stopping on first reject.
    pub fn validate_tags(&self, tags: &[String]) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        for tag in tags {
            if let Some(warning) = self.validate_tag(tag)? {
                warnings.push(warning);
            }
        }
        Ok(warnings)
    }
}

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub states: StatesConfig,

    #[serde(default)]
    pub dependencies: DependenciesConfig,

    #[serde(default)]
    pub auto_advance: AutoAdvanceConfig,

    #[serde(default)]
    pub attachments: AttachmentsConfig,

    #[serde(default)]
    pub phases: PhasesConfig,

    #[serde(default)]
    pub tags: TagsConfig,
}

/// Paths configured for the server, returned by connect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPaths {
    /// Path to the SQLite database file.
    pub db_path: PathBuf,
    /// Path to the media directory for file attachments.
    pub media_dir: PathBuf,
    /// Path to the log directory.
    pub log_dir: PathBuf,
    /// Path to the configuration file (if one was loaded).
    pub config_path: Option<PathBuf>,
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

    /// Default output format for query results (json or markdown).
    #[serde(default)]
    pub default_format: OutputFormat,

    /// Path to the skills directory for skill overrides.
    #[serde(default = "default_skills_dir")]
    pub skills_dir: PathBuf,

    /// Path to the log directory.
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,

    /// UI configuration for the web dashboard.
    #[serde(default)]
    pub ui: UiConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            media_dir: default_media_dir(),
            claim_limit: default_claim_limit(),
            stale_timeout_seconds: default_stale_timeout(),
            default_format: OutputFormat::default(),
            skills_dir: default_skills_dir(),
            log_dir: default_log_dir(),
            ui: UiConfig::default(),
        }
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from("task-graph/tasks.db")
}

fn default_media_dir() -> PathBuf {
    PathBuf::from("task-graph/media")
}

fn default_skills_dir() -> PathBuf {
    PathBuf::from("task-graph/skills")
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("task-graph/logs")
}

fn default_paths_root() -> String {
    ".".to_string()
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
    /// Root directory for sandboxing (default: ".")
    #[serde(default = "default_paths_root")]
    pub root: String,

    /// Style for representing file paths.
    #[serde(default)]
    pub style: PathStyle,

    /// Auto-map single-letter Windows drives (default: false)
    #[serde(default)]
    pub map_windows_drives: bool,

    /// Prefix mappings (prefix -> path)
    /// Values can be: literal path, $ENV_VAR, or ${config.path}
    #[serde(default)]
    pub mappings: HashMap<String, String>,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            root: default_paths_root(),
            style: PathStyle::Relative,
            map_windows_drives: false,
            mappings: HashMap::new(),
        }
    }
}

/// Path style for file locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PathStyle {
    /// Relative paths (e.g., src/main.rs)
    #[default]
    Relative,
    /// Project-prefixed paths (e.g., ${project}/src/main.rs)
    ProjectPrefixed,
}

/// Task state configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatesConfig {
    /// Default state for new tasks.
    #[serde(default = "default_initial_state")]
    pub initial: String,

    /// Default state for tasks when their owner disconnects (must be untimed).
    #[serde(default = "default_disconnect_state")]
    pub disconnect_state: String,

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
            disconnect_state: default_disconnect_state(),
            blocking_states: default_blocking_states(),
            definitions: default_state_definitions(),
        }
    }
}

fn default_initial_state() -> String {
    "pending".to_string()
}

fn default_disconnect_state() -> String {
    "pending".to_string()
}

fn default_blocking_states() -> Vec<String> {
    vec![
        "pending".to_string(),
        "assigned".to_string(),
        "working".to_string(),
    ]
}

fn default_state_definitions() -> HashMap<String, StateDefinition> {
    let mut defs = HashMap::new();

    defs.insert(
        "pending".to_string(),
        StateDefinition {
            exits: vec![
                "assigned".to_string(),
                "working".to_string(),
                "cancelled".to_string(),
            ],
            timed: false,
        },
    );

    defs.insert(
        "assigned".to_string(),
        StateDefinition {
            exits: vec![
                "working".to_string(),
                "pending".to_string(),
                "cancelled".to_string(),
            ],
            timed: false,
        },
    );

    defs.insert(
        "working".to_string(),
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
            exits: vec!["pending".to_string()],
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

/// Dependency type configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependenciesConfig {
    /// Dependency type definitions.
    #[serde(default = "default_dependency_definitions")]
    pub definitions: HashMap<String, DependencyDefinition>,
}

impl Default for DependenciesConfig {
    fn default() -> Self {
        Self {
            definitions: default_dependency_definitions(),
        }
    }
}

/// Definition of a dependency type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyDefinition {
    /// Display orientation: "horizontal" (same level) or "vertical" (parent-child).
    pub display: DependencyDisplay,

    /// What this dependency blocks: "start" (blocks claiming) or "completion" (blocks completing).
    pub blocks: BlockTarget,
}

/// Display orientation for dependency visualization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyDisplay {
    /// Same level dependencies (blocks, follows).
    Horizontal,
    /// Parent-child relationships (contains).
    Vertical,
}

/// What a dependency blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockTarget {
    /// Does not block - informational link only.
    None,
    /// Blocks the task from being started/claimed.
    Start,
    /// Blocks the task from being completed.
    Completion,
}

fn default_dependency_definitions() -> HashMap<String, DependencyDefinition> {
    let mut defs = HashMap::new();

    // Primary workflow types (blocking)
    defs.insert(
        "blocks".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Horizontal,
            blocks: BlockTarget::Start,
        },
    );

    defs.insert(
        "follows".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Horizontal,
            blocks: BlockTarget::Start,
        },
    );

    defs.insert(
        "contains".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Vertical,
            blocks: BlockTarget::Completion,
        },
    );

    // Non-blocking relationship types
    defs.insert(
        "duplicate".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Horizontal,
            blocks: BlockTarget::None,
        },
    );

    defs.insert(
        "see-also".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Horizontal,
            blocks: BlockTarget::None,
        },
    );

    defs.insert(
        "relates-to".to_string(),
        DependencyDefinition {
            display: DependencyDisplay::Horizontal,
            blocks: BlockTarget::None,
        },
    );

    defs
}

impl DependenciesConfig {
    /// Check if a dependency type is valid.
    pub fn is_valid_dep_type(&self, dep_type: &str) -> bool {
        self.definitions.contains_key(dep_type)
    }

    /// Get the definition for a dependency type.
    pub fn get_definition(&self, dep_type: &str) -> Option<&DependencyDefinition> {
        self.definitions.get(dep_type)
    }

    /// Get all dependency types that block start.
    pub fn start_blocking_types(&self) -> Vec<&str> {
        self.definitions
            .iter()
            .filter(|(_, def)| def.blocks == BlockTarget::Start)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Get all dependency types that block completion.
    pub fn completion_blocking_types(&self) -> Vec<&str> {
        self.definitions
            .iter()
            .filter(|(_, def)| def.blocks == BlockTarget::Completion)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Get all vertical (parent-child) dependency types.
    pub fn vertical_types(&self) -> Vec<&str> {
        self.definitions
            .iter()
            .filter(|(_, def)| def.display == DependencyDisplay::Vertical)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Get all dependency type names.
    pub fn dep_type_names(&self) -> Vec<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    /// Validate the dependencies configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.definitions.is_empty() {
            return Err(anyhow::anyhow!(
                "At least one dependency type must be defined"
            ));
        }

        // Check for at least one start-blocking type (for task sequencing)
        let has_start_blocking = self
            .definitions
            .values()
            .any(|d| d.blocks == BlockTarget::Start);
        if !has_start_blocking {
            return Err(anyhow::anyhow!(
                "At least one dependency type with blocks: start must be defined"
            ));
        }

        Ok(())
    }
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

    /// Get all untimed state names (valid for disconnect final_state).
    pub fn untimed_state_names(&self) -> Vec<&str> {
        self.definitions
            .iter()
            .filter(|(_, def)| !def.timed)
            .map(|(name, _)| name.as_str())
            .collect()
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

        // Check disconnect_state exists and is not timed
        if !self.definitions.contains_key(&self.disconnect_state) {
            return Err(anyhow!(
                "Disconnect state '{}' is not defined in state definitions",
                self.disconnect_state
            ));
        }
        if self.is_timed_state(&self.disconnect_state) {
            return Err(anyhow!(
                "Disconnect state '{}' must not be a timed state",
                self.disconnect_state
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

/// Phase configuration for categorizing type of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhasesConfig {
    /// Behavior for unknown phase values (allow, warn, reject).
    #[serde(default)]
    pub unknown_phase: UnknownKeyBehavior,

    /// Known phase definitions.
    #[serde(default = "default_phases")]
    pub definitions: HashSet<String>,
}

impl Default for PhasesConfig {
    fn default() -> Self {
        Self {
            unknown_phase: UnknownKeyBehavior::Warn,
            definitions: default_phases(),
        }
    }
}

fn default_phases() -> HashSet<String> {
    [
        "deliver",   // Top-level deliverable
        "triage",    // Initial assessment and prioritization
        "explore",   // Research and discovery
        "diagnose",  // Debugging and troubleshooting
        "design",    // Architecture and design
        "plan",      // Planning and specification
        "implement", // Implementation/coding
        "test",      // Testing and validation
        "review",    // Code review
        "security",  // Security review/audit
        "doc",       // Documentation
        "integrate", // Integration work
        "deploy",    // Release to staging/production
        "monitor",   // Observability and metrics
        "optimize",  // Performance tuning
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl PhasesConfig {
    /// Check if a phase is a known/defined phase.
    pub fn is_known_phase(&self, phase: &str) -> bool {
        self.definitions.contains(phase)
    }

    /// Get all defined phase names.
    pub fn phase_names(&self) -> Vec<&str> {
        self.definitions.iter().map(|s| s.as_str()).collect()
    }

    /// Check a phase and return a warning message if unknown (based on unknown_phase behavior).
    /// Returns None if the phase is known or if behavior is Allow.
    /// Returns Some(warning) if behavior is Warn.
    /// Returns Err if behavior is Reject.
    pub fn check_phase(&self, phase: &str) -> Result<Option<String>> {
        if self.is_known_phase(phase) {
            return Ok(None);
        }

        match self.unknown_phase {
            UnknownKeyBehavior::Allow => Ok(None),
            UnknownKeyBehavior::Warn => Ok(Some(format!(
                "Unknown phase '{}'. Known phases: {:?}",
                phase,
                self.phase_names()
            ))),
            UnknownKeyBehavior::Reject => Err(anyhow!(
                "Unknown phase '{}'. Known phases: {:?}. Configure in phases.definitions or set unknown_phase to 'allow' or 'warn'.",
                phase,
                self.phase_names()
            )),
        }
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
    ///
    /// **Deprecated**: Use `ConfigLoader::load()` instead for proper tier merging.
    pub fn load_or_default() -> Self {
        // Try TASK_GRAPH_CONFIG_PATH environment variable first
        if let Ok(config_path) = std::env::var("TASK_GRAPH_CONFIG_PATH")
            && let Ok(config) = Self::load(&config_path)
        {
            return config;
        }

        // Try task-graph/config.yaml (new location)
        if let Ok(config) = Self::load("task-graph/config.yaml") {
            return config;
        }

        // Try .task-graph/config.yaml (deprecated location)
        if let Ok(config) = Self::load(".task-graph/config.yaml") {
            return config;
        }

        // Fall back to defaults with environment variable overrides
        let mut config = Self::default();

        if let Ok(db_path) = std::env::var("TASK_GRAPH_DB_PATH") {
            config.server.db_path = PathBuf::from(db_path);
        }

        if let Ok(media_dir) = std::env::var("TASK_GRAPH_MEDIA_DIR") {
            config.server.media_dir = PathBuf::from(media_dir);
        }

        if let Ok(log_dir) = std::env::var("TASK_GRAPH_LOG_DIR") {
            config.server.log_dir = PathBuf::from(log_dir);
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

    /// Ensure the log directory exists.
    pub fn ensure_log_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.server.log_dir)?;
        Ok(())
    }

    /// Get the media directory path.
    pub fn media_dir(&self) -> &Path {
        &self.server.media_dir
    }

    /// Get the log directory path.
    pub fn log_dir(&self) -> &Path {
        &self.server.log_dir
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
    ///
    /// **Deprecated**: Use `ConfigLoader` for proper tier merging.
    pub fn load_or_default() -> Self {
        // Try task-graph/prompts.yaml (new location)
        if let Ok(prompts) = Self::load("task-graph/prompts.yaml") {
            return prompts;
        }

        // Try .task-graph/prompts.yaml (deprecated location)
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
