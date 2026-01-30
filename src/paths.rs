//! Path mapping and sandboxing system.
//!
//! This module provides a configurable path prefix mapping system that:
//! - Maps custom prefixes (e.g., `home:`, `project:`, `media:`) to configured paths
//! - Enforces lowercase prefixes
//! - Optionally maps Windows drive letters
//! - Sandboxes paths to prevent escape above root
//! - Is pure string manipulation (no filesystem I/O)
//! - Provides reverse translation API to get actual filesystem paths

use crate::config::{Config, PathStyle, PathsConfig};
use crate::error::ToolError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result type for path operations.
pub type PathResult<T> = Result<T, ToolError>;

/// Pure string-based path mapper. No filesystem I/O.
#[derive(Debug, Clone)]
pub struct PathMapper {
    /// Resolved root path (canonical form).
    root: String,
    /// Prefix to resolved path mappings.
    mappings: HashMap<String, String>,
    /// Whether to auto-map single-letter Windows drive prefixes.
    map_windows_drives: bool,
    /// Display style for paths.
    style: PathStyle,
}

#[allow(clippy::result_large_err)]
impl PathMapper {
    /// Create a PathMapper from configuration.
    ///
    /// Resolves `$ENV` and `${config.ref}` in mapping values.
    /// The root path is resolved relative to the current working directory.
    pub fn from_config(config: &PathsConfig, full_config: Option<&Config>) -> PathResult<Self> {
        // Resolve the root path
        let root = Self::resolve_root(&config.root)?;

        // Resolve all mappings
        let mut mappings = HashMap::new();
        for (prefix, value) in &config.mappings {
            // Validate prefix is lowercase
            if !prefix.chars().all(|c: char| c.is_ascii_lowercase()) {
                return Err(ToolError::prefix_not_lowercase(prefix));
            }

            let resolved = Self::resolve_mapping_value(value, &root, full_config)?;
            mappings.insert(prefix.clone(), resolved);
        }

        Ok(Self {
            root,
            mappings,
            map_windows_drives: config.map_windows_drives,
            style: config.style,
        })
    }

    /// Create a PathMapper with default configuration.
    pub fn new() -> PathResult<Self> {
        Self::from_config(&PathsConfig::default(), None)
    }

