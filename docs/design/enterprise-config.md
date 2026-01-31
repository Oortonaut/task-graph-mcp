# Enterprise Configuration: Research and Design

Research document for managed settings, org-wide defaults, and centralized
configuration management for distributed swarm agents.

---

## 1. Current Config Architecture

### 1.1 Layered Config System

The task-graph MCP server uses a four-tier configuration hierarchy, loaded and
merged by `ConfigLoader` in `src/config/loader.rs`:

| Tier | Priority | Source | Example Path |
|------|----------|--------|--------------|
| 0 - Defaults | Lowest | Embedded in binary | (compiled-in `Default` impls) |
| 1 - Project | Low | `$CWD/task-graph/config.yaml` | `./task-graph/config.yaml` |
| 2 - User | Medium | `~/.task-graph/config.yaml` | `~/.task-graph/config.yaml` |
| 3 - Environment | Highest | `TASK_GRAPH_*` env vars | `TASK_GRAPH_DB_PATH` |

Merging uses **deep merge** (`src/config/merge.rs`): objects merge recursively
(later keys override earlier), arrays are replaced entirely, and `null` means
"not specified" (preserves the base value). The function `deep_merge_all` folds
a list of `serde_json::Value` objects from lowest to highest tier.

### 1.2 File Discovery

`ConfigPaths::discover()` resolves paths from the environment or conventions:

- **Project dir**: `TASK_GRAPH_PROJECT_DIR` env, or `$CWD/task-graph/`
  (with fallback to deprecated `.task-graph/`).
- **User dir**: `TASK_GRAPH_USER_DIR` env, or `~/.task-graph/`.
- **Install dir**: `TASK_GRAPH_INSTALL_DIR` env, or `$CWD/config/`
  (used for built-in workflow files shipped with the binary).

Non-YAML files (skills, templates) use **first-found-wins** resolution via
`ConfigLoader::find_file()` in `src/config/files.rs`, searching from highest
tier (user) to lowest (project, deprecated project). The `list_files()` method
aggregates across tiers with higher-tier shadowing.

### 1.3 Configuration Types

The main `Config` struct (`src/config/types.rs`) contains:

- `ServerConfig` -- db path, media dir, claim limit, stale timeout, UI, etc.
- `PathsConfig` -- root, path style, Windows drive mappings, prefix mappings.
- `StatesConfig` -- initial state, disconnect state, blocking states, state
  definitions with exits and timed flags.
- `DependenciesConfig` -- dependency type definitions (blocks, follows,
  contains, etc.) with display orientation and blocking behavior.
- `AutoAdvanceConfig` -- enable/disable auto-advance when deps are satisfied.
- `AttachmentsConfig` -- preconfigured attachment keys with MIME and mode.
- `PhasesConfig` -- known phases with unknown-phase behavior.
- `TagsConfig` -- known tags with categories, unknown-tag behavior.
- `IdsConfig` -- ID word count and case style.
- `FeedbackConfig` -- agent feedback tools toggle.

### 1.4 Workflow and Overlay System

`WorkflowsConfig` (`src/config/workflows.rs`) unifies states, phases, prompts,
gates, and roles:

- **Named workflows**: `workflow-{name}.yaml` files loaded via
  `load_workflow_by_name()`, merged with defaults.
- **Overlays**: `overlay-{name}.yaml` files loaded as **raw deltas** (NOT
  merged with defaults), then applied additively via `apply_overlay()`.
  Overlays union exits, append prompts, extend gates, and first-wins roles.
- Both are searched in user dir, project dir, then install dir.

### 1.5 Hot-Reload (File Watcher)

`src/config/watcher.rs` implements file watching using `notify_debouncer_mini`:

- Watches config dir (non-recursive) and skills dir (recursive).
- Classifies changes into `ConfigYaml`, `WorkflowYaml`, `SkillsChanged`, or
  `BatchChange` events.
- Debounce of 500ms by default.
- Events are published via a `tokio::sync::watch` channel for async consumers.

---

## 2. Enterprise Needs

### 2.1 Org-Wide Defaults

Organizations running task-graph across many projects and teams need a way to
establish baseline configuration that applies everywhere unless a project
explicitly overrides it.

Examples:
- Standard state machine (e.g., requiring a "review" state before "completed").
- Required gate definitions (e.g., all tasks must pass `gate/tests` before
  completing).
- Standard tag taxonomy (e.g., `security`, `compliance`, `p0`..`p3`).
- Default workflow topology (e.g., "swarm" for all repos).
- Consistent ID format and word count across the organization.

