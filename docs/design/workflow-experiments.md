# Workflow Experiments System

> **Version:** 1.0
> **Date:** 2026-01-31
> **Status:** Design Proposal

## Motivation

We have five workflow configurations (solo, swarm, relay, hierarchical, push) that represent different multi-agent coordination patterns. We have opinions about when each works best, but no empirical data. This design describes how to use the existing task-graph infrastructure to run controlled experiments that measure real tradeoffs between coordination patterns.

### What We Want to Learn

1. **Push vs Pull coordination:** Does having a coordinator assign every task (push) add overhead that outweighs better load balancing? Or does self-selection (pull/swarm) cause enough contention and poor distribution to offset its simplicity?

2. **Hierarchical vs Flat:** Does a lead agent decomposing work and monitoring progress justify the cost of that lead's token budget and the serialization bottleneck it introduces?

3. **Specialist vs Generalist:** When specialist agents (relay: designer -> implementer -> reviewer -> tester) produce higher quality through handoff gates, does the sequential bottleneck and handoff overhead cost more in wall-clock time than generalists working in parallel?

4. **Granularity effects:** Do fine-grained atomic tasks (swarm-style) outperform coarse-grained tasks (solo-style) at different team sizes? Where is the crossover?

5. **Scaling behavior:** How does each workflow degrade as agent count increases? Which patterns have linear scaling and which hit coordination walls?

---

## Existing Infrastructure

The current codebase provides most of what an experiment system needs. Here is what exists and what role it plays.

### Templates (`src/db/template.rs`)

Templates are Snapshot-format JSON files that define reusable task structures. They support:

- **Identical starting conditions:** `instantiate_template()` creates fresh copies with new IDs, resetting all status, timestamps, and runtime fields. Two experiments can start from the exact same task graph.
- **ID remapping:** Every instantiation generates unique petname IDs via `remap_snapshot()`, so multiple experiment runs never collide.
- **Parent attachment:** Entry points can be attached to a parent task, allowing an experiment root task to own all instantiated work.
- **Extra tags:** `InstantiateOptions::with_extra_tags()` can stamp every task with an experiment identifier (e.g., `["exp-swarm-3agent-run1"]`).
- **Title prefix:** `with_title_prefix("Run-1")` disambiguates tasks from different runs in the same database.

Templates serve as the **control variable** -- the identical project shape that remains constant while we vary the workflow.

### Workflows (`src/config/workflows.rs`, `config/workflow-*.yaml`)

Workflow configs define the **independent variable** -- the coordination pattern under test. Each workflow specifies:

- **States and transitions:** What states exist and which transitions are allowed.
- **Transition prompts:** Instructions that shape agent behavior at each state change (the mechanism by which coordination patterns are enforced).
- **Roles:** Who can do what (`lead` vs `worker`, `designer` vs `implementer`).
- **Role prompts:** Behavioral instructions per role (claiming strategy, handoff protocol, failure handling).
- **Gates:** Required artifacts before transitions (the relay workflow requires a design spec gate before implementation can proceed).
- **Overlays:** Additive modifications via `apply_overlay()` for cross-cutting concerns (git workflow, troubleshooting) without modifying the base workflow.

Five workflow configs exist: `solo`, `swarm`, `relay`, `hierarchical`, `push`. The push workflow already includes an `experiment_metrics` section documenting what to capture.

### Metrics Tracking (`src/tools/tracking.rs`, `src/db/state_transitions.rs`)

The metrics infrastructure provides the **dependent variables** -- what we measure:

- **Automatic time tracking:** `record_state_transition()` closes the previous transition with an `end_timestamp` and accumulates `time_actual_ms` for timed states. Every status change is recorded in `task_sequence`.
- **`log_metrics` tool:** Agents call this to record `cost_usd` and up to 8 integer metric slots (token counts, custom values). Values are aggregated (added to existing).
- **`get_metrics` tool:** Retrieves metrics for one or more tasks, supporting aggregation across task groups.
- **`task_history` tool:** Returns the full state transition sequence for a task with computed durations, time-per-status, and time-per-agent breakdowns.
- **`project_history` tool:** Returns project-wide transition data with time range filters, transition counts by status and agent, and total tracked time.
- **`get_stats`:** Aggregation queries returning `tasks_by_status`, `total_points`, `completed_points`, `total_time_estimate_ms`, `total_time_actual_ms`, `total_cost_usd`, and the 8-slot metrics array.

