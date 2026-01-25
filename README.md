# Task Graph MCP Server

A Rust MCP (Model Context Protocol) server providing atomic, token-efficient task management for multi-agent coordination.

## Features

- **Task Hierarchy**: Unlimited nesting with parent/child relationships
- **Dependencies**: DAG-based dependencies with cycle detection
- **Task Claiming**: Strict locking with configurable limits and tag-based affinity
- **File Coordination**: Advisory locks with claim tracking for coordinating file edits
- **Cost Tracking**: Token usage and cost accounting per task
- **Time Tracking**: Automatic time accumulation based on state transitions, plus manual logging
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

Create `.task-graph/config.yaml`:

```yaml
server:
  db_path: .task-graph/tasks.db
  media_dir: .task-graph/media  # Directory for file attachments
  skills_dir: .task-graph/skills  # Custom skill overrides
  claim_limit: 5
  stale_timeout_seconds: 900
  default_format: json  # or markdown

paths:
  style: relative  # or project_prefixed

auto_advance:
  enabled: false        # Auto-transition unblocked tasks
  target_state: ready   # Target state (requires custom state in states config)
```

### States Configuration

Task states are configurable. Default states: `pending`, `in_progress`, `completed`, `failed`, `cancelled`.

To add a `ready` state for auto-advance:

```yaml
states:
  initial: pending
  disconnect_state: pending  # State for tasks when owner disconnects (must be untimed)
  blocking_states: [pending, in_progress]
  definitions:
    pending:
      exits: [ready, in_progress, cancelled]
    ready:
      exits: [in_progress, cancelled]
    in_progress:
      exits: [completed, failed, pending]
      timed: true    # Time in this state counts toward time_actual_ms
    completed:
      exits: []
    failed:
      exits: [pending]
    cancelled:
      exits: []

auto_advance:
  enabled: true
  target_state: ready
```

