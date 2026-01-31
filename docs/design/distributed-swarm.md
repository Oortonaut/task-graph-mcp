# Distributed Swarm Support

> **Status:** Planning
> **Epic:** `comforting-orca`
> **Last Updated:** 2026-01-31

## Overview

This document describes the design and phased roadmap for evolving task-graph-mcp
from a single-machine MCP server into a distributed system where multiple
task-graph instances share state and agents on different machines collaborate
through a shared task graph.

---

## 1. Current Architecture

### Single-Machine Model

Today, task-graph-mcp runs as a **single-process, stdio-based MCP server** with
a local SQLite database. Each agent spawns its own server process, and all
processes on the same machine share state via SQLite WAL mode.

```
  Agent A        Agent B        Agent C
    |               |               |
  [stdio]         [stdio]         [stdio]
    |               |               |
  task-graph      task-graph      task-graph
  (process 1)     (process 2)     (process 3)
    \               |               /
     \              |              /
      +--- SQLite WAL (tasks.db) --+
```

**Key characteristics:**

- **Transport:** stdio only (`rmcp::transport::io::stdio`). Each agent talks to
  its own server process over stdin/stdout. There is no network listener.
- **Database:** Single SQLite file with WAL journaling. The `Database` struct
  wraps an `Arc<Mutex<Connection>>` providing serialized write access within a
  process, while WAL enables cross-process concurrent reads.
- **Concurrency control:** Within a process, a `Mutex<Connection>` serializes
  all writes. Across processes, SQLite WAL handles reader/writer concurrency
  with a 5-second busy timeout (`PRAGMA busy_timeout=5000`).
- **State sharing:** Multiple server processes on the same machine share state
  through the filesystem -- they all open the same `tasks.db` file.
- **Configuration:** Tier-based config loading (defaults, project, user,
  environment variables) with hot-reload via filesystem watcher.
- **Subscriptions:** Resource update notifications are in-process only. The
  `SubscriptionManager` tracks subscribed URIs for a single stdio client and
  sends notifications after tool calls that mutate state.
- **Dashboard:** Optional HTTP dashboard (`axum`) serving a read-only web UI on
  a configurable port. Reads directly from the shared SQLite file.

### What Works Well for Distribution

- **Stateless server logic.** Tool handlers read config from `Arc<ArcSwap<...>>`
  and execute SQL against the database. There is no in-memory state that would
  be lost if a server instance restarted.
- **Atomic claiming.** Task claims use SQL transactions with ownership checks,
  which will translate well to any backend that supports transactions.
- **Export/import.** The existing snapshot system (`Snapshot`, `ExportTables`)
  can serialize the full database to JSON, enabling data migration between
  backends.
- **Configurable state machine.** States, transitions, and phases are defined in
  config, not hardcoded, so different deployments can customize behavior.

### What Needs to Change

| Concern | Current | Distributed |
|---------|---------|-------------|
| Database | Local SQLite file | Shared DB (Postgres, TiKV, etc.) |
| Transport | stdio (local) | Network (SSE, WebSocket, gRPC) |
| Auth | None (trusted agents) | Token-based, RBAC |
| Notifications | In-process only | Cross-instance pub/sub |
| Config | Local files | Centralized + local override |
| Conflict resolution | SQLite serialization | Distributed locking / CAS |

---

## 2. Vision

### Target State

Multiple task-graph-mcp instances running on different machines, all connected to
a shared database, with agents connecting over the network:

```
  Machine A                    Machine B
  +------------------+        +------------------+
  | Agent 1          |        | Agent 3          |
  |   |              |        |   |              |
  | task-graph-mcp   |        | task-graph-mcp   |
  |   (instance 1)   |        |   (instance 2)   |
  +--------+---------+        +--------+---------+
           |                           |
           +--- Shared Database -------+
           |   (PostgreSQL / etc.)     |
           |                           |
  +--------+---------+        +--------+---------+
  | Agent 2          |        | Dashboard        |
  |   |   [network]  |        |   [web UI]       |
  | task-graph-mcp   |        | task-graph-mcp   |
  |   (instance 3)   |        |   (instance 4)   |
  +------------------+        +------------------+
```

### Key Properties

- **Horizontal scaling.** Any number of task-graph instances can run
  simultaneously. Agents can connect to any instance.
- **Shared state.** All instances see the same tasks, dependencies, agents, and
  file marks through the shared database.
