//! Configuration loader with tier-based merging.
//!
//! Loads configuration from multiple tiers and merges them field-by-field.

use super::merge::deep_merge_all;
use super::types::{Config, Prompts};
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Configuration tier priority (lowest to highest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigTier {
    /// Embedded defaults (lowest priority)
    Defaults = 0,
    /// Project-level config ($CWD/task-graph/ or .task-graph/)
    Project = 1,
    /// User-level config (~/.task-graph/)
    User = 2,
    /// Environment variables (highest priority)
    Environment = 3,
}

impl std::fmt::Display for ConfigTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigTier::Defaults => write!(f, "defaults"),
            ConfigTier::Project => write!(f, "project"),
            ConfigTier::User => write!(f, "user"),
            ConfigTier::Environment => write!(f, "environment"),
        }
    }
}

/// Paths for each configuration tier.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// Embedded defaults directory (not a real path, but conceptual)
    pub defaults_dir: Option<PathBuf>,
    /// Project-level config directory
    pub project_dir: Option<PathBuf>,
    /// Deprecated project-level config directory (.task-graph)
    pub project_dir_deprecated: Option<PathBuf>,
    /// User-level config directory
    pub user_dir: Option<PathBuf>,
}

impl Default for ConfigPaths {
    fn default() -> Self {
        Self::discover()
    }
}

impl ConfigPaths {
    /// Discover configuration paths from environment and defaults.
    pub fn discover() -> Self {
        // User dir: TASK_GRAPH_USER_DIR or ~/.task-graph
        let user_dir = std::env::var("TASK_GRAPH_USER_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".task-graph")));

        // Project dir: TASK_GRAPH_PROJECT_DIR or $CWD/task-graph
        let project_dir = std::env::var("TASK_GRAPH_PROJECT_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("task-graph")));

        // Deprecated project dir: $CWD/.task-graph
        let project_dir_deprecated = Some(PathBuf::from(".task-graph"));

        Self {
            defaults_dir: None, // Defaults are embedded, not on disk
            project_dir,
            project_dir_deprecated,
            user_dir,
        }
    }

    /// Create paths with explicit directories.
    pub fn with_dirs(
        project_dir: Option<PathBuf>,
        user_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            defaults_dir: None,
            project_dir,
            project_dir_deprecated: Some(PathBuf::from(".task-graph")),
            user_dir,
        }
    }

    /// Get the effective project directory (prefers new location, falls back to deprecated).
    pub fn effective_project_dir(&self) -> Option<&Path> {
        // Check new location first
        if let Some(ref dir) = self.project_dir {
            if dir.exists() {
                return Some(dir);
            }
        }

        // Fall back to deprecated location
        if let Some(ref dir) = self.project_dir_deprecated {
            if dir.exists() {
                return Some(dir);
            }
        }

        // If neither exists, prefer new location for creation
        self.project_dir.as_deref()
    }

    /// Check if using deprecated project directory.
    pub fn is_using_deprecated(&self) -> bool {
        if let Some(ref new_dir) = self.project_dir {
            if new_dir.exists() {
                return false;
            }
        }

        if let Some(ref dep_dir) = self.project_dir_deprecated {
            return dep_dir.exists();
        }

        false
    }
}

/// Configuration loader that handles tier-based merging.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    /// Paths for each tier
    pub paths: ConfigPaths,
    /// Loaded configuration
    config: Config,
    /// Path to the config file that was used (if any)
    config_path: Option<PathBuf>,
    /// Whether deprecated paths are in use
    using_deprecated: bool,
}

impl ConfigLoader {
    /// Load configuration from all tiers with proper merging.
    pub fn load() -> Result<Self> {
        Self::load_with_paths(ConfigPaths::discover())
    }

