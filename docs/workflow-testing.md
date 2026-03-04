# Workflow Testing

## Status

**Hierarchical** is the only workflow tested in production (12 workers, 28 tasks, ~114 pts, 2 rounds).
A second round of fixes is being applied based on that test. The other 6 workflows
(solo, swarm, push, relay, sprint, kanban) need practical testing.

## Available Workflows

| Workflow | Topology | Claiming | Coordination | Best for |
|----------|----------|----------|--------------|----------|
| **solo** | Single agent | Self-claim | None | Simple tasks, one worker |
| **swarm** | Flat peers | Self-claim (race) | File marks, advisory | Parallel independent tasks |
| **push** | Lead → workers | Assigned by lead | Lead dispatches, workers execute | Small teams, clear task ownership |
| **hierarchical** | Lead + sub-leads | Assigned top-down | Multi-level delegation | Large teams, complex decomposition |
| **relay** | Sequential handoff | Claim on completion of predecessor | Chain-based | Pipeline workflows, staged processing |
| **sprint** | Time-boxed swarm | Self-claim within sprint | Sprint boundaries | Iterative development cycles |
| **kanban** | Pull-based | Self-claim from backlog | WIP limits, flow metrics | Continuous delivery, maintenance |

## Available Overlays

Overlays augment any workflow non-destructively. They add prompts, gates, and advisories.

| Overlay | Purpose | Key additions |
|---------|---------|---------------|
| **git** | Basic git workflow | Commit reminders, `mark_file` guidance, `thinking()` usage |
| **git-worktree** | Multi-agent git isolation | Patch-based workflow, commit gate on completed, layered-worktree advisory |
| **reasoning** | Decision documentation | Attach reasoning notes before completing, record alternatives considered |
| **governance** | Approval gates | Review/approval gates at state transitions |
| **troubleshooting** | Diagnostic workflow | Structured problem diagnosis, root cause analysis |

## Known Issues from Hierarchical Test

### Prompt delivery gap in coordinator-assigned workflows — FIXED

**Status:** All rounds complete.

In hierarchical/push workflows, the coordinator calls `update(assignee="worker-id")` and
receives the transition prompts in **their** response. The assigned worker never sees
overlay-prescribed behaviors unless they call `get_prompts` independently.

**Impact:** Overlays had zero behavioral effect on 12 workers across 2 rounds.

**Root cause chain:**
1. `claim` returns prompts to the caller — works when workers self-claim
2. `update(assignee=)` returns prompts to the coordinator, not the assignee
3. Workers don't know to call `get_prompts` after being assigned
4. No push mechanism delivers prompts to workers on assignment

**Fixes applied (Round 1):**
- Prompt attribution added — `get_prompts` now returns `[{"text": "...", "source": "workflow:enter~working"}]`
  so agents can see which overlay/workflow contributed each prompt
- Overlay discovery resources added — `docs://overlays/{name}`, `docs://overlays/list`
- Active overlays surfaced in `config://current`
- Overlays included in `docs://workflows/list` response

**Fixes applied (Round 2):**
- `claim()` now delivers full transition prompts for pre-assigned tasks (assigned→working)
- "Review prompts after claiming" guidance added to all workflow role prompts

**Fixes applied (Round 3):**
- `prompts` parameter on `update` tool (`all`/`none`/`caller`) for coordinator prompt filtering

### Overlay discovery — FIXED

Previously agents could not discover what overlays do or that they're active. All gaps resolved:
- `docs://overlays/{name}` resource — detailed per-overlay documentation
- `docs://overlays/list` resource — lists all available overlays
- `docs://workflows/list` now includes overlays
- `config://current` shows `active_overlays` when non-empty
- `get_prompts` returns source attribution per prompt

### File contention — FIXED

- `claim(files=[...])` auto-marks files on claim (Round 1)
- File contention detection on claim warns when files overlap with other active tasks (Round 2)
- Still potential: coordinator-facing contention report, deeper integration into workflow prompts

### Feedback tool improvements — FIXED

- `give_feedback` now records workflow name and active overlays from the worker's
  registration, providing context for feedback entries

## Lessons from Hierarchical Test

### What worked
- Task decomposition and dependency chains functioned correctly
- Parent auto-rollup (parent completes when all children finish)
- Cascading cancellation (cancelling parent cancels children)
- Worker connect/claim/update lifecycle
- Worktree isolation prevented most direct file conflicts

### What didn't work
- **Overlays were invisible** — workers never learned what overlays expected of them
- **File contention was unmanaged** — `mark_file` existed but no workflow prompted its use
- **Coordinator context pollution** — lead received worker-targeted prompts, shifting behavior
- **Manual merge was error-prone** — integrating 6 worktrees required careful conflict resolution
- **Workers escaped worktrees** — some agents modified main repo files instead of worktree copies

### Coordinator best practices discovered
1. **Analyze file contention before dispatch** — batch tasks touching the same files to one worker
2. **Merge in dependency order** — merge leaf tasks first, work up to tasks with more dependencies
3. **Stash valid changes before restoring** — when main repo gets contaminated, stash good changes first
4. **Use python for batch conflict resolution** — regex-based fixes faster than manual editing
5. **Re-run full test suite after each merge** — catch interaction bugs between independent changes

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
- **Prompt compliance** — did workers follow overlay-prescribed behaviors
- **Coordination overhead** — time spent on task management vs. actual work
- **Rework rate** — tasks that needed re-doing after integration

### Candidate benchmark approaches

1. **Self-hosting** — use task-graph-mcp's own codebase as the benchmark target.
   Define a feature set, run it through each workflow, measure outcomes.
   Advantage: dogfooding, realistic. Disadvantage: moving target.

2. **Replay a real session** — export the task graph from the hierarchical test,
   strip implementation details, reuse the structure with a different codebase.
   Advantage: realistic dependency patterns. Disadvantage: codebase-specific.

3. **Standard refactoring kata** — take a well-known open-source project, define a
   multi-agent refactoring (e.g., rename a module, split a god class, migrate an API).
   Advantage: realistic + reproducible. Disadvantage: setup effort.

4. **Synthetic codebase task** — generate a multi-file project with known structure,
   create a task graph that requires coordinated changes across files.
   Advantage: fully reproducible, no external deps. Disadvantage: artificial.

### Workflow comparison matrix

| Metric | Solo | Swarm | Push | Relay | Hierarchical | Sprint |
|--------|------|-------|------|-------|--------------|--------|
| Wall-clock time | | | | | baseline | |
| Merge conflicts | | | | | ~3 (2 files) | |
| Prompt compliance | | | | | 0% → fixing | |
| Coord. overhead | | | | | high (manual merge) | |
| Rework rate | | | | | 0% (clean merges) | |

## Test Plan

### Phase 1: Fix overlay delivery — DONE
Fix the prompt delivery gap so overlays actually affect behavior.

**Round 1 (DONE):** overlay-resources, overlays-in-workflow-list, active-overlays-config,
prompt-attribution, claim-files-param, feedback-workflow-metadata, commit-gate,
layered-worktree-advisory, mark-file-prompts

**Round 2 (DONE):** prompt-delivery-assigned, file-contention-detection,
audit-overlay-prompts

**Round 3 (DONE):** update-prompts-param

### Phase 2: Choose benchmark
Select one of the approaches above and build the task graph template.
Self-hosting (option 1) is the current front-runner — already dogfooding with the
hierarchical test that produced these improvements.

### Phase 3: Run each workflow
Execute the same benchmark with solo, swarm, hierarchical (minimum).
Push and relay are stretch goals.

### Phase 4: Analyze and iterate
Compare metrics, identify prompt improvements, re-run.
