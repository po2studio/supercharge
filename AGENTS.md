# AGENTS.md

Supercharge is a skill bundle for omp. The skills are `skills/*/SKILL.md`; `README.md` is the user-facing doc; `runs/` holds write-ups of real runs.

- [docs/PRODUCT.md](docs/PRODUCT.md): what the bundle does, its core flows and rules
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how it's built and distributed
- `docs/features/`: feature docs; `docs/adr/`: decisions

## Checks

`python3 scripts/check.py` checks each skill's frontmatter and the relative links in the Markdown. CI runs the same.

## Conventions

- Every line in a skill should change behaviour. Keep additions short and in the skills' existing voice: guide how the agent works, not how it does the work.
- The README describes the skills' behaviour in detail (the `approve` table, Usage): a skill change that alters what the human does or sees updates it too.
- No smoke run and no end-to-end test for skill changes (owner's decision, 2026-10-29): skill text has no runtime. Verification is the independent review of the diff against the acceptance criteria; the next real run confirms the behaviour.
- A real run gets a write-up in `runs/YYYY-MM-DD-slug.md`: setup, result, what went wrong, changes made afterwards.

## Supercharge

```yaml
risk: medium                 # low | medium | high
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```
