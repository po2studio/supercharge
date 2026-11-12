# 0003: The brain is a folder; raw files are the truth

Status: accepted

## Context

The brain holds a company's documents and the decisions agents and humans make across projects, and agents keep it up to date. It must not fill with garbage, go stale, or need anything beyond `git` and `gh` ([feature 0002](../features/0002-second-brain.md)).

## Decision

A brain is a folder, one per human or team, spanning projects; usually a git repo on GitHub, found through `~/.config/supercharge/brain` or a `brain:` override in a project's `AGENTS.md`. Everything in it is raw, never edited by an agent, except what agents own: `BRAIN.md`, `decisions/`, `notes/` and `projects/`. Notes and the index are derived from raw files and can be rebuilt. A project's own docs (ADRs, `BUSINESS.md`) stay in the project and are indexed where they are. Writes are commits pushed to the brain's default branch, gated by lint and `brain check`, not by a second human approval.

## Alternatives

- The GitHub wiki: ruled out by the owner.
- A database: not a folder people can drop files into, and not diffable.
- One brain per project: decisions and company context span projects.
- Moving ADRs into the brain: decided on 2026-11-03, replaced on 2026-11-12 so an ADR lands in the same PR as the code it governs.

## Consequences

Anything that can hold a folder can hold a brain, including a synced docs folder, which is why nothing per machine is written into it. No access control: sensitive content is out of scope. Two machines shipping at once can conflict on push; the writer rebases first and stops on a conflict.
