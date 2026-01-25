---
name: task-graph-worker
description: Worker role for task-graph-mcp - claims tasks, reports progress via thinking(), coordinates file access, and completes work
license: Apache-2.0
metadata:
  version: 1.1.0
  suite: task-graph-mcp
  role: worker
  requires: task-graph-basics
---

# Task Graph Worker

Worker role: find available tasks, claim work matching your capabilities, report progress, and complete tasks.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Quick Start

```
# 1. Connect with your capabilities
connect(worker_id="rust-worker", tags=["rust", "backend"], force=true)
→ worker_id (SAVE THIS!)

# 2. Find work matching your skills
list_tasks(ready=true, worker_id=worker_id)

# 3. Claim a task
claim(worker_id=worker_id, task=task_id)

# 4. Report progress as you work
thinking(worker_id=worker_id, thought="Implementing auth module...")

# 5. Complete when done
update(worker_id=worker_id, task=task_id, state="completed")
```

---

## Role Definition

As a **Worker**, you:

| Do | Don't |
|----|-------|
| Claim tasks matching your tags | Claim tasks you can't complete |
| Report progress frequently | Go silent for long periods |
| Lock files before editing | Edit files others are using |
| Update task state when done | Leave tasks in limbo |
| Log your costs | Forget to track usage |

---

## Work Loop

```
┌─────────────────────────────────────────────────────┐
│ 1. FIND WORK                                        │
│    list_tasks(ready=true, worker_id=worker_id)      │
│    • Returns unclaimed tasks you can claim          │
│    • Filters by your tags automatically             │
├─────────────────────────────────────────────────────┤
│ 2. CLAIM                                            │
│    claim(worker_id=worker_id, task=task_id)         │
│    • Now you own this task                          │
│    • Status changes to in_progress                  │
│    • Timer starts for time tracking                 │
├─────────────────────────────────────────────────────┤
│ 3. WORK                                             │
│    a. Read task details:                            │
│       get(task=task_id)                             │
│       attachments(task=task_id, content=true)       │
│                                                     │
│    b. Lock files you'll edit:                       │
│       claim_file(worker_id=worker_id, file="src/x.rs")│
│                                                     │
│    c. Report progress frequently:                   │
│       thinking(worker_id=worker_id,                 │
│                thought="Implementing X...")         │
│                                                     │
│    d. Do the actual work                            │
├─────────────────────────────────────────────────────┤
│ 4. FINISH                                           │
│    a. Release file locks:                           │
│       release_file(worker_id=worker_id, file="src/x.rs")│
│                                                     │
│    b. Attach outputs (optional):                    │
│       attach(task=task_id, name="result",           │
│              content="...", mime="text/plain")      │
│                                                     │
│    c. Log costs:                                    │
│       log_cost(worker_id=worker_id, task=task_id,   │
│                tokens_in=1000, cost_usd=0.05)       │
│                                                     │
│    d. Complete task:                                │
│       update(worker_id=worker_id, task=task_id,     │
│              state="completed")                     │
├─────────────────────────────────────────────────────┤
│ 5. REPEAT                                           │
│    Go back to step 1                                │
└─────────────────────────────────────────────────────┘
```

---

## Progress Reporting

### Why Report Progress?

- Coordinators see what you're doing
- Other workers know you're active
- Helps identify stuck tasks
- Creates audit trail

### How Often?

| Situation | Frequency |
|-----------|-----------|
| Starting new subtask | Immediately |
| Every few minutes of work | ~2-5 min |
| Before long operation | Before starting |
| After completing subtask | Immediately |
| Hitting a blocker | Immediately |

### Examples

```
thinking(worker_id=worker_id, thought="Reading existing code...")
thinking(worker_id=worker_id, thought="Designing solution approach")
thinking(worker_id=worker_id, thought="Implementing auth middleware")
thinking(worker_id=worker_id, thought="Writing unit tests")
thinking(worker_id=worker_id, thought="Running test suite")
thinking(worker_id=worker_id, thought="Fixing failing test: auth_test_3")
thinking(worker_id=worker_id, thought="All tests passing, preparing to complete")
```

