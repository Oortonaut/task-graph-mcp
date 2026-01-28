//! Task Graph MCP Server
//!
//! A Rust MCP server providing atomic, token-efficient task management
//! for multi-agent coordination.

use anyhow::Result;
use clap::Parser;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, InitializeResult,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResult, ResourceContents, ServerCapabilities,
    },
    service::RequestContext,
    transport::io::stdio,
};
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::sync::Arc;
use task_graph_mcp::cli::{Cli, Command, UiMode as CliUiMode, migrate};
use task_graph_mcp::config::{
    AttachmentsConfig, AutoAdvanceConfig, Config, ConfigLoader, DependenciesConfig, PhasesConfig,
    Prompts, ServerPaths, StatesConfig, UiMode, workflows::WorkflowsConfig,
};
use task_graph_mcp::dashboard;
use task_graph_mcp::db::Database;
use task_graph_mcp::error::ToolError;
use task_graph_mcp::format::OutputFormat;
use task_graph_mcp::resources::ResourceHandler;
use task_graph_mcp::tools::ToolHandler;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

/// MCP server handler.
#[derive(Clone)]
struct TaskGraphServer {
    tool_handler: Arc<ToolHandler>,
    resource_handler: Arc<ResourceHandler>,
    prompts: Arc<Prompts>,
}

