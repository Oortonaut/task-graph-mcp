# Task Graph MCP Server

**Multi-agent AI coordination that actually works.**

When you have multiple AI agents working on the same codebase, things go wrong fast. Agents overwrite each other's changes. They duplicate work. They break dependencies. Task Graph solves this with proper coordination primitives: DAG-based task dependencies, advisory file locks, and atomic claiming—all through the Model Context Protocol.

## Why Task Graph?

**The problem**: You've got a complex task that needs multiple AI agents working in parallel. Maybe a coordinator breaking down work, specialists handling different domains, validators checking results. Without coordination, it's chaos.

**What you get**:

- **No stepping on toes** — Advisory file locks let agents see who's editing what and why. No more blind overwrites or merge conflicts.
- **Dependency-aware execution** — Tasks form a DAG with cycle detection. Agents only claim work when dependencies are satisfied.
- **Token-efficient** — Designed for LLM context limits. Compact queries, minimal round-trips, structured outputs.
- **Built-in accounting** — Track tokens, cost, and time per task. Know exactly what your agents are spending.
- **Zero infrastructure** — SQLite with WAL mode. No database server to run. Just point at a file.
- **Configurable workflows** — Define your own states, transitions, and dependency types. Match your process, not ours.

## Features

| Feature | Description |
|---------|-------------|
| **Task Hierarchy** | Unlimited nesting with parent/child relationships |
| **DAG Dependencies** | Typed edges (blocks, follows, contains) with cycle detection |
| **Atomic Claiming** | Strict locking with limits and tag-based routing |
| **File Coordination** | Advisory locks with reasons and change polling |
| **Cost Tracking** | Token usage and USD cost per task |
| **Time Tracking** | Automatic accumulation from state transitions |
| **Live Status** | Real-time "current thought" visible to other agents |
| **Full-text Search** | FTS5-powered search across tasks and attachments |
| **Attachments** | Inline content, file references, or media storage |

## Quick Start

```bash
# Build
cargo build --release

# Add to your MCP client (Claude Code, etc.)
```

```json
{
  "mcpServers": {
    "task-graph": {
      "command": "task-graph-mcp"
    }
  }
}
```

```
# Worker workflow
connect(worker_id="worker-1", tags=["rust","backend"])  → worker_id
list_tasks(ready=true, worker_id="worker-1")            → claimable work
claim(worker_id="worker-1", task="task-123")            → you own it
thinking(worker_id="worker-1", thought="Implementing...") → visible to others
update(worker_id="worker-1", task="task-123",           → done, deps unblock
       status="completed",
       attachments=[{name:"commit", content:"abc123"}])
```

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

### Attachments Configuration

Preconfigured attachment keys provide default MIME types and modes, reducing boilerplate when attaching common content types.

```yaml
attachments:
  unknown_key: warn  # allow | warn (default) | reject
  definitions:
    commit:
      mime: text/git.hash
      mode: append
    checkin:
      mime: text/p4.changelist
      mode: append
    meta:
      mime: application/json
      mode: replace
    note:
      mime: text/plain
      mode: append
```

| Property | Values | Description |
|----------|--------|-------------|
| `unknown_key` | `allow`, `warn`, `reject` | Behavior for undefined attachment keys |
| `definitions.<key>.mime` | MIME type string | Default MIME type for this key |
| `definitions.<key>.mode` | `append`, `replace` | Default mode (append keeps existing, replace overwrites) |

**Built-in defaults**:

| Key | MIME Type | Mode | Use Case |
|-----|-----------|------|----------|
| `commit` | text/git.hash | append | Git commit hashes |
| `checkin` | text/p4.changelist | append | Perforce changelists |
| `changelist` | text/plain | append | Files changed |
| `meta` | application/json | replace | Structured metadata |
| `note` | text/plain | append | General notes |
| `log` | text/plain | append | Log output |
| `error` | text/plain | append | Error messages |
| `output` | text/plain | append | Command/tool output |
| `diff` | text/x-diff | append | Patches and diffs |
| `plan` | text/markdown | replace | Plans and specs |
| `result` | application/json | replace | Structured results |
| `context` | text/plain | replace | Current context/state |

