# Database Backends Design Document

## Status

**Design** -- not yet implemented.

## Motivation

The task-graph MCP server currently uses SQLite exclusively via `rusqlite` (bundled).
This works well for single-process deployments where the database file lives alongside
the server. However, distributed deployments -- where multiple task-graph instances
share state, or where data must be durable in a networked store -- require support for
additional database backends such as PostgreSQL or MySQL.

This document analyzes the current architecture, identifies SQLite-specific coupling
points, and proposes a strategy for introducing backend abstraction.

---

## 1. Current Architecture Analysis

### 1.1 Core Database Struct

The `Database` struct (`src/db/mod.rs`) wraps a single `rusqlite::Connection` behind
an `Arc<Mutex<Connection>>`:

```rust
pub struct Database {
    conn: Arc<Mutex<Connection>>,  // rusqlite::Connection
}
```

All database operations go through two gated methods:

- `with_conn<F>(&self, f: F)` -- immutable access (read queries)
- `with_conn_mut<F>(&self, f: F)` -- mutable access (transactions)

Every query in the codebase calls one of these two methods with a closure that
receives a `&Connection` or `&mut Connection` from `rusqlite`. This means the
`rusqlite::Connection` type is exposed to every module in `src/db/`.

### 1.2 Module Surface Area

The `src/db/` directory contains 14 modules that issue SQL:

| Module              | Approx. queries | SQLite-specific features used         |
|---------------------|-----------------|---------------------------------------|
| `tasks.rs`          | ~30             | `json_each()`, recursive CTEs         |
| `deps.rs`           | ~25             | `json_each()`, `INSERT OR IGNORE`     |
| `search.rs`         | ~4              | FTS5 (`MATCH`, `bm25()`, `snippet()`) |
| `schema.rs`         | ~6              | `sqlite_master`, PRAGMAs              |
| `attachments.rs`    | ~10             | None (standard SQL)                   |
| `locks.rs`          | ~20             | None (standard SQL)                   |
| `dashboard.rs`      | ~25             | None (standard SQL)                   |
| `stats.rs`          | ~8              | `json_each()`                         |
| `export.rs`         | ~8              | None (standard SQL)                   |
| `import.rs`         | ~5              | None (standard SQL)                   |
| `agents.rs`         | ~5              | None (standard SQL)                   |
| `state_transitions.rs` | ~6           | None (standard SQL)                   |
| `template.rs`       | ~3              | None (standard SQL)                   |
| `migrations.rs`     | 0 (JSON transforms only) | N/A                            |

### 1.3 Coupling Assessment

**Tight coupling points:**

1. **`rusqlite` types leak everywhere.** Every db module imports `rusqlite::params`,
   `rusqlite::Connection`, `rusqlite::Row`, and `rusqlite::ToSql`. There is no
   intermediate abstraction layer.

2. **`Database::open()` calls SQLite PRAGMAs** directly: `journal_mode=WAL`,
   `foreign_keys=ON`, `busy_timeout=5000`.

3. **The `refinery` migration runner** is compiled with the `rusqlite` feature and
   calls `runner().run(&mut *conn)` on a raw `rusqlite::Connection`.

4. **Row mapping** uses `rusqlite::Row` directly in closures passed to
   `query_map` / `query_row`. The column access pattern (`row.get(0)?`,
   `row.get("name")?`) is rusqlite-specific.

5. **Rename task** toggles `PRAGMA foreign_keys` on/off and uses
   `PRAGMA foreign_key_check` -- purely SQLite operations.

**Moderate coupling points:**

6. **`INSERT OR IGNORE`** syntax (used for dependencies and junction tables).
   PostgreSQL uses `ON CONFLICT DO NOTHING`; MySQL uses `INSERT IGNORE`.

7. **`COALESCE(started_at, ?)`** and `CAST(t.priority AS INTEGER)` are standard
   SQL but may differ in edge-case behavior across backends.

8. **`WHERE` clause partial indexes** in `CREATE INDEX ... WHERE ...` (used in
   `V001__initial_schema.sql`) -- supported by PostgreSQL but not MySQL.

