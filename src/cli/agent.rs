//! CLI subcommands for background agents.
//!
//! Provides CLI access to task-graph operations for background agents
//! that don't have MCP access. Calls the same underlying tool functions
//! used by the MCP server.

use crate::config::{
    AppConfig, Config, ConfigLoader, PhasesConfig, Prompts, ServerPaths, StatesConfig,
    workflows::WorkflowsConfig,
};
use crate::db::Database;
use crate::format::{OutputFormat, ToolResult};
use crate::prompts as prompt_system;
use crate::tools::{ToolHandler, advisories, agents, attachments, claiming, tasks, tracking};
use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

/// CLI exit codes for programmatic error handling.
pub mod exit_codes {
    pub const SUCCESS: u8 = 0;
    pub const GENERAL_ERROR: u8 = 1;
    pub const INVALID_ARGUMENTS: u8 = 2;
    pub const TASK_NOT_FOUND: u8 = 3;
    pub const WORKER_NOT_FOUND: u8 = 4;
    pub const CLAIM_FAILED: u8 = 5;
    pub const PERMISSION_DENIED: u8 = 6;
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum CliOutputFormat {
    /// Markdown output (default, matches MCP behavior)
    #[default]
    Markdown,
    /// JSON output
    Json,
}

impl From<CliOutputFormat> for OutputFormat {
    fn from(f: CliOutputFormat) -> Self {
        match f {
            CliOutputFormat::Markdown => OutputFormat::Markdown,
            CliOutputFormat::Json => OutputFormat::Json,
        }
    }
}

/// Agent subcommand arguments.
#[derive(Args, Debug)]
pub struct AgentArgs {
    /// Worker ID (overrides TASK_GRAPH_WORKER_ID env var and config)
    #[arg(long, global = true)]
    pub worker_id: Option<String>,

    /// Output format: markdown (default) or json
    #[arg(long, global = true, value_enum, default_value = "markdown")]
    pub format: CliOutputFormat,

    #[command(subcommand)]
    pub command: AgentCommand,
}

/// Available agent subcommands.
#[derive(Subcommand, Debug)]
pub enum AgentCommand {
    /// Register as a worker
    Connect(ConnectArgs),

    /// Unregister and release all claims
    Disconnect(DisconnectArgs),

    /// Query tasks with flexible filters
    #[command(alias = "ls")]
    ListTasks(ListTasksArgs),

    /// Get detailed task information
    Get(GetArgs),

    /// Claim a task for work
    Claim(ClaimArgs),

    /// Update task properties
    Update(UpdateArgs),

    /// Broadcast real-time status updates
    Thinking(ThinkingArgs),

    /// Add an attachment to a task
    Attach(AttachArgs),

    /// List connected workers
    ListAgents(ListAgentsArgs),

    /// Query workflow prompts and advisories
    Prompts(PromptsArgs),

    /// Run commands interactively or from stdin/file
    #[command(alias = "repl")]
    Interactive(InteractiveArgs),

    /// Execute commands from a file (@file syntax alternative)
    Batch(BatchArgs),
}

/// Arguments for interactive/batch mode.
#[derive(Args, Debug)]
pub struct InteractiveArgs {
    /// Read commands from stdin instead of prompting
    #[arg(long)]
    pub stdin: bool,
}

/// Arguments for batch file execution.
#[derive(Args, Debug)]
pub struct BatchArgs {
    /// Path to file containing commands (one per line)
    pub file: PathBuf,

    /// Continue on errors instead of stopping
    #[arg(long, short = 'k')]
    pub keep_going: bool,
}

/// Arguments for the connect command.
#[derive(Args, Debug)]
pub struct ConnectArgs {
    /// Worker ID (optional, auto-generated if not provided)
    pub worker_id: Option<String>,

    /// Capability/role tags (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,

    /// Named workflow to use (e.g., 'swarm')
    #[arg(long)]
    pub workflow: Option<String>,

    /// Overlays to apply on top of workflow (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub overlays: Vec<String>,

    /// Force reconnection if worker ID already exists
    #[arg(long)]
    pub force: bool,
}

/// Arguments for the disconnect command.
#[derive(Args, Debug)]
pub struct DisconnectArgs {
    /// Worker ID to disconnect
    pub worker_id: String,

    /// Status to set released tasks to
    #[arg(long)]
    pub final_status: Option<String>,
}

/// Arguments for the list-tasks command.
#[derive(Args, Debug)]
pub struct ListTasksArgs {
    /// Filter for claimable tasks
    #[arg(long)]
    pub ready: bool,

    /// Filter for blocked tasks
    #[arg(long)]
    pub blocked: bool,

    /// Filter by status (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub status: Vec<String>,

    /// Filter by parent task ID ('null' for root tasks)
    #[arg(long)]
    pub parent: Option<String>,

    /// Maximum number of tasks to return
    #[arg(long)]
    pub limit: Option<i32>,

    /// Number of tasks to skip for pagination
    #[arg(long)]
    pub offset: Option<i32>,
}

/// Arguments for the get command.
#[derive(Args, Debug)]
pub struct GetArgs {
    /// Task ID to retrieve
    pub task_id: String,
}

/// Arguments for the claim command.
#[derive(Args, Debug)]
pub struct ClaimArgs {
    /// Worker ID claiming the task
    pub worker_id: String,

    /// Task ID to claim
    pub task_id: String,

    /// Force claim even if owned by another agent
    #[arg(long)]
    pub force: bool,
}

/// Arguments for the update command.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Worker ID performing the update
    pub worker_id: String,

    /// Task ID to update
    pub task_id: String,

    /// New status
    #[arg(long)]
    pub status: Option<String>,

    /// New title
    #[arg(long)]
    pub title: Option<String>,

    /// New description
    #[arg(long)]
    pub description: Option<String>,

    /// Reason for the update (stored in audit trail)
    #[arg(long)]
    pub reason: Option<String>,

    /// Force update even if owned by another worker
    #[arg(long)]
    pub force: bool,
}

/// Arguments for the thinking command.
#[derive(Args, Debug)]
pub struct ThinkingArgs {
    /// Worker ID broadcasting the thought
    pub worker_id: String,

    /// Current thought/status message
    pub message: String,

    /// Specific task IDs to update (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub tasks: Vec<String>,
}

/// Arguments for the attach command.
#[derive(Args, Debug)]
pub struct AttachArgs {
    /// Worker ID adding the attachment
    pub worker_id: String,

    /// Task ID to attach to
    pub task_id: String,

    /// Attachment type/category (e.g., 'commit', 'note')
    #[arg(long, short = 't')]
    pub r#type: String,

    /// Attachment content (text)
    #[arg(long, short = 'c', conflicts_with = "file")]
    pub content: Option<String>,

    /// Read content from file
    #[arg(long, short = 'f', conflicts_with = "content")]
    pub file: Option<PathBuf>,

    /// Optional label/name for the attachment
    #[arg(long)]
    pub name: Option<String>,
}

/// Arguments for the list-agents command.
#[derive(Args, Debug)]
pub struct ListAgentsArgs {
    /// Filter workers that have ALL of these tags (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,

    /// Filter workers that have claimed this file
    #[arg(long)]
    pub file: Option<String>,

    /// Filter workers related to this task ID
    #[arg(long)]
    pub task: Option<String>,
}

/// Arguments for the prompts command.
#[derive(Args, Debug)]
pub struct PromptsArgs {
    /// Show prompts for entering this status
    #[arg(long)]
    pub status: Option<String>,

    /// Show prompts for entering this phase
    #[arg(long)]
    pub phase: Option<String>,

    /// List advisories or show a specific one (use without value to list, with value to show)
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    pub advisory: Option<String>,

    /// Task ID for context-sensitive filtering
    #[arg(long)]
    pub task: Option<String>,
}

/// Build a ToolHandler with the given configuration.
fn build_tool_handler(
    db: Arc<Database>,
    config: &Config,
    prompts: Arc<Prompts>,
    workflows: Arc<WorkflowsConfig>,
    server_paths: Arc<ServerPaths>,
) -> ToolHandler {
    // Derive states and phases from workflows
    let states_config: StatesConfig = workflows.as_ref().into();
    let phases_config: PhasesConfig = workflows.as_ref().into();

    // Wrap configs in Arc
    let states_config = Arc::new(states_config);
    let phases_config = Arc::new(phases_config);
    let deps_config = Arc::new(config.dependencies.clone());
    let auto_advance = Arc::new(config.auto_advance.clone());
    let attachments_config = Arc::new(config.attachments.clone());
    let mut tags_config = config.tags.clone();
    tags_config.register_workflow_tags(&workflows.all_role_tags());
    let tags_config = Arc::new(tags_config);
    let ids_config = Arc::new(config.ids.clone());
    let feedback_config = Arc::new(config.feedback.clone());

    let app_config = AppConfig::new(
        states_config,
        phases_config,
        deps_config,
        auto_advance,
        attachments_config,
        tags_config,
        ids_config,
        workflows,
        feedback_config,
    );

    // Create path mapper
    let path_mapper = Arc::new(
        crate::paths::PathMapper::from_config(&config.paths, Some(config))
            .unwrap_or_else(|_| crate::paths::PathMapper::default()),
    );

    ToolHandler::new(
        db,
        config.server.media_dir.clone(),
        config.server.skills_dir.clone(),
        server_paths,
        prompts,
        app_config,
        config.server.default_format,
        config.server.default_page_size,
        path_mapper,
    )
}

