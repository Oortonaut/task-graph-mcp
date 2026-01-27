//! Unified configuration system.
//!
//! Consolidates configuration from three tiers with field-by-field YAML merging:
//! 1. **Defaults** - Embedded at build time from `./config/`
//! 2. **Project** - `$CWD/task-graph/` (with `.task-graph/` backward compat)
//! 3. **User** - `~/.task-graph/` and environment variables
//!
//! ## Merge Strategy
//! - YAML files (`config.yaml`, `prompts.yaml`): Deep merge field-by-field
//! - Other files (skills, templates): First-found-wins from highest tier
//!
//! ## Environment Variables
//! - `TASK_GRAPH_CONFIG_PATH` - Explicit config file (overrides all)
//! - `TASK_GRAPH_DB_PATH` - Database path
//! - `TASK_GRAPH_MEDIA_DIR` - Media directory
//! - `TASK_GRAPH_LOG_DIR` - Log directory
//! - `TASK_GRAPH_SKILLS_DIR` - Skills directory
//! - `TASK_GRAPH_USER_DIR` - User config dir (default: `~/.task-graph`)
//! - `TASK_GRAPH_PROJECT_DIR` - Project config dir (default: `./task-graph`)

mod files;
mod loader;
mod merge;
mod types;

pub use files::{FileSource, ResolvedFile};
pub use loader::{ConfigLoader, ConfigPaths, ConfigTier};
pub use merge::deep_merge;
pub use types::*;
