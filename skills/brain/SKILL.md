---
name: brain
description: Search your documents (176 MB download on first use). Answers from a project's docs and the folders it lists, citing paths and dates; /brain refresh keeps its notes true. Use to find why something is the way it is, or what the company already knows, before asking the human or guessing. No API key, nothing else to install.
---

# brain

Answer a question from the project's own documents and the outside folders it lists, and say which files and dates it came from. Search before asking the human or guessing.

`/brain <question>` or `/brain refresh`. The `brain` binary indexes and searches on this machine, offline. Installing this skill turns the brain on for the project you're in; there is nothing to configure and no location to ask for.

## Sources

Run every `brain` command from the project's root; the binary works out the sources itself:

- **The project:** its docs folder (the `docs` key in the `## Supercharge` block of `AGENTS.md`, default `docs`), `README.md` and `AGENTS.md`, read in place. The brain never writes to them.
- **Outside sources:** the `brain:` list in that block, none by default: a list of `path:` (absolute, `~/…` or relative to the repo) with an optional `update: true`. A source is read-only unless it sets `update: true`; at most one does, and two stop every command with a message naming both: relay it and stop. Only in that one do agents write, and only their own files: `BRAIN.md`, `decisions/`, `notes/`, `projects/` and, when it is a GitHub repo, its CI workflow `.github/workflows/brain.yml`. Raw files are never edited, in any source.

Never write the `brain:` key, and never ask for it as a missing key. To add a source, say the lines to add to the block (`brain:`, then `  - path: ~/Dropbox/ClientA/Policies`, with `    update: true` if agents may write there) and leave the edit to the human. A search covers only this project and the sources it lists.

## First-run setup

The pinned binary isn't cached. Don't search, and write nothing, before the human's `go`.

1. Show what will be downloaded, in one message. Take the platform from `uname -sm` (`linux-x64`, `linux-arm64`, `macos-arm64`, `macos-x64`) and read its entry in `brain.json` next to this file: the size in MB (`bytes`), the release it comes from (`repo` and `tag`, at the top of the file), and where it is cached: `~/.cache/supercharge/brain/<version>/brain`. Say that nothing else is installed, no key is needed, and indexes of the project's docs and any listed sources are built under `~/.cache/supercharge/index/` (a big folder takes a while: about 12 chunks a second). Wait for `go`.
2. Download with `gh release download <tag> --repo <repo> --pattern <asset> --dir <temp dir>`, from the file's `tag` and `repo` and the entry's `asset`. Check the file's sha256 against the entry. If it differs, delete the file, say the checksum failed, and stop: run nothing. Otherwise make it executable and move it to the cache path.
3. Run the binary on the question, as below.

Never write into the project, or into a source's raw files. Only `brain init` and the steps below write into a source, and only the `update: true` one.

## Answer

Run `brain init` and then `brain index`. `init` creates `BRAIN.md`, `decisions/`, `notes/` and `projects/` in the `update: true` source if they're missing, and nothing else; with no such source it writes nothing. If it printed `created …`, or Brain repo CI below wrote the workflow, commit and push those files now, as under Writing to the source, so nothing agents made stays untracked (a folder is tracked once it holds a file). `index` embeds only what changed, so it takes seconds when nothing did. Its `indexed N/M files` lines are progress, not a count of failures; the last lines are the result: a `not converted` list names files with no text to give (scans, images, audio, video, encrypted or corrupt files) and a `left out` list files it skipped for size, each with the reason. Mention them only if the answer may sit in one. A `brain: warning` line names a listed source this machine lacks: tell the human. Then spawn a subagent to do the searching, and don't run `search` yourself, so the snippets stay out of this session (omp's `sonic` agent, which has a shell and runs on `smol`: omp's `scout` is read-only and cannot run the binary; in Claude Code a fresh subagent on `haiku`). Give it the question, the binary's path and the project's root, and these rules:

