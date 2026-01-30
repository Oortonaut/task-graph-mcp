---
name: task-graph-migration
description: Migration utilities for task-graph-mcp - import tasks from GitHub Issues, Linear, Jira, markdown TODOs, and other task systems
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: migration
  requires: task-graph-basics
---

# Task Graph Migration

Import tasks from external systems: GitHub Issues, Linear, Jira, markdown TODOs, and plain text task lists.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Quick Start

```
# 1. Connect
connect(name="migrator", tags=["migration"])
→ agent_id

# 2. Create root task for migration batch
create(title="Migrated: {source}")
→ root_id

# 3. For each source task, create in task-graph
create(parent=root_id, title="...", description="...")

# 4. Preserve relationships
block(blocker=task_a, blocked=task_b)
```

---

## Source Formats

### GitHub Issues

```
Source: gh issue list --json number,title,body,state,labels

Mapping:
  issue.title      → task.title
  issue.body       → task.description
  issue.state      → task.status (open→pending, closed→completed)
  issue.labels     → task.agent_tags_all
  issue.milestone  → parent task
```

### Linear

```
Source: Linear API or export

Mapping:
  issue.title       → task.title
  issue.description → task.description
  issue.state       → task.status
  issue.priority    → task.priority (1→critical, 2→high, 3→medium, 4→low)
  issue.estimate    → task.points
  issue.parent      → parent relationship
  issue.blockedBy   → block() dependencies
```

### Jira

```
Source: Jira API or CSV export

Mapping:
  issue.summary     → task.title
  issue.description → task.description
  issue.status      → task.status
  issue.priority    → task.priority
  issue.storyPoints → task.points
  issue.epic        → parent task
  issue.links       → block() dependencies
```

### Markdown TODOs

```
Source: Any markdown file with task lists

Pattern:
  - [ ] Task title
  - [x] Completed task
  - [ ] Task with @tag

Mapping:
  [ ]  → status: pending
  [x]  → status: completed
  @tag → agent_tags_all
  indent level → parent/child hierarchy
```

### Plain Text

```
Source: Numbered or bulleted lists

Pattern:
  1. First task
  2. Second task (depends on 1)
  3. Third task

Mapping:
  Sequential numbers → sibling_type: "follows" (top-level param)
  Parallel items → sibling_type: null (or omit)
  Indentation → hierarchy
```

---

## Migration Workflow

```
┌─────────────────────────────────────────────────────┐
│ 1. ANALYZE SOURCE                                   │
│    • Identify task format                           │
│    • Map fields to task-graph schema                │
│    • Identify relationships (parent, deps)          │
├─────────────────────────────────────────────────────┤
│ 2. CREATE STRUCTURE                                 │
│    • Create root task for migration batch           │
│    • Create parent tasks (epics/milestones)         │
│    • Preserve hierarchy                             │
├─────────────────────────────────────────────────────┤
│ 3. MIGRATE TASKS                                    │
│    • Create each task with mapped fields            │
│    • Attach original data as reference              │
│    • Track source→target ID mapping                 │
├─────────────────────────────────────────────────────┤
│ 4. RESTORE RELATIONSHIPS                            │
│    • Add dependencies via block()                   │
│    • Set parent relationships                       │
│    • Apply tag requirements                         │
├─────────────────────────────────────────────────────┤
│ 5. VERIFY                                           │
│    • Count tasks migrated                           │
│    • Check for broken references                    │
│    • Validate hierarchy                             │
└─────────────────────────────────────────────────────┘
```

---

## Migration Patterns

### GitHub Issues Migration

```
# 1. Get issues (via gh CLI or API)
issues = `gh issue list --json number,title,body,state,labels,milestone`

# 2. Create root
root = create(title="GitHub Migration: repo-name")

# 3. Group by milestone
for milestone in milestones:
    parent = create(parent=root, title=milestone.title)

    for issue in milestone.issues:
        create(
            parent=parent,
            title=issue.title,
            description=issue.body,
            status="completed" if issue.state == "closed" else "pending",
            agent_tags_all=issue.labels
        )

# 4. Preserve issue links as attachments
attach(task=task_id, name="github-issue",
       content=f"#{issue.number}", mime="text/plain")
```