- **Network connectivity.** Agents connect via network transport (SSE or
  WebSocket) instead of stdio, enabling cross-machine operation.
- **Access control.** Multi-tenant authorization prevents agents from interfering
  with each other's work inappropriately.
- **Centralized management.** Enterprise deployments can push config from a
  central source while allowing local overrides.

---

## 3. Component Breakdown

### 3a. Database Backends (prerequisite)

**Sub-task:** `unique-mouflon`

This is the foundational prerequisite. Without a shared database, there is no
distribution.

**Current state:**
- `Database` struct in `src/db/mod.rs` wraps `Arc<Mutex<rusqlite::Connection>>`
- All SQL is written directly against `rusqlite` APIs across 12+ modules:
  `tasks.rs`, `agents.rs`, `deps.rs`, `locks.rs`, `attachments.rs`,
  `state_transitions.rs`, `search.rs`, `stats.rs`, `schema.rs`, `export.rs`,
  `import.rs`, `template.rs`, `dashboard.rs`
- Migrations use `refinery` with embedded SQL files (`migrations/V001..V006`)
- SQLite-specific features used: `PRAGMA journal_mode=WAL`, `PRAGMA
  foreign_keys`, `PRAGMA busy_timeout`, `json_each()`, FTS5 (full-text search),
  recursive CTEs

**What needs to happen:**

1. **Define a `DatabaseBackend` trait** abstracting the core operations:
   ```rust
   #[async_trait]
   trait DatabaseBackend: Send + Sync {
       async fn create_task(&self, ...) -> Result<Task>;
       async fn get_task(&self, id: &str) -> Result<Option<Task>>;
       async fn claim_task(&self, ...) -> Result<Task>;
       async fn list_tasks(&self, query: ListTasksQuery) -> Result<Vec<Task>>;
       // ... ~40 methods covering tasks, agents, deps, locks, attachments, etc.
   }
   ```

2. **Refactor `Database` into `SqliteBackend` implementing the trait.** The
   existing code continues to work unchanged for single-machine deployments.

3. **Add `PostgresBackend`** as the primary distributed backend:
   - Translate SQLite-specific SQL (json_each, FTS5, PRAGMAs) to PostgreSQL
     equivalents (jsonb operators, tsvector/tsquery, configuration tables)
   - Use connection pooling (e.g., `deadpool-postgres` or `sqlx`)
   - Implement migrations for PostgreSQL schema
   - Use `SELECT ... FOR UPDATE` or advisory locks for atomic claiming

4. **Backend selection via config:**
   ```yaml
   server:
     database:
       backend: sqlite          # or "postgres"
       # SQLite options
       path: "task-graph/tasks.db"
       # PostgreSQL options
       url: "postgresql://user:pass@host/taskgraph"
       pool_size: 10
   ```

**Risks:**
- The trait surface area is large (~40+ methods). Keeping two implementations in
  sync requires good test coverage.
- SQLite-specific features (json_each, recursive CTEs, FTS5) need PostgreSQL
  equivalents. Some may perform differently.
- The `with_conn` / `with_conn_mut` pattern assumes synchronous access; async
  backends will need a different pattern.

**Dependencies:** None (this is the root prerequisite).

---

### 3b. Authorization (prerequisite)

**Sub-task:** `intriguing-mooneye`

In the current system, all agents are trusted. Any agent can claim any task,
disconnect any worker, or delete any task (with `force=true`). For distributed
multi-tenant operation, this must change.

**Current state:**
- No authentication or authorization anywhere in the codebase
- Worker IDs are self-assigned (agents pick their own ID on `connect()`)
- The only ownership check is "is this task claimed by the calling agent?"
  (enforced in `update_task_unified` and `delete_task`)
- File lock ownership is advisory

**What needs to happen:**

1. **Authentication layer:**
   - Token-based authentication (API keys or JWT) at the transport level
   - Each connection presents a token that maps to an identity and role
   - Tokens can be scoped to specific projects or tag sets

2. **Role-based access control (RBAC):**
   ```yaml
   authorization:
     enabled: true
     roles:
       admin:
         permissions: [create, read, update, delete, manage_agents, force]
       lead:
         permissions: [create, read, update, assign, manage_own_subtasks]
       worker:
         permissions: [read, claim, update_owned, attach]
       viewer:
         permissions: [read]
   ```