- `brain search [--terms …] [--about …] [-k N]`, run from the project's root, prints one hit per file across the project and its sources, best first: its rank, `[source]` (`project`, or the path as listed), path and heading, date and score, then the best passage. Add `--json` when you need a hit's absolute `file` to read it whole.
- `--terms` is full-text (FTS5): words are ANDed, `OR` between alternatives, `"quotes"` for a phrase. Use it for what must match literally: an ID, a name, an error code. It matches whole words, so give a prefix (`mötuneyti*`) for languages that inflect, Icelandic above all.
- `--about` describes the answer in plain language and matches by meaning, in any language: use it alone when the question and the answer may share no words.
- Give both only when you have an exact token and a description. Loose keywords beside `--about` push the right file down.
- Narrow with `--kind` (`decision` for every decision and ADR; or `business`, `product`, `process`, `adr`, `note`, `raw`), `--project`, `--since YYYY-MM-DD` and `--status` (`proposed`, `accepted`, `superseded`). Decisions rank above documents, and a superseded one is left out unless `--all`; then it is marked `superseded` with `superseded_by` naming what replaced it. An amended ADR comes with the ADRs that amend it (`amended_by`); read them together.
- Read the snippets. If nothing answers, rephrase in other words or another language, narrow the terms, or raise `-k`; stop after three rounds. Read a whole file (read-only) only when a snippet isn't enough.
- A hit of kind `note` (a path under `notes/`) is a summary an agent wrote from several files, and a fresh one ranks above the files it was made from. One marked `STALE note` (`"stale": true` in `--json`) was made from a file that has changed or is gone: it ranks below the raw files, so don't answer from it; answer from its sources, which `brain stats` lists, and say the note is out of date.
- Answer in at most 10 lines, each claim with its path, the source it is from (leave out `project`) and the file's date. A decision or ADR that is accepted and not superseded settles what it covers; a superseded one is only history, said as such. Prefer the newer file where two others disagree, and say so. If nothing answers it, say that; don't guess.
- When the answer had to combine two or more files, none of which answers it alone, and no fresh note already does, end the report with `note: <title>` and the `[source] path` of each file it used. An answer from one file, or from a fresh note, proposes none.

Never edit anything in the project or a source while answering, beyond that commit.

## Decisions

Applies only when the binary is already cached: these steps never start the download. With no `update: true` source nothing is written, and decisions stay in the feature doc and the ADRs, which search finds where they are.

**Before asking.** `supercharge` calls this before it asks the human a business question. Search for each as under Answer (one search subagent can take them all, after `init` and `index`). An accepted decision or ADR that covers the question settles it: cite it (path and date) and don't ask. Documents, proposals and notes only inform the default you offer. If a hit contradicts what the human is asking for, say so in the questions.

