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
- **`brain` binary** (Rust, in this repo under `brain/`, released for 4 platforms): converts any file to text, chunks it, embeds it on the CPU with a multilingual model compiled in, and keeps one SQLite index (full-text plus vectors). Commands: `index`, `search`, `check`, `stats`, `eval`, `bench`. Decided in [ADR 0001](../adr/0001-one-binary-in-the-bundle.md) and [ADR 0002](../adr/0002-local-embeddings-one-sqlite-index.md).
- **`brain` skill**, which owns every brain behaviour:
  - first-run setup: the brain's location, then the download with its size and checksum, then the first index;
  - agentic hybrid search: the agent picks terms and/or a plain-language description, filters, and refines, in a scout on the small model, answering with citations and dates;
  - the ship step and `/brain refresh`;
  - the brain repo's CI.
- **Decisions** in the brain: `supercharge` searches before it asks and records the human's answers, and the ship step records what shipped work decided. A project's ADRs stay in its repo and are searched where they are.
- **Notes**, written on demand when a `/brain` answer had to synthesise several files, each citing its sources by hash, so a change in a source marks the note stale. A lint pass keeps, updates, merges, replaces or deletes.
- **Ship step** in the `brain` skill, run after every merge into `main` by whichever skill merged it: brings the brain up to date with what shipped, and reports the brain's health.
- **Checks:** `brain check` in the brain repo's CI, which also keeps one issue open while notes are stale, plus the PR check in `build-slice`: decisions and notes whose `touches:` match the changed paths.
- **Project docs defer to the brain:** with a brain, `living-docs` writes no new BUSINESS.md; an existing one and the ADRs stay in the project and are indexed where they are ([ADR 0003](../adr/0003-the-brain-is-a-folder.md)).
- **This bundle's own docs describe a bundle with a binary:** the README's Philosophy ("nothing beyond `git` and `gh`", "read it all in one sitting") and `docs/ARCHITECTURE.md` ("no code to run") are rewritten for a bundle whose optional `brain` skill downloads one binary. The ADRs are the decision; `ARCHITECTURE.md` describes today, so it changes when the binary ships.
- **Shared index** for big brains: built once in CI, pulled by everyone.
- **Eval:** a corpus plus golden questions, run in CI, with every slice reporting its before and after.

## Acceptance criteria
1. On a machine with only omp or Claude Code and the skills, `/brain <question>` with nothing configured asks where the brain is and shows the download size. On `go` it downloads and checks the binary, indexes, and answers with cited sources. No API key, nothing else installed, nothing written into any project.
2. Any file a person drops into the brain is indexed or reported as unconverted, including flat text with no structure, PDFs, Office files, email, CSV and Icelandic text. Nothing in it is ever edited by an agent, and nothing per machine is written into it.
3. A question that shares no words with its answer finds it. A question with an exact term (an ID, a name) finds that.
4. A business rule decided in an earlier `supercharge` session is cited, not asked again. A superseded decision never comes back as current.
5. When a raw source changes, the notes built from it show as stale until the ship step or `/brain refresh` refreshes them. In a brain on GitHub, stale notes show as one open issue that starts the refresh.
6. A PR that changes code a decision or note `touches:` lists it for the ship step to update (a decision by superseding it), or says `brain: unchanged` and why. The brain repo's CI runs `brain check`.
7. The eval runs in CI. Hybrid search beats full-text alone on it, and a regression fails CI.
8. At 1M chunks, search p50 is ≤ 500 ms on a 6-core laptop CPU. A fresh machine on a big brain pulls the shared index and embeds only what changed since it was built.
9. With no brain configured, every skill behaves as it does today, and each of `supercharge`, `build-slice`, `drive-feature` and `living-docs` carries at most one line about the brain.
10. Every merge into `main`, by any route (`supercharge`, `drive-feature`, or `build-slice` on its own), runs the ship step, and its report ends with the brain's health line. Commits that skipped the skills are caught at the next ship. Nothing about code is written to the brain before that code merges.
11. A project's ADRs and BUSINESS.md are found by `brain search` where they are: a superseded ADR only with `--all`, an amended one together with its amendments.

