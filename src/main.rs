//! Task Graph MCP Server
//!
//! A Rust MCP server providing atomic, token-efficient task management
//! for multi-agent coordination.

use anyhow::Result;
use clap::Parser;
use task_graph_mcp::config::Config;
use task_graph_mcp::db::Database;
use task_graph_mcp::resources::ResourceHandler;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, InitializeResult,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult,
        ResourceContents, ServerCapabilities,
    },
    service::RequestContext,
    transport::io::stdio,
};
use serde_json::{json, Value};
use std::sync::Arc;
use task_graph_mcp::tools::ToolHandler;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Task Graph MCP Server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<String>,

    /// Path to database file (overrides config)
    #[arg(short, long)]
    database: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// MCP server handler.
#[derive(Clone)]
struct TaskGraphServer {
    tool_handler: Arc<ToolHandler>,
    resource_handler: Arc<ResourceHandler>,
}

impl TaskGraphServer {
    fn new(db: Arc<Database>) -> Self {
        Self {
            tool_handler: Arc::new(ToolHandler::new(Arc::clone(&db))),
            resource_handler: Arc::new(ResourceHandler::new(db)),
        }
    }
}

impl ServerHandler for TaskGraphServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: Default::default(),
            server_info: rmcp::model::Implementation {
                name: "task-graph-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability::default()),
                resources: Some(rmcp::model::ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                ..Default::default()
            },
            instructions: Some(
                "Task Graph MCP Server provides atomic, token-efficient task management \
                for multi-agent coordination. Use tools for task CRUD, claiming, dependencies, \
                file locking, and pub/sub. Query resources for task graphs, stats, and plans."
                    .into(),
            ),
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_handler.get_tools(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        match self.tool_handler.call_tool(&request.name, args).await {
            Ok(result) => Ok(CallToolResult {
                content: vec![Content::text(
                    serde_json::to_string_pretty(&result).unwrap_or_default(),
                )],
                is_error: None,
                meta: None,
                structured_content: None,
            }),
            Err(e) => Ok(CallToolResult {
                content: vec![Content::text(
                    json!({ "error": e.to_string() }).to_string(),
                )],
                is_error: Some(true),
                meta: None,
                structured_content: None,
            }),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult {
            resource_templates: self.resource_handler.get_resource_templates(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, ErrorData> {
        let uri_string = request.uri.to_string();
        match self.resource_handler.read_resource(&uri_string).await {
            Ok(result) => Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(
                    serde_json::to_string_pretty(&result).unwrap_or_default(),
                    request.uri,
                )],
            }),
            Err(e) => Err(ErrorData::resource_not_found(
                format!("Unknown resource: {}", uri_string),
                Some(json!({ "error": e.to_string() })),
            )),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let level = if args.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Load configuration
    let mut config = if let Some(config_path) = &args.config {
        Config::load(config_path)?
    } else {
        Config::load_or_default()
    };

    // Override database path if specified
    if let Some(db_path) = &args.database {
        config.server.db_path = db_path.into();
    }

    // Ensure database directory exists
    config.ensure_db_dir()?;

    info!("Starting Task Graph MCP Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Database: {:?}", config.server.db_path);

    // Open database
    let db = Database::open(&config.server.db_path)?;
    let db = Arc::new(db);

    info!("Database initialized successfully");

    // Create server handler
    let server = TaskGraphServer::new(db);

    // Run the stdio server
    info!("Server ready, listening on stdio");
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
