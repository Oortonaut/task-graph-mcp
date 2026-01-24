---
name: task-graph-basics
description: Foundation skill for task-graph-mcp - connection workflow, tool reference, and shared patterns for multi-agent coordination
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: foundation
---

# Task Graph Basics

Foundation skill providing shared patterns, tool reference, and connection workflow for task-graph-mcp.

**This skill is automatically referenced by all other task-graph skills.**

---

## Quick Start

```
# First thing in any session - connect as an agent
connect(tags=["your", "capabilities"])
→ Returns agent_id (SAVE THIS for all subsequent calls)

# Find work
list_tasks(ready=true, agent=agent_id)

# Claim and work
claim(agent=agent_id, task=task_id)
thinking(agent=agent_id, thought="Working on X...")
complete(agent=agent_id, task=task_id)
```

---

## Connection Workflow

Every agent MUST connect before using task-graph tools:

```
┌─────────────────────────────────────────────────────┐
│ 1. CONNECT                                          │
│    connect(                                         │
│      name="my-agent",           # Display name      │
│      tags=["python", "testing"], # Capabilities     │
│      max_claims=5               # Concurrent limit  │
│    )                                                │
│    → Returns: agent_id (UUID7)                      │
│    → SAVE THIS ID for all subsequent calls          │
├─────────────────────────────────────────────────────┤
│ 2. WORK (use agent_id in all calls)                 │
│    list_tasks, claim, thinking, complete, etc.      │
├─────────────────────────────────────────────────────┤
│ 3. DISCONNECT (when done)                           │
│    disconnect(agent=agent_id)                       │
│    → Releases all claims and locks                  │
└─────────────────────────────────────────────────────┘
```

**Tags enable task affinity:**
- `needed_tags` on tasks: Agent must have ALL (AND logic)
- `wanted_tags` on tasks: Agent must have AT LEAST ONE (OR logic)

---

## Tool Reference

### Agent Management

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `connect` | Register as agent | `name`, `tags[]`, `max_claims` |
| `disconnect` | Unregister, release all | `agent` |
| `list_agents` | See all agents | `format` |

### Task CRUD

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `create` | Single task | `title`, `description`, `parent`, `priority`, `blocked_by[]`, `needed_tags[]`, `wanted_tags[]` |
| `create_tree` | Nested structure | `tree` (recursive), `parent` |
| `get` | Fetch task | `task`, `children`, `format` |
| `list_tasks` | Query tasks | `status`, `ready`, `blocked`, `owner`, `parent`, `agent`, `format` |
| `update` | Modify task | `agent`, `task`, `state`, `title`, `description`, `priority` |
| `delete` | Remove task | `task`, `cascade` |

### Claiming & Completion

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `claim` | Take ownership | `agent`, `task`, `force` |
| `release` | Give up task | `agent`, `task`, `state` |
| `complete` | Mark done | `agent`, `task` |

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
| `claim_file` | Advisory lock | `agent`, `file`, `reason` |
| `release_file` | Release lock | `agent`, `file`, `reason` |
| `list_files` | Current locks | `agent`, `files[]` |
| `claim_updates` | Poll changes | `agent`, `files[]` |

### Progress & Metrics

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `thinking` | Live status | `agent`, `thought`, `tasks[]` |
| `get_state_history` | Audit trail | `task` |
| `log_cost` | Track usage | `agent`, `task`, `tokens_in`, `tokens_out`, `cost_usd` |

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

**Timed states** automatically track `time_actual_ms`.

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
list_tasks(ready=true, agent=agent_id)
# Returns: unclaimed tasks with satisfied deps matching agent's tags
```

### Find blocked tasks
```
list_tasks(blocked=true)
# Returns: tasks waiting on dependencies
```

### Find my tasks
```
list_tasks(owner=agent_id)
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
- Save your `agent_id` after connecting
- Use `thinking()` frequently to show progress
- Claim files before editing (`claim_file`)
- Check `claim_updates` before editing shared files
- Log costs with `log_cost` for tracking

### Never Do
- Claim tasks without checking dependencies
- Edit files without advisory locks
- Forget to `complete` or `release` tasks
- Ignore tag requirements on tasks

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| "Task already claimed" | Another agent owns it | Use `force=true` or pick another |
| "Dependencies not satisfied" | Blockers incomplete | Wait or help complete blockers |
| "Agent not found" | Invalid/expired agent_id | Reconnect |
| "Tag mismatch" | Agent lacks required tags | Check `needed_tags`/`wanted_tags` |

---

## Related Skills

| Skill | Purpose |
|-------|---------|
| `task-graph-coordinator` | Orchestrate work, create task trees |
| `task-graph-worker` | Claim and complete tasks |
| `task-graph-reporting` | Analyze metrics and progress |
| `task-graph-migration` | Import from other systems |
| `task-graph-repair` | Fix orphaned/broken tasks |
