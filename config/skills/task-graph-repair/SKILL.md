---
name: task-graph-repair
description: Repair utilities for task-graph-mcp - fix orphaned tasks, broken dependencies, stale claims, disconnected agents, and data inconsistencies
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: repair
  requires: task-graph-basics
---

# Task Graph Repair

Diagnose and fix common issues: orphaned tasks, broken dependencies, stale claims, disconnected agents, and data inconsistencies.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Quick Start

```
# 1. Connect with repair capabilities
connect(name="repair-agent", tags=["repair", "admin"])
→ agent_id

# 2. Run diagnostics
list_tasks(format="markdown")        # Overview
list_tasks(blocked=true)             # Blocked tasks
list_agents(format="markdown")       # Agent status

# 3. Fix issues as discovered
# (See specific repair procedures below)
```

---

## Diagnostic Checklist

Run these queries to identify issues:

| Query | Looking For |
|-------|-------------|
| `list_tasks(blocked=true)` | Tasks stuck on deps |
| `list_tasks(status="working")` | Potentially stale work |
| `list_agents()` | Disconnected/stale agents |
| `list_marks()` | Abandoned file marks |
| `list_tasks(parent="null")` | Unexpected root tasks |

---

## Issue: Orphaned Tasks

**Symptoms:** Tasks with no parent that shouldn't be root tasks.

**Diagnosis:**

```
# Find all root tasks
list_tasks(parent="null")

# Identify which are intentional vs orphaned
# Orphaned = was child, parent deleted without cascade
```

**Repair Options:**

```
# Option A: Re-parent to correct parent
update(agent=agent_id, task=orphan_id, parent=correct_parent_id)

# Option B: Delete if no longer needed
delete(task=orphan_id, cascade=true)

# Option C: Make intentional root
# (No action needed, just document)
```

---

## Issue: Broken Dependencies

**Symptoms:** Task blocked by non-existent task, or circular dependency.

**Diagnosis:**

```
# Find blocked tasks
list_tasks(blocked=true)

# For each, check blockers exist
get(task=blocked_task_id)  # Look at blockedBy field

# Try to fetch each blocker
get(task=blocker_id)  # Will fail if deleted
```

**Repair:**

```
# Remove reference to deleted blocker
unblock(blocker=deleted_task_id, blocked=stuck_task_id)

# For circular deps (rare, should be rejected):
# Break the cycle by removing one edge
unblock(blocker=task_a, blocked=task_b)
```

---

## Issue: Stale Claims

**Symptoms:** Task claimed by agent that disconnected or crashed.

**Diagnosis:**

```
# Find claimed tasks
list_tasks(status="working")

# Check agent status
list_agents()

# Look for:
# - last_heartbeat far in past
# - agent not in list (disconnected)
```

**Repair:**

```
# Option A: Force-claim for yourself or another agent
claim(agent=new_agent_id, task=stale_task_id, force=true)

# Option B: Release back to pool
release(agent=your_agent_id, task=stale_task_id, state="pending")
# Note: You can release tasks you don't own with force

# Option C: If work was partially done
# Add handoff notes first
attach(task=task_id, name="handoff",
       content="Previous agent disconnected. Work state: ...")
release(agent=your_agent_id, task=task_id, state="pending")
```

---

## Issue: Abandoned File Locks

**Symptoms:** Files locked by disconnected agents.

**Diagnosis:**

```
# List all marks
list_marks()

# Cross-reference with active agents
list_agents()

# Marks by agents not in list = abandoned
```

**Repair:**

```
# Unmark the file (as repair agent)
unmark_file(agent=your_agent_id, file=marked_file,
            reason="Unmarked by repair: original agent disconnected")
```

---

## Issue: Stuck in Terminal State

**Symptoms:** Need to reopen completed/cancelled task.

**Note:** Terminal states (`completed`, `cancelled`) have no exits by default.

**Repair Options:**

```
# Option A: Create new task instead
create(title="Reopen: Original Title",
       description="Continuation of task X which was closed prematurely")

# Option B: If custom states configured, add exit
# (Requires config change - see task-graph-mcp docs)

# Option C: Direct DB intervention (last resort)
# Not recommended - use task-graph tools
```

---

## Issue: Inconsistent Hierarchy

**Symptoms:** Task tree has gaps or unexpected structure.

