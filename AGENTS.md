# AGENTS.md

Supercharge is a skill bundle for omp and Claude Code. The skills are `skills/*/SKILL.md` (`skills/supercharge/` also holds `issue-template.md`, the default issue format, which a project's own `docs/issue-template.md` replaces; `skills/brain/` holds `brain.json`, the pins of its binary); `brain/` is the Rust source of that binary, the one piece of code, released from this repo; `README.md` is the user-facing doc; `runs/` holds write-ups of real runs. A feature is one GitHub issue whose slice checklist is its progress record; features planned earlier with sub-issues still build (see the Issues rule in PRODUCT).

- [docs/PRODUCT.md](docs/PRODUCT.md): what the bundle does, its core flows and rules
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how it's built and distributed
- `docs/features/`: feature docs; `docs/adr/`: decisions

## Checks

`python3 scripts/check.py` checks each skill's frontmatter (a description's first sentence is at most 57 characters, the whole at most 1024), `skills/brain/brain.json` and the relative links in the Markdown; `python3 -m unittest scripts/test_check.py` tests the `brain.json` rules. CI runs both.

For `brain/` (CI runs these when `brain/` changes; the first build needs `curl` and the network once, to fetch the model into `brain/assets/`, and needs a C and C++ compiler): `cd brain && cargo fmt --check && cargo clippy --release --all-targets -- -D warnings && cargo test --release && cargo run --release -- eval`. `SUPERCHARGE_CACHE_DIR` moves the index cache, which tests use. `brain/scripts/smoke.sh <binary>` runs a built binary alone with an empty environment.

## Conventions

- Every line in a skill should change behaviour. Keep additions short and in the skills' existing voice: guide how the agent works, not how it does the work.
- The README describes the skills' behaviour in detail (the `approve` table, Usage): a skill change that alters what the human does or sees updates it too.
- No smoke run and no end-to-end test for skill changes (owner's decision, 2026-10-29): skill text has no runtime. Verification is the independent review of the diff against the acceptance criteria; the next real run confirms the behaviour.
- A real run gets a write-up in `runs/YYYY-MM-DD-slug.md`: setup, result, what went wrong, changes made afterwards.
- The `brain/` crate is real code: test first, and a change to what's indexed or how it ranks adds cases to `brain/eval/questions/` and puts the before and after of `brain eval` in the PR. `brain eval --update-baseline` rewrites `brain/eval/baseline.json`; only do that for a change you can explain. Eval corpus files pass `scripts/check.py` like any Markdown, so no broken relative links.
- To release the binary, push a tag `brain-vX.Y.Z`: `.github/workflows/brain-release.yml` builds the four platforms and publishes a pre-release. Then pin its URLs, sizes and sha256 in `skills/brain/brain.json`, and update the size in the `brain` skill's description and the README.

## Supercharge

```yaml
risk: medium                 # low | medium | high
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```
