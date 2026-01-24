//! MCP tool implementations.

pub mod agents;
pub mod attachments;
pub mod claiming;
pub mod deps;
pub mod files;
pub mod pubsub;
pub mod tasks;
pub mod tracking;

use crate::db::Database;
use anyhow::Result;
use rmcp::model::Tool;
use serde_json::Value;
use std::sync::Arc;

/// Tool handler that processes MCP tool calls.
pub struct ToolHandler {
    pub db: Arc<Database>,
}

impl ToolHandler {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Get all available tools.
    pub fn get_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();

        // Agent tools
        tools.extend(agents::get_tools());

        // Task tools
        tools.extend(tasks::get_tools());

        // Tracking tools
        tools.extend(tracking::get_tools());

        // Dependency tools
        tools.extend(deps::get_tools());

        // Claiming tools
        tools.extend(claiming::get_tools());

        // File lock tools
        tools.extend(files::get_tools());

        // Attachment tools
        tools.extend(attachments::get_tools());

        // Pub/sub tools
        tools.extend(pubsub::get_tools());

        tools
    }

    /// Call a tool by name.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            // Agent tools
            "register_agent" => agents::register_agent(&self.db, arguments),
            "update_agent" => agents::update_agent(&self.db, arguments),
            "heartbeat" => agents::heartbeat(&self.db, arguments),
            "unregister_agent" => agents::unregister_agent(&self.db, arguments),

            // Task tools
            "create_task" => tasks::create_task(&self.db, arguments),
            "create_task_tree" => tasks::create_task_tree(&self.db, arguments),
            "get_task" => tasks::get_task(&self.db, arguments),
            "update_task" => tasks::update_task(&self.db, arguments),
            "delete_task" => tasks::delete_task(&self.db, arguments),
            "list_tasks" => tasks::list_tasks(&self.db, arguments),

            // Tracking tools
            "set_thought" => tracking::set_thought(&self.db, arguments),
            "log_time" => tracking::log_time(&self.db, arguments),
            "log_cost" => tracking::log_cost(&self.db, arguments),

            // Dependency tools
            "add_dependency" => deps::add_dependency(&self.db, arguments),
            "remove_dependency" => deps::remove_dependency(&self.db, arguments),
            "get_blocked_tasks" => deps::get_blocked_tasks(&self.db, arguments),
            "get_ready_tasks" => deps::get_ready_tasks(&self.db, arguments),

            // Claiming tools
            "claim_task" => claiming::claim_task(&self.db, arguments),
            "release_task" => claiming::release_task(&self.db, arguments),
            "force_release" => claiming::force_release(&self.db, arguments),
            "force_release_stale" => claiming::force_release_stale(&self.db, arguments),

            // File lock tools
            "lock_file" => files::lock_file(&self.db, arguments),
            "unlock_file" => files::unlock_file(&self.db, arguments),
            "get_file_locks" => files::get_file_locks(&self.db, arguments),

            // Attachment tools
            "add_attachment" => attachments::add_attachment(&self.db, arguments),
            "get_attachments" => attachments::get_attachments(&self.db, arguments),
            "get_attachment" => attachments::get_attachment(&self.db, arguments),
            "delete_attachment" => attachments::delete_attachment(&self.db, arguments),

            // Pub/sub tools
            "subscribe" => pubsub::subscribe(&self.db, arguments),
            "unsubscribe" => pubsub::unsubscribe(&self.db, arguments),
            "poll_inbox" => pubsub::poll_inbox(&self.db, arguments),
            "clear_inbox" => pubsub::clear_inbox(&self.db, arguments),
            "get_subscriptions" => pubsub::get_subscriptions(&self.db, arguments),

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

/// Helper to get a string from arguments.
pub fn get_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str().map(String::from))
}

/// Helper to get a UUID from arguments.
pub fn get_uuid(args: &Value, key: &str) -> Option<uuid::Uuid> {
    get_string(args, key).and_then(|s| uuid::Uuid::parse_str(&s).ok())
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

/// Helper to get a UUID array from arguments.
pub fn get_uuid_array(args: &Value, key: &str) -> Option<Vec<uuid::Uuid>> {
    args.get(key).and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()))
                .collect()
        })
    })
}
