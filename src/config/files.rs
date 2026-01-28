//! File resolution across configuration tiers.
//!
//! Non-YAML files (skills, templates, etc.) use first-found-wins resolution
//! from highest tier to lowest.

use super::loader::{ConfigLoader, ConfigTier};
use std::path::PathBuf;

/// Source of a resolved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSource {
    /// File was found in user config directory
    User,
    /// File was found in project config directory
    Project,
    /// File was found in deprecated project config directory
    ProjectDeprecated,
    /// File is embedded in the binary
    Embedded,
}

impl std::fmt::Display for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileSource::User => write!(f, "user"),
            FileSource::Project => write!(f, "project"),
            FileSource::ProjectDeprecated => write!(f, "project (deprecated)"),
            FileSource::Embedded => write!(f, "embedded"),
        }
    }
}

/// A resolved file with its content and metadata.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    /// The file content
    pub content: String,
    /// The path where the file was found (None for embedded)
    pub path: Option<PathBuf>,
    /// The source tier
    pub source: FileSource,
}

impl ResolvedFile {
    /// Create a new resolved file from disk.
    pub fn from_disk(content: String, path: PathBuf, source: FileSource) -> Self {
        Self {
            content,
            path: Some(path),
            source,
        }
    }

    /// Create a new resolved file from embedded content.
    pub fn from_embedded(content: &'static str) -> Self {
        Self {
            content: content.to_string(),
            path: None,
            source: FileSource::Embedded,
        }
    }
}

impl ConfigLoader {
    /// Find a file by relative path, searching from highest tier to lowest.
    ///
    /// Returns the first file found, or None if not found in any tier.
    pub fn find_file(&self, relative_path: &str) -> Option<ResolvedFile> {
        // Tier 3: User directory (highest priority for files)
        if let Some(ref user_dir) = self.paths.user_dir {
            let path = user_dir.join(relative_path);
            if path.exists()
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                return Some(ResolvedFile::from_disk(content, path, FileSource::User));
            }
        }

