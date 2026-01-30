//! Documentation resources - expose markdown files from docs/ directory via MCP resources.
//!
//! Provides access to project documentation through docs:// URIs:
//! - `docs://index` - Lists all available documentation files
//! - `docs://search/{query}` - Full-text search across all documentation files
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
            if let Some(ext) = path.extension()
                && (ext == "md" || ext == "markdown")
            {
                let relative = path
                    .strip_prefix(base)
                    .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
                    .unwrap_or_else(|_| path.file_name().unwrap().to_string_lossy().to_string());

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
    if let Ok(canonical_file) = file_path.canonicalize()
        && let Ok(canonical_dir) = dir.canonicalize()
        && !canonical_file.starts_with(&canonical_dir)
    {
        return Err(anyhow::anyhow!("Invalid doc path: outside docs directory"));
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

/// A single match within a documentation file.
#[derive(Debug, Clone)]
struct DocMatch {
    /// Line number (1-based) where the match was found
    line_number: usize,
    /// The matching line (trimmed)
    line_text: String,
    /// Context: a few lines around the match
    context: String,
}

/// A search result for a documentation file.
#[derive(Debug, Clone)]
struct DocSearchResult {
    /// Relative path from docs/ directory
    relative_path: String,
    /// File name only
    name: String,
    /// Size in bytes
    size: u64,
    /// Whether the filename itself matched the query
    name_match: bool,
    /// Matched lines within the file content
    matches: Vec<DocMatch>,
    /// Total number of matches in this file
    match_count: usize,
}

/// Extract a context snippet around a line in the content.
/// Returns up to `context_lines` lines before and after the matched line.
fn extract_context(lines: &[&str], line_idx: usize, context_lines: usize) -> String {
    let start = line_idx.saturating_sub(context_lines);
    let end = (line_idx + context_lines + 1).min(lines.len());
    lines[start..end].join("\n")
}

/// Search documentation files for a query string.
///
/// Performs case-insensitive substring matching across all markdown files.
/// Searches both filenames and file content. Results are ranked by relevance:
/// filename matches first, then by number of content matches.
///
/// The query is split into terms (space-separated) and all terms must appear
/// in the file (either in the filename or content) for it to be a match.
///
/// Supports pagination via `limit` and `offset` parameters.
pub fn search_docs(
    docs_dir: Option<&Path>,
    query: &str,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Value> {
    let Some(dir) = docs_dir else {
        return Ok(json!({
            "query": query,
            "results": [],
            "result_count": 0,
            "total_matches": 0,
            "has_more": false,
            "error": "No docs directory configured"
        }));
    };

    if !dir.exists() {
        return Ok(json!({
            "query": query,
            "results": [],
            "result_count": 0,
            "total_matches": 0,
            "has_more": false,
            "docs_dir": dir.display().to_string(),
            "error": "Docs directory does not exist"
        }));
    }

    if query.trim().is_empty() {
        return Err(anyhow::anyhow!("Search query cannot be empty"));
    }

    let limit = limit.unwrap_or(20).min(100);
    let offset = offset.unwrap_or(0);
    let context_lines = 2;
    let max_matches_per_file = 5;

    // Normalize query: split into lowercase terms
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    if terms.is_empty() {
        return Err(anyhow::anyhow!("Search query cannot be empty"));
    }

    // Find all markdown files
    let mut files = Vec::new();
    find_markdown_files(dir, dir, &mut files)?;

    // Search each file
    let mut results: Vec<DocSearchResult> = Vec::new();

    for doc in &files {
        let file_path = dir.join(&doc.relative_path);
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue, // Skip unreadable files
        };

        let content_lower = content.to_lowercase();
        let name_lower = doc.name.to_lowercase();
        let path_lower = doc.relative_path.to_lowercase();

        // Check if ALL terms appear somewhere in the file (name or content)
        let all_terms_present = terms.iter().all(|term| {
            name_lower.contains(term) || path_lower.contains(term) || content_lower.contains(term)
        });

        if !all_terms_present {
            continue;
        }

        // Check if filename matches any term
        let name_match = terms
            .iter()
            .any(|term| name_lower.contains(term) || path_lower.contains(term));

        // Find matching lines in content
        let lines: Vec<&str> = content.lines().collect();
        let mut doc_matches = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            let line_lower = line.to_lowercase();
            // A line matches if it contains any of the search terms
            if terms.iter().any(|term| line_lower.contains(term)) {
                let context = extract_context(&lines, idx, context_lines);
                doc_matches.push(DocMatch {
                    line_number: idx + 1,
                    line_text: line.trim().to_string(),
                    context,
                });

                if doc_matches.len() >= max_matches_per_file {
                    break;
                }
            }
        }

        let match_count = if doc_matches.len() >= max_matches_per_file {
            // Count all matches if we hit the per-file limit
            lines
                .iter()
                .filter(|line| {
                    let ll = line.to_lowercase();
                    terms.iter().any(|term| ll.contains(term))
                })
                .count()
        } else {
            doc_matches.len()
        };

        // Only include files that have at least one match (name or content)
        if name_match || !doc_matches.is_empty() {
            results.push(DocSearchResult {
                relative_path: doc.relative_path.clone(),
                name: doc.name.clone(),
                size: doc.size,
                name_match,
                matches: doc_matches,
                match_count,
            });
        }
    }

    // Sort results: filename matches first, then by match count (descending)
    results.sort_by(|a, b| {
        b.name_match
            .cmp(&a.name_match)
            .then_with(|| b.match_count.cmp(&a.match_count))
    });

    let total_results = results.len();
    let total_matches: usize = results.iter().map(|r| r.match_count).sum();

    // Apply pagination
    let paginated: Vec<&DocSearchResult> = results.iter().skip(offset).take(limit + 1).collect();
    let has_more = paginated.len() > limit;
    let paginated: Vec<&DocSearchResult> = paginated.into_iter().take(limit).collect();

    // Convert to JSON
    let results_json: Vec<Value> = paginated
        .iter()
        .map(|r| {
            let matches_json: Vec<Value> = r
                .matches
                .iter()
                .map(|m| {
                    json!({
                        "line": m.line_number,
                        "text": m.line_text,
                        "context": m.context,
                    })
                })
                .collect();

            json!({
                "name": r.name,
                "path": r.relative_path,
                "uri": format!("docs://{}", r.relative_path),
                "size": r.size,
                "name_match": r.name_match,
                "match_count": r.match_count,
                "matches": matches_json,
            })
        })
        .collect();

    let result_count = results_json.len();

    Ok(json!({
        "query": query,
        "results": results_json,
        "result_count": result_count,
        "total_files_matched": total_results,
        "total_matches": total_matches,
        "has_more": has_more,
        "offset": offset,
        "limit": limit,
        "docs_dir": dir.display().to_string(),
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

    #[test]
    fn test_search_docs_no_dir() {
        let result = search_docs(None, "test", None, None).unwrap();
        assert_eq!(result["result_count"], 0);
        assert!(result["error"].is_string());
    }

    #[test]
    fn test_search_docs_empty_query() {
        let temp_dir = TempDir::new().unwrap();
        let result = search_docs(Some(temp_dir.path()), "", None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_docs_finds_content() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        fs::write(
            docs_path.join("GATES.md"),
            "# Gates\n\nGates are quality checkpoints.\nThey verify task completion.",
        )
        .unwrap();
        fs::write(
            docs_path.join("DESIGN.md"),
            "# Design\n\nArchitecture overview.\nSystem design document.",
        )
        .unwrap();

        // Search for content in GATES.md
        let result = search_docs(Some(docs_path), "checkpoints", None, None).unwrap();
        assert_eq!(result["result_count"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["name"], "GATES.md");
        assert!(results[0]["match_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_search_docs_filename_match() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        fs::write(docs_path.join("GATES.md"), "# Gates\n\nContent here.").unwrap();
        fs::write(docs_path.join("DESIGN.md"), "# Design\n\nOther content.").unwrap();

        // Search for filename
        let result = search_docs(Some(docs_path), "gates", None, None).unwrap();
        assert_eq!(result["result_count"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["name"], "GATES.md");
        assert!(results[0]["name_match"].as_bool().unwrap());
    }

    #[test]
    fn test_search_docs_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        fs::write(
            docs_path.join("TEST.md"),
            "# Test\n\nThis has UPPERCASE and lowercase content.",
        )
        .unwrap();

        // Case-insensitive search
        let result = search_docs(Some(docs_path), "uppercase", None, None).unwrap();
        assert_eq!(result["result_count"], 1);

        let result = search_docs(Some(docs_path), "UPPERCASE", None, None).unwrap();
        assert_eq!(result["result_count"], 1);
    }

    #[test]
    fn test_search_docs_multi_term() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        fs::write(
            docs_path.join("A.md"),
            "# Alpha\n\nThis has alpha and beta content.",
        )
        .unwrap();
        fs::write(
            docs_path.join("B.md"),
            "# Beta\n\nThis only has beta content.",
        )
        .unwrap();

        // Multi-term: both must be present
        let result = search_docs(Some(docs_path), "alpha beta", None, None).unwrap();
        assert_eq!(result["result_count"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["name"], "A.md");
    }

    #[test]
    fn test_search_docs_pagination() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        // Create several matching files
        for i in 0..5 {
            fs::write(
                docs_path.join(format!("DOC{}.md", i)),
                format!("# Doc {}\n\nSearchable content in doc {}.", i, i),
            )
            .unwrap();
        }

        // Search with limit
        let result = search_docs(Some(docs_path), "searchable", Some(2), None).unwrap();
        assert_eq!(result["result_count"], 2);
        assert!(result["has_more"].as_bool().unwrap());

        // Search with offset
        let result = search_docs(Some(docs_path), "searchable", Some(2), Some(2)).unwrap();
        assert_eq!(result["result_count"], 2);
        assert!(result["has_more"].as_bool().unwrap());

        // Search with offset past results
        let result = search_docs(Some(docs_path), "searchable", Some(2), Some(4)).unwrap();
        assert_eq!(result["result_count"], 1);
        assert!(!result["has_more"].as_bool().unwrap());
    }

    #[test]
    fn test_search_docs_with_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let docs_path = temp_dir.path();

        fs::write(docs_path.join("ROOT.md"), "# Root\n\nRoot level content.").unwrap();
        fs::create_dir(docs_path.join("subdir")).unwrap();
        fs::write(
            docs_path.join("subdir/NESTED.md"),
            "# Nested\n\nNested searchable content.",
        )
        .unwrap();

        let result = search_docs(Some(docs_path), "searchable", None, None).unwrap();
        assert_eq!(result["result_count"], 1);
        let results = result["results"].as_array().unwrap();
        assert_eq!(results[0]["path"], "subdir/NESTED.md");
    }
}
