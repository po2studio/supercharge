---
name: living-docs
description: Create or refresh a project's living docs — AGENTS.md plus docs/BUSINESS, PRODUCT, ARCHITECTURE and DESIGN. Use at the start of a project, when docs have drifted from the code, or when a fresh agent would lack the context to work well.
---

# living-docs

Give any agent arriving cold what it needs to work well, in as few words as possible.

Only write docs: don't plan or build. When you're done, say what comes next (usually `supercharge`).

## The docs

`AGENTS.md` stays at the repo root. The others live in the folder set by `docs` in the config block; the paths below assume the default, `docs/`. When `docs` isn't set yet, check whether `docs/` already has another job, such as a published docs site (mkdocs, Docusaurus, GitHub Pages); if it does, offer another folder such as `docs/supercharge` as the default. If the docs aren't where `docs` says, move them there and fix the links.

- `AGENTS.md`: the entry point. How to run, test, lint and format; conventions that aren't obvious from the code; the `## Supercharge` config block; links to the docs below. Keep it short.
- `docs/BUSINESS.md`, optional: the business this lives in, who uses it and why, the domain rules that matter. Leave it out if there's no business context; its absence is information too.
- `docs/PRODUCT.md`: what the product does **today**, its users, core flows, domain rules. What users see, not the numbers and mechanics behind it.
- `docs/ARCHITECTURE.md`: how it's built **today**: components, data, external services, key constraints. Say which external services have a fake and whether it runs offline on them (`build-slice` requires both); if not, say so.
- `docs/DESIGN.md`, optional: visual design (tokens, components, tone) when the product has a UI worth keeping consistent.

## How to work

- Docs describe current truth. Plans and history belong elsewhere (features, ADRs, git).
- Derive from the code and existing docs first. Where they disagree, believe the code and flag the gap.
- Ask the human only what you can't find out: business context and rules, intent, architecture direction, third parties. Decide technical details yourself. Ask in one batch, each question with a suggested default.
- Anything the human wrote or approved is binding. Refine the wording; don't change the meaning.
- Check accepted ADRs against the code too. If one no longer holds, supersede it with a new ADR, or flag it.
- Check facts about outside services (model IDs, API versions, prices) against the service itself, not memory or search results.
- Prefer short and accurate over complete. A doc nobody keeps up to date is worse than no doc.
- On a new project, keep the docs minimal: say what exists and what was asked for, not what's missing.

## Config block (in AGENTS.md)

If it's under the bundle's old name, `## Sprengikraftur`, rename the heading to `## Supercharge`. If the block or any key below is missing, ask for every missing key in your one batch of questions, each with its default and one line on what it does, and write the answers; `go` writes the defaults. Don't ask about keys already set.

```yaml
risk: medium                 # low | medium | high: how costly a mistake would be
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```