/// Format output according to the specified format.
fn format_output(result: ToolResult, format: CliOutputFormat) -> String {
    match result {
        ToolResult::Json(v) => {
            if format == CliOutputFormat::Json {
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
            } else {
                // For markdown format, we need to convert JSON to a readable format
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
            }
        }
        ToolResult::Raw(s) => {
            if format == CliOutputFormat::Json {
                // Wrap raw text in JSON
                json!({ "output": s }).to_string()
            } else {
                s
            }
        }
    }
}

/// Format a JSON value as pretty-printed JSON (fallback for commands without dedicated formatters).
fn format_json_output(value: Value, format: CliOutputFormat) -> String {
    if format == CliOutputFormat::Json {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    } else {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    }
}

/// Format connect response with structured markdown.
fn format_connect_output(value: Value, format: CliOutputFormat) -> String {
    if format == CliOutputFormat::Json {
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    }

    let mut out = String::new();

    // Worker ID and tags
    if let Some(wid) = value.get("worker_id").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Worker ID:** `{}`\n", wid));
    }
    if let Some(tags) = value.get("tags").and_then(|v| v.as_array()) {
        let tag_list: Vec<&str> = tags.iter().filter_map(|t| t.as_str()).collect();
        if !tag_list.is_empty() {
            out.push_str(&format!("**Tags:** {}\n", tag_list.join(", ")));
        }
    }
    if let Some(workflow) = value.get("workflow").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Workflow:** {}\n", workflow));
    }
    if let Some(overlays) = value.get("overlays").and_then(|v| v.as_array()) {
        let names: Vec<&str> = overlays.iter().filter_map(|o| o.as_str()).collect();
        if !names.is_empty() {
            out.push_str(&format!("**Overlays:** {}\n", names.join(", ")));
        }
    }

    // Role info
    if let Some(role) = value.get("role") {
        out.push('\n');
        if let Some(name) = role.get("role").and_then(|v| v.as_str()) {
            out.push_str(&format!("**Role:** `{}`", name));
        }
        if let Some(desc) = role.get("description").and_then(|v| v.as_str()) {
            out.push_str(&format!(" - {}", desc));
        }
        out.push('\n');
    }

    // State machine summary
    if let Some(config) = value.get("config") {
        out.push_str("\n### State Machine\n");
        if let Some(initial) = config.get("initial_state").and_then(|v| v.as_str()) {
            out.push_str(&format!("- **Initial:** `{}`\n", initial));
        }
        if let Some(states) = config.get("states").and_then(|v| v.as_array()) {
            let names: Vec<&str> = states.iter().filter_map(|s| s.as_str()).collect();
            out.push_str(&format!("- **States:** {}\n", names.join(", ")));
        }
        if let Some(timed) = config.get("timed_states").and_then(|v| v.as_array()) {
            let names: Vec<&str> = timed.iter().filter_map(|s| s.as_str()).collect();
            if !names.is_empty() {
                out.push_str(&format!("- **Timed:** {}\n", names.join(", ")));
            }
        }
        if let Some(terminal) = config.get("terminal_states").and_then(|v| v.as_array()) {
            let names: Vec<&str> = terminal.iter().filter_map(|s| s.as_str()).collect();
            if !names.is_empty() {
                out.push_str(&format!("- **Terminal:** {}\n", names.join(", ")));
            }
        }
        if let Some(phases) = config.get("phases").and_then(|v| v.as_array()) {
            let names: Vec<&str> = phases.iter().filter_map(|s| s.as_str()).collect();
            if !names.is_empty() {
                out.push_str(&format!("- **Phases:** {}\n", names.join(", ")));
            }
        }
    }

    // Role prompts
    if let Some(prompts) = value.get("role_prompts").and_then(|v| v.as_array()) {
        out.push_str("\n### Role Prompts\n");
        for prompt in prompts {
            if let Some(text) = prompt.as_str() {
                // Render each prompt as a blockquote
                for line in text.lines() {
                    out.push_str(&format!("> {}\n", line));
                }
                out.push_str("---\n");
            }
        }
    }

    // Workflow description
    if let Some(desc) = value.get("workflow_description").and_then(|v| v.as_str()) {
        out.push_str("\n### Workflow\n");
        out.push_str(desc);
        out.push('\n');
    }

    // Paths
    if let Some(paths) = value.get("paths") {
        out.push_str("\n### Paths\n");
        if let Some(db) = paths.get("db_path").and_then(|v| v.as_str()) {
            out.push_str(&format!("- db: `{}`\n", db));
        }
        if let Some(media) = paths.get("media_dir").and_then(|v| v.as_str()) {
            out.push_str(&format!("- media: `{}`\n", media));
        }
        if let Some(log) = paths.get("log_dir").and_then(|v| v.as_str()) {
            out.push_str(&format!("- log: `{}`\n", log));
        }
    }

    // Warnings
    if let Some(warnings) = value.get("path_warnings").and_then(|v| v.as_array()) {
        out.push_str("\n### Warnings\n");
        for w in warnings {
            if let Some(text) = w.as_str() {
                out.push_str(&format!("- {}\n", text));
            }
        }
    }
    if let Some(warnings) = value.get("tag_warnings").and_then(|v| v.as_array()) {
        for w in warnings {
            if let Some(text) = w.as_str() {
                out.push_str(&format!("- {}\n", text));
            }
        }
    }

    out
}

/// Format update response with structured markdown, surfacing prompts prominently.
fn format_update_output(value: Value, format: CliOutputFormat) -> String {
    if format == CliOutputFormat::Json {
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    }

    let mut out = String::new();

    // Task summary
    if let Some(task_id) = value.get("task").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Task:** `{}`", task_id));
    }
    if let Some(title) = value.get("title").and_then(|v| v.as_str()) {
        out.push_str(&format!(" - {}", title));
    }
    out.push('\n');
    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Status:** `{}`\n", status));
    }
    if let Some(phase) = value.get("phase").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Phase:** `{}`\n", phase));
    }

    // Transition prompts - render prominently
    format_prompts_section(&value, &mut out);

    // Advisory hints
    if let Some(hints) = value.get("advisory_hints").and_then(|v| v.as_array()) {
        if !hints.is_empty() {
            let names: Vec<&str> = hints.iter().filter_map(|h| h.as_str()).collect();
            out.push_str(&format!(
                "\n**Advisories:** `get_advisory` topics: {}\n",
                names.join(", ")
            ));
        }
    }

    // Warnings
    if let Some(warnings) = value.get("warnings").and_then(|v| v.as_array()) {
        for w in warnings {
            if let Some(text) = w.as_str() {
                out.push_str(&format!("- \u{26a0} {}\n", text));
            }
        }
    }

    out
}

/// Format claim response with structured markdown, surfacing prompts prominently.
fn format_claim_output(value: Value, format: CliOutputFormat) -> String {
    if format == CliOutputFormat::Json {
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    }

    let mut out = String::new();

    // Task summary
    if let Some(task_id) = value.get("task").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Task:** `{}`", task_id));
    }
    if let Some(title) = value.get("title").and_then(|v| v.as_str()) {
        out.push_str(&format!(" - {}", title));
    }
    out.push('\n');
    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Status:** `{}`\n", status));
    }
    if let Some(owner) = value.get("owner").and_then(|v| v.as_str()) {
        out.push_str(&format!("**Owner:** `{}`\n", owner));
    }

    // Transition prompts - render prominently
    format_prompts_section(&value, &mut out);

    // Advisory hints
    if let Some(hints) = value.get("advisory_hints").and_then(|v| v.as_array()) {
        if !hints.is_empty() {
            let names: Vec<&str> = hints.iter().filter_map(|h| h.as_str()).collect();
            out.push_str(&format!(
                "\n**Advisories:** `get_advisory` topics: {}\n",
                names.join(", ")
            ));
        }
    }

    out
}

