//! File watcher for configuration files.
//!
//! Watches for changes to:
//! - `config/*.yaml` files (workflow definitions, config)
//! - `task-graph/skills/` directory (custom skills)
//!
//! Emits reload events through a tokio watch channel when changes are detected.
//! Uses debouncing to coalesce rapid file changes.

use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// Event types emitted when configuration files change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChangeEvent {
    /// A config YAML file changed (config.yaml, prompts.yaml, workflow-*.yaml)
    ConfigYaml(PathBuf),
    /// A workflow YAML file changed
    WorkflowYaml(PathBuf),
    /// Skills directory changed (file added, modified, or removed)
    SkillsChanged(PathBuf),
    /// Multiple files changed in quick succession
    BatchChange(Vec<PathBuf>),
    /// Watcher encountered an error
    Error(String),
}

impl ConfigChangeEvent {
    /// Returns true if this event requires a config reload.
    pub fn requires_reload(&self) -> bool {
        !matches!(self, ConfigChangeEvent::Error(_))
    }

    /// Get the affected paths for this event.
    pub fn affected_paths(&self) -> Vec<&Path> {
        match self {
            ConfigChangeEvent::ConfigYaml(p) => vec![p.as_path()],
            ConfigChangeEvent::WorkflowYaml(p) => vec![p.as_path()],
            ConfigChangeEvent::SkillsChanged(p) => vec![p.as_path()],
            ConfigChangeEvent::BatchChange(paths) => paths.iter().map(|p| p.as_path()).collect(),
            ConfigChangeEvent::Error(_) => vec![],
        }
    }
}

/// Configuration for the file watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Debounce duration for coalescing rapid changes.
    pub debounce_duration: Duration,
    /// Whether to watch config YAML files.
    pub watch_config: bool,
    /// Whether to watch skills directory.
    pub watch_skills: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_duration: Duration::from_millis(500),
            watch_config: true,
            watch_skills: true,
        }
    }
}

/// Paths to watch for configuration changes.
#[derive(Debug, Clone)]
pub struct WatchPaths {
    /// Config directory (typically `config/` or `task-graph/`)
    pub config_dir: Option<PathBuf>,
    /// Skills directory (typically `task-graph/skills/`)
    pub skills_dir: Option<PathBuf>,
}

/// Handle to control the config watcher.
pub struct ConfigWatcherHandle {
    /// Receiver for config change events.
    /// Cloning this receiver will allow multiple consumers to receive events.
    pub events: watch::Receiver<Option<ConfigChangeEvent>>,
    /// Handle to the watcher task (dropping this will stop the watcher).
    _task_handle: tokio::task::JoinHandle<()>,
}

impl ConfigWatcherHandle {
    /// Wait for the next config change event.
    pub async fn wait_for_change(&mut self) -> Option<ConfigChangeEvent> {
        // Skip the initial None value
        loop {
            if self.events.changed().await.is_err() {
                return None; // Sender dropped
            }
            let event = self.events.borrow().clone();
            if event.is_some() {
                return event;
            }
        }
    }

    /// Check if there's a pending change without blocking.
    pub fn has_pending_change(&self) -> bool {
        self.events.borrow().is_some()
    }

    /// Get the latest event without waiting.
    pub fn latest_event(&self) -> Option<ConfigChangeEvent> {
        self.events.borrow().clone()
    }
}

/// Starts the configuration file watcher.
///
/// Returns a handle that provides:
/// - A watch channel receiver for config change events
/// - Automatic cleanup when dropped
///
/// # Arguments
/// * `paths` - Directories to watch
/// * `config` - Watcher configuration
///
/// # Example
/// ```ignore
/// let paths = WatchPaths {
///     config_dir: Some(PathBuf::from("./task-graph")),
///     skills_dir: Some(PathBuf::from("./task-graph/skills")),
/// };
/// let handle = start_config_watcher(paths, WatcherConfig::default())?;
///
/// // In an async context:
/// tokio::spawn(async move {
///     let mut events = handle.events;
///     while events.changed().await.is_ok() {
///         if let Some(event) = events.borrow().clone() {
///             println!("Config changed: {:?}", event);
///         }
///     }
/// });
/// ```
pub fn start_config_watcher(
    paths: WatchPaths,
    config: WatcherConfig,
) -> Result<ConfigWatcherHandle, notify::Error> {
    let (event_tx, event_rx) = watch::channel(None);
    let (notify_tx, notify_rx) = mpsc::channel();

    // Create the debounced watcher
    let mut debouncer = new_debouncer(config.debounce_duration, notify_tx)?;

    // Set up watches for each configured path
    let watcher = debouncer.watcher();

    if config.watch_config
        && let Some(ref config_dir) = paths.config_dir
    {
        if config_dir.exists() {
            info!("Watching config directory: {}", config_dir.display());
            watcher.watch(config_dir, notify::RecursiveMode::NonRecursive)?;
        } else {
            warn!(
                "Config directory does not exist, skipping watch: {}",
                config_dir.display()
            );
        }
    }

    if config.watch_skills
        && let Some(ref skills_dir) = paths.skills_dir
    {
        if skills_dir.exists() {
            info!("Watching skills directory: {}", skills_dir.display());
            watcher.watch(skills_dir, notify::RecursiveMode::Recursive)?;
        } else {
            warn!(
                "Skills directory does not exist, skipping watch: {}",
                skills_dir.display()
            );
        }
    }

    // Spawn the event processing task
    let task_handle = tokio::task::spawn_blocking(move || {
        // Keep the debouncer alive
        let _debouncer = debouncer;

        // Process events from the notify channel
        process_notify_events(notify_rx, event_tx, &paths);
    });

    Ok(ConfigWatcherHandle {
        events: event_rx,
        _task_handle: task_handle,
    })
}

