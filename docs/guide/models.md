# Models

The [README](../../README.md#model-stacks) has six ready-made stacks. This page says what each job needs, what the runs showed and what the models cost.

## The jobs

There are three, and they need different things:

- **Planning** (`supercharge`, `living-docs`): judgement about the business, such as which questions to ask, safe defaults, and checking facts. It uses few tokens, so use the strongest model you have. `living-docs` always runs in the session, on this model, never in a builder subagent.
- **Building** (`build-slice`): good technical judgement. It uses most of the tokens, so this is where a cheaper model pays off. A slice or ticket started from its issue is built in a builder subagent too, so the session's model doesn't build.
- **Reviewing** (omp's `reviewer` agent, or a fresh subagent in Claude Code): an independent check. Always a different model from the builder, ideally from a different company. It's cheap.

## Roles in omp

In omp the agent picks the agent (`task`, `reviewer`, `sonic`), never the model: omp's model roles set each agent's model. The session (planning and driving) runs on `default`, builders (omp's `task` agent) on `task`, the reviewer on `slow`, and small jobs (`sonic` searches, session titles) on `smol`. Set them in `~/.omp/agent/config.yml` and check with `omp config get modelRoles`.

Without `task`, builders run on `default`; without `smol`, so do the small jobs, at the planning model's price. Keep `task` and `slow` different. On a subscription, choose by quota rather than price: a cheaper builder still saves most of it, because building uses most of the tokens.

Model IDs take the provider prefix omp needs: `anthropic/`, `openai-codex/` (a ChatGPT subscription; use `openai/` with an API key), `google/`, and `openrouter/` for models reached through OpenRouter. The stacks use the IDs in omp's model list on 2026-11-24.

## In Claude Code

There are no model roles: the session runs on the model you pick (`/model`), builders on `sonnet`, and the reviewer on a different Claude model (`opus` for a `sonnet` builder, otherwise `sonnet`). So a ticket is built on `sonnet` and reviewed by `opus`, whatever model the session runs on. That is the Anthropic stack, with its roles fixed to those aliases (`sonnet`, `opus`, `haiku`) rather than the exact versions the README names. The cross-company reviewer is omp-only; if no such reviewer can be started, `build-slice` stops and says so.

The `brain` skill's searches run in a small-model subagent: omp's `sonic` on `smol` (`scout` is read-only and can't run the binary), and in Claude Code a fresh subagent on `haiku`.

## Prices

Per million tokens, input / output, from omp's model list on 2026-11-24. Prices change; check yours.

| Model | In / out |
|---|---|
| Claude Opus 5.5 | $4 / $20 |
| Claude Sonnet 5.5 | $2 / $10 |
| Claude Haiku 5.5 | $0.10 / $0.50 |
| GPT-6.2 Astra | $8 / $40 |
| GPT-6.2 Sol | $2 / $10 |
| GPT-6 Luna | $0.10 / $0.50 |
| Gemini 4 Carbon | $2 / $10, then $4 / $20 from 30 November |
| Gemini 4 Neon | $3 / $15 |
| Gemini 4 Hydrogen | $1.50 / $7.50 |
| Muse Wonder | $4 / $20 |
| Muse Spark 1.3 | $1.25 / $4.25 |
| Muse Ember | $0.10 / $0.50 |
| Kimi K3.1 Pro | $0.24 / $8.40 |
| GLM-5.3 | $0.07 / $1.40 |
| Qwen4-Max | $1.43 / $4.28 |
| DeepSeek V4.1 Flash | $0.14 / $0.42 |

Claude Fable 5.2 (`anthropic/claude-fable-5-2`, $8 / $40) is a stronger planner than Opus if you want one for `default`. Left out on purpose: Mythos 5.2 (vetted organisations only), and Muse Wonder Full (needs a subscription; Muse Wonder is API-only).

## What the runs showed

The write-ups in [`runs/`](../../runs/) tested earlier versions of these models. The stacks use the latest ones, and none of the six exact combinations has had a run of its own. The frontier models qualify for every job without one: they're at least as strong as the models that passed.

- **Opus plans, GLM 5.3 builds, GPT-6.1 Sol or Kimi K3 reviews** worked, for about $6 for a small MVP ([run 6](../../runs/2026-10-15-receipt-scanner-opus-plans-glm-builds.md)). On a real 5-day project with Opus building, building was 70% of the cost ([the project](../../runs/2026-10-22-first-real-project.md)).
- **Cheap models plan badly:** they fill business gaps with silent defaults and state unchecked facts. GLM 5.3 and Kimi K3 failed as planners. Kimi K2.7 Code couldn't drive the bundle at all ([run 4](../../runs/2026-10-14-receipt-scanner-cheap-models.md)).
- **Sonnet 5.5 builds** in the owner's daily use (2026-11-12), without a write-up.
- **The smallest tiers** (Haiku, GPT Luna, Muse Ember, the Flash models) are untested for building and probably too small for it; they suit `smol`.

## When to swap in GLM-5.4

GLM-5.4 (`zai/glm-5.4`) is on Z.ai's Coding Plan only, not yet on its API or OpenRouter, so the Alternatives stack uses GLM-5.3. With the Coding Plan, set `task` to `zai/glm-5.4`. It hasn't had a run yet.
