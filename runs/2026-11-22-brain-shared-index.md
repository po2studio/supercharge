# The shared index, run for real (2026-11-22)

Slice 6 of feature 0002. The binary built from the slice's branch (`brain` 0.6.0) and the skill's CI template, run against the private throwaway repo `heidar/supercharge-brain-test` on GitHub-hosted runners. The scale measurements the acceptance criteria name (a 100 000-document brain, 1 000 000 chunks) were **not** run on this machine; see "What is still open".

## Machine and method

- **Local machine:** AMD Ryzen 7 5700G (6 cores, no SMT), 15 GB RAM, Linux, shared with the owner's other work (so speeds are noisy: indexing ran from 8.3 to 14.6 chunks/s across runs of the same code). The binary was built at opt-level 1 in an `ubuntu:22.04` container capped at 6 GB, because the host has no C compiler and little spare memory; a release build is faster only in the Rust code, and the embedding model's speed is ONNX Runtime's, not the crate's. Every run used a throwaway cache folder; nothing under `~/.config` or `~/.cache/supercharge/`.
- **CI:** GitHub-hosted `ubuntu-latest`. The workflow was the skill's `brain-ci.yml` unchanged except for its four placeholders, filled with this repo, a release (`test-bin`) holding the locally built 0.6.0 binary and its sha256, because the 0.6.0 release did not exist yet; the last row below repeats it with the released binary.

## The shared index, through GitHub

| Step | Result |
|---|---|
| Push of the workflow with the `index` job | run green, `index` job 14 s, `check` job 9 s; release `brain-index` created with one asset, 7.9 KB, named `brain-index_multilingual-e5-small-int8_0.6.0_<commit>.sqlite.gz.001` |
| Push of one changed file (`policies/travel.md`) | the job pulled the last published index and embedded 1 chunk (`started_from` the previous commit, `files: 6`, `chunks: 6`, `embedded: 1`, 0.01 s); the release then held the newest two indexes |
| Push of another changed file | same: 1 chunk embedded, previous index started from, older asset deleted |
| A commit pushed with `[skip ci]` (so newer than the newest index) and a fresh clone of the repo, listed by a throwaway project | `brain index`: `pulled the shared index built at 8a13533ffd24`, then `1 files changed, 1 chunks embedded` for the file that differed, and the project's docs built locally (they are never shared); 3.3 s in all, mostly loading the model. `brain stats` says which commit the index started from. `brain search --terms parking` found the file that only the delta held |
| The same with `HTTPS_PROXY` pointing nowhere | `brain: <brain>: no shared index (gh said: Post "https://api.github.com/graphql": proxyconnect tcp: dial tcp 127.0.0.1:9: connect: connection refused); building the index locally`, then all 6 files embedded locally |
| After the release: the workflow filled from `brain.json`'s real pins (`po2studio/supercharge`, `brain-v0.6.0`, the linux-x64 sha256), one more changed file | run green; `started_from` the previous commit's index, `embedded: 1`; the newest two indexes kept |
| Creating the `test-bin` and `brain-index` releases | each pushed a tag, which started the workflow once on that tag; the `index` and stale-issue steps are limited to the default branch, so only `check` ran |

The tests (`brain/tests/shared.rs`) cover what the real run cannot easily: another model or binary version (named right but built by another version inside) ignored, a partial upload (a part missing) never pulled, an index in several parts, no `gh`, and a folder that is no brain repo never asking GitHub.

## Storage: release assets or Actions artifacts

The same 102 MB file (a real index of 50 000 synthetic chunks, 262 MB before gzip, 2.6 times smaller) on a GitHub-hosted runner and from the local machine:

| | release asset | Actions artifact |
|---|---|---|
| upload from the runner | 4.0 s | 4 s |
| download in the runner | 0.9 s and 2.1 s (two runs) | 1 s |
| download from the local machine | 14.0 s (`gh release download`) | 37.3 s (`gh run download`, which first needs a run id) |
| expires | never | 90 days by default (a private repo may set up to 400) |
| size limit | 2 GiB a file, no limit on the total | counts against the plan's artifact storage: 500 MB on Free, 1 GB on Pro, 2 GB on Team |

