# Experiment Framework Design

> **Version:** 1.0
> **Date:** 2026-01-31
> **Status:** Design Proposal
> **Task:** 019c024f-f126-7cb2-8434-b62a0313436e

---

## 1. Goals

The experiment framework enables systematic comparison of multi-agent coordination patterns by providing repeatable experiment execution, automated metric collection, and structured result reporting.

### What an Experiment Looks Like

An experiment answers a question like:

> "Does running task decomposition with the swarm workflow (pure pull) vs the hierarchical workflow (push) produce faster completion times, lower token cost, or better task distribution for the same workload?"

Concretely, an experiment:

1. **Defines a hypothesis** about a coordination pattern (e.g., "hybrid push/pull achieves higher throughput than pure push").
2. **Loads a task template** (e.g., `browser-parallel.json`) into a fresh database to create an identical workload across runs.
3. **Configures agents** with a specific workflow, role assignments, and agent count.
4. **Executes the workload** while the task-graph system automatically tracks state transitions, time, and cost.
5. **Collects metrics** from the database (wall-clock time, tokens, cost, blocking ratio, rework rate, etc.).
6. **Compares results** across experiment variants (e.g., swarm vs hierarchical vs hybrid) using the same template and agent count.

### Key Design Principles

- **Grounded in existing infrastructure.** The task-graph already tracks state transitions (`task_sequence`), metrics (`log_metrics`/`get_metrics`), templates (`experiments/templates/`), workflows, and export/import. The experiment framework orchestrates these existing capabilities rather than replacing them.
- **Declarative experiment definitions.** Experiments are YAML files (building on the existing `experiments/*.yaml` pattern) that declare what to run, not how to run it.
- **Isolation via database snapshotting.** Each experiment run uses a fresh database (or a `Replace`-mode import) so runs do not contaminate each other. The existing `ImportMode::Replace` and `ImportMode::Fresh` modes support this.
- **Automated metric extraction.** Metrics are computed from SQL queries against the task-graph database, reusing the `query` tool and `task_sequence` table that already exist.

---

## 2. Components

### 2.1 Experiment Definition Schema

Each experiment is a YAML file under `experiments/`. Three experiment definitions already exist in the codebase (`pure-pull.yaml`, `push-experiment.yaml`, `experiment-hybrid.yaml`). The framework formalizes and extends this existing pattern.

```yaml
# experiments/example-experiment.yaml

experiment:
  id: "swarm-vs-hierarchical-001"
  name: "Swarm vs Hierarchical"
  version: "1.0.0"
  created: "2026-01-31"
  hypothesis: >
    Pure pull (swarm) coordination achieves higher throughput than
    hierarchical (push) for independent tasks, but hierarchical
    produces lower rework rates due to lead oversight.

# What task set to use
template:
  source: "experiments/templates/browser-parallel.json"
  instantiate_options:
    reset_status: true
    extra_tags: ["experiment:swarm-vs-hierarchical-001"]

# Variants to compare (each is a separate run)
variants:
  - name: "swarm-4"
    workflow: swarm
    agents:
      count: 4
      config:
        tags: [worker, implementer, code]
        max_claims: 1

  - name: "hierarchical-4"
    workflow: hierarchical
    agents:
      count: 5  # 1 lead + 4 workers
      definitions:
        - id: lead
          tags: [lead, coordinator]
          workflow: hierarchical
        - id: "worker-{n}"
          count: 4
          tags: [worker, implementer, code]
          workflow: hierarchical

# Metrics to collect (references docs/METRICS.md categories)
metrics:
  primary:
    - wall_clock_duration_ms
    - total_cost_usd
    - tasks_per_hour
    - completion_rate_pct
  coordination:
    - blocking_ratio_pct
    - avg_queue_wait_ms
    - rework_rate_pct
  token_efficiency:
    - total_billable_tokens
    - tokens_per_completed_task
    - cache_hit_rate_pct

# Success criteria
success_criteria:
  minimum:
    completion_rate_pct: ">= 80"
  target:
    tasks_per_hour: "> baseline"

# Execution settings
execution:
  timeout_seconds: 7200
  poll_interval_seconds: 30
  output_dir: "experiments/results/{experiment.id}/{variant.name}"
```

**Relationship to existing files:** The three experiment YAMLs already in the repository (`pure-pull.yaml`, `push-experiment.yaml`, `experiment-hybrid.yaml`) follow a similar but ad-hoc structure. This schema formalizes the common elements across them.