### Export/Import Pipeline (`src/export/mod.rs`, `src/db/export.rs`, `src/db/import.rs`)

The export system captures experiment results:

- **Snapshot format:** JSON with all tables (`tasks`, `dependencies`, `attachments`, `task_tags`, `task_sequence`) ordered deterministically for diff-friendly output.
- **Full state history:** The `task_sequence` table is exported, preserving the complete transition timeline for post-hoc analysis.
- **Gzip support:** Large experiments can export compressed.
- **Schema versioning:** Exports include `schema_version` and `export_version` for forward compatibility.

---

## Experiment Protocol

### Overview

An experiment compares two or more workflow configurations running the same template project with the same agent count. The process has four phases: prepare, execute, collect, analyze.

### Phase 1: Prepare

#### 1a. Create the Template Project

Build a representative task graph that exercises the dimensions you want to test. Export it as a template.

```
# Work in a scratch database to build the template
task-graph export --output templates/medium-feature.json
```

Template design guidelines:

- **Realistic structure:** 15-30 tasks with a mix of independent and dependent work. Include at least one critical path (chain of `follows` dependencies) and at least one parallel fan-out (multiple independent siblings under a parent).
- **Tag the tasks:** Use `needed_tags` to indicate specialist requirements (e.g., `designer`, `implementer`, `tester`). Generalist workflows will ignore these; specialist workflows will route by them.
- **Include estimates:** Set `time_estimate_ms` and `points` on tasks so metrics can compute estimate accuracy and weighted throughput.
- **Variety:** Mix task sizes. Some 1-point quick tasks, some 5-point substantial ones. This tests whether the workflow handles heterogeneous work.

#### 1b. Define the Experiment Matrix

Decide which variables to test and hold constant.

| Variable | Role | Example Values |
|----------|------|----------------|
| Workflow config | Independent | `swarm`, `hierarchical`, `push` |
| Agent count | Control (or 2nd independent) | 3 |
| Template | Control | `medium-feature.json` |
| Model | Control | Same model for all agents |
| Run count | Replication | 3 runs per configuration |

#### 1c. Create the Experiment Manifest

A YAML file describing the experiment:

```yaml
# experiments/push-vs-pull.yaml
name: push-vs-pull
description: Compare push coordination overhead against pull self-selection
template: templates/medium-feature.json
agent_count: 3
runs_per_config: 3

configs:
  - workflow: swarm
    description: Pure pull - agents self-select from ready queue
  - workflow: push
    description: Pure push - coordinator assigns every task
  - workflow: hierarchical
    description: Hybrid - lead decomposes, workers pull subtasks

metrics:
  - wall_clock_time_ms
  - total_cost_usd
  - rework_rate
  - agent_utilization_pct
  - dependency_wait_time_ms
  - tasks_per_hour
  - coordination_overhead_pct
```

### Phase 2: Execute

Each run follows this sequence:

1. **Instantiate the template** with experiment-specific tags:
   ```
   instantiate_template(
     template: "medium-feature.json",
     options: {
       parent_task_id: "experiment-root",
       extra_tags: ["exp:push-vs-pull", "run:swarm-1"],
       title_prefix: "swarm-r1"
     }
   )
   ```

2. **Load the workflow config** for this run. The workflow's prompts and role definitions shape agent behavior -- this is the independent variable.

3. **Connect agents** with appropriate tags. For specialist workflows (relay), connect agents with role-specific tags (`designer`, `implementer`, `tester`). For generalist workflows (swarm), all agents get `worker` tags.

4. **Run to completion.** Agents follow the workflow's prompts, claiming and completing tasks. The system automatically records:
   - Every state transition in `task_sequence` (with timestamps)
   - Accumulated `time_actual_ms` on each task
   - `cost_usd` and `metrics` via `log_metrics` calls
   - File coordination events via `mark_file`/`unmark_file`

