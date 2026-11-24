# Brain

Optional. `/brain <question>` answers from your project's own docs and any folders you list as sources, and says which files and dates it found the answer in, for example `/brain why did we add a second phone field to signup?`. It needs no API key. Indexing, embedding and search run on your machine; the passages the agent reads to answer go to your model provider, like any file it reads.

## Setup

`brain` comes with the bundle; leave it out at install and nothing else changes. The first `/brain` in a project shows the download size and the cache path, and waits for `go`. Then it downloads one binary (about 176 MB, from this repo's releases, cached in `~/.cache/supercharge/brain/<version>/`) for Linux x64 or arm64 (glibc 2.34 or newer) or macOS arm64 or x64, checks its sha256 (a mismatch aborts and runs nothing), indexes and answers. It never asks where your documents are and writes nothing into the project or any source, only under `~/.cache/supercharge/`.

## Sources

The project is always a source, read in place: its docs folder (the [`docs`](config.md#docs) key), `README.md` and `AGENTS.md`. Outside folders are opt-in, listed per project under `brain:` in its `AGENTS.md`, so an agency's clients stay in separate brains: a search covers only the current project and the sources it lists. A source can be any folder of documents, flat or nested, a synced one (Dropbox, Drive) included. Each entry is a `path` (absolute, `~/…`, or relative to the repo) and an optional `update`:

```yaml
brain:
  - path: ~/brains/client-a
    update: true                # agents keep this source's documents true
  - path: ~/Dropbox/ClientA/Policies
```

`update` defaults to `false`: a source is only read. `/brain` never writes the key; asked to add a source, it tells you the lines to add. A committed `brain:` should use paths every teammate has (`~/…` or relative).

It reads Markdown and plain text, HTML, PDF (its text layer), Word, Excel, PowerPoint, OpenDocument, RTF, email, CSV, JSON, YAML and source code. A file with no text to give (a scan, an image, audio, an archive, an encrypted file) is listed by `brain stats` as not converted, with the reason; there is no OCR. Copies and near-identical versions of a document give one hit, the newest.

## Search

`brain search "<question>"` brings the index up to date, then searches by meaning (a question that shares no words with its answer, in any language, Icelandic included) and adds the best full-text matches of any ID, code, acronym or quoted phrase in it, so an invoice number is found by its number. The agent runs it in a small-model subagent, rephrases up to three times, and answers in at most 10 lines with paths, the source and dates. A project ADR whose status line says `superseded by` comes after the current ones, marked with what replaced it. `supercharge` searches the brain before it asks you a business question, and cites what a document already settles instead of asking again.

The index is one SQLite file per source under `~/.cache/supercharge/index/<source-hash>/`, never inside a project or a source. It embeds only files whose content changed; a first index runs at about 12 chunks a second on a 6-core laptop CPU, about 30 on an Apple Silicon MacBook. On 49 golden questions an agent with the brain answered 47 right on both a 93-file and a 742-file corpus, against 45 and 44 for an agent with only `grep` ([the run](../../runs/2026-11-21-brain-vs-grep.md)).

## Keeping documents true

`update: true` on a source means agents keep its documents true. After your `go`, and after every merge into `main`, a builder subagent searches those sources for the documents your answers or the shipped work make untrue and edits them in place, appending one line under a `## History` heading: the date, what changed and where it was decided. In a git repo it commits and pushes to the default branch; elsewhere it just saves. It edits Markdown and plain text only: an out-of-date Word file, PDF or spreadsheet is reported, never edited, and the project's own docs change through PRs as before. A decision no document covers becomes a new document in the first `update: true` source. Writing `update: true` is the consent: nothing is asked first.

When such a source is a GitHub repo, the workflow the skill adds publishes its index as a release asset on every push, so another machine pulls it with `gh` and embeds only what differs ([the measurements](../../runs/2026-11-22-brain-shared-index.md)). Without `gh` or a match, it builds the index itself.

## The CLI

After the first `/brain` the binary is at `~/.cache/supercharge/brain/<version>/brain` (put it on your `PATH`, or `gh release download brain-v<version> --repo po2studio/supercharge --pattern brain-linux-x64` for your platform). Run it inside a project, with no flags, and it resolves that project's sources itself:

```sh
cd ~/code/my-project
brain search "why do we store money as integers"
brain search "INV-2024-0042" --json   # one hit per file: source, path, file, date, snippet, versions
brain stats                           # the sources it sees, and what is indexed in each
brain pack --out dist                 # in a brain repo: its index as the asset(s) to publish (what the CI workflow runs)
```

`--repo <folder>` names the project from anywhere, and `--source <folder>` (repeat for several) reads those folders instead of the `brain:` list. `brain index` only indexes, for CI and debugging. `brain eval` and `brain bench` are for developing the binary: see [`AGENTS.md`](../../AGENTS.md#checks).
