# Notes that stay true, run for real (2026-11-22)

Slice 4 of feature 0002. The `brain` skill, with the `brain` 0.4.0 binary built from the slice, run in omp on the eval corpus: one answer that needed two files (the note flow), the same question again, an answer from one file, and a full `/brain refresh`. One run each, so the figures are one sample, not an average.

## Setup

- **Project and brain:** a throwaway git repo (`proj/`, one README, one doc, an `AGENTS.md` with `brain:` listing `../brain` as `update: true`) and a copy of `brain/eval/corpus/` as that source (126 files, its own git repo with a local bare remote, so the skill's pull, commit and push really ran). Index and conversion cache in a throwaway `SUPERCHARGE_CACHE_DIR`; nothing under `~/.config` or `~/.cache/supercharge/`.
- **The skill:** `skills/brain/` of the branch, loaded by omp from the project's `.claude/skills/`. The prompt said the binary was already at a given path, so first-run setup (the download) was skipped; the rest is the skill as written.
- **Models (omp's roles):** session `claude-opus-5-5`, search subagent (`sonic`) `claude-haiku-5-5`, builder subagents (`task`) `claude-sonnet-5-5`. Tokens and cost are what omp recorded for the session and its subagents; "all" counts cache reads, "new" is input, output and cache writes.
- **The corpus had five notes** (`brain/eval/corpus/notes/`): three fresh and two stale on purpose. For the refresh, a sixth note (a copy of one with the same sources) and a change to one raw file (`policies/remote-work.md`: three days a week became four) were added and committed first.

## Result

| Run | Time | Tokens, all / new | Cost |
|---|---|---|---|
| Question needing two files (budget and beta date of the mobile app), `update: true` source: answer and note | 63 s | 316k / 64k | $0.38 |
| Same question, source without `update`: answer only (the control) | 49 s | 185k / 26k | $0.16 |
| **What the note cost** | **+14 s** | **+132k / +38k** | **+$0.22** |
| Same question again, with the note | 29 s | 156k / 16k | $0.11 |
| Question answered from one file, `update: true` source | 36 s | 194k / 16k | $0.13 |
| `/brain refresh`, 7 notes (3 stale, 1 duplicate) | 85 s | 303k / 33k | $0.29 |

- **A note:** the builder subagent that wrote it ran 10 s on 47k tokens, $0.08 of the $0.22 (the rest is the session proposing it and relaying it). With the note in place the same question took 29 s instead of 49, and 156k tokens instead of 185k: it pays for itself after about five repeats. The search ranked the note first for the question (score 0.906).
- **An answer from one file** produced no note, and the session said why. The control run, with no `update: true` source, produced none and said no source accepts notes.
- **`/brain refresh`:** 7 notes linted in one builder subagent (41 s, 158k tokens, $0.11) and a session turn to start it and relay the report. Outcomes: `hotel-cap-2023` deleted (its only source, a 2023 policy, is gone); `sick-leave-summary` updated (its source changed); `working-from-home` updated (the raw file changed from three days to four, and the note now says the Icelandic page still says three); `trip-limits` merged into its duplicate, which was kept; two notes kept. Nothing was written that wasn't there before, the raw files were untouched, and after `index` `brain stats` shows 5 notes, none stale, none pointing at nothing, none duplicated. Two commits were pushed to the brain.
- **Cost grows with what is stale, not with the brain:** only the 3 stale notes and the duplicate were rewritten; the rest were read against the decisions and kept.

## What went wrong

- **The text search output did not show that a note was stale** (the reviewer found it; the run's own stale notes were reached through `stats`). The skill's search subagent reads the text output, so it could have answered from a stale note. A stale hit now prints `STALE note`, and the skill says so.
- **`brain init` creates `BRAIN.md`, and nothing commits it.** The skill adds only the files it wrote, so `BRAIN.md` stayed untracked in the brain repo through every write. Not changed in this slice (it is slice 3's step); reported.
- **A hit from an outside source was called "a project file" by the search subagent** in the single-file run. The text output labels it `[../brain]`; the answer was right and cited the right path.

## Changes made afterwards

- `STALE note` in the text of a search, the skill line for it, the per-project duplicate grouping and the streaming hash of a cited file the index doesn't hold, all from the review of the diff.