### Markdown TODO Migration

```
# Source markdown:
# - [ ] Design API
#   - [ ] Define endpoints
#   - [ ] Write OpenAPI spec
# - [ ] Implement
# - [x] Deploy

# 1. Parse structure
tasks = parse_markdown_todos(content)

# 2. Create hierarchy
root = create(title="Migrated TODOs")

for task in tasks:
    parent_id = id_map.get(task.parent) or root

    new_task = create(
        parent=parent_id,
        title=task.title,
        status="completed" if task.checked else "pending",
        agent_tags_all=task.tags
    )

    id_map[task] = new_task.id
```

### Sequential List Migration

```
# Source:
# 1. First thing
# 2. Second thing (after first)
# 3. Third thing (after second)

# Use create_tree with sibling_type: follows
create_tree(tree={
    "title": "Migrated List",
    "children": [
        {"title": "First thing"},
        {"title": "Second thing"},
        {"title": "Third thing"}
    ]
}, sibling_type="follows")
# Automatically creates sequential dependencies
```

---

## Field Mapping Reference

### Status Mapping

| Source | task-graph |
|--------|------------|
| open, todo, backlog | pending |
| in progress, doing, active | working |
| done, closed, complete | completed |
| blocked, on hold | pending + blocked_by |
| cancelled, wontfix | cancelled |

### Priority Mapping

| Source | task-graph |
|--------|------------|
| P0, Critical, Highest, 1 | critical |
| P1, High, 2 | high |
| P2, Medium, Normal, 3 | medium |
| P3, Low, Minor, 4 | low |

### Estimate Mapping

| Source | task-graph |
|--------|------------|
| Story points | points |
| Hours/days | time_estimate_ms (convert) |
| T-shirt sizes | points (XS=1, S=2, M=3, L=5, XL=8) |

---

## Preserving Metadata

Store original data for reference:

```
# Attach original issue data
attach(
    task=new_task_id,
    name="source-metadata",
    content=json.dumps({
        "source": "github",
        "issue_number": 123,
        "original_url": "https://github.com/...",
        "migrated_at": timestamp,
        "original_labels": ["bug", "urgent"]
    }),
    mime="application/json"
)
```

---

## Handling Relationships

### Parent-Child (Epics/Subtasks)

```
# Create parent first
epic = create(title="Epic: Feature X")

# Create children with parent reference
create(parent=epic.id, title="Subtask 1")
create(parent=epic.id, title="Subtask 2")
```

### Dependencies (Blocks/Blocked By)

```
# After creating all tasks, add dependencies
# Using source→target ID mapping

for dep in source_dependencies:
    blocker_id = id_map[dep.blocks]
    blocked_id = id_map[dep.blocked_by]

    block(blocker=blocker_id, blocked=blocked_id)
```

### Labels → Tags

```
# Convert labels to tag requirements
create(
    title="Task title",
    agent_tags_all=source.labels.filter(is_skill_tag),  # ["backend"]
    agent_tags_any=source.labels.filter(is_optional)   # ["help-wanted"]
)
```

---

## Verification Checklist

After migration:

- [ ] Task count matches source
- [ ] Hierarchy preserved (check parent relationships)
- [ ] Dependencies restored (no orphaned blocks)
- [ ] Statuses mapped correctly
- [ ] Tags applied
- [ ] Original data attached
- [ ] No duplicate tasks

### Verification Queries

```
# Count total
list_tasks()  # Compare to source count

# Find orphans (no parent, not intentional root)
list_tasks(parent="null")

# Find broken deps
list_tasks(blocked=true)  # Check if blockers exist

# Verify completed
list_tasks(status="completed")  # Compare to source
```

---

## Rollback

If migration fails:

```
# Delete migration batch
delete(task=migration_root_id, cascade=true)
# Removes root and all descendants
```

---

## Best Practices

| Do | Don't |
|----|-------|
| Create root task for batch | Mix migrated with native tasks |
| Preserve source IDs | Lose traceability |
| Migrate in dry-run first | Migrate production directly |
| Verify counts | Assume success |
| Keep original metadata | Discard source data |

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference, task trees |
| `task-graph-repair` | Fix migration issues |
