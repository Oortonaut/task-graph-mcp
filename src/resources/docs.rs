//! Documentation resources - expose markdown files from docs/ directory via MCP resources.
//!
//! Provides access to project documentation through docs:// URIs:
//! - `docs://index` - Lists all available documentation files
//! - `docs://FILENAME.md` - Returns content of specific doc file
//! - `docs://subdir/FILENAME.md` - Supports recursive subdirectories

use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

/// Metadata for a documentation file.
#[derive(Debug, Clone)]
pub struct DocInfo {
    /// Relative path from docs/ directory (e.g., "GATES.md" or "diagrams/README.md")
    pub relative_path: String,
    /// File name only
    pub name: String,
    /// Size in bytes
    pub size: u64,
}

/// Recursively find all markdown files in a directory.
fn find_markdown_files(dir: &Path, base: &Path, files: &mut Vec<DocInfo>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            find_markdown_files(&path, base, files)?;
        } else if path.is_file() {
            // Check if it's a markdown file
            if let Some(ext) = path.extension() {
                if ext == "md" || ext == "markdown" {
                    let relative = path
                        .strip_prefix(base)
                        .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
                        .unwrap_or_else(|_| {
                            path.file_name().unwrap().to_string_lossy().to_string()
                        });

                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

                    files.push(DocInfo {
                        relative_path: relative,
                        name,
                        size,
                    });
                }
            }
        }
    }

    Ok(())
}

/// Validate a doc path to prevent path traversal attacks.
/// Only allows alphanumeric characters, hyphens, underscores, dots, and forward slashes.
fn validate_doc_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(anyhow::anyhow!("Doc path cannot be empty"));
    }

    if path.len() > 256 {
        return Err(anyhow::anyhow!("Doc path too long (max 256 chars)"));
    }

    // Check for path traversal attempts
    if path.contains("..") {
        return Err(anyhow::anyhow!(
            "Invalid doc path: path traversal not allowed"
        ));
    }

    // Only allow safe characters
    if !path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return Err(anyhow::anyhow!(
            "Invalid doc path: only alphanumeric, hyphen, underscore, dot, and slash allowed"
        ));
    }

    // Must end with .md or .markdown
    if !path.ends_with(".md") && !path.ends_with(".markdown") {
        return Err(anyhow::anyhow!(
            "Invalid doc path: must end with .md or .markdown"
        ));
    }

    Ok(())
}

/// List all documentation files as JSON.
pub fn list_docs(docs_dir: Option<&Path>) -> Result<Value> {
    let Some(dir) = docs_dir else {
        return Ok(json!({
            "docs": [],
            "count": 0,
            "docs_dir": null,
            "error": "No docs directory configured"
        }));
    };

    if !dir.exists() {
        return Ok(json!({
            "docs": [],
            "count": 0,
            "docs_dir": dir.display().to_string(),
            "error": "Docs directory does not exist"
        }));
    }

    let mut files = Vec::new();
    find_markdown_files(dir, dir, &mut files)?;

    // Sort by relative path for consistent ordering
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    let docs_list: Vec<Value> = files
        .iter()
        .map(|doc| {
            json!({
                "name": doc.name,
                "path": doc.relative_path,
                "uri": format!("docs://{}", doc.relative_path),
                "size": doc.size,
                "mime_type": "text/markdown",
            })
        })
        .collect();

    let count = docs_list.len();

    Ok(json!({
        "docs": docs_list,
        "count": count,
        "docs_dir": dir.display().to_string(),
    }))
}

/// Get a specific documentation file's content as JSON.
pub fn get_doc_resource(docs_dir: Option<&Path>, path: &str) -> Result<Value> {
    validate_doc_path(path)?;

    let Some(dir) = docs_dir else {
        return Err(anyhow::anyhow!("No docs directory configured"));
    };

    // Construct the full path
    let file_path = dir.join(path);

    // Security check: ensure resolved path is within docs_dir
    if let Ok(canonical_file) = file_path.canonicalize() {
        if let Ok(canonical_dir) = dir.canonicalize() {
            if !canonical_file.starts_with(&canonical_dir) {
                return Err(anyhow::anyhow!("Invalid doc path: outside docs directory"));
            }
        }
    }

    if !file_path.exists() {
        return Err(anyhow::anyhow!("Documentation file not found: {}", path));
    }

    if !file_path.is_file() {
        return Err(anyhow::anyhow!("Not a file: {}", path));
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read doc file: {}", e))?;

    let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);

    let name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(json!({
        "name": name,
        "path": path,
        "uri": format!("docs://{}", path),
        "content": content,
        "size": size,
        "mime_type": "text/markdown",
    }))
}

/// Get resources for all documentation files (for get_resources registration).
pub fn get_doc_resources(docs_dir: Option<&Path>) -> Vec<(String, String, String)> {
    let mut resources = Vec::new();

    let Some(dir) = docs_dir else {
        return resources;
    };

    if !dir.exists() {
        return resources;
    }

    let mut files = Vec::new();
    if find_markdown_files(dir, dir, &mut files).is_ok() {
        for doc in files {
            let uri = format!("docs://{}", doc.relative_path);
            let name = doc.name.clone();
            let description = format!("Documentation: {}", doc.relative_path);
            resources.push((uri, name, description));
        }
    }

    resources
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_validate_doc_path() {
        // Valid paths
        assert!(validate_doc_path("README.md").is_ok());
        assert!(validate_doc_path("GATES.md").is_ok());
        assert!(validate_doc_path("diagrams/README.md").is_ok());
        assert!(validate_doc_path("sub/dir/file.md").is_ok());
        assert!(validate_doc_path("file_name-test.md").is_ok());

        // Invalid paths
        assert!(validate_doc_path("").is_err());
        assert!(validate_doc_path("../etc/passwd").is_err());
        assert!(validate_doc_path("..\\windows\\system32").is_err());
        assert!(validate_doc_path("file.txt").is_err()); // wrong extension
        assert!(validate_doc_path("file<script>.md").is_err()); // invalid chars
    }

    #[test]
    fn test_list_docs_no_dir() {
        let result = list_docs(None).unwrap();
        assert_eq!(result["count"], 0);
        assert!(result["error"].is_string());
    }

    #[test]
    fn test_list_docs_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        // Create test files
        fs::write(docs_path.join("README.md"), "# Readme").unwrap();
        fs::write(docs_path.join("GUIDE.md"), "# Guide").unwrap();

        // Create subdirectory with file
        fs::create_dir(docs_path.join("subdir")).unwrap();
        fs::write(docs_path.join("subdir/NESTED.md"), "# Nested").unwrap();

        let result = list_docs(Some(docs_path)).unwrap();
        assert_eq!(result["count"], 3);

        let docs = result["docs"].as_array().unwrap();
        let paths: Vec<&str> = docs.iter().map(|d| d["path"].as_str().unwrap()).collect();

        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"GUIDE.md"));
        assert!(paths.contains(&"subdir/NESTED.md"));
    }

    #[test]
    fn test_get_doc_resource() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        let content = "# Test Document\n\nThis is a test.";
        fs::write(docs_path.join("TEST.md"), content).unwrap();

        let result = get_doc_resource(Some(docs_path), "TEST.md").unwrap();
        assert_eq!(result["name"], "TEST.md");
        assert_eq!(result["content"], content);
        assert_eq!(result["mime_type"], "text/markdown");
    }

    #[test]
    fn test_get_doc_resource_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = get_doc_resource(Some(temp_dir.path()), "NONEXISTENT.md");
        assert!(result.is_err());
    }
}
