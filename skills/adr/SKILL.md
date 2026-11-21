---
name: adr
description: Record a hard-to-reverse decision as an ADR in docs/adr/. Use for architecture or technical decisions a future agent would likely undo. Not for choices the code already makes obvious.
---

# adr

Record why something is the way it is, so nobody relitigates it. Only record the decision; don't implement it.

`docs/` here means the folder set by `docs` in `AGENTS.md` (default `docs`).

- One decision per file: `docs/adr/NNNN-short-slug.md`, numbered after the highest existing number.
- Cover: context, the decision, alternatives considered, consequences, status (`proposed`, `accepted`, `superseded by NNNN`, or `amended by NNNN` when only part of it changed).
- Keep it short. A few paragraphs at most.
- An accepted ADR's body is never edited; only its status line may change (e.g. "amended by 0012"). To change a decision, write a new ADR and mark the old one superseded. Check existing ADRs for one your decision contradicts.
- Decisions that cross the codebase boundary (architecture direction, third parties, cost, data leaving the system, licenses) stay `proposed` until the human accepts them: whoever writes one stops and asks, and on `go` sets it `accepted`.
- If the decision changes how things are built, update `docs/ARCHITECTURE.md` to match.
