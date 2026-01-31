# Authorization Design

> **Version:** 0.1 (Draft)
> **Last Updated:** 2026-01-31
> **Status:** Proposal
> **Task:** intriguing-mooneye

This document proposes an authorization model for task-graph-mcp. The goal is to let project owners control which agents and users can perform specific operations within the task graph, without breaking existing deployments that have no authorization configured.

---

## Table of Contents

- [Current State Analysis](#current-state-analysis)
- [Proposed Authorization Model](#proposed-authorization-model)
- [Operations That Need Authorization Gates](#operations-that-need-authorization-gates)
- [Configuration Schema](#configuration-schema)
- [Enforcement Mechanism](#enforcement-mechanism)
- [Migration Path](#migration-path)
- [Open Questions](#open-questions)

---

## Current State Analysis

Task-graph-mcp already has several authorization-like features scattered across its codebase. Understanding these is critical to designing a coherent authorization layer.

### 1. Ownership Checks (force flag)

The `force` parameter is the primary access control mechanism today. It appears on `update`, `claim`, and `delete` operations.

**How it works:**
- When a task has a `worker_id` (owner), only that owner can modify it.
- Any other agent receives an error like `"Task is claimed by agent 'X'. Only the owner can update claimed tasks (use force=true to override)"`.
- Setting `force=true` bypasses the ownership check entirely -- there is no audit of *who* forced it or *why* beyond the optional `reason` field.

**Relevant code:** `src/db/tasks.rs` (lines 771-777 in `update_task_unified`), `src/tools/tasks.rs` (line 842), `src/tools/claiming.rs` (line 54).

**Limitation:** `force=true` is all-or-nothing. There is no way to say "agent X can force, but agent Y cannot."

### 2. Tag-Based Task Affinity (needed_tags / wanted_tags)

Tasks can declare which agents are qualified to claim them:

- `needed_tags` (AND): The claiming agent must have ALL of these tags.
- `wanted_tags` (OR): The claiming agent must have AT LEAST ONE of these tags.

**How it works:**
- Checked during `claim` and `update` (when transitioning to a timed state).
- Tags are set on the task at creation time and on the agent at `connect` time.
- `list_tasks(ready=true, agent="X")` respects tag affinity in query results.

**Relevant code:** `src/db/tasks.rs` (lines 903-921 in `update_task_unified`), `src/types.rs` (Task struct fields `needed_tags`, `wanted_tags`).

**Limitation:** Tag affinity controls *who can claim*, but not who can *view*, *create*, *delete*, or *modify metadata*. An agent that cannot claim a task can still update its title, description, priority, or tags.

### 3. Workflow Roles (roles in workflows.yaml)

Workflow configurations define roles with associated permissions:

```yaml
roles:
  lead:
    description: "Coordinates workers"
    tags: ["lead", "coordinator"]
    max_claims: 10
    can_assign: true
    can_create_subtasks: true
  worker:
    description: "Executes tasks"
    tags: ["worker"]
    max_claims: 3
```

**How it works:**
- Roles are matched by checking if an agent's tags overlap with a role's `tags` array.
- `can_assign` and `can_create_subtasks` are *declared* in the config but **not enforced** in the code. They are returned in the `connect` response as informational hints.
- `max_claims` is used to override the server-wide `claim_limit`.

**Relevant code:** `src/config/workflows.rs` (`RoleDefinition` struct, `match_role` method), `src/tools/agents.rs` (lines 289-314 in `connect`).

**Limitation:** Role permissions are advisory. Nothing prevents a "worker" from assigning tasks or creating subtasks. The `can_assign` / `can_create_subtasks` fields are dead config -- present in the schema but not checked.

### 4. Gate Enforcement (gates in workflows.yaml)

Gates enforce quality checklists before status/phase transitions. They use a three-tier enforcement model (`allow`, `warn`, `reject`) that the `force` flag interacts with.

**How it works:**
- Gates are checked when exiting a status or phase.
- `reject` gates cannot be bypassed, even with `force=true`.
- `warn` gates block unless `force=true` is provided.
- `allow` gates are advisory only.

**Relevant code:** `src/gates.rs`, `src/tools/tasks.rs` (lines 968-1071 for status gates, 1074-1205 for phase gates).

**Relationship to authorization:** Gates control *what work must be done* before a transition, not *who can do it*. However, the three-tier enforcement model (`allow` / `warn` / `reject`) is a proven pattern that should be reused for authorization.

### 5. Exclusive Locks (lock: prefix)

The file coordination system supports exclusive locks via the `lock:` prefix. These are mutual-exclusion primitives that reject (not just warn) when another agent holds the lock.

**Relevant code:** `src/tools/files.rs`, `src/db/locks.rs`.

**Relationship to authorization:** This is resource-level access control. The pattern of "check holder, reject if different agent" could be generalized.

### Summary of Gaps

| Capability | Exists Today | Gap |
|-----------|-------------|-----|
| Owner-only updates | Yes (force flag) | No granularity on who can force |
| Claim qualification | Yes (needed_tags/wanted_tags) | Only controls claiming, not other operations |
| Role definitions | Yes (workflows.yaml) | Declared but not enforced |
| Quality gates | Yes (gates system) | Controls "what" not "who" |
| Read access control | No | All agents can read all tasks |
| Create/delete control | No | Any connected agent can create or delete |
| Admin operations | No | cleanup_stale, force operations uncontrolled |

---

## Proposed Authorization Model

### Design Principles

1. **Tag-based, not identity-based.** Reuse the existing tag system for authorization. Agents declare capabilities at connect time; policies reference tags, not specific agent IDs. This keeps the system composable and avoids hardcoding identities.

2. **Default-open, opt-in restriction.** Without any `authorization` config block, the server behaves exactly as it does today. Authorization is additive -- you add rules to restrict, not to permit.

3. **Three-tier enforcement.** Reuse the proven `allow` / `warn` / `reject` pattern from gates for consistency.

4. **Local-first.** This is a local MCP server. The authorization model does not need tokens, sessions, or cryptographic identity. Agent identity is established at `connect` time via `worker_id` and `tags`, which are self-declared. The threat model is accidental misuse, not adversarial agents.

### Conceptual Model

Authorization is expressed as **policies** that match operations to tag requirements. A policy has:

- **scope**: Which operation(s) the policy applies to (e.g., `create`, `delete`, `force`, `claim`, `update.status`).
- **require_tags**: Tags the agent must have (AND logic, same as `needed_tags`).
- **any_tags**: Tags the agent must have at least one of (OR logic, same as `wanted_tags`).
- **enforcement**: How violations are handled (`allow`, `warn`, `reject`). Default: `warn`.

When an agent attempts an operation:
1. Find all policies whose scope matches the operation.
2. For each matching policy, check whether the agent's tags satisfy the requirements.
3. If any policy is unsatisfied, apply the enforcement level (skip / warn / block).
4. If no policies match the operation, allow it (default-open).

### Why Tag-Based (Not Role-Based or ACL)

**Role-based (RBAC):** Would require defining roles, assigning agents to roles, and mapping roles to permissions. Roles already exist in workflows.yaml, but they are advisory and single-match (an agent matches at most one role). RBAC adds a layer of indirection that is unnecessary for this use case.

**ACL (Access Control Lists):** Would require per-task or per-resource access lists. This is overkill for a local MCP server and creates management overhead.

**Tag-based:** Tags are already the lingua franca of task-graph-mcp. Agents have tags, tasks have tags, roles are matched by tags. Authorization policies that reference tags fit naturally into the existing conceptual model. A policy like "only agents with the `admin` tag can delete tasks" reads naturally and requires no new primitives.

In practice, tag-based authorization *can express* role-based patterns: define a policy that requires the `lead` tag for `force` operations, and any agent connecting with `tags: ["lead"]` gets that capability. But it also supports finer-grained patterns without the rigid role hierarchy.

---

## Operations That Need Authorization Gates

### Tier 1: High-Impact Operations (ship first)

| Operation | Current Control | Proposed Default | Rationale |
|-----------|----------------|------------------|-----------|
| `force` (on update/claim/delete) | None (any agent) | `warn` without required tags | Forcing ownership changes is the most dangerous operation. Today any agent can force-claim another's work. |
| `delete` (soft and hard) | Owner check only | `warn` without required tags | Deleting tasks (especially `obliterate`) is destructive and irreversible. |
| `cleanup_stale` | None | `warn` without required tags | Evicts workers and releases their claims. Should be restricted to coordinators. |
| `disconnect` (other agent) | N/A (self only) | No change needed | Agents can only disconnect themselves. |

### Tier 2: Coordination Operations

| Operation | Current Control | Proposed Default | Rationale |
|-----------|----------------|------------------|-----------|
| `assignee` (push assignment) | None beyond target existence | `warn` without required tags | Assigning work to another agent should be a coordinator privilege. |
| `create` | None | None (default-open) | Task creation is generally safe. Can be restricted per project. |
| `create_tree` | None | None (default-open) | Same as create. |
| `rename` | None | `warn` without required tags | Renaming changes the task ID across all references. |

### Tier 3: Metadata Operations

| Operation | Current Control | Proposed Default | Rationale |
|-----------|----------------|------------------|-----------|
| `update.status` (non-owner) | Owner check + force | Covered by force policy | Already gated by ownership. |
| `update.tags` | Owner check + force | Covered by force policy | Tag changes affect affinity. |
| `update.needed_tags/wanted_tags` | Owner check + force | Covered by force policy | Changes who can claim. |
| `link` / `unlink` / `relink` | None | None (default-open) | Dependency management is generally safe. |
| `attach` / `detach` | None | None (default-open) | Attachments are append-mostly. |

### Tier 4: Read Operations

| Operation | Current Control | Proposed Default | Rationale |
|-----------|----------------|------------------|-----------|
| `get` / `list_tasks` / `search` / `scan` | None | None (default-open) | Read access should remain open. Multi-agent coordination requires shared visibility. |
| `query` (raw SQL) | None | Consider restricting | Raw SQL could expose data or cause performance issues. |

---

## Configuration Schema

Authorization is configured in `config.yaml` under a new top-level `authorization` key.

### Minimal Configuration (Restrict Force Operations)

```yaml
authorization:
  policies:
    - scope: [force]
      require_tags: [lead]
      enforcement: warn
      description: "Only leads can force ownership changes"
```

### Full Configuration Example

```yaml
authorization:
  # Global default enforcement for unmatched policies.
  # "allow" means no restriction (default, backwards-compatible).
  default_enforcement: allow

  policies:
    # Only agents with "lead" or "admin" tag can force operations
    - scope: [force]
      any_tags: [lead, admin]
      enforcement: warn
      description: "Force operations require lead or admin tag"

    # Only admins can hard-delete (obliterate)
    - scope: [obliterate]
      require_tags: [admin]
      enforcement: reject
      description: "Permanent deletion requires admin tag"

    # Only leads can soft-delete
    - scope: [delete]
      any_tags: [lead, admin]
      enforcement: warn
      description: "Task deletion requires lead or admin tag"

    # Only leads can assign tasks to other agents
    - scope: [assign]
      any_tags: [lead, admin]
      enforcement: warn
      description: "Push assignment requires lead or admin tag"

    # Only admins can evict stale workers
    - scope: [cleanup_stale]
      require_tags: [admin]
      enforcement: warn
      description: "Stale worker cleanup requires admin tag"

    # Restrict rename to leads
    - scope: [rename]
      any_tags: [lead, admin]
      enforcement: warn
      description: "Task rename requires lead or admin tag"

    # Restrict raw SQL queries
    - scope: [query]
      any_tags: [lead, admin]
      enforcement: reject
      description: "Raw SQL queries require elevated access"
```

### Scope Values

The `scope` field accepts an array of operation identifiers:

| Scope Value | Maps To | Description |
|-------------|---------|-------------|
| `force` | `force=true` on update/claim/delete | Bypassing ownership checks |
| `delete` | `delete` tool | Soft-deleting tasks |
| `obliterate` | `delete` with `obliterate=true` | Permanently removing tasks |
| `assign` | `update` with `assignee` set | Push-assigning tasks |
| `create` | `create` and `create_tree` tools | Creating new tasks |
| `rename` | `rename` tool | Changing task IDs |
| `cleanup_stale` | `cleanup_stale` tool | Evicting stale workers |
| `query` | `query` tool | Raw SQL queries |
| `update.tags` | Changing `needed_tags` or `wanted_tags` | Modifying claim affinity |
| `claim` | `claim` tool (beyond tag affinity) | Additional claim restrictions |

### Config Type Definition (Rust)

```rust
/// Authorization policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPolicy {
    /// Operation scopes this policy applies to.
    pub scope: Vec<String>,

    /// Tags the agent must have ALL of (AND logic).
    #[serde(default)]
    pub require_tags: Vec<String>,

    /// Tags the agent must have AT LEAST ONE of (OR logic).
    #[serde(default)]
    pub any_tags: Vec<String>,

    /// Enforcement level: allow, warn, reject.
    #[serde(default)]
    pub enforcement: GateEnforcement,  // Reuse existing enum

    /// Human-readable description for error messages.
    #[serde(default)]
    pub description: String,
}

/// Authorization configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthorizationConfig {
    /// Global default enforcement when no policies match.
    /// Default: "allow" (backwards-compatible, no restrictions).
    #[serde(default)]
    pub default_enforcement: GateEnforcement,

    /// Authorization policies evaluated in order.
    #[serde(default)]
    pub policies: Vec<AuthPolicy>,
}
```

The `AuthorizationConfig` would be added to the `Config` struct in `src/config/types.rs` and wrapped in `Arc` inside `AppConfig`.

---

## Enforcement Mechanism

### Check Function

A single function evaluates authorization for an operation:

```rust
pub fn check_authorization(
    agent_tags: &[String],
    operation: &str,
    config: &AuthorizationConfig,
) -> AuthCheckResult {
    // Find all policies whose scope includes this operation
    // Check tag requirements against agent's tags
    // Return pass/warn/reject with relevant policy descriptions
}
```

The return type mirrors `GateCheckResult`:

```rust
pub struct AuthCheckResult {
    /// "pass", "warn", or "reject"
    pub status: String,
    /// Policies that were not satisfied
    pub violations: Vec<AuthViolation>,
}

pub struct AuthViolation {
    pub scope: String,
    pub enforcement: GateEnforcement,
    pub description: String,
    pub missing_tags: Vec<String>,
}
```

### Integration Points

Authorization checks are inserted at the tool handler level (`src/tools/mod.rs`), before delegating to the operation-specific function. This keeps authorization logic centralized rather than scattered across individual tool implementations.

```
Agent calls tool
    |
    v
ToolHandler::call_tool()
    |
    +-- Extract operation scope from tool name + args
    |   (e.g., "delete" + obliterate=true -> "obliterate")
    |
    +-- Look up agent tags from worker_id
    |
    +-- check_authorization(agent_tags, scope, &config.authorization)
    |
    +-- If reject: return error immediately
    +-- If warn: return error (unless force=true bypasses warn-level auth)
    +-- If pass: proceed to tool implementation
    |
    v
Tool implementation (existing code, unchanged)
```

### Interaction with force Flag

The `force` flag currently serves two purposes:
1. Bypass ownership checks (claim someone else's task)
2. Bypass warn-level gates

With authorization, `force` adds a third dimension:
3. A separate authorization scope (`force`) that can itself be restricted

This creates a clean layering:
- Ownership check: "Is this agent the owner?" (existing)
- Authorization check: "Does this agent have permission for this operation?" (new)
- Gate check: "Has the required work been done?" (existing)

When `force=true` is provided:
1. First check if the agent is authorized to force (scope: `force`).
2. If authorized, then `force` bypasses ownership and warn-level gates as today.
3. If not authorized, the force attempt itself is rejected/warned.

---

## Migration Path

### Phase 0: No-Op Default (v0.3.0)

- Add `AuthorizationConfig` to `Config` with `Default` implementation (empty policies, `default_enforcement: allow`).
- Add `authorization` field to `AppConfig`.
- No behavioral change. All existing deployments continue working identically.
- Wire the config parsing so that `authorization:` blocks in `config.yaml` are accepted without error.

### Phase 1: Enforce Existing Role Declarations (v0.3.1)

- Implement `check_authorization` function.
- Wire it into `call_tool` for `force`, `delete`, and `cleanup_stale` operations.
- Activate enforcement of `can_assign` from role definitions by translating them into implicit authorization policies at config load time:
  - `can_assign: false` on a role becomes a `warn`-level policy on `assign` scope requiring tags NOT in that role's tag set.
- Add authorization check results to tool responses (similar to `gate_warnings`).
- Document the new `authorization` config block.

### Phase 2: Full Policy Support (v0.3.2)

- Support all scope values from the table above.
- Add an `authorization` section to the `connect` response showing the agent's effective permissions.
- Add a `check_auth` tool (similar to `check_gates`) that lets agents pre-flight their permissions.
- Add authorization events to the audit trail (task_sequence / project_history).

### Phase 3: Per-Task Authorization (v0.4.0, if needed)

- Allow tasks to carry authorization overrides (e.g., `auth_tags` on a task that restrict who can view/modify it).
- This is the "ACL" escape hatch for sensitive tasks, but implemented via tags to stay consistent with the model.
- Only build this if real-world usage demonstrates the need.

### Backwards Compatibility Guarantees

1. **No `authorization` block in config:** Server behaves identically to current behavior. All operations are allowed for all agents. No warnings, no errors.
2. **Empty `authorization.policies` array:** Same as no block. No restrictions.
3. **`default_enforcement: allow`:** Explicitly stated default. Operations not covered by any policy are allowed.
4. **Existing `force=true` usage:** Continues to work unless a policy on the `force` scope is configured. Agents that currently use `force=true` will not be broken unless the project owner explicitly adds a restricting policy.

---

## Open Questions

1. **Should authorization policies be definable per-workflow (like gates)?** The current proposal puts policies in `config.yaml` globally. Alternatively, `workflow-swarm.yaml` could define its own policies that apply to agents using that workflow. This adds complexity but enables per-topology authorization (e.g., swarm workers have fewer permissions than relay specialists).

2. **Should `warn`-level authorization violations be bypassable with `force=true`?** This creates a chicken-and-egg: the agent needs `force` permission to bypass a `warn`-level auth check, but the `force` scope itself might require authorization. The proposal avoids this by making authorization checks independent of `force` -- a `warn`-level auth violation returns an error with a message like "Insufficient permissions. Your agent needs tag 'lead' for this operation." The `force` flag does not help here.

3. **Should there be a built-in "admin" concept?** Rather than a special admin role, the proposal uses tags consistently. But some operations (like `obliterate`) might warrant a hardcoded admin check. The current proposal leaves this to config -- if you want admin-only obliterate, add a policy.

4. **How should authorization interact with the `query` tool?** Raw SQL queries are powerful and potentially dangerous. Should authorization restrict which tables/operations are available, or just gate access to the tool entirely?

5. **Should denied operations be logged?** Adding authorization denial events to the audit trail would help project owners understand which agents are hitting permission boundaries. This is straightforward to implement using the existing `task_sequence` table pattern.

---

## Document History

| Version | Date | Changes |
|---------|------|---------|
| 0.1 | 2026-01-31 | Initial draft |