/// Render transition prompts as blockquotes, shared by update and claim formatters.
fn format_prompts_section(value: &Value, out: &mut String) {
    if let Some(prompts) = value.get("prompts").and_then(|v| v.as_array()) {
        if !prompts.is_empty() {
            out.push_str("\n### Guidance\n");
            for (i, prompt) in prompts.iter().enumerate() {
                if let Some(text) = prompt.as_str() {
                    for line in text.lines() {
                        out.push_str(&format!("> {}\n", line));
                    }
                    if i + 1 < prompts.len() {
                        out.push_str("\n---\n\n");
                    }
                }
            }
        }
    }
}

/// Map error codes to CLI exit codes.
fn error_to_exit_code(err: &anyhow::Error) -> u8 {
    // Try to downcast to ToolError for precise error code mapping
    if let Some(tool_err) = err.downcast_ref::<crate::error::ToolError>() {
        use crate::error::ErrorCode;
        return match tool_err.code {
            ErrorCode::TaskNotFound | ErrorCode::FileNotFound | ErrorCode::AttachmentNotFound => {
                exit_codes::TASK_NOT_FOUND
            }
            ErrorCode::AgentNotFound => exit_codes::WORKER_NOT_FOUND,
            ErrorCode::AlreadyClaimed
            | ErrorCode::LockConflict
            | ErrorCode::DependencyCycle
            | ErrorCode::DependencyNotSatisfied
            | ErrorCode::GatesNotSatisfied
            | ErrorCode::TagMismatch => exit_codes::CLAIM_FAILED,
            ErrorCode::NotOwner => exit_codes::PERMISSION_DENIED,
            ErrorCode::MissingRequiredField
            | ErrorCode::InvalidFieldValue
            | ErrorCode::InvalidState
            | ErrorCode::InvalidPath
            | ErrorCode::InvalidPrefix => exit_codes::INVALID_ARGUMENTS,
            ErrorCode::AlreadyExists => exit_codes::CLAIM_FAILED,
            ErrorCode::DatabaseError | ErrorCode::InternalError | ErrorCode::UnknownTool => {
                exit_codes::GENERAL_ERROR
            }
        };
    }

    // Fallback: string matching for non-ToolError errors
    let msg = err.to_string().to_lowercase();
    if msg.contains("not found") {
        if msg.contains("task") {
            exit_codes::TASK_NOT_FOUND
        } else if msg.contains("worker") || msg.contains("agent") {
            exit_codes::WORKER_NOT_FOUND
        } else {
            exit_codes::GENERAL_ERROR
        }
    } else if msg.contains("already claimed")
        || msg.contains("dependency")
        || msg.contains("blocked")
    {
        exit_codes::CLAIM_FAILED
    } else if msg.contains("not own") || msg.contains("permission") {
        exit_codes::PERMISSION_DENIED
    } else if msg.contains("required") || msg.contains("invalid") {
        exit_codes::INVALID_ARGUMENTS
    } else {
        exit_codes::GENERAL_ERROR
    }
}

/// Run agent subcommands.
pub fn run_agent_command(args: AgentArgs) -> ExitCode {
    // Load configuration
    let loader = match ConfigLoader::load() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            return ExitCode::from(exit_codes::GENERAL_ERROR);
        }
    };

    // Get config from loader
    let config = loader.config();

    // Load prompts and workflows
    let prompts = Arc::new(loader.load_prompts());
    let workflows = Arc::new(load_workflows(&loader, config));

    // Open database
    let db = match Database::open(&config.server.db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            return ExitCode::from(exit_codes::GENERAL_ERROR);
        }
    };

    // Create server paths
    let server_paths = Arc::new(ServerPaths {
        db_path: config.server.db_path.clone(),
        media_dir: config.server.media_dir.clone(),
        log_dir: config.server.log_dir.clone(),
        config_path: loader.config_path().map(PathBuf::from),
    });

    // Build tool handler
    let handler = build_tool_handler(
        Arc::clone(&db),
        config,
        Arc::clone(&prompts),
        Arc::clone(&workflows),
        Arc::clone(&server_paths),
    );

    // Execute the command
    match &args.command {
        AgentCommand::Interactive(cmd_args) => {
            return run_interactive(&handler, &args, cmd_args);
        }
        AgentCommand::Batch(cmd_args) => {
            return run_batch(&handler, &args, cmd_args);
        }
        _ => {}
    }

    let result = match &args.command {
        AgentCommand::Connect(cmd_args) => run_connect(&handler, &args, cmd_args),
        AgentCommand::Disconnect(cmd_args) => run_disconnect(&handler, &args, cmd_args),
        AgentCommand::ListTasks(cmd_args) => run_list_tasks(&handler, &args, cmd_args),
        AgentCommand::Get(cmd_args) => run_get(&handler, &args, cmd_args),
        AgentCommand::Claim(cmd_args) => run_claim(&handler, &args, cmd_args),
        AgentCommand::Update(cmd_args) => run_update(&handler, &args, cmd_args),
        AgentCommand::Thinking(cmd_args) => run_thinking(&handler, &args, cmd_args),
        AgentCommand::Attach(cmd_args) => run_attach(&handler, &args, cmd_args),
        AgentCommand::ListAgents(cmd_args) => run_list_agents(&handler, &args, cmd_args),
        AgentCommand::Prompts(cmd_args) => run_prompts(&handler, &args, cmd_args),
        // Interactive and Batch handled above
        AgentCommand::Interactive(_) | AgentCommand::Batch(_) => unreachable!(),
    };

    match result {
        Ok(output) => {
            println!("{}", output);
            ExitCode::from(exit_codes::SUCCESS)
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(error_to_exit_code(&e))
        }
    }
}

/// Run agent subcommands and exit the process.
/// This is a convenience wrapper that calls std::process::exit with the
/// appropriate exit code, since the main function returns Result<()>.
pub fn run_agent_command_and_exit(args: AgentArgs) -> ! {
    let exit_code = run_agent_command(args);
    // Map ExitCode to u8 via matching on expected codes
    let code = match exit_code {
        code if code == ExitCode::from(exit_codes::SUCCESS) => exit_codes::SUCCESS,
        code if code == ExitCode::from(exit_codes::GENERAL_ERROR) => exit_codes::GENERAL_ERROR,
        code if code == ExitCode::from(exit_codes::INVALID_ARGUMENTS) => {
            exit_codes::INVALID_ARGUMENTS
        }
        code if code == ExitCode::from(exit_codes::TASK_NOT_FOUND) => exit_codes::TASK_NOT_FOUND,
        code if code == ExitCode::from(exit_codes::WORKER_NOT_FOUND) => {
            exit_codes::WORKER_NOT_FOUND
        }
        code if code == ExitCode::from(exit_codes::CLAIM_FAILED) => exit_codes::CLAIM_FAILED,
        code if code == ExitCode::from(exit_codes::PERMISSION_DENIED) => {
            exit_codes::PERMISSION_DENIED
        }
        _ => exit_codes::GENERAL_ERROR,
    };
    std::process::exit(code as i32);
}

/// Load workflows with cache (similar to main.rs pattern).
fn load_workflows(loader: &ConfigLoader, config: &Config) -> WorkflowsConfig {
    let default_workflow_name = config.server.default_workflow.clone();

    // If a default workflow is configured, load it as the base
    let mut workflows = if let Some(ref name) = default_workflow_name {
        match loader.load_workflow_by_name(name) {
            Ok(workflow_config) => workflow_config,
            Err(_) => loader.load_workflows(),
        }
    } else {
        loader.load_workflows()
    };

    // Load named workflows into cache
    for name in loader.list_workflows() {
        if let Ok(workflow_config) = loader.load_workflow_by_name(&name) {
            workflows
                .named_workflows
                .insert(name, Arc::new(workflow_config));
        }
    }

    // Load overlays into cache
    for name in loader.list_overlays() {
        if let Ok(overlay_config) = loader.load_overlay_by_name(&name) {
            workflows
                .named_overlays
                .insert(name, Arc::new(overlay_config));
        }
    }

    workflows
}

// Command handlers