    /// Resolve the root path to an absolute canonical string.
    fn resolve_root(root: &str) -> PathResult<String> {
        let root_path = if root == "." || root.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            let path = Path::new(root);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            }
        };

        // Normalize the path (resolve . and ..)
        let normalized = normalize_path_components(&root_path);
        Ok(path_to_forward_slashes(&normalized))
    }

    /// Resolve a mapping value, expanding $ENV and ${config.ref}.
    fn resolve_mapping_value(
        value: &str,
        root: &str,
        full_config: Option<&Config>,
    ) -> PathResult<String> {
        // Handle "." as root
        if value == "." {
            return Ok(root.to_string());
        }

        // Handle $ENV_VAR
        if let Some(env_var) = value.strip_prefix('$') {
            // Check if it's ${config.path} format
            if let Some(config_path) = env_var.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                return Self::resolve_config_ref(config_path, root, full_config);
            }

            // Plain $ENV_VAR
            return match std::env::var(env_var) {
                Ok(val) => {
                    // Normalize the resolved path
                    let path = Path::new(&val);
                    let absolute = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        std::env::current_dir()
                            .unwrap_or_else(|_| PathBuf::from("."))
                            .join(path)
                    };
                    let normalized = normalize_path_components(&absolute);
                    Ok(path_to_forward_slashes(&normalized))
                }
                Err(_) => Err(ToolError::invalid_path(
                    value,
                    &format!("Environment variable {} not set", env_var),
                )),
            };
        }

        // Literal path - make it absolute and normalize
        let path = Path::new(value);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Relative paths are relative to root
            PathBuf::from(root).join(path)
        };
        let normalized = normalize_path_components(&absolute);
        Ok(path_to_forward_slashes(&normalized))
    }

    /// Resolve a ${config.path} reference.
    fn resolve_config_ref(
        config_path: &str,
        root: &str,
        full_config: Option<&Config>,
    ) -> PathResult<String> {
        let config = full_config.ok_or_else(|| {
            ToolError::invalid_path(
                config_path,
                "Config reference requires full config, but none provided",
            )
        })?;

        // Parse config.path format (e.g., "server.media_dir")
        let parts: Vec<&str> = config_path.split('.').collect();
        if parts.len() != 2 {
            return Err(ToolError::invalid_path(
                config_path,
                "Config reference must be in format 'section.field'",
            ));
        }

        let value = match (parts[0], parts[1]) {
            ("server", "media_dir") => config.server.media_dir.to_string_lossy().to_string(),
            ("server", "db_path") => config.server.db_path.to_string_lossy().to_string(),
            ("server", "skills_dir") => config.server.skills_dir.to_string_lossy().to_string(),
            ("server", "log_dir") => config.server.log_dir.to_string_lossy().to_string(),
            _ => {
                return Err(ToolError::invalid_path(
                    config_path,
                    &format!("Unknown config path: {}", config_path),
                ));
            }
        };

        // Make absolute and normalize
        let path = Path::new(&value);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            PathBuf::from(root).join(path)
        };
        let normalized = normalize_path_components(&absolute);
        Ok(path_to_forward_slashes(&normalized))
    }

    /// Normalize a path to canonical internal form.
    ///
    /// This function:
    /// - Resolves prefixes (home:, project:, c:)
    /// - Resolves . and ..
    /// - Converts to forward slashes
    /// - Validates sandbox (no escape above root)
    ///
    /// Returns: Canonical path string (still virtual/internal)
    pub fn normalize(&self, path: &str) -> PathResult<String> {
        // Parse prefix if present
        let (resolved_base, remainder) = self.resolve_prefix(path)?;

        // Build the full path
        let full_path = if let Some(base) = resolved_base {
            if remainder.is_empty() {
                base
            } else {
                format!("{}/{}", base.trim_end_matches('/'), remainder)
            }
        } else {
            // No prefix - relative to root
            if Path::new(remainder).is_absolute() {
                remainder.to_string()
            } else {
                format!("{}/{}", self.root.trim_end_matches('/'), remainder)
            }
        };

        // Normalize path components (resolve . and ..)
        let path_buf = PathBuf::from(&full_path);
        let normalized = normalize_path_components(&path_buf);
        let canonical = path_to_forward_slashes(&normalized);

        // Check sandbox - path must start with root (or be within root)
        self.check_sandbox(&canonical)?;

        Ok(canonical)
    }

    /// Normalize multiple paths.
    pub fn normalize_all(&self, paths: Vec<String>) -> PathResult<Vec<String>> {
        paths.into_iter().map(|p| self.normalize(&p)).collect()
    }

    /// Parse and resolve a prefix from a path.
    ///
    /// Returns (Some(resolved_base), remainder) if a prefix was found,
    /// or (None, original_path) if no prefix.
    fn resolve_prefix<'a>(&self, path: &'a str) -> PathResult<(Option<String>, &'a str)> {
        // Check for prefix pattern: letters followed by colon
        if let Some(colon_pos) = path.find(':') {
            let prefix = &path[..colon_pos];
            let remainder = &path[colon_pos + 1..].trim_start_matches('/');

            // Validate prefix is all lowercase letters (or single letter for Windows drives)
            if prefix.is_empty() {
                return Err(ToolError::invalid_path(path, "Empty prefix before colon"));
            }

            // Check for uppercase in prefix
            if prefix.chars().any(|c: char| c.is_ascii_uppercase()) {
                return Err(ToolError::prefix_not_lowercase(prefix));
            }

            // Check if all characters are ASCII letters
            if !prefix.chars().all(|c: char| c.is_ascii_lowercase()) {
                return Err(ToolError::invalid_path(
                    path,
                    &format!("Prefix '{}' contains non-letter characters", prefix),
                ));
            }

            // Single letter prefix - could be Windows drive
            if prefix.len() == 1 {
                // First check if it's in mappings
                if let Some(base) = self.mappings.get(prefix) {
                    return Ok((Some(base.clone()), remainder));
                }

                // Check for Windows drive mapping
                if self.map_windows_drives {
                    // Map single letter to Windows drive path (e.g., "c" -> "C:/")
                    let drive = prefix.to_ascii_uppercase();
                    let drive_path = format!("{}:/", drive);
                    return Ok((Some(drive_path), remainder));
                }

                // Single letter not in mappings and drive mapping disabled
                return Err(ToolError::unknown_prefix(prefix));
            }

            // Multi-letter prefix - must be in mappings
            if let Some(base) = self.mappings.get(prefix) {
                return Ok((Some(base.clone()), remainder));
            }

            // Unknown prefix
            return Err(ToolError::unknown_prefix(prefix));
        }

        // Check for Windows absolute path (e.g., C:\... or C:/...)
        if path.len() >= 2 {
            let first_char = path.chars().next().unwrap();
            let second_char = path.chars().nth(1).unwrap();
            if first_char.is_ascii_alphabetic() && second_char == ':' {
                // This is a Windows absolute path without our prefix system
                // Just return as-is (no prefix resolution)
                return Ok((None, path));
            }
        }

        // No prefix
        Ok((None, path))
    }

    /// Check that a normalized path doesn't escape the sandbox.
    fn check_sandbox(&self, canonical: &str) -> PathResult<()> {
        // Normalize both for comparison
        let canonical_normalized = canonical.to_lowercase();
        let root_normalized = self.root.to_lowercase();

        // Path must start with root (case-insensitive for cross-platform)
        if !canonical_normalized.starts_with(&root_normalized) {
            return Err(ToolError::sandbox_escape(canonical, &self.root));
        }

        // Additional check: ensure we're not just matching a prefix of a directory name
        // e.g., root = "/home/user" should not match "/home/username"
        if canonical_normalized.len() > root_normalized.len() {
            let next_char = canonical_normalized.chars().nth(root_normalized.len());
            if next_char != Some('/') && next_char.is_some() {
                return Err(ToolError::sandbox_escape(canonical, &self.root));
            }
        }

        Ok(())
    }

    /// Convert canonical path to display format based on style.
    pub fn to_display(&self, canonical: &str) -> String {
        match self.style {
            PathStyle::Relative => {
                // Strip the root prefix to get relative path
                let root_with_slash = if self.root.ends_with('/') {
                    self.root.clone()
                } else {
                    format!("{}/", self.root)
                };

                if let Some(relative) = canonical.strip_prefix(&root_with_slash) {
                    relative.to_string()
                } else if canonical == self.root {
                    ".".to_string()
                } else {
                    canonical.to_string()
                }
            }
            PathStyle::ProjectPrefixed => {
                // Same as relative but with ${project}/ prefix
                let root_with_slash = if self.root.ends_with('/') {
                    self.root.clone()
                } else {
                    format!("{}/", self.root)
                };

                if let Some(relative) = canonical.strip_prefix(&root_with_slash) {
                    format!("${{project}}/{}", relative)
                } else if canonical == self.root {
                    "${project}".to_string()
                } else {
                    canonical.to_string()
                }
            }
        }
    }

    /// Convert canonical path to actual filesystem path.
    /// This is where virtual paths become real OS paths.
    pub fn to_filesystem_path(&self, canonical: &str) -> PathBuf {
        PathBuf::from(canonical)
    }

    /// Convert filesystem path back to canonical form.
    pub fn from_filesystem_path(&self, fs_path: &Path) -> PathResult<String> {
        // Make path absolute if not already
        let absolute = if fs_path.is_absolute() {
            fs_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(fs_path)
        };

        // Normalize and convert to canonical form
        let normalized = normalize_path_components(&absolute);
        let canonical = path_to_forward_slashes(&normalized);

        // Validate sandbox
        self.check_sandbox(&canonical)?;

        Ok(canonical)
    }

    /// Get the resolved root.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Get the path style.
    pub fn style(&self) -> PathStyle {
        self.style
    }

    /// Check if a prefix is defined in mappings.
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.mappings.contains_key(prefix)
    }

    /// Get all defined prefixes.
    pub fn prefixes(&self) -> Vec<&str> {
        self.mappings.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for PathMapper {
    fn default() -> Self {
        Self::new().expect("Failed to create default PathMapper")
    }
}

/// Normalize path components without requiring the file to exist.
/// Handles `.` and `..` components.
fn normalize_path_components(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut components = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                // Windows drive prefix (e.g., C:)
                components.push(Component::Prefix(p));
            }
            Component::RootDir => {
                components.push(Component::RootDir);
            }
            Component::CurDir => {
                // Skip `.` - it refers to current directory
            }
            Component::ParentDir => {
                // Go up one directory if possible
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                } else {
                    // Can't go up from root, keep the component
                    // (this handles edge cases like `/../foo`)
                    components.push(Component::ParentDir);
                }
            }
            Component::Normal(name) => {
                components.push(Component::Normal(name));
            }
        }
    }

    components.iter().collect()
}

