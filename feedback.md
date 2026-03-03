# Agent Feedback

---

### 2026-03-03 03:23:52 UTC | ux | suggestion

- **Agent:** coordinator

## Overlay Discovery is Broken for Agent Bootstrap

### Problem
Overlays configured in `config.yaml` (e.g. `git-worktree`, `reasoning`) are effectively invisible to agents after `list_workflows`. This means overlays have zero behavioral impact — agents never learn what the overlay expects them to do.

### Discovery paths tested and results

| Path | Shows overlays? |
|---|---|
| `list_workflows` (tool) | **Yes** — one-line descriptions only |
| `docs://workflows/list` (resource) | **No** — only 7 workflows, overlays omitted |
| `docs://overlays/{name}` | **Does not exist** (Unknown resource error) |
| `docs://workflows/{overlay-name}` | **Does not exist** |
| `docs://index` | No overlay docs among 27 entries |
| `get_prompts()` | Shows merged triggers — no attribution to overlay vs base workflow |
| `config://current` | Merged state machine — no overlay attribution |

### Specific issues

1. **No overlay detail docs.** `reasoning` says "instructs agents to document thought processes" but no prompt at any state transition actually instructs this. `git-worktree` says "generate patches, integrator applies sequentially" but no prompt explains how.

2. **`docs://workflows/list` omits overlays.** Agents using the resource path (the natural read-only discovery path) never see overlays at all.

3. **No prompt attribution.** `get_prompts(status="working")` returns merged prompts with no indication of which overlay contributed what. Can't reason about "what does this overlay add?"

4. **Overlay-promised behaviors have no corresponding prompts.** The git-worktree overlay's value prop is sequential patch application, but no prompt at `exit~working` or `enter~completed` says "generate a patch." The reasoning overlay promises decision documentation, but no prompt says "attach reasoning notes."

### Concrete impact
In a 2-round, 12-worker coordination session, both `git-worktree` and `reasoning` overlays were configured and had **zero effect** on agent behavior. Workers never generated patches (did raw git merges instead), never attached reasoning, and the coordinator had no prompt-based guidance for the patch-and-apply integration pattern.

### Suggestions

1. **Add `docs://overlays/{name}` resources** with detailed docs per overlay: workflow modifications, expected behaviors, integration patterns
2. **Include overlays in `docs://workflows/list`** or add `docs://overlays/list` as parallel resource
3. **Prompt attribution** — tag prompt sources (e.g. `[git-worktree]`) or allow `get_prompts(source="overlay-name")` filtering
4. **Inject actionable prompts from overlays.** Examples:
   - `reasoning` → on `enter~working`: "Before marking complete, attach a `note` with reasoning and alternatives considered"
   - `git-worktree` → on `exit~working`: "Generate patch: `git diff integration-branch > task-{id}.patch` and attach as `changelist`"
   - `git-worktree` → on `enter~completed` for coordinator: "Apply pending patches sequentially to integration branch"
5. **Surface active overlays in `config://current`** with per-overlay breakdown of what states/phases/prompts they contribute

