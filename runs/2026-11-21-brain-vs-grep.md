# The brain against plain grep (2026-11-21)

Slice 1 of feature 0002, the first run of the `brain` skill. Two things were run for real: the skill itself in omp, and the same 49 golden questions answered by an agent with the `brain` binary and by an agent with only `grep`, `find` and `cat`.

## Setup

- **Binary:** the published pre-release `brain-v0.1.0` (linux x64, sha256 as pinned in `skills/brain/brain.json`), the same file the skill downloads.
- **Corpora:** the eval corpus in `brain/eval/corpus/` (93 files: markdown and flat text, English and Icelandic), and the same plus 649 files of unrelated technical documentation from crate READMEs (742 files, 5,290 chunks), so grep has something to wade through.
- **Questions:** the 49 in `brain/eval/questions/`: exact terms and IDs, paraphrases that share no words with the answer, Icelandic, flat text, and answers spread over several files. Each has the expected files and a gold answer.
- **Agents:** one `omp` run per question, on `claude-haiku-5.5` (the `smol` model, what the skill's search subagent runs on), thinking off, tools `bash` and `read` only, no skills or context files. The brain arm got the search rules from `skills/brain/SKILL.md` and was told to use only the binary; the grep arm was told to use only shell tools. Both answer in at most 10 lines with paths. The two arms of a corpus ran at the same time, three at once, so the machine's load was the same for both.
- **Grading:** an LLM judged each answer against the gold answer, then I read every answer it marked wrong and corrected four it got wrong (a correct answer with an extra caveat). A correct answer also had to cite an expected file; all of them did. One run per arm: with 49 questions, a difference of two or three answers is within the noise.

## Result

| | answers right | wall time per question (mean) | tokens per question (mean, median) | tokens not served from cache (mean) | tool calls |
|---|---|---|---|---|---|
| 93 files, brain | **47 / 49** | 7.2 s | 17.4k, 14.2k | 4.2k | 1.8 |
| 93 files, grep | 45 / 49 | **6.2 s** | 18.1k, 16.9k | 2.9k | 2.3 |
| 742 files, brain | **47 / 49** | 7.3 s | **19.0k**, **14.6k** | **4.3k** | 1.8 |
| 742 files, grep | 44 / 49 | **6.3 s** | 25.2k, 18.1k | 5.5k | 2.2 |

- **Accuracy:** the brain answered 47 of 49 on both corpora; grep 45 on the small one and 44 on the large one. The brain won on questions whose answer is spread over several files (a budget and a start date, a name and a job title, an approval rule and a daily rate), which grep answered with half of it or with "no document says". Grep won one question the brain lost, `en-usb` on the small corpus; both missed it on the large one, and both missed `flat-paylink-key`.
- **Tokens:** on the large corpus the brain used about a quarter fewer tokens per question (19.0k against 25.2k) and 20% fewer new ones; on the small one the two were level, because grep over 93 files is cheap.
- **Time:** the brain was about a second slower per question in both corpora, not faster. Each `--about` search loads the model (about 1.4 s of CPU). The index is not in these numbers: 6 s once for 93 files, and 376 s once for 742 files (14 chunks a second on a busy machine), then seconds per change.
- **Where the brain failed:** `en-usb` (an English question whose answer is in an Icelandic policy: the best hit was not it, and the agent said nothing answers) and `flat-paylink-key`. In the second the right file, a flat standup log, is the top hit for both `--terms` and `--about`, but the fact sits past the 600-character passage the search prints, and the agent stopped after three rounds without opening the file.
- **Retrieval alone** (`brain eval`, no agent): recall@5 is 0.599 for full-text only, 0.867 for meaning only and 0.803 for both fused, so the hybrid beats full-text alone as the feature requires. It does not beat meaning alone on this corpus.

## The skill, run in omp

A sandbox with only the `brain` skill, `sonnet` as the session model, a fresh `HOME`, an empty project folder and a copy of the eval corpus as the brain. `/skill:brain Why were customers' search pages unavailable in the early hours of 9 January?` with nothing configured.

- **Setup flow:** it asked where the brain lives (default `~/brain`), showed the asset, 171,888,832 bytes (172 MB), the release and the cache path, and stopped. After `go` and a path it downloaded with `gh release download`, checked the sha256, cached the binary, created `BRAIN.md`, `decisions/`, `notes/` and `projects/`, indexed 94 files, and answered with the cause (the search-edge TLS certificate expired after the ACME renewal failed behind a firewall change), the file, and the follow-ups. The project folder stayed empty, no key was asked for, and the index went under `~/.cache/supercharge/index/`.
- **A wrong checksum:** with one hex digit of the pinned sha256 changed, it downloaded, printed both hashes, deleted the file, cached no binary, ran nothing, and offered to retry or to grep by hand instead of going on.

## What went wrong

- **`scout` can't run the binary.** The feature doc said to search in omp's `scout` on `smol`. The first full run showed the scout agent is read-only, with no shell, so it grepped the files and never touched the binary. The skill now names `sonic` (a shell, and the `smol` model). A model also skipped the subagent entirely when the skill only said to search "in a scout"; "spawn a subagent and don't run `search` yourself" fixed that.
- **A progress line read as a failure.** `brain index` prints `indexed 93/94 files` after each full group of chunks, so its last progress line never says 94/94, and the agent twice answered that one file "may have been skipped". The skill now says the progress lines are not failures. The binary still prints that line; a final `indexed 94/94 files` is a one-line change for the next release.
- **`--json` output on one line each.** An earlier run, with search results as one JSON object per hit, scored the brain 43 of 49 against grep's 45 on the small corpus: omp cut the long lines and the agent lost hits. The skill now uses the text output, wrapped at 100 columns, and the runs above use it.
- **The size cap.** A 4 MB file with no spaces made indexing run for more than five minutes, because the tokenizer is slow on huge strings. Chunking no longer hands it long strings, and text files over 1 MiB are listed as left out.

## Changes made afterwards

- The skill uses `sonic` for the search and ignores progress lines; the README, `ARCHITECTURE.md`, `PRODUCT.md` and the feature doc say so.
- Not changed, for a later slice: the passage length (a longer one, or reading the top file when it is the only plausible one, would have answered `flat-paylink-key`), and cross-language retrieval for `en-usb`, which belongs to the model bake-off.
