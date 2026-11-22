# 0004: Brain sources are set per project; writing is opt-in

Status: accepted

## Context

[ADR 0003](0003-the-brain-is-a-folder.md) made the brain one folder per human or team, found through a global path (`~/.config/supercharge/brain`) that `/brain` asked for and wrote on first use. On a machine that works for several organisations (an agency), one global brain mixes every client's documents and decisions. And a project's own docs should be searchable without any setup ([feature 0002](../features/0002-second-brain.md), owner, 2026-11-22).

## Decision

- **Installing the `brain` skill turns the brain on.** Not installing it keeps it off. There is no global path and no location question.
- **The project is always a source,** read in place and never written by the brain: its docs folder (the `docs` key), `README.md` and `AGENTS.md`.
- **Outside sources are listed per project** in the `AGENTS.md` config block, none by default:
  ```yaml
  brain:
    - path: ~/brains/client-a
      update: true
    - path: ~/Dropbox/ClientA/Policies
  ```
  `/brain` never writes the key, and no skill asks for it as missing.
- **`update` defaults to `false`.** At most one source per project sets `update: true`. Only there do agents write, and only their own files: `BRAIN.md`, `decisions/`, `notes/` and `projects/`. Raw files are never edited in any source. With no `update: true` source, the brain writes no documents anywhere.
- **One index per source** under `~/.cache/supercharge/index/<source-hash>/` (the project's docs count as one source), so projects that list the same source share its index. A search covers only the current project and the sources it lists.

The rest of 0003 stands: raw files are the truth; notes and the index are derived; writes to a git source are commits pushed to its default branch, gated by lint and `brain check`. "Per brain" in [ADR 0002](0002-local-embeddings-one-sqlite-index.md) reads "per source".

## Alternatives

- A global path with a per-project override (0003): one brain per machine, so organisations mix.
- An `org` setting: a second name for what the source list already says.
- Writing to every outside source: an agent would add files to folders their owners never offered.

## Consequences

Organisations stay apart because each project names its own sources. A committed `brain:` must use paths every teammate has (`~/…` or relative). Without an `update: true` source, decisions stay in the feature doc and the ADRs, as today, and are searched there; no notes are written and the ship step has nothing to update.