### 2.2 Experiment Runner

A CLI tool (or script) that automates the experiment lifecycle:

```
experiment run <experiment.yaml> [--variant <name>] [--output <dir>]
experiment compare <result-dir-1> <result-dir-2> [--output <dir>]
experiment report <result-dir>
```

#### Run Lifecycle

```
1. SETUP
   |-- Parse experiment YAML
   |-- For each variant:
   |     |-- Create fresh database (ImportMode::Fresh or Replace)
   |     |-- Import template via instantiate_template()
   |     |     (uses existing src/db/template.rs machinery)
   |     |-- Apply extra_tags for experiment tracking
   |     |-- Record experiment metadata as an attachment on root task
   |
2. LAUNCH
   |-- Start task-graph-mcp server (existing binary)
   |-- For each agent definition:
   |     |-- Launch agent process (e.g., `claude --task "..."`)
   |     |-- Agent calls `connect(workflow=..., tags=...)` (existing tool)
   |     |-- Agents work autonomously per their workflow
   |
3. MONITOR
   |-- Poll database via `project_history` or `query` tool
   |-- Track: pending count, working count, completed count
   |-- Detect completion: all tasks in terminal state
   |-- Detect timeout: wall-clock exceeds timeout_seconds
   |-- Detect stale agents: heartbeat tracking (existing)
   |
4. COLLECT
   |-- Export database snapshot via export_tables() (existing)
   |-- Run metric queries against the database
   |-- Compute derived metrics (throughput, ratios, Gini)
   |-- Write results to output directory:
   |     |-- tasks.db          (SQLite database copy)
   |     |-- snapshot.json     (full Snapshot export)
   |     |-- metrics.json      (computed metrics)
   |     |-- summary.md        (human-readable report)
   |     |-- experiment.yaml   (copy of experiment definition)
   |
5. CLEANUP
   |-- Disconnect all agents (existing disconnect tool)
   |-- Stop task-graph server
```

### 2.3 Metric Collection

Metrics are collected from existing data sources in the task-graph database. No new tables or columns are needed.

#### Existing Data Sources

| Source | What It Provides | Location |
|--------|-----------------|----------|
| `tasks` table | Status, cost_usd, metrics[0..7], points, timestamps | `src/db/tasks.rs` |
| `task_sequence` table | State transition history with timestamps | `src/db/state_transitions.rs` |
| `workers` table | Agent registrations, heartbeats, tags | `src/db/agents.rs` |
| `log_metrics` tool | Per-task cost and 8 integer metric slots | `src/tools/tracking.rs` |
| `get_metrics` tool | Aggregated metrics across tasks | `src/tools/tracking.rs` |
| `project_history` tool | Project-wide state transition stats | `src/tools/tracking.rs` |
| `task_history` tool | Per-task time-per-status breakdown | `src/tools/tracking.rs` |
| `export_tables()` | Full database snapshot for archival | `src/db/export.rs` |

#### Metric Queries

The metric collection layer runs SQL queries against the database using the existing `query` tool (read-only SQL). Queries are defined in the experiment YAML or referenced from `docs/METRICS.md`. Key metrics and their queries:

| Metric | Query Source |
|--------|-------------|
| `wall_clock_duration_ms` | `MAX(completed_at) - MIN(started_at) FROM tasks` |
| `total_cost_usd` | `SUM(cost_usd) FROM tasks` |
| `tasks_per_hour` | Derived: `completed_count / (wall_clock_ms / 3600000)` |
| `completion_rate_pct` | `100 * SUM(status='completed') / COUNT(*) FROM tasks` |
| `blocking_ratio_pct` | Ratio of time in pending/assigned vs working in `task_sequence` |
| `rework_rate_pct` | Count of tasks with >1 working period in `task_sequence` |
| `total_billable_tokens` | `SUM(metric_0 + metric_1 + metric_3) FROM tasks` (per METRICS.md convention) |

#### The metrics[0..7] Convention

The `log_metrics` tool provides 8 integer slots per task. The established convention from `docs/METRICS.md` and the existing experiment YAMLs is:

| Slot | Meaning |
|------|---------|
| `metric_0` | Input tokens |
| `metric_1` | Output tokens |
| `metric_2` | Cached tokens |
| `metric_3` | Thinking tokens |
| `metric_4` | Image tokens |
| `metric_5` | Audio tokens |
| `metric_6` | (available) |
| `metric_7` | (available) |

### 2.4 Baseline Comparison

