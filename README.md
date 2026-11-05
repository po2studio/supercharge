# Supercharge

A small skill bundle for building software with strong LLM agents on [oh-my-pi (omp)](https://github.com/can1357/oh-my-pi).

Made by [Saankofi](https://saankofi.com) and [PO2 Academy](https://www.po2academy.com).

## Philosophy

**Guide how the agent works, not how it does the work.** The skills set direction, boundaries and a definition of done, and leave the technique to the model. The bundle therefore assumes a strong model; weaker models need a more prescriptive process than this one.

### Less is more

The bundle is small on purpose: five skills, a few short docs, one config block. Every line in a skill should change behaviour; a line that changes nothing when deleted gets deleted. Why:

- **Strong models already know the craft.** They can write a test, shape a module and debug a failure. Spelling that out adds nothing, and a fixed recipe overrides judgement that is often better than the recipe.
- **Every line costs attention.** Skill text is loaded on every run and competes with the task. The more a skill says, the less weight each line gets, so the lines that matter get lost among the ones that don't.
- **Process ages; direction doesn't.** Step-by-step instructions written around one model's weaknesses become dead weight, or get in the way, with the next model. Goals, boundaries and a definition of done stay true.
- **You can read all of it.** The whole bundle reads in one sitting, so you know what the agent was told and can see why it did what it did.

The same goes for what the bundle produces. It takes the lightest route that's safe (a one-line fix is just done; only bigger work gets a feature doc), keeps a few short docs rather than a wiki, deletes acceptance tests once they pass, and keeps the test suite fast. Less to maintain, less to drift out of date, less for the next session to read.

### Good engineering by default

Few instructions doesn't mean low standards. Every slice meets the bar a careful senior engineer sets for their own work:

- **Tests first.** One acceptance test per criterion, seen failing, then the inside test-driven until it passes. What stays is the unit tests and one happy-path end-to-end test per core flow, through what users use.
- **Run it for real.** Green tests aren't done: the agent runs the thing end to end, in the browser for a UI.
- **One builder, one independent reviewer.** A different model reviews the diff against the acceptance criteria before the PR, so the builder's blind spots aren't the reviewer's.
- **Small changes, green CI.** Each slice is its own reviewable change, merged only with CI passing, and CI kept within a time budget.
- **Build for what exists.** No compatibility layers for data, users or releases that don't exist yet. Every external service sits behind an interface with a fake, so everything runs offline.
- **Rigour scales with risk.** More edge-case tests and a stricter review where a mistake costs more, not everywhere.
- **Decisions written down.** A decision ledger in each PR, ADRs for what's hard to reverse, and the docs updated as part of done.

### Principles

These run through every skill:

1. **Docs are the current truth.** A few short docs say why and what; code and tests show how. Updating the docs is part of done.
2. **Ask about the business, decide the technical.** Business rules (what gets recorded, money, who can do what, what users see) are never guessed silently. Technical choices are the agent's, recorded as it goes. Architecture and commitments outside the codebase (third parties, cost, data leaving the system, licenses) are proposed as ADRs for the human to accept. Questions come in one batch, each with a default, so `go` answers them all.
3. **What's written is binding.** Anything the human wrote or approved is a requirement. Fill the gaps; flag conflicts rather than silently "improving" things.
4. **Vertical slices, sized by what they deliver.** Each slice works end to end and has acceptance criteria. The plan marks which slices can be built together, and those are built in parallel; rules they share are settled in the feature doc first, so each slice doesn't invent its own.
5. **Stop only where a human is needed.** Run every other step. At a stop, say exactly what to do and what happens after, so nothing has to be remembered.

## How it works

```mermaid
flowchart TB
    Y["You: requests"] --> P["plan-feature, one session per request<br>one batch of questions, feature doc, issues"]
    P --> O["ADE such as Orca: the repo's issue list"]
    O --> F2["session: feature 13<br>drive-feature"]
    O --> F1["session: feature 12<br>drive-feature"]
    O --> T["session: ticket 14<br>build-slice"]
    F2 --> F2s["its slices, the same way"] --> PR2["PR into main"]
    subgraph S ["build-slice: one subagent per slice, in parallel where the plan allows"]
        direction LR
        A1["slice 1"]
        A2["slice 2"]
        A3["slice 3"]
    end
    F1 --> A1 & A2 & A3
    A1 & A2 & A3 --> B["feature branch"]
    B --> L["living-docs<br>docs match what shipped"] --> PR1["PR into main"]
    T --> PR3["PR into main"]
    PR2 & PR1 & PR3 -->|"you: go"| M["main"]
```

Each slice: acceptance tests first, built, run for real, reviewed by a different model, PR with green CI. One session per feature or ticket, one subagent per slice, each in its own worktree; every issue starts with the command that builds it, so starting it from the ADE's issue list is the whole handoff. You answer one batch of questions at the start and reply `go` at the end; the docs on `main` carry the context between sessions. Without an ADE, it's several omp sessions side by side, started with the issue URLs.

## Install

```sh
npx skills add po2studio/supercharge -g -a pi
```

`pi` is omp's upstream, so this installs into `~/.agents/skills`, which omp reads. Leave out `-g` to install into one project's `.agents/skills` instead. Check with `omp read skill://plan-feature`; update with `npx skills update -g`.

## Config

A YAML block in a `## Supercharge` section of the project's `AGENTS.md`. `living-docs` asks for each key that isn't set yet, with its default, and writes the block:

```yaml
risk: medium                 # low | medium | high; a feature doc may override this, with a reason
approve: [merge]             # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```

Missing keys take the defaults.

### `risk`

How costly a mistake would be. It sets the rigour (edge-case tests, how strict the review is) and how readily the agent asks about business rules:

- **`low`:** asks only when it can't go on without an answer.
- **`medium`:** asks where a wrong guess would record wrong data or be hard to undo, for example how currencies are handled.
- **`high`:** asks about any business rule that isn't written down.

### `approve`

Everything runs on its own except the steps listed in `approve`. At each one the agent stops and says what to check, what to reply, and what happens next.

| Value | Stops | You | Then |
|---|---|---|---|
| `spec` | After the feature doc's deliverables and acceptance criteria, before slicing | Check *what* gets built. Reply `go` or say what to change | It plans the slices |
| `plan` | After the slices are planned, before building | Check what gets built and how it's cut. Reply `go` or say what to change | It builds every slice |
| `slice` | After each slice, or each batch built together, is built, reviewed and merged into the feature branch | Try it: the stop says how to run it and what to check. Reply `go` or report a problem | It fixes the problem, or starts the next slice |
| `merge` | After every slice is done and the docs are refreshed on the feature branch | Review the one PR (or branch) into `main` and its checklist of human-only checks. Reply `go` | It checks CI, merges into `main` and cleans up the branches. In an ADE such as Orca, it deletes only the remote branches and leaves the worktrees and their local branches for you to remove there. The feature is done |

- **`[merge]` (default):** you only see the finished feature.
- **`[plan, merge]`:** also catches wrong guesses about the product before any code is written. Use it with models that don't ask questions (see [run 4](runs/2026-10-14-receipt-scanner-cheap-models.md)).
- **`[slice, merge]`:** test as you go; use it when you want to see each step working on a real device or real data. To see every slice on its own, add "one at a time" to the command.
- **`[]`:** fully hands-off; the agent merges into `main` as soon as CI passes.

You never merge anything yourself: approving is replying `go` in the session.

Small changes skip the feature machinery: a one-line fix is just done, and a single-slice ticket gets its own PR into `main`, which waits for your `go` if `merge` is set.

### `ci_budget`

How long CI may take. When it takes longer, `build-slice` opens an issue to speed it up, if there isn't one open already.

### `docs`

Where the docs, ADRs and feature docs live; every `docs/` path in the skills and in this README means this folder. Leave it as `docs` unless `docs/` already has another job, such as a published docs site; then use something like `docs/supercharge`. `living-docs` checks for this when it asks for `docs`, and offers that as the default. `AGENTS.md` always stays at the repo root. To move the docs later, change `docs` and run `/living-docs`; it moves them and fixes the links.

## Usage

### Setup (once per project)

1. **Install** the skills and **set the model roles** (below).
2. In omp, in the project's directory, run `/living-docs`, adding what you know: what it's for, who uses it, the stack. It reads any existing code and asks one batch of questions, including every [config](#config) key that isn't set yet, each with its default; `go` accepts them all.
3. **Read the docs it writes** and fix anything wrong; what you write there is binding.

### Daily use

1. **Start a new session for each request.** The docs carry the context, so a fresh session loses nothing, and it's cheaper and sharper than one that has run for days.
2. **Ask for what you want**, from one line to a full spec. `plan-feature` usually loads by itself; if not, run `/plan-feature <request>`. Small things are done directly; bigger ones get a feature doc, plus a GitHub feature issue with the slices as its sub-issues. The doc reaches `main` on its own PR, which the agent merges without waiting for `merge`, so a session started from any of the issues can read it.
3. **Do what each stop says.** Answer its questions (`go` accepts the defaults), check the plan if `plan` is in `approve`, and approve the PR into `main` with `go`. The agent merges it, and the docs are already current. When a request is finished, start a new session for the next one.

Every issue the bundle creates starts with the command that builds it, so you can start work from Orca's issue list. Filter it with `is:open label:feature` for the features, `is:open label:ready-for-agent -label:slice` for everything you can start at the top level (features and tickets), or `label:feature:NNNN` for one feature and its slices. Orca doesn't show sub-issues or filter on them, so each slice also links to its feature issue.

To build an existing plan:
- **In one session:** start the feature issue from Orca, or run `/drive-feature <feature issue URL or docs/features/NNNN-*.md>`. It builds the slices the plan marks as buildable together in parallel, and the rest in dependency order, closes each slice's issue as it merges, and the final PR closes the feature issue. To check as you go, or to build fewer at once, add a limit in plain words. It stops after each batch on the feature branch, and `go` starts the next. For example:
  ```
  /drive-feature docs/features/0003-csv-export.md build 3 slices, then stop
  /drive-feature docs/features/0003-csv-export.md at most 2 at once
  /drive-feature docs/features/0003-csv-export.md one slice at a time, I'll test each on my phone
  /drive-feature docs/features/0003-csv-export.md build only slice 2
  ```
- **One slice or ticket per session** (for example, several sessions side by side): start its issue from Orca, or run `/build-slice <issue URL>`. With no feature branch, each gets its own PR into `main` that closes its issue, merged on your `go`. Pick slices the feature doc marks as buildable together. When the last slice of a feature merges, its stop tells you to start the feature issue from Orca: it skips the finished slices, does only the finish (docs, `living-docs`, status), and its PR closes the feature issue. After a big change made outside the bundle, run `/living-docs` to bring the docs back in line.

## Skills

| Skill | Does |
|---|---|
| `living-docs` | Creates or refreshes `AGENTS.md` and `docs/{BUSINESS,PRODUCT,ARCHITECTURE,DESIGN}.md` (BUSINESS and DESIGN are optional) |
| `adr` | Records a decision in `docs/adr/NNNN-*.md` |
| `plan-feature` | Triage, then `docs/features/NNNN-*.md` plus a feature issue with its slices as sub-issues |
| `build-slice` | Builds one slice to done: tests, end-to-end check, review, PR, green CI, docs updated |
| `drive-feature` | Runs `build-slice` across a feature's slices, then reports once |

Each skill works on its own; they coordinate only through the files above and their GitHub issues.
Execution uses omp's built-in features (`task`, the `reviewer` agent, `browser`, the `github` tool), plus `gh` for issues, which the `github` tool can't create. If one is missing, the skill uses an equivalent, or stops and says so.

## Models

There are three jobs, and they need different things:

- **Planning** (`living-docs`, `plan-feature`): judgement about the business, such as which questions to ask, safe defaults, and checking facts. It uses few tokens, so use the strongest model you have.
- **Building** (`build-slice`): good technical judgement. It uses most of the tokens, so this is where a cheaper model pays off.
- **Reviewing** (omp's `reviewer` agent): an independent check. Always a different model from the builder, ideally from a different company. It's cheap.

Each job runs on an omp model role: the session (planning and driving) on `default`, builders (omp's `task` agent) on `task`, and the reviewer on `slow`. omp's small jobs (`scout` searches, session titles) run on `smol`. Set them in `~/.omp/agent/config.yml` (check with `omp config get modelRoles`):

```yaml
modelRoles:
  default: anthropic/claude-opus-5.5     # plans and drives
  task: anthropic/claude-sonnet-5.5      # builds
  slow: openai-codex/gpt-6.1-sol         # reviews
  smol: anthropic/claude-haiku-5.5       # scouts, titles
```

Without `task`, builders run on `default`; without `smol`, so do the small jobs, at the planning model's price. Keep `task` and `slow` different. On a subscription, choose by quota rather than price: a cheaper builder still saves most of it, because building uses most of the tokens.

Tested in [`runs/`](runs/) (✓ good, ✗ not good enough, ? untested, – not worth testing for that job; prices per million tokens in/out on OpenRouter, October 2026). The frontier models (Fable, Opus, Astra, Sol) qualify for every job without a run of their own: they're at least as strong as the models that passed.

| Model | In / out | Plan | Build | Review |
|---|---|---|---|---|
| `anthropic/claude-fable-5.1` | $10 / $50 | ✓ | ✓ | ✓ |
| `anthropic/claude-opus-5.5` | $4 / $20 | ✓ | ✓ | ✓ |
| `anthropic/claude-sonnet-5.5` | $2 / $10 | ? | ? | ? |
| `openai/gpt-6-astra` | $10 / $50 | ✓ | ✓ | ✓ |
| `openai/gpt-6.1-sol` | $2 / $10 | ✓ | ✓ | ✓ |
| `z-ai/glm-5.3` | $0.07 / $1.40 | ✗ | ✓ | ? |
| `moonshotai/kimi-k3.1-pro` | $2 / $10 | ? | ? | ? |
| `moonshotai/kimi-k3` | $0.24 / $10.80 | ✗ | ? | ✓ |
| `deepseek/deepseek-v4-pro` | $0.96 / $1.91 | ? | ? | ? |
| `qwen/qwen3.8-max-0902` | $2 / $6 | ? | ? | ? |

Proven so far: **Opus plans, GLM 5.3 builds, GPT-6.1 Sol or Kimi K3 reviews**, for about $6 for a small MVP. On a real 5-day project with Opus building, building was 70% of the cost. Cheap models plan badly: they fill business gaps with silent defaults and state unchecked facts. Kimi K2.7 Code couldn't drive the bundle at all ([run 4](runs/2026-10-14-receipt-scanner-cheap-models.md)). The smallest tiers (Haiku, GPT Luna, the Flash models) are untested and probably too small to build; they suit `smol`.

Ideas borrowed from OpenSpec, BMAD, Matt Pocock's skills, Compound Engineering, Spec Kit, Superpowers and cc-sdd.

## Commercial support

Using Supercharge at your company? We offer commercial support, extra tooling and managed spending for teams. Email us at [info@po2studio.com](mailto:info@po2studio.com).

## License

MIT. See [LICENSE](LICENSE).