(Limits from GitHub's documentation: "About releases", and Actions billing and artifact retention.) Speed does not tell them apart in the runner; the limits do. An index at 1 000 000 chunks is about 2 GB gzipped (2 KB a chunk on the synthetic brain), over a free plan's artifact storage and at the edge of an asset, so a pack is cut into parts of at most 1.9 GB and the fetch joins them. An artifact that expires would also leave a brain nobody touched for three months with no shared index. Release assets: nothing outside the GitHub features the bundle already uses (releases, Actions, `gh`), so no ADR.

One cost the human should know: a first build of a big brain runs on their Actions minutes. At the 10.7 chunks/s this machine managed, 100 000 one-chunk documents are about 2.6 hours of embedding; a hosted runner has 2 to 4 cores, so expect more. The job stops at 330 minutes (`--budget`), publishes what it has and starts the workflow again, and a machine that pulls a partial index finishes it itself.

## Measured here, at the size that fits

`brain/scripts/scale.sh` with the 0.6.0 binary, 6 cores:

| | measured |
|---|---|
| `brain bench`, 50 000 chunks | search p50 83 ms hybrid (79 ms meaning, 2 ms full-text), of which 7 ms embeds the question: at this size every vector is scored exactly |
| the same with the 1-bit first pass forced (`--exact-limit 1000`), as a brain over 50 000 chunks searches | 32 ms hybrid, about 27 ms of it the scan of 50 000 codes (about 0.5 µs a chunk) |
| `brain search` as the command line runs it | about 1.0 s: loading the model is almost all of it, whatever the brain's size |
| indexing with the real model | 11.1 chunks/s here in this run (8.3 to 14.6 in others); `brain eval` on CI-built binaries reports its own figure |
| a 3 000-document brain (one chunk each): CI's build from scratch | 280 s (10.7 chunks/s); asset 6.2 MB |
| a fresh machine's first `brain index` of a clone of it | 1 s: pulled, 0 files changed, 0 chunks embedded |
| 1 500 documents | 185 s to build, 1 s to pull |

Two things the bench found and the slice fixed: the 1-bit scan read every code through a query that also dragged the wide rows along and copied each code (80 ms at 50 000 chunks), now a covering index and no copy (35 ms); and opening a connection per thread costs more than it saves at 50 000 chunks (27 ms in one thread, 40 ms in six), so the scan splits over threads only from 200 000 chunks. The threaded scan is tested for giving the same chunks as the single one, and **not measured** at scale.

**Extrapolations, not measurements.** The scan is linear: 1 000 000 chunks is about 500 ms single-threaded plus the rest of the search, so just over the 500 ms target on this machine in one thread; over 6 threads it should be about a sixth of that plus a fixed 10 to 20 ms. A 100 000-document brain is about 210 MB packed (2.1 KB a document), about 30 s to download at the 14 s per 102 MB seen here. Neither is a pass until the Mac run below.

## What is still open

Acceptance criteria 4 and 5 of slice 6 need two numbers on a machine this host is not: the first index of a 100 000-document brain from a shared one, and `brain bench` at 1 000 000 chunks (p50 at most 500 ms; indexing at least 12 chunks/s). The owner runs them on an Apple Silicon MacBook after the 0.6.0 release:

```sh
git clone --branch feature/0002-second-brain https://github.com/po2studio/supercharge.git && cd supercharge   # or main, once the feature has shipped
mkdir -p ~/brain-bench && gh release download brain-v0.6.0 --repo po2studio/supercharge --pattern brain-macos-arm64 --dir ~/brain-bench && chmod +x ~/brain-bench/brain-macos-arm64
~/brain-bench/brain-macos-arm64 bench --chunks 1000000                       # AC 5: about 5 GB of disk, removed afterwards
brain/scripts/scale.sh ~/brain-bench/brain-macos-arm64 1000 100000           # AC 4: the 100 000-document brain (1000 chunks for its bench, which AC 5 already ran)
```

`scale.sh` with 100 000 documents prints two times: the CI's build from scratch (it embeds every document once, so hours on a laptop) and a fresh machine's first index from the shared one it made (the number the criterion is about). Both go in the PR. A pass is search p50 at most 500 ms on the `search p50 hybrid` line of the first command at 1 000 000 chunks, at least 12 chunks/s on its `indexing` line, and the second command's fresh-machine line in minutes. Until those are in the PR, the two criteria stay open.

### The owner's Mac run

On the owner's MacBook (Apple Silicon, Darwin arm64, 12 cores), `brain` 0.6.0 from the `brain-macos-arm64` release asset, 2026-11-22.

**AC 5: passes.** `bench --chunks 1000000`: 1 000 000 chunks in 125 000 files (5.2 GB), stored in 121 s, 12 threads.

| at 1 000 000 chunks | measured | target |
|---|---|---|
| search p50, hybrid | 179 ms | ≤ 500 ms |
| search p50, terms / about | 10 ms / 160 ms (3 ms of it embeds the question) | |
| `brain search` from the command line | 558 ms (mostly loading the model) | |
| indexing with the model | 32.6 chunks/s | ≥ 12 chunks/s |

The criterion names a 6-core CPU and this one has 12; the 1-bit scan uses at most 8 threads, so on 6 cores the hybrid p50 should be about 240 to 300 ms (an estimate, not measured).

**AC 4:** `scale.sh … 1000 100000` was still building the 100 000-document brain when this was written; its fresh-machine line goes in the feature's PR into `main`.

## What went wrong

- **My first bench of the 1-bit path was a copy of the code.** It showed up only because the bench takes a lower `--exact-limit` to reach that path on a small brain; at 1 000 000 chunks it would have cost over a second.
- **The first threaded scan was slower than the single one** at 50 000 chunks, as above.
- **Command-line search is a second slower than in-process search** because it loads the 118 MB model for every question. Not changed here: the model is slice 7's question.
- **A second brain-ci template step was needed for the log:** the first CI run said nothing about what `pack` did; the template now prints its summary, which is how "embedded 1 chunk" above was read.

## Changes made afterwards

The covering index and the threshold for threads (above), the log line in the workflow, and the parts: the first version published one asset, which a million-chunk index would have broken.
