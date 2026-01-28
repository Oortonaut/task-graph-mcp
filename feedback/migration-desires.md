# Structured Export/Import Specification

## Overview

A schema-versioned structured export format for task-graph databases, enabling:
- Version control of project task data
- Database reconstruction from exports
- Migration between schema versions
- Human-readable diffs in git

## Design Goals

1. **Low overhead**: Single file, minimal processing
2. **Schema-aware**: Version field drives import/migration logic
3. **Diffable**: JSON structure produces meaningful git diffs
4. **Compressible**: Optional gzip for storage efficiency
5. **Selective**: Export project data, skip ephemeral runtime state

## Export Format

### File Structure

```
.task-graph/
  db.sqlite              # gitignored - runtime database
  snapshot.json          # tracked - structured export
  snapshot.json.gz       # alternative: compressed export
```

### JSON Schema

```json
{
  "schema_version": 3,
  "export_version": "1.0.0",
  "exported_at": "2026-01-25T10:30:00Z",
  "exported_by": "task-graph-mcp v0.5.0",
  "tables": {
    "tasks": [...],
    "dependencies": [...],
    "attachments": [...],
    "task_tags": [...],
    "task_needed_tags": [...],
    "task_wanted_tags": [...],
    "task_state_sequence": [...]
  }
}
```

### Header Fields

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | integer | Database schema version (from task-graph internals) |
| `export_version` | string | Export format version (semver) |
| `exported_at` | string | ISO 8601 timestamp |
| `exported_by` | string | Tool name and version |

### Tables Included (Project Data)

| Table | Purpose | Ordering |
|-------|---------|----------|
| `tasks` | Task records | `ORDER BY id` |
| `dependencies` | Task relationships | `ORDER BY from_task_id, to_task_id, dep_type` |
| `attachments` | Task attachments | `ORDER BY task_id, order_index` |
| `task_tags` | Categorization tags | `ORDER BY task_id, tag` |
| `task_needed_tags` | Required agent tags (AND) | `ORDER BY task_id, tag` |
| `task_wanted_tags` | Optional agent tags (OR) | `ORDER BY task_id, tag` |
| `task_state_sequence` | State transition audit log | `ORDER BY task_id, id` |

### Tables Excluded (Ephemeral/Runtime)

| Table | Reason |
|-------|--------|
| `workers` | Runtime worker registrations |
| `file_locks` | Active file marks |
| `claim_sequence` | File lock audit (runtime) |
| `*_fts*` | FTS virtual tables (rebuilt on import) |

## Row Format

Each table entry is a JSON object with column names as keys:

```json
{
  "tables": {
    "tasks": [
      {
        "id": "node-001",
        "title": "Originator Field Migration",
        "description": "Move actor field from MessageNode...",
        "status": "completed",
        "priority": "medium",
        "worker_id": null,
        "claimed_at": null,
        "tags": "[\"stream-a\"]",
        "points": 3,
        "time_estimate_ms": null,
        "time_actual_ms": 1200000,
        "started_at": 1737481500000,
        "completed_at": 1737482700000,
        "cost_usd": 0.45,
        "metric_0": 15000,
        "metric_1": 3200,
        "metric_2": 45000,
        "metric_3": 8000,
        "metric_4": 0,
        "metric_5": 0,
        "metric_6": 0,
        "metric_7": 0,
        "deleted_at": null,
        "created_at": 1737480000000,
        "updated_at": 1737482700000
      }
    ],
    "dependencies": [
      {
        "from_task_id": "node-002",
        "to_task_id": "node-003",
        "dep_type": "blocks"
      }
    ],
    "task_state_sequence": [
      {
        "id": 1,
        "task_id": "node-001",
        "worker_id": "opus-4e7b2a",
        "event": "in_progress",
        "reason": "Starting work",
        "timestamp": 1737481500000,
        "end_timestamp": 1737482700000
      }
    ]
  }
}
```

## CLI Interface

### Export

```bash
# Standard export
task-graph export > snapshot.json
task-graph export -o snapshot.json

# Compressed export
task-graph export --gzip -o snapshot.json.gz
task-graph export -o snapshot.json.gz  # auto-detect from extension

# Export specific tables only
task-graph export --tables tasks,dependencies

# Export without audit history
task-graph export --no-history
```

### Import