### 1.4 SQLite-Specific Features Inventory

| Feature                          | Where used                           | Portability |
|----------------------------------|--------------------------------------|-------------|
| **FTS5 virtual tables**          | `tasks_fts`, `attachments_fts`       | No direct equivalent |
| **FTS5 `MATCH` syntax**          | `search.rs` queries                  | No direct equivalent |
| **FTS5 `bm25()` ranking**        | `search.rs` ORDER BY                 | No direct equivalent |
| **FTS5 `snippet()` highlighting**| `search.rs` SELECT                   | No direct equivalent |
| **FTS5 triggers (insert/update/delete)** | `V001__initial_schema.sql`    | Must be reimplemented |
| **`json_each()` table-valued fn**| `deps.rs`, `tasks.rs`, `stats.rs`    | PG: `jsonb_array_elements_text()` |
| **`INSERT OR IGNORE`**           | `deps.rs`, `tasks.rs`                | PG: `ON CONFLICT DO NOTHING` |
| **`sqlite_master`**              | `schema.rs`                          | PG: `information_schema` |
| **PRAGMAs**                      | `mod.rs`, `tasks.rs` (rename)        | No equivalent (backend config) |
| **`INTEGER PRIMARY KEY AUTOINCREMENT`** | `claim_sequence`, `task_state_sequence` | PG: `SERIAL`/`BIGSERIAL` |
| **Partial indexes (`WHERE ...`)**| Multiple indexes in V001             | PG: yes, MySQL: no |
| **Recursive CTEs (`WITH RECURSIVE`)** | `tasks.rs` (cascade delete/update) | PG: yes, MySQL 8+: yes |
| **WAL journal mode**             | `mod.rs`                             | N/A (backend handles concurrency) |

---

## 2. Abstraction Strategy

### 2.1 Recommended Approach: Trait-Based Abstraction with Feature Flags

The recommended approach combines a database trait with Cargo feature flags:

```
                        +-------------------+
                        |  trait DbBackend  |
                        +-------------------+
                         /        |         \
                        /         |          \
              +----------+  +-----------+  +-----------+
              | SqliteDb |  | PostgresDb|  |  MySqlDb  |
              +----------+  +-----------+  +-----------+
              (default)      (feature:      (feature:
                              "postgres")    "mysql")
```

**`trait DbBackend`** defines the operations that each module needs. Rather than
exposing raw connections, the trait provides domain-specific methods:

```rust
pub trait DbBackend: Send + Sync {
    // Task CRUD
    fn create_task(&self, ...) -> Result<Task>;
    fn get_task(&self, task_id: &str) -> Result<Option<Task>>;
    fn update_task(&self, ...) -> Result<Task>;
    fn delete_task(&self, ...) -> Result<()>;
    fn list_tasks(&self, query: ListTasksQuery<'_>) -> Result<Vec<Task>>;

    // Search (backend-specific full-text implementation)
    fn search_tasks(&self, query: &str, ...) -> Result<Vec<SearchResult>>;

    // Dependencies
    fn add_dependency(&self, ...) -> Result<AddDependencyResult>;
    fn get_dependencies(&self, ...) -> Result<Vec<Dependency>>;

    // ... etc for all db modules
}
```

**Feature flags** control which backend implementations are compiled:

```toml
[features]
default = ["sqlite"]
sqlite = ["rusqlite", "refinery/rusqlite"]
postgres = ["sqlx/postgres", "sqlx/runtime-tokio"]
mysql = ["sqlx/mysql", "sqlx/runtime-tokio"]
```

### 2.2 Why Not a Generic Query Builder (e.g., Diesel, SeaORM)?

Adopting a full ORM would be a larger lift than the trait approach and would introduce
significant new dependencies. The codebase already has well-structured hand-written
SQL that is readable and well-tested. The main effort is not in SQL dialect differences
(which are minor for the standard queries) but in handling **FTS5 and json_each()**,
which no ORM abstracts well.

