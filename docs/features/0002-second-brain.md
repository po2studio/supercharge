# 0002: Second brain: a searchable, self-maintaining knowledge base

Feature issue: [#15](https://github.com/po2studio/supercharge/issues/15). Status: **planned**.

## Why
Living docs say what a project is today: business, product, architecture, design, ADRs. Nothing holds the rest:
- detailed decisions and why they were made ("why was a second phone number field added to the signup form two years ago?");
- the company's documents: processes, features, policies, meeting notes, whatever people have.

Agents can read code to see *how* things work, but not *why*, and they can't find what the company already knows. The owner wants a second brain that agents search before they ask or guess, and keep up to date as work ships, without it filling with garbage, going stale, wasting tokens or slowing work down.

Hard requirements (owner, 2026-11-03):
- **No dependencies, no API keys, no services.** Install the skills, point at a folder, it works.
- **One binary.** Downloaded on first use, after showing its size.
- **Raw files stay the source of truth.** Everything else is derived and can be rebuilt whenever.
- **Scales** to a big company's documents: 1M docs is the wall.
- **Measured:** complexity has to earn its place in numbers.

## Deliverables
- **`brain` binary** (Rust, in this repo under `brain/`, released for 4 platforms): converts any file to text, chunks it, embeds it on the CPU with a multilingual model compiled in, and keeps one SQLite index per source (full-text plus vectors). Its interface is one command, `brain search "<question>"`; `index`, `stats`, `pack`, `eval` and `bench` serve CI and debugging. Decided in [ADR 0001](../adr/0001-one-binary-in-the-bundle.md), [ADR 0002](../adr/0002-local-embeddings-one-sqlite-index.md) and [ADR 0005](../adr/0005-agents-keep-documents-true.md).
- **`brain` skill**, which owns every brain behaviour, in about 500 words:
  - first-run setup: the download with its size and checksum. There's no location question: the project is always a source, and outside sources are listed per project ([ADR 0004](../adr/0004-brain-sources-per-project.md));
  - search in a subagent on the small model, answering with citations and dates;
  - keeping documents true: when something changes, search, then edit the documents it makes untrue in `update: true` sources, each with a history line;
  - the brain repo's CI, which publishes the shared index.
- **Decisions are documents:** `supercharge` searches before it asks; after `go`, and after a merge into `main`, the documents in `update: true` sources that the answers or the shipped work make untrue are edited in place. With no such source, decisions stay in the feature doc and the ADRs, as today, and are searched there. A project's ADRs stay in its repo and are searched where they are.
- **Project docs are read in place:** the project's docs folder, README and AGENTS.md are always a source, never written by the brain; they change through PRs, as today. With an outside source, `living-docs` writes no new BUSINESS.md; an existing one stays in the project ([ADR 0004](../adr/0004-brain-sources-per-project.md)).
- **This bundle's own docs describe a bundle with a binary:** the README's Philosophy ("nothing beyond `git` and `gh`", "read it all in one sitting") and `docs/ARCHITECTURE.md` ("no code to run") are rewritten for a bundle whose optional `brain` skill downloads one binary. The ADRs are the decision; `ARCHITECTURE.md` describes today, so it changes when the binary ships.
- **Shared index** for big brains: built once in CI, pulled by everyone.
- **Eval:** a corpus plus golden questions, run in CI, with every slice reporting its before and after.

## Acceptance criteria
1. On a machine with only omp or Claude Code and the skills, `/brain <question>` in a project with nothing configured shows the download size. On `go` it downloads and checks the binary, indexes the project's docs, and answers with cited sources. No API key, nothing else installed, nothing written into the project or any source.
2. Any file a person drops into a source is indexed or reported as unconverted, including flat text with no structure, PDFs, Office files, email, CSV and Icelandic text. Nothing per machine is written into it, and only a source set `update: true` is ever edited.
3. A question that shares no words with its answer finds it. A question with an exact term (an ID, a name) finds that.
4. A business rule decided in an earlier `supercharge` session is cited, not asked again. When a decision changes, the document that holds it says the new rule, with a history line for the old one.
5. After a merge into `main` or an answer at `go`, the Markdown and text documents in `update: true` sources that it makes untrue are edited, each with a history line, and the report lists them; an out-of-date Word file, PDF or spreadsheet is reported, not edited.
6. `brain search "<question>"` is the whole interface: no flags needed, the index brought up to date first.
7. The eval runs in CI. Hybrid search beats full-text alone on it, and a regression fails CI.
8. At 1M chunks, search p50 is ≤ 500 ms on a 6-core laptop CPU. A fresh machine on a big brain pulls the shared index and embeds only what changed since it was built.
9. Without the `brain` skill installed, every skill behaves as it does today, and each of `supercharge`, `build-slice`, `drive-feature` and `living-docs` carries at most one line about the brain.
10. Every merge into `main`, by any route (`supercharge`, `drive-feature`, or `build-slice` on its own), runs the update step. Nothing about code is written to a source before that code merges.
11. A project's ADRs and BUSINESS.md are found by `brain search` where they are: a superseded ADR after the current ones, marked with what replaced it, and an amended one together with its amendments.

## Core flows
Adds **Ask the brain** (`/brain <question>`, first-run setup included) to [PRODUCT.md](../PRODUCT.md). Changes **Start a request** (search the brain before asking, update the documents after `go` and after a direct change or ticket merges), **Build a slice or ticket** (search for why before removing unclear code) and **Build a feature** (the finish runs the update step). The README gets a Brain section.

## Out of scope
- Access control and sensitive content (owner: not part of this).
- Embeddings over an API, GPUs, any service or daemon.
- OCR of scans and images, audio and video. These files are listed as unconverted.
- Cross-encoder rerankers (the agent reranks) and approximate nearest-neighbour indexes (brute force over 1-bit codes is enough up to the wall).
- The GitHub wiki as storage.
- Harnesses other than omp and Claude Code, and Windows. The binary works standalone.
- Watching folders in real time. Indexing happens on demand and at ship.

## Risk
Medium (default). This is the first runnable code in the bundle, so the binary gets real tests and runs, not just a review.

## Settled rules (shared by every slice)
**Sources** ([ADR 0004](../adr/0004-brain-sources-per-project.md)). Installing the `brain` skill turns the brain on; there's no global path and no location question.
- **The project** is always a source, read in place and never written by the brain: its docs folder (the `docs` key in AGENTS.md, which may move it), `README.md` and `AGENTS.md`.
- **Outside sources** are listed in the project's AGENTS.md config block, none by default: `brain:` is a list of `{path, update}`, each path absolute, `~/…` or relative to the repo. `update` defaults to `false`. `/brain` never writes the key; it says which line to add. It's not in the block `living-docs` writes and asks about, so no skill asks for it as missing.
- **Writing** ([ADR 0005](../adr/0005-agents-keep-documents-true.md)): a source that sets `update: true` is kept true by agents; any number may set it, and a new document goes into the first one listed. With none, the brain writes no documents anywhere. Agents edit Markdown and plain text in place and report other formats as out of date. Each edit appends one line under a `## History` heading at the document's end: `- YYYY-MM-DD: <what changed> (<where it was decided>)`. No git is required; when the source is a git repo, the edit is committed and pushed. The one other file an agent writes into a source is the brain repo's CI workflow, below.
- **Everything is raw,** in every source: any files, any folders, flat or nested. Nothing per machine is written into a source: the index and caches live under `~/.cache/supercharge/`, since a source may be a synced folder.
- **Separation:** a search covers only the current project and the sources it lists, so organisations stay apart.
- **The brain repo** below means an `update: true` source that is a GitHub repo.

**Project name.** The repo name from the `origin` remote, else the folder name.

**Decisions** are ordinary documents in an `update: true` source, written and edited like any other: the rule, why, who decided and where, in a few lines. When a decision changes, its document is edited to the new rule and its history line records the old one. Architecture decisions stay as ADRs in each project's `docs/adr/`.

**Project docs in place.** `brain search` run from a project indexes its docs folder, `README.md` and `AGENTS.md` where they are, keyed by project and path relative to the project root, and never writes there; `adr` works as it does today. With an outside source, `living-docs` writes no new BUSINESS.md but leaves an existing one alone. These entries are never part of the shared index. An ADR's status comes from its status line: a `superseded by` one ranks after every current hit and is marked with what replaced it; an `amended by` one comes together with its amendments.

**Update step.** When something changes: after `go` (`supercharge`'s answers) and right after a merge into `main`, by any route (whichever skill merged: `supercharge` for a direct change or a ticket, `drive-feature` for a feature, `build-slice` when it merges into `main` itself). With no `update: true` source it is skipped. Otherwise a builder subagent searches for what the answers or the shipped work make untrue in the `update: true` sources, edits those documents with a history line each, writes a new one where none covers a decision a future agent would undo or ask about, and reports the files it edited and the ones it could not (other formats). A technical decision belongs in an ADR in the project's PR, not in a source.

**Search contract.**
- Invocation: `brain search "<question>" [-k N] [--json]`, run from the project's root. It first indexes what changed in each source.
- It runs by-meaning search on the question as given, and adds the best full-text (BM25) matches of the exact tokens in it (IDs, codes, acronyms, quoted phrases; each only if it matches under 1% of an index's chunks), fuses them by rank, and returns the top `k` (default 5), one hit per file: its source, path, heading, date and the best passage. Full-text search of every word of the question lowered recall on plain questions in the eval (0.906 by meaning alone, 0.80 fused with it), so words are left to the by-meaning half.
- Each hit: `{source, path, file, title, heading, date, kind, status, score, snippet, versions}`. `date` is the frontmatter date, else the file's last git commit date, else its mtime.
- On the eval, the fused search must find at least as many answers in the top 5 as the best of full-text or by-meaning alone.

**Where `supercharge` uses the brain.** Any route that asks business questions (ticket, feature, and the triage after a brainstorm) searches the brain first and cites what's settled instead of asking. After `go`, with an `update: true` source, the update step writes the answers into the documents they belong in, and the feature doc's assumptions and decisions name those files rather than repeating them; without one, the answers go in the feature doc as today. A brainstorm that ends in "not now" writes nothing, to the brain included. The direct route asks nothing, so writes nothing until its merge.

**Harnesses.** The `brain` skill and every skill change work in omp and Claude Code, following [0003](0003-claude-code.md)'s wording rule: name the capability, and name a harness only in a short clause where the two differ.
- Search subagent: omp's `sonic` on `smol` (it has a shell; `scout` is read-only and can't run the binary, found in slice 1's first real run); in Claude Code a fresh subagent on `haiku`.
- Update step: a builder subagent, omp's `task`; in Claude Code a fresh subagent on `sonnet`.

**Index.**
- One SQLite file per source at `~/.cache/supercharge/index/<source-hash>/index.sqlite` (the project's docs are one source), holding FTS5 and the vectors (int8, plus 1-bit codes for the first pass). Conversion caches sit beside it. Projects that list the same source share its index. A search runs over the project's index and those of its sources, fused by rank, and each hit says which source it came from.
- Incremental by content hash.
- Records the model ID and binary version; a mismatch means a rebuild.

**Chunking.**
- Split at the strongest boundary available, falling back in order: headings, blank-line paragraphs, lines, sentence ends, whitespace, a hard cut.
- About 400 tokens per chunk, about 10% overlap, tiny pieces merged into their neighbours.
- Each chunk is embedded and indexed with a context header: title › heading path · file path · date.

**Distribution.**
- `skills/brain/brain.json` pins the version, the download size, and a URL and sha256 per platform.
- Platforms: linux x64 and arm64, macOS arm64 and x64.
- The binary is cached in `~/.cache/supercharge/brain/<version>/`.
- It's downloaded with `gh release download`, so the bundle still needs nothing beyond `git` and `gh`.
- Nothing is downloaded before the human says `go`, and nothing runs whose checksum doesn't match.

**Eval.**
- Corpus in `brain/eval/corpus/`, questions in `brain/eval/questions/*.yaml` (one file per topic, so slices don't collide).
- Each question has the expected source paths.
- `brain eval` reports recall@5, MRR, p50 search latency and indexing chunks/s, against a committed baseline. CI fails on a regression beyond tolerance.
- Every slice that changes what's indexed or how it ranks adds its cases and puts the before/after in its PR.

**Writing to a source.** Only an `update: true` source is written. If it's a git repo, skills pull with rebase, commit and push to its default branch, and stop on a conflict. Otherwise they just write the files. What shipped code changes is written by the update step after the merge; the answers recorded at `go` are the one exception, since they're true before anything is built.

**Not installed, no change.** The `brain` skill owns every brain behaviour. `supercharge`, `build-slice`, `drive-feature` and `living-docs` each carry at most one line, pointing at `brain` for that step, which applies only when the `brain` skill is installed and its binary is already cached. They never start the download; only `/brain` asks for it.

## Assumptions and decisions
- **Owner, 2026-11-03:**
  - no dependencies or API keys; one binary; a first-run download is fine if its size is shown;
  - the binary lives in this repo; ADRs become part of the brain;
  - search is hybrid and agentic: the agent picks terms and refines;
  - plan the final form and skip the intermediate stages;
  - measure as we go;
  - sensitivity is out of scope; no GitHub wiki;
  - 1M docs is the wall, and the expected size is well below it; multilingual content.
- **Owner, 2026-11-12** (review before building, after feature [0003](0003-claude-code.md) and the `supercharge` entry point shipped):
  - the brain works in Claude Code too, not only omp;
  - `brain` is an optional config key, set only by `/brain`, never asked for as missing (the third review made it a per-project override of a global path);
  - `supercharge` searches and records in every route that asks business questions; "not now" records nothing;
  - the brain stays in this repo as an opt-in skill: `npx skills add` lets the human pick which skills to install (or `--skill <name>`), and copies only `skills/<name>/`, so `brain/` source never reaches a user's machine and skipping `brain` at install changes nothing else. The README's Philosophy and `docs/ARCHITECTURE.md` are updated as part of this feature.
- **Owner, 2026-11-12** (second review, on keeping docs from going stale):
  - ADRs stay in each project's repo, landing in the same PR as the code they govern, and the brain indexes them where they are. This replaces "ADRs become part of the brain" from 2026-11-03, and keeps `amended by`;
  - ship means any merge into `main`; the brain is brought up to date then, by one ship step every route runs.
- **Owner, 2026-11-12** (third review, of the whole bundle and this plan):
  - the architecture is recorded as the bundle's rule requires, in [ADR 0001](../adr/0001-one-binary-in-the-bundle.md), [0002](../adr/0002-local-embeddings-one-sqlite-index.md) and [0003](../adr/0003-the-brain-is-a-folder.md);
  - the ship step runs after every merge into `main`, `build-slice`'s own included, in a builder subagent;
  - notes are written on demand from answers, never by turning raw files into notes wholesale; `/brain refresh` only lints;
  - the brain's path is global (`~/.config/supercharge/brain`) with `brain:` as a per-project override; nothing per machine goes into the root;
  - BUSINESS.md stays in the project and is indexed in place, like ADRs; moving it into notes would have left every note stale;
  - `INDEX.md` and Windows are cut; the 1M-chunk bench moves to the big-brains slice; the slice that moved BUSINESS.md into notes folds into the decisions slice (3);
  - the other skills carry one line each about the brain; the `brain` skill owns the rest.
- **Default model `multilingual-e5-small`, int8 (MIT).** Measured on a Ryzen 7 5700G with ONNX Runtime int8:

  | Model | English chunks/s | Icelandic chunks/s |
  |---|---|---|
  | multilingual-e5-small | 12.5 | 16.1 |
  | bge-small-en-v1.5 (English-only) | 17.3 | 9.8 (Icelandic overflowed 512 tokens) |

  Supporting other languages costs no speed at this size. Slice 7 measured EmbeddingGemma 2 (Apache 2.0, 256 dimensions) on the full eval and kept e5: hybrid recall@5 0.917 → 0.911, indexing about 7× slower, the binary 2.2× bigger ([runs/2026-11-22-brain-model-choice.md](../../runs/2026-11-22-brain-model-choice.md)).
- **Brute force, no approximate-nearest-neighbour index.** 50k × 256-dim cosine took 14 ms even in JS. A 1-bit first pass plus int8 rescoring covers the 1M wall without a graph to build or keep in memory.
- **Distribution:** the skills CLI (vercel-labs/skills) copies the whole skill folder and has no install hook. It shows only the `description` (as a hint cut at 57 characters, and in full in a panel), so that's where the size goes, and first-run setup repeats it before downloading.
- **The PR check runs in skills, not in project CI.** Project CI would need a token for the brain repo. `build-slice` runs `brain check --changed` before opening its PR, and the brain repo's own CI runs `brain check`.
- **Owner, 2026-11-21:** replanned in the one-issue format of feature [0004](0004-readable-issues.md). The slice issues #16–#20, #22 and #23 are closed; their criteria, risks and touches are below, unchanged, and [#15](https://github.com/po2studio/supercharge/issues/15) carries the checklist. Two titles now state the outcome: slice 5 (was "Checks: nothing touched goes unreviewed") and slice 7 (was "Model bake-off on the full eval").
- *Assumptions, not asked:*
  - writing to the brain means a direct commit and push, gated by the admission rule, lint and `check`, not by a second human approval;
  - the eval corpus is synthetic and committed here. A run on the owner's real corpus happens outside the repo, with only the numbers going into `runs/`;
  - the 1M-chunk search target is p50 ≤ 500 ms, and indexing with the default model is at least 12 chunks/s on a 6-core CPU;
  - a team shares its sources, so paths in a committed `brain:` are relative or ones every member has (`~/brains/client-a`).
- **Owner, 2026-11-22** (after trying slice 1; [ADR 0004](../adr/0004-brain-sources-per-project.md)):
  - the brain's knowledge stays out of the project; the project's docs are read in place;
  - installing the `brain` skill turns it on, for the current project only, with no outside source by default; not installing it keeps it off;
  - outside sources go in the project's AGENTS.md config block, so an agency's clients stay in separate brains; one source is enough, several are nice to have;
  - writing must be explicit: `update: true` per source, default `false`, raw files never edited even then;
  - a flat folder with no structure works as a source.
  
  This replaces the global path (`~/.config/supercharge/brain`) and the location question from 2026-11-12; slice 1b reworks what slice 1 shipped.
- **Owner, 2026-11-22** (while building):
  - binaries are built on GitHub-hosted runners only, macOS included;
  - the scale measurements of slice 6 (1M chunks, a 100k-document brain) run on the owner's Apple Silicon MacBook, not the shared build machine; results in [runs/2026-11-22-brain-shared-index.md](../../runs/2026-11-22-brain-shared-index.md);
  - slice 5 also commits every agent-owned file `brain init` creates (`BRAIN.md` stayed untracked) and lets the eval accept a fresh note as a right answer.
- **Owner, 2026-11-22** (before merging, after reading the 3,000-word `brain` skill; [ADR 0005](../adr/0005-agents-keep-documents-true.md)):
  - the binary may be complex inside, but its interface and the skill must be trivial to use: `brain search "<question>"`, no flags;
  - `update: true` means agents keep that source's documents true, editing them in place; leaving documents wrong was never the point;
  - history is recorded in the document itself; no git requirement (Dropbox and Drive are expected), no backup, no first-time prompt;
  - decisions are ordinary documents; notes, decision frontmatter, supersession, `touches:`, `brain check`, `BRAIN.md`, `projects/` and reconciled commits are cut, from the skill and the binary.
  
  This replaces "raw files never edited" (2026-11-12, 2026-11-22) and what slices 3, 4 and 5 built on it; slice 8 removes it.

## Slices
1 first, then 1b. Then 2 and 3 together. 4 after 3. 5 after 4. 6 after 5. 7 once the corpus is complete. 8 last. (The bundle ticket [#49](https://github.com/po2studio/supercharge/issues/49) that 1, 3 and 5 build on has shipped.)

### Slice 1: Point at a folder, ask why, get a cited answer
**Delivers** the thinnest path through the whole thing: install the skills, run `/brain <question>`, answer two questions (where the brain lives, and whether to download, with the size shown), and get an answer with citations from a folder of markdown and text files. It also sets up everything later slices build on:
- **The `brain/` Rust crate:** `index`, `search`, `stats`, `eval`; chunker v1; `multilingual-e5-small` int8 compiled in; one SQLite file under `~/.cache/supercharge/index/<root-hash>/` with FTS5, int8 vectors and 1-bit codes; hybrid search fused by rank (RRF); incremental indexing by content hash.
- **Release workflow:** a `brain-v*` tag builds the 4 platforms and uploads binaries plus sha256.
- **Rust CI:** tests, clippy and `brain eval`, run only when `brain/` changes.
- **The `brain` skill** (`skills/brain/SKILL.md` and `brain.json`):
  - first-run setup: location, written to `~/.config/supercharge/brain` (a `brain:` key in AGENTS.md overrides it, but setup never writes one); consent with the size; download; checksum; cache; layout created if missing; first index;
  - agentic search in a subagent (omp's `sonic` on `smol`, not `scout`, which can't run the binary; in Claude Code a fresh subagent on `haiku`): the agent picks terms and/or a description, reads snippets, refines, and answers in ≤ 10 lines with paths and dates;
  - its description states the download size.
- **Eval v1:** markdown and text corpus, including flat text, Icelandic, and a pair with no shared words.
- **A `runs/` write-up:** agent-level accuracy, tokens and time per question, against an agent grepping the same folder without the brain.
- **This repo's AGENTS.md** says what the repo now is and adds the Rust checks. `scripts/check.py` also fails a skill description over 57 characters before its first full stop, or over 1024 in total. **README:** a Brain section (install size, setup, search), the optional `brain` config key in Config, and in Models what the scout runs on in each harness. **The README's Philosophy** ("nothing beyond `git` and `gh`", "read it all in one sitting", "five skills") **and `docs/ARCHITECTURE.md`** ("no code to run") are rewritten for a bundle whose optional `brain` skill downloads one binary, as ADRs 0001–0003 decide.

**Acceptance criteria** (feature AC 1, 3, 7, 9, and 2 for flat text)
1. On a machine with only omp or Claude Code and the skills, `/brain <question>` with nothing configured:
   - asks for the location, defaulting to `~/brain`;
   - shows the download size and cache path, and waits for `go`;
   - downloads, verifies the sha256, creates the layout, indexes, and answers with cited paths.

   It never asks for an API key, and writes nothing into any project.
2. A wrong checksum aborts, says so, and runs nothing.
3. A flat text file with no headings or newlines is indexed and found.
4. A question that shares no words with its answer file finds it in the top 5. An exact term (an ID or a name) is found with `--terms`. An Icelandic question finds its Icelandic answer.
5. After one file changes, `brain index` re-embeds only that file's chunks (`stats` shows the count).
6. `brain eval` prints recall@5, MRR, p50 latency and chunks/s. Hybrid beats `--terms`-only on recall@5. CI fails on a regression against the committed baseline.
7. The release workflow produces 4 binaries plus checksums. A binary indexes and searches with no network and no other files.
8. No raw file is ever written by the binary or the skill, and nothing is written into the brain root but `BRAIN.md`, `decisions/`, `notes/` and `projects/`.
9. The `runs/` write-up compares the brain with plain grep on the same questions.
10. A project without `brain:` is never asked for it: not by `living-docs`, not by `supercharge` as a missing key.
11. Nothing in the README's Philosophy or `docs/ARCHITECTURE.md` is made untrue by the binary: both describe the bundle with its optional `brain` skill and its one download, and `ARCHITECTURE.md` links the ADRs.

**Dependencies:** none. **Build with:** nothing.
**Touches:** `brain/` (new), `skills/brain/` (new), `.github/workflows/`, `scripts/check.py`, AGENTS.md, the README (Philosophy, Brain, Config and Models sections), `docs/ARCHITECTURE.md`, `runs/`.
**Risks:**
- Statically linking an inference runtime on all 4 platforms, macOS especially. If one platform can't be done, stop and say so; don't ship fewer platforms silently.
- e5 needs `query: ` and `passage: ` prefixes.
- Binary size. Target ≤ 200 MB; the skill description states the real number.
- This is the biggest slice. If it has to split, split along the steps of the end-to-end path, never into layers.

Shipped 2026-11-21 ([#65](https://github.com/po2studio/supercharge/pull/65)). Its location question and global path are replaced by slice 1b.

### Slice 1b: The brain works on the project with no setup; outside sources opt in
**Delivers** the source model of [ADR 0004](../adr/0004-brain-sources-per-project.md), reworking slice 1's setup:
- **Binary:** run inside a project, resolves its sources itself (the project's docs in place: its `docs` folder, `README.md`, `AGENTS.md`; read-only, keyed by project and path relative to the repo root; plus the `brain:` list of AGENTS.md; `--repo` and `--source` override) and indexes each, one index per source under `~/.cache/supercharge/index/<source-hash>/`; `search` runs over the project's index and its sources', fuses them by rank, and each hit names its source. `init` creates the layout in the `update: true` source. The ADR status handling stays in slice 3.
- **The `brain` skill:**
  - reads sources from the `brain:` list in the project's AGENTS.md; no location question, no `~/.config/supercharge/brain`;
  - first run asks only for the download: size and cache path, then `go`;
  - runs `brain init` on every answer, which creates `BRAIN.md`, `decisions/`, `notes/` and `projects/` only in the `update: true` source, if missing (the binary owns the layout; `index`, `search` and `stats` never write into a source); two `update: true` sources stop every command with a message naming both;
  - never writes `brain:`; when asked to add a source, it says the line to add.
- **Docs:** README (Brain and Config), `docs/PRODUCT.md` and `docs/ARCHITECTURE.md` describe the source model; nothing still mentions `~/.config/supercharge/brain`.

**Acceptance criteria** (feature AC 1, 9, and 11 for finding project docs in place)
1. In a project with the skill installed and nothing configured, `/brain <question>` shows the download size and cache path and waits for `go`; then it downloads, verifies, indexes the project's docs folder, `README.md` and `AGENTS.md`, and answers citing them. Nothing is written into the project, and nothing anywhere but `~/.cache/supercharge/`.
2. A project whose `docs` key points elsewhere indexes that folder.
3. With `brain:` listing a source without `update`, answers cite the project and that source, and nothing is written into the source.
4. With a source set `update: true`, the first run creates `BRAIN.md`, `decisions/`, `notes/` and `projects/` there if missing, and nothing else. Two `update: true` sources stop with a message naming both.
5. Two projects listing different sources: a search in one never returns the other's source files. Two projects listing the same source index it once.
6. A flat folder (no subfolders, no headings) works as a source.
7. No skill or doc reads, writes or mentions `~/.config/supercharge/brain`, and a project without `brain:` is never asked for it.
8. The eval runs over more than one source with no regression; the PR shows before and after.

**Dependencies:** after 1. **Build with:** nothing.
**Touches:** `brain/` (sources, one index per source, search across them), `skills/brain/`, `brain/eval/`, README (Brain, Config), `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`.
**Risks:** search across several index files must keep latency; measure it in the eval.

### Slice 2: Any file goes in
**Delivers:** whatever people drop into the brain gets indexed.
- **Conversion to text in the binary:** html, pdf (text layer), docx, xlsx, pptx, odt/ods/odp, rtf, eml/mbox, csv/tsv, json/yaml, and source code. Cached by content hash beside the index under `~/.cache/supercharge/index/<root-hash>/`, never in the brain.
- **Chunking rules for specific formats:** tables and CSV as row groups, with the header repeated in each; transcripts by speaker turn; email headers as metadata, quoted replies stripped; code blocks kept whole.
- **Duplicates:** exact copies by hash; near-copies by SimHash (a fingerprint for near-identical text); the newest version ranks, and the others are listed in the hit's `versions`.
- **Unconvertible files are reported, not errors:** scans, images, audio, video, encrypted files. They're listed by `brain stats`.

**Acceptance criteria** (feature AC 2)
1. For each format above, an eval question whose answer exists only in a file of that format finds it.
2. A scanned PDF and an image are listed as unconverted by `brain stats`, and indexing completes.
3. A corrupt file is skipped and reported, and indexing completes.
4. Three near-identical versions of a document (`v1`, `v2`, `final_final.docx`) return one hit, the newest, with the others in `versions`.
5. Running the index again over unchanged files converts nothing.
6. The eval gets one questions file for formats and one for duplicates. The PR shows before and after, with no regression.

**Dependencies:** after 1b. **Build with:** 3.
**Touches:** `brain/`: the convert, chunk and dedup modules; search only for filtering duplicates out, using the `versions` field from the settled hit shape. `brain/eval/` (new corpus files and question files). README Brain section (formats line).
**Risks:**
- PDF text extraction quality on complex layouts. Measure it in the eval, and don't add OCR.
- Binary size growth from the format libraries.

### Slice 3: Decisions are recorded where they're made and found before anyone asks
**Delivers:**
- **Binary:** indexes decision frontmatter; `search` filters by `--kind --project --since --status`; applies the trust order; superseded decisions are hidden unless `--all`, and point to what replaced them; a project's ADRs (indexed in place since slice 1b) take their status from their status line, as the settled rule says.
- **`supercharge`, with the `brain` skill installed** (one line, pointing at `brain`): in any route that asks business questions (ticket, feature, the triage after a brainstorm), searches it before asking and cites the settled ones instead of asking again; after `go`, with an `update: true` source, records each owner decision there as a decision file with `decided-by` and `source` set to the issue, and links them from the feature doc instead of repeating them.
- **`living-docs`, with an outside source** (one line): writes no new BUSINESS.md, and leaves an existing one alone. **`adr`** doesn't change.
- **The ship step, v1** (in the `brain` skill, per the settled rule): after any merge into `main`, in a builder subagent, with an `update: true` source, record the business, product and process decisions the shipped work made that the brain doesn't hold yet (a technical one at that bar is flagged as a missing ADR, not written), then pull with rebase, commit and push that source. One line each in `supercharge` (after a direct change or ticket merges), `drive-feature` (the finish) and `build-slice` (when it merges into `main` itself) runs it. Slices 4 and 5 add the rest of the ship step.

**Acceptance criteria** (feature AC 4, 9, 11, and 10 for recording decisions)
1. `supercharge`, asked for a ticket or feature that touches a rule already decided in the brain, cites that decision and doesn't ask the question.
2. After `go`, with an `update: true` source, every answered business question exists there as a decision file with valid frontmatter and a source link, and the feature doc links to it. Without one, the answers are in the feature doc as today.
3. Searching a topic whose decision was superseded returns the current decision. The old one appears only with `--all`, marked superseded and pointing to its replacement.
4. A project's ADRs and BUSINESS.md are found by `brain search` where they are: a superseded ADR only with `--all`, an amended one together with its amendments. Nothing in the project is written by the brain.
5. After a merge into `main` by any route (direct, ticket, feature, `build-slice` on its own), the ship step records the shipped work's decisions; running it twice records each exactly once. Nothing about code is written to the brain before its merge.
6. A brainstorm that ends in "not now" writes nothing to the brain.
7. With an outside source, `living-docs` on a project without BUSINESS.md creates none; on one with it, leaves it as it is. Without the `brain` skill installed, its docs are exactly today's.
8. The eval gets a decisions questions file, including a superseded-decision trap and an amended ADR, and the PR shows before and after.

**Dependencies:** after 1b. **Build with:** 2.
**Touches:** `brain/`: frontmatter metadata, ADR status, filters, ranking. `skills/brain` (ship step), one line each in `skills/supercharge`, `skills/drive-feature`, `skills/build-slice`, `skills/living-docs`. `brain/eval/`. README Brain section.
**Risks:** decision bloat. Record only what a future agent would undo or ask about, which is the bar `adr` already uses.

### Slice 4: Notes stay true
**Delivers:**
- **Notes on demand** (in the `brain` skill): when a `/brain` answer had to synthesise several files, the scout proposes a note and a builder subagent writes it in the `update: true` source (with none, no note), following the notes rule: `sources` with hashes, `summary`, `aliases`, `questions`, `verified`. Nothing turns raw files into notes wholesale.
- **Binary:** `index` marks a note stale when a source's hash changed or the source is gone; stale notes rank below raw files and are flagged in hits; `stats` reports stale notes, notes nothing points to, and duplicates.
- **Lint** (in the `brain` skill, in a builder subagent): stale notes are refreshed or deleted; contradictions with code or decisions are flagged, not silently fixed; duplicates are merged; each note found ends in one of keep, update, merge, replace, delete. The ship step lints only the stale notes and the entries the changed paths matched; `/brain refresh` lints the whole brain. Neither writes new notes.

**Acceptance criteria** (feature AC 5, without the brain repo's issue)
1. After a raw source changes, search hits from its notes carry `stale: true` and `stats` lists them.
2. After the ship step or `/brain refresh`, those notes are refreshed with new hashes and today's `verified`, or deleted, with the reason in the report.
3. A `/brain` answer that drew on several files produces one note that answers the question next time, ranked first; an answer from one file produces none.
4. A note whose only source was deleted is removed or flagged.
5. Two notes covering the same thing are merged by lint.
6. No note restates how the code works. The reviewer checks this against the notes rule.
7. The eval gets a notes questions file, including a stale-note trap, and the PR shows before and after.
8. The PR reports what a note costs (tokens and time) and what a full `/brain refresh` costs on the eval corpus.
9. The ship step's lint covers only stale notes and the entries the changed paths matched; a full lint runs only on `/brain refresh`.

**Dependencies:** after 3 (the ship step and the metadata module). **Build with:** nothing.
**Touches:** `skills/brain` (notes, `/brain refresh`, the ship step's lint), `brain/` (note metadata, staleness, stats), `brain/eval/`, README Brain section.
**Risks:** note bloat and token cost. A note only when an answer needed several files; lint on the cheap model; report the cost.

### Slice 5: Nothing a change touches goes unreviewed
**Delivers:**
- **`brain check`** fails (with file and reason) on any of: invalid decision or note frontmatter, including `touches` without `project`; broken links; an accepted decision whose body was edited (checked against git history when the brain is a git repo). Stale notes are a warning, not a failure.
- **`brain check --changed <paths or commit range> --project <name>`** lists the decisions and notes whose `touches:` match, and warns on entries whose `touches:` match no file in the project (dangling).
- **`brain` skill:**
  - when the `update: true` source is a GitHub repo (the brain repo), setup adds a CI workflow to it that downloads the pinned binary, runs `brain check`, and keeps one open issue listing stale notes, starting `/brain refresh` (closed once none are left). In CI it sees only the brain's own files, so a note made from a file of a project or another source is judged by the ship step and `/brain refresh`;
  - the ship step gains: `--changed` over every commit since the project's reconciled commit (`projects/<project>.yaml`), not just the PR's; applying the updates the PRs listed (`drive-feature`'s final PR combines its slices' `brain:` lines); superseding or retargeting dangling entries; recording the new reconciled commit; and ending the report with `brain: N stale, N dangling touches, N duplicates`.
- **`build-slice`, with the `brain` skill installed** (its one line points at `brain`, which says): before removing or reworking code whose purpose isn't clear from the code or tests, search the brain for why and cite it in the decision ledger; before opening the PR, run `--changed` with the PR's paths, and the PR body lists the matched entries for the ship step to update (a decision by superseding it), or says `brain: unchanged` and why. It writes nothing to the brain itself.
- **Commit what is created** (human-approved addition, 2026-11-22): `brain init` creates `BRAIN.md` in the `update: true` source, but the skill's write step added only the files it wrote, so `BRAIN.md` stayed untracked. Every agent-owned file `init` or the skill creates (`BRAIN.md`, the CI workflow) is committed and pushed as soon as it exists, even when nothing else is written.
- **Eval: a fresh note answers too** (human-approved addition, 2026-11-22): a question may name in `accept` a fresh note that answers it; one of them in the top 5 counts as a right answer beside the raw files in `expect`. Notes rank above the files they summarise, so with only the raw files expected they lowered hybrid recall (0.884 → 0.856 on CI). The PR reports the before and after.

**Acceptance criteria** (feature AC 6, 10, and 5 for the brain repo's issue)
1. Each failure case above makes `brain check` exit non-zero, naming the file and reason. A stale note only warns.
2. Editing an accepted decision's body fails `check`. Changing only its `status` or `superseded-by` passes.
3. In a brain that's a GitHub repo, the setup-added workflow runs `brain check` on push, and keeps exactly one open issue while notes are stale.
4. A `build-slice` PR that changes a path a decision `touches:` lists it for the ship step (a decision by superseding it) or says `brain: unchanged` with a reason; the ship step applies it after the merge, including from a feature's combined PR.
5. `build-slice`, asked to remove a field whose reason is recorded only in the brain, finds and cites that decision before acting.
6. A commit pushed to `main` outside the skills is caught by the next ship step's `--changed`.
7. A decision whose `touches:` match no file is flagged by `check --project` and superseded or retargeted at the next ship.
8. Every ship report ends with the brain's health line.
9. After `brain init` creates `BRAIN.md` in an `update: true` source that is a git repo, the skill commits and pushes it, and the workflow too: nothing `init` or the skill created stays untracked.
10. A question whose `accept` names a fresh note counts the note as a right answer; an accepted note that is missing or stale fails the eval. The PR shows before and after.

**Dependencies:** after 4. **Build with:** nothing.
**Touches:** `brain/` (`check`, the eval's `accept`), `brain/eval/`, `skills/brain` (brain-repo CI template, setup, ship step, the `build-slice` rules), the one brain line in `skills/build-slice` and `skills/drive-feature`, `docs/adr/0004-brain-sources-per-project.md` (the workflow among the files agents own), README Brain section.

### Slice 6: Big brains: build the index once, everyone pulls
**Delivers:**
- **Building and publishing:** the brain repo's CI workflow (from slice 5) also builds the index and publishes it, keyed by the brain commit, the model ID and the binary version. It never contains a project's docs.
- **Fetching:** on a machine with no index, `brain index` fetches the newest prebuilt index matching its model and version, using the GitHub access the bundle already has. Then it embeds only the files that differ, and the project docs of the project it runs from.
- **Falling back:** with no match, or offline, it builds locally.
- **`brain bench`:** a synthetic brain at a given chunk count, reporting search p50 and indexing chunks/s.

**Acceptance criteria** (feature AC 8)
1. On a fresh clone of a brain whose CI has published an index, `brain index` downloads it and embeds only files changed since that commit (`stats` shows the count), plus the current project's docs.
2. An index from a different model or binary version is ignored, and a local build runs.
3. With no network, indexing builds locally and says why.
4. On a 100k-document synthetic brain, a fresh machine's first index takes minutes, not hours. Both times go in the PR.
5. `brain bench` at 1M synthetic chunks: search p50 ≤ 500 ms on a 6-core laptop CPU. Indexing with the default model is at least 12 chunks/s. Both numbers go in the PR.

**Dependencies:** after 5 (same workflow template). **Build with:** nothing.
**Touches:** `brain/` (fetching the index, `bench`), `skills/brain` (CI template), README Brain section.
**Risks:** artifact size limits for large indexes. Pick the storage (release assets or Actions artifacts) by measuring.

### Slice 7: The default model is picked on the full eval
**Delivers:** the full eval corpus (all formats, decisions, notes) run with the default `multilingual-e5-small` and with EmbeddingGemma 2 (text only, 256 dimensions, Apache 2.0), compared on recall@5, MRR, search latency, indexing chunks/s and binary size, on one machine named in the write-up, with the results written up in `runs/`.
- **Rule:** switch only if recall@5 rises by 5 points or more, and indexing a 1M-doc brain on one machine is still possible with the shared index from slice 6.
- **If it switches:** release a new binary, update the pins and size in `skills/brain/brain.json` and the skill description, and have indexes rebuild because the model ID changed.

**Acceptance criteria** (the hard requirement: measured)
1. The `runs/` write-up has a table of both models on every metric, measured on the same machine.
2. The decision follows the rule, and the write-up says which way it went and why.
3. If switched: a new release, updated pins and size, and an existing index rebuilds on the next `brain index`.
4. If not switched: nothing in `brain/` changes except the eval baseline, if the corpus grew.

**Dependencies:** last: after 6, so the corpus is complete and the shared index exists. **Build with:** nothing.
**Touches:** `brain/` (the embedding module), `skills/brain/brain.json` and its description, the release, `runs/`.

### Slice 8: The brain is braindead to use
**Delivers** [ADR 0005](../adr/0005-agents-keep-documents-true.md): one command, one short skill, and documents that agents keep true.
- **Binary:**
  - `brain search "<question>" [-k N] [--json]` indexes what changed first, then runs full-text and by-meaning search on the question as given and fuses them; `--terms`, `--about`, `--kind`, `--project`, `--since`, `--status` and `--all` are removed;
  - a superseded ADR ranks after the current hits, marked with what replaced it; an amended one keeps its amendments;
  - removed, code and tests: notes and staleness, decision frontmatter and supersession, `touches:`, `check` and `--changed`, `init` and `BRAIN.md`, the trust-order lift for decisions and notes, and their parts of `stats`;
  - `index`, `stats`, `pack`, `eval` and `bench` stay for CI and debugging; formats, duplicates, sources and the shared index are unchanged.
- **The `brain` skill,** rewritten in at most 500 words: first run; answer (search in a small subagent, rephrase up to three times, cite paths and dates); before asking (search, cite what's settled); the update step (search, edit Markdown and text in `update: true` sources in place with a history line, commit and push in a git repo, report edited files and other formats it could not edit). `brain-ci.yml` keeps only the shared-index job.
- **The other skills:** `supercharge`, `build-slice`, `drive-feature` and `living-docs` keep one line each, reworded: no `brain:` line in PRs, no `brain check`.
- **Eval:** questions are asked as plain questions; the notes questions and the decision-file supersession traps go, the ADR traps stay. The PR shows before and after per mode, and the fused search clears the bar in the search contract.
- **Docs:** README (Brain, Config, Usage), PRODUCT, ARCHITECTURE and AGENTS.md describe the new model; no doc still mentions what's removed.

**Acceptance criteria** (feature AC 2, 4, 5, 6, 10, 11)
1. `brain search "<question>"` with no flags answers the eval: hybrid recall@5 at least that of by-meaning alone (0.917 on today's baseline, re-measured on the trimmed eval), and the exact-term, Icelandic and paraphrase questions still pass.
2. After a file changes, `brain search` finds the new content without a separate `index`.
3. A superseded ADR comes after the current ADR that replaced it, marked; an amended one comes with its amendments.
4. `brain --help` lists no removed command or flag, and no skill or doc mentions one.
5. The `brain` skill is at most 500 words; each of the other four skills carries at most one line about the brain.
6. Run for real in omp with an `update: true` source that is a plain folder (no git): an answer at `go` that changes a rule edits the document holding it, adds a history line, and lists the file; a `.docx` holding an out-of-date rule is reported, not edited. Repeat with a git repo source: the edit is committed and pushed.
7. The binary stays as fast: search p50 on the eval no worse than today's beyond tolerance.

**Dependencies:** after 7. **Build with:** nothing.
**Touches:** `brain/` (search, removal of notes, decisions, check, init), `skills/brain/` (SKILL.md, brain-ci.yml, brain.json after the release), one line each in `skills/supercharge`, `skills/build-slice`, `skills/drive-feature`, `skills/living-docs`, `brain/eval/`, README, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, AGENTS.md, `runs/`.
**Risks:**
- Fusion that loses recall on plain questions: measure each mode; stop and say so if the bar can't be met.
- An agent's bad edit to a document: one history line, one version back; the run in AC 6 checks the edit is minimal.

## Notes for the builder
- The **Settled rules** above are binding for every slice; the ADRs in `docs/adr/` are the architecture.
- AGENTS.md: `risk: medium`, `approve: [plan, merge]`. The PR into `main` waits for the owner's `go`. CI runs `python3 scripts/check.py`, which also checks relative links in every `.md` file, so eval corpus files must pass it or be excluded by the check.
- Claude Code facts (model aliases, subagents, nesting) are in [0003](0003-claude-code.md); check them at code.claude.com/docs again, not from memory. Claude Code stays marked untested until a run is written up in `runs/`.
- **Smoke runs:** "no smoke run for skill changes" covers skill text only. The binary gets tests, real runs and the eval. Skill-text changes are verified by review against the criteria.
- Every line in a skill should change behaviour. Keep skill additions short and in the existing voice; `supercharge`, `build-slice`, `drive-feature` and `living-docs` get at most one line each about the brain.
- Without the `brain` skill installed, nothing you change may alter today's behaviour.

