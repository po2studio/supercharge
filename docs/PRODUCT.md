# Product

Supercharge is a skill bundle for [omp](https://github.com/can1357/oh-my-pi): five skills that take a request from one line to merged, tested code. The human answers one batch of questions and checks the plan at the start, and replies `go` at the end. The [README](../README.md) is the user-facing doc; this is the short version agents work from.

## Users

Developers and owners building software with strong LLM agents in omp, often starting work from an ADE's issue list (Orca). They settle business rules and approve; agents do the rest.

## Core flows

1. **Start a request:** install the skills globally and set omp's model roles once; then every request is `/supercharge <request>`: an idea, a bug, a change or a feature. In a project that isn't set up, it first runs `living-docs`, which writes `AGENTS.md` (with the config block) and the docs, and stops once for the human to check them and answer its questions, each with its default. Then it routes the request:
   - **direction open, or the human asks to explore:** a brainstorm with the human; nothing is written and no issue created until they settle the direction, and "not now" writes nothing;
   - **new work:** triaged as direct (done now), a ticket (one issue, built in the same session) or a feature (a feature doc, a feature issue, and the slices as sub-issues), with one batch of business questions and the stops `approve` sets. A feature doc reaches `main` on its own PR, and the session ends with a stop saying to start the feature issue in a fresh session;
   - **planned work:** handed to `drive-feature` or `build-slice`;
   - **drifted docs:** handed to `living-docs`.
2. **Build a feature:** start the feature issue, or run `/drive-feature <issue URL or doc>`. `build-slice` subagents build the slices, in parallel where the doc allows. The finish refreshes the docs, sets the status to `shipped`, and opens one PR into `main`.
3. **Build a slice or ticket:** start its issue, or run `/build-slice <issue URL>`: tests first, run for real, reviewed by a different model, then a PR with green CI that closes the issue.

None has an automated end-to-end test: skill text has no runtime. A real run checks a flow, written up in [`runs/`](../runs/).

## Rules

- **Config:** `risk`, `approve`, `ci_budget` and `docs`, in the `## Supercharge` block of a project's `AGENTS.md` (a block under the old `## Sprengikraftur` counts). A missing key is asked for with the next request's questions, and takes its default until answered. See [Config](../README.md#config).
- **The human acts only at the `approve` steps**, by replying `go`. The agent merges; the human never does.
- **Ask about the business, decide the technical.** Business questions are asked in one batch, each with a default, as far as `risk` says; the one exception is a brainstorm, a back-and-forth until the human settles the direction; anything not asked is recorded as an assumption. A commitment outside the codebase is a `proposed` ADR the human accepts.
- **What the human wrote or approved is binding.** Conflicts are flagged, not silently resolved.
- **Issues:** every issue the bundle creates starts with the command that builds it. A feature issue is labelled `feature`, `feature:NNNN` and `ready-for-agent`. Its slices are sub-issues labelled `slice`, `feature:NNNN` and `ready-for-agent`, each linking to the feature issue. A ticket carries only `ready-for-agent`. The feature doc is the truth; the issue starts the work.
- **Docs are the current truth**, and updating them is part of done.
