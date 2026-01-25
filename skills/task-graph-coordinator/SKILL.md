---
name: task-graph-coordinator
description: Orchestrator role for task-graph-mcp - creates task trees, assigns work via tags, monitors progress, and manages multi-agent workflows
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: coordinator
  requires: task-graph-basics
---

# Task Graph Coordinator

Orchestrator role: design task trees, assign work via tags, monitor agent progress, and manage multi-agent workflows.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Quick Start

```
# 1. Connect as coordinator
connect(name="coordinator", tags=["coordinator", "planning"])
→ agent_id

# 2. Design and create task tree
create_tree(tree={
  "title": "Sprint Goal",
  "children": [...]
}, sibling_type="follows")

# 3. Monitor progress
list_tasks(format="markdown")
list_agents(format="markdown")
```

---

## Role Definition

As a **Coordinator**, you:

| Do | Don't |
|----|-------|
| Design task decomposition | Claim implementation tasks |
| Set tag requirements | Do the work yourself |
| Monitor overall progress | Micromanage workers |
| Unblock stuck agents | Force-claim from workers |
| Adjust priorities | Delete workers' progress |

---

## Task Decomposition

### Workflow

```
┌─────────────────────────────────────────────────────┐
│ 1. ANALYZE                                          │
│    • Break goal into deliverables                   │
│    • Identify dependencies                          │
│    • Estimate complexity (points)                   │
├─────────────────────────────────────────────────────┤
│ 2. STRUCTURE                                        │
│    • Choose sibling_type (follows or null)          │
│    • Set tag requirements                           │
│    • Define acceptance criteria (attachments)       │
├─────────────────────────────────────────────────────┤
│ 3. CREATE                                           │
│    • Use create_tree for hierarchy                  │
│    • Add blocked_by for cross-branch deps           │
│    • Attach context/requirements                    │
├─────────────────────────────────────────────────────┤
│ 4. MONITOR                                          │
│    • Watch task status                              │
│    • Track agent progress via thinking()            │
│    • Adjust as needed                               │
└─────────────────────────────────────────────────────┘
```

### Join Mode Selection

| Scenario | Mode | Example |
|----------|------|---------|
| Steps must be sequential | `then` | Design → Implement → Test |
| Steps can parallelize | `also` | Test A + Test B + Test C |
| Mixed | Nested | Parent `also`, children `then` |

### Tag Strategy

```
# Require specific expertise (AND - must have ALL)
agent_tags_all: ["rust", "database"]

# Accept any matching skill (OR - must have ONE)
agent_tags_any: ["frontend", "react", "vue"]

# Combine for nuanced matching
agent_tags_all: ["senior"]
agent_tags_any: ["python", "rust"]  # Senior + (Python OR Rust)
```

---

## Tree Templates

### Sequential Pipeline

```json
{
  "tree": {
    "title": "Feature: User Auth",
    "children": [
      {
        "title": "Design API schema",
        "points": 2,
        "agent_tags_all": ["api-design"]
      },
      {
        "title": "Implement backend",
        "points": 5,
        "agent_tags_all": ["backend"]
      },
      {
        "title": "Add frontend",
        "points": 3,
        "agent_tags_all": ["frontend"]
      },
      {
        "title": "Write tests",
        "points": 2,
        "agent_tags_any": ["testing", "qa"]
      }
    ]
  },
  "sibling_type": "follows"
}
```

### Parallel Workstreams

For complex patterns with parallel tracks containing sequential tasks, build the structure then add links manually:

```json
// Create tree without sibling linking
{
  "tree": {
    "title": "Sprint 5",
    "children": [
      {
        "title": "Backend Track",
        "children": [
          {"title": "API endpoints", "id": "api", "agent_tags_all": ["backend"]},
          {"title": "Database migrations", "id": "db", "agent_tags_all": ["database"]}
        ]
      },
      {
        "title": "Frontend Track",
        "children": [
          {"title": "Component library", "id": "comp", "agent_tags_all": ["frontend"]},
          {"title": "Page integration", "id": "page", "agent_tags_all": ["frontend"]}
        ]
      }
    ]
  }
}
```

```
# Add sequential deps within each track:
link(from="api", to="db", type="follows")
link(from="comp", to="page", type="follows")

# Add cross-branch deps:
link(from="api", to="page", type="blocks")
```

---

## Monitoring

### Dashboard Query

```
# Overview of all work
list_tasks(format="markdown")

# See who's working
list_agents(format="markdown")

# Find bottlenecks
list_tasks(blocked=true, format="markdown")

# Check specific branch
get(task=parent_id, children=true, format="markdown")
```

### Progress Indicators

| Signal | Meaning | Action |
|--------|---------|--------|
| Task `in_progress` long | Agent may be stuck | Check `thinking`, offer help |
| Multiple tasks blocked | Bottleneck | Prioritize blocker |
| Agent disconnected | Crashed or done | Check for orphaned claims |
| No ready tasks | Work complete or stuck | Verify tree completion |

### Intervention Patterns

```
# Reprioritize
update(agent=my_id, task=task_id, priority="critical")

# Unblock manually (if blocker is obsolete)
unlink(from=old_task, to=waiting_task, type="blocks")

# Reassign stuck task
update(agent=my_id, task=stuck_task, state="pending")
# Or force-claim for another agent
claim(agent=other_agent, task=stuck_task, force=true)
```

### Stale Worker Recovery

When a worker disconnects unexpectedly, their claimed tasks remain in-progress:

