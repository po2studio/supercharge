# 0004: Issues a human can read: one per feature, written from a template

Feature issue: [#58](https://github.com/po2studio/supercharge/issues/58). Status: **planned**.

## Why
The owner plans in the session but reads and starts work from the issue list (GitHub, Orca). The issues the bundle writes don't serve that:
- **They're written for the builder.** A slice or ticket body is the build command, acceptance criteria and a file list. Nothing says who has the problem, what changes for them or how to check it, and the criteria drift into file edits ("`build-slice`: the review is by…", [#33](https://github.com/po2studio/supercharge/issues/33)).
- **A feature is one issue per slice, plus one.** Feature 0002 is eight issues. The list needs a filter (`-label:slice`) to be usable, each slice is described in two places, and they drift: [#21](https://github.com/po2studio/supercharge/issues/21) and [#22](https://github.com/po2studio/supercharge/issues/22) are both "0002 slice 6".
- **Progress is kept in two places.** `build-slice` reads whether a slice's issue is open; `drive-feature` reads ticks in the feature doc. Each builder ticks the doc in its own branch, so slices built in parallel each have their own version, and nobody on `main` sees any of them.
- **The format is fixed.** Other teams want other issues.

## Deliverables
- A default issue template shipped with `supercharge`, human first: why, what changes, how to check, then the builder's detail.
- A project can replace it with its own `docs/issue-template.md`.
- A feature is one issue: a short summary, the slices as a checklist, a link to the feature doc. No sub-issues.
- The checklist on the feature issue is the progress record, ticked by `drive-feature`.
- Features already planned with sub-issues still build.
- README, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md` and `AGENTS.md` describe all of this.

## Acceptance criteria
1. A ticket `supercharge` creates reads top down: the build command, why (the problem and who has it), what changes for them, acceptance criteria, how a human checks it, out of scope, touches. Its title states the outcome.
2. A feature issue `supercharge` creates has the build command, why, what changes, the slices as a checklist of outcome titles, and a link to the feature doc. It copies no acceptance criteria. From the issue alone a human can tell what is being built and how far it is.
3. Acceptance criteria, in issues and in feature docs, say what someone can observe. Files and modules appear only under touches.
4. In a project with `docs/issue-template.md`, issues follow that file; in one without, the default. Either way the body starts with the build command, a ticket has acceptance criteria, and a feature issue has the slice checklist and the link to its doc.
5. Planning a feature creates exactly one issue, labelled `feature` and `ready-for-agent`: no sub-issues, no `slice` or `feature:NNNN` label. Each slice's acceptance criteria, risks, dependencies, what it can be built with and what it touches are in the feature doc.
6. While a feature is built, its issue shows each slice ticked once that slice is merged into the feature branch. `drive-feature` ticks it; a builder never edits the issue, and no slice branch edits the feature doc to record progress. With no tracker, the ticks stay in the feature doc as today.
7. `drive-feature` started again in a fresh session skips the ticked slices, and a slice whose dependency isn't ticked isn't started.
8. One slice of such a feature is built with `/drive-feature <feature issue URL> build only slice N`. Its PR says `Part of #N`; the feature issue stays open until the final PR closes it.
9. A feature planned with sub-issues builds as today, from its feature issue or from a slice issue: the slice's issue is closed when it merges, the doc is ticked, and the last slice's stop says to start the feature issue. Nothing converts it.
10. README, `docs/PRODUCT.md` (flows 1 to 3, the Issues rule), `docs/ARCHITECTURE.md` and `AGENTS.md` say what an issue contains, how to change the format, that a feature is one issue with its progress on it, and that older features with sub-issues still build.
11. `skills/supercharge/SKILL.md`, `skills/build-slice/SKILL.md` and `skills/drive-feature/SKILL.md` together are no longer than today: 3,416 words.
12. `python3 scripts/check.py` passes.

## Core flows
No new flow. All three in [PRODUCT.md](../PRODUCT.md) change:
- **Start a request:** a feature becomes a feature doc and one issue.
- **Build a feature:** `drive-feature` ticks the slices on the feature issue.
- **Build a slice or ticket:** a slice has an issue of its own only in a feature planned with sub-issues.

No end-to-end harness is set up: skill text has no runtime ([AGENTS.md](../../AGENTS.md)).

## Out of scope
- Converting open sub-issues ([#16](https://github.com/po2studio/supercharge/issues/16) to [#23](https://github.com/po2studio/supercharge/issues/23)) or the features they belong to.
- Removing the build path for features planned with sub-issues. It stays until the owner says otherwise.
- GitHub's own issue templates (`.github/ISSUE_TEMPLATE/`), and a config key for the format.
- A template for the feature doc.
- Project-management practice beyond what the template asks for: estimates, priorities, milestones, boards, a required "As a user I want…".
- Trackers other than GitHub.
- A smoke run or an end-to-end test (AGENTS.md).

## Risk
Medium (default). A wrong word in a skill loses something silently: an older feature that no longer builds, or a slice that merges and is never ticked, so the next session builds it again. Review slice 2 against AC 6, 7 and 9 word by word.

## Settled rules (shared by both slices)
**The template.** The default is `skills/supercharge/issue-template.md`, beside the skill, with one body for a ticket and one for a feature issue. A project's `docs/issue-template.md` (in the folder `docs` names) replaces it whole. Three things hold whatever the template says: the body starts with the build command; a ticket has acceptance criteria; a feature issue has the slice checklist and the link to its doc, and copies no criteria.

**Human first.** An issue opens with the problem and who has it, then what changes for them, then how to check it. What it touches comes last. Short: enough for a human to understand what is being done, no more.

**Titles** state the outcome, with no number and no "Feature" or "slice" in front.

**Labels.** A feature issue: `feature` and `ready-for-agent`. A ticket: `ready-for-agent`.

**Done.** One definition, in `build-slice`, that every other line refers to. A slice is done when:
- it's ticked on the feature issue;
- or, in a feature planned with sub-issues, its issue is closed;
- or, with no tracker, it's ticked in the feature doc.

**Who ticks.** `drive-feature`, once the slice is merged into the feature branch, reading the issue body again just before it edits it. Builders never edit the feature issue. A slice of a one-issue feature is therefore built only through `drive-feature`; `/build-slice <URL>` stays for tickets and for the slice issues of older features.

**Older features.** Whether a feature was planned with sub-issues is read from the feature issue: it has sub-issues. For those, every line that works today keeps working.

## Assumptions and decisions
- **Owner, 2026-11-18:**
  - vertical slices stay;
  - issues are written for humans first, short, with the builder's detail after;
  - one issue per feature and one per ticket; no new sub-issues;
  - the feature issue carries a summary and the slice checklist, and is the progress record, so it's outside the codebase and the same for every agent;
  - the orchestrator does the project management, not the builders;
  - the default template ships in the skill; `docs/issue-template.md` overrides it;
  - features already planned with sub-issues keep building; new planning uses only the new format;
  - one GitHub issue for this feature;
  - one slice of a one-issue feature is built only through `drive-feature`;
  - the three skills together don't grow;
  - titles state the outcome, with no number in front.
- **A file in `docs/`, not `.github/ISSUE_TEMPLATE/`.** GitHub's templates are intake forms for people reporting something, often several, some as YAML forms. An agent-ready issue is a spec; one file with one purpose needs no choosing or mapping.
- **A template, not rules.** The agent already knows the practices; a form it fills in changes what it writes more than prose about them, and moves the format out of `SKILL.md`.
- **One writer for the ticks.** Ticking rewrites the whole issue body, so two writers finishing together can undo each other's tick.
- **No ADR.** No new component, service or storage; where progress is recorded is settled above.
- *Assumptions, not asked:*
  - the skills CLI copies a second file in a skill folder (ARCHITECTURE.md: it "copies each `skills/<name>` folder"); no skill ships one yet. If it doesn't arrive, the default goes inline in `SKILL.md`;
  - the README keeps one line on the `slice` and `feature:NNNN` labels while older features are open;
  - a feature issue needs no "how to check": the final PR lists the checks only a human can do.

## Slices
1 first; 2 after 1. Both rewrite the Issues section of `supercharge` and the README's Usage, so they aren't built together.
- [ ] 1: Issues a human can read, from a template a project can edit
- [ ] 2: A feature is one issue, with its progress on it

### Slice 1: Issues a human can read, from a template a project can edit
**Acceptance criteria** (feature AC 1, 3, 4, and 2 without the checklist)
1. The default template exists beside the `supercharge` skill with two bodies. Ticket: the build command, why, what changes, acceptance criteria, how to check, out of scope, touches. Feature issue: the build command, why, what changes, a link to the feature doc. Each section says in a few words what goes in it.
2. `supercharge` writes a ticket and a feature issue from the project's `docs/issue-template.md` if there is one, otherwise from the default. Either way the body starts with the build command, a ticket has acceptance criteria, and a feature issue links to its doc and copies no criteria.
3. `supercharge` asks for titles that state the outcome, and for acceptance criteria, in issues and feature docs, that say what someone can observe, with files and modules only under touches.
4. Slices are still created as sub-issues, exactly as today.
5. The README says what an issue contains and how a project changes the format. The Issues rule in `docs/PRODUCT.md`, and `docs/ARCHITECTURE.md` and `AGENTS.md` where they say what a skill folder holds, match.
6. `skills/supercharge/SKILL.md` is shorter than today's 1,416 words.
7. `python3 scripts/check.py` passes.

**Dependencies:** none. **Build with:** nothing.
**Touches:** `skills/supercharge/SKILL.md` (Issues; the criteria wording in the feature doc and Slices sections), new `skills/supercharge/issue-template.md`, README (Usage, Skills), `docs/PRODUCT.md` (Issues rule), `docs/ARCHITECTURE.md` (Components), `AGENTS.md` (first paragraph).
**Risks:** `scripts/check.py` reads every Markdown file, so a placeholder link in the template counts as a broken link.

### Slice 2: A feature is one issue, with its progress on it
**Acceptance criteria** (feature AC 2, 5 to 11)
1. Planning a feature creates one issue labelled `feature` and `ready-for-agent`, with no sub-issues and no `slice` or `feature:NNNN` label. The template's feature issue gains the slices as a checklist of outcome titles, in build order.
2. The feature doc holds each slice's acceptance criteria, risks, dependencies, what it can be built with and what it touches, with or without a tracker. It links the feature issue, and carries ticks only when there is no tracker.
3. `build-slice` defines done once, as the settled rule has it. The dependency check, the slices `drive-feature` skips and the last-slice stop all use it.
4. `drive-feature` ticks a slice on the feature issue once it's merged into the feature branch, reading the body again just before. A builder edits neither the feature issue nor, to record progress, the feature doc.
5. One slice of a one-issue feature is built with `/drive-feature <feature issue URL> build only slice N`. Its PR says `Part of #N`, and only the final PR closes the feature issue.
6. A feature planned with sub-issues builds as today from its feature issue or a slice issue: the slice's issue closed on merge, the doc ticked, the last-slice stop. `supercharge` creates nothing in that format and converts nothing.
7. The README (Usage: the list filters, building a plan, one slice; the Skills table), `docs/PRODUCT.md` (flows 1 to 3, the Issues rule) and `docs/ARCHITECTURE.md` (Runtime, External services) match.
8. The three skills together are no longer than 3,416 words.
9. `python3 scripts/check.py` passes.

**Dependencies:** after slice 1 (it extends the template and the same Issues section).
**Touches:** `skills/supercharge/SKILL.md` (The feature doc, Slices, Issues, the handoff), `skills/supercharge/issue-template.md`, `skills/build-slice/SKILL.md` (the PR line, the docs rule's tick, the dependency check, the last-slice stop), `skills/drive-feature/SKILL.md` (skipping done slices, ticking, one slice), README, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`.
**Risks:** the older path breaks without anyone noticing; nothing runs it until a project with open sub-issues builds one.

## Notes for the builder
- AGENTS.md: `risk: medium`, `approve: [plan, merge]`. The PR into `main` waits for the owner's `go`. CI runs `python3 scripts/check.py`.
- No smoke run (AGENTS.md): verification is the independent review of the diff against these criteria.
- Every line in a skill should change behaviour. Keep additions short and in the existing voice; prefer rewording a line over adding one. Count with `wc -w`.
- This feature is built by the skills as installed, before its own change. It has no slice issues, so the ticks in **Slices** above are its progress record, ticked by `build-slice` as today. `drive-feature`: when a slice merges, also tick it on [#58](https://github.com/po2studio/supercharge/issues/58).
