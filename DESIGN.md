# Task Graph MCP - Design Document

> **Version:** 1.0  
> **Last Updated:** 2026-01-25  
> **Status:** Living Document

This document describes the architecture, design decisions, and implicit assumptions of the Task Graph MCP server.

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Component Architecture](#component-architecture)
- [Task State Machine](#task-state-machine)
- [Dependency System](#dependency-system)
- [Data Flow](#data-flow)
- [File Coordination Model](#file-coordination-model)
- [Design Decisions](#design-decisions)
- [Assumptions & Constraints](#assumptions--constraints)
- [Path Conventions](#path-conventions)

---

## Architecture Overview

Task Graph MCP is a multi-agent coordination server that enables AI agents to work together on complex tasks without conflicts. It provides:

- **Task DAG** - Hierarchical task management with typed dependencies
- **Atomic Claiming** - Prevents race conditions when agents claim work
- **File Coordination** - Advisory locks for file-level coordination
- **Cost Tracking** - Token and USD accounting per task

```plantuml
@startuml architecture-overview
!theme plain
skinparam backgroundColor #FEFEFE

title Task Graph MCP - High-Level Architecture

cloud "AI Agents" as agents {
    actor "Agent A\n(Claude)" as a1
    actor "Agent B\n(GPT-4)" as a2
    actor "Agent C\n(Worker)" as a3
}

package "MCP Layer" as mcp {
    component "task-graph\nMCP Server" as tg1
    component "task-graph\nMCP Server" as tg2
    component "task-graph\nMCP Server" as tg3
}

database "SQLite + WAL\n.task-graph/tasks.db" as db

a1 -down-> tg1 : stdio
a2 -down-> tg2 : stdio
a3 -down-> tg3 : stdio

tg1 -down-> db
tg2 -down-> db
tg3 -down-> db

note right of db
  WAL mode enables
  concurrent access
  across processes
end note

@enduml
```

---

## Component Architecture

The server is organized into distinct layers:

```plantuml
@startuml component-architecture
!theme plain
skinparam backgroundColor #FEFEFE

title Task Graph MCP - Component Architecture

package "Entry Point" {
    [main.rs] as main
    [TaskGraphServer] as server
}

package "Tool Layer" {
    [ToolHandler] as tools
    
    package "Tool Modules" {
        [agents.rs] as t_agents
        [tasks.rs] as t_tasks
        [deps.rs] as t_deps
        [claiming.rs] as t_claiming
        [files.rs] as t_files
        [attachments.rs] as t_attachments
        [tracking.rs] as t_tracking
        [search.rs] as t_search
        [query.rs] as t_query
        [schema.rs] as t_schema
        [skills.rs] as t_skills
    }
}

package "Resource Layer" {
    [ResourceHandler] as resources
    
    package "Resource Modules" {
        [tasks.rs] as r_tasks
        [agents.rs] as r_agents
        [files.rs] as r_files
        [stats.rs] as r_stats
        [skills.rs] as r_skills
    }
}

package "Database Layer" {
    [Database] as db
    
    package "DB Modules" {
        [tasks.rs] as d_tasks
        [agents.rs] as d_agents
        [deps.rs] as d_deps
        [locks.rs] as d_locks
        [attachments.rs] as d_attachments
        [state_transitions.rs] as d_state
        [search.rs] as d_search
        [stats.rs] as d_stats
        [schema.rs] as d_schema
    }
}

package "Support" {
    [config.rs] as config
    [types.rs] as types
    [error.rs] as error
    [format.rs] as format
}

main --> server
server --> tools
server --> resources
tools --> db
resources --> db
tools ..> config
tools ..> types
tools ..> error
tools ..> format
db ..> types

@enduml
```

### Layer Responsibilities

| Layer | Responsibility |
|-------|---------------|
| **Entry Point** | CLI parsing, server initialization, MCP protocol handling |
| **Tool Layer** | MCP tool definitions, parameter validation, business logic |
| **Resource Layer** | MCP resource definitions, read-only data access |
| **Database Layer** | SQLite operations, transactions, migrations |
| **Support** | Configuration, types, error handling, output formatting |

---

## Task State Machine

Tasks follow a configurable state machine. The default configuration:

```plantuml
@startuml task-state-machine
!theme plain
skinparam backgroundColor #FEFEFE

title Task State Machine (Default Configuration)

state "pending" as pending : Initial state
state "assigned" as assigned : Push coordination
state "in_progress" as in_progress : **Timed state**\nAccumulates time_actual_ms
state "completed" as completed : Terminal (success)
state "failed" as failed : Terminal (retriable)
state "cancelled" as cancelled : Terminal (abandoned)

[*] --> pending : create()

pending --> assigned : update(assignee=X)
pending --> in_progress : claim() / update(status)
pending --> cancelled : update(status)

assigned --> in_progress : claim() by assignee
assigned --> pending : release
assigned --> cancelled : update(status)

in_progress --> completed : update(status)
in_progress --> failed : update(status)
in_progress --> pending : release / disconnect

failed --> pending : retry

completed --> [*]
cancelled --> [*]

note right of in_progress
  Only timed states contribute
  to time_actual_ms tracking.
  
  Worker heartbeat refreshed
  via thinking() calls.
end note

note left of assigned
  Push coordination:
  Coordinator assigns task
  to specific worker.
end note

@enduml
```

### State Properties

| State | Timed | Terminal | Blocking | Description |
|-------|-------|----------|----------|-------------|
| `pending` | No | No | Yes | Initial state, waiting for claim |
| `assigned` | No | No | Yes | Assigned to specific worker (push model) |
| `in_progress` | **Yes** | No | Yes | Active work, time tracked |
| `completed` | No | Yes | No | Successfully finished |
| `failed` | No | Yes | No | Failed, can retry |
| `cancelled` | No | Yes | No | Abandoned, cannot retry |

### Auto-Advance

When enabled, tasks automatically transition from `pending` to a target state (e.g., `ready`) when their blocking dependencies are satisfied.

---

## Dependency System

Dependencies form a DAG (Directed Acyclic Graph) with typed edges:

```plantuml
@startuml dependency-types
!theme plain
skinparam backgroundColor #FEFEFE

title Dependency Types

package "Blocking Dependencies" {
    rectangle "Task A" as a1
    rectangle "Task B" as b1
    a1 -right-> b1 : **blocks**\n(start)
    note bottom of b1
      B cannot be claimed
      until A completes
    end note
}

package "Sequential Dependencies" {
    rectangle "Step 1" as s1
    rectangle "Step 2" as s2
    rectangle "Step 3" as s3
    s1 -right-> s2 : **follows**\n(start)
    s2 -right-> s3 : **follows**\n(start)
    note bottom of s3
      Auto-created by
      sibling_type='follows'
    end note
}

package "Hierarchical Dependencies" {
    rectangle "Parent" as p1
    rectangle "Child 1" as c1
    rectangle "Child 2" as c2
    p1 -down-> c1 : **contains**\n(completion)
    p1 -down-> c2 : **contains**\n(completion)
    note right of p1
      Parent cannot complete
      until all children complete
    end note
}

package "Informational Links" {
    rectangle "Original" as o1
    rectangle "Duplicate" as d1
    rectangle "Related" as r1
    o1 ..> d1 : **duplicate**
    o1 ..> r1 : **see-also**
    note bottom of d1
      No blocking effect,
      just metadata
    end note
}

@enduml
```

### Dependency Properties

| Type | Display | Blocks | Use Case |
|------|---------|--------|----------|
| `blocks` | Horizontal | Start | Explicit prerequisite |
| `follows` | Horizontal | Start | Sequential execution |
| `contains` | Vertical | Completion | Parent-child hierarchy |
| `duplicate` | Horizontal | None | Mark as duplicate |
| `see-also` | Horizontal | None | Related reference |
| `relates-to` | Horizontal | None | General relationship |

### Cycle Detection

The system prevents cycles in blocking dependencies (`blocks`, `follows`, `contains`). Non-blocking links (`duplicate`, `see-also`, `relates-to`) are not checked for cycles.

---

## Data Flow

```plantuml
@startuml data-flow
!theme plain
skinparam backgroundColor #FEFEFE

title Data Flow - Task Lifecycle

participant "Agent" as agent
participant "MCP Server" as server
participant "ToolHandler" as tools
participant "Database" as db

== Connection ==
agent -> server : connect(worker_id, tags)
server -> tools : call_tool("connect", args)
tools -> db : register_worker()
db --> tools : worker record
tools --> server : {worker_id, paths}
server --> agent : result

== Task Discovery ==
agent -> server : list_tasks(ready=true)
server -> tools : call_tool("list_tasks", args)
tools -> db : query_tasks(filters)
db --> tools : task list
tools --> server : formatted tasks
server --> agent : claimable tasks

== Claiming ==
agent -> server : claim(worker_id, task)
server -> tools : call_tool("claim", args)
tools -> db : BEGIN TRANSACTION
tools -> db : check_dependencies()
tools -> db : check_tag_requirements()
tools -> db : update_task(status=in_progress)
tools -> db : record_state_transition()
tools -> db : COMMIT
db --> tools : claimed task
tools --> server : {success, task}
server --> agent : confirmation

== Working ==
agent -> server : thinking(worker_id, thought)
server -> tools : call_tool("thinking", args)
tools -> db : update_heartbeat()
tools -> db : update_current_thought()
db --> tools : ok
tools --> server : {success}
server --> agent : ack

== Completion ==
agent -> server : update(status=completed, attachments)
server -> tools : call_tool("update", args)
tools -> db : BEGIN TRANSACTION
tools -> db : update_task(status, attachments)
tools -> db : record_state_transition()
tools -> db : calculate_time_actual_ms()
tools -> db : find_unblocked_tasks()
tools -> db : COMMIT
db --> tools : {task, unblocked}
tools --> server : {task, unblocked}
server --> agent : result with unblocked list

@enduml
```

---

## File Coordination Model

Advisory file locks enable agents to coordinate without conflicts:

```plantuml
@startuml file-coordination
!theme plain
skinparam backgroundColor #FEFEFE

title File Coordination Flow

participant "Agent A" as a
participant "Agent B" as b
participant "Server" as s
database "claim_sequence" as cs

== Agent A marks file ==
a -> s : mark_file("src/main.rs", "refactoring auth")
s -> cs : INSERT claimed event
s --> a : ok

== Agent B checks before editing ==
b -> s : mark_file("src/main.rs", "fixing bug")
s -> cs : check existing marks
s --> b : WARNING: Agent A has mark\n"refactoring auth"

note over b
  Agent B can now decide:
  - Wait for A to finish
  - Work around A's changes
  - Choose different file
end note

== Agent B polls for updates ==
b -> s : mark_updates()
s -> cs : SELECT since last_sequence
s --> b : no changes yet

== Agent A releases ==
a -> s : unmark_file("src/main.rs", "ready for review")
s -> cs : INSERT released event
s --> a : ok

== Agent B sees release ==
b -> s : mark_updates()
s -> cs : SELECT since last_sequence
s --> b : [{file: "src/main.rs", event: "released",\n  reason: "ready for review"}]

b -> s : mark_file("src/main.rs", "fixing bug")
s --> b : ok (no conflict)

@enduml
```

### Key Points

- **Advisory, not mandatory** - Marks signal intent, don't prevent access
- **Reason visibility** - Agents see *why* a file is marked
- **Polling-based** - `mark_updates()` returns changes since last call
- **Task association** - Marks can be tied to tasks for auto-cleanup

---

## Design Decisions

### Why SQLite?

| Alternative | Rejected Because |
|-------------|------------------|
| PostgreSQL | Requires server setup, network overhead |
| Redis | No persistence guarantees, complex setup |
| File-based | No concurrent access, no transactions |
| In-memory | Lost on restart |

**SQLite with WAL mode** provides:
- Zero configuration
- ACID transactions
- Concurrent readers
- Process-safe writes
- Single file deployment

### Why Configurable States?

Different workflows need different state machines:
- Simple: `pending` → `in_progress` → `completed`
- With review: Add `review` state before `completed`
- With ready queue: Add `ready` state after `pending`

### Why Typed Dependencies?

Generic "depends on" is insufficient:
- Need to distinguish blocking vs informational
- Need to support both sequence and hierarchy
- Need to allow custom workflow edges

---

## Assumptions & Constraints

### Runtime Assumptions

| Assumption | Impact | Mitigation |
|------------|--------|------------|
| **Single machine** | WAL mode assumes local filesystem | Document limitation |
| **SQLite available** | No abstraction for other DBs | Could add trait layer |
| **Filesystem access** | Media dir, log dir, config | Check permissions |
| **UTF-8 everywhere** | JSON content, file paths | Document requirement |

### Data Assumptions

| Assumption | Impact | Mitigation |
|------------|--------|------------|
| **Unix timestamps** | All times in epoch seconds | Consistent across platforms |
| **Task IDs unique** | UUID7 or user-provided | Validation on create |
| **Worker IDs unique** | User-provided or petname | Force flag for recovery |
| **JSON in TEXT** | Tags stored as JSON strings | Parse on read |

### Concurrency Assumptions

| Assumption | Impact | Mitigation |
|------------|--------|------------|
| **WAL mode enabled** | Concurrent reads, serial writes | Auto-enabled on open |
| **No distributed locking** | Single SQLite file | Document limitation |
| **Eventual consistency** | Polling-based coordination | Document latency |
| **Heartbeat timeout** | Default 5 min stale detection | Configurable |

### Security Assumptions

| Assumption | Impact | Mitigation |
|------------|--------|------------|
| **Trusted agents** | No auth/authz | Deploy in trusted environment |
| **Local filesystem** | No network exposure | Stdio transport only |
| **Read-only queries** | SQL tool restricted | Statement validation |

---

## Path Conventions

All paths in the system are **relative to the project root** unless they begin with a recognized prefix.

### Recognized Prefixes

| Prefix | Meaning | Example |
|--------|---------|---------|
| `~` | User home directory | `~/.config/app` |
| `$HOME` | User home (env var) | `$HOME/.config/app` |
| `/` | Absolute path (Unix) | `/etc/config` |
| `C:\`, `D:\`, etc. | Absolute path (Windows) | `C:\Users\config` |

### Relative Path Examples

| Path | Resolves To |
|------|-------------|
| `src/main.rs` | `{project_root}/src/main.rs` |
| `.task-graph/tasks.db` | `{project_root}/.task-graph/tasks.db` |
| `docs/README.md` | `{project_root}/docs/README.md` |

### Configuration Paths

```yaml
server:
  db_path: .task-graph/tasks.db     # Relative to project root
  media_dir: .task-graph/media      # Relative to project root
  log_dir: .task-graph/logs         # Relative to project root
  skills_dir: .task-graph/skills    # Relative to project root
```

### File Lock Paths

File marks use paths relative to project root:
```
mark_file(file="src/auth/login.rs", ...)  # Relative
mark_file(file="~/global/config", ...)    # Absolute (home)
```

---

## Appendix: Entity Relationship Diagram

```plantuml
@startuml entity-relationship
!theme plain
skinparam backgroundColor #FEFEFE

title Database Entity Relationships

entity "workers" as workers {
    * id : TEXT <<PK>>
    --
    tags : TEXT (JSON)
    max_claims : INTEGER
    registered_at : INTEGER
    last_heartbeat : INTEGER
    last_claim_sequence : INTEGER
}

entity "tasks" as tasks {
    * id : TEXT <<PK>>
    --
    title : TEXT
    description : TEXT
    status : TEXT
    priority : TEXT
    worker_id : TEXT <<FK>>
    claimed_at : INTEGER
    needed_tags : TEXT (JSON)
    wanted_tags : TEXT (JSON)
    tags : TEXT (JSON)
    points : INTEGER
    time_estimate_ms : INTEGER
    time_actual_ms : INTEGER
    started_at : INTEGER
    completed_at : INTEGER
    current_thought : TEXT
    cost_usd : REAL
    metrics : TEXT (JSON)
    created_at : INTEGER
    updated_at : INTEGER
}

entity "dependencies" as deps {
    * from_task_id : TEXT <<PK,FK>>
    * to_task_id : TEXT <<PK,FK>>
    * dep_type : TEXT <<PK>>
    --
}

entity "attachments" as attachments {
    * task_id : TEXT <<PK,FK>>
    * order_index : INTEGER <<PK>>
    --
    name : TEXT
    mime_type : TEXT
    content : TEXT
    file_path : TEXT
    created_at : INTEGER
}

entity "file_locks" as locks {
    * file_path : TEXT <<PK>>
    --
    worker_id : TEXT <<FK>>
    task_id : TEXT <<FK>>
    reason : TEXT
    locked_at : INTEGER
}

entity "claim_sequence" as claims {
    * id : INTEGER <<PK>>
    --
    file_path : TEXT
    agent_id : TEXT
    event : TEXT
    reason : TEXT
    timestamp : INTEGER
    end_timestamp : INTEGER
    claim_id : INTEGER
}

entity "task_state_sequence" as states {
    * id : INTEGER <<PK>>
    --
    task_id : TEXT <<FK>>
    agent_id : TEXT
    event : TEXT
    reason : TEXT
    timestamp : INTEGER
    end_timestamp : INTEGER
}

workers ||--o{ tasks : "claims"
workers ||--o{ locks : "holds"
workers ||--o{ claims : "creates"
workers ||--o{ states : "triggers"

tasks ||--o{ attachments : "has"
tasks ||--o{ deps : "from"
tasks ||--o{ deps : "to"
tasks ||--o{ states : "tracks"
tasks ||--o{ locks : "associated"

@enduml
```

---

## Document History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-01-25 | Initial design document |

---

*This document is maintained alongside the codebase. Update it when making architectural changes.*
