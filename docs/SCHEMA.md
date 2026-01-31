# Task Graph MCP - Database Schema

> **Current Version:** V006
> **Last Updated:** 2026-01-31
> **Database:** SQLite 3

## Overview

The Task Graph MCP server uses a SQLite database to store tasks, workers, dependencies, file locks, and attachments. The schema supports hierarchical task management via typed dependency edges, multi-agent coordination, phase tracking, and cost accounting.

---

## Tables

### `workers`

Session-based worker registration for multi-agent coordination.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | TEXT | PRIMARY KEY | Unique worker identifier |
| `tags` | TEXT | | JSON array of freeform capability tags |
| `max_claims` | INTEGER | NOT NULL DEFAULT 5 | Maximum concurrent task claims |
| `registered_at` | INTEGER | NOT NULL | Unix timestamp of registration |
| `last_heartbeat` | INTEGER | NOT NULL | Unix timestamp of last activity |
| `last_claim_sequence` | INTEGER | NOT NULL DEFAULT 0 | Last polled claim sequence ID |
| `last_status` | TEXT | | Last status the worker transitioned to (for prompts/dashboard) |
| `last_phase` | TEXT | | Last phase the worker transitioned to (for prompts/dashboard) |
| `workflow` | TEXT | | Named workflow file in use (e.g., `"swarm"` for `workflow-swarm.yaml`); NULL means default `workflows.yaml` |
| `overlays` | TEXT | | JSON array of overlay names applied on top of the workflow (e.g., `'["git","user-request"]'`); NULL means no overlays |

**Indexes:**
- `idx_workers_heartbeat` on `last_heartbeat`

---

### `tasks`

Core task storage with estimation, tracking, phase classification, and cost accounting.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | TEXT | PRIMARY KEY | Unique task identifier |
| `title` | TEXT | NOT NULL | Task title |
| `description` | TEXT | | Detailed task description |
| `status` | TEXT | NOT NULL DEFAULT 'pending' | Task status (configurable, see States Configuration) |
| `phase` | TEXT | | Type of work being done (e.g., explore, design, implement, test) |
| `priority` | TEXT | NOT NULL DEFAULT 'medium' | Priority 0-10 (higher = more important, default 5). DDL type is TEXT for legacy reasons; the application layer parses, clamps to 0-10, and stores as a string |
| `worker_id` | TEXT | FK -> workers(id) | Claiming worker |
| `claimed_at` | INTEGER | | Unix timestamp when claimed |
| `needed_tags` | TEXT | | JSON array - worker must have ALL (AND logic) for claiming |
| `wanted_tags` | TEXT | | JSON array - worker must have AT LEAST ONE (OR logic) for claiming |
| `tags` | TEXT | DEFAULT '[]' | JSON array - categorization/discovery tags (queryable) |
| `points` | INTEGER | | Story points or complexity estimate |
| `time_estimate_ms` | INTEGER | | Estimated duration in milliseconds |
| `time_actual_ms` | INTEGER | | Actual duration in milliseconds (accumulated from timed states) |
| `started_at` | INTEGER | | Unix timestamp when work began |
| `completed_at` | INTEGER | | Unix timestamp when finished |
| `current_thought` | TEXT | | Live status message from agent |
| `metric_0` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 0 |
| `metric_1` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 1 |
| `metric_2` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 2 |
| `metric_3` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 3 |
| `metric_4` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 4 |
| `metric_5` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 5 |
| `metric_6` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 6 |
| `metric_7` | INTEGER | NOT NULL DEFAULT 0 | Generic metric slot 7 |
| `cost_usd` | REAL | NOT NULL DEFAULT 0.0 | Total cost in USD |
| `deleted_at` | INTEGER | | Unix timestamp of soft deletion |
| `deleted_by` | TEXT | | Worker ID that performed the soft deletion |
| `deleted_reason` | TEXT | | Reason for soft deletion |
| `created_at` | INTEGER | NOT NULL | Unix timestamp of creation |
| `updated_at` | INTEGER | NOT NULL | Unix timestamp of last update |

**Indexes:**
- `idx_tasks_worker` on `worker_id`
- `idx_tasks_worker_status` on `(worker_id, status)`
- `idx_tasks_status` on `status`
- `idx_tasks_claimed` on `claimed_at` WHERE `worker_id IS NOT NULL`
- `idx_tasks_deleted` on `deleted_at`
- `idx_tasks_phase` on `phase`
- `idx_tasks_phase_status` on `(phase, status)`

---

### `attachments`

