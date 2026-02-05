# Workflow Customization Guide

When to add, remove, or modify states, phases, gates, and overlays — and when not to.

## Before You Change Anything

Ask yourself three questions:

1. **Is the default workflow actually failing?** A vague feeling that "we need more structure" is not a signal. Look for concrete symptoms: tasks stuck in the wrong state, workers confused about what to do next, time tracking missing data you need.

2. **Can the existing primitives solve this?** Tags, phases, gates, and overlays already cover a wide range. A new tag is cheaper than a new state. A gate is cheaper than a status. An advisory is cheaper than a gate.

3. **Who pays the cost?** Every new state, phase, or gate is a tax on every worker for every task. The worker claiming a quick bug fix pays the same overhead as the one running a complex migration.

## States

States are the most expensive thing to add. Every state appears in every task's lifecycle, affects dependency blocking, time tracking, and transition prompts.

### When to Add a State

- **There's a real waiting condition** that isn't "working" or "pending". Example: `review` where the task is out of the worker's hands and blocked on someone else. Without this state, workers can't express "I'm done with my part" vs "this is still in progress".

- **You need time tracking for a distinct activity**. Timed states contribute to `time_actual_ms`. If you need to measure how long tasks spend in code review separately from implementation, that's a state. If you just want to label the kind of work, that's a phase.

- **Dependency blocking semantics require it**. `blocking_states` controls which tasks prevent dependents from becoming ready. If you have a state that should NOT block downstream work (like "archived"), it must be defined as non-blocking.

### When NOT to Add a State

- **To categorize work type.** That's what phases are for. Don't create `implementing`, `testing`, `reviewing` states — use the `implement`, `test`, `review` phases with the `working` state.

- **To track sub-steps within a status.** Use `thinking()` for progress updates, or decompose into subtasks.

- **To mirror an external system.** If your CI pipeline has 8 stages, don't create 8 states. Use one `working` state with phases or tags.

- **Because "it would be nice to know."** Every state adds a transition prompt, a gate check, and a decision point for every worker on every task. If fewer than half your tasks would ever enter the state, it's probably a tag or a phase.

### State Design Checklist

Before adding a state, verify:

- [ ] It has clear entry and exit conditions (what triggers the transition?)
- [ ] At least one exit leads to a terminal or completed state (no dead ends)
- [ ] You've decided whether it's **timed** (contributes to time tracking) or untimed
- [ ] You've decided whether it's **blocking** (prevents dependents from starting)
- [ ] The disconnect behavior is defined (what happens when a worker in this state disconnects?)
- [ ] You've written the transition prompt (what should the worker know when entering this state?)

### State Anti-patterns

**The Linear Pipeline.** `draft → ready → assigned → working → review → testing → staging → deployed → completed`. This forces every task through 9 transitions. Most tasks don't need all of them. Use fewer states with phases instead: `pending → working → completed`, with phases handling the work type.

**The Approval Chain.** Multiple states that are just waiting for different people: `waiting-lead`, `waiting-qa`, `waiting-security`. Use one `review` state with tags indicating who is needed (`needed_tags: [reviewer]`, `needed_tags: [qa]`).

**The Undo State.** A state that exists only so you can "go back" from another state (e.g., `rework` as distinct from `working`). Usually `pending` or `working` with a tag like `rework` is sufficient.

## Phases

Phases are lightweight labels for the type of work happening within a state. They're orthogonal to states — a task in `working` state can be in `explore`, `implement`, `test`, or `review` phase.

### When to Add a Phase

- **You want to guide workers differently based on work type** without changing the state machine. The `working+implement` combo prompt says different things than `working+test`.

- **You want to track what kind of work is happening** for metrics or reporting. Phases appear in task history and can be filtered in queries.

- **Your gates need to distinguish work types.** A gate that fires on `working → completed` can check whether the task passed through the `test` phase.

### When NOT to Add a Phase

- **It overlaps with an existing phase.** The defaults cover most software work: `explore`, `plan`, `design`, `implement`, `test`, `review`, `security`, `deploy`, `doc`, `monitor`, `diagnose`, `triage`, `integrate`, `optimize`, `deliver`. Check this list before adding.

- **It's project-specific jargon** that means the same as an existing phase. "Spike" is `explore`. "Coding" is `implement`. "QA" is `test`. "PR review" is `review`.

- **It's a one-time activity** that only applies to a specific task type. Use a tag instead.

### Phase Design Checklist

