# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-01-31

### Added

- **Agent feedback tools**: `give_feedback` and `list_feedback` for inter-agent communication during workflows
- **Dynamic overlay management**: `add_overlay` and `remove_overlay` tools for runtime workflow customization
- **Worker overlays**: Agents can request overlays on `connect` for role-specific workflow behavior
- **Troubleshooting overlay**: Pre-built overlay for debugging agent issues (`overlay-troubleshooting.yaml`)
- **Git overlay**: Pre-built overlay for git-aware workflows (`overlay-git.yaml`)
- **Offset pagination**: `offset` parameter for `list_tasks` with `has_more` metadata
- **Skill frontmatter parsing**: Skill index listings now include `description` from SKILL.md frontmatter
- **Kanban workflow**: New `workflow-kanban.yaml` topology for board-style task management
- **Sprint workflow**: New `workflow-sprint.yaml` topology for time-boxed iteration planning
- **MCP registry publishing**: MCPB bundle generation and `server.json` in release workflow
- **Browser project template**: Template for coordination experiments
- **Experiment and design documentation**: Distributed swarm, authorization, database backends, experiment framework

### Changed

- Basics skill rewritten to v2.0.0 with unified coordinator/worker guidance
- Workflow role sections expanded across all topologies (solo, swarm, relay, hierarchical, push)
- Agent feedback enabled in default project config
- Overlay file changes detected by config watcher and hot-reloaded

### Removed

- `task-graph-coordinator` skill (merged into basics v2.0.0)
- `task-graph-worker` skill (merged into basics v2.0.0)

### Fixed

- Flaky child ordering test (deterministic rowid tiebreaker)
- Release notes preservation in CI workflow
- Clippy warnings: `map_or` → `is_some_and`, `len() > 0` → `!is_empty()`, field reassignment, unnecessary deref

## [0.2.2] - 2026-01-29

### Added

- **Template system**: Template instantiation with entry/exit point detection, ID remapping, parent attachment, and builder-pattern options
- **Rename tool**: Atomically rename task IDs across all 9 referencing tables
- **List workflows tool**: Discover available workflow configurations before connecting
- **Get schema tool**: Inspect database schema, columns, types, and foreign keys
- **Documentation search**: `docs://search/{query}` full-text search and `docs://index` listing via MCP resources
- **Workflow roles**: Role definitions with tags, permissions, and role-specific prompts delivered on connect and claim
- **Config hot-reload**: File watcher for config/workflow/skills changes with debounced atomic reload via ArcSwap
- **Context-sensitive prompts**: 7 template variables (`task_id`, `task_title`, `task_priority`, `task_tags`, `agent_id`, `agent_role`, `agent_tags`) for richer transition guidance
- **MCP resource subscriptions**: Change notification system for resource URIs
- **Import enhancements**: `--parent` flag for attaching imports under parent tasks, `--remap-ids` for fresh ID generation
- **GitHub linking skill**: Bidirectional linking between GitHub issues/PRs and task-graph tasks via attachments, tags, and `tracks` dependency type
- **MIME type validation**: RFC 6838-compliant validation and 255-char filename limits for attachments
- **Experiment framework**: Push/pull/hybrid experiment configs, workflow-push topology, runner and comparison scripts, browser task templates
- **Process documentation**: `docs/PROCESSES.md` with release process and changelog maintenance

### Changed

- **BREAKING**: MCP resource URIs consolidated from 9 schemes to 3 (`query://`, `config://`, `docs://`)
- Default generated IDs shortened from 4 words to 2; PascalCase for agent IDs via `agent_id_case` config
- Workflow role tags auto-registered in TagsConfig to suppress unknown tag warnings
- Title truncation and tiered priority markers in list/scan output
- Container tasks excluded from ready task queries
- `TaskTreeInput.title` now optional (derived from description)
- Token-optimized workflow prompts (~33% reduction across all topologies)
- Skill system simplified: removed approval/trust gate, skills served directly
- Unified string-or-array parsing across dependency, attachment, task, tracking, and file tools

### Fixed

- Template test fixtures using integer priority instead of string
- 31 clippy warnings resolved (collapsible_if, too_many_arguments, result_large_err, redundant_closure, type_complexity)

## [0.2.1] - 2026-01-28

### Added

