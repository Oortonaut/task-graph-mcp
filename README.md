# Task Graph MCP Server

A Rust MCP (Model Context Protocol) server providing atomic, token-efficient task management for multi-agent coordination.

## Features

- **Task Hierarchy**: Unlimited nesting with parent/child relationships
- **Dependencies**: DAG-based dependencies with cycle detection
- **Task Claiming**: Strict locking with configurable limits and tag-based affinity
- **File Locking**: Advisory locks for coordinating file edits
- **Pub/Sub**: Event subscriptions with polling-based inbox
- **Cost Tracking**: Token usage and cost accounting per task
- **Time Tracking**: Estimation and actual time logging
- **Live Status**: Real-time "current thought" for claimed tasks

## Installation

```bash
cargo build --release
```

## Usage

### As an MCP Server

Add to your MCP client configuration:

```json
{
  "mcpServers": {
    "task-graph": {
      "command": "task-graph-mcp",
      "args": []
    }
  }
}
```

### CLI Options

```
task-graph-mcp [OPTIONS]

Options:
  -c, --config <FILE>     Path to configuration file
  -d, --database <FILE>   Path to database file (overrides config)
  -v, --verbose           Enable verbose logging
  -h, --help              Print help
  -V, --version           Print version
```

## Configuration

Create `.task-graph/config.toml`:

```toml
[server]
db_path = ".task-graph/tasks.db"
default_claim_limit = 5
default_stale_timeout_seconds = 900

[paths]
style = "relative"  # or "project_prefixed"
```

Environment variables:
- `TASK_GRAPH_DB_PATH`: Database file path
- `TASK_GRAPH_CLAIM_LIMIT`: Default claim limit
- `TASK_GRAPH_STALE_TIMEOUT`: Stale timeout in seconds

## MCP Tools

### Agent Management

| Tool | Description |
|------|-------------|
| `register_agent` | Register a new agent session |
| `update_agent` | Update agent properties |
| `heartbeat` | Refresh agent heartbeat |
| `unregister_agent` | Unregister and release all claims |

### Task CRUD

| Tool | Description |
|------|-------------|
| `create_task` | Create a new task |
| `create_task_tree` | Create a nested task tree |
| `get_task` | Get a task by ID |
| `update_task` | Update task properties |
| `delete_task` | Delete a task |
| `list_tasks` | List tasks with filters |

### Task Claiming

| Tool | Description |
|------|-------------|
| `claim_task` | Claim a task for an agent |
| `release_task` | Release a task claim |
| `force_release` | Force release regardless of owner |
| `force_release_stale` | Release stale claims |

### Dependencies

| Tool | Description |
|------|-------------|
| `add_dependency` | Add a dependency (from blocks to) |
| `remove_dependency` | Remove a dependency |
| `get_blocked_tasks` | Get blocked tasks |
| `get_ready_tasks` | Get tasks ready to claim |

### Tracking

| Tool | Description |
|------|-------------|
| `set_thought` | Set current thought for claimed tasks |
| `log_time` | Log time spent on a task |
| `log_cost` | Log token usage and cost |

### File Locking

| Tool | Description |
|------|-------------|
| `lock_file` | Declare intent to work on a file |
| `unlock_file` | Release a file lock |
| `get_file_locks` | Get current file locks |

### Attachments

| Tool | Description |
|------|-------------|
| `add_attachment` | Add an attachment to a task |
| `get_attachments` | Get attachment metadata |
| `get_attachment` | Get full attachment with content |
| `delete_attachment` | Delete an attachment |

### Pub/Sub

| Tool | Description |
|------|-------------|
| `subscribe` | Subscribe to events |
| `unsubscribe` | Unsubscribe from events |
| `poll_inbox` | Poll for new messages |
| `clear_inbox` | Clear inbox |

## MCP Resources

| URI | Description |
|-----|-------------|
| `tasks://all` | Full task graph with dependencies |
| `tasks://ready` | Tasks ready to claim |
| `tasks://blocked` | Tasks blocked by dependencies |
| `tasks://claimed` | All claimed tasks |
| `tasks://agent/{id}` | Tasks owned by an agent |
| `tasks://tree/{id}` | Task with all descendants |
| `files://locks` | All file locks |
| `inbox://{agent_id}` | Unread messages |
| `agents://all` | Registered agents |
| `plan://acp` | ACP-compatible plan export |
| `stats://summary` | Aggregate statistics |

## Task Tree Structure

Create hierarchical tasks with `then`/`also` join modes:

```json
{
  "title": "Implement auth",
  "children": [
    { "title": "Design schema", "join_mode": "then" },
    { "title": "Write migrations", "join_mode": "then" },
    { "title": "Implement endpoints", "join_mode": "then", "children": [
      { "title": "Login endpoint", "join_mode": "then" },
      { "title": "Logout endpoint", "join_mode": "also" },
      { "title": "Refresh endpoint", "join_mode": "also" }
    ]},
    { "title": "Write tests", "join_mode": "then" }
  ]
}
```

- `then`: Depends on previous sibling completing
- `also`: Runs in parallel with previous sibling

## Tag-Based Affinity

Tasks can specify required capabilities:

- `needed_tags`: Agent must have ALL of these (AND)
- `wanted_tags`: Agent must have AT LEAST ONE (OR)

```json
{
  "title": "Deploy to production",
  "needed_tags": ["deploy", "prod-access"],
  "wanted_tags": ["aws", "gcp"]
}
```

## Architecture

- **Transport**: Stdio (each agent spawns own server process)
- **Database**: SQLite with WAL mode for concurrent access
- **Concurrency**: All processes share `.task-graph/tasks.db`

## License

MIT