Comparing experiment variants requires:

1. **Identical starting state.** Each variant imports the same template into a fresh database. The `instantiate_template()` function (in `src/db/template.rs`) already supports this with ID remapping and status reset.

2. **Aligned metric collection.** All variants collect the same metric set defined in the experiment YAML.

3. **Comparison report.** A report tool that loads `metrics.json` from each variant's output directory and produces:
   - Side-by-side metric table (variant name as columns)
   - Delta columns (absolute and percentage difference from baseline)
   - Color-coded direction indicators (green for improvements, red for regressions)
   - Statistical significance notes (for experiments with multiple runs per variant)

#### Comparison Output Example

```markdown
## Experiment: Swarm vs Hierarchical

| Metric                    | swarm-4     | hierarchical-4 | Delta     |
|---------------------------|-------------|-----------------|-----------|
| wall_clock_duration_ms    | 1,245,000   | 1,890,000       | -34.1%    |
| total_cost_usd            | $2.34       | $3.67           | -36.2%    |
| tasks_per_hour            | 38.5        | 25.3            | +52.2%    |
| completion_rate_pct       | 95.0%       | 100.0%          | -5.0pp    |
| blocking_ratio_pct        | 8.2%        | 22.1%           | -13.9pp   |
| rework_rate_pct           | 12.0%       | 3.0%            | +9.0pp    |
```

### 2.5 Result Reporting

Each experiment run produces a results directory:

```
experiments/results/swarm-vs-hierarchical-001/
  swarm-4/
    tasks.db              # SQLite database (complete state)
    snapshot.json         # Snapshot export (version-controllable)
    metrics.json          # Computed metrics
    summary.md            # Human-readable run summary
    experiment.yaml       # Experiment definition (frozen copy)
  hierarchical-4/
    tasks.db
    snapshot.json
    metrics.json
    summary.md
    experiment.yaml
  comparison.md           # Cross-variant comparison report
  comparison.json         # Machine-readable comparison data
```

The `snapshot.json` file uses the existing `Snapshot` format from `src/export/mod.rs`, which means results are importable back into any task-graph database for further analysis.

---

## 3. Integration with Existing Infrastructure

### 3.1 Templates

The experiment framework uses templates from `experiments/templates/` and `task-graph/templates/`. Two templates already exist:

- **`browser-parallel.json`**: 22 tasks in a parallel (fan-out/fan-in) structure. Designed for swarm workflow testing with maximum concurrency.
- **`browser-phased.json`**: 40 tasks organized by component, each with explore/design/implement/test subtasks. Designed for relay workflow testing with phase-based handoffs and `needed_tags` for role matching.

Templates are imported via `Database::instantiate_template()` (`src/db/template.rs`) which handles:
- ID remapping (fresh petname IDs per run)
- Status reset to `pending`
- Timestamp reset to current time
- Optional parent task attachment
- Optional extra tags (useful for tagging all tasks with the experiment ID)
- Merge-mode import (no conflicts with existing data)

### 3.2 Workflows

Experiments reference workflows by name via the `connect` tool's `workflow` parameter. The workflow system (`src/config/workflows.rs`) supports:

- **Named workflows** (e.g., `swarm`, `hierarchical`, `relay`, `solo`) loaded from `workflow-*.yaml` files
- **Role definitions** with tag matching, max_claims, and can_assign permissions
- **Phase workflows** defining per-phase state machines
- **Overlays** that modify workflow behavior dynamically (e.g., `git`, `troubleshooting`)
- **Prompts** delivered to agents based on their matched role

Experiments specify which workflow each variant uses. The runner passes this to agents who include it in their `connect()` call.

### 3.3 Metrics Tracking

The existing metrics system requires no changes:

- **`log_metrics` tool** (`src/tools/tracking.rs`): Agents call this to report cost_usd and up to 8 integer metric values per task. Values are aggregated (added to existing).
- **`get_metrics` tool** (`src/tools/tracking.rs`): Retrieves aggregated metrics for one or more tasks.
- **`task_history` tool** (`src/tools/tracking.rs`): Returns per-task state transition history with time-per-status and time-per-agent breakdowns.
- **`project_history` tool** (`src/tools/tracking.rs`): Returns project-wide transition statistics with date/time range filters.
- **`time_actual_ms`** on tasks: Automatically accumulated when tasks exit timed states (e.g., `working`).

### 3.4 Export/Import

The experiment framework uses the existing export/import pipeline:

- **`Database::export_tables()`** (`src/db/export.rs`): Exports all project data tables (tasks, dependencies, attachments, tags, task_sequence) in deterministic order. Excludes ephemeral tables (workers, file_locks).
- **`Database::import_snapshot()`** (`src/db/import.rs`): Imports a `Snapshot` with support for `Fresh`, `Replace`, and `Merge` modes. Handles foreign key ordering and FTS rebuild.
- **`Snapshot` struct** (`src/export/mod.rs`): The portable JSON format for database state. Includes schema_version, export metadata, and all table data.

For experiments, the workflow is:
1. **Setup**: `import_snapshot` with `ImportMode::Fresh` or `ImportMode::Replace` to load the template.
2. **Collect**: `export_tables` after experiment completion to capture the full result state.
3. **Archive**: Save the `Snapshot` JSON alongside the SQLite database for both machine and human consumption.

### 3.5 Agent Connection

Agents connect via the `connect` tool (`src/tools/agents.rs`) which:
- Registers the worker with ID, tags, and workflow name
- Returns workflow configuration, role information, and role-specific prompts
- Supports overlay application for dynamic behavior modification

The experiment runner launches agent processes that call `connect()` with the workflow and tags specified in the experiment variant.

### 3.6 Query Tool

The `query` tool (`src/tools/query.rs`) provides read-only SQL access to the database. The metric collection phase uses this to run the SQL queries defined in the experiment's metric section, extracting computed values from the raw data.

---

## 4. CLI Commands / Tools

### 4.1 New MCP Tools (Future)

These tools would extend the existing tool set in `src/tools/`:

| Tool | Description |
|------|-------------|
| `experiment_run` | Start an experiment from a YAML definition. Sets up database, imports template, returns experiment ID. |
| `experiment_status` | Check experiment progress (pending/working/completed/failed counts, elapsed time, estimated completion). |
| `experiment_metrics` | Compute and return metrics for a completed experiment run. |
| `experiment_compare` | Compare metrics across two or more experiment result directories. |

### 4.2 CLI Script Commands

Before the tools are built into the server, a Python runner script (`scripts/run_experiment.py`) provides the same functionality. The existing experiment YAMLs already reference this script:

```bash
# Run a single experiment variant
python scripts/run_experiment.py \
  --config experiments/pure-pull.yaml \
  --variant pull-4 \
  --output experiments/results/pure-pull/pull-4

# Wait for completion and export
python scripts/run_experiment.py \
  --wait --poll-interval 30 --timeout 7200 \
  --output experiments/results/pure-pull/pull-4

# Compare results across variants
python scripts/compare_experiments.py \
  experiments/results/pure-pull/pull-4/tasks.db \
  experiments/results/push/push-4/tasks.db \
  experiments/results/hybrid/hybrid-4/tasks.db \
  --labels "pull,push,hybrid" \
  --output experiments/results/comparison
```

### 4.3 Dashboard Integration

The existing web dashboard (`src/dashboard/`) includes a metrics page (`templates/metrics.html`). Experiment results could be surfaced there by:
- Adding an experiment selector dropdown
- Loading metrics.json from the results directory
- Rendering comparison charts

This is a future enhancement, not part of the initial framework.

---

## 5. Example Experiment Definition

This is a complete, runnable experiment definition that uses only existing infrastructure:

