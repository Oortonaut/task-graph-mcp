# Export/Import Guide

Task-graph supports schema-versioned export/import of task data, enabling:

- Version control of project task data
- Database reconstruction from exports
- Migration between schema versions
- Human-readable diffs in git

## Quick Start

```bash
# Export to JSON file
task-graph export -o snapshot.json

# Import from snapshot
task-graph import snapshot.json

# Compare snapshot to database
task-graph diff snapshot.json
```

## Export

Export creates a structured JSON file containing all project data from the task database.

### Basic Export

```bash
# Export to stdout
task-graph export

# Export to file
task-graph export -o snapshot.json

# Export with gzip compression
task-graph export --gzip -o snapshot.json.gz
task-graph export -o snapshot.json.gz  # Auto-detected from .gz extension
```

### Selective Export

```bash
# Export specific tables only
task-graph export --tables tasks,dependencies

# Export without audit history (smaller file)
task-graph export --no-history

# Exclude soft-deleted tasks
task-graph export --exclude-deleted
```

### Automatic Compression

```bash
# Compress if output exceeds threshold
task-graph export --compress-threshold 100KB -o snapshot.json
# Outputs snapshot.json or snapshot.json.gz based on actual size
```

### Export Options Reference

| Option | Description |
|--------|-------------|
| `-o, --output <FILE>` | Output file path (default: stdout) |
| `--gzip` | Force gzip compression |
| `--tables <LIST>` | Comma-separated list of tables to export |
| `--no-history` | Exclude task_state_sequence table |
| `--exclude-deleted` | Filter out soft-deleted tasks |
| `--compress-threshold <SIZE>` | Auto-compress if exceeds size (e.g., 100KB, 1MB) |

### Available Tables

- `tasks` - Task records
- `dependencies` - Task relationships
- `attachments` - Task attachments
- `task_tags` - Categorization tags
- `task_needed_tags` - Required agent tags (AND matching)
- `task_wanted_tags` - Optional agent tags (OR matching)
- `task_state_sequence` - State transition audit log

## Import

Import loads task data from a snapshot file into the database.

### Import Modes

**Replace Mode (Default)**
```bash
# Replace all project data with snapshot contents
task-graph import snapshot.json
```

**Merge Mode**
```bash
# Add missing items, skip existing
task-graph import --merge snapshot.json
```

**Dry Run**
```bash
# Preview what would be imported without making changes
task-graph import --dry-run snapshot.json
```

### Import Options Reference

| Option | Description |
|--------|-------------|
| `--dry-run` | Validate without modifying database |
| `--merge` | Add missing items, skip existing |
| `--force` | Overwrite conflicts without prompting |
| `--strict` | Reject imports with circular dependencies or invalid references |

### Import Behavior by Mode

**Replace Mode**
1. Clears existing project data tables
2. Imports all rows from snapshot
3. Preserves runtime tables (workers, file_locks)
4. Rebuilds FTS indexes

**Merge Mode**
- Tasks: skip if ID exists, insert if new
- Dependencies: skip if exact match exists
- Attachments: append new attachments

**With --force in Merge Mode**
- Overwrites existing records instead of skipping

### Dry Run Output

Dry run validates the import and reports:
- Schema compatibility
- Task counts (new, existing, would skip/replace)
- Dependency counts
- Potential issues (warnings/errors)

```bash
$ task-graph import --dry-run snapshot.json

Import Dry Run Summary
======================
Schema version: 3 (compatible)
Mode: Replace

Tasks:
  Total in snapshot: 45
  Existing in DB: 12
  Would be added: 45 (replacing 12)

Dependencies: 38
Attachments: 15
Tags: 127

No issues found. Run without --dry-run to apply.
```

## Diff

Compare snapshot files or a snapshot against the current database state.

### Basic Diff

```bash
# Compare snapshot to current database
task-graph diff snapshot.json

# Compare two snapshots
task-graph diff old.json new.json
```

### Diff Options

```bash
# Output as JSON
task-graph diff -f json snapshot.json

# Summary counts only
task-graph diff --summary-only snapshot.json

# Show specific tables only
task-graph diff --tables tasks,dependencies snapshot.json

# Include unchanged tables in output
task-graph diff --include-unchanged snapshot.json
```

### Diff Options Reference

| Option | Description |
|--------|-------------|
| `-f, --format <FORMAT>` | Output format: text (default), json, or summary |
| `--tables <LIST>` | Only show changes for specific tables |
| `--summary-only` | Show only summary counts |
| `--include-unchanged` | Include unchanged tables |