3. **Permission enforcement points:**
   - `connect()`: Validate token, assign role
   - `create()` / `create_tree()`: Check create permission
   - `claim()`: Check claim permission + tag affinity
   - `update()`: Check update permission + ownership
   - `delete()`: Check delete permission + cascade permissions
   - `disconnect()` / `cleanup_stale()`: Check manage_agents permission
   - `query()`: Filter visible tasks based on role/scope

4. **Audit logging:**
   - Log all mutations with authenticated identity
   - Track who performed each state transition (the `task_sequence` table
     already records `worker_id` but without authentication)

**Risks:**
- Retrofitting auth into an existing system is complex. Every tool handler needs
  permission checks.
- Performance impact of permission checks on every operation.
- Token management and rotation for long-running agent sessions.

**Dependencies:** Partially independent of database backends, but the full
benefit requires network transport (3c) since stdio is inherently local and
trusted.

---

### 3c. Network Transport

Agents connecting over the network instead of stdio.

**Current state:**
- Transport is exclusively `rmcp::transport::io::stdio()` (line 848 of
  `main.rs`)
- The `rmcp` crate supports multiple transports; `transport-io` is the only
  feature enabled in `Cargo.toml`
- MCP protocol itself is transport-agnostic -- it defines messages, not how they
  are delivered

**What needs to happen:**

1. **Add SSE (Server-Sent Events) transport:**
   - Enable `transport-sse` feature in `rmcp` (if available) or implement custom
   - Agents connect via HTTP, server pushes events via SSE
   - This is the emerging standard for remote MCP connections

2. **Add WebSocket transport as alternative:**
   - Full-duplex communication for higher throughput
   - Better for scenarios with frequent bidirectional messaging

3. **Listener configuration:**
   ```yaml
   server:
     transport:
       - type: stdio       # Keep for backward compatibility
       - type: sse
         host: "0.0.0.0"
         port: 31995
         tls:
           cert: "/path/to/cert.pem"
           key: "/path/to/key.pem"
       - type: websocket
         host: "0.0.0.0"
         port: 31996
   ```

4. **Multi-client support:**
   - The current `SubscriptionManager` assumes a single client. For network
     transport, each connected client needs its own subscription tracking.
   - The `TaskGraphServer` struct needs to handle multiple concurrent sessions.

**Risks:**
- Network transport introduces latency, connection drops, and reconnection logic
  that stdio does not have.
- TLS configuration adds operational complexity.
- Need to handle graceful degradation when connections drop (stale worker
  cleanup already exists but may need tuning).

**Dependencies:** Authorization (3b) should be in place before exposing network
transport, to prevent unauthorized access.

---

### 3d. Conflict Resolution

When multiple task-graph instances write to the same database concurrently,
conflicts can arise that SQLite's single-writer model previously prevented.

**Current state:**
- SQLite WAL serializes all writes through a single writer lock
- Within a process, `Mutex<Connection>` prevents concurrent writes
- Claiming uses a transaction: check status, check tags, update ownership
- File locks use transactions: check existing lock, insert/update

**What needs to happen:**

1. **Optimistic concurrency control:**
   - Add `version` column to tasks table (monotonically increasing)
   - Updates include `WHERE version = ?` to detect concurrent modifications
   - On conflict, retry with fresh data or return error to agent

2. **Distributed claiming:**
   - Use `SELECT ... FOR UPDATE` (PostgreSQL) or equivalent to serialize claims
   - Add claim timeout/TTL so abandoned claims are automatically released
   - Consider a dedicated claim service for high-contention scenarios

3. **Event ordering:**
   - The `task_sequence` and `claim_sequence` tables use auto-increment IDs for
     ordering. In a distributed system, use timestamp + instance ID for
     globally consistent ordering.
   - Consider using logical clocks (Lamport timestamps) for causal ordering.

4. **File lock coordination:**
   - Advisory file locks currently use SQLite rows. With a shared DB, the same
     approach works but contention increases.
   - Consider lock leasing with automatic expiry.

**Risks:**
- Distributed concurrency is inherently complex. Edge cases around network
  partitions, clock skew, and partial failures.
- Optimistic concurrency adds retry logic throughout the codebase.
- Performance may suffer under high contention.

**Dependencies:** Requires database backends (3a) to be in place.

---

### 3e. Cross-Instance Notifications

When one task-graph instance modifies state, other instances (and their connected
agents) need to be notified.

**Current state:**
- `SubscriptionManager` is in-process only. After a tool call, `mutations_for_tool()`
  determines which resource URIs are affected, and notifications are sent to the
  single connected stdio client.
