# Architecture

## Components

- **Skills:** `skills/<name>/SKILL.md`, Markdown with `name` (matching the folder) and `description` frontmatter. No code: each skill guides an omp or Claude Code session or subagent.
  - `living-docs` writes `AGENTS.md` and `docs/`; `adr` writes `docs/adr/`; `supercharge` is the entry point: it runs `living-docs` to set a project up, writes `docs/features/` and GitHub issues, and hands planned work to `drive-feature` or `build-slice`.
  - `drive-feature` runs `build-slice` per slice in `task` subagents, then `living-docs` at the finish. `build-slice` writes the code, tests and docs, and opens the PR.
  - Skills share no code. Apart from one skill starting another, they coordinate only through those files, the config block in `AGENTS.md`, and GitHub issues and PRs.
- **`runs/`:** write-ups of real runs, the evidence behind the README's model table.
- **`scripts/check.py`:** checks each skill's frontmatter and every relative link in the repo's Markdown. CI (`.github/workflows/check.yml`) runs it on PRs and on pushes to `main`.

## Runtime

The skills run in omp and in Claude Code. In omp they use `task` subagents (builders, on the `task` model role), the `reviewer` agent (on `slow`), `scout` (on `smol`), `browser`, the `github` tool, isolated worktrees for parallel slices, and `gh` to create issues and sub-issues, which the `github` tool can't do. In Claude Code they use `gh` for GitHub and `AGENTS.md` through a `CLAUDE.md` that imports it. If a feature a skill names isn't available, it uses an equivalent or stops and says so.

## Distribution

The skills CLI installs from this GitHub repo: `npx skills add po2studio/supercharge -g -a pi` (omp) or `-a claude-code` (Claude Code) copies each `skills/<name>` folder into `~/.agents/skills` or `~/.claude/skills`, and `npx skills update -g` refreshes them. There is no build or release step.

## External services

- **GitHub:** issues, sub-issues, labels, PRs, Actions. No fake: without a GitHub remote, the skills use local branches and the checks in `AGENTS.md` instead.
- **Model providers,** through omp.

There is no code to run, so nothing runs offline on fakes.
