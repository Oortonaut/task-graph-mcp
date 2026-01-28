# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
