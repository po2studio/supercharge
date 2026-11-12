---
name: supercharge
description: The entry point for any request in a project — an idea, a bug, a change or a feature. Sets the project up on first use, brainstorms when the direction is open, triages and plans new work (a fix done now, a ticket, or a feature doc with a GitHub feature issue and its slices as sub-issues), and hands planned work to drive-feature or build-slice.
---

# supercharge

Take any request to its right next step, and turn new work into something an agent can build and a human can check.

Only route and plan: don't write code. Building happens in `build-slice` or `drive-feature`. The one exception is the `direct` route below.

`docs/` here means the folder set by `docs` in `AGENTS.md` (default `docs`).

## Route the request

Read `AGENTS.md`, then say which route you chose:
- **Not set up** (no `## Supercharge` config block in `AGENTS.md`): run `living-docs`, drafting on its defaults so its questions and the docs it wrote go to the human in one stop. On `go`, go on with the request.
- **Direction open, or the human asks to explore:** brainstorm with them until they settle the direction: options, tradeoffs, what you'd pick and why. Write nothing and create no issues before then. Then triage what they settled. If it ends in "not now", write nothing.
- **New work:** triage and plan it, below.
- **Planned work** (an issue, a feature doc, "build feature 3, two at a time"): hand it to `drive-feature` for a feature, `build-slice` for a slice or ticket, with the human's words.
- **Drifted docs:** hand them to `living-docs`.

If the config block lacks a key, ask for it in the request's batch of questions, with the default `living-docs` gives, and write the answer into the block.

## Triage first

A request may hold several unrelated changes; triage and plan each on its own. Choose the lightest route that's safe, and say which you chose:
- **direct:** small, low-risk and obvious. Do it here and now: no issue, no acceptance tests, no independent review. It still goes on its own PR into `main`, which waits for `go` if `approve` includes `merge`; then check CI is green and merge it yourself.
- **ticket:** one slice. A single issue with acceptance criteria.
- **feature:** anything bigger, or touching money, auth, deleting data, the data model, or a new external service.

A request about a feature that has shipped is new work: triage it on its own, and don't reopen the feature or its issues.

Then ask: **does this change the architecture?** A new component, storage, service or deployment shape, or a change across the codebase (auth, background jobs). If so, decide it before slicing: ADRs (use the `adr` skill) and `docs/ARCHITECTURE.md`, then make the first slice a thin path through the new structure, end to end. Usually the answer is no, in one line.

## The feature doc

`docs/features/NNNN-short-slug.md`. Cover:
- **why:** the problem and who has it
- **deliverables:** what will exist when it's done
- **acceptance criteria:** observable and testable
- **core flows:** which flows in `docs/PRODUCT.md` the feature adds or changes, or none
- **out of scope**
- **risk:** inherits the project default; any override needs a reason
- **slices** (below), with links to the issues
- **assumptions and decisions** made along the way. Owner decisions go in PRODUCT.md or an ADR; link to them rather than repeating them.
- **status:** `planned` or `shipped`

Describe what and why; leave the how to the builder.

## Slices

- Each slice is vertical: it works end to end and delivers something observable.
- Size slices by what they deliver, not by count. One slice is fine; so is ten.
- Before slicing, settle in the feature doc any rule that several slices share (how a value is parsed, shown or checked), asking as below, so slices built in parallel don't each invent their own.
- Where two cuts are equally vertical, choose the one that lets more slices be built together. Never give up verticality for it: no slice becomes a layer (only the schema, only the API) to free up another.
- Each slice has its own acceptance criteria and notes any known risks, its dependencies, and what it touches: screens, tables, modules, rules.
- In the feature doc, show which slices can be built together: those with no dependency between them and no screen, table or module in common. A shared rule is fine once the feature doc settles it. For example: "1 first; then 2, 3 and 4 together; 5 after 3."

## Issues

Create issues with `gh` (omp's `github` tool can't create them; in Claude Code, `gh` is the GitHub tool). Each body starts with the command that builds it (edit the URL in once the issue exists), so a session started from the issue runs the right skill.
- **Feature:** one issue labelled `feature`, `feature:NNNN` and `ready-for-agent`, starting `/drive-feature <its URL>`, then a short summary and a link to the feature doc. Don't copy the deliverables or acceptance criteria; the doc stays the truth.
- **Slices:** each a sub-issue of it (`gh issue create --parent`), labelled `slice`, `feature:NNNN` and `ready-for-agent`, starting `/build-slice <its URL>`, then a link to the feature issue, its acceptance criteria, dependencies, the slices it can be built with, and what it touches.
- **Ticket:** one issue labelled only `ready-for-agent`, starting `/build-slice <its URL>`, then its acceptance criteria.

With no issue tracker, list the slices in the feature doc.

Once the plan is final (after `go` at any `spec` or `plan` stop), create the issues. For a feature, link them in the doc, then put the doc and any ADRs on `main` through a PR of their own, and merge it once checks pass, so a session started from an issue finds them. It's a plan, not code: `merge` in `approve` doesn't hold it. With no GitHub remote, commit them to `main`.

## Working with the human

- Start from whatever exists, from a one-line idea to a full spec. What they wrote is binding; fill only the gaps.
- Ask about business rules the docs don't settle (what gets recorded, money, who can do what, what users see) only as far as `risk` says: `low`, when you can't go on without an answer; `medium`, where a wrong guess records wrong data or is hard to undo; `high`, any rule that isn't written down. Never guess silently: whatever you don't ask goes in the doc as an assumption.
- Decide technical questions yourself and record them in the doc. Architecture, and anything that commits outside the codebase (a new service, cost, data leaving the system), goes in a `proposed` ADR, and accepting it is one of the questions in the batch: on `go`, set it `accepted`.
- Ask in one batch, each question with your default; replying `go` accepts them all. A brainstorm is the exception: a conversation, until the human settles the direction. If `approve` includes `spec` or `plan`, put the questions in that stop instead of stopping twice.
- Read `approve` from `AGENTS.md` when you get there; the human may have changed it. If it includes `spec`, stop once the deliverables and acceptance criteria are written, before slicing. If it includes `plan`, stop once the slices are planned, before building. At a stop, say exactly what to do, what to check, and what happens after ("reply `go` and I'll …").
- Then a ticket is built by `build-slice` in a fresh subagent, not a fork (in omp a `task` subagent; in Claude Code, model `sonnet`), given the issue URL. Pass its questions and `proposed` ADRs to the human, and the answers back; relay its stop (the PR, what to check, `go`), and on `go` merge per `build-slice`'s rule. A feature doesn't: once its doc is on `main`, end the session with a stop that gives each feature issue and says to start it from Orca (or run `/drive-feature <its URL>`; with no tracker, `/drive-feature <its feature doc>`) in a fresh session.
