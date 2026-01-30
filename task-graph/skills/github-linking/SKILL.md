---
name: github-linking
description: Processes for linking GitHub issues and PRs to task-graph tasks - bidirectional traceability between task-graph and GitHub
license: Apache-2.0
metadata:
  version: 1.0.0
  suite: task-graph-mcp
  role: integration
  requires: task-graph-basics
---

# GitHub ↔ Task Graph Linking

Conventions and processes for maintaining bidirectional traceability between GitHub issues/PRs and task-graph tasks.

**Prerequisite:** Understand `task-graph-basics` for tool reference.

---

## Overview

```
GitHub                          Task Graph
┌──────────────┐    attach      ┌──────────────┐
│  Issue #42   │◄──────────────►│  brave-fox    │
│  (bug report)│  github/issue  │  (work task)  │
└──────────────┘                └──────────────┘
       │                               │
       │  PR #99 "Fixes #42"           │  attach github/pr
       ▼                               ▼
┌──────────────┐    attach      ┌──────────────┐
│  PR #99      │◄──────────────►│  brave-fox    │
│  (fix)       │  github/pr     │  (same task)  │
└──────────────┘                └──────────────┘
```

**Key idea:** GitHub is the external record of truth. Task-graph is the internal coordination layer. Attachments bridge them.

---

## Configuration

The project config (`task-graph/config.yaml`) defines these integration primitives:

### Attachment Types

| Type | MIME | Mode | Purpose |
|------|------|------|---------|
| `github/issue` | `application/json` | replace | Link to a GitHub issue |
| `github/pr` | `application/json` | replace | Link to a GitHub PR |
| `github/comment` | `text/plain` | append | Notable comments from GitHub |

### Dependency Type

| Type | Display | Blocks | Purpose |
|------|---------|--------|---------|
| `tracks` | horizontal | none | Informational cross-reference to an external item |

### Tags

| Tag | Category | Purpose |
|-----|----------|---------|
| `github` | integration | Mark a task as linked to GitHub |
| `bug` | type | Mirror GitHub "bug" label |
| `enhancement` | type | Mirror GitHub "enhancement" label |

---

## Process 1: Link Existing Issue to New Task

When a GitHub issue exists and you want to track work in task-graph:

```
# 1. Create the task
create(
    title="Fix list_tasks ID truncation",
    description="list_tasks truncates petname IDs to 8 chars in markdown output, making them unusable",
    tags=["github", "bug"]
)
→ task_id (e.g. "brave-fox")

# 2. Attach the GitHub issue reference
attach(
    agent=worker_id,
    task="brave-fox",
    type="github/issue",
    content='{"repo": "Oortonaut/task-graph-mcp", "number": 1, "url": "https://github.com/Oortonaut/task-graph-mcp/issues/1"}'
)

# 3. (Optional) Add a comment on the GitHub issue pointing back
#    gh issue comment 1 --body "Tracked in task-graph: brave-fox"
```

### JSON Schema for github/issue

```json
{
    "repo": "owner/repo",
    "number": 42,
    "url": "https://github.com/owner/repo/issues/42",
    "title": "Original issue title",
    "labels": ["bug", "urgent"]
}
```

Only `repo` and `number` are required. The rest is convenient context.

---

## Process 2: Link PR to Existing Task

When a PR is opened to address a task:

```
# Attach the PR reference
attach(
    agent=worker_id,
    task="brave-fox",
    type="github/pr",
    content='{"repo": "Oortonaut/task-graph-mcp", "number": 99, "url": "https://github.com/Oortonaut/task-graph-mcp/pull/99"}'
)
```

### JSON Schema for github/pr

```json
{
    "repo": "owner/repo",
    "number": 99,
    "url": "https://github.com/owner/repo/pull/99",
    "branch": "fix/id-truncation",
    "closes": [42]
}
```

Only `repo` and `number` are required.

---

## Process 3: Create Task from GitHub Issue (Migration)

Bulk-link issues during project setup:

```
# 1. Fetch issues via gh CLI
#    gh issue list --repo Oortonaut/task-graph-mcp --json number,title,body,state,labels --limit 100

# 2. For each open issue, create a linked task
for issue in issues:
    task = create(
        title=issue.title,
        description=issue.body,
        tags=["github"] + map_labels(issue.labels)
    )

    attach(
        agent=worker_id,
        task=task.id,
        type="github/issue",
        content=json.dumps({
            "repo": "Oortonaut/task-graph-mcp",
            "number": issue.number,
            "url": f"https://github.com/Oortonaut/task-graph-mcp/issues/{issue.number}",
            "labels": issue.labels
        })
    )
```

---

## Process 4: Cross-Reference Between Tasks

Use the `tracks` dependency type for informational links between task-graph tasks that represent different aspects of the same GitHub issue:

```
# Task A tracks the bug investigation
# Task B tracks the fix implementation
# Both are linked to GitHub issue #42

link(from="task-a", to="task-b", type="tracks")
```

This is non-blocking — it exists purely for traceability.

---

## Querying Linked Tasks

### Find all GitHub-linked tasks

```
list_tasks(tags_any=["github"])
```

### Find a task's GitHub references

```
attachments(task="brave-fox", type="github/*")
# Returns: github/issue and github/pr attachments
```

### Search by issue number

```
search(query="#42", include_attachments=true)
# Searches attachment content for the issue number
```

### Search by repo

```
search(query="Oortonaut/task-graph-mcp", include_attachments=true)
```

---

## State Synchronization

Task-graph does not auto-sync with GitHub. Use these manual conventions:

| GitHub Event | Task Graph Action |
|--------------|-------------------|
| Issue opened | `create()` + `attach(type="github/issue")` |
| Issue closed | `update(task=id, state="completed")` |
| Issue reopened | `update(task=id, state="pending")` |
| PR opened | `attach(type="github/pr")` |
| PR merged | `update(task=id, state="completed")` if not already |
| PR closed (not merged) | No change (issue is still open) |
| Label added | Update task tags if relevant |

---

## Convention: GitHub Comment Back-Link

After linking a task to a GitHub issue, leave a comment on the issue for human team members:

```bash
gh issue comment 42 --body "Tracked internally: task-graph \`brave-fox\`"
```

This creates a two-way paper trail without requiring any automation.

---

## Convention: Commit Messages

Reference both systems in commit messages when applicable:

```
Fix ID truncation in list_tasks markdown output

Fixes #42
Task: brave-fox
```

Use the `commit` attachment type to record the commit hash on the task:

```
attach(agent=worker_id, task="brave-fox", type="commit", content="abc123f")
```

---

## Label-to-Tag Mapping

When importing issues, map GitHub labels to task-graph tags:

| GitHub Label | Task Graph Tag | Category |
|-------------|----------------|----------|
| `bug` | `bug` | type |
| `enhancement` | `enhancement` | type |
| `documentation` | (use task description) | — |
| `good first issue` | (use low priority) | — |
| `help wanted` | (use `wanted_tags`) | — |

Not every label needs a tag. Only map labels that affect task routing or filtering.

---

## Related Skills

| Skill | When to Use |
|-------|-------------|
| `task-graph-basics` | Tool reference, task trees |
| `task-graph-migration` | Bulk import from GitHub Issues |