If the project later wants compile-time-checked SQL across backends, `sqlx` with its
`query!` macro (which checks SQL at compile time against a live database) would be a
good middle ground that avoids a full ORM.

### 2.3 Why Not Separate Crate?

A separate `task-graph-db` crate is a valid option for cleanly isolating the
abstraction boundary. However, the current codebase is a single crate, and the
db modules are tightly integrated with the `config`, `types`, and `error` modules.
Extracting into a separate crate would require also extracting or duplicating those
shared types. The trait + feature-flag approach within the existing crate is simpler
as a first step; crate extraction can follow later if the abstraction proves stable.

---

## 3. Candidate Backends

### 3.1 PostgreSQL (via `sqlx` or `tokio-postgres`)

**Recommended as the first alternative backend.**

| Aspect               | Assessment |
|----------------------|------------|
| Async support        | Native (sqlx/tokio-postgres are fully async) |
| Full-text search     | Built-in `tsvector`/`tsquery` with ranking |
| JSON support         | `jsonb` type, `jsonb_array_elements_text()` replaces `json_each()` |
| Recursive CTEs       | Full support |
| Partial indexes      | Full support |
| UPSERT               | `ON CONFLICT DO NOTHING` / `DO UPDATE` |
| Migration tooling    | `sqlx-cli`, `refinery` (has `postgres` feature) |
| Ecosystem maturity   | Excellent |

**Key mapping decisions:**

- FTS5 `MATCH` queries map to `WHERE to_tsvector('english', title || ' ' || description) @@ to_tsquery(?)`
- FTS5 `bm25()` maps to `ts_rank()` or `ts_rank_cd()`
- FTS5 `snippet()` maps to `ts_headline()`
- FTS5 triggers map to PostgreSQL trigger functions that maintain a `tsvector` column
- `json_each(?1)` maps to `jsonb_array_elements_text(?1::jsonb)`
- `INSERT OR IGNORE` maps to `INSERT ... ON CONFLICT DO NOTHING`
- `sqlite_master` / PRAGMAs map to `information_schema` / `pg_catalog`

### 3.2 MySQL (via `sqlx`)

**Lower priority -- possible future addition.**

| Aspect               | Assessment |
|----------------------|------------|
| Async support        | Via sqlx |
| Full-text search     | InnoDB `FULLTEXT` indexes (less capable than PG) |
| JSON support         | `JSON_TABLE()` in MySQL 8+ replaces `json_each()` |
| Recursive CTEs       | MySQL 8.0+ only |
| Partial indexes      | Not supported (would need workarounds) |
| UPSERT               | `INSERT IGNORE` or `ON DUPLICATE KEY UPDATE` |
| Migration tooling    | `sqlx-cli`, `refinery` (has `mysql` feature) |

MySQL is feasible but requires more workarounds (no partial indexes, weaker FTS).
Recommend deferring until PostgreSQL support is stable.

### 3.3 Other Backends

- **CockroachDB**: Wire-compatible with PostgreSQL; likely works with the PG backend
  with minimal changes.
- **TiDB**: Wire-compatible with MySQL; same caveat as MySQL.
- **DuckDB**: Interesting for analytics but not suited for OLTP workloads.
- **libSQL/Turso**: SQLite-compatible with distributed replication; could reuse the
  existing SQLite backend almost unchanged. Worth investigating as the lowest-effort
  path to distributed SQLite.

---

## 4. Feature-by-Feature Porting Analysis

### 4.1 FTS5 Full-Text Search (Hardest)

FTS5 is the single largest porting challenge. The current implementation uses:

- **Virtual tables** (`tasks_fts`, `attachments_fts`) maintained by triggers
- **`MATCH` queries** with FTS5-specific syntax (prefix `*`, boolean `AND`/`NOT`,
  column-specific `title:error`)
- **`bm25()` ranking function** for relevance scoring
- **`snippet()` function** for highlighted excerpts

**PostgreSQL approach:**

Add a `search_vector tsvector` column to the `tasks` table, maintained by a trigger:

```sql
-- Add search vector column
ALTER TABLE tasks ADD COLUMN search_vector tsvector;

-- Create GIN index for fast search
CREATE INDEX idx_tasks_search ON tasks USING GIN(search_vector);

-- Trigger to maintain the vector
CREATE FUNCTION tasks_search_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := to_tsvector('english',
        coalesce(NEW.title, '') || ' ' || coalesce(NEW.description, ''));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_tasks_search
    BEFORE INSERT OR UPDATE ON tasks
    FOR EACH ROW EXECUTE FUNCTION tasks_search_update();
```

Query mapping:

```sql
-- SQLite FTS5
SELECT *, bm25(tasks_fts) as score,
    snippet(tasks_fts, 1, '<mark>', '</mark>', '...', 32)
FROM tasks_fts WHERE tasks_fts MATCH ?1

-- PostgreSQL equivalent
SELECT *, ts_rank(search_vector, query) as score,
    ts_headline('english', title, query,
        'StartSel=<mark>, StopSel=</mark>, MaxFragments=1') as title_snippet
FROM tasks, to_tsquery('english', ?1) query
WHERE search_vector @@ query
```

**Important caveats:**

- FTS5 query syntax (`title:error AND NOT warning*`) does not map 1:1 to
  `tsquery` syntax. A query translator layer is needed.
- FTS5 column-specific search (`title:error`) requires either separate
  `tsvector` columns per field or a custom search parser.
- Attachment FTS requires a similar `tsvector` column on the `attachments` table.

**MySQL approach:**

MySQL has `FULLTEXT` indexes on InnoDB, queried via `MATCH ... AGAINST`:

```sql
ALTER TABLE tasks ADD FULLTEXT INDEX ft_tasks(title, description);
SELECT *, MATCH(title, description) AGAINST (? IN BOOLEAN MODE) as score
FROM tasks WHERE MATCH(title, description) AGAINST (? IN BOOLEAN MODE);
```

MySQL lacks an equivalent of `snippet()`/`ts_headline()` -- highlighting must be
done in application code.

### 4.2 `json_each()` (Moderate)

Used in approximately 10 queries across `deps.rs`, `tasks.rs`, and `stats.rs` for
matching values in a JSON array against table rows. Example:

```sql
-- SQLite
SELECT value FROM json_each(?1)

-- Used in JOINs like:
JOIN (SELECT value FROM json_each(?1)) types
WHERE d.dep_type = types.value
```

**PostgreSQL:**

```sql
-- Direct replacement
jsonb_array_elements_text(?1::jsonb)

-- In context:
JOIN jsonb_array_elements_text(?1::jsonb) AS types(value)
ON d.dep_type = types.value
```

**MySQL 8+:**

```sql
-- Using JSON_TABLE
JSON_TABLE(?1, '$[*]' COLUMNS (value VARCHAR(255) PATH '$'))
```

**Alternative approach:** Instead of translating `json_each()` calls, the Rust code
could expand the JSON array in application code and generate `IN (?, ?, ?)` clauses
with positional parameters. This is already done in several places in the codebase
(e.g., `get_blocked_tasks`, `get_ready_tasks`) and would be backend-agnostic. This
is the recommended path for new code.

### 4.3 `INSERT OR IGNORE` (Easy)

Used for idempotent dependency creation and junction table updates.

| Backend    | Syntax                                    |
|------------|-------------------------------------------|
| SQLite     | `INSERT OR IGNORE INTO ...`               |
| PostgreSQL | `INSERT INTO ... ON CONFLICT DO NOTHING`  |
| MySQL      | `INSERT IGNORE INTO ...`                  |

This can be abstracted via a helper method or a backend-specific SQL template.

### 4.4 Schema Introspection (Moderate)

`schema.rs` queries `sqlite_master` and uses PRAGMAs (`table_info`, `index_list`,
`foreign_key_list`, `foreign_key_check`). These are entirely SQLite-specific.

**PostgreSQL:** Use `information_schema.tables`, `information_schema.columns`,
`pg_indexes`, and `information_schema.table_constraints`.