    /// Load configuration with explicit paths.
    pub fn load_with_paths(paths: ConfigPaths) -> Result<Self> {
        let using_deprecated = paths.is_using_deprecated();
        
        if using_deprecated {
            warn!(
                "Using deprecated config directory '.task-graph/'. \
                 Run 'task-graph migrate' to move to 'task-graph/'."
            );
        }

        // Check for explicit config path override
        if let Ok(explicit_path) = std::env::var("TASK_GRAPH_CONFIG_PATH") {
            let path = PathBuf::from(&explicit_path);
            let config = Config::load(&path)?;
            return Ok(Self {
                paths,
                config,
                config_path: Some(path),
                using_deprecated,
            });
        }

        // Collect configs from each tier
        let mut configs: Vec<Value> = Vec::new();

        // Tier 1: Defaults (embedded)
        let default_config = Config::default();
        if let Ok(default_json) = serde_json::to_value(&default_config) {
            configs.push(default_json);
        }

        // Tier 2: Project config
        let mut project_config_path = None;
        if let Some(project_dir) = paths.effective_project_dir() {
            let config_file = project_dir.join("config.yaml");
            if config_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&config_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        configs.push(yaml_value);
                        project_config_path = Some(config_file);
                    }
                }
            }
        }

        // Tier 3: User config
        if let Some(ref user_dir) = paths.user_dir {
            let config_file = user_dir.join("config.yaml");
            if config_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&config_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        configs.push(yaml_value);
                    }
                }
            }
        }

        // Merge all configs
        let merged = deep_merge_all(configs);
        let mut config: Config = serde_json::from_value(merged)?;

        // Tier 4: Environment variable overrides
        Self::apply_env_overrides(&mut config);

        Ok(Self {
            paths,
            config,
            config_path: project_config_path,
            using_deprecated,
        })
    }

    /// Apply environment variable overrides to config.
    fn apply_env_overrides(config: &mut Config) {
        if let Ok(db_path) = std::env::var("TASK_GRAPH_DB_PATH") {
            config.server.db_path = PathBuf::from(db_path);
        }

        if let Ok(media_dir) = std::env::var("TASK_GRAPH_MEDIA_DIR") {
            config.server.media_dir = PathBuf::from(media_dir);
        }

        if let Ok(log_dir) = std::env::var("TASK_GRAPH_LOG_DIR") {
            config.server.log_dir = PathBuf::from(log_dir);
        }

        if let Ok(skills_dir) = std::env::var("TASK_GRAPH_SKILLS_DIR") {
            config.server.skills_dir = PathBuf::from(skills_dir);
        }
    }

    /// Load prompts configuration with tier merging.
    pub fn load_prompts(&self) -> Prompts {
        let mut prompts_configs: Vec<Value> = Vec::new();

        // Tier 1: Defaults (empty)
        if let Ok(default_json) = serde_json::to_value(&Prompts::default()) {
            prompts_configs.push(default_json);
        }

        // Tier 2: Project prompts
        if let Some(project_dir) = self.paths.effective_project_dir() {
            let prompts_file = project_dir.join("prompts.yaml");
            if prompts_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&prompts_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        prompts_configs.push(yaml_value);
                    }
                }
            }
        }

        // Tier 3: User prompts
        if let Some(ref user_dir) = self.paths.user_dir {
            let prompts_file = user_dir.join("prompts.yaml");
            if prompts_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&prompts_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        prompts_configs.push(yaml_value);
                    }
                }
            }
        }

        // Merge and deserialize
        let merged = deep_merge_all(prompts_configs);
        serde_json::from_value(merged).unwrap_or_default()
    }

    /// Load workflows configuration with tier merging.
    ///
    /// Loads from embedded defaults, then project workflows.yaml, then user workflows.yaml.
    /// Later tiers override earlier ones (objects are deep-merged, prompts are replaced).
    pub fn load_workflows(&self) -> super::workflows::WorkflowsConfig {
        let mut workflows_configs: Vec<Value> = Vec::new();

        // Tier 1: Defaults (embedded)
        if let Ok(default_json) = serde_json::to_value(&super::workflows::WorkflowsConfig::default()) {
            workflows_configs.push(default_json);
        }

        // Tier 2: Project workflows
        if let Some(project_dir) = self.paths.effective_project_dir() {
            let workflows_file = project_dir.join("workflows.yaml");
            if workflows_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&workflows_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        workflows_configs.push(yaml_value);
                    }
                }
            }
        }

        // Tier 3: User workflows
        if let Some(ref user_dir) = self.paths.user_dir {
            let workflows_file = user_dir.join("workflows.yaml");
            if workflows_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&workflows_file) {
                    if let Ok(yaml_value) = serde_yaml::from_str::<Value>(&content) {
                        workflows_configs.push(yaml_value);
                    }
                }
            }
        }

        // Merge and deserialize
        let merged = deep_merge_all(workflows_configs);
        serde_json::from_value(merged).unwrap_or_default()
    }

    /// Get the loaded configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get mutable access to the configuration.
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Consume the loader and return the configuration.
    pub fn into_config(self) -> Config {
        self.config
    }

    /// Get the config file path that was used.
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Check if using deprecated paths.
    pub fn is_using_deprecated(&self) -> bool {
        self.using_deprecated
    }

    /// Get the effective project directory.
    pub fn project_dir(&self) -> Option<&Path> {
        self.paths.effective_project_dir()
    }

    /// Get the user directory.
    pub fn user_dir(&self) -> Option<&Path> {
        self.paths.user_dir.as_deref()
    }

    /// Get the skills directory, checking all tiers.
    pub fn skills_dir(&self) -> PathBuf {
        // Environment override takes precedence
        if let Ok(skills_dir) = std::env::var("TASK_GRAPH_SKILLS_DIR") {
            return PathBuf::from(skills_dir);
        }

        // Check project dir
        if let Some(project_dir) = self.paths.effective_project_dir() {
            let skills_dir = project_dir.join("skills");
            if skills_dir.exists() {
                return skills_dir;
            }
        }

        // Use config default
        self.config.server.skills_dir.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_paths_discover() {
        let paths = ConfigPaths::discover();
        assert!(paths.project_dir.is_some());
        // user_dir may or may not exist depending on environment
    }

    #[test]
    fn test_load_defaults_only() {
        // Create empty temp dirs so no config files are found
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::with_dirs(
            Some(temp.path().join("project")),
            Some(temp.path().join("user")),
        );

        let loader = ConfigLoader::load_with_paths(paths).unwrap();
        let config = loader.config();

        // Should have default values
        assert_eq!(config.server.claim_limit, 5);
        assert_eq!(config.server.stale_timeout_seconds, 900);
    }

    #[test]
    fn test_project_config_overrides_defaults() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        std::fs::create_dir_all(&project_dir).unwrap();

        // Create project config that overrides claim_limit
        let config_content = r#"
server:
  claim_limit: 10
"#;
        std::fs::write(project_dir.join("config.yaml"), config_content).unwrap();

        let paths = ConfigPaths::with_dirs(
            Some(project_dir),
            Some(temp.path().join("user")),
        );

        let loader = ConfigLoader::load_with_paths(paths).unwrap();
        let config = loader.config();

        // claim_limit should be overridden
        assert_eq!(config.server.claim_limit, 10);
        // stale_timeout_seconds should be default
        assert_eq!(config.server.stale_timeout_seconds, 900);
    }

    #[test]
    fn test_user_config_overrides_project() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();

        // Project config
        let project_config = r#"
server:
  claim_limit: 10
  stale_timeout_seconds: 600
"#;
        std::fs::write(project_dir.join("config.yaml"), project_config).unwrap();

        // User config overrides claim_limit
        let user_config = r#"
server:
  claim_limit: 20
"#;
        std::fs::write(user_dir.join("config.yaml"), user_config).unwrap();

        let paths = ConfigPaths::with_dirs(Some(project_dir), Some(user_dir));

        let loader = ConfigLoader::load_with_paths(paths).unwrap();
        let config = loader.config();

        // claim_limit should be from user
        assert_eq!(config.server.claim_limit, 20);
        // stale_timeout_seconds should be from project
        assert_eq!(config.server.stale_timeout_seconds, 600);
    }
}
