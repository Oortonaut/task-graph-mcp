# Gates

> **Version:** 1.0
> **Last Updated:** 2026-01-28
> **Status:** Living Document

Gates are checklists that must be satisfied before exiting a status or phase. They enforce quality and process requirements by requiring specific attachments before a task can transition to another state.

---

## Table of Contents

- [Concept](#concept)
- [Configuration](#configuration)
- [Enforcement Levels](#enforcement-levels)
- [The check_gates Tool](#the-check_gates-tool)
- [Attachment Conventions](#attachment-conventions)
- [Example Workflow](#example-workflow)
- [Integration with Status Transitions](#integration-with-status-transitions)
- [Choosing and Configuring Gates](#choosing-and-configuring-gates)
- [Best Practices](#best-practices)

---

## Concept

Gates act as quality checkpoints in your workflow. When an agent attempts to transition a task out of a status (e.g., `working` to `completed`) or out of a phase (e.g., `implement` to `review`), the system checks whether the task has the required attachments.

**Key principles:**

1. **Attachment-based**: A gate is satisfied when the task has an attachment with a matching type
2. **Exit-focused**: Gates are checked when *leaving* a status or phase, not when entering
3. **Configurable enforcement**: Each gate can allow, warn, or reject based on your workflow needs
4. **Pre-flight checking**: Agents can check gate requirements before attempting a transition

```
+-------------------------------------------------------------+
|                    Task: implement-auth                      |
|                                                             |
|  Status: working                                            |
|  Phase: implement                                           |
|                                                             |
|  Attachments:                                               |
|    - gate/commit: "abc123"                                  |
|    - impl-notes: "Added OAuth2 flow..."                     |
|                                                             |
|  Gates for exiting 'working':                               |
|    [x] gate/tests (reject) - has attachment                 |
|    [x] gate/commit (warn) - has attachment                  |
|                                                             |
|  => Can transition to 'completed'                           |
+-------------------------------------------------------------+
```

---

## Configuration

Gates are defined in your `workflows.yaml` file under the `gates` section. The key format is `status:<name>` or `phase:<name>` to specify when the gates apply.

### Basic Example

```yaml
gates:
  # Gates checked when exiting the 'working' status
  status:working:
    - type: "gate/tests"
      enforcement: reject
      description: "Attach test results before completing"
    - type: "gate/commit"
      enforcement: warn
      description: "Attach commit hash"

  # Gates checked when exiting the 'implement' phase
  phase:implement:
    - type: "gate/review"
      enforcement: warn
      description: "Attach review notes or self-review"
```

### Full Configuration Reference

```yaml
gates:
  # Status exit gates
  status:working:
    - type: "gate/tests"           # Attachment type that satisfies this gate
      enforcement: reject          # reject | warn | allow
      description: "Run tests and attach results"

    - type: "gate/commit"
      enforcement: warn
      description: "Attach commit hash or explain why no commit"

    - type: "gate/cost"
      enforcement: allow           # Advisory only, never blocks
      description: "Log costs with log_metrics()"

  # Phase exit gates
  phase:design:
    - type: "gate/spec"
      enforcement: reject
      description: "Attach design specification"

  phase:implement:
    - type: "gate/tests"
      enforcement: warn
      description: "Attach test results"
    - type: "gate/commit"
      enforcement: warn
      description: "Attach commit hash"

  phase:review:
    - type: "gate/approval"
      enforcement: reject
      description: "Attach review approval or rejection with notes"

  phase:test:
    - type: "gate/test-results"
      enforcement: reject
      description: "Attach comprehensive test results"
```

### Per-Workflow Gates

Named workflows (e.g., `workflow-relay.yaml`) can define their own gates that apply when workers connect with that workflow:

```yaml
# workflow-relay.yaml
name: relay
description: Specialists hand off work through phases

gates:
  status:working:
    - type: "gate/deliverable"
      enforcement: reject
      description: "Attach phase deliverables before completing"

  phase:design:
    - type: "gate/spec"
      enforcement: reject
      description: "Design specification required"
```

---

## Enforcement Levels

Each gate has an enforcement level that determines how strictly the requirement is applied:

| Level | Behavior | Use Case |
|-------|----------|----------|
| `allow` | Advisory only, never blocks. Shows in response but does not prevent transition. | Soft reminders, optional best practices |
| `warn` | Blocks transition unless `force=true`. For recommended but overridable requirements. | Standard process requirements that can be bypassed with justification |
| `reject` | Hard block, cannot be forced. Transition is rejected until gate requirements are satisfied. | Mandatory requirements (e.g., tests must pass, spec must exist) |

### Enforcement Flow

```
Agent calls: update(task="X", status="completed")

                    +------------------+
                    |  Check all gates |
                    |  for current     |
                    |  status & phase  |
                    +--------+---------+
                             |
              +--------------+--------------+
              |              |              |
              v              v              v
         +--------+    +--------+    +--------+
         | reject |    |  warn  |    | allow  |
         | gates  |    | gates  |    | gates  |
         |missing?|    |missing?|    |missing?|
         +---+----+    +---+----+    +---+----+
             |             |             |
             v             v             v
        +---------+   +---------+   +---------+
        |  ERROR  |   |force=   |   | Include |
        | Cannot  |   | true?   |   |   in    |
        | proceed |   |         |   |warnings |
        +---------+   +----+----+   +---------+
                           |
                    +------+------+
                    |             |
                    v             v
              +---------+   +---------+
              |   No    |   |   Yes   |
              |  ERROR  |   |Proceed  |
              |         |   |+ warning|
              +---------+   +---------+
```

### Examples by Enforcement Level

**Allow (advisory):**
```json
// Response includes warning but transition succeeds
{
  "success": true,
  "warnings": ["Optional gates not satisfied: gate/cost (Log costs with log_metrics())"]
}
```

**Warn (soft block):**
```json
// Without force=true
{
  "error": "Cannot exit 'working' without force=true: unsatisfied gates: gate/commit (Attach commit hash)"
}

// With force=true
{
  "success": true,
  "warnings": ["Proceeding despite unsatisfied gates (force=true): gate/commit (Attach commit hash)"]
}
```

**Reject (hard block):**
```json
// Cannot proceed regardless of force flag
{
  "error": "Cannot exit 'working': unsatisfied gates: gate/tests (Run tests and attach results)"
}
```

---

## The check_gates Tool

The `check_gates` tool allows agents to pre-flight check gate requirements before attempting a status or phase transition.

### Usage

```javascript
check_gates(task="task-id")
```

### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `task` | string | Yes | Task ID to check gates for |

### Response Format

```json
{
  "status": "pass" | "warn" | "fail",
  "gates": [
    {
      "type": "gate/tests",
      "enforcement": "reject",
      "description": "Run tests and attach results",
      "satisfied": false
    },
    {
      "type": "gate/commit",
      "enforcement": "warn",
      "description": "Attach commit hash",
      "satisfied": false
    }
  ]
}
```

### Status Values

| Status | Meaning |
|--------|---------|
| `pass` | All gates satisfied, OR only `allow`-level gates are unsatisfied |
| `warn` | Some `warn`-level gates are unsatisfied (would need `force=true` to proceed) |
| `fail` | Some `reject`-level gates are unsatisfied (cannot proceed) |

### Example Usage Pattern

```javascript
// Before completing a task, check if gates are satisfied
const result = check_gates(task="implement-auth");

if (result.status === "fail") {
  // Must satisfy reject-level gates first
  for (const gate of result.gates.filter(g => g.enforcement === "reject")) {
    console.log(`Must satisfy: ${gate.type} - ${gate.description}`);
  }

  // Attach required items
  attach(task="implement-auth", type="gate/tests", content="All 47 tests passed");

} else if (result.status === "warn") {
  // Can proceed with force=true, or satisfy the gates
  for (const gate of result.gates.filter(g => g.enforcement === "warn")) {
    console.log(`Recommended: ${gate.type} - ${gate.description}`);
  }

  // Either attach the items or use force=true
  update(task="implement-auth", status="completed", force=true);

} else {
  // All gates satisfied, can proceed normally
  update(task="implement-auth", status="completed");
}
```

---

## Attachment Conventions

Gates are satisfied by attachments with matching types. Here are common conventions:

### Standard Gate Types

| Attachment Type | Purpose | Example Content |
|-----------------|---------|-----------------|
| `gate/tests` | Test results or explanation | `"All 47 tests passed"` or `"No tests needed - pure refactor"` |
| `gate/commit` | Commit hash or reason for no commit | `"abc123def"` or `"No code changes - documentation only"` |
| `gate/review` | Review notes or self-review | `"Self-review: checked error handling, added edge case tests"` |
| `gate/spec` | Design specification | `"## API Design\n### Endpoints..."` |
| `gate/approval` | Review approval with notes | `"Approved. Minor suggestion: consider caching."` |
| `gate/test-results` | Detailed test results | `"Integration tests: 12/12 passed\nE2E tests: 5/5 passed"` |
| `gate/cost` | Cost tracking confirmation | `"Logged via log_metrics()"` |
| `gate/deliverable` | Phase-specific deliverable | Varies by phase |

### Attaching Gate Artifacts

```javascript
// Attach test results
attach(
  task="implement-auth",
  type="gate/tests",
  content="```\nRunning 47 tests\n...\nAll tests passed in 2.3s\n```"
);

// Attach commit hash
attach(
  task="implement-auth",
  type="gate/commit",
  content="Commit: abc123def456\nBranch: feature/auth-oauth2"
);

// Attach review notes
attach(
  task="implement-auth",
  type="gate/review",
  content="Self-review completed:\n- [x] Error handling\n- [x] Input validation\n- [x] Test coverage > 80%"
);
```

### Explaining Gate Bypasses

When using `force=true` to bypass warn-level gates, document why:

```javascript
// First, document why the gate is being bypassed
attach(
  task="quick-fix",
  type="note",
  content="Bypassing gate/tests: Hotfix for production issue. Tests will be added in follow-up task TASK-456."
);

// Then proceed with force
update(task="quick-fix", status="completed", force=true,
       reason="Hotfix - tests in follow-up");
```

---

## Example Workflow

Here is a complete example of an agent working on a task with gates:

### 1. Claim and Start Work

```javascript
// Connect as a worker
connect(worker_id="worker-11", workflow="solo");

// Claim the task
claim(worker_id="worker-11", task="implement-auth");

// Set the implementation phase
update(worker_id="worker-11", task="implement-auth", phase="implement");
```

### 2. Do the Work

```javascript
// Keep progress updated
thinking(agent="worker-11", thought="Implementing OAuth2 flow...");

// ... actual implementation work ...

thinking(agent="worker-11", thought="Writing tests...");

// ... write tests ...

thinking(agent="worker-11", thought="Committing changes...");

// ... git commit ...
```

### 3. Check Gates Before Completing

```javascript
// Pre-flight check before attempting to complete
const result = check_gates(task="implement-auth");
// Returns: { status: "fail", gates: [{ type: "gate/tests", ... }, { type: "gate/commit", ... }] }
```

### 4. Satisfy the Gates

```javascript
// Attach test results
attach(
  task="implement-auth",
  type="gate/tests",
  content="47 tests passed, 0 failed, coverage: 87%"
);

// Attach commit hash
attach(
  task="implement-auth",
  type="gate/commit",
  content="abc123def - Add OAuth2 authentication flow"
);

// Check again
const result2 = check_gates(task="implement-auth");
// Returns: { status: "pass", gates: [] }
```

### 5. Complete the Task

```javascript
// Now we can complete without issues
update(
  worker_id="worker-11",
  task="implement-auth",
  status="completed",
  reason="OAuth2 implementation complete with tests"
);
```

### 6. Alternative: Force Through Warnings

If only warn-level gates are unsatisfied and there is a valid reason:

```javascript
// Check gates
const result = check_gates(task="quick-hotfix");
// Returns: { status: "warn", gates: [{ type: "gate/commit", enforcement: "warn", ... }] }

// Attach explanation
attach(
  task="quick-hotfix",
  type="note",
  content="Emergency hotfix - config change only, no code commit needed"
);

// Force through the warning
update(
  worker_id="worker-11",
  task="quick-hotfix",
  status="completed",
  force=true,
  reason="Config-only hotfix, no commit required"
);
```

---

## Integration with Status Transitions

Gates are automatically checked during `update()` calls that change status or phase:

### Status Transitions

```javascript
// This will check gates defined under "status:working"
update(task="X", status="completed");  // Exiting 'working' status
```

### Phase Transitions

```javascript
// This will check gates defined under "phase:implement"
update(task="X", phase="review");  // Exiting 'implement' phase
```

### Combined Transitions

When both status and phase change, gates for both are checked:

```javascript
// Checks gates for "status:working" AND "phase:implement"
update(task="X", status="completed", phase="deliver");
```

### The force Parameter

The `force` parameter on `update()` allows bypassing `warn`-level gates:

```javascript
// Without force - will fail if warn gates unsatisfied
update(task="X", status="completed");

// With force - proceeds despite warn gates (but NOT reject gates)
update(task="X", status="completed", force=true);
```

---

## Choosing and Configuring Gates

This section provides guidance on when to use gates, how to choose enforcement levels, and patterns for different workflow topologies.

### When to Use Gates

**Use gates when:**
- You need audit trails for process compliance (e.g., "tests must be attached before shipping")
- Handoffs between agents require explicit artifacts (e.g., design spec → implementation)
- You want to prevent accidental status changes without required documentation
- Quality checkpoints should be enforced consistently across all agents

**Don't use gates when:**
- Simple tasks don't need documentation overhead
- Dependencies between tasks already enforce ordering (use `blocks`/`follows` instead)
- The requirement is about *doing* something rather than *documenting* something (use subtasks instead)
- You want to track progress within a task (use `thinking()` or phase transitions)

### Choosing Enforcement Levels

| Enforcement | Choose When | Example |
|-------------|-------------|---------|
| **reject** | Skipping would cause real problems (data loss, broken builds, compliance violations) | Tests must pass before merge |
| **warn** | Important but occasionally has valid exceptions | Commit hash (not all changes need commits) |
| **allow** | Nice-to-have reminders, tracking what was/wasn't done | Cost logging |

**Decision flowchart:**

```
Is this requirement truly mandatory?
├─ Yes → Could an experienced agent ever legitimately skip it?
│        ├─ No → Use `reject`
│        └─ Yes → Use `warn`
└─ No → Is it useful to track when it's skipped?
         ├─ Yes → Use `allow`
         └─ No → Don't create a gate
```

### Gates by Workflow Topology

Different workflow patterns benefit from different gate configurations:

#### Solo Workflow
Single agent, minimal coordination. Gates primarily serve as self-discipline reminders.

```yaml
gates:
  status:working:
    - type: "gate/commit"
      enforcement: warn  # Agent can skip with explanation
      description: "Attach commit hash or note why no commit"
    - type: "gate/tests"
      enforcement: warn  # Allow skipping for non-code tasks
      description: "Attach test results if applicable"
```

**Rationale:** Solo workers have full context; use `warn` to allow judgment calls.

#### Swarm Workflow
Parallel generalists, fine-grained tasks, high throughput.

```yaml
gates:
  status:working:
    - type: "gate/summary"
      enforcement: warn
      description: "Brief summary for other agents"
  # Minimal gates - swarm prioritizes throughput
```

**Rationale:** Keep gates lightweight. Swarm tasks are atomic; heavy gates slow down the swarm. Use `allow` or minimal `warn` gates.

#### Relay Workflow
Specialists hand off through phases. Gates ensure proper handoff artifacts.

```yaml
gates:
  phase:design:
    - type: "gate/spec"
      enforcement: reject  # Implementer cannot proceed without spec
      description: "Design specification for handoff to implementer"

  phase:implement:
    - type: "gate/commit"
      enforcement: reject
      description: "Commit hash for reviewer"
    - type: "gate/impl-notes"
      enforcement: warn
      description: "Implementation notes explaining decisions"

  phase:review:
    - type: "gate/approval"
      enforcement: reject
      description: "Explicit approval or rejection with notes"
```

**Rationale:** Relay depends on clean handoffs. Use `reject` for artifacts the next specialist needs to do their job.

#### Hierarchical Workflow
Lead coordinates workers; mixed push/pull.

```yaml
gates:
  status:working:
    - type: "gate/results"
      enforcement: warn
      description: "Attach results for lead visibility"

  # Lead tasks have different requirements
  status:assigned:  # Before worker claims
    - type: "gate/spec"
      enforcement: warn
      description: "Lead should provide task specification"
```

**Rationale:** Workers report up to leads; leads provide specs down to workers. Gates enforce communication in both directions.

### Gate Granularity

**Too many gates:**
```yaml
# Anti-pattern: gate overload
gates:
  status:working:
    - type: "gate/tests"
    - type: "gate/commit"
    - type: "gate/docs"
    - type: "gate/review"
    - type: "gate/coverage"
    - type: "gate/lint"
    - type: "gate/security"
    - type: "gate/performance"
```
This creates friction and encourages agents to write meaningless content just to satisfy gates.

**Right-sized gates:**
```yaml
# Better: meaningful checkpoints
gates:
  status:working:
    - type: "gate/tests"
      enforcement: warn
      description: "Test results or explanation"
    - type: "gate/commit"
      enforcement: warn
      description: "Commit hash if code changed"
```

**Rule of thumb:** 1-3 gates per status/phase exit. If you need more, consider whether some should be subtasks instead.

### Status vs Phase Gates

| Use Status Gates | Use Phase Gates |
|------------------|-----------------|
| Requirements for *completing* work | Requirements for *transitioning* between types of work |
| Applies regardless of what kind of work | Specific to the type of work done |
| `status:working` → `completed` | `phase:implement` → `review` |

**Example:** Both can work together:

```yaml
gates:
  # Always need test results when completing
  status:working:
    - type: "gate/tests"
      enforcement: warn

  # Additional requirement when leaving implementation phase
  phase:implement:
    - type: "gate/commit"
      enforcement: warn
```

### Common Anti-Patterns

**1. Gates that don't match enforcement level:**
```yaml
# Bad: trivial requirement with hard block
- type: "gate/cost"
  enforcement: reject  # Cost logging shouldn't block completion
```

**2. Vague descriptions:**
```yaml
# Bad: what does "done" mean?
- type: "gate/done"
  description: "Must be done"

# Good: specific and actionable
- type: "gate/tests"
  description: "Run `cargo test` and attach output showing all tests pass"
```

**3. Redundant gates:**
```yaml
# Bad: gate duplicates dependency
gates:
  phase:implement:
    - type: "gate/design-approved"  # Just use a 'blocks' dependency instead

# Better: use task dependencies for ordering, gates for artifacts
dependencies:
  - from: design-task
    to: implement-task
    type: blocks
```

**4. Gates without escape hatches:**
```yaml
# Risky: no way to handle edge cases
- type: "gate/tests"
  enforcement: reject  # What about doc-only changes?

# Better: allow judgment
- type: "gate/tests"
  enforcement: warn
  description: "Attach test results, or explain why tests don't apply"
```

---

## Best Practices

1. **Start with warn, escalate to reject**: Begin with `warn` enforcement to understand your team's patterns, then escalate critical gates to `reject`.

2. **Provide clear descriptions**: Gate descriptions should tell agents exactly what is expected.

3. **Use check_gates proactively**: Have agents check gates before attempting transitions to avoid failed updates.

4. **Document bypasses**: When using `force=true`, attach a note explaining why the gate was bypassed.

5. **Keep gate types consistent**: Use a naming convention like `gate/` prefix for gate-specific attachments.

6. **Phase gates for handoffs**: Use phase gates to ensure proper handoff artifacts in relay-style workflows.

---

## Document History

| Version | Date | Changes |
|---------|------|---------|
| 1.1 | 2026-01-28 | Added "Choosing and Configuring Gates" guidance |
| 1.0 | 2026-01-28 | Initial gates documentation |

---

*This document is maintained alongside the codebase. Update it when making changes to gate functionality.*
