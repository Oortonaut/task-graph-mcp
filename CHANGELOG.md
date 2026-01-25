# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