5. **Export the results:**
   ```
   task-graph export --output results/push-vs-pull/swarm-run1.json
   ```

#### Isolation Between Runs

Each run should use a **fresh database** to avoid cross-contamination. The simplest approach:

```bash
# Set a unique DB path for each run
TASK_GRAPH_DB_PATH=experiments/swarm-run1.db task-graph-mcp
```

Alternatively, use the template system's ID remapping to run multiple experiments in one database, filtering by experiment tags in analysis. But separate databases are cleaner.

### Phase 3: Collect

After all runs complete, gather metrics from each exported snapshot.

#### Metrics to Collect

| Metric | Source | Computed From |
|--------|--------|---------------|
| **Wall-clock time** | `tasks` table | `MAX(completed_at) - MIN(created_at)` across all tasks in the experiment |
| **Total cost** | `tasks.cost_usd` | `SUM(cost_usd)` across all tasks |
| **Active work time** | `tasks.time_actual_ms` | `SUM(time_actual_ms)` across all tasks |
| **Dependency wait time** | `task_sequence` | Total time tasks spent in `pending` state after their creation (excluding initial pending before first claim) |
| **Agent utilization** | `task_sequence` | Per agent: `time_in_working / wall_clock_time`. Measures how much of the experiment duration each agent spent doing productive work vs waiting. |
| **Rework rate** | `task_sequence` | Tasks with more than one `working` period divided by total tasks |
| **Throughput** | `tasks` | `completed_tasks / wall_clock_hours` |
| **Coordination overhead** | `task_sequence` | `(time_in_pending + time_in_assigned) / (time_in_pending + time_in_assigned + time_in_working)` |
| **Dispatch latency** (push only) | `task_sequence` | Time from task creation to `assigned` transition |
| **Pickup latency** | `task_sequence` | Time from `assigned` (or `pending`) to `working` transition |
| **Failure rate** | `tasks` | Tasks ending in `failed` / total tasks |
| **Estimate accuracy** | `tasks` | `AVG(time_actual_ms / time_estimate_ms)` for tasks with estimates |
| **Points throughput** | `tasks` | `SUM(points) / wall_clock_hours` for weighted throughput |
| **Per-metric slots** | `tasks.metric_0..7` | Token counts, custom counters logged via `log_metrics` |

#### Collection Method

A post-hoc analysis script loads each exported snapshot and computes the metrics above. No runtime instrumentation is needed beyond what the system already records.

```python
# Pseudocode for metric extraction
snapshot = load_snapshot("results/swarm-run1.json")
tasks = snapshot["tables"]["tasks"]
sequence = snapshot["tables"]["task_sequence"]

wall_clock = max(t["completed_at"] for t in tasks if t["completed_at"]) \
           - min(t["created_at"] for t in tasks)
total_cost = sum(t["cost_usd"] for t in tasks)
# ... etc
```

### Phase 4: Analyze

Compare metrics across configurations using the standard tools:

1. **Tabular comparison:** One row per configuration, columns for each metric, averaged across runs with standard deviation.

2. **Key ratios:**
   - Speedup: `wall_clock(solo) / wall_clock(workflow)` -- how much faster is multi-agent?
   - Cost ratio: `total_cost(workflow) / total_cost(solo)` -- how much more expensive?
   - Efficiency: `speedup / agent_count` -- is each additional agent paying for itself?

3. **Distribution analysis:** Per-agent utilization histograms to detect load imbalance. Dependency wait time distributions to find bottleneck tasks.

4. **Scaling curves:** If running experiments at multiple agent counts, plot throughput vs agent count per workflow to identify scaling limits.

---

## Control Variables

To produce meaningful comparisons, these must remain constant across experiment arms:

| Variable | Why | How to Control |
|----------|-----|----------------|
| **Template project** | Same task graph structure, same dependencies, same estimates | Use `instantiate_template` from a single JSON file |
| **Agent count** | Same parallelism budget | Start the same number of agent processes per run |
| **Model** | Same underlying capability | Configure all agents to use the same model and temperature |
| **Prompt foundation** | Same base instructions | Vary only workflow-specific prompts; keep system prompts constant |
| **Hardware/network** | Same latency environment | Run experiments on the same machine or cloud instance |
| **Task content** | Same actual work to perform | Templates define task descriptions; agents follow them |

