---
name: build-slice
description: Build one vertical slice to done — tests first, a real end-to-end check, an independent review by a different model, a PR with green CI, and updated docs. Use for a ready-for-agent issue, a slice from a feature doc, or a small well-defined request.
---

# build-slice

Ship one slice so it's verifiably done and the docs still tell the truth.

Build only this slice. Anything else you notice goes in the report.

`docs/` here means the folder set by `docs` in `AGENTS.md` (default `docs`).

## Done means

- Acceptance tests came first: one per acceptance criterion, at the outermost level that shows it (the browser for a UI flow, the API for a backend rule), seen failing. The inside was then test-driven until they passed.
- The real thing works end to end: run it, and use the browser for UI.
- Unit tests written while test-driving stay; only the slice's acceptance tests are deleted once they pass (git keeps them).
- The permanent end-to-end suite has one happy-path test per core flow in `docs/PRODUCT.md`, through the surface users use: the browser for a UI (an HTTP-only test doesn't count), the API or CLI otherwise. If this slice adds or changes a core flow, update that flow and its test, and flag it. With no end-to-end harness yet, the first slice that touches a core flow sets one up and records how to run it in `AGENTS.md`.
- Every external service sits behind an interface with a fake, and the whole thing runs offline on the fakes.
- An independent review passed: omp's `reviewer` agent (it runs on omp's `slow` role, a different model from the builder), given the acceptance criteria and the diff. Fix the findings, or record why not.
- A PR is open (omp's `github` tool), with CI green. Before opening it, merge the target branch in and run the checks again: slices built in parallel edit the same docs. Fix failures and push until it passes. It targets the feature's branch (`feature/NNNN-*`) if one exists, otherwise `main`. Given an issue, the PR description says `Closes #N` for it; GitHub acts on that only for PRs into the default branch, so when you merge into a feature branch, close the issue with `gh issue close`. With no GitHub remote, use local branches and run the checks in `AGENTS.md` in place of CI.
- The docs match reality, and stay short: `docs/PRODUCT.md` (what users see, not the numbers behind it) and `docs/ARCHITECTURE.md` updated, ADRs written for decisions that are hard to reverse, superseding any they contradict, and the slice ticked off in the feature doc.

## How to work

- Read `AGENTS.md` and the relevant docs first; given a slice issue, also the feature doc its feature issue links to. What's written is binding; if something looks wrong, flag it rather than working around it.
- If the slice depends on one that isn't done (its issue is open; with no tracker, it isn't ticked off in the feature doc), stop and say which to build first.
- Make technical decisions yourself and keep a decision ledger: what you decided, and what it costs if wrong. It goes in the PR description.
- Ask about a business rule the docs and acceptance criteria don't settle only as far as `risk` says: `low`, when you can't go on without an answer; `medium`, where a wrong guess records wrong data or is hard to undo; `high`, any rule that isn't written down. Ask in one batch, each question with your default, and record any rule you don't ask about as an assumption in the ledger. Before anything that commits outside the codebase (a new service or vendor, cost, data leaving the system), write a `proposed` ADR and stop for the human to accept it; on `go`, set it `accepted`. Technical questions are yours.
- Let risk set the rigour: higher risk gets more edge-case tests and a stricter review. Rigour scales with the cost of being wrong, not with what's possible.
- Build for what exists. Don't build compatibility for data, users or releases that don't exist yet.
- If CI exceeds `ci_budget`, open an issue to speed it up, unless one is already open.
- If the same failure or review finding survives three fixes, stop and say so.
- Merge into a feature branch yourself, then delete the slice branch, remote and local. A PR into `main` waits for the human's approval if `approve` includes `merge`; once they approve, check CI is green, merge it yourself, and clean up the branch. Remove a worktree you added for this slice once you're done with it and anything in it is merged; keep one that isn't, and say where it is in the report. Clean up only this slice's own branches and worktrees, never leftovers from earlier work, and never anything holding uncommitted changes or unmerged commits. In an ADE such as Orca, leave the session's worktree and the branch checked out there: removing them breaks the ADE's sessions, so the human removes them there. At the stop, say exactly what to do, what to check, and what happens after ("reply `go` and I'll …").
- Unless `drive-feature` runs you, if this slice is its feature's last open one (the feature issue has no other open sub-issues), end your stop and report with: start feature issue #N in Orca (or run `/drive-feature <its URL>`) to finish. `drive-feature` skips the done slices and only does the finish.
- Don't work around security controls (commit signing, auth, hooks). Stop and say so.
- If an omp feature named here isn't available, use an equivalent. If there isn't one, stop and say so.

## Report

What shipped, the PR link, the decision ledger, anything flagged, and what the human should look at. Report what you checked, not what you suspect; if you couldn't check something, say so.
