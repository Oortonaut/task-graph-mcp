# Workflow Testing

## Status

Only **hierarchical** has been tested in production (12 workers, 28 tasks, ~114 pts, 2 rounds).
The other 5 workflows (solo, swarm, push, relay, sprint, kanban) need practical testing.

## Known Issues from Hierarchical Test

### Prompt delivery gap in coordinator-assigned workflows

In hierarchical/push workflows, the coordinator calls `update(assignee="worker-id")` and
receives the transition prompts (including overlay contributions) in **their** response.
The assigned worker never sees these prompts unless they independently call
`get_prompts(status="working", task="...", worker_id="...")`.

**Impact:** Overlays had zero behavioral effect on 12 workers across 2 rounds. Workers
never generated patches (git-worktree), never attached reasoning notes (reasoning).

**Root cause chain:**
1. `claim` returns prompts to the caller — works when workers self-claim
2. `update(assignee=)` returns prompts to the coordinator, not the assignee
3. Workers don't know to call `get_prompts` after being assigned
4. No push mechanism delivers prompts to workers on assignment

**Two-sided fix needed:**

Worker side (task #18):
- Add "call get_prompts after claiming" instruction to worker role prompts
- Or: store pending prompts on the task, delivered when worker next interacts
- Or: ensure `claim` returns full prompts even when pre-assigned

Coordinator side (task #19):
- Add `prompts` parameter to `update`: `none | all | caller`
- `none` — suppress prompts (coordinator doesn't want worker-targeted guidance)
- `all` — current behavior (default, backward compat)
- `caller` — return only prompts relevant to the caller's own role/state
- Without this, coordinators receive worker-targeted prompts that pollute their
  own context and shift them out of "lead" mode

### Overlay discovery is broken

Agents cannot discover what overlays do or that they're active. See feedback.md for full
details. Key gaps:
- No `docs://overlays/{name}` resource
- `docs://workflows/list` omits overlays
- `config://current` doesn't show active overlays
- `get_prompts` has no source attribution

### File contention

`mark_file` exists but wasn't used. Multiple workers touched the same files without
coordination. Need to integrate file marking into the claim workflow.

## Benchmark Requirements

A good workflow benchmark needs:

### Task characteristics
- **Dependency chains** — at least 2 rounds of serial dependencies
- **File contention** — multiple tasks that touch shared files
- **Varying complexity** — mix of small (1-3 pt) and medium (5-8 pt) tasks
- **Clear acceptance criteria** — objective pass/fail for each task
- **Reproducible** — same task graph can be run against different workflows

### Measurable outcomes
- **Wall-clock time** — total time from first claim to all tasks completed
- **Merge conflict count** — how many conflicts during integration
- **Prompt compliance** — did workers follow overlay-prescribed behaviors (patches, reasoning, commits)
- **Coordination overhead** — time spent on task management vs. actual work
- **Rework rate** — tasks that needed re-doing after integration

### Candidate benchmark approaches

1. **Synthetic codebase task** — generate a multi-file project with known structure,
   create a task graph that requires coordinated changes across files. Advantage:
   fully reproducible, no external deps. Disadvantage: artificial.

2. **Replay a real session** — export the task graph from the acp-unreal session,
   strip implementation details, reuse the structure with a different codebase.
   Advantage: realistic dependency patterns. Disadvantage: codebase-specific.

3. **Standard refactoring kata** — take a well-known open-source project, define a
   multi-agent refactoring (e.g., rename a module, split a god class, migrate an API).
   Advantage: realistic + reproducible. Disadvantage: setup effort.

4. **Self-hosting** — use task-graph-mcp's own codebase as the benchmark target.
   Define a feature set, run it through each workflow, measure outcomes.
   Advantage: dogfooding. Disadvantage: moving target.

### Workflow comparison matrix

| Metric | Solo | Swarm | Push | Relay | Hierarchical | Sprint |
|--------|------|-------|------|-------|--------------|--------|
| Wall-clock time | | | | | baseline | |
| Merge conflicts | | | | | baseline | |
| Prompt compliance | | | | | 0% (broken) | |
| Coord. overhead | | | | | high (manual merge) | |
| Rework rate | | | | | ? | |

## Test Plan

### Phase 1: Fix overlay delivery
Before benchmarking, fix the prompt delivery gap so overlays actually affect behavior.
Tasks: #13-17 (overlay discovery), prompt delivery fix.

### Phase 2: Choose benchmark
Select one of the approaches above and build the task graph template.

### Phase 3: Run each workflow
Execute the same benchmark with solo, swarm, hierarchical (minimum).
Push and relay are stretch goals.

### Phase 4: Analyze and iterate
Compare metrics, identify prompt improvements, re-run.