### Diff Output Example

```
Snapshot Diff: snapshot.json vs database
=========================================

tasks:
  + node-006: "New Feature"
  ~ node-001: status: pending -> completed
  - node-003: (deleted)

dependencies:
  + node-002 -> node-006 (blocks)

Summary: 1 added, 1 modified, 1 deleted
```

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
| `schema_version` | integer | Database schema version |
| `export_version` | string | Export format version (semver) |
| `exported_at` | string | ISO 8601 timestamp |
| `exported_by` | string | Tool name and version |

### Task Record Example

```json
{
  "id": "node-001",
  "title": "Implement feature",
  "description": "Full description...",
  "status": "completed",
  "priority": 5,
  "worker_id": null,
  "claimed_at": null,
  "points": 3,
  "time_estimate_ms": null,
  "time_actual_ms": 1200000,
  "started_at": 1737481500000,
  "completed_at": 1737482700000,
  "cost_usd": 0.45,
  "metric_0": 15000,
  "metric_1": 3200,
  "deleted_at": null,
  "created_at": 1737480000000,
  "updated_at": 1737482700000
}
```

## Git Integration

### Recommended .gitignore

Add to your project's `.gitignore`:

```gitignore
# Task-graph runtime files (not project data)
.task-graph/db.sqlite
.task-graph/db.sqlite-*
.task-graph/*.log
```

### Tracking Snapshots

```bash
# Track the snapshot file
git add .task-graph/snapshot.json

# Or compressed version
git add .task-graph/snapshot.json.gz
```

### Pre-commit Hook (Optional)

Auto-export before each commit:

```bash
#!/bin/bash
# .git/hooks/pre-commit

if [ -f .task-graph/db.sqlite ]; then
  task-graph export -o .task-graph/snapshot.json
  git add .task-graph/snapshot.json
fi
```

### Workflow Example

```bash
# 1. Work on tasks throughout the day
# ... (tasks are stored in db.sqlite)

# 2. Before committing, export current state
task-graph export -o .task-graph/snapshot.json

# 3. Review changes
git diff .task-graph/snapshot.json

# 4. Commit with your code changes
git add .task-graph/snapshot.json
git commit -m "feat: implement feature X"
```

### Restoring from Git

```bash
# 1. Clone repository
git clone repo.git

# 2. Import task state from tracked snapshot
task-graph import .task-graph/snapshot.json
```

## Schema Migration

Exports include the database schema version. When importing an export with a different schema version:

```
Export v2 → Database v3

1. Validates schema compatibility
2. Applies migration transformations
3. Imports transformed data
```

### Migration Errors

If migration between versions is not supported:

```
Error: Cannot import schema v1 export into v4 database.
Supported migrations: v2→v3, v3→v4

Consider upgrading the export file to a newer schema version first.
```

## Best Practices

### Regular Exports

- Export before major operations
- Include exports in version control
- Use `--no-history` for smaller diffs when audit log is not needed

### Import Safety

- Always use `--dry-run` first for large imports
- Use `--merge` to add new items without replacing existing work
- Use `--strict` in CI/CD pipelines

### Git Integration

- Export after completing sprints or milestones
- Exclude runtime tables (workers, file_locks) are automatically excluded
- Large projects: use `.json.gz` format for efficient storage

### Metrics Convention

For imports from external systems, recommended metric slot usage:

| Slot | Purpose |
|------|---------|
| `metric_0` | tokens.input |
| `metric_1` | tokens.output |
| `metric_2` | tokens.cache_read |
| `metric_3` | tokens.cache_create |
| `metric_4` | wall_time_seconds |
| `metric_5-7` | (reserved) |

## Troubleshooting

### "Schema version mismatch"

The export was created with a different database schema version. Check if migrations are available:

```bash
task-graph import --dry-run snapshot.json
```

### "FTS rebuild failed"

Full-text search indexes couldn't be rebuilt. Usually resolves by re-running import:

```bash
task-graph import snapshot.json
```

### Large exports are slow

Use selective export or compression:

```bash
# Export only essential tables
task-graph export --tables tasks,dependencies --no-history -o snapshot.json

# Or use compression
task-graph export --gzip -o snapshot.json.gz
```

### Import creates duplicates in merge mode

This is expected behavior. Use `--force` to overwrite instead of skip:

```bash
task-graph import --merge --force snapshot.json
```