/// Process events from the notify debouncer and convert to ConfigChangeEvents.
fn process_notify_events(
    rx: mpsc::Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>,
    tx: watch::Sender<Option<ConfigChangeEvent>>,
    paths: &WatchPaths,
) {
    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let change_events = classify_events(events, paths);
                for event in change_events {
                    debug!("Config change detected: {:?}", event);
                    if tx.send(Some(event)).is_err() {
                        // Receiver dropped, exit
                        info!("Config watcher receiver dropped, stopping");
                        return;
                    }
                }
            }
            Ok(Err(e)) => {
                error!("File watcher error: {}", e);
                let _ = tx.send(Some(ConfigChangeEvent::Error(e.to_string())));
            }
            Err(_) => {
                // Channel closed, exit
                info!("Config watcher channel closed, stopping");
                return;
            }
        }
    }
}

/// Classify debounced events into ConfigChangeEvents.
fn classify_events(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    paths: &WatchPaths,
) -> Vec<ConfigChangeEvent> {
    let mut result = Vec::new();
    let mut changed_paths: Vec<PathBuf> = Vec::new();

    for event in events {
        // Only process data change events (not just any access)
        if !matches!(
            event.kind,
            DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous
        ) {
            continue;
        }

        let path = event.path;

        // Classify the path
        if let Some(event) = classify_path(&path, paths) {
            match event {
                ConfigChangeEvent::BatchChange(mut batch_paths) => {
                    changed_paths.append(&mut batch_paths);
                }
                _ => {
                    // For non-batch events, check if we should batch them
                    if let Some(p) = event.affected_paths().first() {
                        changed_paths.push(p.to_path_buf());
                    }
                }
            }
        }
    }

    // If we have multiple paths, create a batch event
    if changed_paths.len() > 1 {
        result.push(ConfigChangeEvent::BatchChange(changed_paths));
    } else if let Some(path) = changed_paths.into_iter().next()
        && let Some(event) = classify_path(&path, paths)
    {
        result.push(event);
    }

    result
}

/// Classify a single path into a ConfigChangeEvent.
fn classify_path(path: &Path, paths: &WatchPaths) -> Option<ConfigChangeEvent> {
    let extension = path.extension().and_then(|e| e.to_str());
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Check if it's a YAML file
    if matches!(extension, Some("yaml") | Some("yml")) {
        // Check if it's a workflow file
        if file_name.starts_with("workflow") {
            return Some(ConfigChangeEvent::WorkflowYaml(path.to_path_buf()));
        }
        // Check if it's config.yaml or prompts.yaml
        if file_name == "config.yaml" || file_name == "prompts.yaml" {
            return Some(ConfigChangeEvent::ConfigYaml(path.to_path_buf()));
        }
        // Other YAML files in config dir are treated as config
        if let Some(ref config_dir) = paths.config_dir
            && path.starts_with(config_dir)
        {
            return Some(ConfigChangeEvent::ConfigYaml(path.to_path_buf()));
        }
    }

    // Check if it's in the skills directory
    if let Some(ref skills_dir) = paths.skills_dir
        && path.starts_with(skills_dir)
    {
        return Some(ConfigChangeEvent::SkillsChanged(path.to_path_buf()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_config_yaml() {
        let paths = WatchPaths {
            config_dir: Some(PathBuf::from("task-graph")),
            skills_dir: Some(PathBuf::from("task-graph/skills")),
        };

        let result = classify_path(&PathBuf::from("task-graph/config.yaml"), &paths);
        assert!(matches!(result, Some(ConfigChangeEvent::ConfigYaml(_))));
    }

    #[test]
    fn test_classify_workflow_yaml() {
        let paths = WatchPaths {
            config_dir: Some(PathBuf::from("config")),
            skills_dir: None,
        };

        let result = classify_path(&PathBuf::from("config/workflow-swarm.yaml"), &paths);
        assert!(matches!(result, Some(ConfigChangeEvent::WorkflowYaml(_))));
    }

    #[test]
    fn test_classify_skills_change() {
        let paths = WatchPaths {
            config_dir: None,
            skills_dir: Some(PathBuf::from("task-graph/skills")),
        };

        let result = classify_path(
            &PathBuf::from("task-graph/skills/coordinator/SKILL.md"),
            &paths,
        );
        assert!(matches!(result, Some(ConfigChangeEvent::SkillsChanged(_))));
    }

    #[test]
    fn test_classify_unknown_file() {
        let paths = WatchPaths {
            config_dir: Some(PathBuf::from("config")),
            skills_dir: None,
        };

        let result = classify_path(&PathBuf::from("src/main.rs"), &paths);
        assert!(result.is_none());
    }

    #[test]
    fn test_event_requires_reload() {
        assert!(ConfigChangeEvent::ConfigYaml(PathBuf::new()).requires_reload());
        assert!(ConfigChangeEvent::WorkflowYaml(PathBuf::new()).requires_reload());
        assert!(ConfigChangeEvent::SkillsChanged(PathBuf::new()).requires_reload());
        assert!(!ConfigChangeEvent::Error("test".to_string()).requires_reload());
    }
}
