---
name: supercharge
description: The entry point for any request in a project. An idea, a bug, a change or a feature. Sets the project up on first use, brainstorms when the direction is open, triages and plans new work, and hands planned work to drive-feature or build-slice.
---

# supercharge

Take any request to its right next step, and turn new work into something an agent can build and a human can check.

Only route and plan: don't write code.

`docs/` here means the folder set by `docs` in `AGENTS.md` (default `docs`).

## Route the request

Read `AGENTS.md`, then say which route you chose:
- **Not set up** (no `## Supercharge` config block in `AGENTS.md`): run `living-docs` yourself, drafting on its defaults so its questions and the docs it wrote go to the human in one stop. On `go`, go on with the request.
- **Direction open, or the human asks to explore:** brainstorm with them until they settle the direction: options, tradeoffs, what you'd pick and why. Write nothing and create no issues before then. Then triage what they settled; "not now" writes nothing.
- **New work:** triage and plan it, below.
- **Planned work** (an issue, a feature doc, "build feature 3, two at a time"): hand it to `drive-feature` for a feature or a slice without an issue, `build-slice` for a ticket or a slice issue, with the human's words.
- **Drifted docs:** run `living-docs` yourself, not in a subagent.

If the config block lacks a key, ask for it in the request's batch of questions, with the default `living-docs` gives, and write the answer into the block.

## Triage first

A request may hold several unrelated changes; triage and plan each on its own. Choose the lightest route that's safe, and say which you chose:
- **direct:** small, low-risk and obvious. No issue and no planning stop, nothing else skipped: `build-slice` builds it in a fresh subagent, not a fork (in omp a `task` subagent; in Claude Code, model `sonnet`; in Orca, in the worktree `build-slice` has you create first), given the request and one to three acceptance criteria you write, with the same independent review and docs rule. Its PR into `main` waits for `go` if `approve` includes `merge`; on `go` merge per `build-slice`'s rule.
- **ticket:** one slice. A single issue with acceptance criteria.
- **feature:** anything bigger, or touching money, auth, deleting data, the data model, or a new external service.

A request about a shipped feature is new work: don't reopen the feature or its issues.

Then ask: **does this change the architecture?** A new component, storage, service or deployment shape, or a change across the codebase (auth, background jobs). If so, decide it before slicing: ADRs (use the `adr` skill) and `docs/ARCHITECTURE.md`, then make the first slice a thin path through the new structure, end to end. Usually the answer is no, in one line.

## The feature doc

`docs/features/NNNN-short-slug.md`. Cover:
- **why:** the problem and who has it
- **deliverables:** what will exist when it's done; each appears in some slice's "touches"
- **acceptance criteria:** what someone can observe; files and modules only under a slice's touches
- **core flows:** which flows in `docs/PRODUCT.md` the feature adds or changes, or none
- **out of scope**
- **risk:** inherits the project default; any override needs a reason
- **slices** (below), ticked off only with no issue tracker
- **assumptions and decisions.** Owner decisions go in PRODUCT.md or an ADR; link to them rather than repeating them.
- **status:** `planned` or `shipped`

Describe what and why; leave the how to the builder.

## Slices

- Each slice is vertical: it works end to end and delivers something observable.
- Size slices by what they deliver, not by count.
- Before slicing, settle in the feature doc any rule that several slices share (how a value is parsed, shown or checked), asking as below, so slices built in parallel don't each invent their own.
- Where two cuts are equally vertical, choose the one that lets more slices be built together. Never give up verticality for it: no slice becomes a layer (only the schema, only the API) to free up another.
- If the feature adds or changes a core flow and the project has no end-to-end harness (as `build-slice` defines it), the first slice that touches a core flow sets it up and records how to run it in `AGENTS.md`: put that in its acceptance criteria and size it for that.
- In the feature doc, each slice has its own acceptance criteria and notes any known risks, its dependencies, and what it touches: screens, tables, modules, rules, whatever the tracker.
- Show which slices can be built together: those with no dependency between them and no screen, table or module in common. A shared rule is fine once the feature doc settles it. For example: "1 first; then 2, 3 and 4 together; 5 after 3."

## Issues

Create issues with `gh` (omp's `github` tool can't create them; in Claude Code, `gh` is the GitHub tool). Write a feature issue and a ticket from `docs/issue-template.md` if the project has one, else `issue-template.md` beside this skill. Whatever it says, the body starts with the command that builds it (edit the URL in once the issue exists), a ticket has acceptance criteria someone can observe, and a feature issue has the slice checklist and a link to its feature doc and copies no deliverables or criteria. Titles state the outcome, with no number or "Feature"/"slice" prefix.
- **Feature:** one issue labelled `feature` and `ready-for-agent`, starting `/drive-feature <its URL>`, its slices a checklist of their outcome titles in build order. No sub-issues, no `slice` or `feature:NNNN` label.
- **Ticket:** one issue labelled only `ready-for-agent`, starting `/build-slice <its URL>`.

With no issue tracker, list the slices as a checklist in the feature doc.

Once the plan is final (after `go` at any `spec` or `plan` stop), create the issues. For a feature, link the issue in the doc, then put the doc and any ADRs on `main` through a PR of their own, and merge it once checks pass, so a session started from an issue finds them. It's a plan, not code: `merge` in `approve` doesn't hold it. With no GitHub remote, commit them to `main`.

## Working with the human

- Start from whatever exists. What they wrote is binding; fill only the gaps.
- Ask about business rules the docs don't settle (what gets recorded, money, who can do what, what users see) only as far as `risk` says: `low`, when you can't go on without an answer; `medium`, where a wrong guess records wrong data or is hard to undo; `high`, any rule that isn't written down. Never guess silently: whatever you don't ask goes in the doc as an assumption.
- With the `brain` skill installed and its binary already cached (never start its download), search before you ask each business question and cite what's settled instead of asking; run its update step after `go` and after a direct change or ticket merges into `main`.
- Decide technical questions yourself and record them in the doc. Architecture, and anything that commits outside the codebase (a new service, cost, data leaving the system), goes in a `proposed` ADR, and accepting it is one of the questions in the batch: on `go`, set it `accepted`.
- Ask in one batch, each question with your default; replying `go` accepts them all. A brainstorm is the exception: a conversation, until the human settles the direction. If `approve` includes `spec` or `plan`, put the questions in that stop instead of stopping twice.
- Read `approve` from `AGENTS.md` when you get there; the human may have changed it. If it includes `spec`, stop once the deliverables and acceptance criteria are written, before slicing. If it includes `plan`, stop once the slices are planned, before building. At a stop, say exactly what to do, what to check, and what happens after ("reply `go` and I'll …").
- A ticket is built by `build-slice` in a subagent as for the direct route, given the issue URL. Pass its questions and `proposed` ADRs to the human, and the answers back; relay its stop, and on `go` merge per `build-slice`'s rule. A feature doesn't: once its doc is on `main`, end the session with a stop that gives each feature issue and says to start it from Orca (or run `/drive-feature <its URL>`; with no tracker, `/drive-feature <its feature doc>`) in a fresh session.
