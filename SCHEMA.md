# Task Graph MCP - Database Schema

> **Current Version:** V005
> **Last Updated:** 2026-01-23
> **Database:** SQLite 3

## Overview

The Task Graph MCP server uses a SQLite database to store tasks, agents, dependencies, file locks, and attachments. The schema supports hierarchical task management, multi-agent coordination, and cost tracking.

---

## Tables

### `agents`

Session-based agent registration for multi-agent coordination.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | TEXT | PRIMARY KEY | Unique agent identifier |
| `name` | TEXT | | Human-readable agent name |
| `tags` | TEXT | | JSON array of freeform capability tags |
| `max_claims` | INTEGER | NOT NULL DEFAULT 5 | Maximum concurrent task claims |
| `registered_at` | INTEGER | NOT NULL | Unix timestamp of registration |
| `last_heartbeat` | INTEGER | NOT NULL | Unix timestamp of last activity |
| `last_claim_sequence` | INTEGER | NOT NULL DEFAULT 0 | Last polled claim sequence ID |

---

### `tasks`

Core task storage with hierarchy, estimation, tracking, and cost accounting.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | TEXT | PRIMARY KEY | Unique task identifier |
| `parent_id` | TEXT | FK → tasks(id) CASCADE | Parent task for hierarchy |
| `title` | TEXT | NOT NULL | Task title |
| `description` | TEXT | | Detailed task description |
| `status` | TEXT | NOT NULL DEFAULT 'pending' | One of: pending, in_progress, completed, failed, cancelled |
| `priority` | TEXT | NOT NULL DEFAULT 'medium' | One of: low, medium, high, critical |
| `join_mode` | TEXT | NOT NULL DEFAULT 'then' | 'then' (sequential) or 'also' (parallel) |
| `sibling_order` | INTEGER | NOT NULL DEFAULT 0 | Position among sibling tasks |
| `owner_agent` | TEXT | FK → agents(id) | Claiming agent |
| `claimed_at` | INTEGER | | Unix timestamp when claimed |
| `needed_tags` | TEXT | | JSON array - agent must have ALL (AND logic) |
| `wanted_tags` | TEXT | | JSON array - agent must have AT LEAST ONE (OR logic) |
| `points` | INTEGER | | Story points or complexity estimate |
| `time_estimate_ms` | INTEGER | | Estimated duration in milliseconds |
| `time_actual_ms` | INTEGER | | Actual duration in milliseconds |
| `started_at` | INTEGER | | Unix timestamp when work began |
| `completed_at` | INTEGER | | Unix timestamp when finished |
| `current_thought` | TEXT | | Live status message from agent |
| `tokens_in` | INTEGER | NOT NULL DEFAULT 0 | Input tokens consumed |
| `tokens_cached` | INTEGER | NOT NULL DEFAULT 0 | Cached tokens used |
| `tokens_out` | INTEGER | NOT NULL DEFAULT 0 | Output tokens generated |
| `tokens_thinking` | INTEGER | NOT NULL DEFAULT 0 | Thinking/reasoning tokens |
| `tokens_image` | INTEGER | NOT NULL DEFAULT 0 | Image tokens processed |
| `tokens_audio` | INTEGER | NOT NULL DEFAULT 0 | Audio tokens processed |
| `cost_usd` | REAL | NOT NULL DEFAULT 0.0 | Total cost in USD |
| `user_metrics` | TEXT | | JSON object for custom metrics |
| `created_at` | INTEGER | NOT NULL | Unix timestamp of creation |
| `updated_at` | INTEGER | NOT NULL | Unix timestamp of last update |

**Indexes:**
- `idx_tasks_parent` on `parent_id`
- `idx_tasks_owner` on `owner_agent`
- `idx_tasks_status` on `status`

---

### `attachments`

