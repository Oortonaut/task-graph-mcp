//! MCP tool implementations.

pub mod agents;
pub mod attachments;
pub mod claiming;
pub mod deps;
pub mod files;
pub mod query;
pub mod schema;
pub mod search;
pub mod skills;
pub mod tasks;
pub mod tracking;

use crate::config::{
    AttachmentsConfig, AutoAdvanceConfig, DependenciesConfig, IdsConfig, PhasesConfig, Prompts,
    ServerPaths, StatesConfig, TagsConfig, workflows::WorkflowsConfig,
};
use crate::db::Database;
use crate::error::ToolError;
use crate::format::{OutputFormat, ToolResult};
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Tool handler that processes MCP tool calls.
pub struct ToolHandler {
    pub db: Arc<Database>,
    pub media_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub server_paths: Arc<ServerPaths>,
    pub prompts: Arc<Prompts>,
    pub states_config: Arc<StatesConfig>,
    pub phases_config: Arc<PhasesConfig>,
    pub deps_config: Arc<DependenciesConfig>,
    pub auto_advance: Arc<AutoAdvanceConfig>,
    pub attachments_config: Arc<AttachmentsConfig>,
    pub tags_config: Arc<TagsConfig>,
    pub ids_config: Arc<IdsConfig>,
    pub workflows: Arc<WorkflowsConfig>,
    pub default_format: OutputFormat,
    pub path_mapper: Arc<crate::paths::PathMapper>,
}

impl ToolHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<Database>,
        media_dir: PathBuf,
        skills_dir: PathBuf,
        server_paths: Arc<ServerPaths>,
        prompts: Arc<Prompts>,
        states_config: Arc<StatesConfig>,
        phases_config: Arc<PhasesConfig>,
        deps_config: Arc<DependenciesConfig>,
        auto_advance: Arc<AutoAdvanceConfig>,
        attachments_config: Arc<AttachmentsConfig>,
        tags_config: Arc<TagsConfig>,
        ids_config: Arc<IdsConfig>,
        workflows: Arc<WorkflowsConfig>,
        default_format: OutputFormat,
        path_mapper: Arc<crate::paths::PathMapper>,
    ) -> Self {
        Self {
            db,
            media_dir,
            skills_dir,
            server_paths,
            prompts,
            states_config,
            phases_config,
            deps_config,
            auto_advance,
            attachments_config,
            tags_config,
            ids_config,
            workflows,
            default_format,
            path_mapper,
        }
    }

    /// Get all available tools.
    pub fn get_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();

        // Worker tools
        tools.extend(agents::get_tools(&self.prompts));

        // Task tools (with dynamic state schema)
        tools.extend(tasks::get_tools(&self.prompts, &self.states_config));

        // Tracking tools
        tools.extend(tracking::get_tools(&self.prompts, &self.states_config));

        // Dependency tools
        tools.extend(deps::get_tools(&self.prompts, &self.deps_config));

        // Claiming tools (with dynamic state schema)
        tools.extend(claiming::get_tools(&self.prompts, &self.states_config));

        // File coordination tools
        tools.extend(files::get_tools(&self.prompts));

        // Attachment tools
        tools.extend(attachments::get_tools(&self.prompts));

        // Skill tools (no prompts needed, always available)
        tools.extend(skills::get_tools());

        // Schema introspection tools
        tools.extend(schema::get_tools());

        // Search tools
        tools.extend(search::get_tools(&self.prompts));

        // Query tools (read-only SQL)
        tools.extend(query::get_tools());

        tools
    }

    /// Call a tool by name.
    /// Call a tool by name.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        // Helper to wrap JSON results
        let json = |r: Result<Value>| r.map(ToolResult::Json);

        match name {
            // Worker tools
            "connect" => json(agents::connect(
                &self.db,
                &self.server_paths,
                &self.states_config,
                &self.phases_config,
                &self.deps_config,
                &self.tags_config,
                &self.ids_config,
                arguments,
            )),
            "disconnect" => json(agents::disconnect(&self.db, &self.states_config, arguments)),
            "list_agents" => agents::list_agents(
                &self.db,
                &self.states_config,
                self.default_format,
                arguments,
            ),
            "cleanup_stale" => json(agents::cleanup_stale(
                &self.db,
                &self.states_config,
                arguments,
            )),

            // Task tools
            "create" => json(tasks::create(
                &self.db,
                &self.states_config,
                &self.phases_config,
                &self.tags_config,
                &self.ids_config,
                arguments,
            )),
            "create_tree" => json(tasks::create_tree(
                &self.db,
                &self.states_config,
                &self.phases_config,
                &self.tags_config,
                &self.ids_config,
                arguments,
            )),
            "get" => json(tasks::get(&self.db, self.default_format, arguments)),
            "list_tasks" => json(tasks::list_tasks(
                &self.db,
                &self.states_config,
                &self.deps_config,
                self.default_format,
                arguments,
            )),
            "update" => json(tasks::update(
                &self.db,
                &self.attachments_config,
                &self.states_config,
                &self.phases_config,
                &self.deps_config,
                &self.auto_advance,
                &self.tags_config,
                &self.workflows,
                arguments,
            )),
            "delete" => json(tasks::delete(&self.db, arguments)),
            "scan" => json(tasks::scan(&self.db, self.default_format, arguments)),

            // Tracking tools
            "thinking" => json(tracking::thinking(&self.db, arguments)),
            "task_history" => json(tracking::task_history(
                &self.db,
                &self.states_config,
                self.default_format,
                arguments,
            )),
            "log_metrics" => json(tracking::log_metrics(&self.db, arguments)),
            "get_metrics" => json(tracking::get_metrics(&self.db, arguments)),
            "project_history" => json(tracking::project_history(
                &self.db,
                self.default_format,
                arguments,
            )),

            // Dependency tools
            "link" => json(deps::link(&self.db, &self.deps_config, arguments)),
            "unlink" => json(deps::unlink(&self.db, arguments)),
            "relink" => json(deps::relink(&self.db, &self.deps_config, arguments)),

            // Claiming tools
            "claim" => json(claiming::claim(
                &self.db,
                &self.states_config,
                &self.phases_config,
                &self.deps_config,
                &self.auto_advance,
                &self.workflows,
                arguments,
            )),

            // File coordination tools
            "mark_file" => json(files::mark_file(&self.db, arguments)),
            "unmark_file" => json(files::unmark_file(&self.db, arguments)),
            "list_marks" => json(files::list_marks(&self.db, self.default_format, arguments)),
            "mark_updates" => {
                json(files::mark_updates_async(std::sync::Arc::clone(&self.db), arguments).await)
            }

            // Attachment tools
            "attach" => json(attachments::attach(
                &self.db,
                &self.media_dir,
                &self.attachments_config,
                arguments,
            )),
            "attachments" => json(attachments::attachments(
                &self.db,
                &self.media_dir,
                self.default_format,
                arguments,
            )),
            "detach" => json(attachments::detach(&self.db, &self.media_dir, arguments)),

            // Skill tools
            name if skills::is_skill_tool(name) => {
                json(skills::call_tool(&self.skills_dir, name, &arguments))
            }

            // Schema introspection tools
            "get_schema" => json(schema::get_schema(&self.db, arguments)),

            // Search tools
            "search" => json(search::search(&self.db, arguments)),

            // Query tools (read-only SQL)
            "query" => query::query(&self.db, self.default_format, arguments),

            _ => Err(ToolError::unknown_tool(name).into()),
        }
    }
}

