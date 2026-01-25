---
name: task-graph-basics
description: Foundation skill for task-graph-mcp - connection workflow, tool reference, and shared patterns for multi-worker coordination
license: Apache-2.0
metadata:
  version: 1.1.0
  suite: task-graph-mcp
  role: foundation
---

# Task Graph Basics

Foundation skill providing shared patterns, tool reference, and connection workflow for task-graph-mcp.

**This skill is automatically referenced by all other task-graph skills.**

---

## Quick Start

```
# First thing in any session - connect as a worker
connect(tags=["your", "capabilities"])
→ Returns worker_id (SAVE THIS for all subsequent calls)

# Find work
list_tasks(ready=true, worker_id=worker_id)

# Claim and work
claim(worker_id=worker_id, task=task_id)
thinking(worker_id=worker_id, thought="Working on X...")
update(worker_id=worker_id, task=task_id, state="completed")
```

---

## Connection Workflow

Every worker MUST connect before using task-graph tools:

```
┌─────────────────────────────────────────────────────┐
│ 1. CONNECT                                          │
│    connect(                                         │
│      worker_id="my-worker-id", # Optional custom ID │
│      tags=["python", "testing"], # Capabilities     │
│      force=true               # Reconnect if exists │
│    )                                                │
│    → Returns: worker_id                             │
│    → SAVE THIS ID for all subsequent calls          │
├─────────────────────────────────────────────────────┤
│ 2. WORK (use worker_id in all calls)                │
│    list_tasks, claim, thinking, update, etc.        │
├─────────────────────────────────────────────────────┤
│ 3. DISCONNECT (when done)                           │
│    disconnect(worker_id=worker_id)                  │
│    → Releases all claims and locks                  │
└─────────────────────────────────────────────────────┘
```

**Tags enable task affinity:**
- `needed_tags` on tasks: Worker must have ALL (AND logic)
- `wanted_tags` on tasks: Worker must have AT LEAST ONE (OR logic)

---

## Tool Reference

### Worker Management

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `connect` | Register as worker | `worker_id` (optional ID), `tags[]`, `force` |
| `disconnect` | Unregister, release all | `worker_id` |
| `list_workers` | See all workers | `format` |

### Task CRUD

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `create` | Single task | `title`, `description`, `parent`, `priority`, `blocked_by[]`, `needed_tags[]`, `wanted_tags[]` |
| `create_tree` | Nested structure | `tree` (recursive), `parent` |
| `get` | Fetch task | `task`, `children`, `format` |
| `list_tasks` | Query tasks | `status`, `ready`, `blocked`, `owner`, `parent`, `worker_id`, `format` |
| `update` | Modify task & state | `worker_id`, `task`, `state`, `title`, `description`, `priority`, `force` |
| `delete` | Remove task | `task`, `cascade` |

### Claiming & Ownership

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `claim` | Take ownership (shortcut) | `worker_id`, `task`, `force` |

**Ownership via `update`:**
- `update(state="in_progress")` → Claims task (sets owner)
- `update(state="pending")` → Releases task (clears owner)
- `update(state="completed")` → Completes task (clears owner)
- Use `force=true` to take from another worker

### Dependencies

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `block` | Add dependency | `blocker`, `blocked`, `type` |
| `unblock` | Remove dependency | `blocker`, `blocked`, `type` |

**Dependency types:**
- `blocks` - blocker must complete before blocked starts
- `follows` - sequential ordering
- `contains` - parent-child relationship

### File Coordination

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `claim_file` | Advisory lock | `worker_id`, `file`, `reason` |
| `release_file` | Release lock | `worker_id`, `file`, `reason` |
| `list_files` | Current locks | `worker_id`, `files[]` |
| `claim_updates` | Poll changes | `worker_id`, `files[]` |

### Progress & Metrics

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `thinking` | Live status | `worker_id`, `thought`, `tasks[]` |
| `get_state_history` | Audit trail | `task` |
| `log_cost` | Track usage | `worker_id`, `task`, `tokens_in`, `tokens_out`, `cost_usd` |