/// Convert path to string using forward slashes.
fn path_to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_path_mapper() {
        let mapper = PathMapper::new().unwrap();
        assert!(!mapper.root().is_empty());
    }

    #[test]
    fn test_normalize_relative_path() {
        let mapper = PathMapper::new().unwrap();
        let result = mapper.normalize("src/main.rs").unwrap();
        assert!(result.contains("src/main.rs"));
        assert!(result.starts_with(&*mapper.root()));
    }

    #[test]
    fn test_normalize_with_dot_components() {
        let mapper = PathMapper::new().unwrap();
        let result = mapper.normalize("./src/../src/main.rs").unwrap();
        assert!(result.ends_with("/src/main.rs"));
    }

    #[test]
    fn test_sandbox_escape_blocked() {
        let mapper = PathMapper::new().unwrap();
        // Try to escape with ..
        let result = mapper.normalize("../../../etc/passwd");
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.code, crate::error::ErrorCode::InvalidPath);
        }
    }

    #[test]
    fn test_prefix_must_be_lowercase() {
        let mapper = PathMapper::new().unwrap();
        let result = mapper.normalize("HOME:projects/foo");
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.code, crate::error::ErrorCode::InvalidPrefix);
        }
    }

    #[test]
    fn test_unknown_prefix_rejected() {
        let mapper = PathMapper::new().unwrap();
        let result = mapper.normalize("unknown:path/to/file");
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.code, crate::error::ErrorCode::InvalidPrefix);
        }
    }

    #[test]
    fn test_display_relative_style() {
        let mapper = PathMapper::new().unwrap();
        let canonical = mapper.normalize("src/main.rs").unwrap();
        let display = mapper.to_display(&canonical);
        assert_eq!(display, "src/main.rs");
    }

    #[test]
    fn test_round_trip_filesystem_path() {
        let mapper = PathMapper::new().unwrap();
        let original = "src/main.rs";
        let canonical = mapper.normalize(original).unwrap();
        let fs_path = mapper.to_filesystem_path(&canonical);
        let back = mapper.from_filesystem_path(&fs_path).unwrap();
        assert_eq!(canonical, back);
    }

    #[test]
    fn test_normalize_all() {
        let mapper = PathMapper::new().unwrap();
        let paths = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        let results = mapper.normalize_all(paths).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].ends_with("/src/main.rs"));
        assert!(results[1].ends_with("/src/lib.rs"));
    }

    #[test]
    fn test_config_with_mappings() {
        let mut config = PathsConfig::default();
        config.mappings.insert("test".to_string(), ".".to_string());

        let mapper = PathMapper::from_config(&config, None).unwrap();
        assert!(mapper.has_prefix("test"));
    }

    #[test]
    fn test_normalize_path_components() {
        let path = Path::new("/foo/bar/../baz/./qux");
        let normalized = normalize_path_components(path);
        let result = path_to_forward_slashes(&normalized);
        assert_eq!(result, "/foo/baz/qux");
    }

    #[test]
    fn test_path_to_forward_slashes() {
        let path = Path::new("foo\\bar\\baz");
        let result = path_to_forward_slashes(path);
        assert_eq!(result, "foo/bar/baz");
    }

    #[test]
    fn test_uppercase_prefix_in_config_rejected() {
        let mut config = PathsConfig::default();
        config.mappings.insert("Home".to_string(), ".".to_string());

        let result = PathMapper::from_config(&config, None);
        assert!(result.is_err());
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_drive_mapping() {
        let mut config = PathsConfig::default();
        config.map_windows_drives = true;

        let mapper = PathMapper::from_config(&config, None).unwrap();
        // Note: This test would need a valid Windows path within the sandbox
        // For now, just verify the mapper was created
        assert!(mapper.map_windows_drives);
    }
}
