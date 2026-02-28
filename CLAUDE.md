# CLAUDE.md

## Project Tools

This project uses the **task-graph MCP** for project and task tracking, workflows, and bug management.

## Worktree Merge Strategy

When merging parallel agent worktrees back to main, use Git's merge tooling — not manual diff+edit:

1. **Preferred: commit in worktree, then `git merge`** — Git auto-merges non-overlapping changes.
2. **`git apply --3way`** — for patch files, uses blob history for 3-way merge with conflict markers.
3. **`git cherry-pick`** — apply commits from worktree onto main. Clean for sequential application.
4. **`git merge-file -p ours base theirs > merged`** — for individual file-level conflicts.
5. **`git config rerere.enabled true`** — already enabled. Records conflict resolutions for auto-reuse.

## Build & Test

- `cargo build` / `cargo test` — ~545 tests across 10 test binaries
- Pre-commit hook runs `cargo fmt` — if commit fails, re-stage and create a NEW commit
- Migrations in `migrations/` — sequential V00N naming
