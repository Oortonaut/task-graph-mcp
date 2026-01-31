# Push vs Pull Coordination Experiments

Comparative experiments testing three task distribution strategies using the browser project template.

## Experiment Matrix

| Variant | Workflow | Coordination | Agents | Coordinator? |
|---------|----------|-------------|--------|-------------|
| **pure-push** | `push` | Coordinator assigns all tasks via `update(assignee=)` | 1+4 | Yes |
| **pure-pull** | `swarm` | All agents self-select via `list_tasks(ready=true)` + `claim()` | 4 | No |
| **hybrid** | `hierarchical` | Lead pushes top-level, workers pull subtasks | 1+4 | Yes (partial) |

## Hypotheses

1. **Push** reduces contention but introduces coordinator bottleneck — expect even task distribution but higher idle time
2. **Pull** maximizes throughput for independent tasks but may suffer claim contention at scale
3. **Hybrid** balances both — strategic push at top level, autonomous pull at leaf level — expected best overall

## Templates

- `templates/browser-parallel.json` — Flat parallelizable tasks (tests contention)
- `templates/browser-phased.json` — Phased sequential tasks (tests dependency handling)

## Running an Experiment

```bash
# 1. Reset database
cargo run -- --db experiments/results/push-4/tasks.db reset

# 2. Import template
cargo run -- --db experiments/results/push-4/tasks.db import experiments/templates/browser-parallel.json

# 3. Launch agents (see variant YAML for per-agent instructions)
# Each agent connects with the specified workflow:
#   connect(worker_id="coordinator", workflow="push", tags=["coordinator", "lead"])
#   connect(worker_id="worker-1", workflow="push", tags=["worker", "implementer"])

# 4. After completion, export results
cargo run -- --db experiments/results/push-4/tasks.db export experiments/results/push-4/snapshot.json
```

Repeat for each variant, then compare metrics across result databases.

## Key Metrics

| Metric | Push | Pull | Hybrid | Notes |
|--------|------|------|--------|-------|
| Wall clock time | Higher (dispatch overhead) | Lowest (no bottleneck) | Medium | Primary throughput measure |
| Coordinator overhead | Measurable | Zero | Low | Time spent dispatching vs doing |
| Task distribution (Gini) | Low (even) | Variable | Medium | Push should distribute most evenly |
| Claim contention | Zero | Scales with agents | Low | Only relevant in pull model |
| Worker idle time | High (waiting for assignment) | Low | Medium | Push workers wait between assignments |

## Files

- `push-experiment.yaml` — Pure push variant definition with 3-way comparison
- `pure-pull.yaml` — Pure pull variant with scaling analysis
- `experiment-hybrid.yaml` — Hybrid push/pull variant definition
