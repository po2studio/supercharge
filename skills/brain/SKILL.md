---
name: brain
description: Search your documents (177 MB download on first use). Answers from a project's docs and the folders it lists, citing paths and dates, and keeps them true. Use to find why something is the way it is before asking the human or guessing. No API key, nothing else to install.
---

# brain

Search before you ask the human or guess; keep the documents true. `/brain <question>` answers one.

Sources: the project (its docs folder, `README.md`, `AGENTS.md`) and the `brain:` list in the `## Supercharge` block of `AGENTS.md`: `path:` entries (absolute, `~/…` or repo-relative), each with an optional `update: true`. Never write that key; say the lines to add.

## First run

No binary at `~/.cache/supercharge/brain/<version>/brain`? Say its size (your platform's `bytes` in `brain.json` beside this file; `uname -sm`), that no key is needed and nothing else installs, and that a big folder's first index is slow. Wait for `go`. Then `gh release download <tag> --repo <repo> --pattern <asset> --dir <temp dir>`; check the sha256 against `brain.json` (mismatch: delete, say so, stop); make it executable; move it to the cache path.

## Answer

Search in a small subagent (omp's `sonic` on `smol`; in Claude Code a fresh one on `haiku`), from the project's root: `brain search "<question>"` (`--json` gives each hit's absolute `file`). Nothing answers? Rephrase, up to three times, in other words or another language. Answer in at most 10 lines, each claim with its path and date. The newer of two disagreeing files wins; say so. A superseded ADR is history; an amended one comes with its amendments. If nothing answers, say so; don't guess.

## Before asking

Search each business question first. A document that settles it: cite it, don't ask. One that only informs sets your default.

## Update step

When a source sets `update: true` and the binary is cached (never download here): after `go`, and right after a merge into `main` by any route, a builder subagent (omp's `task`; in Claude Code a fresh one on `sonnet`):

1. Searches those sources for what the answers or the shipped work make untrue.
2. Edits those Markdown and text documents in place, minimally, and appends `- YYYY-MM-DD: <what changed> (<where it was decided>)` under a `## History` heading at the end. Never the project's own files (PRs change them). A Word file, PDF or spreadsheet is reported, not edited.
3. Writes a new document, in the first `update: true` source, only for a decision nothing covers that a future agent would undo or ask about. Technical ones are ADRs.
4. In a git repo: `git pull --rebase`, commit, push; stop on a conflict.
5. Reports the files edited and those it could not.

## Brain repo CI

An `update: true` source that is a GitHub repo gets `.github/workflows/brain.yml`: `brain-ci.yml` beside this file, `@TAG@`, `@REPO@`, `@ASSET@`, `@SHA256@` filled from `brain.json`. It publishes the index for other machines. Write it when missing or naming another tag; commit and push.