**Diagnosis:**

```
# Get full tree
get(task=root_id, children=true, format="markdown")

# Check for:
# - Missing intermediate parents
# - Tasks at wrong level
# - Sibling order issues
```

**Repair:**

```
# Fix parent reference
update(agent=agent_id, task=misplaced_id, parent=correct_parent_id)

# Note: Direct parent updates may need DB access
# Alternative: recreate task in correct location
new_task = create(parent=correct_parent, ...)
# Copy attachments
# Delete old task
delete(task=old_task_id)
```

---

## Issue: Missing Time Tracking

**Symptoms:** `time_actual_ms` is 0 for completed tasks.

**Diagnosis:**

```
# Check state history
get_state_history(task=task_id)

# Should show transitions through timed states
# If missing working, time wasn't tracked
```

**Repair:**

```
# Can't retroactively add time tracking
# But can add estimated time to attachment

attach(task=task_id, name="time-estimate",
       content="Estimated actual time: 2 hours",
       mime="text/plain")

# For future: ensure workers use claim() which sets working
```

---

## Issue: Duplicate Tasks

**Symptoms:** Same work represented multiple times.

**Diagnosis:**

```
# Search for similar titles
list_tasks(format="markdown")
# Manually identify duplicates

# Or: check attachments for source IDs (if migrated)
attachments(task=task_id, content=true)
```

**Repair:**

```
# 1. Identify authoritative task (most progress)
# 2. Merge information via attachments
attach(task=keep_id, name="merged-from",
       content="Merged from task {delete_id}: {notes}")

# 3. Update any dependencies pointing to duplicate
# Find tasks blocked by duplicate
# Point them to kept task
unblock(blocker=duplicate_id, blocked=dependent_id)
block(blocker=kept_id, blocked=dependent_id)

# 4. Delete duplicate
delete(task=duplicate_id, cascade=false)  # Don't delete children
```

---

## Repair Workflow

```
┌─────────────────────────────────────────────────────┐
│ 1. CONNECT AS REPAIR AGENT                          │
│    connect(tags=["repair", "admin"])                │
├─────────────────────────────────────────────────────┤
│ 2. RUN FULL DIAGNOSTIC                              │
│    • list_tasks (all statuses)                      │
│    • list_tasks(blocked=true)                       │
│    • list_agents                                    │
│    • list_marks                                     │
├─────────────────────────────────────────────────────┤
│ 3. CATEGORIZE ISSUES                                │
│    • Orphaned tasks                                 │
│    • Broken dependencies                            │
│    • Stale claims                                   │
│    • Abandoned locks                                │
│    • Other inconsistencies                          │
├─────────────────────────────────────────────────────┤
│ 4. FIX IN ORDER                                     │
│    a. File locks (unblocks agents)                  │
│    b. Stale claims (frees tasks)                    │
│    c. Broken deps (unblocks tasks)                  │
│    d. Orphaned tasks (cleans structure)             │
├─────────────────────────────────────────────────────┤
│ 5. VERIFY                                           │
│    Re-run diagnostics to confirm fixes              │
├─────────────────────────────────────────────────────┤
│ 6. DOCUMENT                                         │
│    Attach repair log to root task                   │
└─────────────────────────────────────────────────────┘
```

---

## Prevention

| Issue | Prevention |
|-------|------------|
| Orphaned tasks | Always use `cascade=true` when deleting parents |
| Stale claims | Implement heartbeat monitoring |
| Abandoned locks | Release files before disconnect |
| Broken deps | Validate blockers exist before `block()` |
| Duplicates | Check for existing before `create()` |

---

## Repair Log Template

Document repairs for audit trail:

```markdown
# Repair Log - {timestamp}

## Issues Found
1. {issue_type}: {description}
2. ...

## Actions Taken
1. {action}: {details}
   - Before: {state}
   - After: {state}
2. ...

## Verification
- [ ] No blocked tasks without valid blockers
- [ ] No stale claims
- [ ] No abandoned file locks
- [ ] Hierarchy consistent

## Notes
{any additional observations}
```

```
# Attach to root task
attach(task=root_id, name="repair-log-{date}",
       content=repair_log, mime="text/markdown")
```

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference, task trees |
| `task-graph-reporting` | Verify data integrity |
| `task-graph-migration` | Re-migrate if severe corruption |