```
# 1. Check for orphaned claims
list_tasks(status="in_progress", format="markdown")
list_agents(format="markdown")
# Compare: tasks in_progress but owner not in agents list

# 2. Release orphaned tasks back to pool
update(agent=coordinator_id, task=orphaned_task, state="pending", force=true)

# 3. For tasks needing reassignment to specific worker
update(agent=coordinator_id, task=task_id, assignee="new-worker-id", force=true)
```

### Reorganizing Task Trees

When scope changes, use `relink` to atomically move children between parents:

```
# Move children C, D from "Backend" to new "Database" sibling
relink(
  prev_from="backend",
  prev_to=["child-c", "child-d"],
  from="database",
  to=["child-c", "child-d"],
  type="contains"
)
```

---

## Coordination Models: Push vs Pull

Task-graph supports two coordination models. Choose based on your workflow:

### Pull Model (Default)

Workers self-select tasks from the pool. Good for:
- Autonomous agents with clear capabilities
- Dynamic workloads where capacity varies
- Minimal coordinator overhead

```
┌─────────────────────────────────────────────────────────────┐
│ PULL: Workers find and claim their own work                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Coordinator                    Workers                     │
│  ───────────                    ───────                     │
│  1. create_tree(...)            list_tasks(ready=true)      │
│  2. Monitor progress            claim(task)                 │
│                                 ... do work ...             │
│                                 complete(task)              │
│                                                             │
│  Tasks use agent_tags_all/any   Workers register with tags  │
│  to restrict who CAN claim      and claim matching tasks    │
└─────────────────────────────────────────────────────────────┘
```

### Push Model (Assignment)

Coordinator explicitly assigns tasks to specific workers. Good for:
- Specific expertise requirements beyond tags
- Load balancing decisions
- Time-sensitive coordination
- Workers that need direction

```
┌─────────────────────────────────────────────────────────────┐
│ PUSH: Coordinator assigns tasks to specific workers         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Coordinator                    Worker                      │
│  ───────────                    ──────                      │
│  1. create(task)                                            │
│  2. update(task, assignee=      list_tasks(owner=self)      │
│            worker_id)           → sees assigned tasks       │
│     → task.status = "assigned"                              │
│     → task.owner = worker_id    claim(task) to start        │
│                                 → status = "in_progress"    │
│  3. Monitor progress            ... do work ...             │
│                                 complete(task)              │
└─────────────────────────────────────────────────────────────┘
```

### Push Assignment Example

```
# Coordinator assigns task to specific worker
update(
  worker_id=coordinator_id,
  task=task_id,
  assignee="rust-expert-agent"   # ← Push to this worker
)
# Task now has:
#   status: "assigned"
#   owner_agent: "rust-expert-agent"

# Worker sees and starts their assigned work
list_tasks(owner="rust-expert-agent")  # See assigned tasks
claim(worker_id="rust-expert-agent", task=task_id)  # Start work
```

### Hybrid Approach

Combine both models for nuanced control:
- Create tasks with tags for pool eligibility (pull)
- Assign high-priority/specialized tasks directly (push)
- Workers check both assigned tasks AND ready pool

---

## Multi-Agent Patterns

### Hub and Spoke (Pull)

```
Coordinator creates tasks with tags
     │
     ├──→ Worker A claims matching tasks (pull)
     ├──→ Worker B claims matching tasks (pull)
     └──→ Worker C claims matching tasks (pull)
```

### Directed Teams (Push)

```
Coordinator assigns specific tasks
     │
     ├──→ Worker A receives assigned tasks (push)
     ├──→ Worker B receives assigned tasks (push)
     └──→ Worker C receives assigned tasks (push)
```

### Hierarchical

```
Lead Coordinator
     │
     ├──→ Sub-Coordinator (Backend) → assigns to Backend Workers
     │
     └──→ Sub-Coordinator (Frontend) → assigns to Frontend Workers
```

### Peer Review

```json
{
  "tree": {
    "title": "Implement Feature",
    "children": [
      {"title": "Write code", "agent_tags_all": ["developer"]},
      {"title": "Review code", "agent_tags_all": ["reviewer"]}
    ]
  },
  "sibling_type": "follows"
}
```

---

## Cost Management

Track and analyze costs across the project:

```
# Workers log costs on their tasks
log_cost(agent=worker_id, task=task_id,
         tokens_in=1000, tokens_out=500, cost_usd=0.05)

# Coordinator aggregates via queries
get(task=parent_id, children=true)
# Sum up cost_usd across subtasks
```

---

## Handoff Checklist

Before creating a task tree:

- [ ] Goal clearly defined
- [ ] Tasks are atomic (one deliverable each)
- [ ] Dependencies explicit (sibling_type + link)
- [ ] Tags defined for each task
- [ ] Estimates provided (points)
- [ ] Acceptance criteria attached

---

## Anti-Patterns

| Avoid | Why | Instead |
|-------|-----|---------|
| Doing work yourself | Coordinators coordinate | Create tasks for workers |
| Vague task titles | Workers can't understand | Specific, actionable titles |
| Missing dependencies | Race conditions | Explicit `then` or `block` |
| Over-tagging | No one can claim | Minimal necessary tags |
| No monitoring | Issues go unnoticed | Regular `list_tasks` checks |

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference |
| `task-graph-worker` | Understand worker perspective |
| `task-graph-reporting` | Analyze project metrics |
| `task-graph-repair` | Fix broken task structures |