**Recommendation:** The schema introspection module is only used by the `get_schema`
MCP tool for debugging/documentation. It can remain SQLite-specific with a
backend-specific implementation, or be dropped for non-SQLite backends initially.

### 4.5 PRAGMAs and Connection Setup (Easy)

The `Database::open()` method sets:

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
```

For PostgreSQL, these map to connection pool configuration. WAL is irrelevant
(PostgreSQL uses MVCC). Foreign keys are always on. Busy timeout maps to connection
pool wait timeout.

The `rename_task` method toggles `PRAGMA foreign_keys` on/off to defer FK checks
during a rename. PostgreSQL supports `SET CONSTRAINTS ... DEFERRED` or
`ALTER TABLE ... DISABLE TRIGGER` within a transaction.

### 4.6 `AUTOINCREMENT` (Easy)

`claim_sequence` and `task_state_sequence` use `INTEGER PRIMARY KEY AUTOINCREMENT`.

| Backend    | Equivalent                               |
|------------|------------------------------------------|
| PostgreSQL | `SERIAL` or `BIGSERIAL` or `GENERATED ALWAYS AS IDENTITY` |
| MySQL      | `AUTO_INCREMENT`                         |

### 4.7 Partial Indexes (Moderate)

Several indexes in the initial schema use `WHERE` clauses:

```sql
CREATE INDEX idx_tasks_claimed ON tasks(claimed_at) WHERE worker_id IS NOT NULL;
CREATE INDEX idx_claim_seq_open ON claim_sequence(file_path) WHERE end_timestamp IS NULL;
```

PostgreSQL supports these natively. MySQL does not -- these would need to be dropped
or converted to full indexes (with some performance cost).

### 4.8 Recursive CTEs (Easy)

Used in `delete_task` for cascade operations:

```sql
WITH RECURSIVE descendants AS (
    SELECT ?1 AS id
    UNION ALL
    SELECT dep.to_task_id FROM dependencies dep
    INNER JOIN descendants d ON dep.from_task_id = d.id
    WHERE dep.dep_type = 'contains'
)
DELETE FROM tasks WHERE id IN (SELECT id FROM descendants)
```

Both PostgreSQL and MySQL 8+ support recursive CTEs with identical syntax. PostgreSQL
also supports `DELETE ... USING` as an alternative.

---

## 5. Migration Strategy

### 5.1 SQL Schema Migrations

The project currently uses `refinery` with embedded SQL migrations in `migrations/`.
Refinery supports multiple backends via feature flags (`rusqlite`, `postgres`,
`mysql`).

**Option A: Shared migrations with dialect tags**

Maintain a single set of migration files with backend-specific sections marked by
comments, and a pre-processor that selects the right variant:

```
migrations/
  V001__initial_schema.sqlite.sql
  V001__initial_schema.postgres.sql
  V001__initial_schema.mysql.sql
```

**Option B: Backend-specific migration directories**

```
migrations/
  sqlite/
    V001__initial_schema.sql
  postgres/
    V001__initial_schema.sql
```

**Recommendation:** Option B is cleaner. The initial schemas differ significantly
(FTS5 vs tsvector, PRAGMAs vs connection config, AUTOINCREMENT vs SERIAL). Trying
to share migration files would require too many conditional blocks.

Refinery supports directory-based migration sources via `embed_migrations!("path")`,
so the migration runner can select the directory based on the active backend feature.

### 5.2 JSON Export/Import Data Migrations

The existing `MigrationRegistry` (`src/db/migrations.rs`) handles JSON-level data
transformations during import. This system is backend-agnostic (it transforms
`serde_json::Value` objects) and requires no changes.

### 5.3 Transition Path

1. **Phase 1: Extract trait** -- Define `DbBackend` trait with all methods currently
   on `Database`. Implement it for `SqliteBackend` by wrapping the existing code.
   Ship with zero behavior change.

2. **Phase 2: PostgreSQL backend** -- Implement `PostgresBackend` using `sqlx` with
   PostgreSQL-specific SQL. Write PostgreSQL migrations. Add integration tests.

3. **Phase 3: Configuration** -- Add backend selection to `config.yaml` and
   environment variables.

4. **Phase 4 (optional): MySQL backend** -- If demand exists.

---

## 6. Configuration Approach

### 6.1 Config File

```yaml
server:
  # Current: path to SQLite file
  # db_path: ./tasks.db

  # New: backend selection with backend-specific config
  database:
    backend: sqlite          # "sqlite" | "postgres" | "mysql"

    # SQLite-specific (default, backward compatible)
    sqlite:
      path: ./tasks.db
      journal_mode: wal
      busy_timeout_ms: 5000

    # PostgreSQL-specific
    postgres:
      url: "postgres://user:pass@host:5432/taskgraph"
      max_connections: 10
      connect_timeout_secs: 5

    # MySQL-specific
    mysql:
      url: "mysql://user:pass@host:3306/taskgraph"
      max_connections: 10