**Recording.** After the human's `go`, with an `update: true` source, write each business, product or process decision they answered (a default accepted by `go` counts) as `decisions/YYYY-MM-DD-slug.md` in that source, dated today, with the settled frontmatter: `title`, `date`, `kind`, `project` (the repo name from `origin`, else the folder's), `status: accepted`, `decided-by`, `source` (the issue's URL). No `touches`: the code doesn't exist yet. The body is context, decision, alternatives and consequences, in a few lines. If it replaces a decision of the `update: true` source that the search found, add `supersedes: [name]` and set that one's `status: superseded` and `superseded-by`: the only edit ever made to an accepted decision. One in a read-only source or an ADR is never edited: say in the body what it replaces, and for an ADR report that a new ADR is due. Never record a brainstorm that ends in "not now". Name each file in the feature doc's assumptions and decisions (its name and source, not a relative link: the source sits outside the repo) instead of repeating it.

**Writing to the source.** In a git repo: `git pull --rebase` (a remote with nothing yet has nothing to pull), write, `git add` only the files you wrote or `init` created, commit and push to its default branch; for files already written (`init`'s, the workflow), commit first, then `git pull --rebase` and push. Stop and say so on a conflict. Otherwise just write the files.

**Ship step.** Right after a merge into `main`, by any route, the skill that merged runs this in a builder subagent (omp's `task`; in Claude Code a fresh subagent on `sonnet`) and relays its report. Give it the merged PRs, whose descriptions carry their `brain:` lines (a feature's final PR carries its slices' combined), and the feature doc, if any. With no `update: true` source, skip it. In the project on `main` as merged, run `init` and `index`, then:

1. **Everything since the last ship.** `projects/<project>.yaml` in the source holds `commit:`, the project's last reconciled commit. Run `brain check --project <project> --changed <commit>..HEAD --json`: it covers every commit since, so one pushed outside the skills is caught too. With no file yet, or a commit that is no longer in the history, use the merged PRs' paths instead and say so.
2. **Apply the updates.** Each entry the PRs' `brain:` lines list, and each one `check` matched that no `brain:` line covers, is read against the diff. A decision that still holds stays. One the change reworked or contradicts is superseded: write the new decision as under Recording (`source` the PR, `touches` for the new code, `supersedes`) and set the old one's `status: superseded` and `superseded-by`. A matched note is updated or replaced as under Lint. `brain: unchanged` with a reason stands. For each `dangling` entry, whose code is gone or moved: a decision is superseded the same way, with `touches` on the new code, or, when the thing was removed, by a decision saying so and no `touches`; a note's `touches` is retargeted, or the note deleted.
3. **Record what shipped.** The business, product and process decisions the shipped work made that the brain doesn't hold yet, from the feature doc's assumptions and decisions and the PRs' decision ledgers, and only those a future agent would undo or ask about. Run `index` first, so what step 2 wrote counts as held. For each candidate search what the brain holds (`search --kind decision --project <project> --about "<the decision>"`): an accepted decision that states the same rule with the same value is held, whether `supercharge` wrote it at `go` or an earlier ship step did, so running it twice records each once. One that states a different value for it is not: record the shipped one, superseding it as under Recording. Write each as under Recording, with `source` the PR and `touches` globs for the paths it governs. A technical decision at that bar is not written to the brain: report it as a missing ADR.
4. **Lint** the notes that are stale and those whose `touches` match the paths changed since the last ship, and no others: the whole brain is linted only by `/brain refresh`.
5. **Reconcile.** Write `projects/<project>.yaml` (`commit:` the project's `HEAD` now, `reconciled:` today), run `brain check` as the brain's CI will and fix what it fails (never by editing an accepted decision), then commit and push as above.
6. **Report** what was recorded, updated, linted and flagged, and end with the health line `brain: N stale, N dangling touches, N duplicates`: the stale and duplicate notes from the `notes:` line `index` prints (none printed means none), dangling from a fresh `brain check --project <project> --json`.

## Notes

With an `update: true` source (with none, write no note), once the binary is cached.

**Writing.** After a `/brain` answer whose report proposes a note, a builder subagent (omp's `task`; in Claude Code a fresh subagent on `sonnet`) writes `notes/<slug>.md` in that source, from the files themselves, not from the answer. A note holds what is expensive to get from the raw files (a 40-page policy → its ten rules, an answer that took five documents), never how the code works and never a decision (those are `decisions/`). Nothing turns raw files into notes wholesale. Frontmatter, with any value that holds a colon in quotes:

```yaml
title: Business trips, approval and meals
summary: one line
aliases: [other names for the topic]
questions: [the questions it answers, as people ask them]
sources:
  - path: ~/Dropbox/Policies:travel.md
    hash: <sha256 of that file>
verified: <today>
project: <repo name>        # when it is about one project
```

`path` is the `[source]` a hit shows, a colon, and the file's path in it (`project:docs/PRODUCT.md`; a file of the `update: true` source itself needs no source). `hash` is the sha256 of the file (`sha256sum`, or `shasum -a 256` on macOS) at its path under the `root` that `brain stats --json` gives for its source. The body is the rules, figures and names the answer needed, in a few lines, each from a source. Then write and commit as under Writing to the source, and run `index`.

**Lint.** A builder subagent as above lints notes. `brain stats --json` lists, under each source's `notes`, the `stale` (each source `changed` or `gone`, with its `current` hash), the `orphans` (no source left) and the `duplicates` (`path` repeats `of`, a newer note). Each note it lints ends in one of:

- **keep**: fresh, and not a duplicate; left as it is.
- **update**: read the changed sources and rewrite what they changed, or nothing if the note still holds; set each `hash` to its `current` and `verified` to today.
- **replace**: other files now answer it, or in another way: write the new note, delete the old.
- **merge**: a duplicate and its newer note become one, with the sources, questions and aliases of both; delete the other.
- **delete**: no source is left, or it answers nothing now; say why in the report.

A note that contradicts an accepted decision (`search --kind decision --about "<its summary>"`) or the code is flagged in the report, not fixed. Lint writes no note that wasn't there. Commit and push as under Writing to the source, run `index` and `stats`, and report each note's outcome, what is flagged and how many are still stale.

**Refresh.** `/brain refresh` runs `init` and `index`, then lints every note in the `update: true` source, each against the decisions on its topic. With no such source it says there is nothing to refresh.

## Brain repo CI

When the `update: true` source is the root of a GitHub repo (its `origin` is on github.com), `.github/workflows/brain.yml` there is `brain-ci.yml` from next to this file, with `@TAG@`, `@REPO@`, `@ASSET@` and `@SHA256@` filled from `brain.json` (the `tag` and `repo` at the top, the `asset` and `sha256` of `linux-x64`). Write it when it is missing or names another tag than `brain.json`, as the step after `init` under Answer. It runs `brain check` on every push and pull request, and on the default branch keeps one open issue labelled `brain-stale` while notes are stale, listing them and starting `/brain refresh`, closed once none are. In CI it sees only the brain's own files: a note made from a file of a project or another source is judged by the ship step and `/brain refresh`, not there.

## Building a slice

For `build-slice`, once the binary is cached:

- Before removing or reworking code whose purpose isn't clear from the code or its tests, search for why, as under Answer (`--about "<what the code does>"`, and `--kind decision --project <project>`), and cite what you find, with path and date, in the decision ledger. A decision that says it must stay is a reason to keep it, or to supersede it in the ship step.
- With an `update: true` source, after merging the target branch in, run `brain check --project <project> --changed origin/<target>..HEAD --json`. The PR description gets one line: `brain: <entry> (<what the ship step does and why>), …` for each matched entry, a decision by superseding it, or `brain: unchanged: <why>` when none matches or none needs to change. The PR writes nothing to the brain: the code isn't merged yet.
- `drive-feature` combines its slices' `brain:` lines in its final PR.
