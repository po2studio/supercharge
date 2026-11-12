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
- **`brain` binary** (Rust, in this repo under `brain/`, released for 5 platforms): converts any file to text, chunks it, embeds it on the CPU with a multilingual model compiled in, and keeps one SQLite index (full-text plus vectors). Commands: `index`, `search`, `check`, `stats`, `eval`, `bench`.
- **`brain` skill**:
  - first-run setup: the brain's location, then the download with its size and checksum, then the first index;
  - agentic hybrid search: the agent picks terms and/or a plain-language description, filters, and refines, in a scout on the small model, answering with citations and dates;
  - the brain repo's CI.
- **Decisions** in the brain: `supercharge` searches before it asks and records the human's answers, `adr` writes there, and `living-docs` harvests decisions at ship.
- **Notes** distilled from raw files by `living-docs`, each citing its sources by hash, so a change in a source marks the note stale. A lint pass keeps, updates, merges, replaces or deletes.
- **Checks:** `brain check` in the brain repo's CI, plus the PR check in `build-slice`: decisions and notes whose `touches:` match the changed paths.
- **Project docs defer to the brain:** BUSINESS.md and ADRs move there. PRODUCT.md and ARCHITECTURE.md stay per project.
- **Shared index** for big brains: built once in CI, pulled by everyone.
- **Eval:** a corpus plus golden questions, run in CI, with every slice reporting its before and after.

## Acceptance criteria
1. On a machine with only omp or Claude Code and the skills, `/brain <question>` with nothing configured asks where the brain is and shows the download size. On `go` it downloads and checks the binary, indexes, and answers with cited sources. No API key, nothing else installed.
2. Any file a person drops into the brain is indexed or reported as unconverted, including flat text with no structure, PDFs, Office files, email, CSV and Icelandic text. Nothing in it is ever edited by an agent.
3. A question that shares no words with its answer finds it. A question with an exact term (an ID, a name) finds that.
4. A business rule decided in an earlier `supercharge` session is cited, not asked again. A superseded decision never comes back as current.
5. When a raw source changes, the notes built from it show as stale until `living-docs` refreshes them.
6. A PR that changes code a decision or note `touches:` either updates it or says `brain: unchanged` in its body. The brain repo's CI runs `brain check`.
7. The eval runs in CI. Hybrid search beats full-text alone on it, and a regression fails CI.
8. At 1M chunks, search p50 is ≤ 500 ms on a 6-core laptop CPU. A fresh machine on a big brain pulls the shared index and embeds only what changed since it was built.
9. With no brain configured, every skill behaves as it does today.

## Core flows
Adds **Ask the brain** (`/brain <question>`, first-run setup included) to [PRODUCT.md](../PRODUCT.md). Changes **Start a request** (search the brain before asking, record the answers), **Build a slice or ticket** (the brain check before the PR) and **Build a feature** (the finish harvests decisions and refreshes notes). The README gets a Brain section.

## Out of scope
- Access control and sensitive content (owner: not part of this).
- Embeddings over an API, GPUs, any service or daemon.
- OCR of scans and images, audio and video. These files are listed as unconverted.
- Cross-encoder rerankers (the agent reranks) and approximate nearest-neighbour indexes (brute force over 1-bit codes is enough up to the wall).
- The GitHub wiki as storage.
- Harnesses other than omp and Claude Code. The binary works standalone.
- Watching folders in real time. Indexing happens on demand and at ship.

## Risk
Medium (default). This is the first runnable code in the bundle, so the binary gets real tests and runs, not just a review.

## Settled rules (shared by every slice)
**Brain layout.** `brain:` in a project's AGENTS.md config block is the path to the brain root (absolute, `~/…`, or relative to the repo). It's optional: it isn't in the block `living-docs` writes and asks about, so neither `living-docs` nor `supercharge` asks for it as a missing key; only `/brain`'s first run sets it. The default offered on first run is `~/brain`. Inside the root:
- `BRAIN.md`: conventions and format version, short.
- `decisions/YYYY-MM-DD-slug.md`: decisions, ADRs included. Named by date, not number, because many projects and sessions write to one brain.
- `notes/*.md`: distilled by agents.
- `INDEX.md`: generated, one line per decision and note, committed.
- `.brain/`: index and caches, gitignored, per machine.
- **Everything else is raw:** any files, any folders. Agents never edit raw files. Point the brain at an existing docs folder and it works as it is.

