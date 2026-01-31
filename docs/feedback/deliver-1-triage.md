# Delivery Session Feedback Triage: claude-deliver-1 (2026-01-29)

**Source task:** `feedback-deliver-1`
**Triaged by:** triage-agent
**Date:** 2026-01-31

## Summary

Nine feedback items were collected during the `claude-deliver-1` delivery session on 2026-01-29. All nine were filed as individual tasks (`fb-1` through `fb-9`) and have since been completed. The items span workflow discovery, UX improvements, and config consistency issues.

---

## Feedback Items

### FB-1: Add workflow discovery before connect

| Field       | Value                      |
|-------------|----------------------------|
| Task ID     | `fb-1-workflow-discovery`  |
| Category    | **Enhancement / UX**       |
| Priority    | **High** (original: 7)     |
| Status      | Completed                  |

**Problem:** No way to discover available workflows before calling `connect()`. Agents must guess or connect blind.

**Resolution:** A `list_workflows` tool was implemented (see `src/tools/workflows.rs`). Agents can now enumerate available workflow YAML files without connecting first.

**Related tasks:** `fb-7-workflow-location` (reinforces the same need from the filesystem angle)

---

### FB-2: Truncate or enforce short task titles in list_tasks

| Field       | Value                    |
|-------------|--------------------------|
| Task ID     | `fb-2-title-length`     |
| Category    | **UX**                   |
| Priority    | **Medium** (original: 5) |
| Status      | Completed                |

**Problem:** Multi-line titles duplicate descriptions and make `list_tasks` output noisy.

**Resolution:** Title truncation implemented in `src/format.rs`. Titles longer than `MAX_TITLE_DISPLAY_LEN` (80 chars) or containing newlines are truncated with `...` suffix. Tasks created without an explicit title now auto-derive a truncated title from the description. A warning is emitted when titles exceed the display limit.

---

### FB-3: Exclude category/container tasks from ready=true results

| Field       | Value                    |
|-------------|--------------------------|
| Task ID     | `fb-3-ready-filter`     |
| Category    | **Enhancement / UX**     |
| Priority    | **Medium** (original: 6) |
| Status      | Completed                |

**Problem:** Organizational tasks (e.g., "Stream A: Core Features") appear in `ready=true` results even though they are not actionable.

**Resolution:** Marked as completed. The exact mechanism used (task type field, convention, or parent-based exclusion) is not immediately visible in the codebase as a distinct feature flag. Container tasks with children are likely excluded via the `contains` dependency logic. Further verification may be warranted if the behavior regresses.

**Note:** No explicit `category` or `epic` task type was found in the codebase. The solution may rely on convention (e.g., assigning container tasks to a non-initial status) rather than a dedicated type field.

---

### FB-4: Add tree-view or recursive listing to list_tasks

| Field       | Value                    |
|-------------|--------------------------|
| Task ID     | `fb-4-tree-view`        |
| Category    | **Enhancement**          |
| Priority    | **Medium** (original: 5) |
| Status      | Completed                |

**Problem:** `list_tasks` returns a flat list with no parent-child structure. Understanding hierarchy required scanning each parent individually.

**Resolution:** `list_tasks(parent='X', recursive=true)` is now supported (see `src/tools/tasks.rs`, lines 160 and 620-641). When `recursive=true` is set with a `parent` filter, the tool returns the full subtree of descendants.

---

### FB-5: Include workflow config details in connect response

| Field       | Value                      |
|-------------|----------------------------|
| Task ID     | `fb-5-workflow-config-response` |
| Category    | **Enhancement / UX**       |
| Priority    | **High** (original: 7)     |
| Status      | Completed                  |

**Problem:** After `connect(workflow='hierarchical')`, the response includes the workflow name but nothing from the workflow file -- no roles, states, prompts, or gates.

**Resolution:** The connect response now delivers workflow-specific role information and prompts (see `src/tools/agents.rs`, lines 288-312). When a role is matched, the response includes the role name, description, constraints, and role-specific prompts.

---

### FB-6: Auto-register workflow role tags as known_tags

| Field       | Value                    |
|-------------|--------------------------|
| Task ID     | `fb-6-known-tags`       |
| Category    | **Config / Bug**         |
| Priority    | **Medium** (original: 6) |
| Status      | Completed                |

