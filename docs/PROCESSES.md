# Processes

Agent-targeted reference for project processes. Follow these procedures exactly when executing releases or maintaining the changelog.

## Release Process

### Pre-Release Checklist

Run all quality gates before any release:

1. **Tests, clippy, fmt**
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```

2. **test-coverage-pro skill** — Run to analyze coverage, identify gaps, and generate missing tests. Do not proceed if critical paths lack coverage.

3. **doc-sentinel skill** — Run to audit documentation quality, detect code-doc drift, and verify completeness. Fix any drift or gaps before release.

4. **Reconciliation** — Verify consistency across all project artifacts:
   - `CHANGELOG.md` includes all user-facing changes under the correct version heading
   - `README.md` feature table matches current capabilities
   - `README.md` MCP Tools table matches implemented tools (check `src/tools/`)
   - `README.md` MCP Resources table matches implemented resources (check `src/resources/`)
   - All files in `docs/` are current with the codebase — no stale references, no missing docs for new features
   - `Cargo.toml` version matches the release version

5. **Release build verification**
   ```bash
   cargo build --release
   ```

### Publish to crates.io

```bash
cargo publish --dry-run
cargo publish
```

### Publish to GitHub

1. Tag the release commit:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

2. Create a GitHub release:
   - Title: `vX.Y.Z`
   - Body: copy the relevant section from `CHANGELOG.md`
   - Attach cross-platform binaries:
     - Linux x86_64
     - macOS Intel (x86_64)
     - macOS ARM (aarch64)
     - Windows x86_64

### Post-Release Verification

1. Verify crates.io install:
   ```bash
   cargo install task-graph-mcp
   ```
2. Check the GitHub release page — binaries present, release notes correct.

## Changelog Process

Follow [Keep a Changelog](https://keepachangelog.com/) format.

- Update `CHANGELOG.md` as changes are made, not only at release time.
- Use these categories: **Added**, **Changed**, **Fixed**, **Removed**, **Deprecated**, **Security**.
- Each entry is a single line describing the user-facing effect.
- The `[Unreleased]` section accumulates changes between releases. At release, rename it to the version and add a fresh `[Unreleased]` heading.
