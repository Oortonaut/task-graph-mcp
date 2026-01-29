# Task Graph MCP - Experiment Metrics

> **Version:** 1.0  
> **Last Updated:** 2026-01-28  
> **Status:** Design Document

This document defines the metrics to capture for multi-agent experiment analysis. These metrics enable evaluation of agent coordination efficiency, resource utilization, and workflow optimization.

---

## Table of Contents

- [Overview](#overview)
- [Time Metrics](#time-metrics)
  - [Wall-Clock Time](#wall-clock-time)
  - [Time Blocked vs Working](#time-blocked-vs-working)
- [Token Metrics](#token-metrics)
- [Task Distribution Metrics](#task-distribution-metrics)
- [Quality Metrics](#quality-metrics)
  - [Rework Rate](#rework-rate)
- [Throughput Metrics](#throughput-metrics)
- [Coordination Overhead](#coordination-overhead)
- [Data Collection](#data-collection)
- [Visualization Approaches](#visualization-approaches)
- [Analysis Use Cases](#analysis-use-cases)

---

## Overview

Metrics fall into six categories:

| Category | Purpose |
|----------|---------|
| **Time** | Measure duration and efficiency of work |
| **Tokens** | Track LLM resource consumption |
| **Distribution** | Analyze workload balance across agents |
| **Quality** | Assess rework and error rates |
| **Throughput** | Measure task completion velocity |
| **Coordination** | Quantify multi-agent overhead |

---

## Time Metrics

### Wall-Clock Time

Wall-clock time measures elapsed real-world time at various granularities.

#### Total Experiment Time

**Definition:** Elapsed time from first task creation to last task completion.

**Data Collection:**
```sql
SELECT 
  MIN(created_at) AS experiment_start,
  MAX(completed_at) AS experiment_end,
  (MAX(completed_at) - MIN(created_at)) AS total_duration_ms
FROM tasks
WHERE status = 'completed';
```

**Use Cases:**
- Compare overall experiment duration across different configurations
- Establish baseline for single-agent vs multi-agent comparisons
- Calculate experiment cost (duration * agent count * rate)

---

#### Per-Task Time

**Definition:** Time spent actively working on each task (from `started_at` to `completed_at` or accumulated in `time_actual_ms`).

**Data Collection:**
- **Estimated:** `time_estimate_ms` (set at task creation)
- **Actual:** `time_actual_ms` (accumulated automatically from timed states)
- **Timestamps:** `started_at`, `completed_at`, `claimed_at`

```sql
SELECT 
  id,
  title,
  time_estimate_ms,
  time_actual_ms,
  (completed_at - started_at) AS elapsed_ms,
  CASE 
    WHEN time_estimate_ms > 0 
    THEN (time_actual_ms * 100.0 / time_estimate_ms)
    ELSE NULL 
  END AS estimate_accuracy_pct
FROM tasks
WHERE status = 'completed';
```

**Use Cases:**
- Identify tasks that exceed estimates (planning improvement)
- Find task types that are consistently fast/slow
- Train estimation models based on task characteristics

---

#### Per-Phase Time

**Definition:** Aggregate time spent in each workflow phase across all tasks.

**Data Collection:**
Phase transitions are recorded in `task_state_sequence` with the `event` field indicating state changes. To track phases, use task tags or a dedicated phase field.

```sql
-- Using tags to identify phases
SELECT 
  json_extract(tags, '$[0]') AS phase,
  COUNT(*) AS task_count,
  SUM(time_actual_ms) AS total_time_ms,
  AVG(time_actual_ms) AS avg_time_ms
FROM tasks
WHERE status = 'completed'
GROUP BY json_extract(tags, '$[0]');
```

**Use Cases:**
- Identify bottleneck phases in the workflow
- Balance agent allocation across phases
- Optimize phase-specific tooling or prompts

---

### Time Blocked vs Working

**Definition:** Ratio of time tasks spend waiting (blocked by dependencies or unclaimed) versus actively being worked on.

**Data Collection:**
From `task_state_sequence`, calculate time in each state:

```sql
-- Time in each state per task
SELECT 
  task_id,
  event AS state,
  SUM(COALESCE(end_timestamp, strftime('%s','now')*1000) - timestamp) AS duration_ms
FROM task_state_sequence
GROUP BY task_id, event;

-- Aggregate blocked vs working
SELECT 
  CASE 
    WHEN event IN ('pending', 'assigned') THEN 'blocked'
    WHEN event = 'working' THEN 'working'
    ELSE 'other'
  END AS category,
  SUM(COALESCE(end_timestamp, strftime('%s','now')*1000) - timestamp) AS total_ms
FROM task_state_sequence
GROUP BY category;
```

**Metrics Derived:**
- **Blocked Time:** Sum of time in `pending` + `assigned` states
- **Working Time:** Sum of time in `working` state
- **Blocking Ratio:** `blocked_time / (blocked_time + working_time)`
- **Queue Wait Time:** Time from task creation to first claim

**Use Cases:**
- Identify dependency bottlenecks (high blocked time)
- Detect under-provisioned agent pools (long queue wait)
- Optimize task ordering to minimize blocking

---

## Token Metrics

### Token Consumption

**Definition:** LLM tokens consumed, categorized by type and attribution.

**Data Collection:**
The `tasks` table tracks tokens at the task level:

| Column | Description |
|--------|-------------|
| `tokens_in` | Input tokens (prompt) |
| `tokens_cached` | Cached/reused tokens |
| `tokens_out` | Output tokens (completion) |
| `tokens_thinking` | Reasoning/chain-of-thought tokens |
| `tokens_image` | Image processing tokens |
| `tokens_audio` | Audio processing tokens |

```sql
-- Total tokens by type
SELECT 
  SUM(tokens_in) AS total_input,
  SUM(tokens_cached) AS total_cached,
  SUM(tokens_out) AS total_output,
  SUM(tokens_thinking) AS total_thinking,
  SUM(tokens_in + tokens_out + tokens_thinking) AS total_billable
FROM tasks;

-- Tokens per agent
SELECT 
  worker_id,
  COUNT(*) AS tasks_completed,
  SUM(tokens_in) AS input_tokens,
  SUM(tokens_out) AS output_tokens,
  SUM(cost_usd) AS total_cost
FROM tasks
WHERE status = 'completed' AND worker_id IS NOT NULL
GROUP BY worker_id;
```

**Metrics Derived:**
- **Cache Hit Rate:** `tokens_cached / (tokens_in + tokens_cached)`
- **Output Ratio:** `tokens_out / tokens_in`
- **Thinking Overhead:** `tokens_thinking / tokens_out`
- **Cost per Task:** `cost_usd / task_count`
- **Cost per Token:** `cost_usd / total_tokens`

**Use Cases:**
- Compare prompt efficiency across different agent configurations
- Identify tasks with unexpectedly high token usage
- Optimize caching strategies (higher cache hit = lower cost)
- Budget forecasting for large experiments

---

### Per-Agent Token Analysis

**Definition:** Token usage attributed to individual agents to identify efficiency differences.

**Data Collection:**
```sql
SELECT 
  worker_id,
  COUNT(*) AS task_count,
  SUM(tokens_in) AS tokens_in,
  SUM(tokens_out) AS tokens_out,
  AVG(tokens_in) AS avg_input_per_task,
  AVG(tokens_out) AS avg_output_per_task,
  SUM(cost_usd) AS total_cost,
  AVG(cost_usd) AS avg_cost_per_task
FROM tasks
WHERE status = 'completed'
GROUP BY worker_id
ORDER BY total_cost DESC;
```

**Use Cases:**
- Identify verbose vs concise agents
- Detect agents that may be stuck in loops (high token usage, low completion)
- Balance workload to optimize total cost

---

## Task Distribution Metrics

### Tasks per Agent

**Definition:** Number of tasks claimed, completed, and failed by each agent.

**Data Collection:**
```sql
SELECT 
  worker_id,
  COUNT(*) AS total_claimed,
  SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed,
  SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed,
  SUM(CASE WHEN status = 'working' THEN 1 ELSE 0 END) AS in_progress,
  AVG(time_actual_ms) AS avg_time_per_task
FROM tasks
WHERE worker_id IS NOT NULL
GROUP BY worker_id;
```

**Metrics Derived:**
- **Completion Rate:** `completed / total_claimed`
- **Failure Rate:** `failed / total_claimed`
- **Load Balance Score:** Standard deviation of task counts across agents
- **Gini Coefficient:** Inequality measure for task distribution

**Use Cases:**
- Detect overloaded or underutilized agents
- Identify agents with high failure rates (may need different task types)
- Tune claiming strategies for better balance

---

### Task Type Distribution

**Definition:** Distribution of task types/phases across agents.

**Data Collection:**
```sql
-- Using tags as task type indicator
SELECT 
  worker_id,
  json_extract(tags, '$[0]') AS task_type,
  COUNT(*) AS count
FROM tasks
WHERE worker_id IS NOT NULL
GROUP BY worker_id, json_extract(tags, '$[0]');
```

**Use Cases:**
- Verify agents are claiming tasks matching their capabilities
- Identify specialization patterns
- Optimize `needed_tags`/`wanted_tags` requirements

---

## Quality Metrics

### Rework Rate

**Definition:** Percentage of tasks that were reopened after being marked complete, indicating quality issues.

**Data Collection:**
Track state transitions in `task_state_sequence`:

```sql
-- Tasks with multiple working periods
SELECT 
  task_id,
  COUNT(*) AS working_periods
FROM task_state_sequence
WHERE event = 'working'
GROUP BY task_id
HAVING COUNT(*) > 1;

-- Rework rate
WITH rework AS (
  SELECT 
    task_id,
    COUNT(*) AS working_periods
  FROM task_state_sequence
  WHERE event = 'working'
  GROUP BY task_id
)
SELECT 
  COUNT(CASE WHEN working_periods > 1 THEN 1 END) AS reworked_tasks,
  COUNT(*) AS total_tasks,
  (COUNT(CASE WHEN working_periods > 1 THEN 1 END) * 100.0 / COUNT(*)) AS rework_rate_pct
FROM rework;
```

**Metrics Derived:**
- **Rework Rate:** Tasks with >1 working period / total tasks
- **Rework Cycles:** Average number of working periods for reworked tasks
- **Rework Time:** Additional time spent on reworked tasks
- **First-Pass Success Rate:** Tasks completed in single working period

**Use Cases:**
- Identify systemic quality issues in task definitions
- Compare agent accuracy (high rework = potential skill mismatch)
- Measure impact of review processes

---

### Failed Task Analysis

**Definition:** Analysis of tasks that ended in `failed` state.

**Data Collection:**
```sql
SELECT 
  t.id,
  t.title,
  t.worker_id,
  t.time_actual_ms,
  tss.reason AS failure_reason
FROM tasks t
LEFT JOIN task_state_sequence tss ON t.id = tss.task_id AND tss.event = 'failed'
WHERE t.status = 'failed';
```

**Use Cases:**
- Categorize failure reasons
- Identify patterns (certain task types, agents, or times)
- Improve task definitions or agent capabilities

---

## Throughput Metrics

### Tasks per Hour

**Definition:** Rate of task completion over time.

**Data Collection:**
```sql
-- Hourly throughput
SELECT 
  datetime(completed_at/1000, 'unixepoch', 'start of hour') AS hour,
  COUNT(*) AS tasks_completed
FROM tasks
WHERE status = 'completed'
GROUP BY hour
ORDER BY hour;

-- Overall throughput
SELECT 
  COUNT(*) AS total_completed,
  (MAX(completed_at) - MIN(started_at)) / 3600000.0 AS duration_hours,
  COUNT(*) / ((MAX(completed_at) - MIN(started_at)) / 3600000.0) AS tasks_per_hour
FROM tasks
WHERE status = 'completed';
```

**Metrics Derived:**
- **Instantaneous Throughput:** Tasks completed in rolling window
- **Sustained Throughput:** Tasks/hour over full experiment
- **Peak Throughput:** Maximum hourly completion rate
- **Throughput per Agent:** tasks_per_hour / agent_count

**Use Cases:**
- Measure scaling efficiency (throughput vs agent count)
- Identify throughput degradation over time
- Compare workflow configurations

---

### Points per Hour

**Definition:** Story points or complexity units completed per hour (for weighted throughput).

**Data Collection:**
```sql
SELECT 
  SUM(points) AS total_points,
  (MAX(completed_at) - MIN(started_at)) / 3600000.0 AS duration_hours,
  SUM(points) / ((MAX(completed_at) - MIN(started_at)) / 3600000.0) AS points_per_hour
FROM tasks
WHERE status = 'completed' AND points IS NOT NULL;
```

**Use Cases:**
- Account for task complexity in throughput calculations
- Compare experiments with different task mixes
- Velocity tracking for sprint planning

---

## Coordination Overhead

**Definition:** Time and resources spent on coordination rather than direct task work.

### Components

| Component | Description | Measurement |
|-----------|-------------|-------------|
| **Claim Contention** | Time spent retrying failed claims | Count of claim attempts vs successes |
| **File Lock Waiting** | Time blocked waiting for file access | Time between mark attempt and success |
| **Dependency Blocking** | Time waiting for blockers to complete | Time in `pending` state due to deps |
| **Communication** | Token usage for coordination messages | Tokens in `thinking` calls |
| **Heartbeat Traffic** | Resources for liveness checks | Count of heartbeat updates |

### Data Collection

```sql
-- Claim contention (from state transitions)
SELECT 
  agent_id,
  COUNT(*) AS claim_attempts,
  SUM(CASE WHEN event = 'working' THEN 1 ELSE 0 END) AS successful_claims
FROM task_state_sequence
WHERE event IN ('assigned', 'working')
GROUP BY agent_id;

-- File coordination overhead
SELECT 
  COUNT(*) AS total_marks,
  COUNT(CASE WHEN event = 'released' THEN 1 END) AS releases,
  AVG(CASE 
    WHEN event = 'released' AND claim_id IS NOT NULL 
    THEN timestamp - (SELECT timestamp FROM claim_sequence c2 WHERE c2.id = claim_sequence.claim_id)
    ELSE NULL 
  END) AS avg_hold_time_ms
FROM claim_sequence;

-- Dependency blocking time
WITH blocked_time AS (
  SELECT 
    task_id,
    SUM(COALESCE(end_timestamp, strftime('%s','now')*1000) - timestamp) AS pending_ms
  FROM task_state_sequence
  WHERE event = 'pending'
  GROUP BY task_id
)
SELECT 
  AVG(pending_ms) AS avg_blocked_ms,
  SUM(pending_ms) AS total_blocked_ms
FROM blocked_time;
```

### Overhead Ratio

**Definition:** Proportion of total time spent on coordination vs productive work.

```sql
WITH times AS (
  SELECT 
    SUM(CASE WHEN event = 'working' THEN 
      COALESCE(end_timestamp, strftime('%s','now')*1000) - timestamp 
    ELSE 0 END) AS working_ms,
    SUM(CASE WHEN event IN ('pending', 'assigned') THEN 
      COALESCE(end_timestamp, strftime('%s','now')*1000) - timestamp 
    ELSE 0 END) AS waiting_ms
  FROM task_state_sequence
)
SELECT 
  working_ms,
  waiting_ms,
  (waiting_ms * 100.0 / (working_ms + waiting_ms)) AS overhead_pct
FROM times;
```

**Use Cases:**
- Identify coordination bottlenecks
- Compare pull vs push task assignment efficiency
- Optimize dependency structures to reduce blocking

---

## Data Collection

### Automatic Collection

The following data is collected automatically by the task-graph system:

| Data | Storage | Trigger |
|------|---------|---------|
| Task timestamps | `tasks` table | CRUD operations |
| State transitions | `task_state_sequence` | Status changes |
| Time actual | `tasks.time_actual_ms` | Exit from timed state |
| File marks | `claim_sequence` | mark/unmark operations |
| Worker heartbeats | `workers.last_heartbeat` | Any tool call |

### Manual Collection

Some metrics require explicit logging:

| Data | How to Collect |
|------|----------------|
| Token counts | Call `log_metrics` with token values |
| Cost USD | Call `log_metrics` with cost |
| Custom metrics | Use `metrics` array (8 integer slots) |

```javascript
// Example: Log tokens after task completion
log_metrics({
  agent: "worker-1",
  task: "task-123",
  values: [1500, 500, 0, 0, 0, 0, 0, 0],  // [in, out, cached, thinking, ...]
  cost_usd: 0.0023
});
```

### Export for Analysis

Use the export functionality to extract data for external analysis:

```bash
# Export full database snapshot
task-graph export --format json --output experiment-data.json

# Export specific tables
task-graph query --sql "SELECT * FROM tasks" --format csv > tasks.csv
task-graph query --sql "SELECT * FROM task_state_sequence" --format csv > transitions.csv
```

---

## Visualization Approaches

### Time Metrics

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Total duration | Single value card | Any dashboard |
| Per-task time | Histogram, box plot | Matplotlib, Plotly |
| Phase time | Stacked bar chart | D3.js, Tableau |
| Blocked vs working | Pie chart, timeline | Gantt chart tools |

### Token Metrics

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Token breakdown | Stacked area chart | Plotly |
| Per-agent tokens | Grouped bar chart | Matplotlib |
| Cost over time | Line chart | Grafana |
| Cache hit rate | Gauge | Dashboard widgets |

### Distribution Metrics

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Tasks per agent | Bar chart | Any |
| Load balance | Heatmap | Seaborn |
| Task flow | Sankey diagram | D3.js |
| Agent activity | Timeline/Gantt | Custom |

### Quality Metrics

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Rework rate | Trend line | Time series |
| Failure analysis | Treemap by category | D3.js |
| State transitions | State diagram | Custom |

### Throughput Metrics

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Tasks/hour | Line chart | Any |
| Cumulative completion | Area chart | Plotly |
| Scaling efficiency | Tasks/agent scatter | Matplotlib |

### Coordination Overhead

| Metric | Visualization | Tool Suggestions |
|--------|---------------|------------------|
| Overhead ratio | Pie/donut chart | Any |
| File contention | Heatmap by file | Custom |
| Dependency graph | Network diagram | D3.js, vis.js |
| Blocking cascade | Critical path diagram | MS Project style |

---

## Analysis Use Cases

### Experiment Comparison

Compare different configurations (agent count, task structure, coordination model):

```sql
-- Assuming experiment_id stored in task tags
SELECT 
  json_extract(tags, '$.experiment') AS experiment,
  COUNT(*) AS tasks,
  SUM(time_actual_ms) / 1000.0 AS total_seconds,
  SUM(cost_usd) AS total_cost,
  COUNT(*) / (SUM(time_actual_ms) / 3600000.0) AS tasks_per_hour
FROM tasks
WHERE status = 'completed'
GROUP BY json_extract(tags, '$.experiment');
```

### Bottleneck Identification

Find tasks or phases that slow down the overall system:

```sql
-- Tasks with longest blocked time
SELECT 
  t.id,
  t.title,
  SUM(tss.end_timestamp - tss.timestamp) AS blocked_ms
FROM tasks t
JOIN task_state_sequence tss ON t.id = tss.task_id
WHERE tss.event = 'pending' AND tss.end_timestamp IS NOT NULL
GROUP BY t.id
ORDER BY blocked_ms DESC
LIMIT 10;
```

### Agent Performance Ranking

Evaluate agent efficiency across multiple dimensions:

```sql
SELECT 
  worker_id,
  COUNT(*) AS tasks_completed,
  AVG(time_actual_ms) AS avg_time_ms,
  SUM(cost_usd) / COUNT(*) AS cost_per_task,
  SUM(CASE WHEN points IS NOT NULL THEN points ELSE 1 END) / 
    (SUM(time_actual_ms) / 3600000.0) AS velocity
FROM tasks
WHERE status = 'completed' AND worker_id IS NOT NULL
GROUP BY worker_id
ORDER BY velocity DESC;
```

### Cost Optimization

Identify opportunities to reduce experiment cost:

```sql
-- High-cost tasks
SELECT 
  id,
  title,
  cost_usd,
  tokens_in,
  tokens_out,
  (tokens_in + tokens_out) / cost_usd AS tokens_per_dollar
FROM tasks
WHERE cost_usd > 0
ORDER BY cost_usd DESC
LIMIT 20;

-- Cost by phase
SELECT 
  json_extract(tags, '$[0]') AS phase,
  SUM(cost_usd) AS total_cost,
  AVG(cost_usd) AS avg_cost,
  COUNT(*) AS task_count
FROM tasks
WHERE status = 'completed'
GROUP BY phase
ORDER BY total_cost DESC;
```

---

## Appendix: Metric Summary Table

| Metric | Formula | Unit | Target Direction |
|--------|---------|------|------------------|
| Total Duration | `max(completed_at) - min(created_at)` | ms | Lower |
| Avg Task Time | `mean(time_actual_ms)` | ms | Lower |
| Blocking Ratio | `blocked_time / total_time` | % | Lower |
| Token Efficiency | `output_tokens / input_tokens` | ratio | Higher |
| Cache Hit Rate | `cached / (input + cached)` | % | Higher |
| Task Balance | `stddev(tasks_per_agent)` | count | Lower |
| Rework Rate | `reworked_tasks / total_tasks` | % | Lower |
| Throughput | `completed_tasks / duration_hours` | tasks/hr | Higher |
| Coordination Overhead | `waiting_time / total_time` | % | Lower |
| Cost per Task | `total_cost / task_count` | USD | Lower |

---

## Document History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-01-28 | Initial metrics definition |

---

*This document defines the experiment metrics framework. Implementation details for automated collection and reporting will be added as the experiment system is built.*
