# Configuration Reference

Complete reference for configuring the Task Graph MCP Server.

## Configuration Files

Configuration is loaded from multiple sources with the following precedence (highest first):

1. **Environment variables** - `TASK_GRAPH_*` variables
2. **CLI arguments** - `--config`, `--database`, etc.
3. **Project-level** - `task-graph/config.yaml` or `.task-graph/config.yaml`
4. **User-level** - `~/.task-graph/config.yaml`
5. **Built-in defaults**

### File Locations

| File | Purpose |
|------|---------|
| `task-graph/config.yaml` | Main configuration |
| `task-graph/workflows.yaml` | States, phases, prompts |
| `task-graph/prompts.yaml` | Tool description overrides |
| `task-graph/workflow-{name}.yaml` | Named workflow topologies |
| `task-graph/skills/` | Custom skill definitions |

---

## config.yaml Reference

### Server Settings

```yaml
server:
  # Path to SQLite database file
  db_path: "task-graph/tasks.db"

  # Directory for file attachments
  media_dir: "task-graph/media"

  # Directory for custom skills
  skills_dir: "task-graph/skills"

  # Directory for logs
  log_dir: "task-graph/logs"

  # Maximum tasks an agent can claim simultaneously
  claim_limit: 5

  # Seconds before a worker is considered stale (default: 900 = 15 min)
  stale_timeout_seconds: 900

  # Default output format: json or markdown
  default_format: json

  # Default workflow to use when agent connects without specifying one
  default_workflow: null  # e.g., "swarm", "solo"

  # UI configuration
  ui:
    mode: none      # none (MCP only) or web (enable dashboard)
    port: 31994     # Port for web dashboard

    # Retry settings for dashboard startup
    retry_initial_ms: 15000    # Initial retry delay
    retry_jitter_ms: 5000      # Jitter range (±ms)
    retry_max_ms: 240000       # Maximum retry interval (4 min)
    retry_multiplier: 2.0      # Exponential backoff multiplier
```

### ID Generation

```yaml
ids:
  # Number of words for generated task IDs (default: 4)
  task_id_words: 4

  # Number of words for generated agent IDs (default: 4)
  agent_id_words: 4

  # Case style for generated IDs
  id_case: kebab-case  # See options below
```

**ID Case Options:**

| Value | Example |
|-------|---------|
| `kebab-case` | happy-turtle-swift-fox (default) |
| `snake_case` | happy_turtle_swift_fox |
| `camelCase` | happyTurtleSwiftFox |
| `PascalCase` | HappyTurtleSwiftFox |
| `lowercase` | happyturtleswiftfox |
| `UPPERCASE` | HAPPYTURTLESWIFTFOX |
| `Title Case` | Happy Turtle Swift Fox |

### Path Handling

```yaml
paths:
  # Root directory for sandboxing (all paths relative to this)
  root: "."

  # Path style in output: relative or project_prefixed
  style: relative

  # Auto-map single-letter Windows drives (e.g., "c:path")
  map_windows_drives: false

  # Prefix mappings (prefix -> path)
  # Values: literal path, $ENV_VAR, or ${config.path}
  mappings:
    home: "$HOME"
    project: "."
    media: "${server.media_dir}"
```

### Auto-Advance

Automatically transition unblocked tasks to a target state.

```yaml
auto_advance:
  enabled: false
  target_state: ready  # Requires this state in states config
```

---

## States Configuration

Task states define the lifecycle of tasks. Configure in `config.yaml` or `workflows.yaml`.

```yaml
states:
  # State for new tasks
  initial: pending

  # State for tasks when owner disconnects (must be untimed)
  disconnect_state: pending

  # States that block dependent tasks
  blocking_states: [pending, assigned, working]

  # Per-state definitions
  definitions:
    pending:
      exits: [assigned, working, cancelled]
      timed: false

    assigned:
      exits: [working, pending, cancelled]
      timed: false

    working:
      exits: [completed, failed, pending]
      timed: true    # Time in this state counts toward time_actual_ms

    completed:
      exits: [pending]  # Can reopen
      timed: false

    failed:
      exits: [pending]  # Can retry
      timed: false

    cancelled:
      exits: []         # Terminal state
      timed: false
```

**State Properties:**

| Property | Type | Description |
|----------|------|-------------|
| `exits` | string[] | Valid states to transition to |
| `timed` | bool | Whether time in this state is tracked |

---

## Dependencies Configuration

```yaml
dependencies:
  definitions:
    blocks:
      display: horizontal  # Same-level relationship
      blocks: start        # Blocks claiming the dependent task

    follows:
      display: horizontal
      blocks: start

    contains:
      display: vertical    # Parent-child relationship
      blocks: completion   # Blocks completing the parent

    duplicate:
      display: horizontal
      blocks: none         # Informational only

    see-also:
      display: horizontal
      blocks: none

    relates-to:
      display: horizontal
      blocks: none
```

**Dependency Properties:**

| Property | Values | Description |
|----------|--------|-------------|
| `display` | `horizontal`, `vertical` | Visual relationship type |
| `blocks` | `none`, `start`, `completion` | What the dependency blocks |