fn run_connect(handler: &ToolHandler, args: &AgentArgs, cmd_args: &ConnectArgs) -> Result<String> {
    // For connect, worker_id is optional (can be auto-generated)
    // Use cmd_args.worker_id or fall back to global --worker-id
    let worker_id = cmd_args
        .worker_id
        .clone()
        .or_else(|| args.worker_id.clone());

    // Resolve base workflow
    let base_workflow = cmd_args
        .workflow
        .as_ref()
        .and_then(|name| handler.config.workflows.get_named_workflow(name))
        .map(Arc::clone)
        .or_else(|| {
            handler
                .config
                .workflows
                .get_default_workflow()
                .map(Arc::clone)
        })
        .unwrap_or_else(|| Arc::clone(&handler.config.workflows));

    // Apply overlays if specified
    let workflow = if cmd_args.overlays.is_empty() {
        base_workflow
    } else {
        let mut merged = (*base_workflow).clone();
        for name in &cmd_args.overlays {
            if let Some(overlay) = handler.config.workflows.named_overlays.get(name) {
                merged.apply_overlay(overlay);
            }
        }
        merged.active_overlays = cmd_args.overlays.clone();
        Arc::new(merged)
    };

    let tool_args = json!({
        "worker_id": worker_id,
        "tags": cmd_args.tags,
        "force": cmd_args.force,
        "workflow": cmd_args.workflow,
        "overlays": cmd_args.overlays
    });

    let result = agents::connect(
        agents::ConnectOptions {
            db: &handler.db,
            server_paths: &handler.server_paths,
            config: &handler.config,
            workflows: &workflow,
        },
        tool_args,
    )?;

    Ok(format_connect_output(result, args.format))
}

fn run_disconnect(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd_args: &DisconnectArgs,
) -> Result<String> {
    // Derive states from workflows
    let states_config: StatesConfig = handler.config.workflows.as_ref().into();

    let tool_args = json!({
        "worker_id": &cmd_args.worker_id,
        "final_status": cmd_args.final_status
    });

    let result = agents::disconnect(&handler.db, &states_config, tool_args)?;
    Ok(format_json_output(result, args.format))
}

fn run_list_tasks(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd_args: &ListTasksArgs,
) -> Result<String> {
    let states_config: StatesConfig = handler.config.workflows.as_ref().into();

    let mut tool_args = json!({
        "ready": cmd_args.ready,
        "blocked": cmd_args.blocked,
        "format": if args.format == CliOutputFormat::Json { "json" } else { "markdown" }
    });

    if !cmd_args.status.is_empty() {
        tool_args["status"] = json!(cmd_args.status);
    }
    if let Some(ref parent) = cmd_args.parent {
        tool_args["parent"] = json!(parent);
    }
    if let Some(limit) = cmd_args.limit {
        tool_args["limit"] = json!(limit);
    }
    if let Some(offset) = cmd_args.offset {
        tool_args["offset"] = json!(offset);
    }

    let result = tasks::list_tasks(
        &handler.db,
        &states_config,
        &handler.config.deps,
        args.format.into(),
        tool_args,
    )?;

    Ok(format_output(result, args.format))
}

fn run_get(handler: &ToolHandler, args: &AgentArgs, cmd_args: &GetArgs) -> Result<String> {
    let tool_args = json!({
        "task": cmd_args.task_id,
        "format": if args.format == CliOutputFormat::Json { "json" } else { "markdown" }
    });

    let result = tasks::get(&handler.db, args.format.into(), tool_args)?;
    Ok(format_output(result, args.format))
}

fn run_claim(handler: &ToolHandler, args: &AgentArgs, cmd_args: &ClaimArgs) -> Result<String> {
    // Get worker's workflow
    let workflow = handler.get_workflow_for_worker(&cmd_args.worker_id);

    let tool_args = json!({
        "worker_id": &cmd_args.worker_id,
        "task": cmd_args.task_id,
        "force": cmd_args.force
    });

    let result = claiming::claim(&handler.db, &handler.config, &workflow, tool_args)?;
    Ok(format_claim_output(result, args.format))
}

fn run_update(handler: &ToolHandler, args: &AgentArgs, cmd_args: &UpdateArgs) -> Result<String> {
    // Get worker's workflow
    let workflow = handler.get_workflow_for_worker(&cmd_args.worker_id);

    let mut tool_args = json!({
        "worker_id": &cmd_args.worker_id,
        "task": cmd_args.task_id,
        "force": cmd_args.force
    });

    if let Some(ref status) = cmd_args.status {
        tool_args["status"] = json!(status);
    }
    if let Some(ref title) = cmd_args.title {
        tool_args["title"] = json!(title);
    }
    if let Some(ref description) = cmd_args.description {
        tool_args["description"] = json!(description);
    }
    if let Some(ref reason) = cmd_args.reason {
        tool_args["reason"] = json!(reason);
    }

    let result = tasks::update(
        tasks::UpdateOptions {
            db: &handler.db,
            config: &handler.config,
            workflows: &workflow,
        },
        tool_args,
    )?;
    Ok(format_update_output(result, args.format))
}

fn run_thinking(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd_args: &ThinkingArgs,
) -> Result<String> {
    let mut tool_args = json!({
        "agent": &cmd_args.worker_id,
        "thought": cmd_args.message
    });

    if !cmd_args.tasks.is_empty() {
        tool_args["tasks"] = json!(cmd_args.tasks);
    }

    let result = tracking::thinking(&handler.db, tool_args)?;
    Ok(format_json_output(result, args.format))
}

fn run_attach(handler: &ToolHandler, args: &AgentArgs, cmd_args: &AttachArgs) -> Result<String> {
    // Resolve content from --content or --file
    let content = if let Some(ref content) = cmd_args.content {
        content.clone()
    } else if let Some(ref file_path) = cmd_args.file {
        std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", file_path.display(), e))?
    } else {
        return Err(anyhow::anyhow!(
            "Either --content or --file must be provided"
        ));
    };

    let mut tool_args = json!({
        "agent": &cmd_args.worker_id,
        "task": cmd_args.task_id,
        "type": cmd_args.r#type,
        "content": content
    });

    if let Some(ref name) = cmd_args.name {
        tool_args["name"] = json!(name);
    }

    let result = attachments::attach(
        &handler.db,
        &handler.media_dir,
        &handler.config.attachments,
        tool_args,
    )?;
    Ok(format_json_output(result, args.format))
}

fn run_list_agents(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd_args: &ListAgentsArgs,
) -> Result<String> {
    let states_config: StatesConfig = handler.config.workflows.as_ref().into();

    let mut tool_args = json!({
        "format": if args.format == CliOutputFormat::Json { "json" } else { "markdown" }
    });

    if !cmd_args.tags.is_empty() {
        tool_args["tags"] = json!(cmd_args.tags);
    }
    if let Some(ref file) = cmd_args.file {
        tool_args["file"] = json!(file);
    }
    if let Some(ref task) = cmd_args.task {
        tool_args["task"] = json!(task);
    }

    let result = agents::list_agents(&handler.db, &states_config, args.format.into(), tool_args)?;
    Ok(format_output(result, args.format))
}

