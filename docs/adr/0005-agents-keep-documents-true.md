# 0005: Agents keep the documents true; one search, no layer of their own

Status: accepted

## Context

[ADR 0003](0003-the-brain-is-a-folder.md) and [ADR 0004](0004-brain-sources-per-project.md) never let an agent edit a raw file, even in an `update: true` source. So agents kept a layer of their own beside the documents (`BRAIN.md`, `decisions/`, `notes/`, `projects/`), and that layer needed supersession chains, staleness hashes, `touches:` globs, `brain check`, reconciled commits and a ship step of six steps to stay honest. The documents themselves went out of date, and the `brain` skill grew to 3,000 words for what is, to its user, index and search (owner, 2026-11-22, before merging [feature 0002](../features/0002-second-brain.md)).

## Decision

- **`update: true` means agents keep that source's documents true.** When something changes (an answer at `go`, a merge into `main`), they search, find the documents it makes untrue, and edit them in place. A source without it is read-only. Any number of sources may set it; a new document goes into the first one listed.
- **History is kept in the document:** each edit appends one line under a `## History` heading at its end: the date, what changed, and where it was decided. Git, or the synced folder's own versions, keep the rest. No git is required, and there is no backup or prompt: writing `update: true` is the consent.
- **Agents edit text formats only** (Markdown, plain text). A Word file, PDF or spreadsheet that is out of date is reported to the human, not edited.
- **A decision is an ordinary document,** edited in place when it changes. There is no decision frontmatter, supersession chain, note, staleness, `touches:`, `brain check`, `BRAIN.md`, `projects/` or reconciled commit.
- **One search:** `brain search "<question>"` brings the index up to date, runs full-text and by-meaning search on the question as given, fuses and ranks them, and returns the top hits. There are no `--terms`, `--about` or filter flags. A project's ADRs keep their status lines: a superseded one ranks after the current ones and is marked with what replaced it; an amended one comes with its amendments.

The rest of 0003 and 0004 stands: raw files are the truth and the index is derived; the project is always a source and is never edited by the brain (its docs change through PRs); outside sources are listed per project; nothing per machine is written into a source.

## Alternatives

- Keep agents' own layer beside immutable documents (0003, 0004): the documents go stale, and keeping the layer honest costs most of the skill and the binary.
- Require a git repo for `update: true`: rules out Dropbox and Google Drive, where most documents live; they keep versions of their own.
- A backup copy or a first-time prompt before editing: the `update: true` line already says yes, and the folder already keeps versions.

## Consequences

Documents in an `update: true` source stay current, and their history is readable in place. An agent can make a bad edit; it is one history line and one version back. Commits made outside the skills are not caught by a reconciliation; the next search-and-update usually covers them. The skill and the binary's interface shrink to first run, search and edit.