        // Tier 2: Project directory (new location)
        if let Some(ref project_dir) = self.paths.project_dir
            && project_dir.exists()
        {
            let path = project_dir.join(relative_path);
            if path.exists()
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                return Some(ResolvedFile::from_disk(content, path, FileSource::Project));
            }
        }

        // Tier 2b: Project directory (deprecated location)
        if let Some(ref project_dir) = self.paths.project_dir_deprecated
            && project_dir.exists()
        {
            let path = project_dir.join(relative_path);
            if path.exists()
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                return Some(ResolvedFile::from_disk(
                    content,
                    path,
                    FileSource::ProjectDeprecated,
                ));
            }
        }

        // Tier 1: Embedded content is handled separately by callers
        None
    }

    /// Find a file, returning the path if found on disk.
    pub fn find_file_path(&self, relative_path: &str) -> Option<PathBuf> {
        // Tier 3: User directory
        if let Some(ref user_dir) = self.paths.user_dir {
            let path = user_dir.join(relative_path);
            if path.exists() {
                return Some(path);
            }
        }

        // Tier 2: Project directory (new location)
        if let Some(ref project_dir) = self.paths.project_dir
            && project_dir.exists()
        {
            let path = project_dir.join(relative_path);
            if path.exists() {
                return Some(path);
            }
        }

        // Tier 2b: Project directory (deprecated location)
        if let Some(ref project_dir) = self.paths.project_dir_deprecated
            && project_dir.exists()
        {
            let path = project_dir.join(relative_path);
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    /// Check if a file exists in any tier.
    pub fn file_exists(&self, relative_path: &str) -> bool {
        self.find_file_path(relative_path).is_some()
    }

    /// Get the tier where a file would be found.
    pub fn file_tier(&self, relative_path: &str) -> Option<ConfigTier> {
        // Tier 3: User directory
        if let Some(ref user_dir) = self.paths.user_dir
            && user_dir.join(relative_path).exists()
        {
            return Some(ConfigTier::User);
        }

        // Tier 2: Project directory
        if let Some(ref project_dir) = self.paths.project_dir
            && project_dir.exists()
            && project_dir.join(relative_path).exists()
        {
            return Some(ConfigTier::Project);
        }

        // Tier 2b: Deprecated project directory
        if let Some(ref project_dir) = self.paths.project_dir_deprecated
            && project_dir.exists()
            && project_dir.join(relative_path).exists()
        {
            return Some(ConfigTier::Project);
        }

        None
    }

    /// List files in a directory across all tiers.
    ///
    /// Returns a deduplicated list where higher-tier files shadow lower-tier ones.
    pub fn list_files(&self, relative_dir: &str) -> Vec<(String, FileSource)> {
        use std::collections::HashMap;

        let mut files: HashMap<String, FileSource> = HashMap::new();

        // Scan from lowest to highest tier (higher tiers override)

        // Tier 2b: Deprecated project directory
        if let Some(ref project_dir) = self.paths.project_dir_deprecated {
            let dir = project_dir.join(relative_dir);
            if dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&dir)
            {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.insert(name.to_string(), FileSource::ProjectDeprecated);
                    }
                }
            }
        }

        // Tier 2: Project directory (new location)
        if let Some(ref project_dir) = self.paths.project_dir {
            let dir = project_dir.join(relative_dir);
            if dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&dir)
            {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.insert(name.to_string(), FileSource::Project);
                    }
                }
            }
        }

        // Tier 3: User directory
        if let Some(ref user_dir) = self.paths.user_dir {
            let dir = user_dir.join(relative_dir);
            if dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&dir)
            {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.insert(name.to_string(), FileSource::User);
                    }
                }
            }
        }

        let mut result: Vec<_> = files.into_iter().collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigPaths;
    use tempfile::TempDir;

    fn create_test_loader(temp: &TempDir) -> ConfigLoader {
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();

        let paths = ConfigPaths::with_dirs(Some(project_dir), Some(user_dir));
        ConfigLoader::load_with_paths(paths).unwrap()
    }

    #[test]
    fn test_find_file_user_priority() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();

        // Create file in both locations
        std::fs::write(project_dir.join("test.txt"), "project content").unwrap();
        std::fs::write(user_dir.join("test.txt"), "user content").unwrap();

        let paths = ConfigPaths::with_dirs(Some(project_dir), Some(user_dir));
        let loader = ConfigLoader::load_with_paths(paths).unwrap();

        let file = loader.find_file("test.txt").unwrap();
        assert_eq!(file.content, "user content");
        assert_eq!(file.source, FileSource::User);
    }

    #[test]
    fn test_find_file_project_fallback() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();

        // Create file only in project
        std::fs::write(project_dir.join("test.txt"), "project content").unwrap();

        let paths = ConfigPaths::with_dirs(Some(project_dir), Some(user_dir));
        let loader = ConfigLoader::load_with_paths(paths).unwrap();

        let file = loader.find_file("test.txt").unwrap();
        assert_eq!(file.content, "project content");
        assert_eq!(file.source, FileSource::Project);
    }

    #[test]
    fn test_find_file_not_found() {
        let temp = TempDir::new().unwrap();
        let loader = create_test_loader(&temp);

        assert!(loader.find_file("nonexistent.txt").is_none());
    }

    #[test]
    fn test_list_files_deduplication() {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join("task-graph");
        let user_dir = temp.path().join("user");
        let project_skills = project_dir.join("skills");
        let user_skills = user_dir.join("skills");
        std::fs::create_dir_all(&project_skills).unwrap();
        std::fs::create_dir_all(&user_skills).unwrap();

        // Create overlapping files
        std::fs::write(project_skills.join("shared.md"), "project").unwrap();
        std::fs::write(project_skills.join("project-only.md"), "project").unwrap();
        std::fs::write(user_skills.join("shared.md"), "user").unwrap();
        std::fs::write(user_skills.join("user-only.md"), "user").unwrap();

        let paths = ConfigPaths::with_dirs(Some(project_dir), Some(user_dir));
        let loader = ConfigLoader::load_with_paths(paths).unwrap();

        let files = loader.list_files("skills");

        // Should have 3 unique files
        assert_eq!(files.len(), 3);

        // shared.md should be from user (higher priority)
        let shared = files.iter().find(|(name, _)| name == "shared.md").unwrap();
        assert_eq!(shared.1, FileSource::User);
    }
}