See [SCHEMA.md](SCHEMA.md#states-configuration) for full documentation on state definitions.

### Dependencies Configuration

Dependency types define how tasks relate to each other. Default types: `blocks`, `follows`, `contains`, `duplicate`, `see-also`.

```yaml
dependencies:
  definitions:
    blocks:
      display: horizontal  # Same-level relationship
      blocks: start        # Blocks claiming the dependent task
    follows:
      display: horizontal
      blocks: start
    contains:
      display: vertical    # Parent-child relationship
      blocks: completion   # Blocks completing the parent
    duplicate:
      display: horizontal
      blocks: none         # Informational only
    see-also:
      display: horizontal
      blocks: none
```

| Property | Values | Description |
|----------|--------|-------------|
| `display` | `horizontal`, `vertical` | Visual relationship (same-level vs parent-child) |
| `blocks` | `none`, `start`, `completion` | What the dependency blocks |

Environment variables:
- `TASK_GRAPH_CONFIG_PATH`: Path to configuration file (takes precedence over `.task-graph/config.yaml`)
- `TASK_GRAPH_DB_PATH`: Database file path (fallback if no config file)
- `TASK_GRAPH_MEDIA_DIR`: Media directory for file attachments (fallback if no config file)

## MCP Tools

### Agent Management

| Tool | Arguments | Description |
|------|-----------|-------------|
| `connect` | `agent?`, `name?`, `tags?`, `max_claims?` | Register a new agent session. Returns `agent_id`. |
| `disconnect` | `agent` | Unregister agent and release all claims/locks. |
| `list_agents` | `format?` | List all connected agents. |

### Task CRUD

| Tool | Arguments | Description |
|------|-----------|-------------|
| `create` | `title`, `description?`, `parent?`, `priority?`, `points?`, `time_estimate_ms?`, `agent_tags_all?`, `agent_tags_any?`, `blocked_by?` | Create a new task. |
| `create_tree` | `tree`, `parent?` | Create a nested task tree with `then`/`also` join modes. |
| `get` | `task`, `children?`, `format?` | Get a task by ID with optional descendants. |
| `list_tasks` | `status?`, `ready?`, `blocked?`, `owner?`, `parent?`, `agent?`, `limit?`, `format?` | Query tasks with filters. |
| `search` | `query`, `limit?`, `include_attachments?`, `status_filter?` | Full-text search across tasks and attachments with FTS5 ranking. |
| `update` | `agent`, `task`, `state`, `title?`, `description?`, `priority?`, `points?` | Update task properties. |
| `delete` | `task`, `cascade?` | Delete a task. Use `cascade=true` to delete children. |

### Task Claiming

| Tool | Arguments | Description |
|------|-----------|-------------|
| `claim` | `agent`, `task`, `state?`, `force?` | Claim a task. Use `force=true` to steal from another agent. |
| `release` | `agent`, `task`, `state?` | Release a task. Use `state` to set status (default: pending). |
| `complete` | `agent`, `task` | Shorthand for release with `state=completed`. |

### Dependencies

| Tool | Arguments | Description |
|------|-----------|-------------|
| `block` | `blocker`, `blocked` | Add dependency: `blocker` must complete before `blocked` can be claimed. |
| `unblock` | `blocker`, `blocked` | Remove a dependency. |

### Tracking

| Tool | Arguments | Description |
|------|-----------|-------------|
| `thinking` | `agent`, `thought`, `tasks?` | Update current activity (visible to other agents). Refreshes heartbeat. |
| `log_time` | `agent`, `task`, `duration_ms` | Manually log time spent on a task (in addition to automatic tracking). |
| `log_cost` | `agent`, `task`, `tokens_in?`, `tokens_cached?`, `tokens_out?`, `tokens_thinking?`, `tokens_image?`, `tokens_audio?`, `cost_usd?`, `user_metrics?` | Log token usage and cost. |
| `get_state_history` | `task` | Get state transition history and current duration in state. |

**Note:** Time spent in working states (like `in_progress`) is automatically added to `time_actual_ms` when transitioning to another state. The `log_time` tool can be used for additional manual adjustments.

### File Coordination

| Tool | Arguments | Description |
|------|-----------|-------------|
| `claim_file` | `agent`, `file`, `reason?` | Claim advisory lock on a file. |
| `release_file` | `agent`, `file`, `reason?` | Release file lock. Use `reason` to leave notes. |
| `list_files` | `files?`, `agent?` | Get current file locks. |
| `claim_updates` | `agent`, `files?`, `timeout?` | Poll for file claim changes. Use `timeout` (ms) for long-polling. |

### Attachments

| Tool | Arguments | Description |
|------|-----------|-------------|
| `attach` | `task`, `name`, `content?`, `mime?`, `file?`, `store_as_file?` | Add an attachment. |
| `attachments` | `task`, `content?` | Get attachment metadata. Use `content=true` for full content. |
| `detach` | `task`, `index` | Delete an attachment by task and index. |

Attachment modes:
- **Inline**: Content stored in database (`content` parameter)
- **File reference**: Reference an existing file (`file` parameter)
- **Media storage**: Store to `.task-graph/media/` (`content` + `store_as_file=true`)

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

Tasks can specify required capabilities with a non-empty list. Use either for one tag:

- `agent_tags_all`: Agent must have ALL of these (AND)
- `agent_tags_any`: Agent must have AT LEAST ONE (OR)

```json
{
  "title": "Deploy to production",
  "agent_tags_all": ["deploy", "prod-access"],
  "agent_tags_any": ["aws", "gcp"]
}
```

## File Coordination

Agents can coordinate file edits using advisory locks with change tracking:

```
Agent A: connect() -> "agent-a"
Agent A: claim_file("agent-a", "src/main.rs", "refactoring")
Agent B: connect() -> "agent-b"
Agent B: claim_updates("agent-b", ["src/main.rs"]) -> sees agent-a's claim
Agent A: release_file("agent-a", "src/main.rs", "ready for review")
Agent B: claim_updates("agent-b") -> sees release with reason
Agent B: claim_file("agent-b", "src/main.rs", "adding tests")
```

## Architecture

- **Transport**: Stdio (each agent spawns own server process)
- **Database**: SQLite with WAL mode for concurrent access
- **Concurrency**: All processes share `.task-graph/tasks.db`

## License

Apache 2.0