## Core flows
Adds **Ask the brain** (`/brain <question>`, first-run setup included) to [PRODUCT.md](../PRODUCT.md). Changes **Start a request** (search the brain before asking, record the answers, the ship step after a direct change or ticket merges), **Build a slice or ticket** (the brain check before the PR) and **Build a feature** (the finish runs the ship step). The README gets a Brain section.

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
**Brain location.** One brain per human or team, spanning projects ([ADR 0003](../adr/0003-the-brain-is-a-folder.md)). `/brain`'s first run asks where it is (default `~/brain`) and writes the path to `~/.config/supercharge/brain`; a `brain:` key in a project's AGENTS.md config block (absolute, `~/…`, or relative to the repo) overrides it for that project. Neither is in the block `living-docs` writes and asks about, so no skill asks for it as a missing key; with neither set there is no brain. Inside the root:
- `BRAIN.md`: conventions and format version, short.
- `decisions/YYYY-MM-DD-slug.md`: business, product and process decisions. Named by date, not number, because many projects and sessions write to one brain. Architecture decisions stay as ADRs in each project's `docs/adr/`.
- `notes/*.md`: written by agents.
- `projects/<project>.yaml`: the last commit of that project the ship step reconciled.
- **Everything else is raw:** any files, any folders. Agents never edit raw files. Point the brain at an existing docs folder and it works as it is. Nothing per machine is written into the root: the index and caches live under `~/.cache/supercharge/`, since the root may be a synced folder.

**Project name.** The repo name from the `origin` remote, else the folder name. `brain index` and `brain check --changed` take `--project`, defaulting to it inside a git repo.

**Decision frontmatter:**
```yaml
title: Second phone field on signup
date: 2024-05-14
kind: business | product | process
project: courier-app          # optional
status: proposed | accepted | superseded
supersedes: [2023-11-02-single-phone]      # optional
superseded-by: 2026-01-10-sms-fallback     # set when superseded
decided-by: owner
touches: [src/signup/**]      # optional, globs in that project; requires project
source: https://github.com/…/issues/42     # where it was decided
```
The body has context, decision, alternatives and consequences, in a few lines. Once a decision is accepted, only `status` and `superseded-by` may change. To change a decision, write a new one.

**Note frontmatter:** `title`, `summary` (one line), `aliases`, `questions` (the questions it answers), `sources: [{path, hash}]`, `verified` (date), and optionally `project` and `touches`. A note is stale when any source's hash differs or the source is gone.

**Notes.** A note holds what's expensive to get from the raw files (a 40-page policy → its ten rules; an answer that took five documents), never how the code works. Notes are written on demand: by `/brain` when an answer had to synthesise several files. The ship step's lint and `/brain refresh` refresh, merge or delete existing notes and write none. Nothing turns raw files into notes wholesale.

**Project docs in place.** With a brain, `brain index` run from a project also indexes that project's `docs/adr/` and `docs/BUSINESS.md` where they are, keyed by project and path relative to the project root, and never writes there; `adr` works as it does today, and `living-docs` writes no new BUSINESS.md but leaves an existing one alone. These entries are refreshed whenever `brain index` runs from that project (every `/brain` and ship step there) and are never part of the shared index. An ADR's status comes from its status line: `superseded by` hides it like a superseded decision, and `amended by` returns it together with its amendments. ADRs rank with accepted decisions.

**Ship step.** Ship means a merge into `main`, by any route. Right after it, whichever skill merged (`supercharge` for a direct change or a ticket, `drive-feature` for a feature, `build-slice` when it merges into `main` itself) runs the `brain` skill's ship step in a builder subagent and relays its report:
- `brain check --changed` over every commit since the project's reconciled commit, not just this PR's, so changes made outside the skills are caught;
- apply the brain updates the PRs listed (`drive-feature`'s final PR combines its slices' `brain:` lines, as it combines their ledgers); supersede or retarget decisions and notes whose `touches:` now match no file;
- record the business, product and process decisions the shipped work made that the brain doesn't hold yet: from the feature doc's assumptions and decisions and the decision ledgers, only what a future agent would undo or ask about. A technical decision at that bar belongs in an ADR in the project's PR; one found only in a ledger is flagged in the report, not written to the brain;
- lint the stale notes and the entries the changed paths matched; the whole brain is linted only on `/brain refresh`;
- record the reconciled commit, commit and push the brain;
- end the report with one health line: `brain: N stale, N dangling touches, N duplicates`.

