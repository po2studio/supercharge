---
name: drive-feature
description: Build a planned feature end to end by running build-slice for each of its slices, then report back once. Use when a feature doc or feature issue with slices exists and the human wants it built.
---

# drive-feature

Get a whole feature built while the human is away, then hand back one clear report.

Don't build slices yourself: each one runs as `build-slice` in its own subagent.

`docs/` here means the folder set by `docs` in `AGENTS.md` (default `docs`).

## How to work

- Read `AGENTS.md`. Work on the feature's branch `feature/NNNN-short-slug`: the existing one if an earlier session started it, otherwise a new one from `main`.
- On it, read the feature doc (given a feature issue, the doc it links to). Slices already ticked off in it are done; skip them. If none are left, go straight to the finish.
- Build each slice with `build-slice` in its own fresh subagent, not a fork (in omp a `task` subagent; in Claude Code, model `sonnet`), in dependency order. Before starting builders, make sure the feature branch exists locally, so each builder can start its slice's branch from it. Keep your own context small: take back only each slice's report, never its diffs. If a slice's subagent dies, resume it or start a new one from where it stopped. Once a slice is merged into the feature branch, release its builder so its worktree is removed, and check your own checkout is still clean. Keep the worktree of a slice that isn't merged, or that holds uncommitted changes, and say where it is in the report.
- By default, build in parallel the slices the feature doc marks as buildable together, each in an isolated worktree (in Claude Code, `isolation: "worktree"`). If the doc doesn't mark them, build in parallel only slices that share no screen, table, module or rule.
- If the human asks for only slice N, build just that slice, then stop; don't start other slices or the finish. If they give a limit ("build 3 slices", "at most 2 at once", "one at a time"), keep to it. After that many slices, stop as for `slice` below, and on `go` carry on with the next batch.
- If `approve` includes `slice`, stop after each slice, or each batch built together, is merged into the feature branch so the human can try it: how to run it, what to try (its acceptance criteria), and the checks only a human can do. On `go`, start the next; if they report a problem, fix it first.
- Stop for the human only at a step listed in `approve` (read it from `AGENTS.md` when you get there; the human may have changed it), or when a question blocks the work, including a `proposed` ADR from a builder: pass it to the human, and the answer back. When you stop, say exactly what to do, what to check, and what happens after ("reply `go` and I'll …"). Otherwise keep going: park the blocked slice and carry on with the others.

## Finish

- Check that every core flow the feature doc lists has a passing happy-path end-to-end test, as `build-slice` defines it. Get a missing one built with `build-slice`; don't skip it. Flag gaps from before this feature in the report.
- Check that the docs describe what actually shipped: PRODUCT, ARCHITECTURE, ADRs, the feature doc. Fix any drift you find in them, including drift from before this feature. Check new ADRs against older ones; supersede any they contradict. Where docs conflict on a product rule, say which rule the code follows today, and flag it.
- Then run the `living-docs` skill on the feature branch, even if the docs look current. Set the status to `shipped`.
- Open one PR from the feature branch into `main`, with the report as its description, `Closes #N` for the feature issue if there is one, and the checks only a human can do (real devices, real files) as a checklist. If `approve` includes `merge`, stop for the human's approval; once they approve, check CI is green, merge it yourself, and clean up the branches and worktrees this run created, never the worktree your own session runs in or the branch checked out there. Otherwise merge it once CI passes.
- Hand back one report:
  - what shipped, with links
  - the combined decision ledger
  - anything flagged or parked
  - what needs human review, most important first
  - last, the stop: what the human does now, and what happens after. Once nothing is left, say to start a new session for the next request; the docs carry everything.
