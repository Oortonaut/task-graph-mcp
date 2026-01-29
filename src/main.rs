//! Task Graph MCP Server
//!
//! A Rust MCP server providing atomic, token-efficient task management
//! for multi-agent coordination.

use anyhow::Result;
use arc_swap::ArcSwap;
use clap::Parser;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, InitializeResult,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResult, ResourceContents,
        ResourceUpdatedNotificationParam, ServerCapabilities, SubscribeRequestParams,
        UnsubscribeRequestParams,
    },
    service::RequestContext,
    transport::io::stdio,
};
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use task_graph_mcp::cli::diff::DiffArgs;
use task_graph_mcp::cli::diff::DiffFormat;
use task_graph_mcp::cli::export::ExportArgs;
use task_graph_mcp::cli::import::ImportArgs;
use task_graph_mcp::cli::{Cli, Command, UiMode as CliUiMode, migrate};
use task_graph_mcp::config::{
    AppConfig, Config, ConfigLoader, PhasesConfig, Prompts, ServerPaths, StatesConfig, UiMode,
    watcher::{WatchPaths, WatcherConfig, start_config_watcher},
    workflows::WorkflowsConfig,
};
use task_graph_mcp::dashboard;
use task_graph_mcp::db::Database;
use task_graph_mcp::db::export::ExportOptions;
use task_graph_mcp::db::import::ImportMode;
use task_graph_mcp::error::ToolError;
use task_graph_mcp::export::diff::{diff_snapshot_vs_database, diff_snapshots};
use task_graph_mcp::export::{CURRENT_SCHEMA_VERSION, Snapshot};
use task_graph_mcp::format::OutputFormat;
use task_graph_mcp::logging::{LogLevelFilter, Logger};
use task_graph_mcp::resources::ResourceHandler;
use task_graph_mcp::subscriptions::{MutationKind, SubscriptionManager};
use task_graph_mcp::tools::{ToolContext, ToolHandler};
use tracing::{Level, debug, info, warn};
use tracing_subscriber::FmtSubscriber;

/// Auto-discover the docs directory.
///
/// Looks for a `docs/` directory in the current working directory.
/// Returns `None` if no docs directory is found.
fn discover_docs_dir() -> Option<std::path::PathBuf> {
    let docs_path = std::path::PathBuf::from("docs");
    if docs_path.exists() && docs_path.is_dir() {
        Some(docs_path)
    } else {
        None
    }
}

/// MCP server handler.
///
/// Uses `ArcSwap` for `tool_handler` and `resource_handler` so that the config
/// file watcher can atomically swap in rebuilt handlers when config files change
/// on disk, without restarting the server.
#[derive(Clone)]
struct TaskGraphServer {
    tool_handler: Arc<ArcSwap<ToolHandler>>,
    resource_handler: Arc<ArcSwap<ResourceHandler>>,
    prompts: Arc<ArcSwap<Prompts>>,
    /// Atomic level filter for logging (client can adjust via logging/setLevel).
    level_filter: Arc<LogLevelFilter>,
    /// Tracks which resource URIs the client has subscribed to for update
    /// notifications, enabling interrupt-style coordination instead of polling.
    subscriptions: Arc<SubscriptionManager>,
}

impl TaskGraphServer {
    fn new(
        db: Arc<Database>,
        media_dir: std::path::PathBuf,
        skills_dir: std::path::PathBuf,
        server_paths: Arc<ServerPaths>,
        prompts: Arc<Prompts>,
        app_config: AppConfig,
        default_format: OutputFormat,
        default_page_size: i32,
        path_mapper: Arc<task_graph_mcp::paths::PathMapper>,
        level_filter: Arc<LogLevelFilter>,
    ) -> Self {
        let tool_handler = Arc::new(ToolHandler::new(
            Arc::clone(&db),
            media_dir,
            skills_dir.clone(),
            server_paths,
            Arc::clone(&prompts),
            app_config.clone(),
            default_format,
            default_page_size,
            path_mapper,
        ));
        // Auto-discover docs directory
        let docs_dir = discover_docs_dir();
        let mut resource_handler = ResourceHandler::new(db, app_config).with_skills_dir(skills_dir);
        if let Some(ref dir) = docs_dir {
            resource_handler = resource_handler.with_docs_dir(dir.clone());
        }
        let resource_handler = Arc::new(resource_handler);

        Self {
            tool_handler: Arc::new(ArcSwap::from(tool_handler)),
            resource_handler: Arc::new(ArcSwap::from(resource_handler)),
            prompts: Arc::new(ArcSwap::from(prompts)),
            level_filter,
            subscriptions: Arc::new(SubscriptionManager::new()),
        }
    }
}