---

## File Coordination

### Why Lock Files?

Multiple workers may edit the same codebase. Advisory locks prevent conflicts.

### Workflow

```
# 1. Check if file is locked
list_files(files=["src/auth.rs"])

# 2. Check for recent activity
claim_updates(worker_id=worker_id)

# 3. Claim the file
claim_file(worker_id=worker_id, file="src/auth.rs",
           reason="Adding auth middleware")

# 4. Do your work...

# 5. Release when done
release_file(worker_id=worker_id, file="src/auth.rs",
             reason="Auth middleware complete")
```

### Handling Conflicts

If file is locked by another worker:

1. **Wait** - Poll with `claim_updates` for release
2. **Coordinate** - Work on different files
3. **Request** - Ask coordinator to intervene

---

## Claiming Strategies

### Standard Claim

```
claim(worker_id=worker_id, task=task_id)
```

### Force Claim (use sparingly)

```
# Takes task from another worker (e.g., they disconnected)
claim(worker_id=worker_id, task=task_id, force=true)
```

### Multi-Task

```
# Claim multiple tasks if you'll work on them
# (up to your max_claims limit)
claim(worker_id=worker_id, task=task_1)
claim(worker_id=worker_id, task=task_2)
```

---

## Handling Failures

### Task Failed

```
# If you can't complete the task:
update(worker_id=worker_id, task=task_id, state="failed")

# Attach explanation
attach(task=task_id, name="failure-reason",
       content="Could not compile: missing dependency X")
```

### Need to Handoff

```
# Release back to pending for another worker
update(worker_id=worker_id, task=task_id, state="pending")

# Attach progress notes
attach(task=task_id, name="handoff-notes",
       content="Completed steps 1-3, step 4 needs database expertise")
```

### Blocked by Dependency

```
# Check what's blocking
get(task=task_id)  # Look at blockedBy field

# Either wait, or help complete the blocker
list_tasks(ready=true, worker_id=worker_id)  # Find other work
```

---

## Cost Tracking

Track your resource usage for project accounting:

```
log_cost(
  worker_id=worker_id,
  task=task_id,
  tokens_in=1500,      # Input tokens
  tokens_out=800,      # Output tokens
  tokens_cached=200,   # Cache hits
  tokens_thinking=500, # Reasoning tokens
  cost_usd=0.08        # Total cost
)
```

Call `log_cost` periodically or at task completion.

---

## Attachment Patterns

### Progress Logs

```
attach(task=task_id, name="progress-log",
       content="Step 1: Done\nStep 2: In progress",
       mime="text/plain")
```

### Code Snippets

```
attach(task=task_id, name="implementation",
       content="fn auth() { ... }",
       mime="text/x-rust")
```

### File References

```
attach(task=task_id, name="output-file",
       file="./output/report.json")
```

### Binary/Large Files

```
attach(task=task_id, name="screenshot",
       content=base64_data,
       mime="image/png",
       store_as_file=true)  # Saves to .task-graph/media/
```

---

## Checklist

Before starting work:
- [ ] Connected with appropriate tags
- [ ] Found task via `list_tasks(ready=true)`
- [ ] Claimed the task
- [ ] Read task description and attachments
- [ ] Checked file locks

During work:
- [ ] Reporting progress via `thinking()`
- [ ] Locked files before editing
- [ ] Tracking costs

After completing:
- [ ] Released file locks
- [ ] Attached outputs/notes
- [ ] Logged final costs
- [ ] Updated task to completed state

---

## Anti-Patterns

| Avoid | Why | Instead |
|-------|-----|---------|
| Claiming without capacity | Blocks others | Only claim what you'll do |
| Silent work | Looks stuck | Report progress |
| Editing unlocked files | Conflicts | Always `claim_file` first |
| Abandoning tasks | Blocks progress | Update state if can't finish |
| Ignoring dependencies | Task will fail | Check `ready=true` |

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference |
| `task-graph-coordinator` | Understand task structure |
| `task-graph-reporting` | Check your metrics |