- **Consult status**: Non-timed, blocking state for human review. Workers transition from `working` to `consult` when they need human input; task releases ownership and blocks dependents until moved back
- **Pagination**: `offset` parameter and metadata (`has_more`, `total_count`) for `list_tasks` and `search` tools. Configurable `default_page_size` (default: 50)
- **Config file watcher**: File watcher using `notify` crate for detecting config/workflow/skills changes with debouncing
- **Docs resource handler**: `docs://` resource scheme for serving markdown files from `docs/` directory
- **Config resources**: `config://` resource scheme for accessing current configuration, states, phases, dependencies, and tags
- **Metrics documentation**: `docs/METRICS.md` with experiment metrics definitions and SQL examples
- **Workflow gates**: Exit requirements for all workflow topologies (solo: warn, swarm: allow, relay: reject, hierarchical: warn)
- **Worker coordination guidance**: Anti-revert rules, scope estimation, file conflict detection, and pre-refactoring guidance in workflow prompts
- **Heartbeat/polling guidance**: Workflow-specific `thinking()` and `mark_updates()` reminders

### Changed

- Workflow prompts optimized for conciseness (prose headers converted to bullet summaries)
- `workflow-solo.yaml` set as default workflow
- Removed generic `workflows.yaml` in favor of topology-specific workflows

### Fixed

- Dashboard starting regardless of `ui.mode` setting (exhaustive match on `UiMode` enum)
- `claim()` now returns structured `blocked_by` info when failing on unsatisfied dependencies
- MCP `list_resources()` now returns all defined resources
- 74 clippy warnings resolved

## [0.2.0] - 2026-01-28

### Added

- **Phases**: New `phase` field on tasks to categorize work type (explore, implement, review, test, security, deploy, etc.)
- **Unified Workflows**: Consolidated states, phases, and prompts into `workflows.yaml` configuration
- **Transition Prompts**: Automatic agent guidance on status and phase changes with template variables (`{{current_status}}`, `{{valid_exits}}`, `{{current_phase}}`, `{{valid_phases}}`)
- **Combo Prompts**: State+phase combination prompts for context-specific guidance (e.g., `working+implement`)
- **Workflow Gates**: Exit requirements for status/phase transitions with enforcement levels (allow, warn, reject)
- **Named Workflows**: Pre-built workflow topologies with detailed guidance:
  - `solo` - Single agent workflows
  - `swarm` - Parallel generalists with fine-grained tasks
  - `relay` - Sequential specialists with handoffs
  - `hierarchical` - Lead/worker delegation patterns
- **Per-Worker Workflow Selection**: Agents can select workflow on `connect(workflow="swarm")`
- **Workflow Resources**: MCP resources for accessing workflow configurations
- **Configurable ID Generation**: Control word count (`task_id_words`, `agent_id_words`) and case style (`kebab-case`, `snake_case`, `camelCase`, `PascalCase`, etc.)
- **Comprehensive Configuration Documentation**: Full reference in `docs/CONFIGURATION.md`
- **Attachment Guidance**: Workflow-specific attachment recommendations with examples

### Changed

- **BREAKING**: Status `in_progress` renamed to `working`
- **BREAKING**: Attachment `name` field now split into `type` (indexed category) and `name` (optional label) for better querying
- Configuration merged from multiple tiers: environment > CLI > project > user > defaults
- Documentation moved to `docs/` folder
- 4-word petnames from large wordlist for more unique generated IDs

### Fixed

- Server resilience: dashboard startup is now non-fatal with automatic retry
- Critical crash points fixed for improved server stability
- Soft dependency linking now supports warnings for informational deps

## [0.1.1] - 2026-01-25

### Added

- Path override arguments to `connect` tool for flexible database location

### Fixed

- BUG-001: `needed_tags`/`wanted_tags` now properly stored on task create
- BUG-002: `claim` now enforces blocking dependencies correctly
- BUG-003/004: `end_timestamp` column added to `claim_sequence` schema
- BUG-005: Attachments MIME filter escape sequence fixed

### Changed

- Remove `max_claims` enforcement (now unlimited)
- Remove `user_metrics` field from Task (use `metrics` array instead)
- Consolidate all migrations into V001 for cleaner schema

## [0.1.0] - 2026-01-25

Initial release.

### Added

- Task graph management with parent-child hierarchies
- Typed dependency edges (blocks, needs, suggests)
- Worker coordination with claim/release mechanics
- Advisory file marks for conflict prevention
- Configurable task states with automatic time tracking
- Tag-based worker affinity matching
- Full-text search across tasks
- Attachments system with MIME types and storage modes
- Bundled skills with MCP resources
- Structured error codes for tool responses
- Markdown tree formatting for task hierarchies
- `relink` tool for atomic dependency moves
