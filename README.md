# Supercharge

A small skill bundle for building software with strong LLM agents, in [oh-my-pi (omp)](https://github.com/can1357/oh-my-pi) or [Claude Code](https://code.claude.com).

Made by [Saankofi](https://saankofi.com) and [PO2 Academy](https://www.po2academy.com).

## Philosophy

**Guide how the agent works, not how it does the work.** The skills set direction, boundaries and a definition of done, and leave the technique to the model. The bundle therefore assumes a strong model; weaker models need a more prescriptive process than this one.

### Less is more

The bundle is small on purpose: five skills, a few short docs, one config block, and an optional sixth skill, `brain`, with one downloaded binary of its own. Every line in a skill should change behaviour; a line that changes nothing when deleted gets deleted. Why:

- **Strong models already know the craft.** They can write a test, shape a module and debug a failure. Spelling that out adds nothing, and a fixed recipe overrides judgement that is often better than the recipe.
- **Every line costs attention.** Skill text is loaded on every run and competes with the task. The more a skill says, the less weight each line gets, so the lines that matter get lost among the ones that don't.
- **Process ages; direction doesn't.** Step-by-step instructions written around one model's weaknesses become dead weight, or get in the way, with the next model. Goals, boundaries and a definition of done stay true.
- **You can read all of it.** The skills, the docs and the config read in one sitting, so you know what the agent was told and can see why it did what it did. The one piece of code the agent runs, the optional `brain` binary, lives in `brain/`; nothing else depends on it.

The same goes for what the bundle produces. It takes the lightest route that's safe (a one-line fix is just done; only bigger work gets a feature doc), keeps a few short docs rather than a wiki, deletes acceptance tests once they pass, and keeps the test suite fast. Less to maintain, less to drift out of date, less for the next session to read.

### Good engineering by default

Few instructions doesn't mean low standards. Every slice meets the bar a careful senior engineer sets for their own work:

- **Tests first.** One acceptance test per criterion, seen failing, then the inside test-driven until it passes. What stays is the unit tests and one happy-path end-to-end test per core flow, through what users use; when there's no harness yet, the plan names setting it up in the first slice.
- **Run it for real.** Green tests aren't done: the agent runs the thing end to end, in the browser for a UI.
- **One builder, one independent reviewer.** A different model reviews the diff against the acceptance criteria before the PR, so the builder's blind spots aren't the reviewer's.
- **Small changes, green CI.** Each slice is its own reviewable change, merged only with CI passing, and CI kept within a time budget.
- **Build for what exists.** No compatibility layers for data, users or releases that don't exist yet. Every external service sits behind an interface with a fake, so everything runs offline.
- **Rigour scales with risk.** More edge-case tests and a stricter review where a mistake costs more, not everywhere.
- **Decisions written down.** A decision ledger in each PR, ADRs for what's hard to reverse, and the docs updated as part of done.

### Principles

These run through every skill:

1. **Docs are the current truth.** A few short docs say why and what; code and tests show how. Updating the docs is part of done.
2. **Ask about the business, decide the technical.** Business rules (what gets recorded, money, who can do what, what users see) are never guessed silently. Technical choices are the agent's, recorded as it goes. Architecture and commitments outside the codebase (third parties, cost, data leaving the system, licenses) are proposed as ADRs for the human to accept. Questions come in one batch, each with a default, so `go` answers them all; only a brainstorm, while the direction is still open, is a back-and-forth.
3. **What's written is binding.** Anything the human wrote or approved is a requirement. Fill the gaps; flag conflicts rather than silently "improving" things.
4. **Vertical slices, sized by what they deliver.** Each slice works end to end and has acceptance criteria. The plan marks which slices can be built together, and those are built in parallel; rules they share are settled in the feature doc first, so each slice doesn't invent its own.
5. **Stop only where a human is needed.** Run every other step. At a stop, say exactly what to do and what happens after, so nothing has to be remembered.

## How it works

```mermaid
flowchart TB
    Y["You: supercharge …, one session per request"] --> P["supercharge<br>sets the project up if needed, explores with you, one batch of questions, plan"]
    P -->|"direct change: request + criteria"| D["build-slice, builder subagent"]
    P -->|"ticket: issue"| T["build-slice, builder subagent"]
    P -->|"features: issues"| O["ADE such as Orca: the repo's issue list"]
    O --> F2["fresh session: feature 13<br>drive-feature"]
    O --> F1["fresh session: feature 12<br>drive-feature"]
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
    D --> PR4["PR into main"]
    T --> PR3["PR into main"]
    PR2 & PR1 & PR3 & PR4 -->|"you: go"| M["main"]
```

Each slice: acceptance tests first, built, run for real, reviewed by a different model, PR with green CI. One session per request, which hands a direct change or a ticket to a builder subagent; each feature gets a fresh session of its own, one subagent per slice, each in its own worktree (in Orca, created through Orca as a child of the session's worktree, so slices nest under the feature in the sidebar). Every issue starts with the command that builds it, so starting it from the ADE's issue list is the whole handoff. You answer one batch of questions and check the plan at the start, and reply `go` at the end; the docs on `main` carry the context between sessions. Without an ADE, it's several omp or Claude Code sessions side by side, started with the issue URLs.

## Install

In omp:

```sh
npx skills add po2studio/supercharge -g -a pi
```

`pi` is omp's upstream, so this installs into `~/.agents/skills`, which omp reads. Leave out `-g` to install into one project's `.agents/skills` instead. Check with `omp read skill://supercharge`; update with `npx skills update -g`.

In Claude Code:

```sh
npx skills add po2studio/supercharge -g -a claude-code
```

This installs into `~/.claude/skills`; leave out `-g` for one project's `.claude/skills`. Check by typing `/supercharge` in a session, or run `/skills`; update with `npx skills update -g`. That's the whole install: no plugin, agent files, settings or hooks, and nothing beyond `git` and `gh`. The optional `brain` skill downloads one binary the first time you use it, after you say `go` (see [Brain](#brain)); leave it out at install and nothing else changes.

Claude Code needs v2.1.283 or later, where auto mode is the starting permission mode. On an older version, or where auto mode isn't the starting mode, start with `claude --permission-mode auto`. Claude Code is supported but untested: it stays that way until a run in it is written up in [`runs/`](runs/).

## Config

A YAML block in a `## Supercharge` section of the project's `AGENTS.md`. `living-docs` writes it when `supercharge` first sets a project up, asking for each key with its default. A key that's still missing is asked for with the next request's questions:

```yaml
risk: medium                 # low | medium | high; a feature doc may override this, with a reason
approve: [plan, merge]       # spec | plan | slice | merge: wait for the human at these points
ci_budget: 5m                # how long CI may take
docs: docs                   # folder for the docs, ADRs and feature docs
```

Missing keys take the defaults.

One more key is optional and never part of the block `living-docs` writes: `brain:`, the path of this project's brain folder (absolute, `~/…`, or relative to the repo). It overrides the global path in `~/.config/supercharge/brain`, which `/brain` writes on first use. No skill asks for it as a missing key; with neither set there is no brain, and every skill behaves as it did before.

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
| `plan` | After the slices are planned, before building | Check what gets built and how it's cut. Reply `go` or say what to change | It creates the issues. A ticket is built in a builder subagent, reviewed on a different model; a feature is built once you start its issue in a fresh session |
| `slice` | After each slice, or each batch built together, is built, reviewed and merged into the feature branch | Try it: the stop says how to run it and what to check. Reply `go` or report a problem | It fixes the problem, or starts the next slice |
| `merge` | After every slice is done and the docs are refreshed on the feature branch | Review the one PR (or branch) into `main` and its checklist of human-only checks. Reply `go` | It checks CI, merges into `main` and cleans up the branches and worktrees its agents created (in Orca, through Orca), never the one your session runs in. The feature is done |

- **`[plan, merge]` (default):** you check what gets built and how it's cut before any code is written, then see the finished feature. It catches wrong guesses about the product early, which matters most with models that don't ask questions (see [run 4](runs/2026-10-14-receipt-scanner-cheap-models.md)).
- **`[merge]`:** you only see the finished feature.
- **`[slice, merge]`:** test as you go; use it when you want to see each step working on a real device or real data. To see every slice on its own, add "one at a time" to the command.
- **`[]`:** no approvals; the agent merges into `main` as soon as CI passes.

Whatever `approve` says, planning a feature ends its session with a handoff: start the feature issue from Orca, or run `/drive-feature <its URL>`, in a fresh session. You never merge anything yourself: approving is replying `go` in the session.

Small changes skip the feature machinery. A direct change skips only the issue and the planning stop: a builder subagent builds it, a different model reviews it, and the docs are kept true, as for a ticket, which also gets its issue. Each gets its own PR into `main`, which waits for your `go` if `merge` is set.

### `ci_budget`

How long CI may take. When it takes longer, `build-slice` opens an issue to speed it up, if there isn't one open already.

### `docs`

Where the docs, ADRs and feature docs live; every `docs/` path in the skills and in this README means this folder. Leave it as `docs` unless `docs/` already has another job, such as a published docs site; then use something like `docs/supercharge`. `living-docs` checks for this when it asks for `docs`, and offers that as the default. `AGENTS.md` always stays at the repo root. To move the docs later, change `docs` and run `/living-docs`; it moves them and fixes the links.

## Usage

### Setup (once)

**Install** the skills and **set the models** (below; in omp or Claude Code). That's all: the first request in a project sets it up.

### Daily use

1. **Start a new session for each request.** The docs carry the context, so a fresh session loses nothing, and it's cheaper and sharper than one that has run for days.
2. **Run `/supercharge` with what you want, in omp or Claude Code:** an idea, a bug, a change or a feature, from one line to a full spec. For example, `/supercharge the login page fails on an empty email`, or `/supercharge let's explore pricing before we build anything`. In a project that isn't set up yet, it first runs `living-docs`: it reads any existing code, writes `AGENTS.md` (with the [config](#config) block) and the docs, and stops once for you to read them and answer its questions, including every config key that isn't set yet, each with its default; `go` accepts them all. Fix anything wrong in the docs; what you write there is binding. Then it takes your request:
   - **Direction open, or you ask to explore:** it brainstorms with you, back and forth, and writes nothing until you settle the direction. "Not now" ends it with nothing written.
   - **New work:** small things are done directly (no issue, no planning stop, but built in a builder subagent and reviewed on a different model); a ticket gets one issue and is built the same way; bigger work gets a feature doc, plus one GitHub feature issue that lists the slices as a checklist. The doc reaches `main` on its own PR, which the agent merges without waiting for `merge`, so a session started from the issue can read it.
   - **Planned work** (an issue, a feature doc, "build feature 3, two at a time"): it hands it to `drive-feature` or `build-slice`.
   - **Drifted docs:** it runs `living-docs` in the session.
3. **Do what each stop says.** Answer its questions (`go` accepts the defaults), check the plan if `plan` is in `approve`, and approve the PR into `main` with `go`. The agent merges it, and the docs are already current. A feature ends the planning session: start its issue from Orca, or run `/drive-feature <its URL>`, in a fresh session. When a request is finished, start a new session for the next one.

Every issue the bundle creates starts with the command that builds it, so you can start work from Orca's issue list. It's written for you first, then the builder: a ticket has why (the problem and who has it), what changes for them, acceptance criteria (what you can observe), how to check it, what's out of scope, and last what it touches. Titles state the outcome. A feature is one issue, with no sub-issues: the why, what changes, its slices as a checklist of outcome titles in build order, and a link to the feature doc, which holds each slice's criteria. `drive-feature` ticks a slice on the issue once it's merged into the feature branch, so the issue shows how far the feature is. To change the format, put your own `docs/issue-template.md` in the project (in the folder `docs` names): it replaces the default, `skills/supercharge/issue-template.md`, whole. Every issue still starts with the build command, a ticket has acceptance criteria, and a feature issue has the slice checklist, links its doc and copies no criteria. Filter the list with `is:open label:feature` for the features, or `is:open label:ready-for-agent -label:slice` for everything you can start (features and tickets). Features planned with sub-issues, before this format, still build as before, and nothing converts them: their slices are issues labelled `slice` and `feature:NNNN` (`label:feature:NNNN` lists one feature's), each linking to its feature issue.

To build an existing plan:
- **In one session:** start the feature issue from Orca, or run `/drive-feature <feature issue URL or docs/features/NNNN-*.md>`, in omp or Claude Code. It builds the slices the plan marks as buildable together in parallel, and the rest in dependency order, ticks each slice on the feature issue as it merges into the feature branch (in a feature planned with sub-issues, each slice's builder closes its issue instead), and the final PR closes the feature issue. Started again in a fresh session, it skips the slices already done. To check as you go, or to build fewer at once, add a limit in plain words. It stops after each batch on the feature branch, and `go` starts the next. `build only slice N` builds one slice: in a feature with one issue its PR says `Part of #N`, and that issue stays open until the final PR closes it. For example:
  ```
  /drive-feature docs/features/0003-csv-export.md build 3 slices, then stop
  /drive-feature docs/features/0003-csv-export.md at most 2 at once
  /drive-feature docs/features/0003-csv-export.md one slice at a time, I'll test each on my phone
  /drive-feature <feature issue URL> build only slice 2
  ```
- **One ticket, or one slice issue of an older feature, per session** (for example, several sessions side by side, in omp or Claude Code): start its issue from Orca, or run `/build-slice <issue URL>`. The session hands it to a builder subagent, reviewed on a different model, and relays its stop. With no feature branch, each gets its own PR into `main` that closes its issue, merged on your `go`. A slice of a one-issue feature has no issue of its own and is built only through `drive-feature`, which ticks it on the feature issue. For slice issues of an older feature, pick slices the feature doc marks as buildable together. When the last one merges, its stop tells you to start the feature issue from Orca: it skips the finished slices, does only the finish (docs, `living-docs`, status), and its PR closes the feature issue. After a big change made outside the bundle, run `/living-docs` to bring the docs back in line.

## Skills

| Skill | Does |
|---|---|
| `supercharge` | The entry point for any request: sets the project up, brainstorms, triages, then a ticket or `docs/features/NNNN-*.md` plus one feature issue that lists its slices as a checklist, written from an issue template, and hands planned work to `drive-feature` or `build-slice` |
| `living-docs` | Creates or refreshes `AGENTS.md` and `docs/{BUSINESS,PRODUCT,ARCHITECTURE,DESIGN}.md` (BUSINESS and DESIGN are optional) |
| `adr` | Records a decision in `docs/adr/NNNN-*.md` |
| `build-slice` | Builds one slice to done: tests, end-to-end check, review, PR, green CI, docs updated |
| `drive-feature` | Runs `build-slice` across a feature's slices, ticks each on the feature issue as it merges, then reports once |
| `brain` | Optional. Answers a question from a folder of your documents, with paths and dates, using one downloaded binary (SIZE MB) |

Each skill works on its own; `supercharge` and `drive-feature` start the others, and otherwise they coordinate only through the files above and their GitHub issues.
Execution uses omp's built-in features (`task`, the `reviewer` agent, `browser`, the `github` tool), plus `gh` for issues, which the `github` tool can't create; in Claude Code it uses subagents, a headless browser and `gh`. If one is missing, the skill uses an equivalent, or stops and says so. No equivalent replaces a fresh subagent (not a fork) or a reviewer on a different model.

## Brain

Optional. `/brain <question>` answers from a folder of your documents and says which files and dates it found the answer in, for example `/brain why did we add a second phone field to signup?`. It needs no API key and installs nothing but one binary. Indexing, embedding and search all run on your machine; the passages the agent reads to answer go to your model provider, like any file it reads.

- **Install:** `brain` comes with the bundle; leave it out at install and nothing else changes. Its first use downloads one binary (about SIZE MB, from this repo's releases, sha256-checked, cached in `~/.cache/supercharge/brain/<version>/`) for Linux x64 or arm64 (glibc 2.34 or newer) or macOS arm64 or x64.
- **Setup:** with nothing configured, the first `/brain` asks where the brain is (default `~/brain`; an existing folder of documents works as it is), shows the download size and the cache path, and waits for `go`. Then it downloads, checks the checksum (a mismatch aborts and runs nothing), creates `BRAIN.md`, `decisions/`, `notes/` and `projects/` if they're missing, indexes the folder and answers. The path goes in `~/.config/supercharge/brain` (a project can override it with `brain:` in its `AGENTS.md`); nothing is written into any project, and no file in the brain is ever edited.
- **Search:** the agent searches in a scout (so snippets stay out of your session), with full-text (`--terms`: an ID, a name, an error code) and by meaning (`--about`: a question that shares no words with its answer, in any language, Icelandic included), refines up to three times, and answers in at most 10 lines with paths and dates.
- **Index:** one SQLite file per machine under `~/.cache/supercharge/index/`, never inside the brain, so a synced folder is safe. It embeds only files whose content changed; a first index runs at about 12 chunks a second on a laptop CPU.
- **What it reads today:** markdown and plain text (`.md`, `.markdown`, `.txt`, `.text`). Other files are counted as not indexed in `brain stats`, and a text file over 1 MiB, or one it cannot read, is listed there as left out, never skipped silently.
- **The binary alone:** `brain --root <folder> index`, `search --terms … --about … --json`, `stats` and `eval`. `brain eval` indexes the corpus in `brain/eval/` and prints recall@5, MRR, search latency and indexing speed for full-text only, meaning only, and both, against the committed baseline; CI fails on a regression.

## Models

There are three jobs, and they need different things:

- **Planning** (`supercharge`, `living-docs`): judgement about the business, such as which questions to ask, safe defaults, and checking facts. It uses few tokens, so use the strongest model you have. `living-docs` always runs in the session, on this model, never in a builder subagent.
- **Building** (`build-slice`): good technical judgement. It uses most of the tokens, so this is where a cheaper model pays off. A slice or ticket started from its issue is built in a builder subagent too, so the session's model doesn't build.
- **Reviewing** (omp's `reviewer` agent, or a fresh subagent in Claude Code): an independent check. Always a different model from the builder, ideally from a different company. It's cheap.

In omp the agent picks the agent (`task`, `reviewer`, `scout`), never the model: omp's model roles set each agent's model. Each job runs on a role: the session (planning and driving) on `default`, builders (omp's `task` agent) on `task`, and the reviewer on `slow`. omp's small jobs (`scout` searches, session titles) run on `smol`. Set them in `~/.omp/agent/config.yml` (check with `omp config get modelRoles`):

```yaml
modelRoles:
  default: anthropic/claude-opus-5.5     # plans and drives
  task: anthropic/claude-sonnet-5.5      # builds
  slow: openai-codex/gpt-6.1-sol         # reviews
  smol: anthropic/claude-haiku-5.5       # scouts, titles
```

Without `task`, builders run on `default`; without `smol`, so do the small jobs, at the planning model's price. Keep `task` and `slow` different. On a subscription, choose by quota rather than price: a cheaper builder still saves most of it, because building uses most of the tokens.

**In Claude Code** there are no model roles: the session runs on the model you pick (`/model`), builders on `sonnet`, and the reviewer on a different Claude model (`opus` for a `sonnet` builder, otherwise `sonnet`). So a ticket is built on `sonnet` and reviewed by `opus`, whatever model the session runs on. The cross-company reviewer is omp-only. If no such reviewer can be started, `build-slice` stops and says so.

**The `brain` skill's searches** run in a scout, the small job: omp's `scout` on `smol`, and in Claude Code a fresh subagent on `haiku`. Finding a file is cheap work, and the snippets it reads never enter your session.

Tested in [`runs/`](runs/) (✓ good, ✗ not good enough, ? untested, – not worth testing for that job; prices per million tokens in/out on OpenRouter, October 2026). The frontier models (Fable, Opus, Astra, Sol) qualify for every job without a run of their own: they're at least as strong as the models that passed. Sonnet 5.5 builds in the owner's daily use (2026-11-12), without a write-up.

| Model | In / out | Plan | Build | Review |
|---|---|---|---|---|
| `anthropic/claude-fable-5.1` | $10 / $50 | ✓ | ✓ | ✓ |
| `anthropic/claude-opus-5.5` | $4 / $20 | ✓ | ✓ | ✓ |
| `anthropic/claude-sonnet-5.5` | $2 / $10 | ? | ✓ | ? |
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
