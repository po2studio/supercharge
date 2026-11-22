---
name: brain
description: Search your documents (172 MB download on first use). Answers from a project's docs and the folders it lists, citing paths and dates. Use to find why something is the way it is, or what the company already knows, before asking the human or guessing. No API key, nothing else to install.
---

# brain

Answer a question from the project's own documents and the outside folders it lists, and say which files and dates it came from. Search before asking the human or guessing.

`/brain <question>`. The `brain` binary indexes and searches on this machine, offline. Installing this skill turns the brain on for the project you're in; there is nothing to configure and no location to ask for.

## Sources

Run every `brain` command from the project's root; the binary works out the sources itself:

- **The project:** its docs folder (the `docs` key in the `## Supercharge` block of `AGENTS.md`, default `docs`), `README.md` and `AGENTS.md`, read in place. The brain never writes to them.
- **Outside sources:** the `brain:` list in that block, none by default: a list of `path:` (absolute, `~/…` or relative to the repo) with an optional `update: true`. A source is read-only unless it sets `update: true`; at most one does, and two stop every command with a message naming both: relay it and stop. Only in that one do agents write, and only their own files: `BRAIN.md`, `decisions/`, `notes/` and `projects/`. Raw files are never edited, in any source.

Never write the `brain:` key, and never ask for it as a missing key. To add a source, say the lines to add to the block (`brain:`, then `  - path: ~/Dropbox/ClientA/Policies`, with `    update: true` if agents may write there) and leave the edit to the human. A search covers only this project and the sources it lists.

## First-run setup

The pinned binary isn't cached. Don't search, and write nothing, before the human's `go`.

1. Show what will be downloaded, in one message. Take the platform from `uname -sm` (`linux-x64`, `linux-arm64`, `macos-arm64`, `macos-x64`) and read its entry in `brain.json` next to this file: the size in MB (`bytes`), the release it comes from (`repo` and `tag`, at the top of the file), and where it is cached: `~/.cache/supercharge/brain/<version>/brain`. Say that nothing else is installed, no key is needed, and indexes of the project's docs and any listed sources are built under `~/.cache/supercharge/index/` (a big folder takes a while: about 12 chunks a second). Wait for `go`.
2. Download with `gh release download <tag> --repo <repo> --pattern <asset> --dir <temp dir>`, from the file's `tag` and `repo` and the entry's `asset`. Check the file's sha256 against the entry. If it differs, delete the file, say the checksum failed, and stop: run nothing. Otherwise make it executable and move it to the cache path.
3. Run the binary on the question, as below.

Never write into the project, or into a source's raw files. Only `brain init` writes into a source, and only the `update: true` one.

## Answer

Run `brain init` and then `brain index`. `init` creates `BRAIN.md`, `decisions/`, `notes/` and `projects/` in the `update: true` source if they're missing, and nothing else; with no such source it writes nothing. `index` embeds only what changed, so it takes seconds when nothing did. Its `indexed N/M files` lines are progress, not a count of failures; the last lines are the result, and any `left out` list names files it skipped. A `brain: warning` line names a listed source this machine lacks: tell the human. Then spawn a subagent to do the searching, and don't run `search` yourself, so the snippets stay out of this session (omp's `sonic` agent, which has a shell and runs on `smol`: omp's `scout` is read-only and cannot run the binary; in Claude Code a fresh subagent on `haiku`). Give it the question, the binary's path and the project's root, and these rules:

- `brain search [--terms …] [--about …] [-k N]`, run from the project's root, prints one hit per file across the project and its sources, best first: its rank, `[source]` (`project`, or the path as listed), path and heading, date and score, then the best passage. Add `--json` when you need a hit's absolute `file` to read it whole.
- `--terms` is full-text (FTS5): words are ANDed, `OR` between alternatives, `"quotes"` for a phrase. Use it for what must match literally: an ID, a name, an error code. It matches whole words, so give a prefix (`mötuneyti*`) for languages that inflect, Icelandic above all.
- `--about` describes the answer in plain language and matches by meaning, in any language: use it alone when the question and the answer may share no words.
- Give both only when you have an exact token and a description. Loose keywords beside `--about` push the right file down.
- Read the snippets. If nothing answers, rephrase in other words or another language, narrow the terms, or raise `-k`; stop after three rounds. Read a whole file (read-only) only when a snippet isn't enough.
- Answer in at most 10 lines, each claim with its path, the source it is from (leave out `project`) and the file's date. Prefer the newer file where two disagree, and say so. If nothing answers it, say that; don't guess.

Never edit anything in the project or a source while answering.
