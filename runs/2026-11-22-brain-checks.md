# Checks and the brain repo's CI, run for real (2026-11-22)

Slice 5 of feature 0002. The released `brain` 0.5.0 binary (`brain-v0.5.0`, the linux x64 asset, checked against the sha256 the skill pins) and the skill's CI template, run against a private throwaway GitHub repo. The skill text (ship step, build-slice rules) has no runtime and was checked by review; this run covers what has one: the binary and the workflow.

## Setup

- **Brain:** a throwaway GitHub repo as the `update: true` source of a throwaway project, with one raw file (`policies/travel.md`). Index and cache in a throwaway `SUPERCHARGE_CACHE_DIR`; nothing under `~/.config` or `~/.cache/supercharge/`.
- **The skill's steps, done by hand in the order the skill gives:** `brain init`, commit and push what it created, fill `skills/brain/brain-ci.yml` from `brain.json` (tag, repo, linux-x64 asset and sha256) into `.github/workflows/brain.yml`, commit, `git pull --rebase`, push.

## Result

| Push | Run (GitHub-hosted, ubuntu) | Issues |
|---|---|---|
| workflow added | passed, 19 s | none |
| a valid note (made from `policies/travel.md`, with its hash) and a decision | passed, 16 s | none |
| `policies/travel.md` changed: the note is stale | passed, 12 s, `warning: notes/business-trips.md: stale: policies/travel.md changed` | one opened, listing the note |
| another raw file pushed while still stale | passed, 10 s | still one (its body and title rewritten) |
| the note refreshed with the new hash | passed, 18 s | the issue closed itself |
| a decision with no frontmatter | **failed**, `FAIL decisions/2026-11-22-bad.md: frontmatter is missing` | none (the check, not the issue, reports it) |
| the file removed, the raw file changed again, then a second push within seconds | both passed | one issue, not two |

- **The init bug, shown:** after `brain init` in the brain, `git status` listed `?? BRAIN.md`, the file the skill's old write step never added. With the step as now written (commit what `init` created, before anything else) the tree was clean.
- **On the binary, locally:** a project and a brain, both git repos: `check` of a valid brain (exit 0), `--changed` with a path (lists the decision), `--changed <reconciled>..HEAD` after two commits made outside any skill (lists the decision and both paths), an accepted decision's body edited and committed (`FAIL ... accepted decision edited: the body changed since a45e9e0`, exit 1), the edit reverted (passes), a note with a broken link and a bad hash (two failures plus the stale warning), and a `touches` that matches no file (a warning naming the globs).

## What went wrong

- **My first release tag was cut before the second review pass and was moved once** (before any release existed): a fix for a backslash in a link destination came in after it. The release that is pinned is built from the final commit of the branch.
- **`git log --follow` took a new decision for a copy of a similar old one,** and made a proposed decision fail the immutability check. The first review had asked for rename handling; `--follow` was the obvious way and it is wrong for this. The check now follows a rename only when git sees it between files of one commit (`git diff-tree -M` on the oldest commit of the path).
- **The first listing of open issues after a push was stale** (GitHub's issue list lags a few seconds behind a close). The workflow closes any issue beyond the first on its next run, so a lag cannot leave two open for long.

## Changes made afterwards

- Merge-only changes and non-ASCII names in a commit range, an empty range, link destinations with spaces or parentheses, a note citing another project's docs, renames in a decision's history, a backslash before a non-punctuation character, and in the skill the pull order for files already written and a reindex before the ship step records decisions: all from the two review passes, each with a test.
