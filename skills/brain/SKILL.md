---
name: brain
description: Search your documents (172 MB download on first use). Answers from a folder of files, citing paths and dates. Use to find why something is the way it is, or what the company already knows, before asking the human or guessing. No API key, nothing else to install.
---

# brain

Answer a question from the owner's own documents, and say which files and dates it came from. Search before asking the human or guessing.

`/brain <question>`. The brain is a folder; the `brain` binary indexes and searches it on this machine, offline.

## Find the brain

1. A `brain:` key in the project's `AGENTS.md` config block (absolute, `~/…`, or relative to the repo) wins.
2. Otherwise the path in `~/.config/supercharge/brain`.
3. With neither, run the first-run setup below. Never ask for `brain:` as a missing config key, and never write it.

## First-run setup

Nothing configured, or the pinned binary isn't cached. Don't search, and write nothing, before the human's `go`.

1. Ask, in one message: where the brain lives (default `~/brain`; an existing folder of documents works as it is, so point at it), and show what will be downloaded. Take the platform from `uname -sm` (`linux-x64`, `linux-arm64`, `macos-arm64`, `macos-x64`) and read its entry in `brain.json` next to this file: the size in MB (`bytes`), the release it comes from (`repo` and `tag`, at the top of the file), and where it is cached: `~/.cache/supercharge/brain/<version>/brain`. Say that nothing else is installed, no key is needed, and an index of the folder is built under `~/.cache/supercharge/index/` (a big folder takes a while: about 12 chunks a second). Wait for `go`.
2. Write the location to `~/.config/supercharge/brain`.
3. Download with `gh release download <tag> --repo <repo> --pattern <asset> --dir <temp dir>`, from the file's `tag` and `repo` and the entry's `asset`. Check the file's sha256 against the entry. If it differs, delete the file, say the checksum failed, and stop: run nothing. Otherwise make it executable and move it to the cache path.
4. Create what the brain folder lacks, and nothing else: `BRAIN.md` (a short note: this is a brain, format version 1, everything is raw except what agents own: `BRAIN.md`, `decisions/`, `notes/` and `projects/`, and agents never edit raw files), and the folders `decisions/`, `notes/` and `projects/`.
5. `brain --root <brain> index`, then answer the question as below, with the search subagent.

Never write into a project, and never into the brain's raw files.

## Answer

Run `brain --root <brain> index` first: it embeds only what changed, so it takes seconds when nothing did. Its `indexed N/M files` lines are progress, not a count of failures; its last line is the result, and any `left out` list names files it skipped. Then spawn a subagent to do the searching, and don't run `search` yourself, so the snippets stay out of this session (omp's `sonic` agent, which has a shell and runs on `smol`: omp's `scout` is read-only and cannot run the binary; in Claude Code a fresh subagent on `haiku`). Give it the question, the binary and the brain's paths, and these rules:

- `brain --root <brain> search [--terms …] [--about …] [-k N]` prints one hit per file, best first: its rank, path and heading, date and score, then the best passage.
- `--terms` is full-text (FTS5): words are ANDed, `OR` between alternatives, `"quotes"` for a phrase. Use it for what must match literally: an ID, a name, an error code. It matches whole words, so give a prefix (`mötuneyti*`) for languages that inflect, Icelandic above all.
- `--about` describes the answer in plain language and matches by meaning, in any language: use it alone when the question and the answer may share no words.
- Give both only when you have an exact token and a description. Loose keywords beside `--about` push the right file down.
- Read the snippets. If nothing answers, rephrase in other words or another language, narrow the terms, or raise `-k`; stop after three rounds. Read a whole file (read-only) only when a snippet isn't enough.
- Answer in at most 10 lines, each claim with its path and the file's date. Prefer the newer file where two disagree, and say so. If nothing answers it, say that; don't guess.

Never edit anything in the brain while answering.