### 2.2 Policy Enforcement

Beyond defaults, enterprises need certain configuration to be **non-overridable
at project level**. This is the distinction between "defaults" and "policies."

Examples:
- **Mandatory gates**: "gate/tests and gate/review are `enforcement: reject`"
  cannot be weakened to `warn` or `allow` by project config.
- **Required tags**: `unknown_tag: reject` with a mandated tag taxonomy.
- **Attachment key registry**: `unknown_key: reject` so teams use standardized
  attachment keys.
- **Stale timeout floor**: cannot be set below 5 minutes.
- **Claim limit cap**: cannot exceed 10 to prevent resource hogging.

### 2.3 Centralized Workflow Distribution

Large orgs want to ship standardized workflow topologies, overlays, and skills
to all agents:

- Shared `workflow-release.yaml` with required gates for release tasks.
- Common overlays like `overlay-security.yaml` or `overlay-compliance.yaml`.
- Org-approved skill definitions.
- Consistent prompts across all projects.

### 2.4 Audit and Visibility

Enterprise admins need to know:
- What org config is active on a given project.
- Whether any project configs are violating org policies.
- Which version of org config each project is using.

---

## 3. Patterns from Similar Tools

### 3.1 ESLint

ESLint uses **shareable configs** published as npm packages:

```json
{ "extends": "@company/eslint-config" }
```

- **Resolution**: npm package resolution (`@company/eslint-config`), then
  merge with local rules.
- **Cascade**: configs extend other configs, forming a chain.
- **Override**: local rules always win over shared configs.
- **Flat config (ESLint 9+)**: single array of config objects, last wins.
  Org configs are simply array entries that come first.

Key takeaway: **package-based distribution** via existing package managers.

### 3.2 Prettier

Prettier uses a simpler model:

- `.prettierrc` in the project root.
- **Shared configs**: npm packages that export a config object.
- No concept of "policies" -- local config always wins.

Key takeaway: simplicity is valued, but no enforcement mechanism.

### 3.3 VS Code (Settings Sync / Managed Settings)

VS Code has the richest enterprise model:

- **User settings** (`settings.json`) -- personal preferences.
- **Workspace settings** (`.vscode/settings.json`) -- project-level.
- **Remote settings** -- per-remote-machine overrides.
- **Policy settings** (GPO / MDM): `HKLM\...\Policies\...` on Windows, or
  `/etc/vscode/policies/` on Linux. Policies **lock** individual settings so
  users cannot override them.
- **Profiles**: switchable bundles of settings.

Key takeaway: **policy layer that locks fields** is the enterprise pattern.
The policy layer sits at the HIGHEST priority, not the lowest.

### 3.4 Terraform / OpenTofu

Terraform uses a module registry for shared configuration:

- `source = "registry.example.com/modules/vpc"` fetches from HTTP.
- Variable validation enforces constraints.
- Sentinel/OPA policies run as post-plan checks.

Key takeaway: **remote registry + policy-as-code** for enforcement.

### 3.5 Kubernetes (Kustomize)

Kubernetes uses base/overlay layering very similar to task-graph's model:

- `kustomization.yaml` references a `base/` and applies `patches/`.
- Helm charts support `values.yaml` hierarchy (chart defaults, release values,
  CLI overrides).

Key takeaway: the **base + overlay** model task-graph already uses maps well
to enterprise distribution.

---

## 4. Proposed Approach

### 4.1 New Config Tier: Organization

Insert a new tier between Defaults and Project in the merge hierarchy:

| Tier | Priority | Source |
|------|----------|--------|
| 0 - Defaults | Lowest | Embedded in binary |
| **1 - Organization** | **Low-Medium** | **`~/.task-graph/org/` or remote** |
| 2 - Project | Medium | `$CWD/task-graph/config.yaml` |
| 3 - User | Medium-High | `~/.task-graph/config.yaml` |
| 4 - Environment | Highest | `TASK_GRAPH_*` env vars |

The `ConfigTier` enum becomes:

```rust
pub enum ConfigTier {
    Defaults = 0,
    Organization = 1,   // NEW
    Project = 2,
    User = 3,
    Environment = 4,
}
```

### 4.2 Organization Config Directory

The org config lives at `~/.task-graph/org/` (or overridden by
`TASK_GRAPH_ORG_DIR`), containing:

```
~/.task-graph/org/
  config.yaml           # Org defaults (merged at tier 1)
  policies.yaml         # Field locks and constraints (NEW concept)
  workflows.yaml        # Org-wide workflow defaults
  workflow-release.yaml # Org-standard named workflows
  overlay-security.yaml # Org-standard overlays
  skills/               # Org-approved skills
  prompts.yaml          # Org-standard LLM prompts
```

### 4.3 Policy Enforcement (`policies.yaml`)

Policies define constraints that **cannot be relaxed** by lower-priority tiers
(project, user). Unlike normal config merging where "later wins," policies
define floors, ceilings, and locks.

```yaml
# ~/.task-graph/org/policies.yaml

# Fields that are locked to a specific value.
# Project and user config cannot change these.
locked:
  tags.unknown_tag: reject
  attachments.unknown_key: reject
  phases.unknown_phase: warn

# Numeric constraints: project/user values are clamped to these bounds.
constraints:
  server.stale_timeout_seconds:
    min: 300       # At least 5 minutes
  server.claim_limit:
    max: 10        # No more than 10 concurrent claims

# Required definitions that must exist (merged additively, cannot be removed).
# If a project defines the same key, the org definition is kept and the
# project value is ignored for locked fields.
required:
  gates:
    status:working:
      - type: gate/tests
        enforcement: reject
        description: "Org policy: tests must pass"
      - type: gate/commit
        enforcement: warn
        description: "Org policy: changes should be committed"

  tags.definitions:
    p0: { category: priority, description: "Critical" }
    p1: { category: priority, description: "High" }
    p2: { category: priority, description: "Medium" }
    p3: { category: priority, description: "Low" }
    security: { category: compliance, description: "Security review needed" }
```

Implementation sketch:

```rust
pub struct PolicyConstraint {
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
}

pub struct Policies {
    /// Fields locked to specific values (path -> value).
    pub locked: HashMap<String, serde_json::Value>,
    /// Numeric constraints (path -> min/max).
    pub constraints: HashMap<String, PolicyConstraint>,
    /// Required definitions that must exist and cannot be removed.
    pub required: serde_json::Value,
}
```

The `ConfigLoader` would apply policies as a post-merge step:

1. Load and merge tiers 0..4 as today.
2. Load `policies.yaml` from org dir.
3. For each `locked` field, overwrite the merged value.
4. For each `constraint`, clamp the merged value.
5. For `required` definitions, deep-merge them on top (so they cannot be
   removed by project config).

### 4.4 Effective Config Introspection

Add a `get_current_config` enhancement that shows provenance:

```json
{
  "server.claim_limit": {
    "value": 5,
    "source": "organization",
    "locked": false,
    "constraint": { "max": 10 }
  },
  "tags.unknown_tag": {
    "value": "reject",
    "source": "organization",
    "locked": true
  }
}
```

This supports the audit use case -- admins can verify org policies are active.

---

## 5. Lockable Fields

Which config fields should be lockable by org admins?

### 5.1 High Priority (Likely Lockable)

| Field Path | Reason |
|-----------|--------|
| `tags.unknown_tag` | Enforce standard tag taxonomy |
| `attachments.unknown_key` | Enforce standard attachment types |
| `phases.unknown_phase` | Enforce standard phase taxonomy |
| `gates.*` | Require quality gates (tests, review, commit) |
| `states.definitions` | Enforce standard state machine |
| `states.blocking_states` | Prevent circumventing dependency blocking |
| `dependencies.definitions` | Ensure consistent dependency semantics |

### 5.2 Medium Priority (Constrainable)

| Field Path | Constraint Type | Reason |
|-----------|----------------|--------|
| `server.claim_limit` | max | Prevent resource monopolization |
| `server.stale_timeout_seconds` | min | Prevent stale workers lingering too long |
| `server.default_page_size` | max | Prevent overloading query results |

### 5.3 Low Priority (Defaults Only, Not Lockable)

| Field Path | Reason |
|-----------|--------|
| `server.db_path` | Project-specific by nature |
| `server.media_dir` | Project-specific by nature |
| `server.log_dir` | Project-specific by nature |
| `ids.*` | Cosmetic preference, low harm |
| `paths.*` | Environment-specific |
| `server.ui.*` | Personal preference |

---

## 6. Distribution Mechanisms

### 6.1 Git-Based (Recommended for v1)

The simplest approach: org config lives in a git repo, cloned or symlinked to
`~/.task-graph/org/`.

**Pros:**
- Zero new infrastructure.
- Version controlled with full history.
- Works offline.
- Teams can fork and propose changes via PRs.