- There is no mechanism to notify other processes.

**What needs to happen:**

1. **Pub/sub layer:**
   - PostgreSQL `LISTEN/NOTIFY` for lightweight change notifications
   - Or Redis pub/sub, or NATS, for higher throughput
   - Each instance subscribes to change channels and forwards notifications to
     its connected clients

2. **Change feed:**
   - Alternatively, poll a changes table (append-only log of mutations)
   - Each instance tracks its read position and processes new changes
   - Simpler but higher latency

3. **Notification coalescing:**
   - Batch rapid changes to avoid notification storms
   - The current debouncing in the config watcher (`notify-debouncer-mini`) is
     a pattern to follow

**Dependencies:** Requires database backends (3a) and network transport (3c).

---

### 3f. Enterprise Config

**Sub-task:** `revered-eft`

Centralized configuration management for enterprise deployments.

**Current state:**
- Config is loaded from local files using `ConfigLoader` with tier merging:
  defaults < project < user < environment variables
- Hot-reload via filesystem watcher monitors `task-graph/` directory
- Named workflows (`workflow-*.yaml`) and overlays (`overlay-*.yaml`) are loaded
  from local directories

**What needs to happen:**

1. **Remote config source:**
   ```yaml
   config:
     sources:
       - type: local       # Existing behavior
         path: "task-graph/"
       - type: remote
         url: "https://config.company.com/task-graph/v1"
         poll_interval: 300s
         auth:
           type: bearer
           token_env: "CONFIG_SERVICE_TOKEN"
   ```

2. **Config inheritance:**
   - Organization defaults -> Team config -> Project config -> Local overrides
   - Remote config provides org/team layers; local files provide project/user

3. **Config versioning and rollback:**
   - Track config versions with the ability to pin or rollback
   - Validate config changes before applying (the existing `validate()` methods
     on `StatesConfig` and `DependenciesConfig` support this)

4. **Secret management:**
   - Database credentials, API keys, and tokens should not be in plaintext YAML
   - Support environment variable references (`$DB_PASSWORD`) and secret
     manager integration (HashiCorp Vault, AWS Secrets Manager)

**Dependencies:** Largely independent, but most useful alongside database
backends (3a) and authorization (3b).

---

## 4. Phased Roadmap

### Phase 1: Foundation (Database Abstraction)

**Goal:** Extract a backend trait and keep SQLite as the default implementation.

```
[unique-mouflon] Database Backends
  |
  +-- Define DatabaseBackend trait
  +-- Refactor Database -> SqliteBackend
  +-- Add PostgresBackend (basic CRUD)
  +-- Migration framework for multi-backend
  +-- Config-driven backend selection
```

**Why first:** Everything else depends on a shared database. Without it,
distribution is impossible.

**Estimated scope:** Large. The trait surface spans 12+ modules and ~40 methods.
The SQLite-specific SQL (json_each, FTS5, CTEs) needs PostgreSQL equivalents.

### Phase 2: Access Control

**Goal:** Add authentication and authorization so the system is safe to expose
over a network.

```
[intriguing-mooneye] Authorization
  |
  +-- Token-based authentication
  +-- RBAC with configurable roles
  +-- Permission enforcement in tool handlers
  +-- Audit trail enhancements
```

**Why second:** Must be in place before opening network transport. Exposing an
unauthenticated task-graph to the network is a security risk.

### Phase 3: Network Transport + Conflict Resolution

**Goal:** Enable agents on different machines to connect.

```
Network Transport               Conflict Resolution
  |                               |
  +-- SSE transport               +-- Optimistic versioning
  +-- WebSocket transport         +-- Distributed claiming
  +-- Multi-client sessions       +-- Event ordering
  +-- TLS support                 +-- Lock leasing
```

**Why together:** Network transport creates the need for conflict resolution.
Once multiple instances can write concurrently from different machines, the
serialization guarantees of single-machine SQLite are gone.

### Phase 4: Enterprise Features

**Goal:** Production-readiness for enterprise deployments.

```
[revered-eft] Enterprise Config     Cross-Instance Notifications
  |                                   |
  +-- Remote config source            +-- Pub/sub layer
  +-- Config inheritance              +-- Change feed
  +-- Secret management               +-- Notification coalescing
  +-- Config versioning
```

**Why last:** These are quality-of-life and operational features. The system is
functionally distributed after Phase 3; Phase 4 makes it manageable at scale.

### Dependency Graph