**Usage**:
```
# MIME and mode auto-filled from config:
attach(task="123", name="commit", content="abc1234")
# → mime=text/git.hash, mode=append

attach(task="123", name="meta", content='{"v":1}')
# → mime=application/json, mode=replace (overwrites existing meta)

# Explicit values override defaults:
attach(task="123", name="commit", mime="text/plain", content="override")
```

Environment variables:
- `TASK_GRAPH_CONFIG_PATH`: Path to configuration file (takes precedence over `.task-graph/config.yaml`)
- `TASK_GRAPH_DB_PATH`: Database file path (fallback if no config file)
- `TASK_GRAPH_MEDIA_DIR`: Media directory for file attachments (fallback if no config file)

## MCP Tools

### Worker Management

| Tool | Description |
|------|-------------|
| `connect(worker_id?: worker_str, tags?: str[], force?: bool = false)` | Register a worker session. Returns `worker_id`. Use `force` to reconnect a stuck worker. |
| `disconnect(worker_id: worker_str, final_status?: status_str = "pending")` | Unregister worker and release all claims/locks. |
| `list_workers(tags?: str[], file?: filename, task?: task_str, depth?: int)` | List connected workers with filters. |

### Task CRUD

| Tool | Description |
|------|-------------|
| `create(description: str, id?: task_str, parent?: task_str, priority?: int = 5, points?: int, time_estimate_ms?: int, tags?: str[])` | Create a task. Priority 0-10 (higher = more important). |
| `create_tree(tree: object, parent?: task_str)` | Create nested task tree. Tree nodes have `title`, `children[]`, `join_mode` (then/also), etc. |
| `get(task: task_str)` | Get task by ID with attachment metadata and counts. |
| `list_tasks(status?: status_str[], ready?: bool, blocked?: bool, claimed?: bool, owner?: worker_str, parent?: task_str, worker_id?: worker_str, tags_any?: str[], tags_all?: str[], sort_by?: str, sort_order?: str, limit?: int)` | Query tasks with filters. Use `ready=true` for claimable tasks. |
| `update(worker_id: worker_str, task: task_str, status?: status_str, assignee?: worker_str, title?: str, description?: str, priority?: int, points?: int, tags?: str[], needed_tags?: str[], wanted_tags?: str[], time_estimate_ms?: int, reason?: str, force?: bool, attachments?: object[])` | Update task. Status changes auto-manage ownership. Include `attachments` to record commits/changelists. |
| `delete(worker_id: worker_str, task: task_str, cascade?: bool, reason?: str, obliterate?: bool, force?: bool)` | Delete task. Soft delete by default; `obliterate=true` for permanent. |
| `scan(task: task_str, before?: int, after?: int, above?: int, below?: int)` | Scan task graph in multiple directions. Depth: 0=none, N=levels, -1=all. |
| `search(query: str, limit?: int = 20, include_attachments?: bool, status_filter?: status_str)` | FTS5 search. Supports phrases, prefix*, AND/OR/NOT, title:word. |

### Task Claiming

| Tool | Description |
|------|-------------|
| `claim(worker_id: worker_str, task: task_str, force?: bool)` | Claim a task. Fails if deps unsatisfied, at limit, or lacks tags. Use `force` to steal. |

**Note**: Release via `update(status="pending")`. Complete via `update(status="completed")`. Status changes auto-manage ownership.

### Dependencies

| Tool | Description |
|------|-------------|
| `link(from: task_str\|task_str[], to: task_str\|task_str[], type?: dep_str = "blocks")` | Create dependencies. Types: blocks, follows, contains, duplicate, see-also. |
| `unlink(from: task_str\|"*", to: task_str\|"*", type?: dep_str)` | Remove dependencies. Use `*` as wildcard. |
| `relink(prev_from: task_str[], prev_to: task_str[], from: task_str[], to: task_str[], type?: dep_str = "contains")` | Atomically move dependencies (unlink then link). |

### Tracking