```yaml
# experiments/decomposition-strategy-001.yaml
#
# Question: Does a coordinator that decomposes tasks into smaller subtasks
# before workers claim them produce better throughput and lower cost than
# letting workers claim coarse tasks and decompose themselves?

experiment:
  id: "decomposition-strategy-001"
  name: "Pre-decomposition vs Self-decomposition"
  version: "1.0.0"
  created: "2026-01-31"
  hypothesis: >
    Pre-decomposing tasks via a coordinator reduces total token cost
    (workers spend less time on planning) but increases wall-clock time
    (coordinator is a serial bottleneck). Self-decomposition by workers
    achieves higher throughput but at higher per-task token cost.

template:
  source: "experiments/templates/browser-parallel.json"
  # browser-parallel has coarse leaf tasks suitable for further decomposition
  instantiate_options:
    reset_status: true
    extra_tags: ["experiment:decomposition-001"]

variants:
  # Variant A: Coordinator pre-decomposes, workers execute atomic tasks
  - name: "pre-decomposed"
    description: >
      Lead decomposes all tasks into fine-grained subtasks using
      create_tree() before workers start. Workers only execute.
    workflow: hierarchical
    agents:
      count: 5
      definitions:
        - id: lead
          tags: [lead, coordinator, designer]
          prompt: >
            You are the lead. Before workers start, decompose every leaf
            task into 2-4 subtasks using create_tree(). Once decomposition
            is complete, workers will pull subtasks. Monitor progress.
        - id: "worker-{n}"
          count: 4
          tags: [worker, implementer, code]
          prompt: >
            Wait for the lead to finish decomposition (subtasks will
            appear with ready=true). Then pull and execute subtasks.

  # Variant B: Workers claim coarse tasks and self-decompose
  - name: "self-decomposed"
    description: >
      No coordinator. Workers claim coarse tasks, decompose into
      subtasks themselves, and execute.
    workflow: swarm
    agents:
      count: 4
      definitions:
        - id: "swarm-{n}"
          count: 4
          tags: [worker, implementer, code]
          prompt: >
            Claim a task. If it is large (>5 points), decompose it into
            subtasks via create_tree(), then work the subtasks. If small,
            execute directly.

  # Variant C: Hybrid -- coordinator decomposes top-level only
  - name: "hybrid-decomposed"
    description: >
      Lead decomposes root into top-level components (push), workers
      decompose their assigned component further (self-decompose within scope).
    workflow: hierarchical
    agents:
      count: 5
      definitions:
        - id: lead
          tags: [lead, coordinator]
          prompt: >
            Decompose the root task into top-level components only.
            Push each component to a worker. Workers handle further
            decomposition within their assigned scope.
        - id: "worker-{n}"
          count: 4
          tags: [worker, implementer, code]
          prompt: >
            Wait for assignment from lead. Once assigned, decompose
            your component into subtasks and execute them.

metrics:
  primary:
    - name: wall_clock_duration_ms
      query: "SELECT MAX(completed_at) - MIN(started_at) FROM tasks WHERE status = 'completed'"
      compare: lower_is_better

    - name: total_cost_usd
      query: "SELECT SUM(cost_usd) FROM tasks WHERE deleted_at IS NULL"
      compare: lower_is_better

    - name: tasks_per_hour
      derived: "completed_count / (wall_clock_duration_ms / 3600000)"
      compare: higher_is_better

    - name: completion_rate_pct
      query: >
        SELECT 100.0 * SUM(CASE WHEN status='completed' THEN 1 ELSE 0 END) / COUNT(*)
        FROM tasks WHERE deleted_at IS NULL
      compare: higher_is_better

  coordination:
    - name: decomposition_time_ms
      description: "Time the lead spends decomposing before workers start"
      query: >
        SELECT MAX(ts.timestamp) - MIN(ts.timestamp)
        FROM task_sequence ts
        JOIN tasks t ON ts.task_id = t.id
        WHERE ts.worker_id IN ('lead') AND ts.status = 'working'
      compare: lower_is_better

    - name: avg_subtask_count
      description: "Average number of subtasks per original task"
      query: >
        SELECT AVG(child_count) FROM (
          SELECT d.from_task_id, COUNT(*) as child_count
          FROM dependencies d WHERE d.dep_type = 'contains'
          GROUP BY d.from_task_id
        )
      compare: neutral

    - name: blocking_ratio_pct
      description: "Fraction of tracked time in pending/assigned vs working"
      query: >
        SELECT 100.0 * SUM(CASE WHEN status IN ('pending','assigned') THEN
          COALESCE(end_timestamp, CAST(strftime('%s','now') AS INTEGER)*1000) - timestamp
        ELSE 0 END) / SUM(
          COALESCE(end_timestamp, CAST(strftime('%s','now') AS INTEGER)*1000) - timestamp
        ) FROM task_sequence
      compare: lower_is_better

  quality:
    - name: rework_rate_pct
      query: >
        SELECT 100.0 * COUNT(CASE WHEN cnt > 1 THEN 1 END) / COUNT(*)
        FROM (SELECT task_id, COUNT(*) as cnt FROM task_sequence
              WHERE status = 'working' GROUP BY task_id)
      compare: lower_is_better

  efficiency:
    - name: tokens_per_completed_task
      query: >
        SELECT AVG(metric_0 + metric_1 + metric_3)
        FROM tasks WHERE status = 'completed' AND deleted_at IS NULL
      compare: lower_is_better

    - name: cost_per_point
      query: >
        SELECT SUM(cost_usd) / NULLIF(SUM(points), 0)
        FROM tasks WHERE status = 'completed' AND deleted_at IS NULL
      compare: lower_is_better

comparison:
  baseline: "self-decomposed"
  hypotheses:
    throughput: >
      Pre-decomposed should have lower wall_clock if decomposition is fast,
      but the serial decomposition phase may dominate. Hybrid should balance.
    cost: >
      Pre-decomposed should have lower tokens_per_completed_task (workers
      do less planning). Self-decomposed should have higher per-task cost
      but may have lower total cost if throughput is significantly better.
    quality: >
      Pre-decomposed should have lower rework_rate (lead catches issues
      during decomposition). Self-decomposed may have more rework as
      workers discover issues late.

success_criteria:
  minimum:
    completion_rate_pct: ">= 80"
    wall_clock_duration_ms: "< 7200000"  # 2 hours
  target:
    tasks_per_hour: "> 20"
    rework_rate_pct: "< 10"

execution:
  timeout_seconds: 7200
  poll_interval_seconds: 30
  output_dir: "experiments/results/decomposition-001"
```