Task outputs, logs, and artifacts with support for inline content or file references. Keyed by `(task_id, attachment_type, sequence)`.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Parent task |
| `attachment_type` | TEXT | NOT NULL | Category of attachment (e.g., `"commit"`, `"note"`, `"changelist"`) |
| `sequence` | INTEGER | NOT NULL | Auto-incrementing order within `(task_id, attachment_type)` |
| `name` | TEXT | NOT NULL DEFAULT '' | Arbitrary label for the attachment |
| `mime_type` | TEXT | NOT NULL DEFAULT 'text/plain' | Content MIME type |
| `content` | TEXT | NOT NULL | Text content or base64-encoded binary |
| `file_path` | TEXT | | Path to file in `.task-graph/media/` (if set, content is in file) |
| `created_at` | INTEGER | NOT NULL | Unix timestamp of creation |

**Primary Key:** `(task_id, attachment_type, sequence)`

**Indexes:**
- `idx_attachments_task` on `task_id`
- `idx_attachments_task_type` on `(task_id, attachment_type)`

---

### `dependencies`

DAG edges representing typed relationships between tasks.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `from_task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Source task |
| `to_task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Target task |
| `dep_type` | TEXT | NOT NULL DEFAULT 'blocks' | Dependency type (see Dependency Types below) |

**Primary Key:** `(from_task_id, to_task_id, dep_type)`

**Indexes:**
- `idx_deps_to` on `to_task_id`
- `idx_deps_from` on `from_task_id`
- `idx_deps_type` on `dep_type`
- `idx_deps_type_to` on `(dep_type, to_task_id)`
- `idx_deps_from_type` on `(from_task_id, dep_type)`

#### Dependency Types

Dependency types are configurable. The default types are:

| Type | Display | Blocks | Description |
|------|---------|--------|-------------|
| `blocks` | Horizontal | Start | Task A must complete before task B can be claimed |
| `follows` | Horizontal | Start | Sequential ordering (task B follows task A) |
| `contains` | Vertical | Completion | Parent-child hierarchy (task A contains task B; A cannot complete until B completes) |
| `duplicate` | Horizontal | None | Informational: marks tasks as duplicates |
| `see-also` | Horizontal | None | Informational: cross-reference between tasks |
| `relates-to` | Horizontal | None | Informational: general relationship link |

**Block targets:**
- `start` - Blocks the target task from being started/claimed
- `completion` - Blocks the source task from being completed
- `none` - Informational link only, no blocking behavior

---

### `file_locks`

Advisory file locks for coordinating file access between agents.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `file_path` | TEXT | PRIMARY KEY | Locked file path (or `lock:resource` for exclusive locks) |
| `worker_id` | TEXT | NOT NULL, FK -> workers(id) | Lock owner |
| `task_id` | TEXT | FK -> tasks(id) | Associated task (optional) |
| `reason` | TEXT | | Reason for the lock |
| `locked_at` | INTEGER | NOT NULL | Unix timestamp of lock acquisition |

**Indexes:**
- `idx_file_locks_worker` on `worker_id`
- `idx_file_locks_task` on `task_id`

---

### `claim_sequence`

Event log for file claim/release tracking, enabling efficient polling.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Monotonic sequence ID |
| `file_path` | TEXT | NOT NULL | Affected file path |
| `worker_id` | TEXT | NOT NULL | Worker performing the action |
| `event` | TEXT | NOT NULL | `'claimed'` or `'released'` |
| `reason` | TEXT | | Optional reason for the event |
| `claim_id` | INTEGER | | For releases: references the original claim event |
| `timestamp` | INTEGER | NOT NULL | Unix timestamp of the event |
| `end_timestamp` | INTEGER | | Unix timestamp when this claim period ended |

**Indexes:**
- `idx_claim_sequence_file` on `(file_path, id)`
- `idx_claim_seq_file_worker` on `(file_path, worker_id)`
- `idx_claim_seq_open` on `file_path` WHERE `end_timestamp IS NULL`

---

### File Coordination Model

File claims enable multi-agent coordination through intent communication:

1. **Claiming with reason**: When an agent claims a file, they provide a reason describing their intent (e.g., "Renaming state to status", "Fixing null check in validate()")

2. **Visibility on conflict**: When another agent tries to claim the same file, they see who has it and why, enabling informed decisions:
   - Wait for the other agent to finish
   - Work around their changes (use their new naming, etc.)
   - Move on to other work if the issue is already being addressed

3. **Polling for updates**: Agents poll `mark_updates` to see marks/removals as they happen, maintaining awareness of what's being worked on

4. **Release notifications**: When a file is released, waiting agents are notified and can claim it

This model prevents:
- Blind overwrites of others' changes
- Duplicate effort on the same problem
- Merge conflicts from uncoordinated edits

---

### `task_sequence`

