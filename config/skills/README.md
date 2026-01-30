# Task Graph MCP Skills

Claude Code skills for multi-agent task coordination with task-graph-mcp.

## Skills in this Suite

| Skill | Role | Description |
|-------|------|-------------|
| `task-graph-basics` | Foundation | Shared patterns, tool reference, connection workflow, task trees, search |
| `task-graph-reporting` | Analytics | Generate reports, track costs and velocity |
| `task-graph-migration` | Import | Migrate from GitHub Issues, Linear, Jira, markdown |
| `task-graph-repair` | Maintenance | Fix orphaned tasks, broken deps, stale claims |

> **Coordination patterns** (roles, phases, states, push/pull models, gates)
> are defined by workflow configs, not skills. See `config/workflow-*.yaml` and
> `docs/WORKFLOW_TOPOLOGIES.md`.

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
python skills/scripts/install.py --skills task-graph-basics,task-graph-reporting

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
/task-graph-reporting

# Or reference in prompts
"Use the task-graph-basics skill to understand available tools"
```

## Skill Dependencies

```
task-graph-basics (foundation)
    ├── task-graph-reporting
    ├── task-graph-migration
    └── task-graph-repair
```

All skills reference `task-graph-basics` for shared tool documentation.

## Multi-Agent Workflows

Coordination patterns are defined by workflow configs loaded at connect time:

| Workflow | Pattern | Use Case |
|----------|---------|----------|
| `workflow-solo.yaml` | Single agent | Prototyping, simple projects |
| `workflow-hierarchical.yaml` | Lead + worker pool | Complex projects, mixed expertise |
| `workflow-swarm.yaml` | Parallel generalists | Parallelizable work, large backlogs |
| `workflow-relay.yaml` | Specialist handoffs | Formal processes, review gates |
| `workflow-push.yaml` | Coordinator assigns all | Experiments, tight control |

See `docs/WORKFLOW_TOPOLOGIES.md` for topology selection guidance.

## MCP Resources (Built-in)

Skills are also exposed via MCP resources - no installation needed:

```
# List available skills
skills://list

# Get specific skill content
skills://basics
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