The only thing that changes between arms is the **workflow configuration file**, which controls:
- State transition prompts (agent behavioral instructions)
- Role definitions and role prompts
- Gates (required artifacts)
- Coordination model (encoded in prompts and roles)

---

## What Is Missing

The existing infrastructure covers about 80% of what a full experiment system needs. Here is what would need to be built, roughly ordered by priority.

### High Priority (Needed for First Experiment)

#### 1. Experiment Runner Script

A CLI command or script that automates the execute phase:

```bash
task-graph experiment run --manifest experiments/push-vs-pull.yaml
```

This would:
- Create a fresh database per run
- Instantiate the template with proper tags
- Start the MCP server with the specified workflow
- Signal agents to connect (or wait for manual connection)
- Wait for all tasks to reach terminal states
- Export results
- Repeat for each configuration and run

**Scope:** Shell script or Rust CLI subcommand. The task-graph binary already has a CLI (`src/cli/mod.rs`) that could host an `experiment` subcommand.

**Complexity:** Medium. The core logic (instantiate + export) exists. The orchestration (starting servers, waiting for completion) is new.

#### 2. Metrics Extraction Script

A script that reads exported snapshots and computes the metrics table from Phase 3.

```bash
task-graph experiment analyze --results-dir results/push-vs-pull/
```

Output: A comparison table (markdown or CSV) with one row per configuration, columns for each metric.

**Scope:** Python or Rust. Could use the existing `Snapshot` struct for parsing. The SQL queries from `docs/METRICS.md` translate directly to Rust `rusqlite` queries or Python `sqlite3` queries.

**Complexity:** Low-medium. The queries are straightforward. The main work is wiring them together with nice output formatting.

#### 3. Template Library

A curated set of template projects at different scales:

| Template | Tasks | Deps | Description |
|----------|-------|------|-------------|
| `tiny-feature.json` | 5 | 3 | Minimal: one parent, four subtasks |
| `medium-feature.json` | 15-20 | 10-15 | Realistic: mixed deps, critical path, parallel fan-out |
| `large-refactor.json` | 40-60 | 30+ | Stress test: deep hierarchy, many dependencies |
| `independent-batch.json` | 20 | 0 | All independent tasks, tests pure parallelism |
| `pipeline.json` | 10 | 9 | Fully sequential chain, tests handoff efficiency |

These should be checked into `templates/` and documented. They need specialist tags on tasks so relay/hierarchical workflows can route properly, and they should have estimates for throughput analysis.

**Scope:** JSON files created by hand or exported from real projects.

**Complexity:** Low. Just careful task graph design.

### Medium Priority (Improves Experiment Quality)

#### 4. Automatic Completion Detection

Currently there is no built-in way to know when "the experiment is done." The runner script would need to poll `get_stats()` and check if all tasks are in terminal states (`completed`, `failed`, `cancelled`).

A simple approach: query tasks where status is in the blocking states list. When count reaches zero, the experiment is done.

```sql
SELECT COUNT(*) FROM tasks
WHERE id IN (SELECT id FROM experiment_tasks)
  AND status IN ('pending', 'assigned', 'working');
```

**Scope:** A polling loop in the experiment runner. Could also be a `--wait` flag on the export CLI command.

**Complexity:** Low.

#### 5. Experiment Tagging in the Database

Add first-class support for experiment metadata. Currently, experiment identity is encoded via `extra_tags` on tasks. A cleaner approach would be an `experiment_runs` table:

```sql
CREATE TABLE experiment_runs (
  id TEXT PRIMARY KEY,
  experiment_name TEXT NOT NULL,
  workflow_name TEXT NOT NULL,
  agent_count INTEGER,
  template_name TEXT,
  started_at INTEGER,
  completed_at INTEGER,
  config_snapshot TEXT  -- JSON of the full workflow config used
);
```

Tasks would reference their experiment run via a tag or a dedicated column. This makes querying cleaner than filtering by tag prefix.