fn run_prompts(handler: &ToolHandler, args: &AgentArgs, cmd_args: &PromptsArgs) -> Result<String> {
    let workflows = &handler.config.workflows;

    // Advisory mode
    if let Some(ref advisory_topic) = cmd_args.advisory {
        let mut tool_args = json!({});
        if !advisory_topic.is_empty() {
            tool_args["topic"] = json!(advisory_topic);
        }
        if let Some(ref task_id) = cmd_args.task {
            tool_args["task"] = json!(task_id);
        }
        if let Some(ref wid) = args.worker_id {
            tool_args["worker_id"] = json!(wid);
        }

        let result = advisories::get_advisory(&handler.db, workflows, tool_args)?;

        if args.format == CliOutputFormat::Json {
            return Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
        }

        // Markdown rendering for advisories
        if advisory_topic.is_empty() {
            // List mode
            let mut out = String::from("### Advisories\n\n");
            if let Some(topics) = result.get("advisories").and_then(|v| v.as_array()) {
                for entry in topics {
                    let name = entry.get("topic").and_then(|v| v.as_str()).unwrap_or("?");
                    let relevant = entry
                        .get("relevant")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let marker = if relevant { " *" } else { "" };
                    out.push_str(&format!("- `{}`{}\n", name, marker));
                }
                if let Some(count) = result.get("count").and_then(|v| v.as_i64()) {
                    out.push_str(&format!(
                        "\n{} advisories (* = relevant to current context)\n",
                        count
                    ));
                }
            }
            return Ok(out);
        } else {
            // Detail mode
            let mut out = String::new();
            if let Some(topic) = result.get("topic").and_then(|v| v.as_str()) {
                out.push_str(&format!("### Advisory: {}\n\n", topic));
            }
            if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
                out.push_str(content);
                out.push('\n');
            }
            return Ok(out);
        }
    }

    // State/phase mode
    if cmd_args.status.is_some() || cmd_args.phase.is_some() {
        let states_config: StatesConfig = workflows.as_ref().into();
        let phases_config: PhasesConfig = workflows.as_ref().into();

        let target_status = cmd_args.status.as_deref().unwrap_or(&states_config.initial);
        let target_phase = cmd_args.phase.as_deref();

        // Build a prompt context for expansion
        let ctx = prompt_system::PromptContext::new(
            target_status,
            target_phase,
            &states_config,
            &phases_config,
        );

        // Get enter prompts: simulate transition from empty to the target
        let prompts_list = prompt_system::get_transition_prompts_with_context(
            "",
            None,
            target_status,
            target_phase,
            workflows,
            &ctx,
        );

        if args.format == CliOutputFormat::Json {
            return Ok(serde_json::to_string_pretty(&json!({
                "status": target_status,
                "phase": target_phase,
                "prompts": prompts_list,
            }))?);
        }

        let mut out = format!("### Prompts for entering `{}`", target_status);
        if let Some(phase) = target_phase {
            out.push_str(&format!(" (phase: `{}`)", phase));
        }
        out.push_str("\n\n");

        if prompts_list.is_empty() {
            out.push_str("_(no prompts configured for this transition)_\n");
        } else {
            for (i, prompt) in prompts_list.iter().enumerate() {
                for line in prompt.lines() {
                    out.push_str(&format!("> {}\n", line));
                }
                if i + 1 < prompts_list.len() {
                    out.push_str("\n---\n\n");
                }
            }
        }

        return Ok(out);
    }

    // List mode (no flags) - list all available prompt triggers
    let triggers = prompt_system::list_available_prompts(workflows);

    if args.format == CliOutputFormat::Json {
        return Ok(serde_json::to_string_pretty(&json!({
            "triggers": triggers,
            "count": triggers.len(),
        }))?);
    }

    let mut out = String::from("### Prompt Triggers\n\n");

    // Group by type
    let mut enter_state: Vec<&str> = Vec::new();
    let mut exit_state: Vec<&str> = Vec::new();
    let mut enter_phase: Vec<&str> = Vec::new();
    let mut exit_phase: Vec<&str> = Vec::new();
    let mut combos: Vec<&str> = Vec::new();

    for t in &triggers {
        if t.contains('~') && t.contains('%') {
            combos.push(t);
        } else if t.starts_with("enter~") {
            enter_state.push(t);
        } else if t.starts_with("exit~") {
            exit_state.push(t);
        } else if t.starts_with("enter%") {
            enter_phase.push(t);
        } else if t.starts_with("exit%") {
            exit_phase.push(t);
        }
    }

    if !enter_state.is_empty() {
        out.push_str("**Enter state:**\n");
        for t in &enter_state {
            out.push_str(&format!("  - `{}`\n", t));
        }
    }
    if !exit_state.is_empty() {
        out.push_str("**Exit state:**\n");
        for t in &exit_state {
            out.push_str(&format!("  - `{}`\n", t));
        }
    }
    if !enter_phase.is_empty() {
        out.push_str("**Enter phase:**\n");
        for t in &enter_phase {
            out.push_str(&format!("  - `{}`\n", t));
        }
    }
    if !exit_phase.is_empty() {
        out.push_str("**Exit phase:**\n");
        for t in &exit_phase {
            out.push_str(&format!("  - `{}`\n", t));
        }
    }
    if !combos.is_empty() {
        out.push_str("**State+phase combos:**\n");
        for t in &combos {
            out.push_str(&format!("  - `{}`\n", t));
        }
    }

    out.push_str(&format!("\n{} triggers total\n", triggers.len()));

    Ok(out)
}

/// Run interactive mode - a REPL for running multiple commands.
fn run_interactive(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd_args: &InteractiveArgs,
) -> ExitCode {
    use std::io::{BufRead, Write};

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    if cmd_args.stdin {
        // Non-interactive: read commands from stdin
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(cmd) => {
                    let cmd = cmd.trim();
                    if cmd.is_empty() || cmd.starts_with('#') {
                        continue;
                    }
                    if let Err(code) = execute_line_command(handler, args, cmd) {
                        return code;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    return ExitCode::from(exit_codes::GENERAL_ERROR);
                }
            }
        }
    } else {
        // Interactive REPL
        println!("task-graph agent interactive mode. Type 'help' for commands, 'exit' to quit.");
        if let Some(ref worker_id) = args.worker_id {
            println!("Worker ID: {}", worker_id);
        }
        println!();

        loop {
            print!("> ");
            let _ = stdout.flush();

            let mut input = String::new();
            match stdin.read_line(&mut input) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let cmd = input.trim();
                    if cmd.is_empty() {
                        continue;
                    }
                    if cmd == "exit" || cmd == "quit" || cmd == "q" {
                        break;
                    }
                    if cmd == "help" || cmd == "?" {
                        print_interactive_help();
                        continue;
                    }
                    if let Err(_) = execute_line_command(handler, args, cmd) {
                        // Continue in interactive mode even on error
                    }
                }
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    break;
                }
            }
        }
    }

    ExitCode::from(exit_codes::SUCCESS)
}