```

### 6.2 Environment Variables

```bash
# Select backend
TASK_GRAPH_DB_BACKEND=postgres

# Backend-specific connection
TASK_GRAPH_DB_URL="postgres://user:pass@host:5432/taskgraph"

# Backward compatible: still works for SQLite
TASK_GRAPH_DB_PATH="./tasks.db"
```

### 6.3 Backward Compatibility

When `database.backend` is absent or set to `"sqlite"`, the server behaves
identically to today. The existing `db_path` field continues to work as a shorthand
for `database.sqlite.path`.

---

## 7. Effort Estimate

| Work item                                 | Estimate    | Risk    |
|-------------------------------------------|-------------|---------|
| Define `DbBackend` trait                  | 2-3 days    | Low     |
| Refactor existing code to use trait       | 3-5 days    | Medium  |
| PostgreSQL migration files                | 2-3 days    | Low     |
| PostgreSQL backend implementation         | 5-8 days    | Medium  |
| FTS5 to tsvector search porting           | 3-5 days    | High    |
| `json_each()` to `jsonb_array_elements`   | 1-2 days    | Low     |
| Schema introspection for PostgreSQL       | 1-2 days    | Low     |
| Configuration and backend selection       | 1-2 days    | Low     |
| Integration tests for PostgreSQL          | 3-5 days    | Medium  |
| Documentation                             | 1-2 days    | Low     |
| **Total**                                 | **22-37 days** | **Medium-High** |

The FTS5 porting is the highest-risk item because FTS5 query syntax does not map
cleanly to PostgreSQL `tsquery` syntax. A query translation layer (or acceptance
of slightly different search behavior) is needed.

### 7.1 Incremental Delivery

The work can be delivered incrementally:

1. **Trait extraction** (Phase 1) is valuable on its own -- it improves testability
   and makes the DB layer mockable.
2. **PostgreSQL support without FTS** can ship first, with search degrading to
   `ILIKE` queries. FTS can be added as a follow-up.
3. **libSQL/Turso** as a distributed SQLite variant could deliver multi-instance
   support with minimal code changes, as an alternative to full PostgreSQL support.

---

## 8. Open Questions

1. **Async vs sync:** The current `rusqlite` usage is synchronous, wrapped in a
   mutex. `sqlx` and `tokio-postgres` are async. Should the trait be async? This
   would require `async-trait` (already a dependency) but would change the calling
   convention for all db operations.

2. **Connection pooling:** SQLite uses a single connection with a mutex. PostgreSQL
   needs a connection pool (`sqlx::PgPool`). The trait should abstract over this
   difference.

3. **Transaction semantics:** The current `with_conn_mut` pattern exposes raw
   transactions to closures. A backend-agnostic transaction API needs careful design
   to avoid leaking backend-specific types.

4. **Search parity:** Should PostgreSQL search support the exact same FTS5 query
   syntax (requiring a translator), or should the MCP search tool expose a
   backend-agnostic subset of search features?

5. **libSQL alternative:** If the primary goal is multi-instance shared state, is
   libSQL/Turso (distributed SQLite) a simpler path than full PostgreSQL support?
   It would preserve FTS5, `json_each()`, and all existing SQL unchanged.
