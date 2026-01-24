---
name: task-graph-reporting
description: Reporting and analytics for task-graph-mcp - generate progress reports, analyze metrics, track costs and velocity across projects
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: reporting
  requires: task-graph-basics
---

# Task Graph Reporting

Generate progress reports, analyze metrics, track costs and velocity across task-graph projects.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Quick Start

```
# Connect (reporting doesn't need special tags)
connect(name="reporter", tags=["reporting"])
→ agent_id

# Get project overview
list_tasks(format="markdown")

# Analyze specific task tree
get(task=root_task_id, children=true, format="markdown")

# Check agent activity
list_agents(format="markdown")
```

---

## Report Types

### 1. Status Report

Quick overview of project state:

```
┌─────────────────────────────────────────┐
│ STATUS REPORT                           │
├─────────────────────────────────────────┤
│ Query: list_tasks(format="markdown")    │
│                                         │
│ Metrics to extract:                     │
│ • Total tasks by status                 │
│ • Blocked tasks and blockers            │
│ • Active agents and their tasks         │
│ • Ready tasks (available work)          │
└─────────────────────────────────────────┘
```

### 2. Progress Report

Track completion over time:

```
┌─────────────────────────────────────────┐
│ PROGRESS REPORT                         │
├─────────────────────────────────────────┤
│ Queries:                                │
│ • list_tasks(status="completed")        │
│ • list_tasks(status="in_progress")      │
│ • list_tasks(status="pending")          │
│                                         │
│ Calculate:                              │
│ • Completion rate (completed/total)     │
│ • Points completed vs remaining         │
│ • Time actual vs estimated              │
└─────────────────────────────────────────┘
```

### 3. Cost Report

Analyze resource consumption:

```
┌─────────────────────────────────────────┐
│ COST REPORT                             │
├─────────────────────────────────────────┤
│ Query: get(task=root, children=true)    │
│                                         │
│ Aggregate across tasks:                 │
│ • tokens_in, tokens_out, tokens_cached  │
│ • tokens_thinking, tokens_image/audio   │
│ • cost_usd total and per-task           │
│ • user_metrics custom fields            │
└─────────────────────────────────────────┘
```

### 4. Velocity Report

Measure team throughput:

```
┌─────────────────────────────────────────┐
│ VELOCITY REPORT                         │
├─────────────────────────────────────────┤
│ Queries:                                │
│ • list_tasks(status="completed")        │
│ • get_state_history(task=task_id)       │
│                                         │
│ Calculate:                              │
│ • Points completed per time period      │
│ • Average time per point                │
│ • Agent productivity comparison         │
└─────────────────────────────────────────┘
```

### 5. Agent Report

Analyze agent activity:

```
┌─────────────────────────────────────────┐
│ AGENT REPORT                            │
├─────────────────────────────────────────┤
│ Query: list_agents(format="markdown")   │
│                                         │
│ Per agent:                              │
│ • Current claims                        │
│ • Tasks completed                       │
│ • Time since last heartbeat             │
│ • Tags (capabilities)                   │
└─────────────────────────────────────────┘
```

---

## Metrics Reference

### Task Metrics

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Current state |
| `priority` | string | low/medium/high/critical |
| `points` | int | Story points estimate |
| `time_estimate_ms` | int | Estimated duration |
| `time_actual_ms` | int | Actual duration (auto-tracked) |
| `started_at` | timestamp | When work began |
| `completed_at` | timestamp | When finished |

### Cost Metrics

| Field | Type | Description |
|-------|------|-------------|
| `tokens_in` | int | Input tokens |
| `tokens_out` | int | Output tokens |
| `tokens_cached` | int | Cache hit tokens |
| `tokens_thinking` | int | Reasoning tokens |
| `tokens_image` | int | Image tokens |
| `tokens_audio` | int | Audio tokens |
| `cost_usd` | float | Total USD cost |
| `user_metrics` | JSON | Custom metrics |

### Agent Metrics

| Field | Type | Description |
|-------|------|-------------|
| `registered_at` | timestamp | When connected |
| `last_heartbeat` | timestamp | Last activity |
| `tags` | array | Capabilities |
| `max_claims` | int | Claim limit |

---

## Query Patterns

### By Status