/// Run batch mode - execute commands from a file.
fn run_batch(handler: &ToolHandler, args: &AgentArgs, cmd_args: &BatchArgs) -> ExitCode {
    use std::io::BufRead;

    let file = match std::fs::File::open(&cmd_args.file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error opening file '{}': {}", cmd_args.file.display(), e);
            return ExitCode::from(exit_codes::GENERAL_ERROR);
        }
    };

    let reader = std::io::BufReader::new(file);
    let mut line_num = 0;
    let mut had_errors = false;

    for line in reader.lines() {
        line_num += 1;
        match line {
            Ok(cmd) => {
                let cmd = cmd.trim();
                if cmd.is_empty() || cmd.starts_with('#') {
                    continue;
                }
                eprintln!("[{}] > {}", line_num, cmd);
                if let Err(_) = execute_line_command(handler, args, cmd) {
                    had_errors = true;
                    if !cmd_args.keep_going {
                        return ExitCode::from(exit_codes::GENERAL_ERROR);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading line {}: {}", line_num, e);
                return ExitCode::from(exit_codes::GENERAL_ERROR);
            }
        }
    }

    if had_errors {
        ExitCode::from(exit_codes::GENERAL_ERROR)
    } else {
        ExitCode::from(exit_codes::SUCCESS)
    }
}

/// Execute a single line command in interactive/batch mode.
/// In interactive mode, worker_id comes from the global --worker-id flag.
fn execute_line_command(
    handler: &ToolHandler,
    args: &AgentArgs,
    cmd: &str,
) -> Result<(), ExitCode> {
    // Parse the command line
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    let subcommand = parts[0];
    let subargs = &parts[1..];

    // Helper to get worker_id from global args (required for most commands in interactive mode)
    let require_worker_id = || -> Result<String, ExitCode> {
        args.worker_id.clone().ok_or_else(|| {
            eprintln!(
                "Error: Worker ID required. Use --worker-id flag with 'interactive' command."
            );
            ExitCode::from(exit_codes::INVALID_ARGUMENTS)
        })
    };
    // This is a simplified approach - for full support we'd need to
    // properly handle argument parsing for each command

    let result: Result<String> = match subcommand {
        "ls" | "list-tasks" | "list_tasks" => {
            // Parse list-tasks args
            let mut list_args = ListTasksArgs {
                ready: false,
                blocked: false,
                status: vec![],
                parent: None,
                limit: None,
                offset: None,
            };
            let mut i = 0;
            while i < subargs.len() {
                match subargs[i] {
                    "--ready" => list_args.ready = true,
                    "--blocked" => list_args.blocked = true,
                    "--status" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.status = subargs[i].split(',').map(String::from).collect();
                    }
                    "--parent" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.parent = Some(subargs[i].to_string());
                    }
                    "--limit" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.limit = subargs[i].parse().ok();
                    }
                    "--offset" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.offset = subargs[i].parse().ok();
                    }
                    _ => {}
                }
                i += 1;
            }
            run_list_tasks(handler, args, &list_args)
        }

        "get" => {
            if subargs.is_empty() {
                Err(anyhow::anyhow!("Usage: get <task-id>"))
            } else {
                let get_args = GetArgs {
                    task_id: subargs[0].to_string(),
                };
                run_get(handler, args, &get_args)
            }
        }

        "claim" => {
            if subargs.is_empty() {
                Err(anyhow::anyhow!("Usage: claim <task-id> [--force]"))
            } else {
                let claim_args = ClaimArgs {
                    worker_id: require_worker_id()?,
                    task_id: subargs[0].to_string(),
                    force: subargs.contains(&"--force"),
                };
                run_claim(handler, args, &claim_args)
            }
        }

        "update" => {
            if subargs.is_empty() {
                Err(anyhow::anyhow!(
                    "Usage: update <task-id> [--status STATUS] [--title TITLE] [--description DESC] [--reason REASON]"
                ))
            } else {
                let mut update_args = UpdateArgs {
                    worker_id: require_worker_id()?,
                    task_id: subargs[0].to_string(),
                    status: None,
                    title: None,
                    description: None,
                    reason: None,
                    force: false,
                };
                let mut i = 1;
                while i < subargs.len() {
                    match subargs[i] {
                        "--status" if i + 1 < subargs.len() => {
                            i += 1;
                            update_args.status = Some(subargs[i].to_string());
                        }
                        "--title" if i + 1 < subargs.len() => {
                            i += 1;
                            update_args.title = Some(subargs[i].to_string());
                        }
                        "--description" if i + 1 < subargs.len() => {
                            i += 1;
                            update_args.description = Some(subargs[i].to_string());
                        }
                        "--reason" if i + 1 < subargs.len() => {
                            i += 1;
                            update_args.reason = Some(subargs[i].to_string());
                        }
                        "--force" => update_args.force = true,
                        _ => {}
                    }
                    i += 1;
                }
                run_update(handler, args, &update_args)
            }
        }

        "thinking" => {
            if subargs.is_empty() {
                Err(anyhow::anyhow!(
                    "Usage: thinking <message> [--tasks TASK1,TASK2]"
                ))
            } else {
                let mut tasks = vec![];
                let mut message_parts = vec![];
                let mut i = 0;
                while i < subargs.len() {
                    if subargs[i] == "--tasks" && i + 1 < subargs.len() {
                        i += 1;
                        tasks = subargs[i].split(',').map(String::from).collect();
                    } else {
                        message_parts.push(subargs[i]);
                    }
                    i += 1;
                }
                let thinking_args = ThinkingArgs {
                    worker_id: require_worker_id()?,
                    message: message_parts.join(" "),
                    tasks,
                };
                run_thinking(handler, args, &thinking_args)
            }
        }

        "list-agents" | "list_agents" | "agents" => {
            let mut list_args = ListAgentsArgs {
                tags: vec![],
                file: None,
                task: None,
            };
            let mut i = 0;
            while i < subargs.len() {
                match subargs[i] {
                    "--tags" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.tags = subargs[i].split(',').map(String::from).collect();
                    }
                    "--file" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.file = Some(subargs[i].to_string());
                    }
                    "--task" if i + 1 < subargs.len() => {
                        i += 1;
                        list_args.task = Some(subargs[i].to_string());
                    }
                    _ => {}
                }
                i += 1;
            }
            run_list_agents(handler, args, &list_args)
        }

        "prompts" => {
            let mut prompts_args = PromptsArgs {
                status: None,
                phase: None,
                advisory: None,
                task: None,
            };
            let mut i = 0;
            while i < subargs.len() {
                match subargs[i] {
                    "--status" if i + 1 < subargs.len() => {
                        i += 1;
                        prompts_args.status = Some(subargs[i].to_string());
                    }
                    "--phase" if i + 1 < subargs.len() => {
                        i += 1;
                        prompts_args.phase = Some(subargs[i].to_string());
                    }
                    "--advisory" => {
                        if i + 1 < subargs.len() && !subargs[i + 1].starts_with("--") {
                            i += 1;
                            prompts_args.advisory = Some(subargs[i].to_string());
                        } else {
                            prompts_args.advisory = Some(String::new());
                        }
                    }
                    "--task" if i + 1 < subargs.len() => {
                        i += 1;
                        prompts_args.task = Some(subargs[i].to_string());
                    }
                    _ => {}
                }
                i += 1;
            }
            run_prompts(handler, args, &prompts_args)
        }

        "connect" => {
            // In interactive mode, connect uses the global --worker-id if provided
            let mut connect_args = ConnectArgs {
                worker_id: args.worker_id.clone(),
                tags: vec![],
                workflow: None,
                overlays: vec![],
                force: false,
            };
            let mut i = 0;
            while i < subargs.len() {
                match subargs[i] {
                    "--tags" if i + 1 < subargs.len() => {
                        i += 1;
                        connect_args.tags = subargs[i].split(',').map(String::from).collect();
                    }
                    "--workflow" if i + 1 < subargs.len() => {
                        i += 1;
                        connect_args.workflow = Some(subargs[i].to_string());
                    }
                    "--overlays" if i + 1 < subargs.len() => {
                        i += 1;
                        connect_args.overlays = subargs[i].split(',').map(String::from).collect();
                    }
                    "--force" => connect_args.force = true,
                    _ => {}
                }
                i += 1;
            }
            run_connect(handler, args, &connect_args)
        }

        "disconnect" => {
            let disconnect_args = DisconnectArgs {
                worker_id: require_worker_id()?,
                final_status: None,
            };
            // Parse --final-status if provided
            let mut disconnect_args = disconnect_args;
            let mut i = 0;
            while i < subargs.len() {
                if subargs[i] == "--final-status" && i + 1 < subargs.len() {
                    i += 1;
                    disconnect_args.final_status = Some(subargs[i].to_string());
                }
                i += 1;
            }
            run_disconnect(handler, args, &disconnect_args)
        }

        _ => Err(anyhow::anyhow!(
            "Unknown command: {}. Type 'help' for available commands.",
            subcommand
        )),
    };

    match result {
        Ok(output) => {
            println!("{}", output);
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(ExitCode::from(error_to_exit_code(&e)))
        }
    }
}