```
Phase 1                Phase 2              Phase 3              Phase 4
+-----------+     +-------------+     +----------------+    +-------------+
| Database  |---->| Authori-    |---->| Network        |    | Enterprise  |
| Backends  |     | zation      |     | Transport      |    | Config      |
| (3a)      |     | (3b)        |     | (3c)           |    | (3f)        |
+-----------+     +-------------+     +-------+--------+    +-------------+
      |                                       |
      |           +-------------+             |
      +---------->| Conflict    |<------------+
                  | Resolution  |
                  | (3d)        |
                  +------+------+
                         |
                  +------+------+
                  | Cross-Inst  |
                  | Notifica-   |
                  | tions (3e)  |
                  +-------------+
```

---

## 5. Open Questions

### Architecture

1. **Trait vs. SQL abstraction?** Should the `DatabaseBackend` trait expose
   high-level operations (create_task, claim_task) or a lower-level query
   abstraction? High-level is simpler but duplicates business logic. Low-level
   (like an async connection pool with query building) is more flexible but
   requires rethinking the DB module structure.

2. **Async all the way?** The current `with_conn` / `with_conn_mut` pattern is
   synchronous. PostgreSQL clients are async. Do we make the trait async
   (requiring `tokio::spawn_blocking` for SQLite) or provide separate sync/async
   paths?

3. **ORM or raw SQL?** Should we adopt `sqlx` (compile-time checked queries) or
   `sea-orm` for the PostgreSQL backend? Or continue with raw SQL translated per
   backend?

4. **Which DB first?** PostgreSQL is the natural choice for shared state, but
   CockroachDB or TiKV could provide better distributed guarantees. Start with
   PostgreSQL and abstract later?

### Operations

5. **Migration between backends?** How do users migrate from SQLite to
   PostgreSQL? The export/import system provides a path, but large databases may
   need streaming migration.

6. **Mixed-mode operation?** Can a deployment run SQLite locally for development
   and PostgreSQL in production with the same config structure? (Yes, via the
   `backend` config field.)

7. **Connection string security?** PostgreSQL connection strings contain
   credentials. Where do they live? Environment variables? Secret manager?

### Protocol

8. **MCP evolution?** The MCP protocol is evolving. Will `rmcp` support SSE
   and WebSocket natively, or do we need custom transport implementations?

9. **Backward compatibility?** Agents using the old stdio transport should
   continue to work alongside agents using network transport. Is this a
   deployment concern (run both) or a protocol concern (same instance serves
   both)?

### Scale

10. **How many concurrent agents?** Current design supports ~10-50 agents on one
    machine. Distributed target is 100-1000+. What are the bottleneck points?
    (Likely: database write throughput for heartbeats and claiming.)

11. **Heartbeat scaling?** Each agent calls `thinking()` periodically, which
    updates the database. With 1000 agents, that is 1000 writes/interval. Need
    to batch or reduce heartbeat frequency.

---

## 6. Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Trait surface too large | High | Start with core operations (CRUD + claim), add others incrementally |
| SQL translation bugs | High | Extensive integration tests against both backends |
| Performance regression | Medium | Benchmark before/after trait abstraction; connection pooling |
| Auth complexity | Medium | Start with simple API key auth; add JWT/OIDC later |
| Network reliability | Medium | Reconnection logic, heartbeat-based stale detection already exists |
| Breaking changes | High | Semantic versioning; keep SQLite as default for backward compatibility |
| Scope creep | High | Each phase should be independently shippable |

---

## 7. Non-Goals (for now)

- **Multi-region replication.** Focus on single-region shared database first.
- **Custom consensus protocols.** Use database-level consistency (PostgreSQL
  transactions) rather than building distributed consensus.
- **Agent-to-agent communication.** Agents coordinate through the task graph,
  not directly with each other.
- **Real-time streaming.** Polling-based coordination (mark_updates, list_tasks)
  is sufficient. Real-time push is a Phase 4 enhancement.

---

## References

- Current architecture: `docs/DESIGN.md`
- Configuration reference: `docs/CONFIGURATION.md`
- Workflow topologies: `docs/WORKFLOW_TOPOLOGIES.md`
- Database schema: `docs/SCHEMA.md`
- Sub-task for DB backends: `unique-mouflon`
- Sub-task for authorization: `intriguing-mooneye`
- Sub-task for enterprise config: `revered-eft`

---

*This is a living planning document. Update it as design decisions are made and
implementation progresses.*