Unified, append-only audit log of task status and phase transitions, enabling automatic time tracking. Uses a snapshot pattern: each row records the new status and/or phase values; NULL means the field did not change from the previous row.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Monotonic sequence ID |
| `task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Task being transitioned |
| `worker_id` | TEXT | | Worker performing the transition (optional) |
| `status` | TEXT | | New status value (NULL if status did not change) |
| `phase` | TEXT | | New phase value (NULL if phase did not change) |
| `reason` | TEXT | | Optional reason for the transition |
| `timestamp` | INTEGER | NOT NULL | Unix timestamp when the transition occurred |
| `end_timestamp` | INTEGER | | Unix timestamp when this transition was superseded |

**Indexes:**
- `idx_task_seq_task_timestamp` on `(task_id, timestamp)`
- `idx_task_seq_timestamp` on `timestamp`
- `idx_task_seq_status` on `status` WHERE `status IS NOT NULL`
- `idx_task_seq_phase` on `phase` WHERE `phase IS NOT NULL`

**Notes:**
- Time spent in "timed" states (e.g., `working`) is automatically added to `time_actual_ms` when transitioning out
- The `end_timestamp` is filled when the next transition occurs
- Provides complete audit trail of both status and phase changes in a single timeline

---

### Tag Junction Tables

Normalized junction tables for efficient tag-based queries. Maintained in sync with the JSON tag columns on `tasks`.

#### `task_tags`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Task |
| `tag` | TEXT | NOT NULL | Tag value |

**Primary Key:** `(task_id, tag)`
**Indexes:** `idx_task_tags_tag` on `tag`

#### `task_needed_tags`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Task |
| `tag` | TEXT | NOT NULL | Required tag |

**Primary Key:** `(task_id, tag)`
**Indexes:** `idx_task_needed_tags_tag` on `tag`

#### `task_wanted_tags`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `task_id` | TEXT | NOT NULL, FK -> tasks(id) CASCADE | Task |
| `tag` | TEXT | NOT NULL | Desired tag |

**Primary Key:** `(task_id, tag)`
**Indexes:** `idx_task_wanted_tags_tag` on `tag`

---

### Full-Text Search (FTS5)

#### `tasks_fts`

Virtual FTS5 table for full-text search over tasks.

| Column | Indexed | Description |
|--------|---------|-------------|
| `task_id` | No (UNINDEXED) | Task ID for joining |
| `title` | Yes | Task title |
| `description` | Yes | Task description |

Maintained by triggers: `tasks_fts_insert`, `tasks_fts_update`, `tasks_fts_delete`.

#### `attachments_fts`

Virtual FTS5 table for full-text search over text attachments.

| Column | Indexed | Description |
|--------|---------|-------------|
| `task_id` | No (UNINDEXED) | Task ID for joining |
| `attachment_type` | No (UNINDEXED) | Attachment type for joining |
| `sequence` | No (UNINDEXED) | Sequence for joining |
| `name` | Yes | Attachment name |
| `content` | Yes | Attachment content |

Maintained by triggers: `attachments_fts_insert`, `attachments_fts_update`, `attachments_fts_delete`. Only text MIME types (`text/%`) are indexed.

---

## States Configuration

Task states are fully configurable via YAML. The configuration defines:

- **initial** - Default state for new tasks (default: `pending`)
- **disconnect_state** - State for tasks when their owner disconnects; must be untimed (default: `pending`)
- **blocking_states** - States that block dependent tasks (default: `[pending, working]`)
- **definitions** - Per-state settings including allowed transitions and time tracking

### Default States

```yaml
states:
  initial: pending
  disconnect_state: pending
  blocking_states: [pending, working]
  definitions:
    pending:
      exits: [working, cancelled]
      timed: false
    working:
      exits: [completed, failed, pending]
      timed: true
    completed:
      exits: []
      timed: false
    failed:
      exits: [pending]
      timed: false
    cancelled:
      exits: []
      timed: false
```

### State Definition Properties

| Property | Type | Description |
|----------|------|-------------|
| `exits` | string[] | Allowed target states for transitions |
| `timed` | boolean | If true, time in this state accumulates to `time_actual_ms` |

### State Transition Rules

1. Transitions are validated against the current state's `exits` list
2. When exiting a `timed` state, duration is added to `time_actual_ms`
3. States with empty `exits` are terminal (e.g., completed, cancelled)
4. The `started_at` timestamp is set on first entry to any timed state
5. The `completed_at` timestamp is set when entering a terminal state

### Dependency Propagation

When a task transitions from a blocking state to a non-blocking state (e.g., `working` -> `completed`), the system automatically:

1. **Reports unblocked tasks** - The `update` tool response includes an `unblocked` array listing task IDs whose dependencies are now satisfied
2. **Optionally auto-advances** - If `auto_advance` is enabled, unblocked tasks transition to the configured target state

### Auto-Advance Configuration

```yaml
auto_advance:
  enabled: false          # Default: disabled
  target_state: "ready"   # Target state for auto-advanced tasks
