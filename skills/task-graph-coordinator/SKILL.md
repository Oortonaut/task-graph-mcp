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
  "join_mode": "then",
  "children": [...]
})

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
│    • Choose join_mode (then vs also)                │
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
    "join_mode": "then",
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
  }
}
```

### Parallel Workstreams

```json
{
  "tree": {
    "title": "Sprint 5",
    "join_mode": "also",
    "children": [
      {
        "title": "Backend Track",
        "join_mode": "then",
        "children": [
          {"title": "API endpoints", "agent_tags_all": ["backend"]},
          {"title": "Database migrations", "agent_tags_all": ["database"]}
        ]
      },
      {
        "title": "Frontend Track",
        "join_mode": "then",
        "children": [
          {"title": "Component library", "agent_tags_all": ["frontend"]},
          {"title": "Page integration", "agent_tags_all": ["frontend"]}
        ]
      }
    ]
  }
}
```

### Cross-Branch Dependencies

```
# After creating tree, add cross-branch deps:
block(blocker="api-endpoints-task-id", blocked="page-integration-task-id")
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
unblock(blocker=old_task, blocked=waiting_task)

# Reassign stuck task
release(agent=my_id, task=stuck_task, state="pending")
# Or force-claim for another agent
claim(agent=other_agent, task=stuck_task, force=true)
```

---

## Multi-Agent Patterns

### Hub and Spoke

```
Coordinator creates tasks
     │
     ├──→ Worker A claims matching tasks
     ├──→ Worker B claims matching tasks
     └──→ Worker C claims matching tasks
```

### Hierarchical

```
Lead Coordinator
     │
     ├──→ Sub-Coordinator (Backend)
     │         └──→ Backend Workers
     │
     └──→ Sub-Coordinator (Frontend)
               └──→ Frontend Workers
```

### Peer Review

```json
{
  "title": "Implement Feature",
  "join_mode": "then",
  "children": [
    {"title": "Write code", "agent_tags_all": ["developer"]},
    {"title": "Review code", "agent_tags_all": ["reviewer"]}
  ]
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
- [ ] Dependencies explicit (join_mode + block)
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