---

## 6. Implementation Roadmap

This is a design document. Implementation should proceed in phases:

### Phase 1: Runner Script (Python)
- Implement `scripts/run_experiment.py` that reads experiment YAML
- Template import via CLI (`task-graph import --file <template> --mode fresh`)
- Agent launch via subprocess
- Polling loop for completion detection
- Metric query execution and JSON export
- Comparison report generation

### Phase 2: MCP Tool Integration
- Add `experiment_run` tool to `src/tools/` that orchestrates setup and monitoring
- Add `experiment_metrics` tool for in-server metric computation
- Add `experiment_compare` tool for cross-run comparison

### Phase 3: Dashboard and Visualization
- Extend the web dashboard with experiment result viewing
- Add chart rendering for metric comparisons
- Timeline visualization for agent activity

### Phase 4: Statistical Rigor
- Support multiple runs per variant (N=3+) for statistical significance
- Confidence interval computation
- Automated hypothesis testing (two-sample t-test on key metrics)

---

## 7. Open Questions

1. **Agent launch mechanism.** The framework needs to start multiple agent processes. Should this use `claude --task` subprocesses, a dedicated agent launcher, or assume agents are started externally?

2. **Database isolation.** Should each variant get its own SQLite file (simplest, most isolated) or its own server instance? Separate files are simpler for archival and comparison.

3. **Template parameterization.** Should templates support variable substitution (e.g., `{agent_count}` in task descriptions) for experiments that vary task content across runs?

4. **Live monitoring.** The existing dashboard shows live task state. Should the experiment framework integrate with it, or is post-hoc analysis sufficient for the initial version?

5. **Metric slot convention.** The `metrics[0..7]` slots have an informal convention (documented in `docs/METRICS.md`). Should the experiment framework enforce this convention or allow per-experiment custom slot assignments?

---

## Appendix: Existing Infrastructure Summary

| Component | Location | Role in Experiments |
|-----------|----------|-------------------|
| Task templates | `experiments/templates/*.json` | Define workload for experiment runs |
| Experiment definitions | `experiments/*.yaml` | Declare experiment parameters (3 exist) |
| Template instantiation | `src/db/template.rs` | Import templates with ID remapping and status reset |
| State transition tracking | `src/db/state_transitions.rs` | Automatic time tracking per state per task |
| Metrics logging | `src/tools/tracking.rs` (`log_metrics`) | Per-task cost and token tracking (8 slots) |
| Metrics retrieval | `src/tools/tracking.rs` (`get_metrics`) | Aggregated metric queries |
| Project history | `src/tools/tracking.rs` (`project_history`) | Project-wide transition statistics |
| Database export | `src/db/export.rs` | Snapshot creation for archival |
| Database import | `src/db/import.rs` | Fresh/Replace/Merge import modes |
| Snapshot format | `src/export/mod.rs` | Portable JSON format for database state |
| Workflow system | `src/config/workflows.rs` | Named workflows, roles, overlays |
| Agent connection | `src/tools/agents.rs` (`connect`) | Workflow-aware agent registration |
| Read-only SQL | `src/tools/query.rs` | Metric query execution |
| Web dashboard | `src/dashboard/` | Live monitoring (future integration) |
| Metrics documentation | `docs/METRICS.md` | Metric definitions and SQL examples |
| Workflow topologies | `docs/WORKFLOW_TOPOLOGIES.md` | Topology dimension documentation |