- [ ] It describes a *kind of work*, not a *status* (wrong: "blocked", right: "diagnose")
- [ ] It's distinct from every existing phase (check the list above)
- [ ] You've written combo prompts for the most common state+phase pairs (at minimum: `working+<phase>`)
- [ ] You've set `unknown_phase` enforcement (`allow`, `warn`, or `reject` in the workflow config)

## Gates

Gates are exit requirements on status or phase transitions. They're the right tool when you want to enforce that something happened before a task can move forward.

### When to Add a Gate

See [GATES.md](GATES.md) for the full decision framework. In short:

- **Compliance or audit requirements.** Legal sign-off, security review, budget approval.
- **Handoff artifacts.** The next worker needs something from the current one.
- **Quality checkpoints.** Tests must pass, design must be reviewed.

### When NOT to Add a Gate

- **To remind workers of best practices.** Use transition prompts or advisories instead. Gates that workers routinely force-bypass are worse than no gate — they teach workers to ignore gates.

- **For every transition.** Gate fatigue is real. Workers who see 3 gates on every status change will stop reading them.

### Enforcement Levels

| Level | Use When | Workers Experience |
|-------|----------|-------------------|
| `reject` | Skipping causes real damage (data loss, compliance violation) | Hard block, must attach artifact or use `force=true` |
| `warn` | Important but has valid exceptions | Warning message, can proceed without artifact |
| `allow` | Informational reminder | Hint text, no friction |

## Overlays

Overlays modify a base workflow for specific contexts without forking the entire configuration.

### When to Create an Overlay

- **A subset of workers need different behavior.** Example: the `git-worktree` overlay adds a `patching` state and integrator role for multi-agent git workflows, but only workers who opt in get it.

- **A temporary process change.** Overlays can be added/removed at runtime with `add_overlay` and `remove_overlay`. Good for incidents, sprints, or experiments.

- **Cross-cutting concerns.** Troubleshooting, governance, and git are all orthogonal to the base workflow topology. They compose as overlays rather than requiring separate workflow files.

### When NOT to Create an Overlay

- **The change applies to all workers.** Modify the base workflow or create a new named workflow instead.

- **It conflicts with other overlays.** Overlays apply in order, and later overlays win on conflicts. If your overlay redefines the same states as another overlay, you'll get confusing behavior.

- **It's a single prompt change.** Use `prompts.yaml` overrides for tool descriptions or transition messages.

### Overlay vs New Workflow

| Overlays | New Workflow |
|----------|-------------|
| Additive changes (new states, advisories, gates) | Different topology (solo vs swarm) |
| Opt-in per worker | All workers use it |
| Composable (multiple overlays stack) | Standalone (one workflow per worker) |
| Runtime add/remove | Set on connect |

## Common Customization Scenarios

### "Tasks sit in pending too long"

Don't add a `ready` state. Instead:
- Use `auto_advance: true` in config to auto-transition tasks whose deps are satisfied
- Use `list_tasks(ready=true)` to find claimable work
- Add a transition prompt on `pending` that tells workers how to find ready tasks

### "We need code review before completion"

Don't add a `review` state unless you need time tracking for review time specifically. Instead:
- Add a `gate/code-review` gate on `working → completed` with `warn` enforcement
- Workers attach review evidence: `attach(type="gate/code-review", content="PR #123 approved")`
- If you DO need the state: add it as timed, non-blocking, with exits to `completed` and `working` (for rework)

### "Different teams use different processes"

Use named workflows (`workflow-swarm.yaml`, `workflow-relay.yaml`) selected on connect:
```
connect(workflow="swarm")
```
Or use overlays for smaller variations within the same topology.

### "We want to track time spent in meetings vs coding"

Don't add states. Use phases: `working+plan` for meetings, `working+implement` for coding. Phases are recorded in task history and queryable:
```sql
SELECT phase, SUM(duration_ms) FROM time_entries WHERE task_id = ? GROUP BY phase
```

## The Cost Gradient

From cheapest to most expensive:

1. **Tag** — free, no workflow change, filterable
2. **Phase** — lightweight, combo prompts optional, no state machine impact
3. **Advisory** — on-demand guidance, no workflow friction
4. **Gate (allow)** — informational, zero friction
5. **Gate (warn)** — light friction, workers can proceed
6. **Overlay** — adds behavior for opt-in workers
7. **Gate (reject)** — hard block, requires artifact or force
8. **State** — changes the state machine for every task and every worker

Start at the top. Only move down when the lighter option genuinely doesn't work.
