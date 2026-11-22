# AGENTS.md

Selkie is a Kotlin service with one Postgres database and one worker. Read `docs/PRODUCT.md` for what it does and `docs/ARCHITECTURE.md` for how it is built.

## Supercharge

```yaml
risk: medium                 # low | medium | high
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```
