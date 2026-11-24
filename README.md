<div align="center">

# Supercharge

**One line in, tested code merged.** A skill bundle for [omp](https://github.com/can1357/oh-my-pi) and [Claude Code](https://code.claude.com).

[![check](https://github.com/po2studio/supercharge/actions/workflows/check.yml/badge.svg)](https://github.com/po2studio/supercharge/actions/workflows/check.yml)
![omp](https://img.shields.io/badge/omp-ready-6f42c1)
![Claude Code](https://img.shields.io/badge/Claude%20Code-ready-d97757)
[![MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

</div>

```text
you     /supercharge the login page fails on an empty email

agent   One ticket. One question (default in brackets):
        inline error, or disable the button? [inline error]
        Plan: reject an empty email on submit, show "Enter your email", add a test.

you     go

agent   Built tests first, run in the browser, reviewed by a different model.
        PR #42 into main, CI green. Reply go to merge.

you     go
```

## What it is

Five skills that take a request from one line to merged, tested code. You answer one batch of questions, check the plan, and say `go` at the end. The agent writes the tests, builds, runs it for real, has a different model review it, and keeps the docs true.

It guides how the agent works, not how it does the work, so it assumes a strong model. Every line should change behaviour; you can read all of it in one sitting.

## Install

```sh
npx skills add po2studio/supercharge -g -a pi            # omp
npx skills add po2studio/supercharge -g -a claude-code   # Claude Code
```

That's the whole install: no plugin, settings or hooks, and nothing beyond `git` and `gh`. Leave out `-g` for one project only; update with `npx skills update -g`. Claude Code needs v2.1.283 or later. The optional [brain](#the-brain) downloads one binary on first use; skip it at install and nothing else changes. Pick your [model stack](#model-stacks) next.

## Use

Start a fresh session for each request, then run `/supercharge` with an idea, a bug, a change or a feature. The first time in a project it sets up `AGENTS.md` and the docs and asks you to check them. After that it takes the lightest safe route:

- **A small fix** is built and reviewed straight away.
- **A ticket** gets one issue and is built the same way.
- **A feature** gets a spec and one issue with its slices, built from a fresh session with `/drive-feature <issue URL>`.

Every stop tells you what to check and what to reply. You never merge anything yourself: `go` does it. See the [usage guide](docs/guide/usage.md).

## How it works

```mermaid
flowchart LR
    A["Your request"] --> B["Plan<br>questions, your check"]
    B --> C["Build<br>tests first, run for real,<br>reviewed by another model"]
    C --> D["PR<br>CI green"]
    D --> E["Your go"]
    E --> F["main"]
```

Each slice is its own small PR with acceptance tests, a decision ledger and updated docs. Features are built in parallel, one subagent per slice. Rigour scales with `risk`, and [`approve`](docs/guide/config.md) sets where you stop.

## Model stacks

The jobs are planning (`default`), building (`task`), reviewing (`slow`) and small jobs (`smol`). Pick a stack, then paste its roles into `~/.omp/agent/config.yml`. The default, Mixed:

```yaml
modelRoles:
  default: anthropic/claude-opus-5-5      # plans and drives
  task: anthropic/claude-sonnet-5-5       # builds
  slow: openai-codex/gpt-6.2-sol          # reviews
  smol: anthropic/claude-haiku-5-5        # searches, titles
```

| Stack | `default` | `task` | `slow` | `smol` |
|---|---|---|---|---|
| **Mixed** (default) | `anthropic/claude-opus-5-5` | `anthropic/claude-sonnet-5-5` | `openai-codex/gpt-6.2-sol` | `anthropic/claude-haiku-5-5` |
| **Anthropic** | `anthropic/claude-opus-5-5` | `anthropic/claude-sonnet-5-5` | `anthropic/claude-opus-5-5` | `anthropic/claude-haiku-5-5` |
| **OpenAI** | `openai-codex/gpt-6.2-astra` | `openai-codex/gpt-6.2-sol` | `openai-codex/gpt-6.2-astra` | `openai-codex/gpt-6-luna` |
| **Google** | `google/gemini-4-carbon` | `google/gemini-4-neon` | `google/gemini-4-carbon` | `google/gemini-4-hydrogen` |
| **Meta** | `openrouter/meta/muse-wonder` | `openrouter/meta/muse-spark-1.3` | `openrouter/meta/muse-wonder` | `openrouter/meta/muse-ember` |
| **Alternatives**, on OpenRouter | `openrouter/moonshotai/kimi-k3.1-pro` | `openrouter/z-ai/glm-5.3` | `openrouter/qwen/qwen4-max` | `openrouter/deepseek/deepseek-v4.1-flash` |

`openai-codex/` is the ChatGPT subscription; with an API key use `openai/`. Check with `omp config get modelRoles`. Claude Code has no model roles: it runs the Anthropic stack, with builders on `sonnet` and the reviewer on `opus`. Prices, evidence and when to swap in GLM-5.4: [models guide](docs/guide/models.md).

## The brain

Optional. `/brain <question>` answers from your project's docs and any folders you list, citing the files and dates it found the answer in. Everything is indexed and searched on your machine; no API key. It downloads one binary (176 MB) the first time, after you say `go`. With `update: true` on a source, agents also keep those documents true after each merge. See the [brain guide](docs/guide/brain.md).

## More docs

- [Usage](docs/guide/usage.md): daily use, building a plan, the issue format
- [Config](docs/guide/config.md): `risk`, `approve`, `ci_budget`, `docs`
- [Models](docs/guide/models.md): the jobs, the evidence from [`runs/`](runs/), prices
- [Brain](docs/guide/brain.md): setup, sources, the update step, the CLI

## Support

Using Supercharge at your company? We offer commercial support, extra tooling and managed spending for teams. Email [info@po2studio.com](mailto:info@po2studio.com).

## License

MIT. See [LICENSE](LICENSE). Made by [Saankofi](https://saankofi.com) and [PO2 Academy](https://www.po2academy.com).