/// Print help for interactive mode.
fn print_interactive_help() {
    println!(
        r#"Available commands (worker_id from --worker-id flag):
  ls, list-tasks   Query tasks (--ready, --blocked, --status S, --parent P, --limit N, --offset N)
  get <task-id>    Get task details
  claim <task-id>  Claim a task (--force) [requires --worker-id]
  update <task-id> Update task (--status S, --title T, --reason R, --force) [requires --worker-id]
  thinking <msg>   Broadcast status (--tasks T1,T2) [requires --worker-id]
  prompts          Query prompts (--status S, --phase P, --advisory [TOPIC], --task T)
  agents           List connected workers (--tags T, --file F, --task T)
  connect          Register as worker (--tags T, --workflow W, --overlays O, --force)
  disconnect       Unregister (--final-status S) [requires --worker-id]
  help, ?          Show this help
  exit, quit, q    Exit interactive mode
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Test CLI struct for parsing (wraps AgentArgs in a subcommand)
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Subcommand)]
    enum TestCommand {
        Agent(AgentArgs),
    }

    /// Helper to parse agent args from a command line.
    fn parse_agent(args: &[&str]) -> AgentArgs {
        let mut full_args = vec!["test", "agent"];
        full_args.extend_from_slice(args);
        let cli = TestCli::try_parse_from(full_args).unwrap();
        let TestCommand::Agent(agent_args) = cli.command;
        agent_args
    }

    // ── Arg parsing: connect ──────────────────────────────────────────

    #[test]
    fn test_parse_connect_no_worker_id() {
        let a = parse_agent(&["connect"]);
        let AgentCommand::Connect(c) = a.command else {
            panic!()
        };
        assert_eq!(c.worker_id, None);
        assert!(c.tags.is_empty());
        assert!(!c.force);
    }

    #[test]
    fn test_parse_connect_with_worker_id() {
        let a = parse_agent(&["connect", "my-worker"]);
        let AgentCommand::Connect(c) = a.command else {
            panic!()
        };
        assert_eq!(c.worker_id, Some("my-worker".to_string()));
    }

    #[test]
    fn test_parse_connect_with_tags_workflow_overlays() {
        let a = parse_agent(&[
            "connect",
            "--tags",
            "build,test",
            "--workflow",
            "swarm",
            "--overlays",
            "reasoning,patch",
            "--force",
        ]);
        let AgentCommand::Connect(c) = a.command else {
            panic!()
        };
        assert_eq!(c.tags, vec!["build", "test"]);
        assert_eq!(c.workflow, Some("swarm".to_string()));
        assert_eq!(c.overlays, vec!["reasoning", "patch"]);
        assert!(c.force);
    }

    // ── Arg parsing: disconnect ───────────────────────────────────────

    #[test]
    fn test_parse_disconnect() {
        let a = parse_agent(&["disconnect", "worker-1"]);
        let AgentCommand::Disconnect(d) = a.command else {
            panic!()
        };
        assert_eq!(d.worker_id, "worker-1");
        assert_eq!(d.final_status, None);
    }

    #[test]
    fn test_parse_disconnect_with_final_status() {
        let a = parse_agent(&["disconnect", "w1", "--final-status", "pending"]);
        let AgentCommand::Disconnect(d) = a.command else {
            panic!()
        };
        assert_eq!(d.worker_id, "w1");
        assert_eq!(d.final_status, Some("pending".to_string()));
    }

    #[test]
    fn test_parse_disconnect_missing_worker_id() {
        let full = vec!["test", "agent", "disconnect"];
        let result = TestCli::try_parse_from(full);
        assert!(result.is_err(), "disconnect without worker_id should fail");
    }

    // ── Arg parsing: list-tasks ───────────────────────────────────────

    #[test]
    fn test_parse_list_tasks_alias_ls() {
        let a = parse_agent(&["ls", "--ready"]);
        let AgentCommand::ListTasks(l) = a.command else {
            panic!()
        };
        assert!(l.ready);
        assert!(!l.blocked);
    }

    #[test]
    fn test_parse_list_tasks_full_filters() {
        let a = parse_agent(&[
            "list-tasks",
            "--ready",
            "--blocked",
            "--status",
            "open,in_progress",
            "--parent",
            "root-1",
            "--limit",
            "10",
            "--offset",
            "5",
        ]);
        let AgentCommand::ListTasks(l) = a.command else {
            panic!()
        };
        assert!(l.ready);
        assert!(l.blocked);
        assert_eq!(l.status, vec!["open", "in_progress"]);
        assert_eq!(l.parent, Some("root-1".to_string()));
        assert_eq!(l.limit, Some(10));
        assert_eq!(l.offset, Some(5));
    }

    #[test]
    fn test_parse_list_tasks_defaults() {
        let a = parse_agent(&["list-tasks"]);
        let AgentCommand::ListTasks(l) = a.command else {
            panic!()
        };
        assert!(!l.ready);
        assert!(!l.blocked);
        assert!(l.status.is_empty());
        assert_eq!(l.parent, None);
        assert_eq!(l.limit, None);
        assert_eq!(l.offset, None);
    }

    // ── Arg parsing: get ──────────────────────────────────────────────

    #[test]
    fn test_parse_get() {
        let a = parse_agent(&["get", "task-abc"]);
        let AgentCommand::Get(g) = a.command else {
            panic!()
        };
        assert_eq!(g.task_id, "task-abc");
    }

    #[test]
    fn test_parse_get_missing_task_id() {
        let result = TestCli::try_parse_from(["test", "agent", "get"]);
        assert!(result.is_err(), "get without task_id should fail");
    }

    // ── Arg parsing: claim ────────────────────────────────────────────

    #[test]
    fn test_parse_claim_basic() {
        let a = parse_agent(&["claim", "worker-1", "task-123"]);
        let AgentCommand::Claim(c) = a.command else {
            panic!()
        };
        assert_eq!(c.worker_id, "worker-1");
        assert_eq!(c.task_id, "task-123");
        assert!(!c.force);
    }

    #[test]
    fn test_parse_claim_force() {
        let a = parse_agent(&["claim", "w1", "t1", "--force"]);
        let AgentCommand::Claim(c) = a.command else {
            panic!()
        };
        assert!(c.force);
    }

    #[test]
    fn test_parse_claim_missing_args() {
        // Missing both worker_id and task_id
        let result = TestCli::try_parse_from(["test", "agent", "claim"]);
        assert!(result.is_err());

        // Missing task_id
        let result = TestCli::try_parse_from(["test", "agent", "claim", "w1"]);
        assert!(result.is_err());
    }

    // ── Arg parsing: update ───────────────────────────────────────────

    #[test]
    fn test_parse_update_all_fields() {
        let a = parse_agent(&[
            "update",
            "w1",
            "task-1",
            "--status",
            "completed",
            "--title",
            "New title",
            "--description",
            "New desc",
            "--reason",
            "Done",
            "--force",
        ]);
        let AgentCommand::Update(u) = a.command else {
            panic!()
        };
        assert_eq!(u.worker_id, "w1");
        assert_eq!(u.task_id, "task-1");
        assert_eq!(u.status, Some("completed".to_string()));
        assert_eq!(u.title, Some("New title".to_string()));
        assert_eq!(u.description, Some("New desc".to_string()));
        assert_eq!(u.reason, Some("Done".to_string()));
        assert!(u.force);
    }

    #[test]
    fn test_parse_update_minimal() {
        let a = parse_agent(&["update", "w1", "task-1"]);
        let AgentCommand::Update(u) = a.command else {
            panic!()
        };
        assert_eq!(u.worker_id, "w1");
        assert_eq!(u.task_id, "task-1");
        assert_eq!(u.status, None);
        assert_eq!(u.title, None);
        assert!(!u.force);
    }

    // ── Arg parsing: thinking ─────────────────────────────────────────

    #[test]
    fn test_parse_thinking() {
        let a = parse_agent(&["thinking", "w1", "Analyzing code"]);
        let AgentCommand::Thinking(t) = a.command else {
            panic!()
        };
        assert_eq!(t.worker_id, "w1");
        assert_eq!(t.message, "Analyzing code");
        assert!(t.tasks.is_empty());
    }

    #[test]
    fn test_parse_thinking_with_tasks() {
        let a = parse_agent(&["thinking", "w1", "Working", "--tasks", "t1,t2,t3"]);
        let AgentCommand::Thinking(t) = a.command else {
            panic!()
        };
        assert_eq!(t.tasks, vec!["t1", "t2", "t3"]);
    }

    // ── Arg parsing: attach ───────────────────────────────────────────

    #[test]
    fn test_parse_attach_with_content() {
        let a = parse_agent(&["attach", "w1", "task-1", "-t", "note", "-c", "My note"]);
        let AgentCommand::Attach(att) = a.command else {
            panic!()
        };
        assert_eq!(att.worker_id, "w1");
        assert_eq!(att.task_id, "task-1");
        assert_eq!(att.r#type, "note");
        assert_eq!(att.content, Some("My note".to_string()));
        assert_eq!(att.file, None);
    }

    #[test]
    fn test_parse_attach_with_file() {
        let a = parse_agent(&["attach", "w1", "task-1", "-t", "log", "--file", "out.log"]);
        let AgentCommand::Attach(att) = a.command else {
            panic!()
        };
        assert_eq!(att.file, Some(PathBuf::from("out.log")));
        assert_eq!(att.content, None);
    }

    #[test]
    fn test_parse_attach_content_and_file_conflict() {
        let result = TestCli::try_parse_from([
            "test", "agent", "attach", "w1", "t1", "-t", "note", "-c", "text", "--file", "f.txt",
        ]);
        assert!(result.is_err(), "--content and --file should conflict");
    }

    // ── Arg parsing: list-agents ──────────────────────────────────────

    #[test]
    fn test_parse_list_agents_defaults() {
        let a = parse_agent(&["list-agents"]);
        let AgentCommand::ListAgents(la) = a.command else {
            panic!()
        };
        assert!(la.tags.is_empty());
        assert_eq!(la.file, None);
        assert_eq!(la.task, None);
    }

    #[test]
    fn test_parse_list_agents_with_filters() {
        let a = parse_agent(&["list-agents", "--tags", "build", "--task", "t1"]);
        let AgentCommand::ListAgents(la) = a.command else {
            panic!()
        };
        assert_eq!(la.tags, vec!["build"]);
        assert_eq!(la.task, Some("t1".to_string()));
    }

    // ── Arg parsing: interactive / batch ──────────────────────────────

    #[test]
    fn test_parse_interactive() {
        let a = parse_agent(&["interactive"]);
        let AgentCommand::Interactive(i) = a.command else {
            panic!()
        };
        assert!(!i.stdin);
    }

    #[test]
    fn test_parse_interactive_stdin() {
        let a = parse_agent(&["interactive", "--stdin"]);
        let AgentCommand::Interactive(i) = a.command else {
            panic!()
        };
        assert!(i.stdin);
    }

    #[test]
    fn test_parse_repl_alias() {
        let a = parse_agent(&["repl"]);
        assert!(matches!(a.command, AgentCommand::Interactive(_)));
    }

    #[test]
    fn test_parse_batch() {
        let a = parse_agent(&["batch", "commands.txt"]);
        let AgentCommand::Batch(b) = a.command else {
            panic!()
        };
        assert_eq!(b.file, PathBuf::from("commands.txt"));
        assert!(!b.keep_going);
    }

    #[test]
    fn test_parse_batch_keep_going() {
        let a = parse_agent(&["batch", "-k", "cmds.txt"]);
        let AgentCommand::Batch(b) = a.command else {
            panic!()
        };
        assert!(b.keep_going);
    }

    // ── Global flags ──────────────────────────────────────────────────

    #[test]
    fn test_parse_format_json() {
        let a = parse_agent(&["--format", "json", "list-tasks"]);
        assert_eq!(a.format, CliOutputFormat::Json);
    }

    #[test]
    fn test_parse_format_default_is_markdown() {
        let a = parse_agent(&["list-tasks"]);
        assert_eq!(a.format, CliOutputFormat::Markdown);
    }

    #[test]
    fn test_parse_global_worker_id() {
        let a = parse_agent(&["--worker-id", "global-w", "list-tasks"]);
        assert_eq!(a.worker_id, Some("global-w".to_string()));
    }

    #[test]
    fn test_parse_no_global_worker_id() {
        let a = parse_agent(&["list-tasks"]);
        assert_eq!(a.worker_id, None);
    }

    // ── CliOutputFormat conversion ────────────────────────────────────

    #[test]
    fn test_cli_format_to_output_format() {
        assert!(matches!(
            OutputFormat::from(CliOutputFormat::Markdown),
            OutputFormat::Markdown
        ));
        assert!(matches!(
            OutputFormat::from(CliOutputFormat::Json),
            OutputFormat::Json
        ));
    }

    // ── Exit code mapping ─────────────────────────────────────────────
    // error_to_exit_code does string matching on the error message

    #[test]
    fn test_error_to_exit_code_task_not_found() {
        let err = anyhow::anyhow!(crate::error::ToolError::task_not_found("abc"));
        assert_eq!(error_to_exit_code(&err), exit_codes::TASK_NOT_FOUND);
    }

    #[test]
    fn test_error_to_exit_code_agent_not_found() {
        let err = anyhow::anyhow!(crate::error::ToolError::agent_not_found("w1"));
        assert_eq!(error_to_exit_code(&err), exit_codes::WORKER_NOT_FOUND);
    }

    #[test]
    fn test_error_to_exit_code_already_claimed() {
        let err = anyhow::anyhow!(crate::error::ToolError::already_claimed("t1", "w2"));
        assert_eq!(error_to_exit_code(&err), exit_codes::CLAIM_FAILED);
    }

    #[test]
    fn test_error_to_exit_code_not_owner() {
        let err = anyhow::anyhow!(crate::error::ToolError::not_owner("t1", "w1"));
        assert_eq!(error_to_exit_code(&err), exit_codes::PERMISSION_DENIED);
    }

    #[test]
    fn test_error_to_exit_code_missing_field() {
        let err = anyhow::anyhow!(crate::error::ToolError::missing_field("worker_id"));
        assert_eq!(error_to_exit_code(&err), exit_codes::INVALID_ARGUMENTS);
    }

    #[test]
    fn test_error_to_exit_code_invalid_value() {
        let err = anyhow::anyhow!(crate::error::ToolError::invalid_value("status", "bad"));
        assert_eq!(error_to_exit_code(&err), exit_codes::INVALID_ARGUMENTS);
    }

    #[test]
    fn test_error_to_exit_code_generic() {
        let err = anyhow::anyhow!("some random error");
        assert_eq!(error_to_exit_code(&err), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn test_error_to_exit_code_dependency_blocked() {
        let err = anyhow::anyhow!(crate::error::ToolError::deps_not_satisfied(&[
            "dep-1".to_string(),
        ]));
        assert_eq!(error_to_exit_code(&err), exit_codes::CLAIM_FAILED);
    }

    // ── Format helpers ────────────────────────────────────────────────

    #[test]
    fn test_format_json_output_json_mode() {
        let v = serde_json::json!({"status": "ok"});
        let out = format_json_output(v.clone(), CliOutputFormat::Json);
        assert_eq!(out, serde_json::to_string_pretty(&v).unwrap());
    }

    #[test]
    fn test_format_json_output_markdown_mode() {
        let v = serde_json::json!({"status": "ok"});
        let out = format_json_output(v.clone(), CliOutputFormat::Markdown);
        // In markdown mode, JSON values are still pretty-printed
        assert!(out.contains("status"));
    }

    #[test]
    fn test_format_output_raw_markdown() {
        let result = ToolResult::Raw("# Tasks\n- task-1".to_string());
        let out = format_output(result, CliOutputFormat::Markdown);
        assert_eq!(out, "# Tasks\n- task-1");
    }

    #[test]
    fn test_format_output_raw_json() {
        let result = ToolResult::Raw("hello".to_string());
        let out = format_output(result, CliOutputFormat::Json);
        assert!(out.contains("\"output\""));
        assert!(out.contains("hello"));
    }

    // ── Arg parsing: prompts ────────────────────────────────────────────

    #[test]
    fn test_parse_prompts_no_args() {
        let a = parse_agent(&["prompts"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.status, None);
        assert_eq!(p.phase, None);
        assert_eq!(p.advisory, None);
        assert_eq!(p.task, None);
    }

    #[test]
    fn test_parse_prompts_with_status() {
        let a = parse_agent(&["prompts", "--status", "working"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.status, Some("working".to_string()));
    }

    #[test]
    fn test_parse_prompts_with_phase() {
        let a = parse_agent(&["prompts", "--phase", "implement"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.phase, Some("implement".to_string()));
    }

    #[test]
    fn test_parse_prompts_advisory_list() {
        let a = parse_agent(&["prompts", "--advisory"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.advisory, Some(String::new()));
    }

    #[test]
    fn test_parse_prompts_advisory_specific() {
        let a = parse_agent(&["prompts", "--advisory", "decompose-epic"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.advisory, Some("decompose-epic".to_string()));
    }

    #[test]
    fn test_parse_prompts_with_task() {
        let a = parse_agent(&["prompts", "--status", "working", "--task", "task-123"]);
        let AgentCommand::Prompts(p) = a.command else {
            panic!()
        };
        assert_eq!(p.status, Some("working".to_string()));
        assert_eq!(p.task, Some("task-123".to_string()));
    }

    // ── Format: connect output ──────────────────────────────────────────

    #[test]
    fn test_format_connect_output_json_mode() {
        let v = json!({
            "worker_id": "w1",
            "tags": ["build"],
            "config": { "states": ["pending", "working"], "initial_state": "pending" }
        });
        let out = format_connect_output(v.clone(), CliOutputFormat::Json);
        assert_eq!(out, serde_json::to_string_pretty(&v).unwrap());
    }

    #[test]
    fn test_format_connect_output_markdown_mode() {
        let v = json!({
            "worker_id": "test-worker",
            "tags": ["build", "test"],
            "config": {
                "states": ["pending", "working", "completed"],
                "initial_state": "pending",
                "timed_states": ["working"],
                "terminal_states": ["completed"],
                "phases": ["implement", "test"]
            },
            "role": { "role": "worker", "description": "A worker role" },
            "role_prompts": ["You are actively working."],
            "paths": {
                "db_path": "tasks.db",
                "media_dir": "media",
                "log_dir": "logs"
            }
        });
        let out = format_connect_output(v, CliOutputFormat::Markdown);
        assert!(out.contains("**Worker ID:** `test-worker`"));
        assert!(out.contains("**Tags:** build, test"));
        assert!(out.contains("**Role:** `worker`"));
        assert!(out.contains("### State Machine"));
        assert!(out.contains("**Initial:** `pending`"));
        assert!(out.contains("### Role Prompts"));
        assert!(out.contains("> You are actively working."));
        assert!(out.contains("### Paths"));
    }

    // ── Format: update output ───────────────────────────────────────────

    #[test]
    fn test_format_update_output_json_mode() {
        let v = json!({ "task": "t1", "status": "working" });
        let out = format_update_output(v.clone(), CliOutputFormat::Json);
        assert_eq!(out, serde_json::to_string_pretty(&v).unwrap());
    }

    #[test]
    fn test_format_update_output_markdown_with_prompts() {
        let v = json!({
            "task": "fix-bug",
            "title": "Fix auth bug",
            "status": "working",
            "prompts": [
                "You are now actively working on this task.",
                "Remember to run tests before completing."
            ]
        });
        let out = format_update_output(v, CliOutputFormat::Markdown);
        assert!(out.contains("**Task:** `fix-bug` - Fix auth bug"));
        assert!(out.contains("**Status:** `working`"));
        assert!(out.contains("### Guidance"));
        assert!(out.contains("> You are now actively working on this task."));
        assert!(out.contains("> Remember to run tests before completing."));
    }

    #[test]
    fn test_format_update_output_markdown_no_prompts() {
        let v = json!({
            "task": "t1",
            "title": "Some task",
            "status": "pending"
        });
        let out = format_update_output(v, CliOutputFormat::Markdown);
        assert!(out.contains("**Task:** `t1`"));
        assert!(!out.contains("### Guidance"));
    }

    // ── Format: claim output ────────────────────────────────────────────

    #[test]
    fn test_format_claim_output_markdown_with_prompts() {
        let v = json!({
            "task": "task-1",
            "title": "Implement feature",
            "status": "working",
            "owner": "worker-5",
            "prompts": ["Start by reading the existing code."]
        });
        let out = format_claim_output(v, CliOutputFormat::Markdown);
        assert!(out.contains("**Task:** `task-1` - Implement feature"));
        assert!(out.contains("**Owner:** `worker-5`"));
        assert!(out.contains("### Guidance"));
        assert!(out.contains("> Start by reading the existing code."));
    }
}
