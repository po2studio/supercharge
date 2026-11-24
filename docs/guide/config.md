# Config

A YAML block in a `## Supercharge` section of the project's `AGENTS.md`. `living-docs` writes it when `supercharge` first sets a project up, asking for each key with its default. A key that's still missing is asked for with the next request's questions, and takes its default until answered.

```yaml
risk: medium                 # low | medium | high; a feature doc may override this, with a reason
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```

One more key is optional and never part of the block `living-docs` writes: `brain:`, the outside folders the [brain](brain.md) reads beside the project's docs. See [Sources](brain.md#sources).

## `risk`

How costly a mistake would be. It sets the rigour (edge-case tests, how strict the review is) and how readily the agent asks about business rules:

- **`low`:** asks only when it can't go on without an answer.
- **`medium`:** asks where a wrong guess would record wrong data or be hard to undo, for example how currencies are handled.
- **`high`:** asks about any business rule that isn't written down.

## `approve`

Everything runs on its own except the steps listed in `approve`. At each one the agent stops and says what to check, what to reply, and what happens next.

| Value | Stops | You | Then |
|---|---|---|---|
| `spec` | After the feature doc's deliverables and acceptance criteria, before slicing | Check *what* gets built. Reply `go` or say what to change | It plans the slices |
| `plan` | After the slices are planned, before building | Check what gets built and how it's cut. Reply `go` or say what to change | It creates the issues. A ticket is built in a builder subagent, reviewed on a different model; a feature is built once you start its issue in a fresh session |
| `slice` | After each slice, or each batch built together, is built, reviewed and merged into the feature branch | Try it: the stop says how to run it and what to check. Reply `go` or report a problem | It fixes the problem, or starts the next slice |
| `merge` | After every slice is done and the docs are refreshed on the feature branch | Review the one PR (or branch) into `main` and its checklist of human-only checks. Reply `go` | It checks CI, merges into `main` and cleans up the branches and worktrees its agents created (in Orca, through Orca), never the one your session runs in. The feature is done |

- **`[plan, merge]` (default):** you check what gets built and how it's cut before any code is written, then see the finished feature. It catches wrong guesses about the product early, which matters most with models that don't ask questions (see [run 4](../../runs/2026-10-14-receipt-scanner-cheap-models.md)).
- **`[merge]`:** you only see the finished feature.
- **`[slice, merge]`:** test as you go; use it when you want to see each step working on a real device or real data. To see every slice on its own, add "one at a time" to the command.
- **`[]`:** no approvals; the agent merges into `main` as soon as CI passes.

Whatever `approve` says, planning a feature ends its session with a handoff: start the feature issue from Orca, or run `/drive-feature <its URL>`, in a fresh session. You never merge anything yourself: approving is replying `go` in the session.

Small changes skip the feature machinery. A direct change skips only the issue and the planning stop: a builder subagent builds it, a different model reviews it, and the docs are kept true, as for a ticket, which also gets its issue. Each gets its own PR into `main`, which waits for your `go` if `merge` is set.

## `ci_budget`

How long CI may take. When it takes longer, `build-slice` opens an issue to speed it up, if there isn't one open already.

## `docs`

Where the docs, ADRs and feature docs live; every `docs/` path in the skills means this folder. Leave it as `docs` unless `docs/` already has another job, such as a published docs site; then use something like `docs/supercharge`. `living-docs` checks for this when it asks for `docs`, and offers that as the default. `AGENTS.md` always stays at the repo root. To move the docs later, change `docs` and run `/living-docs`; it moves them and fixes the links.
