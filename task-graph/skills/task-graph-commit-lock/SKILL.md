---
name: task-graph-commit-lock
description: Git commit serialization pattern using task-graph claim/release as a mutex - prevents concurrent git operations in multi-agent workflows
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: pattern
  requires: task-graph-basics
---

# Git Commit Lock Pattern

Serialize git operations across multiple concurrent agents using a well-known sentinel task as a mutex. This pattern uses existing task-graph primitives (claim/release) -- no code changes or new tools are needed.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## The Problem

When multiple agents work in the same repository concurrently, their git operations can collide:

- Two agents run `git add` + `git commit` simultaneously
- One agent's `git pull --rebase` conflicts with another's in-flight commit
- Merge conflicts arise from interleaved staging and committing

Git's index is a shared mutable resource. Without serialization, concurrent agents produce merge conflicts, lost commits, or corrupted state.

---

## The Solution: Sentinel Task as Mutex

Create a well-known task with a reserved ID (e.g., `_lock:git-commit`) that serves as a mutex. Agents must claim this task before performing git operations and release it immediately after.

### How It Works

The task-graph claiming system already provides the semantics we need:

1. **`claim()`** transitions a task to `working` (a timed state), which sets exclusive ownership. If another agent already owns it, the claim **fails**.
2. **`update(status="pending")`** releases ownership by transitioning to a non-timed state, making the task claimable again.

This is exactly a **mutex lock/unlock** pattern.

---

## Setup: Create the Sentinel Task

The coordinator (or any agent during project setup) creates the sentinel task once:

```
create(
  id="_lock:git-commit",
  title="[LOCK] Git Commit Serialization",
  description="Sentinel task for serializing git operations. Claim before git add/commit/push, release after. Do NOT complete or delete this task.",
  tags=["lock", "infrastructure"],
  priority=0
)
```

**Important properties of the sentinel task:**
- **Fixed well-known ID**: `_lock:git-commit` -- all agents reference this exact ID
- **Never completed**: It stays in `pending`/`working` cycle forever
- **Priority 0**: Should not appear in normal task listings as actionable work
- **Tag `lock`**: Clearly identifies it as infrastructure, not real work

---

## Protocol: Acquire, Commit, Release

### Step-by-Step

```
# 1. ACQUIRE the lock (claim the sentinel task)
claim(worker_id=your_id, task="_lock:git-commit")
# If this fails -> another agent is committing. Wait and retry.

# 2. PERFORM git operations (you have exclusive access)
git add <files>
git commit -m "your message"
git push  # optional

# 3. RELEASE the lock (return sentinel to pending)
update(worker_id=your_id, task="_lock:git-commit", status="pending",
       reason="Git operations complete")
# The sentinel is now available for other agents.
```

### Pseudocode with Retry

```
MAX_RETRIES = 5
RETRY_DELAY_MS = 2000

for attempt in range(MAX_RETRIES):
    try:
        # Acquire lock
        claim(worker_id=my_id, task="_lock:git-commit")

        # Protected section - git operations
        try:
            git add <staged_files>
            git commit -m "message"
            # optional: git push
        finally:
            # ALWAYS release, even on failure
            update(worker_id=my_id, task="_lock:git-commit",
                   status="pending", reason="Released after git ops")

        break  # Success - exit retry loop

    except ClaimFailed:
        # Another agent holds the lock
        thinking(agent=my_id,
                 thought=f"Waiting for git lock (attempt {attempt+1}/{MAX_RETRIES})")
        sleep(RETRY_DELAY_MS * (attempt + 1))  # Linear backoff

raise Error("Could not acquire git lock after retries")
```

---

## Key Semantics

### Why This Works

| Property | Task-Graph Behavior | Mutex Equivalent |
|----------|-------------------|------------------|
| `claim()` succeeds | Task transitions to `working`, owner set | Lock acquired |
| `claim()` fails | Task already owned by another agent | Lock contention |
| `update(status="pending")` | Owner cleared, task returns to pool | Lock released |
| Agent disconnects | Stale reaper releases claims after timeout | Lock timeout / deadlock recovery |
| `claim(force=true)` | Takes ownership regardless | Force-unlock (coordinator recovery) |

