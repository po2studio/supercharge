# 0003: Claude Code: install the skills and they work

Feature issue: [#31](https://github.com/po2studio/supercharge/issues/31). Status: **planned**.

## Why
The bundle runs only in omp. Owners who work in Claude Code (CC) can't use it, or can use it only with guarantees quietly lost: the skills name omp's tools, and where one is missing they fall back to "an equivalent". In CC the obvious equivalents break the bundle's rules:
- a reviewer started as a fork inherits the builder's model and conversation, so the builder reviews itself;
- a subagent's worktree starts from `main`, not the feature branch;
- the omp model roles (`task`, `slow`) have no CC counterpart to configure.

The owner wants CC to work like omp: install the skills, nothing else needed.

## Deliverables
- The five skills work in both omp and CC. They name the capability ("a fresh subagent", "the reviewer") and say what each harness uses only where the two differ.
- README: install for CC, how the jobs map to models in CC, the minimum CC version, and CC marked untested until a real run is written up in `runs/`.
- `docs/ARCHITECTURE.md`, `docs/PRODUCT.md` and `AGENTS.md` describe a bundle for omp and CC.

## Acceptance criteria
1. In CC, `npx skills add po2studio/supercharge -g -a claude-code` is the whole install: no plugin, agent files, settings or hooks, and no tools beyond those omp needs (`git`, `gh`). `/living-docs`, `/supercharge`, `/build-slice <URL>` and `/drive-feature <URL>` run as typed, including as the first line of an issue an ADE starts.
2. In a project that has a `CLAUDE.md`, `living-docs` makes sure CC loads `AGENTS.md`: the `CLAUDE.md` imports it with `@AGENTS.md`.
3. In CC, every slice is reviewed by a fresh subagent (not a fork) on a different model from the builder (`opus` for a `sonnet` builder, otherwise `sonnet`), given only the acceptance criteria and the diff. If no such reviewer can be started, `build-slice` stops and says so; it never falls back to the builder's model.
4. In CC, `drive-feature` builds each slice in a fresh subagent (not a fork) on `sonnet`, and slices built in parallel each in their own worktree. Every slice's branch starts from the PR's target branch (the feature branch, if there is one), wherever its worktree started.
5. In CC, builders run a UI only in a headless browser, through the project's browser test tooling. They never use the human's own browser.
6. In omp, nothing changes: the same roles, agents, tools and stops.
7. README:
   - Install shows both commands, and how to check each.
   - Models says what runs where in CC: the session on the model the human picks (`/model`), builders on `sonnet`, the reviewer on a different Claude model. The cross-company reviewer is omp-only.
   - It names the minimum CC version (v2.1.283, where auto mode is the starting permission mode) and says to start with `--permission-mode auto` where it isn't.
   - It marks CC as supported but untested until a run is written up in `runs/`.
8. `docs/ARCHITECTURE.md` (Runtime, Distribution), `docs/PRODUCT.md` (intro, Users, flow 1) and the first line of `AGENTS.md` name both harnesses.
9. `python3 scripts/check.py` passes.

## Core flows
No new flow. **Start a request**, **Build a feature** and **Build a slice or ticket** in [PRODUCT.md](../PRODUCT.md) now also run in CC.

## Out of scope
- A CC plugin, `.claude/agents/` files, CC settings or hooks shipped by the bundle.
- A reviewer from another company in CC (Codex CLI or a gateway).
- Driving the human's own browser (Claude in Chrome).
- Harnesses other than omp and CC.
- A smoke run or an end-to-end test (AGENTS.md). The first real CC run is written up in `runs/` when it happens.

## Risk
Medium (default). A wrong word in a skill loses a guarantee silently: a same-model or forked reviewer still "passes". Review each slice against AC 3–6 word by word.

## Settled rules (shared by every slice)
**What each harness uses.** Skills use this mapping and no other names:

| Capability | omp | Claude Code |
|---|---|---|
| Builder | `task` subagent, on the `task` role | Fresh subagent (not a fork), model `sonnet` |
| Reviewer | `reviewer` agent, on the `slow` role | Fresh subagent (not a fork): `opus` for a `sonnet` builder, otherwise `sonnet`. Given only the acceptance criteria and the diff |
| Isolated worktree | `task` isolation | `isolation: "worktree"` on the Agent call |
| GitHub | `github` tool; `gh` for issues | `gh` |
| Browser | `browser` tool | Headless, through the project's browser test tooling |
| Project instructions | `AGENTS.md` | `AGENTS.md`; a `CLAUDE.md` must import it |

**Wording.** Name the capability. Name a harness only in a short clause where the two differ, in the skill that needs it. No harness section, no table in the skills. Keep the escape line ("if a feature named here isn't available, use an equivalent, or stop and say so"), but make it harness-neutral, and keep "not a fork" and "a different model" as hard rules: no equivalent replaces them.

**Branch base.** A builder starts its branch from the PR's target branch, whatever its worktree started from. Worktrees share local refs, so the feature branch needn't be pushed first.

## Assumptions and decisions
- **Owner, 2026-11-09:**
  - "install the skills, nothing else needed" in CC, as in omp;
  - the CC reviewer is a different Claude model, not another company's;
  - CC builders run on `sonnet`;
  - builders use a headless browser only;
  - ship marked untested; no real-run slice.
- **The skills CLI, not a CC plugin.** Plugin skills are namespaced (`/supercharge:build-slice`), which breaks the command each issue starts with. Plugin settings can't grant permissions either: only `agent` and `subagentStatusLine` take effect.
- **No agent files.** CC's Agent tool takes a model alias (`sonnet`, `opus`, `haiku`, `fable`) and `isolation: "worktree"` on each call, so the skills can say what to pass.
- **Fresh subagents, never forks, for building and reviewing.** CC's fork mode is on by default (v2.1.302), and Claude may start a fork on its own. A fork inherits the parent's model and whole conversation.
- **An alias in the session's family runs on the session's exact model.** A `sonnet` builder in an Opus session runs on Sonnet; an `opus` reviewer runs on the session's Opus. Either way it differs from the builder.
- **Branch base in the skill, not in settings.** CC subagent worktrees branch from the remote default branch unless the user sets `worktree.baseRef: "head"`; the bundle ships no settings.
- **No change needed:**
  - questions from builders (CC subagents can't ask the human; `drive-feature` already passes questions on and the answers back);
  - nested subagents (CC allows 3 levels by default; drive → build → review needs 2);
  - `scout`, which no skill names.
- **Minimum CC v2.1.283:** auto mode is the starting permission mode from then on, and CC reads `AGENTS.md` itself from v2.1.277.
- *Assumptions, not asked:*
  - Orca starts CC sessions from an issue the same way it starts omp sessions;
  - the README model table stays as it is: the models are the same, only the harness differs.
- **Conflict to settle:** [0002](0002-second-brain.md) puts "agents other than omp" out of scope "as today". Once this ships, that reason no longer holds. Not changed here; the owner decides when 0002 is built.
- **Overlap:** open ticket [#30](https://github.com/po2studio/supercharge/issues/30) (rename `plan-feature` to `supercharge`) edits the same README sections and docs. Whichever merges second resolves the conflicts. If #30 lands first, slice 1 edits `skills/supercharge/` in place of `skills/plan-feature/`.

## Slices
1 ([#32](https://github.com/po2studio/supercharge/issues/32)) and 2 ([#33](https://github.com/po2studio/supercharge/issues/33)) together: they share only the README, in different sections. 3 ([#34](https://github.com/po2studio/supercharge/issues/34)) after 2.
- [x] [#32](https://github.com/po2studio/supercharge/issues/32): Install and plan in Claude Code
- [x] [#33](https://github.com/po2studio/supercharge/issues/33): Build a ticket in Claude Code
- [ ] [#34](https://github.com/po2studio/supercharge/issues/34): Drive a feature in Claude Code

## Notes for the builder
- AGENTS.md: `risk: medium`, `approve: [plan, merge]`. The PR into `main` waits for the owner's `go`. CI runs `python3 scripts/check.py`.
- No smoke run (AGENTS.md): verification is the independent review of the diff against these criteria.
- Every line in a skill should change behaviour. Keep additions short and in the existing voice; prefer rewording an omp-only line over adding one.
- CC facts are from code.claude.com/docs (sub-agents, skills, memory, worktrees, permission-modes, plugins/components), read 2026-11-09. Check them there again, not from memory.
