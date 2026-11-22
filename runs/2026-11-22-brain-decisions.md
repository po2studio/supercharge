# Decisions in the brain: a real run of the binary

Slice 3 of [feature 0002](../docs/features/0002-second-brain.md). The binary was run on throwaway folders, never on a real project or `~/.cache/supercharge/brain/`. The skill text was not run (AGENTS.md: skill text has no runtime); the git steps it prescribes were run by hand.

## Setup

- `repo/`: a git repo with `origin` set to `git@github.com:acme/courier-app.git`, an `AGENTS.md` whose block lists `../brain` with `update: true`, `docs/BUSINESS.md`, and four ADRs: 0001 `superseded by 0003` (as a link), 0002 `accepted; amended by 0004` (as a link), 0003 and 0004 `accepted`.
- `brain/`: a git clone of an empty bare repo. `brain init` created `BRAIN.md`, `decisions/`, `notes/` and `projects/`.
- Three decision files written as the skill's Recording step says: a 2023 product decision (`status: superseded`, `superseded-by`), the 2026 one that replaced it (`supersedes: [2023-11-02-single-phone]`), and a process decision of another project (`project: other-app`).

## What it did

- `brain search --terms "phone number"` returned only the 2026 decision. With `--all` the 2023 one came second, `status: superseded`, `superseded_by: decisions/2026-01-10-sms-fallback.md`. `--status superseded` alone returned it too.
- The ADRs behaved the same: `invoices batched` returned 0003; with `--all`, 0001 followed marked as superseded by `docs/adr/0003-daily-invoices.md`. `--about "which currencies are amounts stored in" -k 1` returned 0002 and, right after it, its amendment 0004.
- `--project courier-app` found the project's own ADRs (named from `origin`) and not the other project's decision; `--project other-app` found only that one. `--kind process --since 2026-01-01` found the refund decision. `docs/BUSINESS.md` was found in place.
- The skill's write steps worked in a git source: `git pull --rebase`, `git add` of the files written, commit, push, to a bare remote.
- Nothing was written into `repo/` by any `brain` command but the cache under `SUPERCHARGE_CACHE_DIR`.

## What went wrong

- `git pull --rebase` on a clone of an empty remote fails ("no such ref was fetched"). It isn't a conflict, so the skill now says a remote with nothing yet has nothing to pull.
- A first trust order that put every accepted decision before every document (and then moved decisions up two places) cost 0.03 MRR on the eval, because decisions that merely resemble a question ranked above the file that answers it. The binary now moves an accepted decision up one place, which costs about nothing; notes join the order with slice 4.

## Changes afterwards

The skill text above; the trust order constant `DECISION_LIFT`.
