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
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "tasks://all".into(),
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
                    uri_template: "tasks://ready".into(),
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
                    uri_template: "tasks://blocked".into(),
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
                    uri_template: "tasks://claimed".into(),
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
                    uri_template: "tasks://agent/{agent_id}".into(),
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
                    uri_template: "tasks://tree/{task_id}".into(),
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
                    uri_template: "files://marks".into(),
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
                    uri_template: "agents://all".into(),
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
                    uri_template: "plan://acp".into(),
                    name: "ACP Plan".into(),
                    title: None,
                    description: Some("ACP-compatible plan export".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "stats://summary".into(),
                    name: "Stats Summary".into(),
                    title: None,
                    description: Some("Aggregate statistics".into()),
                    mime_type: Some("application/json".into()),
                    icons: None,
                },
                None,
            ),
            // Skills resources
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "skills://list".into(),
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
                    uri_template: "skills://{name}".into(),
                    name: "Skill Content".into(),
                    title: None,
                    description: Some("Get a specific skill (basics, coordinator, worker, reporting, migration, repair)".into()),
                    mime_type: Some("text/markdown".into()),
                    icons: None,
                },
                None,
            ),
            // Workflow resources
            Annotated::new(
                RawResourceTemplate {
                    uri_template: "workflows://list".into(),
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
                    uri_template: "workflows://{name}".into(),
                    name: "Workflow Details".into(),
                    title: None,
                    description: Some("Get detailed information about a specific workflow (states, phases, settings)".into()),
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
            // Documentation resources
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
            Annotated::new(
                RawResource {
                    uri: "tasks://all".into(),
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
                    uri: "tasks://ready".into(),
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
                    uri: "tasks://blocked".into(),
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
                    uri: "tasks://claimed".into(),
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
                    uri: "files://marks".into(),
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
                    uri: "agents://all".into(),
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
                    uri: "plan://acp".into(),
                    name: "ACP Plan".into(),
                    title: None,
                    description: Some("ACP-compatible plan export".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            ),
            Annotated::new(
                RawResource {
                    uri: "stats://summary".into(),
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
            Annotated::new(
                RawResource {
                    uri: "skills://list".into(),
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
                    uri: "workflows://list".into(),
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
            // Documentation resources
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
        // Parse the URI
        if uri.starts_with("tasks://") {
            self.read_tasks_resource(uri).await
        } else if uri.starts_with("files://") {
            self.read_files_resource(uri).await
        } else if uri.starts_with("agents://") {
            self.read_agents_resource(uri).await
        } else if uri.starts_with("plan://") {
            self.read_plan_resource(uri).await
        } else if uri.starts_with("stats://") {
            self.read_stats_resource(uri).await
        } else if uri.starts_with("skills://") {
            self.read_skills_resource(uri).await
        } else if uri.starts_with("config://") {
            self.read_config_resource(uri).await
        } else if uri.starts_with("workflows://") {
            self.read_workflows_resource(uri).await
        } else if uri.starts_with("docs://") {
            self.read_docs_resource(uri).await
        } else {
            Err(anyhow::anyhow!("Unknown resource URI: {}", uri))
        }
    }

    async fn read_tasks_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("tasks://").unwrap_or("");

        match path {
            "all" => tasks::get_all_tasks(&self.db),
            "ready" => tasks::get_ready_tasks(&self.db, &self.config.states, &self.config.deps),
            "blocked" => tasks::get_blocked_tasks(&self.db, &self.config.states, &self.config.deps),
            "claimed" => tasks::get_claimed_tasks(&self.db, None),
            _ if path.starts_with("agent/") => {
                let agent_id = path.strip_prefix("agent/").unwrap();
                tasks::get_claimed_tasks(&self.db, Some(agent_id))
            }
            _ if path.starts_with("tree/") => {
                let task_id = path.strip_prefix("tree/").unwrap();
                tasks::get_task_tree(&self.db, task_id)
            }
            _ => Err(anyhow::anyhow!("Unknown tasks resource: {}", path)),
        }
    }

    async fn read_files_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("files://").unwrap_or("");

        match path {
            "marks" => files::get_all_file_locks(&self.db),
            _ => Err(anyhow::anyhow!("Unknown files resource: {}", path)),
        }
    }

    async fn read_agents_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("agents://").unwrap_or("");

        match path {
            "all" => agents::get_all_workers(&self.db),
            _ => Err(anyhow::anyhow!("Unknown agents resource: {}", path)),
        }
    }

    async fn read_plan_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("plan://").unwrap_or("");

        match path {
            "acp" => stats::get_acp_plan(&self.db),
            _ => Err(anyhow::anyhow!("Unknown plan resource: {}", path)),
        }
    }

    async fn read_stats_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("stats://").unwrap_or("");

        match path {
            "summary" => stats::get_stats_summary(&self.db, &self.config.states),
            _ => Err(anyhow::anyhow!("Unknown stats resource: {}", path)),
        }
    }

    async fn read_skills_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("skills://").unwrap_or("");
        let skills_dir = self.skills_dir.as_deref();

        match path {
            "list" => skills::list_skills(skills_dir),
            name => skills::get_skill_resource(skills_dir, name),
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

    async fn read_workflows_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("workflows://").unwrap_or("");

        match path {
            "list" => workflows::list_workflows(&self.config.workflows),
            name => workflows::get_workflow(&self.config.workflows, name),
        }
    }

    async fn read_docs_resource(&self, uri: &str) -> Result<Value> {
        let path = uri.strip_prefix("docs://").unwrap_or("");
        let docs_dir = self.docs_dir.as_deref();

        match path {
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