### Attachments

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `attach` | Add content | `task`, `name`, `content`, `mime`, `file`, `store_as_file` |
| `attachments` | List/get | `task`, `content` |
| `detach` | Remove | `task`, `index` |

---

## Task States

Default state machine (configurable via YAML):

```
pending ──→ in_progress ──→ completed
   │             │
   │             └──→ failed ──→ pending (retry)
   │
   └──→ cancelled
```

| State | Timed | Exits To |
|-------|-------|----------|
| `pending` | No | `in_progress`, `cancelled` |
| `in_progress` | Yes | `completed`, `failed`, `pending` |
| `completed` | No | (terminal) |
| `failed` | No | `pending` |
| `cancelled` | No | (terminal) |

**Timed states** (like `in_progress`) automatically:
- Set owner when entering
- Track `time_actual_ms`
- Clear owner when leaving

---

## Unified Update Behavior

The `update` tool handles ownership based on state transitions:

```
┌────────────────────────────────────────────────────────┐
│ Transition to TIMED state (e.g., in_progress)          │
│ → CLAIMS task: validates tags, checks limit, sets owner│
├────────────────────────────────────────────────────────┤
│ Transition to NON-TIMED state (e.g., pending)          │
│ → RELEASES task: clears owner                          │
├────────────────────────────────────────────────────────┤
│ Transition to TERMINAL state (e.g., completed)         │
│ → COMPLETES task: checks children, releases file locks │
└────────────────────────────────────────────────────────┘
```

**Force parameter:**
- `update(force=true)` takes ownership from another worker
- `claim(force=true)` same behavior (claim is shortcut for update)

---

## Task Trees

Use `create_tree` for hierarchical task structures:

```json
{
  "tree": {
    "title": "Feature X",
    "join_mode": "then",
    "children": [
      {"title": "Design", "points": 3},
      {"title": "Implement", "points": 5},
      {"title": "Test", "points": 2}
    ]
  }
}
```

**Join modes:**
- `then` - Children execute sequentially (auto-creates dependencies)
- `also` - Children execute in parallel

---

## Query Patterns

### Find available work
```
list_tasks(ready=true, worker_id=worker_id)
# Returns: unclaimed tasks with satisfied deps matching worker's tags
```

### Find blocked tasks
```
list_tasks(blocked=true)
# Returns: tasks waiting on dependencies
```

### Find my tasks
```
list_tasks(owner=worker_id)
# Returns: tasks I've claimed
```

### Get root tasks only
```
list_tasks(parent="null")
# Returns: top-level tasks
```

### Get formatted output
```
list_tasks(format="markdown")
get(task=task_id, children=true, format="markdown")
```

---

## Best Practices

### Always Do
- Save your `worker_id` after connecting
- Use `thinking()` frequently to show progress
- Claim files before editing (`claim_file`)
- Check `claim_updates` before editing shared files
- Log costs with `log_cost` for tracking

### Never Do
- Claim tasks without checking dependencies
- Edit files without advisory locks
- Leave tasks in limbo (always update state)
- Ignore tag requirements on tasks

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| "Task already claimed" | Another worker owns it | Use `force=true` or pick another |
| "Dependencies not satisfied" | Blockers incomplete | Wait or help complete blockers |
| "Worker not found" | Invalid/expired worker_id | Reconnect with `force=true` |
| "Tag mismatch" | Worker lacks required tags | Check `needed_tags`/`wanted_tags` |

---

## Related Skills

| Skill | Purpose |
|-------|---------|
| `task-graph-coordinator` | Orchestrate work, create task trees |
| `task-graph-worker` | Claim and complete tasks |
| `task-graph-reporting` | Analyze metrics and progress |
| `task-graph-migration` | Import from other systems |
| `task-graph-repair` | Fix orphaned/broken tasks |