**Decision frontmatter:**
```yaml
title: Second phone field on signup
date: 2024-05-14
kind: business | product | architecture | process
project: courier-app          # optional
status: proposed | accepted | superseded
supersedes: [2023-11-02-single-phone]      # optional
superseded-by: 2026-01-10-sms-fallback     # set when superseded
decided-by: owner
touches: [src/signup/**]      # optional, globs in that project
source: https://github.com/…/issues/42     # where it was decided
```
The body has context, decision, alternatives and consequences, in a few lines. Once a decision is accepted, only `status` and `superseded-by` may change. To change a decision, write a new one.

**Note frontmatter:** `title`, `summary` (one line), `aliases`, `questions` (the questions it answers), `sources: [{path, hash}]`, `verified` (date), and optionally `project` and `touches`. A note is stale when any source's hash differs or the source is gone.

**What goes in a note.** Only what the code and the raw files can't cheaply tell you: why, rules, who, processes, company context. Nothing about how the code works.

**Search contract.**
- Invocation: `brain search [--terms <fts query>] [--about <text>] [--kind] [--project] [--since] [--status] [--all] [-k N] --json`.
- Each hit: `{id, path, title, heading, date, kind, status, stale, score, snippet, versions}`.
- `terms` runs full-text search with BM25; `about` runs vector search. With both, the result lists are fused by rank (RRF).
- **Trust order:** accepted decisions, then fresh notes, then raw files, then stale notes.
- Superseded decisions only appear with `--all`, and point to what replaced them.

**Where `supercharge` uses the brain.** Any route that asks business questions (ticket, feature, and the triage after a brainstorm) searches the brain first and cites what's settled instead of asking. After `go`, it records each answer as a decision whose `source` is the issue. A brainstorm that ends in "not now" writes nothing, to the brain included. The direct route asks nothing, so records nothing.

**Harnesses.** The `brain` skill and every skill change work in omp and Claude Code, following [0003](0003-claude-code.md)'s wording rule: name the capability, and name a harness only in a short clause where the two differ.
- Search scout: omp's `scout` on `smol`; in Claude Code a fresh subagent on `haiku`.
- Note ingest (slice 4): a builder subagent, omp's `task`; in Claude Code a fresh subagent on `sonnet`.

**Index.**
- One SQLite file per brain at `.brain/index.sqlite`, holding FTS5 and the vectors (int8, plus 1-bit codes for the first pass).
- Incremental by content hash.
- Records the model ID and binary version; a mismatch means a rebuild.

**Chunking.**
- Split at the strongest boundary available, falling back in order: headings, blank-line paragraphs, lines, sentence ends, whitespace, a hard cut.
- About 400 tokens per chunk, about 10% overlap, tiny pieces merged into their neighbours.
- Each chunk is embedded and indexed with a context header: title › heading path · file path · date.

**Distribution.**
- `skills/brain/brain.json` pins the version, the download size, and a URL and sha256 per platform.
- Platforms: linux x64 and arm64, macOS arm64 and x64, windows x64.
- The binary is cached in `~/.cache/supercharge/brain/<version>/`.
- It's downloaded with `gh release download`, so the bundle still needs nothing beyond `git` and `gh`.
- Nothing is downloaded before the human says `go`, and nothing runs whose checksum doesn't match.