/// Default server instructions when no prompts.yaml is present.
const DEFAULT_INSTRUCTIONS: &str = "\
Task graph for multi-agent coordination. Start: connect() \u{2192} list_tasks(ready=true) \u{2192} claim() \u{2192} work \u{2192} update(state=\"completed\").
Use get_skill(\"basics\") for full documentation.";

impl ServerHandler for TaskGraphServer {
    fn get_info(&self) -> InitializeResult {
        let prompts = self.prompts.load();
        let instructions = prompts
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
                    subscribe: Some(Default::default()),
                    list_changed: None,
                }),
                logging: Some(Default::default()),
                ..Default::default()
            },
            instructions: Some(instructions),
        }
    }

    async fn set_level(
        &self,
        request: rmcp::model::SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<(), ErrorData> {
        self.level_filter.set(request.level);
        tracing::info!(level = ?request.level, "Logging level updated via MCP");
        Ok(())
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<(), ErrorData> {
        let uri = request.uri;
        let is_new = self.subscriptions.subscribe(&uri);
        if is_new {
            info!(uri = %uri, "Client subscribed to resource");
        } else {
            debug!(uri = %uri, "Client re-subscribed to resource (already subscribed)");
        }
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<(), ErrorData> {
        let uri = request.uri;
        let was_present = self.subscriptions.unsubscribe(&uri);
        if was_present {
            info!(uri = %uri, "Client unsubscribed from resource");
        } else {
            debug!(uri = %uri, "Client unsubscribed from resource (was not subscribed)");
        }
        Ok(())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        let handler = self.tool_handler.load();
        Ok(ListToolsResult {
            tools: handler.get_tools(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        let tool_name = request.name.clone();
        let start = std::time::Instant::now();

        // Create logger for this request
        let logger = Logger::new()
            .with_peer(context.peer.clone())
            .with_level_filter(Arc::clone(&self.level_filter))
            .with_name(format!("tool:{}", tool_name));
        let tool_ctx = ToolContext::new(logger);

        let handler = self.tool_handler.load();
        let args = Value::Object(request.arguments.unwrap_or_default());
        match handler.call_tool(&tool_name, args, &tool_ctx).await {
            Ok(result) => {
                let elapsed = start.elapsed();
                debug!(tool = %tool_name, duration_ms = elapsed.as_millis() as u64, "Tool call succeeded");

                // Notify subscribed resources about mutations from this tool call.
                // Only send notifications if the client has any active subscriptions
                // to avoid unnecessary work.
                if self.subscriptions.has_subscriptions() {
                    let mutations = mutations_for_tool(&tool_name);
                    if !mutations.is_empty() {
                        let affected = self.subscriptions.affected_subscriptions(&mutations);
                        if !affected.is_empty() {
                            let peer = context.peer.clone();
                            tokio::spawn(async move {
                                for uri in affected {
                                    debug!(uri = %uri, tool = %tool_name, "Sending resource updated notification");
                                    let param = ResourceUpdatedNotificationParam { uri };
                                    let _ = peer.notify_resource_updated(param).await;
                                }
                            });
                        }
                    }
                }

                Ok(CallToolResult {
                    content: vec![Content::text(result.into_string())],
                    is_error: None,
                    meta: None,
                    structured_content: None,
                })
            }
            Err(e) => {
                let elapsed = start.elapsed();
                // Try to downcast to ToolError for structured response
                let error_json = match e.downcast::<ToolError>() {
                    Ok(tool_err) => {
                        warn!(
                            tool = %tool_name,
                            error_code = ?tool_err.code,
                            error_message = %tool_err.message,
                            duration_ms = elapsed.as_millis() as u64,
                            "Tool call failed"
                        );
                        serde_json::to_string(&tool_err).unwrap_or_else(|_| {
                            json!({ "error": tool_err.to_string() }).to_string()
                        })
                    }
                    Err(e) => {
                        warn!(
                            tool = %tool_name,
                            error = %e,
                            duration_ms = elapsed.as_millis() as u64,
                            "Tool call failed with internal error"
                        );
                        json!({
                            "code": "INTERNAL_ERROR",
                            "message": e.to_string()
                        })
                        .to_string()
                    }
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
        let handler = self.resource_handler.load();
        Ok(ListResourcesResult {
            resources: handler.get_resources(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourceTemplatesResult, ErrorData> {
        let handler = self.resource_handler.load();
        Ok(ListResourceTemplatesResult {
            resource_templates: handler.get_resource_templates(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, ErrorData> {
        let handler = self.resource_handler.load();
        let uri_string = request.uri.to_string();
        match handler.read_resource(&uri_string).await {
            Ok(result) => Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(
                    serde_json::to_string_pretty(&result).unwrap_or_default(),
                    request.uri,
                )],
            }),
            Err(e) => {
                warn!(
                    resource_uri = %uri_string,
                    error = %e,
                    "Resource read failed"
                );
                Err(ErrorData::resource_not_found(
                    format!("Unknown resource: {}", uri_string),
                    Some(json!({ "error": e.to_string() })),
                ))
            }
        }
    }
}

/// Map a tool name to the mutation categories it causes.
/// Used to determine which subscribed resource URIs need notifications
/// after a successful tool call.
fn mutations_for_tool(tool_name: &str) -> Vec<MutationKind> {
    match tool_name {
        // Task mutations
        "create" | "create_tree" | "delete" | "rename" | "scan" => {
            vec![MutationKind::TaskChanged]
        }
        // Update can change status, which affects claimed/ready/blocked views
        "update" => vec![MutationKind::TaskChanged],
        // Claiming changes task status and agent claims
        "claim" => vec![MutationKind::TaskChanged, MutationKind::AgentChanged],
        // Dependency mutations affect ready/blocked status
        "link" | "unlink" | "relink" => {
            vec![MutationKind::DependencyChanged, MutationKind::TaskChanged]
        }
        // File coordination
        "mark_file" | "unmark_file" => vec![MutationKind::FileMarkChanged],
        // Agent lifecycle
        "connect" | "disconnect" | "cleanup_stale" => vec![MutationKind::AgentChanged],
        // Attachments
        "attach" | "detach" => vec![MutationKind::AttachmentChanged],
        // Tracking tools update agent state
        "thinking" | "log_metrics" => vec![MutationKind::AgentChanged],
        // Read-only tools cause no mutations
        "get" | "list_tasks" | "list_agents" | "list_marks" | "mark_updates" | "attachments"
        | "get_schema" | "search" | "query" | "check_gates" | "task_history" | "get_metrics"
        | "project_history" | "list_workflows" => vec![],
        // Skills tools are read-only
        name if name.starts_with("get_skill") || name.starts_with("list_skills") => vec![],
        // Unknown tools -- conservatively notify nothing
        _ => vec![],
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

    let config_path_used = loader
        .config_path()
        .map(|p| p.to_string_lossy().to_string());

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
        Some(Command::Export(args)) => {
            run_export(config, args)?;
        }
        Some(Command::Import(args)) => {
            run_import(config, args)?;
        }
        Some(Command::Diff(args)) => {
            run_diff(config, args)?;
        }
        Some(Command::Migrate(args)) => {
            // Run migration command
            migrate::run_migrate(&args)?;
        }
        Some(Command::Serve) | None => {
            // Load prompts using the loader (before consuming it)
            let prompts = loader.load_prompts();
            // Load workflows configuration (contains states, phases, and transition prompts)
            // Also pre-loads named workflow configs (workflow-*.yaml) for per-worker selection
            let workflows = load_workflows_with_cache(&loader);
            // Get the final config
            let config = loader.into_config();
            // Default: run MCP server
            run_server(config, prompts, workflows, config_path_used).await?;
        }
    }

    Ok(())
}

/// Load workflows config and pre-load named workflow configs into cache.
/// If default_workflow is configured, that workflow becomes the base config.
fn load_workflows_with_cache(loader: &ConfigLoader) -> WorkflowsConfig {
    let default_workflow_name = loader.config().server.default_workflow.clone();

    // If a default workflow is configured, load it as the base
    let mut workflows = if let Some(ref name) = default_workflow_name {
        match loader.load_workflow_by_name(name) {
            Ok(workflow_config) => {
                info!("Using '{}' as default workflow", name);
                workflow_config
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to load default workflow '{}': {}. Using built-in defaults.",
                    name,
                    e
                );
                loader.load_workflows()
            }
        }
    } else {
        loader.load_workflows()
    };

    // List all available named workflows and load them into cache
    let workflow_names = loader.list_workflows();

    for name in workflow_names {
        match loader.load_workflow_by_name(&name) {
            Ok(workflow_config) => {
                info!("Loaded workflow '{}' for per-worker selection", name);
                workflows
                    .named_workflows
                    .insert(name, Arc::new(workflow_config));
            }
            Err(e) => {
                tracing::warn!("Failed to load workflow '{}': {}", name, e);
            }
        }
    }

    // If default workflow was set, store the key for lookup
    if let Some(ref name) = default_workflow_name
        && workflows.named_workflows.contains_key(name)
    {
        workflows.default_workflow_key = Some(name.clone());
    }

    if !workflows.named_workflows.is_empty() {
        info!(
            "Workflow cache: {} named workflows available",
            workflows.named_workflows.len()
        );
    }

    workflows
}

/// Rebuild config from disk using a fresh ConfigLoader, then rebuild the
/// ToolHandler and ResourceHandler and swap them into the server atomically.
///
/// Immutable state (db, server_paths, path_mapper, level_filter, media/skills dirs)
/// is carried over from the original server construction since those are not
/// expected to change via config file edits.
fn reload_config(server: &TaskGraphServer, reload_ctx: &ReloadContext) {
    info!("Reloading configuration from disk...");

    // Re-load configuration from disk using a fresh ConfigLoader
    let loader = match ConfigLoader::load() {
        Ok(loader) => loader,
        Err(e) => {
            warn!(
                "Config reload failed during load: {}. Keeping current config.",
                e
            );
            return;
        }
    };

    // Reload prompts
    let prompts = loader.load_prompts();

    // Reload workflows with cache
    let workflows = load_workflows_with_cache(&loader);

    // Re-derive states and phases from the new workflows
    let states_config: StatesConfig = (&workflows).into();
    let phases_config: PhasesConfig = (&workflows).into();

    // Validate
    if let Err(e) = states_config.validate() {
        warn!(
            "Config reload failed validation (states): {}. Keeping current config.",
            e
        );
        return;
    }

    // Re-load the base config for dependencies, auto_advance, etc.
    let new_config = loader.into_config();
    if let Err(e) = new_config.dependencies.validate() {
        warn!(
            "Config reload failed validation (dependencies): {}. Keeping current config.",
            e
        );
        return;
    }

    // Wrap in Arc and build consolidated AppConfig
    let prompts = Arc::new(prompts);
    let workflows = Arc::new(workflows);
    let states_config = Arc::new(states_config);
    let phases_config = Arc::new(phases_config);
    let deps_config = Arc::new(new_config.dependencies.clone());
    let auto_advance = Arc::new(new_config.auto_advance.clone());
    let attachments_config = Arc::new(new_config.attachments.clone());
    let mut tags_config = new_config.tags.clone();
    tags_config.register_workflow_tags(&workflows.all_role_tags());
    let tags_config = Arc::new(tags_config);
    let ids_config = Arc::new(new_config.ids.clone());

    let app_config = AppConfig::new(
        Arc::clone(&states_config),
        Arc::clone(&phases_config),
        Arc::clone(&deps_config),
        Arc::clone(&auto_advance),
        Arc::clone(&attachments_config),
        Arc::clone(&tags_config),
        ids_config,
        Arc::clone(&workflows),
    );

    // Build new ToolHandler
    let new_tool_handler = Arc::new(ToolHandler::new(
        Arc::clone(&reload_ctx.db),
        reload_ctx.media_dir.clone(),
        reload_ctx.skills_dir.clone(),
        Arc::clone(&reload_ctx.server_paths),
        Arc::clone(&prompts),
        app_config.clone(),
        reload_ctx.default_format,
        reload_ctx.default_page_size,
        Arc::clone(&reload_ctx.path_mapper),
    ));

    // Build new ResourceHandler
    let docs_dir = discover_docs_dir();
    let mut new_resource_handler = ResourceHandler::new(Arc::clone(&reload_ctx.db), app_config)
        .with_skills_dir(reload_ctx.skills_dir.clone());
    if let Some(ref dir) = docs_dir {
        new_resource_handler = new_resource_handler.with_docs_dir(dir.clone());
    }
    let new_resource_handler = Arc::new(new_resource_handler);

    // Atomically swap in the new handlers
    server.tool_handler.store(new_tool_handler);
    server.resource_handler.store(new_resource_handler);
    server.prompts.store(prompts);

    info!("Configuration reloaded successfully");
}

/// Immutable context needed by the reload path -- values that do not change
/// when config files are edited on disk.
#[derive(Clone)]
struct ReloadContext {
    db: Arc<Database>,
    media_dir: std::path::PathBuf,
    skills_dir: std::path::PathBuf,
    server_paths: Arc<ServerPaths>,
    path_mapper: Arc<task_graph_mcp::paths::PathMapper>,
    default_format: OutputFormat,
    default_page_size: i32,
}

/// Run the MCP server
async fn run_server(
    config: Config,
    prompts: Prompts,
    workflows: WorkflowsConfig,
    config_path_used: Option<String>,
) -> Result<()> {
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

    // Create server handler -- build consolidated AppConfig
    let deps_config = Arc::new(config.dependencies.clone());
    let auto_advance = Arc::new(config.auto_advance.clone());
    let attachments_config = Arc::new(config.attachments.clone());
    let mut tags_config = config.tags.clone();
    tags_config.register_workflow_tags(&workflows.all_role_tags());
    let tags_config = Arc::new(tags_config);
    let ids_config = Arc::new(config.ids.clone());

    let app_config = AppConfig::new(
        Arc::clone(&states_config),
        Arc::clone(&phases_config),
        deps_config,
        auto_advance,
        attachments_config,
        tags_config,
        ids_config,
        Arc::clone(&workflows),
    );

    // Create path mapper from config
    let path_mapper = Arc::new(
        task_graph_mcp::paths::PathMapper::from_config(&config.paths, Some(&config))
            .map_err(|e| anyhow::anyhow!("Failed to create path mapper from config: {}", e))?,
    );

    // Create level filter for unified logging (defaults to Debug - logs everything)
    let level_filter = Arc::new(LogLevelFilter::default());

    let server = TaskGraphServer::new(
        Arc::clone(&db),
        config.server.media_dir.clone(),
        config.server.skills_dir.clone(),
        Arc::clone(&server_paths),
        prompts,
        app_config,
        config.server.default_format,
        config.server.default_page_size,
        Arc::clone(&path_mapper),
        level_filter,
    );

    // Build the reload context with immutable state needed for config hot-reload
    let reload_ctx = ReloadContext {
        db: Arc::clone(&db),
        media_dir: config.server.media_dir.clone(),
        skills_dir: config.server.skills_dir.clone(),
        server_paths: Arc::clone(&server_paths),
        path_mapper,
        default_format: config.server.default_format,
        default_page_size: config.server.default_page_size,
    };

    // Start config file watcher for hot-reload
    start_config_file_watcher(&server, reload_ctx, &config);

    // Start the HTTP dashboard server only when UI mode is explicitly set to Web.
    // When mode is "none", skip the dashboard entirely (bug fix: dashboard was
    // previously starting regardless of the ui.mode config setting).
    let _dashboard_handle = match config.server.ui.mode {
        UiMode::Web => {
            info!("Starting web dashboard on port {}", config.server.ui.port);
            Some(dashboard::start_server_with_retry(
                Arc::clone(&db),
                &config.server.ui,
                Arc::clone(&states_config),
            ))
        }
        UiMode::None => {
            info!("Web dashboard disabled (ui.mode = \"none\")");
            None
        }
    };

    // Run the stdio server
    info!("Server ready, listening on stdio");
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}

/// Start the config file watcher and spawn a background task that listens for
/// change events and triggers a config reload.
///
/// The watcher monitors:
/// - `config.yaml` and `prompts.yaml` in the project config directory
/// - `workflow-*.yaml` files in the project config directory
/// - The skills directory for added/modified/removed skill files
///
/// If the watcher fails to start (e.g., because the directories don't exist),
/// the server continues without hot-reload and a warning is logged.
fn start_config_file_watcher(server: &TaskGraphServer, reload_ctx: ReloadContext, config: &Config) {
    // Determine the config directory to watch.
    // Prefer the project's task-graph/ directory; fall back to .task-graph/.
    let config_dir = if std::path::Path::new("task-graph").exists() {
        Some(std::path::PathBuf::from("task-graph"))
    } else if std::path::Path::new(".task-graph").exists() {
        Some(std::path::PathBuf::from(".task-graph"))
    } else {
        None
    };

    let skills_dir = if config.server.skills_dir.exists() {
        Some(config.server.skills_dir.clone())
    } else {
        None
    };

    // If there is nothing to watch, skip.
    if config_dir.is_none() && skills_dir.is_none() {
        info!("No config or skills directory found to watch; hot-reload disabled");
        return;
    }

    let watch_paths = WatchPaths {
        config_dir,
        skills_dir,
    };

    let watcher_config = WatcherConfig::default();

    match start_config_watcher(watch_paths, watcher_config) {
        Ok(mut handle) => {
            info!("Config file watcher started for hot-reload");

            // Clone the server reference for the background task
            let server = server.clone();

            tokio::spawn(async move {
                loop {
                    match handle.wait_for_change().await {
                        Some(event) => {
                            if event.requires_reload() {
                                info!("Config change detected: {:?}", event);
                                reload_config(&server, &reload_ctx);
                            }
                        }
                        None => {
                            // Sender dropped -- watcher stopped
                            info!("Config file watcher stopped");
                            break;
                        }
                    }
                }
            });
        }
        Err(e) => {
            warn!(
                "Failed to start config file watcher: {}. \
                 Server will continue without hot-reload.",
                e
            );
        }
    }
}

/// Run the export command
fn run_export(config: &Config, args: ExportArgs) -> Result<()> {
    // Open database
    let db = Database::open(&config.server.db_path)?;

    // Build export options from CLI args
    let options = ExportOptions {
        exclude_deleted: args.exclude_deleted,
        tables: args.tables_to_export(),
    };

    // Export tables
    let export_tables = db.export_tables(&options)?;

    // Build snapshot
    let mut snapshot = Snapshot::new();

    // Convert ExportTables to Snapshot tables format
    if let Some(tasks) = export_tables.tasks {
        snapshot.tables.insert(
            "tasks".to_string(),
            tasks
                .into_iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
        );
    }
    if let Some(deps) = export_tables.dependencies {
        snapshot.tables.insert(
            "dependencies".to_string(),
            deps.into_iter()
                .map(|d| serde_json::to_value(d).unwrap())
                .collect(),
        );
    }
    if let Some(attachments) = export_tables.attachments {
        snapshot.tables.insert(
            "attachments".to_string(),
            attachments
                .into_iter()
                .map(|a| serde_json::to_value(a).unwrap())
                .collect(),
        );
    }
    if let Some(tags) = export_tables.task_tags {
        snapshot.tables.insert(
            "task_tags".to_string(),
            tags.into_iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
        );
    }
    if let Some(tags) = export_tables.task_needed_tags {
        snapshot.tables.insert(
            "task_needed_tags".to_string(),
            tags.into_iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
        );
    }
    if let Some(tags) = export_tables.task_wanted_tags {
        snapshot.tables.insert(
            "task_wanted_tags".to_string(),
            tags.into_iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
        );
    }
    if let Some(sequence) = export_tables.task_sequence {
        snapshot.tables.insert(
            "task_sequence".to_string(),
            sequence
                .into_iter()
                .map(|s| serde_json::to_value(s).unwrap())
                .collect(),
        );
    }

    // Serialize to JSON
    let json_output = snapshot.to_json_pretty()?;
    let json_bytes = json_output.as_bytes();

    // Determine if we should compress
    let should_compress = args.should_compress(Some(json_bytes.len() as u64));

    // Write output
    if let Some(ref path) = args.output {
        if should_compress {
            // Write gzipped
            use flate2::Compression;
            use flate2::write::GzEncoder;

            let file = std::fs::File::create(path)?;
            let mut encoder = GzEncoder::new(file, Compression::default());
            encoder.write_all(json_bytes)?;
            encoder.finish()?;
            eprintln!("Exported to {} (gzipped)", path.display());
        } else {
            // Write plain JSON
            std::fs::write(path, &json_output)?;
            eprintln!("Exported to {}", path.display());
        }
    } else {
        // Write to stdout
        if should_compress {
            use flate2::Compression;
            use flate2::write::GzEncoder;

            let stdout = std::io::stdout();
            let mut encoder = GzEncoder::new(stdout.lock(), Compression::default());
            encoder.write_all(json_bytes)?;
            let _ = encoder.finish()?;
        } else {
            print!("{}", json_output);
        }
    }

    Ok(())
}

/// Run the import command
fn run_import(config: &Config, args: ImportArgs) -> Result<()> {
    use task_graph_mcp::db::import::{ImportOptions, remap_snapshot};

    // Load snapshot from file
    let mut snapshot = Snapshot::from_file(&args.file)?;

    // Check schema compatibility
    if !snapshot.is_schema_compatible() {
        eprintln!(
            "Warning: Snapshot schema version {} differs from current version {}",
            snapshot.schema_version, CURRENT_SCHEMA_VERSION
        );
    }

    // Apply ID remapping if requested
    let remap_result = if args.remap_ids {
        let ids_config = config.ids.clone();
        let (remapped, id_map) = remap_snapshot(&snapshot, &ids_config)?;
        snapshot = remapped;
        eprintln!("Remapped {} task IDs to fresh IDs", id_map.len());
        Some(id_map)
    } else {
        None
    };

    // Open database
    let db = Database::open(&config.server.db_path)?;

    // Determine import options
    let mut options = if args.merge {
        ImportOptions::merge()
    } else {
        ImportOptions::replace()
    };
    if args.remap_ids {
        options.remap_ids = true;
    }
    if let Some(ref parent) = args.parent {
        options.parent_id = Some(parent.clone());
    }

    if args.dry_run {
        // Dry run - just validate and report
        let result = db.preview_import(&snapshot, &options);
        println!("Dry run results:");
        println!("  Mode: {:?}", result.mode);
        if args.remap_ids {
            println!("  ID remapping: enabled");
        }
        println!("  Database is empty: {}", result.database_is_empty);
        println!("  Would succeed: {}", result.would_succeed);
        if let Some(reason) = &result.failure_reason {
            println!("  Failure reason: {}", reason);
        }
        println!("  Would insert:");
        for (table, count) in &result.would_insert {
            println!("    {}: {}", table, count);
        }
        if !result.would_skip.is_empty() {
            println!("  Would skip:");
            for (table, count) in &result.would_skip {
                println!("    {}: {}", table, count);
            }
        }
        if !result.would_delete.is_empty() {
            println!("  Would delete:");
            for (table, count) in &result.would_delete {
                println!("    {}: {}", table, count);
            }
        }
        if !result.warnings.is_empty() {
            println!("  Warnings:");
            for warning in &result.warnings {
                println!("    - {}", warning);
            }
        }
        if let Some(ref id_map) = remap_result {
            println!("  ID remapping ({} tasks):", id_map.len());
            let mut entries: Vec<_> = id_map.iter().collect();
            entries.sort_by_key(|(old, _)| (*old).clone());
            for (old_id, new_id) in &entries {
                println!("    {} -> {}", old_id, new_id);
            }
        }
        return Ok(());
    }

    // Check if database has existing data and we're in replace mode without force
    // When remap_ids is active, the IDs are fresh so merge is the natural mode,
    // but if they chose replace mode, still check.
    if options.mode == ImportMode::Replace && !args.force && !args.remap_ids {
        let preview = db.preview_import(&snapshot, &options);
        if !preview.database_is_empty {
            anyhow::bail!(
                "Database contains existing data. Use --force to replace, or --merge to add."
            );
        }
    }

    // Perform import
    let mut result = db.import_snapshot(&snapshot, &options)?;

    // Attach the remap table to the result
    if let Some(id_map) = remap_result {
        result.id_remap = Some(id_map);
    }

    println!("Import complete:");
    println!("  Mode: {:?}", options.mode);
    if args.remap_ids {
        println!("  ID remapping: enabled");
    }
    println!("  Rows imported:");
    for (table, count) in &result.rows_imported {
        println!("    {}: {}", table, count);
    }
    if !result.rows_skipped.is_empty() {
        println!("  Rows skipped:");
        for (table, count) in &result.rows_skipped {
            println!("    {}: {}", table, count);
        }
    }
    if !result.rows_deleted.is_empty() {
        println!("  Rows deleted:");
        for (table, count) in &result.rows_deleted {
            println!("    {}: {}", table, count);
        }
    }
    println!("  FTS indexes rebuilt: {}", result.fts_rebuilt);
    if !result.warnings.is_empty() {
        println!("  Warnings:");
        for warning in &result.warnings {
            println!("    - {}", warning);
        }
    }
    if let Some(ref id_map) = result.id_remap {
        println!("  ID remapping ({} tasks):", id_map.len());
        let mut entries: Vec<_> = id_map.iter().collect();
        entries.sort_by_key(|(old, _)| (*old).clone());
        for (old_id, new_id) in &entries {
            println!("    {} -> {}", old_id, new_id);
        }
    }
    if !result.parent_linked_roots.is_empty() {
        println!(
            "  Parent linking: {} root(s) attached to '{}'",
            result.parent_linked_roots.len(),
            args.parent.as_deref().unwrap_or("?")
        );
        for root_id in &result.parent_linked_roots {
            println!("    -> {}", root_id);
        }
    }

    Ok(())
}

/// Run the diff command
fn run_diff(config: &Config, args: DiffArgs) -> Result<()> {
    // Load source snapshot
    let source = Snapshot::from_file(&args.source)?;

    let diff = if let Some(ref target_path) = args.target {
        // Two-file diff
        let target = Snapshot::from_file(target_path)?;
        let mut d = diff_snapshots(&source, &target);
        d.source_label = args.source.display().to_string();
        d.target_label = target_path.display().to_string();
        d
    } else {
        // Diff against database
        let db = Database::open(&config.server.db_path)?;
        let mut d = diff_snapshot_vs_database(&source, &db)?;
        d.source_label = args.source.display().to_string();
        d.target_label = "database".to_string();
        d
    };

    // Filter tables if requested
    let filtered_tables: std::collections::BTreeMap<_, _> = diff
        .tables
        .into_iter()
        .filter(|(name, _)| args.should_include_table(name))
        .collect();

    let diff = task_graph_mcp::export::diff::SnapshotDiff {
        source_label: diff.source_label,
        target_label: diff.target_label,
        tables: filtered_tables,
    };

    // Output based on format
    match args.format {
        DiffFormat::Text => {
            if args.summary_only {
                println!("Diff: {} -> {}", diff.source_label, diff.target_label);
                if diff.is_empty() {
                    println!("No differences found.");
                } else {
                    for (table, added, removed, modified) in diff.summary() {
                        println!("  {}: +{} -{} ~{}", table, added, removed, modified);
                    }
                    println!("Total: {} changes", diff.total_changes());
                }
            } else {
                print!("{}", diff);
            }
        }
        DiffFormat::Json => {
            let json = serde_json::to_string_pretty(&diff)?;
            println!("{}", json);
        }
        DiffFormat::Summary => {
            println!("Diff: {} -> {}", diff.source_label, diff.target_label);
            if diff.is_empty() {
                println!("No differences found.");
            } else {
                for (table, added, removed, modified) in diff.summary() {
                    println!("  {}: +{} -{} ~{}", table, added, removed, modified);
                }
                println!("Total: {} changes", diff.total_changes());
            }
        }
    }

    Ok(())
}