---

## Attachments Configuration

Preconfigure attachment types with default MIME types and modes.

```yaml
attachments:
  # Behavior for unknown keys: allow, warn, reject
  unknown_key: warn

  definitions:
    commit:
      mime: "text/git.hash"
      mode: append

    note:
      mime: "text/plain"
      mode: append

    meta:
      mime: "application/json"
      mode: replace

    # Gate attachments for workflow gates
    gate/tests:
      mime: "text/plain"
      mode: append

    gate/commit:
      mime: "text/plain"
      mode: append
```

**Attachment Properties:**

| Property | Values | Description |
|----------|--------|-------------|
| `mime` | MIME type | Default MIME type for this key |
| `mode` | `append`, `replace` | append keeps existing, replace overwrites |

**Built-in Attachment Types:**

| Key | MIME | Mode | Purpose |
|-----|------|------|---------|
| `commit` | text/git.hash | append | Git commit hashes |
| `checkin` | text/p4.changelist | append | Perforce changelists |
| `changelist` | text/plain | append | List of changed files |
| `meta` | application/json | replace | Structured metadata |
| `note` | text/plain | append | General notes |
| `log` | text/plain | append | Log output |
| `error` | text/plain | append | Error messages |
| `output` | text/plain | append | Command/tool output |
| `diff` | text/x-diff | append | Patches and diffs |
| `plan` | text/markdown | replace | Plans and specs |
| `result` | application/json | replace | Structured results |
| `context` | text/plain | replace | Current context/state |
| `gate/tests` | text/plain | append | Test gate satisfaction |
| `gate/commit` | text/plain | append | Commit gate satisfaction |
| `gate/review` | text/plain | append | Review gate satisfaction |

---

## Tags Configuration

Define known tags with categories and descriptions.

```yaml
tags:
  # Behavior for unknown tags: allow, warn, reject
  unknown_tag: warn

  definitions:
    rust:
      category: language
      description: "Rust programming tasks"

    python:
      category: language
      description: "Python programming tasks"

    security:
      category: domain
      description: "Security-related tasks"

    lead:
      category: role
      description: "Coordinator/lead agent"

    worker:
      category: role
      description: "Worker agent"
```

**Tag Usage:**

- **Task tags** (`tags`): Categorize tasks for discovery
- **Needed tags** (`needed_tags`): Agent must have ALL of these to claim (AND)
- **Wanted tags** (`wanted_tags`): Agent must have AT LEAST ONE of these (OR)
- **Agent tags**: Set on `connect()` to declare capabilities

---

## workflows.yaml Reference

The workflows configuration defines states, phases, prompts, and gates in a unified file.

### Basic Structure

```yaml
# Workflow metadata
name: default
description: Default workflow configuration

# Global settings
settings:
  initial_state: pending
  disconnect_state: pending
  blocking_states: [pending, assigned, working]
  unknown_phase: warn  # allow, warn, reject

# State definitions with prompts
states:
  pending:
    exits: [assigned, working, cancelled]
    timed: false

  working:
    exits: [completed, failed, pending]
    timed: true
    prompts:
      enter: |
        You are now working on this task.

        ## Valid Next States
        From `{{current_status}}` you can transition to:
        {{valid_exits}}
      exit: |
        Before leaving:
        - [ ] Attach results
        - [ ] Log costs

# Phase definitions
phases:
  explore:
    prompts:
      enter: |
        Exploration phase. Understand the problem space.
      exit: |
        Capture findings before moving on.

  implement:
    prompts:
      enter: |
        Implementation phase. Mark files before editing.

  # Phases without prompts
  test: {}
  review: {}
  deploy: {}

# State+Phase combination prompts
combos:
  working+implement:
    enter: |
      You're implementing. Focus on:
      - Follow existing patterns
      - Write tests alongside code

# Gate definitions
gates:
  status:working:
    - type: gate/tests
      enforcement: warn
      description: "Tests must pass before completing"

  phase:review:
    - type: gate/commit
      enforcement: reject
      description: "Code must be committed before review"
```

### Prompt Variables

Prompts support these template variables:

| Variable | Description |
|----------|-------------|
| `{{current_status}}` | Current task status |
| `{{valid_exits}}` | List of valid next states |
| `{{current_phase}}` | Current task phase |
| `{{valid_phases}}` | List of valid phases |

### Gates

Gates are checklists that must be satisfied before status or phase transitions.

```yaml
gates:
  # Gates for exiting a status
  status:working:
    - type: gate/tests
      enforcement: warn      # allow, warn (default), reject
      description: "Run tests before completing"

    - type: gate/commit
      enforcement: warn
      description: "Commit changes"

  # Gates for exiting a phase
  phase:implement:
    - type: gate/tests
      enforcement: reject    # Hard requirement
      description: "Tests must pass"
```

**Gate Enforcement Levels:**

| Level | Behavior |
|-------|----------|
| `allow` | Advisory only, never blocks |
| `warn` | Blocks unless `force=true` (default) |
| `reject` | Hard block, cannot be forced |

