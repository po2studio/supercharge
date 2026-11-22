# The brain made braindead to use, run for real (2026-11-22)

Slice 8 of feature 0002 ([ADR 0005](../docs/adr/0005-agents-keep-documents-true.md)): one `brain search "<question>"` command, one 495-word skill, documents that agents keep true. This write-up has the search fusion measured on the eval, the speed of the new "index first" search, and the update step run by an omp subagent following the skill, on a plain folder and on a git repo.

## Search fusion

The eval is now 80 plain questions (the notes and decision-file questions went with those features; the questions that named hand-picked search terms are asked as an agent asks them). The bar from the search contract: the fused search finds at least as many answers in the top 5 as the better of full-text or by-meaning alone, and the exact-term, Icelandic and paraphrase questions still pass.

Before the slice (the old binary, 88 questions, the agent supplied `--terms` and `--about` by hand): full-text 0.803 recall@5, by meaning 0.917, hybrid 0.900. After (80 plain questions, no flags):

| Mode | recall@5 | MRR |
|---|---|---|
| full-text alone (BM25, any word of the question) | 0.673 | 0.642 |
| by meaning alone | 0.906 | 0.868 |
| both (what `brain search` runs) | 0.910 | 0.874 |

By category, questions whose first right answer is in the top 5:

| Questions | n | full-text | meaning | both |
|---|---|---|---|---|
| exact terms | 10 | 10 | 10 | 10 |
| Icelandic | 13 | 8 | 11 | 11 |
| paraphrase | 12 | 1 | 12 | 12 |
| formats, flat text, duplicates, multi-document | 35 | 35 | 35 | 35 |
| project docs | 7 | 3 | 7 | 7 |
| ADR traps | 3 | 1 | 2 | 2 |

The bar was not met by the obvious fusion. Full-text over every word of a plain question adds distractors the by-meaning half already ranks below the answer, so it lowered the fused score. The ranked lists of both halves were dumped for all 80 questions and fusion rules tried offline:

| Fusion | recall@5 | MRR |
|---|---|---|
| RRF of question words, exact tokens and meaning, equal weights | 0.804 | 0.727 |
| words weighted 0.05 to 0.1 | 0.854 to 0.885 | |
| words dropped, exact tokens at weight 1 | 0.898 | 0.832 |
| meaning plus the best matches of the full-text rerank of its top 10, 20 or 30 | 0.785 to 0.879 | 0.755 to 0.825 |
| meaning plus full-text of code-like tokens, all matches | 0.873 to 0.892 | 0.83 to 0.867 |
| meaning plus full-text of code-like tokens that match under 1% of the chunks, top 5 | 0.904 | 0.872 |
| the same, top 3 (shipped) | 0.910 | 0.872 |
| the same, top 1 | 0.906 | 0.864 |

Shipped: by-meaning search on the question as given, plus the best 3 full-text matches of its code-like tokens (a digit, `-` `_` `/` `@` inside, an acronym, a quoted phrase), each token only if it matches under 1% of the index's chunks. A question with no such token is answered by meaning alone. The choice of 3 rests on this one eval of 80 questions, so it is a measured setting, not a derived one; the eval fails if the fused search ever finds fewer answers than the better half.

## Speed

- `brain eval` p50 5 to 8 ms, against 4 ms before; within the tolerance, no regression against the regenerated baseline.
- `brain bench --chunks 50000`: search p50 72 ms with both halves, 55 ms full-text, 71 ms by meaning, in process.
- Every search now indexes first. On a 5000-document synthetic brain (a git repo) with nothing changed, a search took 0.95 to 1.0 s on this machine at opt-level 1, and a search of a one-file source 0.9 s: the cost is loading the model, which the old binary paid too, and the walk is about 0.06 s of it (`index` reports 0.1 s). A search no longer reads and hashes files whose size and modification time are what the index recorded when it last read them (a `stamps` table; a file modified within 2 s of being read is never trusted), so the cost does not grow with the bytes of a big unchanged source. The per-term specificity check now runs on code-like tokens only, a few per question, not up to 40.

## The update step, run by a subagent (AC 6)

Setup: two throwaway projects, each with an `AGENTS.md` listing one `update: true` source; the binary built from this branch cached in a sandbox cache root (nothing under `~/.cache/supercharge/`), no download. Each source held `policies/travel.md` ("A trip needs written approval five days ahead.") and `handbook/employee-handbook.docx` saying the same. An omp `task` subagent was given only the skill's text and the answer at `go`: "a business trip now needs written approval 10 days ahead, not five".

| Source | Result |
|---|---|
| Plain folder (no git) | searched twice; edited `policies/travel.md` (one line changed, `## History` heading added at the end with `- 2026-11-22: Written approval for a trip now needed 10 days ahead, not five (human answer on the request, at go)`); reported the `.docx` as out of date and did not touch it (its modification time and text are unchanged); nothing else written |
| Git repo (`heidar/supercharge-brain-test`) | one search (it found both files); same edit and history line; the `.docx` reported; committed and pushed to `main` as `bce0b53` (one file, +5 −1) |

What went wrong: the skill said `git pull --rebase`, commit, push, which git refuses with an edited file; the agent committed first. The skill now says commit, `git pull --rebase`, push. The skill does not say what to cite as "where it was decided" for an answer given at `go`; both agents wrote the answer itself, which reads right, so it was left as is.

The test repo's old-model `BRAIN.md`, `decisions/` and `notes/` were removed in a commit of their own (`2a543b4`) before the runs.
