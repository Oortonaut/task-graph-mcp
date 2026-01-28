## What your MCP already supports (relevant to this design)

Task Graph already has the right "coordination primitives" for scaling role/state complexity: DAG dependencies with cycle detection, atomic claiming, configurable states/transitions (including `blocking_states`, `timed`), tag-based routing (`needed_tags`/`wanted_tags`), advisory file marks with polling, attachments, and project/task history/metrics. 

That means you can implement richer workflows either as:

* **More states**, or
* **Fewer states + orthogonal metadata** (phase, gates, checklists) encoded as tags/attachments.

## Recommendation: don't encode everything as a single linear state machine

Your proposed pipeline `[explore, design, plan/task out, implement, test, document, security, review, integrate, done]` is a good mental model, but it becomes brittle as a *single* global state enum because:

* You'll want **parallelism** (docs/test/security can overlap).
* You'll want **quality loops** (review