**Scope:** Migration + schema change + minor tool updates.

**Complexity:** Medium. Touches the schema, import/export, and stats queries.

#### 6. Warm-Up and Cool-Down Handling

The first task in any experiment run has disproportionate startup cost (agent initialization, context loading). The last task may have cleanup overhead. Metrics should support excluding warm-up and cool-down tasks, either by:
- Marking them with a tag (`warmup`, `cooldown`)
- Excluding the first/last N tasks from aggregation
- Using time-based trimming (exclude first/last M minutes)

**Scope:** Metric extraction script feature.

**Complexity:** Low.

### Low Priority (Nice to Have)

#### 7. Live Dashboard During Experiments

The web dashboard (`src/dashboard/`) already shows task status. Extending it with experiment-specific views (per-workflow progress, live metric counters, agent utilization gauges) would help monitor experiments in real time.

**Scope:** Dashboard template changes.

**Complexity:** Medium-high. The dashboard exists but adding experiment-specific views requires new routes and templates.

#### 8. Statistical Significance Testing

When comparing metrics across configurations, report whether differences are statistically significant. With 3+ runs per configuration, compute:
- Mean and standard deviation per metric per configuration
- p-values from t-tests or Mann-Whitney U tests
- Confidence intervals

**Scope:** Analysis script feature (likely Python with scipy).

**Complexity:** Low once the data extraction works.

#### 9. Automated Report Generation

Generate a markdown or HTML report from experiment results with tables, charts, and narrative summaries. Could use the analysis script output plus a template.

**Scope:** Script that formats analysis output.

**Complexity:** Low-medium.

---

## Example: First Experiment

Here is a concrete plan for the first experiment to validate the system.

### Goal

Compare swarm (pull) vs push coordination with 3 agents on a medium-sized feature.

### Setup

1. Create `templates/medium-feature.json` with ~20 tasks:
   - 1 root task ("Build Widget System")
   - 4 top-level subtasks (Design, Implement Core, Implement UI, Test)
   - Each top-level subtask has 3-5 leaf tasks
   - `follows` dependencies between Design -> Implement -> Test
   - Parallel paths between Implement Core and Implement UI
   - `needed_tags`: `designer` on design tasks, `implementer` on impl tasks, `tester` on test tasks

2. Run 3 iterations of each:
   - **Swarm:** All 3 agents connect as `worker` generalists. Pull coordination.
   - **Push:** 1 agent connects as `coordinator`, 2 as `worker`. Push coordination.

3. Collect: Export each run's database.

4. Analyze: Compare wall-clock time, total cost, rework rate, agent utilization.

### Expected Outcomes

- Swarm should complete faster (3 parallel workers vs 1 coordinator + 2 workers)
- Push should have lower rework rate (coordinator can route tasks to best-fit worker)
- Push should have higher coordination overhead (coordinator's token budget)
- Swarm should have more variable agent utilization (luck of the draw on claiming)

### Success Criteria for the Experiment System

The experiment system works if:
1. Templates instantiate identically across runs (verified by task count and dependency count)
2. Metrics are automatically captured without manual intervention beyond `log_metrics`
3. The analysis script produces a comparison table from exported snapshots
4. Results are reproducible: multiple runs of the same config produce metrics within reasonable variance

---

## Relationship to Existing Docs

- **`docs/METRICS.md`** defines the full metrics catalog with SQL queries. This design uses those metrics as dependent variables.
- **`docs/WORKFLOW_TOPOLOGIES.md`** describes the workflow dimension space. This design turns those qualitative comparisons into quantitative experiments.
- **`config/workflow-push.yaml`** already includes an `experiment_metrics` section. This design generalizes that approach across all workflows.

---

## Summary

The task-graph system already has the three pillars needed for experiments:

1. **Templates** provide identical starting conditions (control)
2. **Workflows** provide the coordination patterns to compare (independent variable)
3. **Metrics tracking** provides automatic measurement (dependent variables)

What needs to be built is the orchestration layer that ties them together: a runner script, a metrics extraction script, and a library of template projects. This is a scripting and tooling effort, not an architectural change. The core infrastructure is ready.