```
# All completed
list_tasks(status="completed")

# Multiple statuses
list_tasks(status=["pending", "in_progress"])

# Only ready (unclaimed, unblocked)
list_tasks(ready=true)

# Only blocked
list_tasks(blocked=true)
```

### By Hierarchy

```
# Root tasks only
list_tasks(parent="null")

# Children of specific task
list_tasks(parent=task_id)

# Full tree
get(task=root_id, children=true)
```

### By Owner

```
# Specific agent's tasks
list_tasks(owner=agent_id)

# Unclaimed only
list_tasks(owner="null", status="pending")
```

### Time-Based (via state history)

```
# Get state transitions for a task
get_state_history(task=task_id)

# Returns:
# - Each state entered and exited
# - Duration in each state
# - Agent who made transitions
```

---

## Report Templates

### Executive Summary

```markdown
# Project Status: {project_name}
Generated: {timestamp}

## Overview
- **Total Tasks:** {total}
- **Completed:** {completed} ({percent}%)
- **In Progress:** {in_progress}
- **Blocked:** {blocked}

## Velocity
- **Points Completed:** {points_done} / {points_total}
- **Avg Time per Point:** {avg_time}

## Cost
- **Total Cost:** ${total_cost}
- **Cost per Point:** ${cost_per_point}

## Active Agents
| Agent | Tasks | Last Active |
|-------|-------|-------------|
{agent_rows}

## Blockers
{blocked_tasks_list}
```

### Burndown Data

```
# Query completed tasks over time
# Plot: remaining points vs time

Day 1: {total_points}
Day 2: {total_points - completed_day_2}
Day 3: {total_points - completed_day_3}
...
```

### Cost Breakdown

```markdown
# Cost Report: {project_name}

## By Task
| Task | Tokens In | Tokens Out | Cost |
|------|-----------|------------|------|
{task_rows}

## By Agent
| Agent | Tasks Done | Total Cost |
|-------|------------|------------|
{agent_rows}

## Totals
- **Total Tokens:** {sum_tokens}
- **Total Cost:** ${sum_cost}
```

---

## Analysis Patterns

### Bottleneck Detection

```
1. list_tasks(blocked=true)
2. For each blocked task, identify blocker
3. Group by blocker → find most-blocking tasks
4. Priority = blocked_count × blocked_priority
```

### Estimation Accuracy

```
1. list_tasks(status="completed")
2. For each: accuracy = time_actual / time_estimate
3. Calculate mean, median, std deviation
4. Flag tasks with accuracy < 0.5 or > 2.0
```

### Agent Utilization

```
1. list_agents()
2. For each agent:
   - Current claims / max_claims = utilization
   - Time since last_heartbeat = idle_time
3. Flag: utilization 0 or idle_time > threshold
```

---

## Generating Reports

### Manual Report

```
# 1. Connect
connect(name="reporter") → agent_id

# 2. Gather data
tasks = list_tasks(format="markdown")
agents = list_agents(format="markdown")

# 3. For cost data, traverse tree
root = get(task=root_id, children=true)

# 4. Aggregate and format
# (Calculate totals, percentages, etc.)
```

### Automated Report (via attachment)

```
# Store report as attachment on root task
attach(
  task=root_id,
  name="weekly-report",
  content=report_markdown,
  mime="text/markdown"
)
```

---

## Custom Metrics

Use `user_metrics` for project-specific tracking:

```
# Worker logs custom metric
log_cost(
  agent=agent_id,
  task=task_id,
  user_metrics={
    "lines_of_code": 150,
    "test_coverage": 0.85,
    "complexity_score": 12
  }
)

# Reporter aggregates
tasks = get(task=root, children=true)
# Sum/average user_metrics across tasks
```

---

## Best Practices

### Reporting Frequency

| Report Type | Frequency |
|-------------|-----------|
| Status | On demand, start of meetings |
| Progress | Daily or per-sprint |
| Cost | Weekly or per-milestone |
| Velocity | Per sprint/iteration |
| Agent | When debugging issues |

### Data Hygiene

- Ensure workers log costs consistently
- Verify time tracking is enabled (timed states)
- Check for orphaned tasks (no parent, not root)
- Validate estimates exist for velocity calcs

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference |
| `task-graph-coordinator` | Understand task structure |
| `task-graph-repair` | Fix data issues before reporting |
