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
use std::io::Write;
use std::sync::Arc;
use task_graph_mcp::cli::diff::DiffArgs;
use task_graph_mcp::cli::diff::DiffFormat;
use task_graph_mcp::cli::export::ExportArgs;
use task_graph_mcp::cli::import::ImportArgs;
use task_graph_mcp::cli::{Cli, Command, UiMode as CliUiMode, migrate};
use task_graph_mcp::config::{
    AttachmentsConfig, AutoAdvanceConfig, Config, ConfigLoader, DependenciesConfig, IdsConfig,
    PhasesConfig, Prompts, ServerPaths, StatesConfig, TagsConfig, UiMode,
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
use task_graph_mcp::tools::{ToolContext, ToolHandler};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

/// MCP server handler.
#[derive(Clone)]
struct TaskGraphServer {
    tool_handler: Arc<ToolHandler>,
    resource_handler: Arc<ResourceHandler>,
    prompts: Arc<Prompts>,
    /// Atomic level filter for logging (client can adjust via logging/setLevel).
    level_filter: Arc<LogLevelFilter>,
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
        tags_config: Arc<TagsConfig>,
        ids_config: Arc<IdsConfig>,
        workflows: Arc<WorkflowsConfig>,
        default_format: OutputFormat,
        path_mapper: Arc<task_graph_mcp::paths::PathMapper>,
        level_filter: Arc<LogLevelFilter>,
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
                Arc::clone(&tags_config),
                ids_config,
                Arc::clone(&workflows),
                default_format,
                path_mapper,
            )),
            resource_handler: Arc::new(
                ResourceHandler::new(
                    db,
                    states_config,
                    phases_config,
                    deps_config,
                    tags_config,
                    workflows,
                )
                .with_skills_dir(skills_dir),
            ),
            prompts,
            level_filter,
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
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, ErrorData> {
        // Create logger for this request
        let logger = Logger::new()
            .with_peer(context.peer.clone())
            .with_level_filter(Arc::clone(&self.level_filter))
            .with_name(format!("tool:{}", request.name));
        let tool_ctx = ToolContext::new(logger);

        let args = Value::Object(request.arguments.unwrap_or_default());
        match self
            .tool_handler
            .call_tool(&request.name, args, &tool_ctx)
            .await
        {
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

    // Create server handler
    let deps_config = Arc::new(config.dependencies.clone());
    let auto_advance = Arc::new(config.auto_advance.clone());
    let attachments_config = Arc::new(config.attachments.clone());
    let tags_config = Arc::new(config.tags.clone());
    let ids_config = Arc::new(config.ids.clone());

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
        server_paths,
        prompts,
        Arc::clone(&states_config),
        Arc::clone(&phases_config),
        deps_config,
        auto_advance,
        attachments_config,
        tags_config,
        ids_config,
        workflows,
        config.server.default_format,
        path_mapper,
        level_filter,
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
    use task_graph_mcp::db::import::ImportOptions;

    // Load snapshot from file
    let snapshot = Snapshot::from_file(&args.file)?;

    // Check schema compatibility
    if !snapshot.is_schema_compatible() {
        eprintln!(
            "Warning: Snapshot schema version {} differs from current version {}",
            snapshot.schema_version, CURRENT_SCHEMA_VERSION
        );
    }

    // Open database
    let db = Database::open(&config.server.db_path)?;

    // Determine import options
    let options = if args.merge {
        ImportOptions::merge()
    } else {
        ImportOptions::replace()
    };

    if args.dry_run {
        // Dry run - just validate and report
        let result = db.preview_import(&snapshot, &options);
        println!("Dry run results:");
        println!("  Mode: {:?}", result.mode);
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
        return Ok(());
    }

    // Check if database has existing data and we're in replace mode without force
    if options.mode == ImportMode::Replace && !args.force {
        let preview = db.preview_import(&snapshot, &options);
        if !preview.database_is_empty {
            anyhow::bail!(
                "Database contains existing data. Use --force to replace, or --merge to add."
            );
        }
    }

    // Perform import
    let result = db.import_snapshot(&snapshot, &options)?;

    println!("Import complete:");
    println!("  Mode: {:?}", options.mode);
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