**Eval.**
- Corpus in `brain/eval/corpus/`, questions in `brain/eval/questions/*.yaml` (one file per topic, so slices don't collide).
- Each question has the expected source paths.
- `brain eval` reports recall@5, MRR, p50 search latency and indexing chunks/s, against a committed baseline. CI fails on a regression beyond tolerance.
- Every slice that changes what's indexed or how it ranks adds its cases and puts the before/after in its PR.

**Writing to the brain.** If the brain is a git repo, skills commit and push to its default branch. Otherwise they just write the files.

**No brain configured, no change.** Every skill change applies only when `brain:` is set.

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
  - `brain` is an optional config key, set only by `/brain`, never asked for as missing;
  - `supercharge` searches and records in every route that asks business questions; "not now" records nothing.
- **Default model `multilingual-e5-small`, int8 (MIT).** Measured on a Ryzen 7 5700G with ONNX Runtime int8:

  | Model | English chunks/s | Icelandic chunks/s |
  |---|---|---|
  | multilingual-e5-small | 12.5 | 16.1 |
  | bge-small-en-v1.5 (English-only) | 17.3 | 9.8 (Icelandic overflowed 512 tokens) |

  Supporting other languages costs no speed at this size. EmbeddingGemma 2 (Apache 2.0, 270M text) is estimated at 3–5× slower; slice 8 decides with the full eval.
- **Brute force, no approximate-nearest-neighbour index.** 50k × 256-dim cosine took 14 ms even in JS. A 1-bit first pass plus int8 rescoring covers the 1M wall without a graph to build or keep in memory.
- **Distribution:** the skills CLI (vercel-labs/skills) copies the whole skill folder and has no install hook. It shows only the `description` (as a hint cut at 57 characters, and in full in a panel), so that's where the size goes, and first-run setup repeats it before downloading.
- **The PR check runs in skills, not in project CI.** Project CI would need a token for the brain repo. `build-slice` runs `brain check --changed` before opening its PR, and the brain repo's own CI runs `brain check`.
- *Assumptions, not asked:*
  - writing to the brain means a direct commit and push, gated by the admission rule, lint and `check`, not by a second human approval;
  - the eval corpus is synthetic and committed here. A run on the owner's real corpus happens outside the repo, with only the numbers going into `runs/`;
  - the 1M-chunk search target is p50 ≤ 500 ms, and indexing with the default model is at least 12 chunks/s on a 6-core CPU;
  - one `brain:` path per project, shared by the team (`~/brain` or a relative path).

## Slices
1 first. Then 2 and 3 together. 4 after 3. 5 and 6 together, after 4. 7 after 5. 8 after 2 and 4, alongside 6.
- [ ] [#16](https://github.com/po2studio/supercharge/issues/16): Point at a folder, ask why, get a cited answer
- [ ] [#17](https://github.com/po2studio/supercharge/issues/17): Any file goes in
- [ ] [#18](https://github.com/po2studio/supercharge/issues/18): Decisions are recorded where they're made and found before anyone asks
- [ ] [#19](https://github.com/po2studio/supercharge/issues/19): Notes stay true
- [ ] [#20](https://github.com/po2studio/supercharge/issues/20): Checks: nothing touched goes unreviewed
- [ ] [#21](https://github.com/po2studio/supercharge/issues/21): Project docs defer to the brain
- [ ] [#22](https://github.com/po2studio/supercharge/issues/22): Big brains: build the index once, everyone pulls
- [ ] [#23](https://github.com/po2studio/supercharge/issues/23): Model bake-off on the full eval

## Notes for the builder
- AGENTS.md: `risk: medium`, `approve: [plan, merge]`. The PR into `main` waits for the owner's `go`. CI runs `python3 scripts/check.py`, which also checks relative links in every `.md` file, so eval corpus files must pass it or be excluded by the check.
- Claude Code facts (model aliases, subagents, nesting) are in [0003](0003-claude-code.md); check them at code.claude.com/docs again, not from memory. Claude Code stays marked untested until a run is written up in `runs/`.
- **Smoke runs:** "no smoke run for skill changes" covers skill text only. The binary gets tests, real runs and the eval. Skill-text changes are verified by review against the criteria.
- Every line in a skill should change behaviour. Keep skill additions short and in the existing voice.

