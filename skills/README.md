# Task Graph MCP Skills

Claude Code skills for multi-agent task coordination with task-graph-mcp.

## Skills in this Suite

| Skill | Role | Description |
|-------|------|-------------|
| `task-graph-basics` | Foundation | Shared patterns, tool reference, connection workflow |
| `task-graph-coordinator` | Orchestrator | Create task trees, assign work, monitor progress |
| `task-graph-worker` | Executor | Claim tasks, report progress, complete work |
| `task-graph-reporting` | Analytics | Generate reports, track costs and velocity |
| `task-graph-migration` | Import | Migrate from GitHub Issues, Linear, Jira, markdown |
| `task-graph-repair` | Maintenance | Fix orphaned tasks, broken deps, stale claims |

## Installation

### Quick Install (default location)

```bash
# Unix/macOS
python skills/scripts/install.py

# Windows
python skills\scripts\install.py

# Or use shell wrappers
./skills/scripts/install.sh           # Unix/macOS
.\skills\scripts\install.ps1          # Windows PowerShell
```

Skills are installed to `~/.claude/skills/` by default.

### Custom Location

```bash
# Install to custom directory
python skills/scripts/install.py --target /path/to/skills/

# Windows
python skills\scripts\install.py --target C:\path\to\skills
```

### Selective Install

```bash
# Install only specific skills
python skills/scripts/install.py --skills task-graph-basics,task-graph-worker

# Preview what would be installed
python skills/scripts/install.py --dry-run

# List available skills
python skills/scripts/install.py --list
```

### Uninstall

```bash
python skills/scripts/install.py --uninstall
```

## Usage

After installation, skills are available in Claude Code:

```
# View skill help
/task-graph-basics
/task-graph-coordinator
/task-graph-worker

# Or reference in prompts
"Use the task-graph-worker skill to claim and complete tasks"
```

## Skill Dependencies

```
task-graph-basics (foundation)
    ├── task-graph-coordinator
    ├── task-graph-worker
    ├── task-graph-reporting
    ├── task-graph-migration
    └── task-graph-repair
```

All skills reference `task-graph-basics` for shared tool documentation.

## Multi-Agent Workflows

### Coordinator + Workers

1. **Coordinator** creates task tree with tag requirements
2. **Workers** connect with capability tags
3. Workers claim matching tasks via `list_tasks(ready=true, agent=id)`
4. Workers report progress via `thinking()`
5. Coordinator monitors via `list_tasks()` and `list_agents()`

### Example Session

**Coordinator:**
```
connect(name="coordinator", tags=["planning"])
create_tree(tree={
  "title": "Feature X",
  "join_mode": "then",
  "children": [
    {"title": "Design", "agent_tags_all": ["design"]},
    {"title": "Implement", "agent_tags_all": ["backend"]},
    {"title": "Test", "agent_tags_all": ["testing"]}
  ]
})
```

**Worker (backend):**
```
connect(name="backend-dev", tags=["backend", "rust"])
list_tasks(ready=true, agent=agent_id)  # Finds "Implement"
claim(agent=agent_id, task=implement_task_id)
thinking(agent=agent_id, thought="Working on implementation...")
# ... do work ...
complete(agent=agent_id, task=implement_task_id)
```

## MCP Resources (Built-in)

Skills are also exposed via MCP resources - no installation needed:

```
# List available skills
skills://list

# Get specific skill content
skills://basics
skills://coordinator
skills://worker
skills://reporting
skills://migration
skills://repair
```

### Overriding Built-in Skills

Place custom skills in `.task-graph/skills/` to override embedded ones:

```
.task-graph/
├── skills/
│   ├── task-graph-basics/     # Overrides embedded basics
│   │   └── SKILL.md
│   └── my-custom-skill/       # Add new skills
│       └── SKILL.md
```

The server checks for overrides first, then falls back to embedded skills.

## Requirements

- Python 3.8+
- Claude Code with skills support
- task-graph-mcp server running

## License

Apache-2.0 (same as task-graph-mcp)