**Cons:**
- Manual initial setup per machine.
- No push notification on changes (requires periodic pull or watcher).

**Setup flow:**

```bash
# One-time setup
git clone git@github.com:acme-corp/task-graph-org.git ~/.task-graph/org

# Periodic update (could be a cron job or systemd timer)
cd ~/.task-graph/org && git pull --ff-only
```

The existing file watcher could be extended to watch `~/.task-graph/org/` for
changes, triggering a hot-reload when the admin runs `git pull`.

### 6.2 HTTP Fetch (Future)

For environments where git is not available or instant propagation is needed:

```yaml
# ~/.task-graph/config.yaml
organization:
  source: "https://config.acme.com/task-graph/org/"
  poll_interval_seconds: 3600  # Re-fetch hourly
  auth:
    type: bearer
    token_env: "TASK_GRAPH_ORG_TOKEN"
```

The server would fetch `config.yaml`, `policies.yaml`, etc. from the URL at
startup and on the configured interval.

**Pros:**
- Central control, instant propagation.
- No git required on agent machines.

**Cons:**
- Requires HTTP infrastructure.
- Network dependency at startup.
- Needs caching for offline resilience.

### 6.3 File Sync (Alternative)

For air-gapped or corporate environments: use existing file sync tools (rsync,
SMB share, OneDrive/Dropbox/Google Drive) to keep `~/.task-graph/org/` updated.

This is effectively the git model without git. The file watcher handles
hot-reload.

### 6.4 Package Manager (Future, Not Recommended for v1)

Similar to ESLint's npm package model, but task-graph config is YAML, not code.
A package manager adds complexity without proportional benefit. If needed,
it could use the same `source` mechanism as HTTP fetch but with a registry
protocol.

---

## 7. Migration and Rollout

### 7.1 Phased Approach

**Phase 1: Org defaults layer (low risk)**
- Add `ConfigTier::Organization` to the enum.
- Add `org_dir` to `ConfigPaths`.
- Load and merge `~/.task-graph/org/config.yaml` at the new tier.
- Extend `list_overlays()`, `list_workflows()`, `find_file()` to include
  the org directory.
- No policy enforcement yet -- org config is just another defaults layer.

**Phase 2: Policy enforcement**
- Add `policies.yaml` parsing.
- Implement lock, constraint, and required logic in `ConfigLoader`.
- Add provenance tracking to `get_current_config`.

**Phase 3: Distribution**
- Document git-based setup.
- Optionally add HTTP fetch for `organization.source` config.
- Add `task-graph org sync` CLI command for manual refresh.

### 7.2 Backward Compatibility

- The org layer is entirely opt-in. If `~/.task-graph/org/` does not exist,
  behavior is identical to today.
- Policy enforcement only activates when `policies.yaml` is present.
- No existing config files need to change.

---

## 8. Open Questions

1. **Org config per-project vs global?** Should an org be able to target
   policies to specific projects (e.g., "project X must use workflow-release")?
   This could be done with a `projects:` section in `policies.yaml` that
   matches by project name or path glob.

2. **Multiple orgs?** Can a machine belong to multiple organizations
   (e.g., contractor working for two clients)? Probably not needed for v1,
   but the directory convention could support it via `org-{name}/` subdirs.

3. **Policy violations: hard fail or warning?** When a project config violates
   an org policy, should the server refuse to start, or start with a warning
   and the org-enforced value? Warning-and-override is safer for initial
   rollout.

4. **Signing/verification?** Should org config be signed to prevent tampering?
   Probably not for v1, but the git commit history provides a basic audit trail.

5. **Overlay vs. org layer?** Overlays are already additive and searched across
   tiers. Org-provided overlays could naturally be discovered by extending the
   overlay search path. But **policies** are a new concept that overlays cannot
   express (overlays do not lock fields).

---

## 9. Summary

The current config system is well-designed for the individual-to-team scale:
layered config with deep merge, overlays for additive customization, and file
watching for hot-reload. Enterprise configuration builds on this foundation
with two additions:

1. **Organization tier**: a new merge layer at `~/.task-graph/org/` providing
   org-wide defaults for config, workflows, overlays, and skills.

2. **Policy enforcement**: a `policies.yaml` file that locks fields, sets
   numeric constraints, and requires certain definitions to exist, applied as
   a post-merge step that cannot be overridden by project or user config.

The recommended distribution mechanism for v1 is **git-based**, requiring no
new infrastructure and leveraging the existing file watcher for hot-reload.
HTTP fetch can be added later for environments that need centralized push.