impl TaskGraphServer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        db: Arc<Database>,
        media_dir: std::path::PathBuf,
        skills_dir: std::path::PathBuf,
        server_paths: Arc<ServerPaths>,
        prompts: Arc<Prompts>,
        states_config: Arc<StatesConfig>,
        phases_config: Arc<PhasesConfig>,
        deps_config: Arc<DependenciesConfig>,
        auto_advance: Arc<AutoAdvanceConfig>,
        attachments_config: Arc<AttachmentsConfig>,
        workflows: Arc<WorkflowsConfig>,
        default_format: OutputFormat,
        path_mapper: Arc<task_graph_mcp::paths::PathMapper>,
    ) -> Self {
        Self {
            tool_handler: Arc::new(ToolHandler::new(
                Arc::clone(&db),
                media_dir,
                skills_dir.clone(),
                server_paths,
                Arc::clone(&prompts),
                Arc::clone(&states_config),
                Arc::clone(&phases_config),
                Arc::clone(&deps_config),
                Arc::clone(&auto_advance),
                Arc::clone(&attachments_config),
                workflows,
                default_format,
                path_mapper,
            )),
            resource_handler: Arc::new(
                ResourceHandler::new(db, states_config, phases_config, deps_config).with_skills_dir(skills_dir),
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
        let instructions = self
            .prompts
            .instructions
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
            instructions: Some(instructions),
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
                    Ok(tool_err) => serde_json::to_string(&tool_err)
                        .unwrap_or_else(|_| json!({ "error": tool_err.to_string() }).to_string()),
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

/// Convert CLI UiMode to config UiMode
fn cli_ui_mode_to_config(cli_mode: CliUiMode) -> UiMode {
    match cli_mode {
        CliUiMode::None => UiMode::None,
        CliUiMode::Web => UiMode::Web,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging based on --log option
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    match cli.log.as_str() {
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

    // Load configuration using the new ConfigLoader with tier merging
    // If explicit config path given, set it as env var for ConfigLoader to pick up
    // SAFETY: This is safe at program startup before any other threads are spawned
    if let Some(config_path) = &cli.config {
        // Use unsafe block for set_var which is required in Rust 2024 edition
        unsafe {
            std::env::set_var("TASK_GRAPH_CONFIG_PATH", config_path);
        }
    }
    let mut loader = ConfigLoader::load()?;

    // Track if using deprecated paths
    if loader.is_using_deprecated() {
        eprintln!(
            "Warning: Using deprecated config directory '.task-graph/'. \
             Run 'task-graph migrate' to move to 'task-graph/'."
        );
    }

    let config_path_used = loader.config_path().map(|p| p.to_string_lossy().to_string());

    // Get mutable reference to apply CLI overrides
    let config = loader.config_mut();

    // Override paths from CLI arguments
    if let Some(db_path) = &cli.database {
        config.server.db_path = db_path.into();
    }
    if let Some(media_dir) = &cli.media_dir {
        config.server.media_dir = media_dir.into();
    }
    if let Some(log_dir) = &cli.log_dir {
        config.server.log_dir = log_dir.into();
    }

    // Override UI settings from CLI arguments
    if let Some(ui_mode) = cli.ui {
        config.server.ui.mode = cli_ui_mode_to_config(ui_mode);
    }
    if let Some(ui_port) = cli.ui_port {
        config.server.ui.port = ui_port;
    }

    // Handle subcommands
    match cli.command {
        Some(Command::Export(_args)) => {
            // TODO: Implement export command
            anyhow::bail!("Export command not yet implemented");
        }
        Some(Command::Import(_args)) => {
            // TODO: Implement import command
            anyhow::bail!("Import command not yet implemented");
        }
        Some(Command::Diff(_args)) => {
            // TODO: Implement diff command
            anyhow::bail!("Diff command not yet implemented");
        }
        Some(Command::Migrate(args)) => {
            // Run migration command
            migrate::run_migrate(&args)?;
        }
        Some(Command::Serve) | None => {
            // Load prompts using the loader (before consuming it)
            let prompts = loader.load_prompts();
            // Load workflows configuration (contains states, phases, and transition prompts)
            let workflows = loader.load_workflows();
            // Get the final config
            let config = loader.into_config();
            // Default: run MCP server
            run_server(config, prompts, workflows, config_path_used).await?;
        }
    }

    Ok(())
}

/// Run the MCP server
async fn run_server(config: Config, prompts: Prompts, workflows: WorkflowsConfig, config_path_used: Option<String>) -> Result<()> {
    // Ensure directories exist
    config.ensure_db_dir()?;
    config.ensure_media_dir()?;
    config.ensure_log_dir()?;

    // Derive states and phases config from workflows
    let states_config: StatesConfig = (&workflows).into();
    let phases_config: PhasesConfig = (&workflows).into();

    // Validate configuration
    states_config.validate()?;
    config.dependencies.validate()?;

    // Wrap in Arc
    let prompts = Arc::new(prompts);
    let workflows = Arc::new(workflows);
    let states_config = Arc::new(states_config);
    let phases_config = Arc::new(phases_config);

    info!(
        "Starting Task Graph MCP Server v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("Database: {:?}", config.server.db_path);
    info!("Media dir: {:?}", config.server.media_dir);
    info!("Log dir: {:?}", config.server.log_dir);
    info!("UI mode: {:?}", config.server.ui.mode);
    if config.server.ui.mode == UiMode::Web {
        info!("UI port: {}", config.server.ui.port);
    }

    // Open database
    let db = Database::open(&config.server.db_path)?;
    let db = Arc::new(db);

    info!("Database initialized successfully");

    // Create server paths for connect response
    let server_paths = Arc::new(ServerPaths {
        db_path: config.server.db_path.clone(),
        media_dir: config.server.media_dir.clone(),
        log_dir: config.server.log_dir.clone(),
        config_path: config_path_used.map(std::path::PathBuf::from),
    });

    // Create server handler
    let deps_config = Arc::new(config.dependencies.clone());
    let auto_advance = Arc::new(config.auto_advance.clone());
    let attachments_config = Arc::new(config.attachments.clone());

    // Create path mapper from config
    let path_mapper = Arc::new(
        task_graph_mcp::paths::PathMapper::from_config(&config.paths, Some(&config))
            .map_err(|e| anyhow::anyhow!("Failed to create path mapper from config: {}", e))?,
    );

    let server = TaskGraphServer::new(
        Arc::clone(&db),
        config.server.media_dir.clone(),
        config.server.skills_dir.clone(),
        server_paths,
        prompts,
        Arc::clone(&states_config),
        Arc::clone(&phases_config),
        deps_config,
        auto_advance,
        attachments_config,
        workflows,
        config.server.default_format,
        path_mapper,
    );

    // Start the HTTP dashboard server if UI mode is Web
    // This never fails - if the port is in use, it retries in the background
    let _dashboard_handle = if config.server.ui.mode == UiMode::Web {
        Some(dashboard::start_server_with_retry(
            Arc::clone(&db),
            &config.server.ui,
            Arc::clone(&states_config),
        ))
    } else {
        None
    };

    // Run the stdio server
    info!("Server ready, listening on stdio");
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
