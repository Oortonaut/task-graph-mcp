//! MCP tool implementations.

pub mod agents;
pub mod attachments;
pub mod claiming;
pub mod deps;
pub mod files;
pub mod tasks;
pub mod tracking;

use crate::config::{Prompts, StatesConfig};
use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Tool handler that processes MCP tool calls.
pub struct ToolHandler {
    pub db: Arc<Database>,
    pub media_dir: PathBuf,
    pub prompts: Arc<Prompts>,
    pub states_config: Arc<StatesConfig>,
}

impl ToolHandler {
    pub fn new(db: Arc<Database>, media_dir: PathBuf, prompts: Arc<Prompts>, states_config: Arc<StatesConfig>) -> Self {
        Self { db, media_dir, prompts, states_config }
    }

    /// Get all available tools.
    pub fn get_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();

        // Agent tools
        tools.extend(agents::get_tools(&self.prompts));

        // Task tools (with dynamic state schema)
        tools.extend(tasks::get_tools(&self.prompts, &self.states_config));

        // Tracking tools
        tools.extend(tracking::get_tools(&self.prompts));

        // Dependency tools
        tools.extend(deps::get_tools(&self.prompts));

        // Claiming tools (with dynamic state schema)
        tools.extend(claiming::get_tools(&self.prompts, &self.states_config));

        // File coordination tools
        tools.extend(files::get_tools(&self.prompts));

        // Attachment tools
        tools.extend(attachments::get_tools(&self.prompts));

        tools
    }

    /// Call a tool by name.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            // Agent tools
            "connect" => agents::connect(&self.db, arguments),
            "disconnect" => agents::disconnect(&self.db, arguments),
            "list_agents" => agents::list_agents(&self.db, arguments),

            // Task tools
            "create" => tasks::create(&self.db, &self.states_config, arguments),
            "create_tree" => tasks::create_tree(&self.db, &self.states_config, arguments),
            "get" => tasks::get(&self.db, arguments),
            "list_tasks" => tasks::list_tasks(&self.db, &self.states_config, arguments),
            "update" => tasks::update(&self.db, &self.states_config, arguments),
            "delete" => tasks::delete(&self.db, arguments),

            // Tracking tools
            "thinking" => tracking::thinking(&self.db, arguments),
            "get_state_history" => tracking::get_state_history(&self.db, &self.states_config, arguments),
            "log_cost" => tracking::log_cost(&self.db, arguments),

            // Dependency tools
            "block" => deps::block(&self.db, arguments),
            "unblock" => deps::unblock(&self.db, arguments),

            // Claiming tools
            "claim" => claiming::claim(&self.db, &self.states_config, arguments),
            "release" => claiming::release(&self.db, &self.states_config, arguments),
            "complete" => claiming::complete(&self.db, &self.states_config, arguments),

            // File coordination tools
            "claim_file" => files::claim_file(&self.db, arguments),
            "release_file" => files::release_file(&self.db, arguments),
            "list_files" => files::list_files(&self.db, arguments),
            "claim_updates" => files::claim_updates(&self.db, arguments),

            // Attachment tools
            "attach" => attachments::attach(&self.db, &self.media_dir, arguments),
            "attachments" => attachments::attachments(&self.db, &self.media_dir, arguments),
            "detach" => attachments::detach(&self.db, &self.media_dir, arguments),

            _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
        }
    }
}

/// Helper to create a tool definition.
pub fn make_tool(name: &str, description: &str, properties: Value, required: Vec<&str>) -> Tool {
    let input_schema = rmcp::model::JsonObject::from_iter([
        ("type".to_string(), serde_json::json!("object")),
        ("properties".to_string(), properties),
        (
            "required".to_string(),
            serde_json::json!(required),
        ),
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
