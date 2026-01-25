# Task-Graph MCP: Coordinator Experience & Recommendations

## Session Summary

Coordinated 16 worker agents completing 15 tasks:
- **rust-worker**: DB layer rename (19 files, 67 tests)
- **rust-worker-2**: Thinking signature refactor
- **rust-worker-3**: Attach signature refactor
- **rust-worker-4**: Petname auto-generation
- **rust-worker-5**: Disconnect signature refactor
- **rust-worker-6**: Claim_file signature refactor
- **rust-worker-7**: Block → link signature refactor
- **rust-worker-8**: List_agents signature refactor
- **rust-worker-10**: Release_file signature refactor
- **rust-worker-11**: List_files signature refactor
- **rust-worker-12**: Get signature refactor
- **rust-worker-13**: Create signature refactor (68 tests passing)
- **rust-worker-15**: FTS5 migration (SQL prepared, manually applied)
- **docs-worker**: Skills documentation updates
- **coordinator**: Tool layer renaming, instructions slimming

## What Works Well

### File Coordination
- `claim_file()` / `release_file()` prevents conflicts
- Workers check `list_files()` before editing
- Advisory locks with reasons visible to others

### Progress Visibility
- `thinking()` updates show real-time worker status
- `list_agents` dashboard shows all workers + current thoughts
- `list_tasks(status="in_progress")` shows active work

### Task Dependencies
- `join_mode: "then"` for sequential subtasks
- `join_mode: "also"` for parallel work
- `blocked_by` prevents premature claiming

### Scope Discovery
- Workers naturally expand scope when needed (rust-worker: 2 files -> 19)
- Task attachments share research between workers

## Pain Points & Recommendations

### 1. No Heartbeat Staleness Detection
**Problem**: Can't detect if a worker hangs or crashes.

**Recommendation**: Add `stale_threshold_ms` config and:
```rust
// In list_agents response
"heartbeat_age_ms": now - last_heartbeat,
"is_stale": heartbeat_age_ms > stale_threshold
```

### 2. No Auto-Reassignment
**Problem**: Stale workers hold tasks hostage.

**Recommendation**: Add `auto_release_stale_tasks` option that:
- Marks tasks as `pending` when owner goes stale
- Releases file locks from stale workers
- Logs the reassignment for audit

### 3. Manual Worker Cycling
**Problem**: Coordinator manually spawns replacement workers.

**Recommendation**: Add worker pool concept:
```
create_worker_pool(min=2, max=5, tags=["rust"])
```
Pool auto-dispatches to `ready=true` tasks matching tags.

### 4. High Context Loading Cost
**Problem**: Each worker spends ~27k tokens loading context before work.

**Recommendation**:
- Warm worker pool that stays connected
- Skill-based worker templates with pre-loaded context
- Consider MCP session resumption

### 5. No Native Completion Notification
**Problem**: Task-graph doesn't notify coordinator when workers complete.

**Recommendation**: Add `subscribe` tool:
```
subscribe(agent="coordinator", events=["task_completed", "worker_stale"])
```
Or webhook/callback support.

### 6. Missing Time Estimates
**Problem**: Can't predict when parallel work will complete.

**Recommendation**: Track historical `time_actual_ms` by task tags, use for estimates:
```
"estimated_completion_ms": avg_time_for_similar_tasks
```

### 7. No Cost Aggregation View
**Problem**: `log_cost` exists but no rollup view.

**Recommendation**: Add `get_costs` tool:
```
get_costs(group_by="worker" | "task" | "tag", since=timestamp)
```

### 8. Subagent Tool Permission Denial (BUG)
**Problem**: Worker subagents spawned via Claude Code's Task tool intermittently lose Write/Edit/Bash tool permissions mid-session. Workers report "Write tool was denied" or "All file write operations blocked."

**Observed behavior**:
- Workers connect and claim tasks successfully
- Workers can read files and report progress via `thinking()`
- When attempting to write, Edit/Write/Bash tools are auto-denied
- Workers save their work as task attachments for manual completion
- Affected workers: rust-worker-9, rust-worker-14, rust-worker-15, rust-worker-16

**Workaround**: Workers attach completed code/SQL to tasks. Coordinator manually applies changes.

**Recommendation**:
- Investigate Claude Code's subagent permission inheritance
- Consider explicit `allowed_tools` parameter when spawning workers
- May be related to tool approval prompts not being shown to subagents

## API Refinements Observed

### Parameter Naming
- `agent` -> `worker_id` transition happened mid-session
- Caused temporary confusion
- **Lesson**: Breaking changes need migration period

### Bulk Operations
- Workers wanted `file: string | string[]` for batch operations
- Pattern should be consistent across all tools

### Replace vs Append Semantics
- Attachment "same name replaces" behavior was implemented
- Should be explicit in all mutating operations

## Suggested New Tools

| Tool | Purpose |
|------|---------|
| `subscribe` | Event notifications to coordinator |
| `get_costs` | Aggregated cost/metrics view |
| `worker_pool` | Auto-dispatch management |
| `reassign` | Force task to different worker |
| `health` | System health + stale worker detection |

## Conclusion

Task-graph works well for coordinating 3-6 parallel workers on independent tasks. The file locking and progress visibility are solid. Main gaps are around lifecycle management (staleness, auto-reassignment) and reducing coordination overhead.

Priority improvements:
1. Heartbeat staleness in `list_agents`
2. Auto-release stale tasks
3. Cost aggregation view
4. Event subscription for coordinators
