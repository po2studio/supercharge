# Product

Supercharge is a skill bundle for [omp](https://github.com/can1357/oh-my-pi): five skills that take a request from one line to merged, tested code. The human answers one batch of questions at the start and replies `go` at the end. The [README](../README.md) is the user-facing doc; this is the short version agents work from.

## Users

Developers and owners building software with strong LLM agents in omp, often starting work from an ADE's issue list (Orca). They settle business rules and approve; agents do the rest.

## Core flows

1. **Set up a project:** install the skills globally, set omp's model roles, and run `/living-docs` in the project. It asks one batch of questions, including any config key not yet set, each with its default, and writes `AGENTS.md` (with the config block) and the docs. Run it again after changes made outside the bundle.
2. **Plan a request:** ask for something, or run `/plan-feature <request>`. It triages the request as direct (done now), a ticket (one issue) or a feature (a feature doc, a feature issue, and the slices as sub-issues), asks one batch of business questions with defaults, and stops where `approve` says. A feature doc reaches `main` on its own PR.
3. **Build a feature:** start the feature issue, or run `/drive-feature <issue URL or doc>`. `build-slice` subagents build the slices, in parallel where the doc allows. The finish refreshes the docs, sets the status to `shipped`, and opens one PR into `main`.
4. **Build a slice or ticket:** start its issue, or run `/build-slice <issue URL>`: tests first, run for real, reviewed by a different model, then a PR with green CI that closes the issue.

None has an automated end-to-end test: skill text has no runtime. A real run checks a flow, written up in [`runs/`](../runs/).

## Rules

- **Config:** `risk`, `approve`, `ci_budget` and `docs`, in the `## Supercharge` block of a project's `AGENTS.md`. Missing keys take the defaults. See [Config](../README.md#config).
- **The human acts only at the `approve` steps**, by replying `go`. The agent merges; the human never does.
- **Ask about the business, decide the technical.** Business questions are asked in one batch, each with a default, as far as `risk` says; anything not asked is recorded as an assumption. A commitment outside the codebase is a `proposed` ADR the human accepts.
- **What the human wrote or approved is binding.** Conflicts are flagged, not silently resolved.
- **Issues:** every issue the bundle creates starts with the command that builds it. A feature issue is labelled `feature`, `feature:NNNN` and `ready-for-agent`. Its slices are sub-issues labelled `slice`, `feature:NNNN` and `ready-for-agent`, each linking to the feature issue. A ticket carries only `ready-for-agent`. The feature doc is the truth; the issue starts the work.
- **Docs are the current truth**, and updating them is part of done.