/// Helper to create a tool definition.
pub fn make_tool(name: &str, description: &str, properties: Value, required: Vec<&str>) -> Tool {
    let input_schema = rmcp::model::JsonObject::from_iter([
        ("type".to_string(), serde_json::json!("object")),
        ("properties".to_string(), properties),
        ("required".to_string(), serde_json::json!(required)),
    ]);

    Tool::new(name.to_string(), description.to_string(), input_schema)
}

/// Helper to create a tool definition with prompt overrides.
/// Looks up the tool description in prompts, falls back to default_description.
pub fn make_tool_with_prompts(
    name: &str,
    default_description: &str,
    properties: Value,
    required: Vec<&str>,
    prompts: &Prompts,
) -> Tool {
    let description = prompts
        .get_tool_description(name)
        .unwrap_or(default_description);
    make_tool(name, description, properties, required)
}

/// Helper to get a string from arguments.
pub fn get_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str().map(String::from))
}

/// Helper to get an i32 from arguments.
pub fn get_i32(args: &Value, key: &str) -> Option<i32> {
    args.get(key).and_then(|v| v.as_i64().map(|n| n as i32))
}

/// Helper to get an i64 from arguments.
pub fn get_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

/// Helper to get an f64 from arguments.
pub fn get_f64(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(|v| v.as_f64())
}

/// Helper to get a bool from arguments.
pub fn get_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Helper to get a string array from arguments.
pub fn get_string_array(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
    })
}

/// Helper to get either a single string or array of strings from arguments.
/// Normalizes to a Vec<String>.
pub fn get_string_or_array(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(|v| {
        if let Some(s) = v.as_str() {
            // Single string - wrap in vec
            Some(vec![s.to_string()])
        } else {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
        }
    })
}
