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

# 2b. Or search for specific tasks
search(query="authentication backend")

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

## Work Models: Pull vs Push

Workers can receive work in two ways:

### Pull Model (Self-Selection)

You find and claim tasks from the ready pool:

```
list_tasks(ready=true, worker_id=worker_id)
claim(worker_id=worker_id, task=task_id)
```

### Push Model (Assignment)

A coordinator assigns tasks directly to you. The task appears with:
- `status: "assigned"` - ready for you to start
- `owner_agent: your_id` - you're the assignee

```
# Check for assigned tasks
list_tasks(owner=worker_id, status="assigned")

# Start work (transitions to in_progress)
claim(worker_id=worker_id, task=task_id)
```

### Check Both Sources

For maximum work discovery, check both assigned and ready tasks:

```
# 1. Check for assigned tasks (push)
list_tasks(owner=worker_id, status="assigned")

# 2. Check ready pool (pull)
list_tasks(ready=true, worker_id=worker_id)
```

---

## Work Loop

```
┌─────────────────────────────────────────────────────┐
│ 1. FIND WORK                                        │
│    # Check for assigned tasks first (push)          │
│    list_tasks(owner=worker_id, status="assigned")   │
│                                                     │
│    # Then check ready pool (pull)                   │
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
│    b. Mark files you'll edit:                       │
│       mark_file(worker_id=worker_id, file="src/x.rs")│
│                                                     │
│    c. Report progress frequently:                   │
│       thinking(worker_id=worker_id,                 │
│                thought="Implementing X...")         │
│                                                     │
│    d. Do the actual work                            │
├─────────────────────────────────────────────────────┤
│ 4. FINISH                                           │
│    a. Unmark files:                                 │
│       unmark_file(worker_id=worker_id, file="src/x.rs")│
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

Multiple workers may edit the same codebase. Advisory locks prevent conflicts and enable coordination.

### Recommended Discipline

**Mark all files before you begin working on any of them.**

Before writing any code, identify every file you'll need to modify and mark them all upfront in a single call. This prevents mid-task conflicts where another worker marks a file you need, forcing you to wait or redo work.

```
# Good: Mark all files upfront with task-level reason
mark_file(worker_id=id,
          file=["src/types.rs", "src/handler.rs", "tests/handler_test.rs"],
          reason="Add Status enum and update handlers")
# Now begin implementation...

# Bad: Mark as you go (risks mid-task conflicts)
mark_file(worker_id=id, file="src/types.rs", reason="...")
# Edit types.rs...
mark_file(worker_id=id, file="src/handler.rs", reason="...")  # May already be marked!
```

### Coordination Model

Marks include a **reason** describing what you're doing:

```
mark_file(worker_id=id, file="src/types.rs",
          reason="Renaming state to status throughout")
```

When you mark a file, you see what others are doing:

1. **On mark**: If already marked, you get a warning with their ID and reason
2. **Via polling**: `mark_updates` shows marks/removals with reasons

Example scenarios:
```
# Scenario 1: Refactoring
Worker A marks types.rs: "Renaming state to status"
Worker B tries to mark types.rs → sees A's reason
Worker B decides to wait, OR uses 'status' not 'state'

# Scenario 2: Test failure
Test fails in auth.rs
Worker A marks auth.rs: "Fixing null check in validate()"
Worker B sees the mark → knows it's being handled
Worker B moves on to other work instead of duplicating effort
```

This lets you:
- **Understand** the nature of others' changes
- **Know** if a problem is already being addressed
- **Work around** changes intelligently
- **Avoid** duplicating effort or reverting work

The key insight: reasons communicate intent, enabling smart coordination.

### Workflow

```
# 1. Check if file is marked
list_marks(files=["src/auth.rs"])

# 2. Check for recent activity
mark_updates(worker_id=worker_id)

# 3. Mark the file
mark_file(worker_id=worker_id, file="src/auth.rs",
          reason="Adding auth middleware")

# 4. Do your work...

# 5. Unmark when done
unmark_file(worker_id=worker_id, file="src/auth.rs",
            reason="Auth middleware complete")
```

### Handling Conflicts

If file is marked by another worker:

1. **Wait** - Use `mark_updates(timeout=30000)` to long-poll for removal
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
claim(worker_id=worker_id, task=task_1)
claim(worker_id=worker_id, task=task_2)
```

---

## Scope Expansion: Moving Children

When a task's scope expands, you may need to create a sibling task and move some children to it. Use `relink` for atomic dependency updates:

```
# Scenario: Task "Backend" has children A, B, C, D
# Need to split: keep A, B in Backend; move C, D to new "Database" sibling

# 1. Create new sibling task
create(title="Database", parent=grandparent_id)
→ new_task_id

# 2. Atomic move - unlinks from old parent, links to new parent
relink(
  prev_from="backend-task-id",
  prev_to=["child-c", "child-d"],
  from=new_task_id,
  to=["child-c", "child-d"],
  type="contains"
)
# Result: Backend has A, B; Database has C, D
```

**Why relink vs unlink+link?**
- Single transaction: either all changes succeed or none do
- No intermediate state where children are orphaned
- Validates constraints (single parent, no cycles) before committing

---

## Finding Tasks with Search

Use `search` for powerful full-text search across tasks and attachments:

### Basic Search
```
search(query="authentication")
# Returns ranked results with highlighted snippets
```

### Search with Filters
```
# Find pending tasks only
search(query="backend API", status_filter="pending")

# Include attachment content in search
search(query="error handling", include_attachments=true)

# Limit results
search(query="refactor", limit=5)
```

### When to Use Search vs list_tasks

| Use Case | Tool |
|----------|------|
| Find ready tasks for your tags | `list_tasks(ready=true)` |
| Find tasks by keyword | `search(query="keyword")` |
| Filter by status only | `list_tasks(status="pending")` |
| Find tasks with specific content | `search(query="...")` |
| Check your claimed tasks | `list_tasks(owner=worker_id)` |
| Find tasks mentioning a topic | `search(query="topic")` |

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
- [ ] Checked for assigned tasks: `list_tasks(owner=worker_id, status="assigned")`
- [ ] Checked ready pool: `list_tasks(ready=true)`
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
| Editing unmarked files | Conflicts | Always `mark_file` first |
| Abandoning tasks | Blocks progress | Update state if can't finish |
| Ignoring dependencies | Task will fail | Check `ready=true` |

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference |
| `task-graph-coordinator` | Understand task structure |
| `task-graph-reporting` | Check your metrics |