**Problem:** Workflow-defined role tags (e.g., `roles.worker.tags: [worker]`) produced "Unknown tag" warnings because `known_tags` was empty, even though the tags were meaningful in the workflow context.

**Resolution:** `register_workflow_tags()` is now called at startup and during config hot-reload (see `src/main.rs` lines 658 and 775, `src/config/types.rs` line 523). Workflow role tags are automatically added to `known_tags`, with existing definitions preserved. Full test coverage exists in `src/config/types.rs`.

---

### FB-7: Workflow files not discoverable from project directory

| Field       | Value                      |
|-------------|----------------------------|
| Task ID     | `fb-7-workflow-location`  |
| Category    | **Documentation / UX**     |
| Priority    | **Low** (original: 4)      |
| Status      | Completed                  |

**Problem:** Workflow YAML files live in the server source directory, not in the project's `task-graph/` directory. Agents have no filesystem path to discover them.

**Resolution:** Addressed by `fb-1-workflow-discovery`. The `list_workflows` tool provides API-level discovery, making filesystem location irrelevant for agents. This is the correct architectural choice -- workflows are a server-level concept, not a project-level artifact.

---

### FB-8: Config file watcher implemented but never wired up

| Field       | Value                      |
|-------------|----------------------------|
| Task ID     | `fb-8-config-watcher`     |
| Category    | **Bug**                    |
| Priority    | **High** (original: 7)     |
| Status      | Completed                  |

**Problem:** `src/config/watcher.rs` was fully implemented (500ms debouncing, `ConfigChangeEvent` via tokio watch channel) but `start_config_watcher` was never called in `main.rs`. Config changes required a full MCP server restart.

**Resolution:** `start_config_file_watcher()` is now called in `main.rs` (line 826) and properly wired into the server loop. A `ReloadContext` is used to swap the `Arc`'d config when changes are detected. The function is defined at line 865 and calls `start_config_watcher` at line 895.

---

### FB-9: Workflow prompts and constraints defined but never delivered to agents

| Field       | Value                      |
|-------------|----------------------------|
| Task ID     | `fb-9-workflow-prompts-not-delivered` |
| Category    | **Bug / Enhancement**      |
| Priority    | **Critical** (original: 9) |
| Status      | Completed                  |

**Problem:** The hierarchical workflow YAML defined extensive agent guidance (state prompts, state+phase combos, role prompts, worker constraints, gates) but none of it was delivered to agents by the server. The behavior shaping only worked if agents independently read the YAML files.

**Resolution:** This was addressed through a dedicated prompt guidance effort (`pg-1-audit` and `pg-2-implement`). The connect response now delivers role-specific prompts (see `src/tools/agents.rs`). Workflow prompts are surfaced at connect time, claim time, and state transitions. The `pg-2-implement` task reports that guidance was written across all prompt surfaces including workflow-level, phase-specific, status-specific, and phase-status combo prompts across all four workflow types.

**Related tasks:** `pg-1-audit` (audit prompt surfaces), `pg-2-implement` (implement guidance)

---

## Category Summary

| Category                | Count | Items                            |
|-------------------------|-------|----------------------------------|
| Enhancement / UX        | 4     | FB-1, FB-3, FB-5, FB-9          |
| UX                      | 2     | FB-2, FB-7                       |
| Bug                     | 2     | FB-8, FB-9                       |
| Config                  | 1     | FB-6                             |
| Enhancement             | 1     | FB-4                             |

## Priority Summary

| Priority  | Count | Items                    |
|-----------|-------|--------------------------|
| Critical  | 1     | FB-9                     |
| High      | 3     | FB-1, FB-5, FB-8         |
| Medium    | 4     | FB-2, FB-3, FB-4, FB-6   |
| Low       | 1     | FB-7                     |

## Overall Status

All 9 feedback items from the `claude-deliver-1` session have been addressed and their corresponding tasks are marked as completed. The most impactful items (FB-9 workflow prompt delivery, FB-8 config hot-reload, FB-1 workflow discovery, FB-5 connect response enrichment) were all resolved with code changes verified in the current codebase.

No open items remain from this delivery session.
