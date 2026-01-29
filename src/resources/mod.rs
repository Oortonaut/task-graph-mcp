//! MCP resource implementations.

pub mod agents;
pub mod config;
pub mod docs;
pub mod files;
pub mod skills;
pub mod stats;
pub mod tasks;
pub mod workflows;

use crate::config::AppConfig;
use crate::db::Database;
use anyhow::Result;
use rmcp::model::{Annotated, RawResource, RawResourceTemplate, Resource, ResourceTemplate};
use serde_json::Value;
use std::sync::Arc;

/// Resource handler that processes MCP resource requests.
pub struct ResourceHandler {
    pub db: Arc<Database>,
    /// Consolidated application configuration.
    pub config: AppConfig,
    /// Directory for skill overrides (e.g., `.task-graph/skills/`)
    pub skills_dir: Option<std::path::PathBuf>,
    /// Directory containing documentation markdown files (e.g., `docs/`)
    pub docs_dir: Option<std::path::PathBuf>,
}

impl ResourceHandler {
    pub fn new(db: Arc<Database>, config: AppConfig) -> Self {
        Self {
            db,
            config,
            skills_dir: None,
            docs_dir: None,
        }
    }

    /// Set the skills override directory.
    pub fn with_skills_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.skills_dir = Some(dir);
        self
    }

    /// Set the documentation directory.
    pub fn with_docs_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.docs_dir = Some(dir);
        self
    }

    /// Get all available resource templates.
    pub fn get_resource_templates(&self) -> Vec<ResourceTemplate> {
        vec![
            // Query resources (live DB queries)
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/all".into(),
                    name: "All Tasks".into(),
                    title: None,
                    description: Some("Full task graph with dependencies".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/ready".into(),
                    name: "Ready Tasks".into(),
                    title: None,
                    description: Some("Tasks ready to claim".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/blocked".into(),
                    name: "Blocked Tasks".into(),
                    title: None,
                    description: Some("Tasks blocked by dependencies".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/claimed".into(),
                    name: "Claimed Tasks".into(),
                    title: None,
                    description: Some("All claimed tasks".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/agent/{agent_id}".into(),
                    name: "Agent Tasks".into(),
                    title: None,
                    description: Some("Tasks owned by an agent".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://tasks/tree/{task_id}".into(),
                    name: "Task Tree".into(),
                    title: None,
                    description: Some("Task with all descendants".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://files/marks".into(),
                    name: "File Marks".into(),
                    title: None,
                    description: Some("All advisory file marks".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://agents/all".into(),
                    name: "All Agents".into(),
                    title: None,
                    description: Some("Registered agents".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "query://stats/summary".into(),
                    name: "Stats Summary".into(),
                    title: None,
                    description: Some("Aggregate statistics".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            // Config resources
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "config://current".into(),
                    name: "Current Configuration".into(),
                    title: None,
                    description: Some("All configuration (states, phases, dependencies, tags) in one response".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "config://states".into(),
                    name: "States Configuration".into(),
                    title: None,
                    description: Some("Task state definitions and transitions".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "config://phases".into(),
                    name: "Phases Configuration".into(),
                    title: None,
                    description: Some("Work phase definitions".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "config://dependencies".into(),
                    name: "Dependencies Configuration".into(),
                    title: None,
                    description: Some("Dependency type definitions".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "config://tags".into(),
                    name: "Tags Configuration".into(),
                    title: None,
                    description: Some("Tag definitions and categories".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            // Docs resources (reference content: docs, skills, workflows)
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://skills/list".into(),
                    name: "Available Skills".into(),
                    title: None,
                    description: Some("List all bundled task-graph skills".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://skills/{name}".into(),
                    name: "Skill Content".into(),
                    title: None,
                    description: Some("Get a specific skill (basics, coordinator, worker, reporting, migration, repair)".into()),
                    mime_type: Some("text/markdown".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://workflows/list".into(),
                    name: "Available Workflows".into(),
                    title: None,
                    description: Some("List all available workflow topologies with descriptions".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://workflows/{name}".into(),
                    name: "Workflow Details".into(),
                    title: None,
                    description: Some("Get detailed information about a specific workflow (states, phases, settings)".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://index".into(),
                    name: "Documentation Index".into(),
                    title: None,
                    description: Some("List all available documentation files".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://search/{query}".into(),
                    name: "Documentation Search".into(),
                    title: None,
                    description: Some(
                        "Full-text search across all documentation files. \
                         Supports multi-term queries (space-separated, all terms must match). \
                         Case-insensitive. Returns matching files with line-level context snippets."
                            .into(),
                    ),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "docs://{path}".into(),
                    name: "Documentation File".into(),
                    title: None,
                    description: Some("Get content of a specific documentation file (e.g., docs://GATES.md)".into()),
                    mime_type: Some("text/markdown".into()),
                    icons: None,
                },
                None,
            ),
        ]
    }

    /// Get all concrete resources (those without template parameters).
    /// These are resources that can be directly accessed without any parameters.
    pub fn get_resources(&self) -> Vec<Resource> {
        vec![
            // Query resources (live DB queries)
            Annotated::new(
                RawResource {
                    uri: "query://tasks/all".into(),
                    name: "All Tasks".into(),
                    title: None,
                    description: Some("Full task graph with dependencies".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://tasks/ready".into(),
                    name: "Ready Tasks".into(),
                    title: None,
                    description: Some("Tasks ready to claim".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://tasks/blocked".into(),
                    name: "Blocked Tasks".into(),
                    title: None,
                    description: Some("Tasks blocked by dependencies".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://tasks/claimed".into(),
                    name: "Claimed Tasks".into(),
                    title: None,
                    description: Some("All claimed tasks".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://files/marks".into(),
                    name: "File Marks".into(),
                    title: None,
                    description: Some("All advisory file marks".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://agents/all".into(),
                    name: "All Agents".into(),
                    title: None,
                    description: Some("Registered agents".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "query://stats/summary".into(),
                    name: "Stats Summary".into(),
                    title: None,
                    description: Some("Aggregate statistics".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            // Config resources
            Annotated::new(
                RawResource {
                    uri: "config://current".into(),
                    name: "Current Configuration".into(),
                    title: None,
                    description: Some(
                        "All configuration (states, phases, dependencies, tags) in one response"
                            .into(),
                    ),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "config://states".into(),
                    name: "States Configuration".into(),
                    title: None,
                    description: Some("Task state definitions and transitions".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "config://phases".into(),
                    name: "Phases Configuration".into(),
                    title: None,
                    description: Some("Work phase definitions".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "config://dependencies".into(),
                    name: "Dependencies Configuration".into(),
                    title: None,
                    description: Some("Dependency type definitions".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "config://tags".into(),
                    name: "Tags Configuration".into(),
                    title: None,
                    description: Some("Tag definitions and categories".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            // Docs resources (reference content: docs, skills, workflows)
            Annotated::new(
                RawResource {
                    uri: "docs://skills/list".into(),
                    name: "Available Skills".into(),
                    title: None,
                    description: Some("List all bundled task-graph skills".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "docs://workflows/list".into(),
                    name: "Available Workflows".into(),
                    title: None,
                    description: Some(
                        "List all available workflow topologies with descriptions".into(),
                    ),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "docs://index".into(),
                    name: "Documentation Index".into(),
                    title: None,
                    description: Some("List all available documentation files".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
        ]
    }

    /// Read a resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<Value> {
        if uri.starts_with("query://") {
            self.read_query_resource(uri).await
        } else if uri.starts_with("config://") {
            self.read_config_resource(uri).await
        } else if uri.starts_with("docs://") {
            self.read_docs_resource(uri).await
        } else {
            Err(anyhow::anyhow!("Unknown resource URI: {}", uri))
        }
    }

    async fn read_query_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("query://").unwrap_or("");

        match path {
            // Tasks
            "tasks/all" => tasks::get_all_tasks(&self.db),
            "tasks/ready" => {
                tasks::get_ready_tasks(&self.db, &self.config.states, &self.config.deps)
            }
            "tasks/blocked" => {
                tasks::get_blocked_tasks(&self.db, &self.config.states, &self.config.deps)
            }
            "tasks/claimed" => tasks::get_claimed_tasks(&self.db, None),
            _ if path.starts_with("tasks/agent/") => {
                let agent_id = path.strip_prefix("tasks/agent/").unwrap();
                tasks::get_claimed_tasks(&self.db, Some(agent_id))
            }
            _ if path.starts_with("tasks/tree/") => {
                let task_id = path.strip_prefix("tasks/tree/").unwrap();
                tasks::get_task_tree(&self.db, task_id)
            }
            // Files
            "files/marks" => files::get_all_file_locks(&self.db),
            // Agents
            "agents/all" => agents::get_all_workers(&self.db),
            // Stats
            "stats/summary" => stats::get_stats_summary(&self.db, &self.config.states),
            _ => Err(anyhow::anyhow!("Unknown query resource: {}", path)),
        }
    }

    async fn read_config_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("config://").unwrap_or("");

        match path {
            "current" => {
                // Return all configuration in one response
                let states = config::get_states_config(&self.config.states)?;
                let phases = config::get_phases_config(&self.config.phases)?;
                let dependencies = config::get_dependencies_config(&self.config.deps)?;
                let tags = config::get_tags_config(&self.config.tags)?;

                Ok(serde_json::json!({
                    "states": states,
                    "phases": phases,
                    "dependencies": dependencies,
                    "tags": tags,
                }))
            }
            "states" => config::get_states_config(&self.config.states),
            "phases" => config::get_phases_config(&self.config.phases),
            "dependencies" => config::get_dependencies_config(&self.config.deps),
            "tags" => config::get_tags_config(&self.config.tags),
            _ => Err(anyhow::anyhow!("Unknown config resource: {}", path)),
        }
    }

    async fn read_docs_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("docs://").unwrap_or("");
        let skills_dir = self.skills_dir.as_deref();
        let docs_dir = self.docs_dir.as_deref();

        match path {
            // Skills
            "skills/list" => skills::list_skills(skills_dir),
            _ if path.starts_with("skills/") => {
                let name = path.strip_prefix("skills/").unwrap();
                skills::get_skill_resource(skills_dir, name)
            }
            // Workflows
            "workflows/list" => workflows::list_workflows(&self.config.workflows),
            _ if path.starts_with("workflows/") => {
                let name = path.strip_prefix("workflows/").unwrap();
                workflows::get_workflow(&self.config.workflows, name)
            }
            // Documentation files
            "index" => docs::list_docs(docs_dir),
            _ if path.starts_with("search/") => {
                let query = path.strip_prefix("search/").unwrap_or("");
                // URL-decode the query string
                let query = urlencoding::decode(query)
                    .unwrap_or_else(|_| query.into())
                    .into_owned();
                docs::search_docs(docs_dir, &query, None, None)
            }
            // Individual doc file (e.g., "GATES.md" or "diagrams/README.md")
            doc_path => docs::get_doc_resource(docs_dir, doc_path),
        }
    }
}