```bash
# Import (creates new or replaces existing)
task-graph import snapshot.json

# Import compressed
task-graph import snapshot.json.gz

# Dry run (validate without modifying)
task-graph import --dry-run snapshot.json

# Merge mode (add missing, skip existing)
task-graph import --merge snapshot.json

# Force overwrite conflicts
task-graph import --force snapshot.json
```

### Diff

```bash
# Compare export to current database
task-graph diff snapshot.json

# Compare two exports
task-graph diff old.json new.json
```

## Import Behavior

### Fresh Import (Empty Database)

1. Validate schema version compatibility
2. Create tables if needed
3. Insert all rows
4. Rebuild FTS indexes

### Existing Database Import

Default behavior: **Replace**
1. Clear existing project data tables
2. Import all rows from snapshot
3. Preserve runtime tables (workers, file_locks)
4. Rebuild FTS indexes

### Merge Mode

With `--merge` flag:
1. For each task: skip if ID exists, insert if new
2. For dependencies: skip if exact match exists
3. For attachments: append or replace by name (configurable)

## Schema Migration

When `schema_version` in export differs from current database:

```
Export v2 → Database v3

1. Import into v2 schema (temporary)
2. Run migration v2→v3
3. Replace target database
```

Migration registry:
```python
MIGRATIONS = {
    (2, 3): migrate_v2_to_v3,
    (3, 4): migrate_v3_to_v4,
}
```

Unsupported migrations fail with clear error:
```
Error: Cannot import schema v1 export into v4 database.
Supported migrations: v2→v3, v3→v4
```

## Compression

### Automatic Compression

- Export: Compress if output file ends in `.gz`
- Import: Detect gzip magic bytes, decompress transparently

### Compression Threshold (Optional)

```bash
# Compress if over 100KB
task-graph export --compress-threshold 100KB -o snapshot.json
# Outputs snapshot.json or snapshot.json.gz based on size
```

## Git Integration

### Recommended .gitignore

```gitignore
.task-graph/db.sqlite
.task-graph/db.sqlite-*
.task-graph/*.log
```

### Recommended Tracking

```bash
# Track the snapshot
git add .task-graph/snapshot.json

# Or compressed
git add .task-graph/snapshot.json.gz
```

### Pre-commit Hook (Optional)

```bash
#!/bin/bash
# .git/hooks/pre-commit

if [ -f .task-graph/db.sqlite ]; then
  task-graph export -o .task-graph/snapshot.json
  git add .task-graph/snapshot.json
fi
```

## Diff Example

Given two snapshots, a git diff would show:

```diff
  "tasks": [
    {
      "id": "node-001",
      "title": "Originator Field Migration",
-     "status": "pending",
+     "status": "completed",
+     "completed_at": 1737482700000,
      ...
    },
+   {
+     "id": "node-006",
+     "title": "New Task",
+     ...
+   }
  ]
```

## Edge Cases

### Deleted Tasks

Soft-deleted tasks (with `deleted_at` set) are included in exports by default. They can be excluded:

```bash
task-graph export --exclude-deleted
```

### Large Attachments

Attachments with `file_path` (stored externally in `.task-graph/media/`) export the path reference, not the content:

```json
{
  "task_id": "node-001",
  "name": "screenshot",
  "content": null,
  "file_path": "media/abc123.png"
}
```

Media files should be tracked separately or via Git LFS.

### Circular Dependencies

Export includes dependencies as-is. Import validates:
- No self-references (from = to)
- Warns on cycles (optional `--strict` to reject)

## Metrics Mapping Convention

For imports from external systems (e.g., plan.yaml), recommended metric slot usage:

| Slot | Purpose |
|------|---------|
| `metric_0` | tokens.input |
| `metric_1` | tokens.output |
| `metric_2` | tokens.cache_read |
| `metric_3` | tokens.cache_create |
| `metric_4` | wall_time_seconds |
| `metric_5` | (reserved) |
| `metric_6` | (reserved) |
| `metric_7` | (reserved) |

This convention should be documented but not enforced by the export/import tooling.

## Future Considerations

- **Streaming export**: For very large databases, JSONL format with one record per line
- **Partial export**: Export subtree rooted at specific task
- **Binary format**: MessagePack or similar for performance-critical use cases
- **Remote sync**: Push/pull exports to/from remote storage