Task outputs, logs, and artifacts with support for inline content or file references.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `task_id` | TEXT | NOT NULL, FK → tasks(id) CASCADE | Parent task |
| `order_index` | INTEGER | NOT NULL | Auto-incrementing order within task |
| `name` | TEXT | NOT NULL | Attachment name/label |
| `mime_type` | TEXT | NOT NULL DEFAULT 'text/plain' | Content MIME type |
| `content` | TEXT | NOT NULL | Text content or base64-encoded binary |
| `file_path` | TEXT | | Path to file in `.task-graph/media/` (if set, content is in file) |
| `created_at` | INTEGER | NOT NULL | Unix timestamp of creation |

**Primary Key:** `(task_id, order_index)`

**Indexes:**
- `idx_attachments_task` on `task_id`

---

### `dependencies`

DAG edges representing task dependencies (task A blocks task B).

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `from_task_id` | TEXT | NOT NULL, FK → tasks(id) CASCADE | Blocking task |
| `to_task_id` | TEXT | NOT NULL, FK → tasks(id) CASCADE | Blocked task |

**Primary Key:** `(from_task_id, to_task_id)`

**Indexes:**
- `idx_deps_to` on `to_task_id`

---

### `file_locks`

Advisory file locks for coordinating file access between agents.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `file_path` | TEXT | PRIMARY KEY | Locked file path |
| `agent_id` | TEXT | NOT NULL, FK → agents(id) | Lock owner |
| `reason` | TEXT | | Reason for the lock |
| `locked_at` | INTEGER | NOT NULL | Unix timestamp of lock acquisition |

**Indexes:**
- `idx_file_locks_agent` on `agent_id`

---

### `claim_sequence`

Event log for file claim/release tracking, enabling efficient polling.

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Monotonic sequence ID |
| `file_path` | TEXT | NOT NULL | Affected file path |
| `agent_id` | TEXT | NOT NULL | Agent performing the action |
| `event` | TEXT | NOT NULL | 'claimed' or 'released' |
| `reason` | TEXT | | Optional reason for the event |
| `timestamp` | INTEGER | NOT NULL | Unix timestamp of the event |

**Indexes:**
- `idx_claim_sequence_file` on `(file_path, id)`

---

## Enums (Application Layer)

### TaskStatus
- `pending` - Not yet started
- `in_progress` - Currently being worked on
- `completed` - Successfully finished
- `failed` - Finished with errors
- `cancelled` - Manually cancelled

### Priority
- `low`
- `medium` (default)
- `high`
- `critical`

### JoinMode
- `then` - Children execute sequentially
- `also` - Children execute in parallel

### ClaimEventType
- `claimed` - File was locked by agent
- `released` - File was unlocked

---

## Revision History

| Version | Date | Description |
|---------|------|-------------|
| V001 | 2026-01-23 | Initial schema with agents, tasks, attachments, dependencies, file_locks, subscriptions, and inbox tables |
| V002 | 2026-01-23 | Remove `metadata` column from tasks (use attachments instead); add `order_index` to attachments |
| V003 | 2026-01-23 | Change attachments primary key from UUID to composite `(task_id, order_index)` |
| V004 | 2026-01-23 | Add `claim_sequence` table for file claim tracking; add `last_claim_sequence` to agents; add `reason` to file_locks; drop pub/sub tables (inbox, subscriptions) |
| V005 | 2026-01-23 | Add `file_path` column to attachments for media file references |

---

## Entity Relationships

```
agents 1──────< tasks (owner_agent)
agents 1──────< file_locks (agent_id)
agents 1──────< claim_sequence (agent_id)

tasks 1──────< tasks (parent_id, self-referential hierarchy)
tasks 1──────< attachments (task_id)
tasks >──────< tasks (via dependencies table, DAG)
```

---

## Notes

- All timestamps are Unix epoch integers (seconds)
- JSON fields (`tags`, `needed_tags`, `wanted_tags`, `user_metrics`) are stored as TEXT
- File paths in `file_locks` and `claim_sequence` are relative to the project root
- Attachment `file_path` references files in `.task-graph/media/` directory
- Foreign keys use `ON DELETE CASCADE` for automatic cleanup