```

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable automatic state transitions for unblocked tasks |
| `target_state` | string | `null` | Target state for auto-advanced tasks (e.g., "ready") |

**Example response when completing a blocker:**
```json
{
  "task": { "id": "task-1", "status": "completed", "..." : "..." },
  "unblocked": ["task-2", "task-3"],
  "auto_advanced": ["task-2", "task-3"]
}
```

**Notes:**
- `unblocked` is always reported, regardless of `auto_advance` settings
- `auto_advanced` only appears when `enabled: true` and `target_state` is set
- Tasks are only auto-advanced if they are in the initial state (e.g., "pending")
- Cascading is supported: if auto-advancing task A unblocks task B, B may also auto-advance

---

## Tagging System

Tasks support two types of tags:

### Categorization Tags (`tags`)

The `tags` column contains a JSON array of strings for categorization and discovery:
- Used for querying/filtering tasks by category
- Does NOT affect who can claim the task
- Think of these as "what the task IS" (e.g., `["backend", "api", "urgent"]`)

### Claiming Requirement Tags (`needed_tags` / `wanted_tags`)

These control which workers can claim a task:

| Field | Logic | Description |
|-------|-------|-------------|
| `needed_tags` | AND | Worker must have ALL tags to claim |
| `wanted_tags` | OR | Worker must have AT LEAST ONE tag to claim |

### Query Parameters

The `list_tasks` tool supports tag-based filtering:

| Parameter | Description |
|-----------|-------------|
| `tags_any` | Return tasks that have ANY of the specified tags (OR) |
| `tags_all` | Return tasks that have ALL of the specified tags (AND) |
| `agent` | When combined with `ready`, filters for tasks the specified agent is qualified to claim (checks needed_tags/wanted_tags) |

### Examples

```
# Create a task with both tag types
create(
  title="API endpoint",
  tags=["backend", "api"],           # For discovery
  needed_tags=["senior"],            # Only seniors can claim
  wanted_tags=["python", "rust"]     # Must know python OR rust
)

# Query by categorization (agent-driven: "what tasks match my interests?")
list_tasks(tags_any=["backend", "frontend"])  # Tasks in either category
list_tasks(tags_all=["urgent", "api"])        # Tasks with BOTH tags

# Query by qualification (task-driven: "what tasks want me?")
list_tasks(ready=true, agent="agent-1")  # Ready tasks this agent can claim
```

---

## Enums (Application Layer)

### Priority
- Integer range `0` to `10` (default `5`)
- Higher values = more important
- Stored as TEXT in the database, parsed and clamped to 0-10 at the application layer
- Legacy string values (`"low"`, `"medium"`, `"high"`, `"critical"`) are converted via `parse_priority()`

### ClaimEventType
- `claimed` - File was locked by agent
- `released` - File was unlocked

---

## Revision History

| Version | Date | Description |
|---------|------|-------------|
| V001 | 2026-01-27 | Initial schema with workers, tasks, attachments, dependencies, file_locks, claim_sequence, task_state_sequence, tag junction tables, FTS5 indexes |
| V002 | 2026-01-27 | Drop unused `user_metrics` column from tasks |
| V003 | 2026-01-28 | Add `phase` column to tasks; create unified `task_sequence` table (replaces `task_state_sequence`); add `last_status` and `last_phase` to workers; rename `in_progress` status to `working` |
| V004 | 2026-01-28 | Replace attachments primary key from `(task_id, order_index)` to `(task_id, attachment_type, sequence)`; add `attachment_type` column |
| V005 | 2026-01-29 | Add `workflow` column to workers for named workflow file tracking |
| V006 | 2026-01-30 | Add `overlays` column to workers for workflow overlay tracking |

---

## Entity Relationships

```
workers 1──────< tasks (worker_id)
workers 1──────< file_locks (worker_id)
workers 1──────< claim_sequence (worker_id)
workers 1──────< task_sequence (worker_id, optional)

tasks 1──────< attachments (task_id)
tasks 1──────< task_sequence (task_id)
tasks 1──────< file_locks (task_id, optional)
tasks >──────< tasks (via dependencies table, typed DAG)
tasks 1──────< task_tags (task_id)
tasks 1──────< task_needed_tags (task_id)
tasks 1──────< task_wanted_tags (task_id)
```

---

## Notes

- All timestamps are Unix epoch integers (milliseconds)
- JSON fields (`tags`, `needed_tags`, `wanted_tags`, `overlays`) are stored as TEXT
- File paths in `file_locks` and `claim_sequence` are relative to the project root
- Attachment `file_path` references files in `.task-graph/media/` directory
- Foreign keys use `ON DELETE CASCADE` for automatic cleanup
- The database uses WAL mode for concurrent access with a 5000ms busy timeout
- Schema migrations are managed by [refinery](https://crates.io/crates/refinery) embedded migrations
