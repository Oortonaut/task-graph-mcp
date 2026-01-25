//! Task Graph MCP Server
//!
//! A Rust MCP server providing atomic, token-efficient task management
//! for multi-agent coordination.

use anyhow::Result;
use clap::Parser;
use task_graph_mcp::config::{AutoAdvanceConfig, Config, DependenciesConfig, Prompts, StatesConfig};
use task_graph_mcp::format::OutputFormat;
use task_graph_mcp::error::ToolError;
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
use std::fs::OpenOptions;
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

    /// Logging output: 0/off, 1/stdout, 2/stderr (default), or filename
    #[arg(short, long, default_value = "2")]
    log: String,
}

/// MCP server handler.
#[derive(Clone)]
struct TaskGraphServer {
    tool_handler: Arc<ToolHandler>,
    resource_handler: Arc<ResourceHandler>,
    prompts: Arc<Prompts>,
}

impl TaskGraphServer {
    fn new(
        db: Arc<Database>,
        media_dir: std::path::PathBuf,
        skills_dir: std::path::PathBuf,
        prompts: Arc<Prompts>,
        states_config: Arc<StatesConfig>,
        deps_config: Arc<DependenciesConfig>,
        auto_advance: Arc<AutoAdvanceConfig>,
        default_format: OutputFormat,
    ) -> Self {
        Self {
            tool_handler: Arc::new(ToolHandler::new(
                Arc::clone(&db),
                media_dir,
                skills_dir.clone(),
                Arc::clone(&prompts),
                Arc::clone(&states_config),
                Arc::clone(&deps_config),
                Arc::clone(&auto_advance),
                default_format,
            )),
            resource_handler: Arc::new(
                ResourceHandler::new(db, states_config, deps_config).with_skills_dir(skills_dir),
            ),
            prompts,
        }
    }
}

/// Default server instructions when no prompts.yaml is present.
const DEFAULT_INSTRUCTIONS: &str = "\
Task graph for multi-agent coordination. Start: connect() → list_tasks(ready=true) → claim() → work → update(state=\"completed\").
Use get_skill(\"basics\") for full documentation.";

impl ServerHandler for TaskGraphServer {
    fn get_info(&self) -> InitializeResult {
        let instructions = self.prompts.instructions
            .clone()
            .unwrap_or_else(|| DEFAULT_INSTRUCTIONS.to_string());

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
            instructions: Some(instructions.into()),
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
                content: vec![Content::text(result.into_string())],
                is_error: None,
                meta: None,
                structured_content: None,
            }),
            Err(e) => {
                // Try to downcast to ToolError for structured response
                let error_json = match e.downcast::<ToolError>() {
                    Ok(tool_err) => serde_json::to_string(&tool_err).unwrap_or_else(|_| {
                        json!({ "error": tool_err.to_string() }).to_string()
                    }),
                    Err(e) => json!({
                        "code": "INTERNAL_ERROR",
                        "message": e.to_string()
                    })
                    .to_string(),
                };
                Ok(CallToolResult {
                    content: vec![Content::text(error_json)],
                    is_error: Some(true),
                    meta: None,
                    structured_content: None,
                })
            }
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

    // Initialize logging based on --log option
    let level = if args.verbose { Level::DEBUG } else { Level::INFO };
    match args.log.as_str() {
        "0" | "off" => {
            // No logging
        }
        "1" | "stdout" => {
            let subscriber = FmtSubscriber::builder()
                .with_max_level(level)
                .with_writer(std::io::stdout)
                .finish();
            tracing::subscriber::set_global_default(subscriber)?;
        }
        "2" | "stderr" => {
            let subscriber = FmtSubscriber::builder()
                .with_max_level(level)
                .with_writer(std::io::stderr)
                .finish();
            tracing::subscriber::set_global_default(subscriber)?;
        }
        filename => {
            // Log to file (append mode)
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(filename)?;
            let subscriber = FmtSubscriber::builder()
                .with_max_level(level)
                .with_writer(file)
                .with_ansi(false)
                .finish();
            tracing::subscriber::set_global_default(subscriber)?;
        }
    }

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

    // Ensure database and media directories exist
    config.ensure_db_dir()?;
    config.ensure_media_dir()?;

    // Validate configuration
    config.states.validate()?;
    config.dependencies.validate()?;

    // Load prompts
    let prompts = Arc::new(Prompts::load_or_default());

    info!("Starting Task Graph MCP Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Database: {:?}", config.server.db_path);
    info!("Media dir: {:?}", config.server.media_dir);

    // Open database
    let db = Database::open(&config.server.db_path)?;
    let db = Arc::new(db);

    info!("Database initialized successfully");

    // Create server handler
    let states_config = Arc::new(config.states.clone());
    let deps_config = Arc::new(config.dependencies.clone());
    let auto_advance = Arc::new(config.auto_advance.clone());
    let server = TaskGraphServer::new(
        db,
        config.server.media_dir.clone(),
        config.server.skills_dir.clone(),
        prompts,
        states_config,
        deps_config,
        auto_advance,
        config.server.default_format,
    );

    // Run the stdio server
    info!("Server ready, listening on stdio");
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