| Tool | Description |
|------|-------------|
| `thinking(worker_id: worker_str, thought: str, tasks?: task_str[])` | Broadcast live status. Visible to other workers. Refreshes heartbeat. |
| `task_history(task: task_str, states?: status_str[])` | Get status transition history with time tracking. |
| `project_history(from?: datetime_str, to?: datetime_str, states?: status_str[], limit?: int = 100)` | Project-wide history with date range filters. |
| `log_metrics(worker_id: worker_str, task: task_str, cost_usd?: float, values?: int[8], user_metrics?: object)` | Log metrics (aggregated). |
| `get_metrics(task: task_str\|task_str[])` | Get metrics for task(s). |

### File Coordination

| Tool | Description |
|------|-------------|
| `mark_file(worker_id: worker_str, file: filename\|filename[], task?: task_str, reason?: str)` | Mark file(s) to signal intent. Advisory, non-blocking. |
| `unmark_file(worker_id: worker_str, file?: filename\|filename[]\|"*", task?: task_str, reason?: str)` | Remove marks. Use `*` for all. |
| `list_marks(files?: filename[], worker_id?: worker_str, task?: task_str)` | Get current file marks. |
| `mark_updates(worker_id: worker_str)` | Poll for mark changes since last call. |

### Attachments

| Tool | Description |
|------|-------------|
| `attach(task: task_str\|task_str[], name: str, content?: str, mime?: mime_str, file?: filename, store_as_file?: bool, mode?: str)` | Add attachment. Use `file` for reference, `store_as_file` for media storage. |
| `attachments(task: task_str, name?: str, mime?: mime_str)` | Get attachment metadata. Glob patterns supported for name. |
| `detach(worker_id: worker_str, task: task_str, name: str, delete_file?: bool)` | Delete attachment by name. |

### Advanced

| Tool | Description |
|------|-------------|
| `query(sql: str, params?: str[], limit?: int = 100, format?: str)` | Execute read-only SQL. SELECT only. Requires permission. |

## MCP Resources

| URI | Description |
|-----|-------------|
| `tasks://all` | Full task graph with dependencies |
| `tasks://ready` | Tasks ready to claim |
| `tasks://blocked` | Tasks blocked by dependencies |
| `tasks://claimed` | All claimed tasks |
| `tasks://worker/{id}` | Tasks owned by a worker |
| `tasks://tree/{id}` | Task with all descendants |
| `files://marks` | All file marks |
| `workers://all` | Registered workers |
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

Agents can coordinate file edits using advisory marks with change tracking:

```
Worker A: connect() -> "worker-a"
Worker A: mark_file("worker-a", "src/main.rs", "refactoring")
Worker B: connect() -> "worker-b"
Worker B: mark_updates("worker-b") -> sees worker-a's mark
Worker A: unmark_file("worker-a", "src/main.rs", "ready for review")
Worker B: mark_updates("worker-b") -> sees removal with reason
Worker B: mark_file("worker-b", "src/main.rs", "adding tests")
```

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Agent A    │     │  Agent B    │     │  Agent C    │
│  (Claude)   │     │  (GPT-4)    │     │  (Worker)   │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │ stdio             │ stdio             │ stdio
       ▼                   ▼                   ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ task-graph  │     │ task-graph  │     │ task-graph  │
│    MCP      │     │    MCP      │     │    MCP      │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       └───────────────────┼───────────────────┘
                           ▼
                  ┌─────────────────┐
                  │   SQLite + WAL  │
                  │  .task-graph/   │
                  │    tasks.db     │
                  └─────────────────┘
```

- **Transport**: Stdio — each worker spawns its own server process
- **Database**: SQLite with WAL mode for concurrent access across processes
- **Deployment**: Single binary, no external dependencies, works offline

## Compared to Alternatives

| | Task Graph | Linear task lists | Custom databases |
|---|---|---|---|
| Multi-agent safe | ✓ Atomic claims, file locks | ✗ Race conditions | Maybe, DIY |
| Dependency tracking | ✓ DAG with cycle detection | ✗ Manual ordering | DIY |
| MCP native | ✓ First-class | ✗ Wrapper needed | ✗ Wrapper needed |
| Token accounting | ✓ Built-in | ✗ | DIY |
| Setup required | None | None | Database server |

## License

Apache 2.0

---

Built for the era of AI agents that actually need to work together.