A gate is satisfied when the task has an attachment with a matching type (e.g., `gate/tests`).

---

## Named Workflows

Create workflow files for different topologies:

- `workflow-solo.yaml` - Single agent
- `workflow-swarm.yaml` - Parallel generalists
- `workflow-relay.yaml` - Sequential specialists
- `workflow-hierarchical.yaml` - Lead/worker delegation

### Using Named Workflows

```yaml
# In config.yaml
server:
  default_workflow: swarm
```

Or specify on connect:

```
connect(worker_id="agent-1", workflow="swarm")
```

### Workflow File Structure

```yaml
# workflow-swarm.yaml
name: swarm
description: Parallel generalists with fine-grained tasks

settings:
  initial_state: pending
  disconnect_state: pending
  blocking_states: [pending, assigned, working]
  unknown_phase: warn

# Override state prompts for this topology
states:
  working:
    exits: [completed, failed, pending]
    timed: true
    prompts:
      enter: |
        ## Swarm Worker Active
        Claim ONE task at a time. Complete quickly, release, repeat.

        **Coordination:**
        - Use `mark_file()` before editing shared files
        - Call `thinking()` frequently for visibility

# Phase prompts for this topology
phases:
  implement:
    prompts:
      enter: |
        In swarm topology, keep changes small and atomic.

# Topology-specific combo prompts
combos:
  working+implement:
    enter: |
      ## Swarm Implementation
      Check `mark_updates()` before touching shared files.
```

### Roles (for Hierarchical/Relay)

```yaml
# workflow-hierarchical.yaml
roles:
  lead:
    description: Coordinates work, decomposes tasks
    tags: [lead, coordinator]
    max_claims: 5
    can_assign: true
    can_create_subtasks: true

  worker:
    description: Claims and completes atomic subtasks
    tags: [worker]
    max_claims: 2
    can_assign: false
    can_create_subtasks: false
```

```yaml
# workflow-relay.yaml
roles:
  designer:
    description: Creates specifications
    tags: [designer, design]
    phases: [design, explore]
    prompts:
      claim: |
        You are taking on a design task.
      handoff: |
        Prepare handoff to implementer.

  implementer:
    description: Implements features
    tags: [implementer, code]
    phases: [implement, integrate]
```

---

## prompts.yaml Reference

Override LLM-facing instructions and tool descriptions.

```yaml
# Custom server instructions
instructions: |
  You are working on the Acme project.
  Follow these conventions:
  - Use TypeScript for all new code
  - Write tests for every feature

# Tool description overrides
tools:
  create:
    description: "Create a new task. Use parent for subtasks."

  claim:
    description: "Claim a task to start working on it."
```

**Load Locations:**

- `task-graph/prompts.yaml` (project-level)
- `~/.task-graph/prompts.yaml` (user-level)

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TASK_GRAPH_CONFIG_PATH` | Path to config file (highest precedence) |
| `TASK_GRAPH_DB_PATH` | Database file path |
| `TASK_GRAPH_MEDIA_DIR` | Media directory for attachments |
| `TASK_GRAPH_LOG_DIR` | Log directory path |
| `TASK_GRAPH_SKILLS_DIR` | Custom skills directory |

---

## CLI Arguments

```bash
task-graph-mcp [OPTIONS]

Options:
  -c, --config <FILE>     Path to configuration file
  -d, --database <FILE>   Path to database file (overrides config)
  -v, --verbose           Enable verbose logging
  -h, --help              Print help
  -V, --version           Print version
```

---

## Complete Example

A full `config.yaml` with all sections:

```yaml
server:
  db_path: "task-graph/tasks.db"
  media_dir: "task-graph/media"
  skills_dir: "task-graph/skills"
  log_dir: "task-graph/logs"
  claim_limit: 5
  stale_timeout_seconds: 900
  default_format: json
  default_workflow: swarm
  ui:
    mode: web
    port: 31994

ids:
  task_id_words: 4
  agent_id_words: 3
  id_case: kebab-case

paths:
  root: "."
  style: relative
  mappings:
    home: "$HOME"

auto_advance:
  enabled: false
  target_state: null

states:
  initial: pending
  disconnect_state: pending
  blocking_states: [pending, assigned, working]
  definitions:
    pending:
      exits: [assigned, working, cancelled]
    assigned:
      exits: [working, pending, cancelled]
    working:
      exits: [completed, failed, pending]
      timed: true
    completed:
      exits: [pending]
    failed:
      exits: [pending]
    cancelled:
      exits: []

dependencies:
  definitions:
    blocks:
      display: horizontal
      blocks: start
    follows:
      display: horizontal
      blocks: start
    contains:
      display: vertical
      blocks: completion
    duplicate:
      display: horizontal
      blocks: none
    see-also:
      display: horizontal
      blocks: none

attachments:
  unknown_key: warn
  definitions:
    commit:
      mime: "text/git.hash"
      mode: append
    note:
      mime: "text/plain"
      mode: append

tags:
  unknown_tag: warn
  definitions:
    rust:
      category: language
      description: "Rust tasks"
    lead:
      category: role
      description: "Lead agent"
```