**Search contract.**
- Invocation: `brain search [--terms <fts query>] [--about <text>] [--kind] [--project] [--since] [--status] [--all] [-k N] --json`.
- Each hit: `{id, path, title, heading, date, kind, status, stale, score, snippet, versions}`. `date` is the frontmatter date, else the file's last git commit date, else its mtime.
- `terms` runs full-text search with BM25; `about` runs vector search. With both, the result lists are fused by rank (RRF).
- **Trust order:** accepted decisions, then fresh notes, then raw files, then stale notes.
- Superseded decisions only appear with `--all`, and point to what replaced them.

**Where `supercharge` uses the brain.** Any route that asks business questions (ticket, feature, and the triage after a brainstorm) searches the brain first and cites what's settled instead of asking. After `go`, it records each answer as a decision whose `source` is the issue, and the feature doc's assumptions and decisions link to those files rather than repeating them. A brainstorm that ends in "not now" writes nothing, to the brain included. The direct route asks nothing, so records nothing until its ship step.

**Harnesses.** The `brain` skill and every skill change work in omp and Claude Code, following [0003](0003-claude-code.md)'s wording rule: name the capability, and name a harness only in a short clause where the two differ.
- Search scout: omp's `scout` on `smol`; in Claude Code a fresh subagent on `haiku`.
- Ship step, note refresh and lint: a builder subagent, omp's `task`; in Claude Code a fresh subagent on `sonnet`.

**Index.**
- One SQLite file per brain at `~/.cache/supercharge/index/<root-hash>/index.sqlite`, holding FTS5 and the vectors (int8, plus 1-bit codes for the first pass). Conversion caches sit beside it.
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

**Writing to the brain.** If the brain is a git repo, skills pull with rebase, commit and push to its default branch, and stop on a conflict. Otherwise they just write the files. Entries about code are written by the ship step, after the merge; the decisions `supercharge` records at `go` are the one exception, since they're true before anything is built.

**No brain configured, no change.** The `brain` skill owns every brain behaviour. `supercharge`, `build-slice`, `drive-feature` and `living-docs` each carry at most one line, pointing at `brain` for that step, which applies only when a brain is configured.

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
  - `INDEX.md` and Windows are cut; the 1M-chunk bench moves to the big-brains slice; slice 6 folds into slice 3;
  - the other skills carry one line each about the brain; the `brain` skill owns the rest.
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
  - a team shares one brain, so `brain:` in a committed AGENTS.md is a relative path or a path every member has (`~/brain`).

## Slices
1 first. Then 2 and 3 together. 4 after 3. 5 after 4. 7 after 5. 8 last, once the corpus is complete. (The bundle ticket [#49](https://github.com/po2studio/supercharge/issues/49) that 1, 3 and 5 build on has shipped.)
- [ ] [#16](https://github.com/po2studio/supercharge/issues/16): Point at a folder, ask why, get a cited answer
- [ ] [#17](https://github.com/po2studio/supercharge/issues/17): Any file goes in
- [ ] [#18](https://github.com/po2studio/supercharge/issues/18): Decisions are recorded where they're made and found before anyone asks (includes what was slice 6, [#21](https://github.com/po2studio/supercharge/issues/21), closed)
- [ ] [#19](https://github.com/po2studio/supercharge/issues/19): Notes stay true
- [ ] [#20](https://github.com/po2studio/supercharge/issues/20): Checks: nothing touched goes unreviewed
- [ ] [#22](https://github.com/po2studio/supercharge/issues/22): Big brains: build the index once, everyone pulls
- [ ] [#23](https://github.com/po2studio/supercharge/issues/23): Model bake-off on the full eval

## Notes for the builder
- AGENTS.md: `risk: medium`, `approve: [plan, merge]`. The PR into `main` waits for the owner's `go`. CI runs `python3 scripts/check.py`, which also checks relative links in every `.md` file, so eval corpus files must pass it or be excluded by the check.
- Claude Code facts (model aliases, subagents, nesting) are in [0003](0003-claude-code.md); check them at code.claude.com/docs again, not from memory. Claude Code stays marked untested until a run is written up in `runs/`.
- **Smoke runs:** "no smoke run for skill changes" covers skill text only. The binary gets tests, real runs and the eval. Skill-text changes are verified by review against the criteria.
- Every line in a skill should change behaviour. Keep skill additions short and in the existing voice.