### Built-in Deadlock Protection

If an agent crashes or disconnects while holding the lock:

1. The agent's heartbeat stops
2. After the stale timeout (default: 5 minutes), the task-graph server evicts the stale worker
3. The eviction releases all claims, including the sentinel task
4. The sentinel returns to `pending`, and other agents can claim it

This provides automatic deadlock recovery with no additional infrastructure.

---

## Variations

### Read-Write Lock (Advanced)

For scenarios where multiple agents can read concurrently but writes need exclusivity:

```
# Write lock (exclusive, as above)
_lock:git-commit     # Only one writer at a time

# Read coordination (advisory, via file marks)
mark_file(worker_id=id, file=".git/index", reason="Reading git state")
# Multiple readers can mark simultaneously - marks are advisory
```

### Per-Branch Locks

For repositories with multiple active branches:

```
# Branch-specific sentinel tasks
_lock:git-commit:main
_lock:git-commit:feature-x
_lock:git-commit:hotfix-y
```

### Scoped Lock (Broader Git Operations)

If you need to protect `git pull --rebase` or other operations beyond commit:

```
# Use a broader lock
_lock:git-ops       # Covers pull, rebase, merge, commit, push

# Or layer locks
_lock:git-commit    # Just add/commit
_lock:git-sync      # Pull/push/rebase (claim BOTH for full protection)
```

---

## Integration with Workflow Prompts

The hierarchical and swarm workflow configs include commit lock guidance in their `working` state prompts. Workers entering the `working` state are reminded to:

1. Acquire the git lock before committing
2. Keep the lock held for the minimum time necessary
3. Always release the lock, even if the commit fails

---

## Coordinator Setup Checklist

When setting up a multi-agent project:

- [ ] Create sentinel task: `create(id="_lock:git-commit", title="[LOCK] Git Commit Serialization", tags=["lock", "infrastructure"], priority=0)`
- [ ] Verify sentinel exists: `get(task="_lock:git-commit")`
- [ ] Inform workers about the protocol (attach to parent task or use this skill)
- [ ] Consider branch-specific locks if workers operate on different branches

---

## Anti-Patterns

| Avoid | Why | Instead |
|-------|-----|---------|
| Completing the sentinel | Removes it from the lock cycle | Only use `pending`/`working` transitions |
| Holding lock during long operations | Blocks all other agents' commits | Stage files first, then lock-commit-unlock |
| Forgetting to release | Deadlocks until stale timeout | Always release in a finally/cleanup block |
| Skipping the lock | Merge conflicts, lost work | Always acquire before git operations |
| Using `force=true` casually | Breaks another agent's commit | Only coordinators should force-unlock |

---

## Troubleshooting

### Lock appears stuck

```
# Check who holds it
get(task="_lock:git-commit")
# Look at worker_id field

# Check if that worker is still alive
list_agents()

# If worker is gone, force-release (coordinator only)
update(worker_id=coordinator_id, task="_lock:git-commit",
       status="pending", force=true, reason="Force-releasing stuck lock")
```

### Sentinel task missing

```
# Recreate it
create(
  id="_lock:git-commit",
  title="[LOCK] Git Commit Serialization",
  description="Sentinel task for serializing git operations.",
  tags=["lock", "infrastructure"],
  priority=0
)
```

### Agent keeps failing to acquire

```
# Check for stale claims
get(task="_lock:git-commit")

# If claimed by a disconnected agent, the stale reaper will
# release it within the stale_timeout (default: 5 min).
# For faster recovery, a coordinator can force-release.
```

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference and connection workflow |
| `task-graph-worker` | Full worker lifecycle |
| `task-graph-coordinator` | Setting up multi-agent projects |
