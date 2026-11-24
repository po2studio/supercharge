# Usage

## Setup (once)

Install the skills and [set the models](models.md). That's all: the first request in a project sets it up.

Claude Code needs v2.1.283 or later, where auto mode is the starting permission mode. On an older version, or where auto mode isn't the starting mode, start with `claude --permission-mode auto`. Claude Code is supported but untested: it stays that way until a run in it is written up in [`runs/`](../../runs/).

## Daily use

1. **Start a new session for each request.** The docs carry the context, so a fresh session loses nothing, and it's cheaper and sharper than one that has run for days.
2. **Run `/supercharge` with what you want:** an idea, a bug, a change or a feature, from one line to a full spec, for example `/supercharge the login page fails on an empty email` or `/supercharge let's explore pricing before we build anything`. In a project that isn't set up yet, it first runs `living-docs`: it reads any existing code, writes `AGENTS.md` (with the [config](config.md) block) and the docs, and stops once for you to read them and answer its questions, each with its default; `go` accepts them all. What you write in the docs is binding. Then it takes your request:
   - **Direction open, or you ask to explore:** it brainstorms with you and writes nothing until you settle the direction. "Not now" ends it with nothing written.
   - **New work:** small things are done directly (no issue, no planning stop, but built in a builder subagent and reviewed on a different model); a ticket gets one issue and is built the same way; bigger work gets a feature doc plus one GitHub feature issue that lists the slices as a checklist. The doc reaches `main` on its own PR, which the agent merges without waiting for `merge`.
   - **Planned work** (an issue, a feature doc, "build feature 3, two at a time"): it hands it to `drive-feature` or `build-slice`.
   - **Drifted docs:** it runs `living-docs` in the session.
3. **Do what each stop says.** Answer the questions (`go` accepts the defaults), check the plan if `plan` is in [`approve`](config.md#approve), and approve the PR into `main` with `go`. The agent merges it and the docs are already current. A feature ends the planning session: start its issue from Orca, or run `/drive-feature <its URL>`, in a fresh session.

## Building a plan

- **A whole feature:** start the feature issue from Orca, or run `/drive-feature <feature issue URL or docs/features/NNNN-*.md>`. It builds the slices the plan marks as buildable together in parallel and the rest in dependency order, ticks each slice on the feature issue as it merges into the feature branch, and the final PR closes the feature issue. Started again in a fresh session, it skips the slices already done. Add a limit in plain words to check as you go; it then stops after each batch on the feature branch, and `go` starts the next. `build only slice N` builds one slice: its PR says `Part of #N`, and that issue stays open until the final PR closes it.

  ```
  /drive-feature docs/features/0003-csv-export.md build 3 slices, then stop
  /drive-feature docs/features/0003-csv-export.md at most 2 at once
  /drive-feature docs/features/0003-csv-export.md one slice at a time, I'll test each on my phone
  /drive-feature <feature issue URL> build only slice 2
  ```

- **One ticket per session**, for example several side by side: start its issue from Orca, or run `/build-slice <issue URL>`. A builder subagent builds it, a different model reviews it, and the session relays its stop. Each gets its own PR into `main` that closes its issue, merged on your `go`.
- **An older feature planned with sub-issues** still builds: its slices are issues labelled `slice` and `feature:NNNN`, each linking to its feature issue. Pick slices the feature doc marks as buildable together. When the last one merges, its stop tells you to start the feature issue, which does only the finish (docs, `living-docs`, status) and whose PR closes it.

After a big change made outside the bundle, run `/living-docs` to bring the docs back in line.

## Issues

Every issue the bundle creates starts with the command that builds it, so you can start work from Orca's issue list. It reads for you first, then the builder:

- **A ticket:** why (the problem and who has it), what changes for them, acceptance criteria (what you can observe), how to check it, what's out of scope, and last what it touches. Titles state the outcome.
- **A feature:** one issue, no sub-issues: the why, what changes, its slices as a checklist of outcome titles in build order, and a link to the feature doc, which holds each slice's criteria. `drive-feature` ticks a slice once it's merged into the feature branch, so the issue shows how far the feature is.

To change the format, put your own `docs/issue-template.md` in the project (in the folder [`docs`](config.md#docs) names): it replaces the default, `skills/supercharge/issue-template.md`, whole. Every issue still starts with the build command, a ticket has acceptance criteria, and a feature issue has the slice checklist, links its doc and copies no criteria.

Filter the list with `is:open label:feature` for the features, or `is:open label:ready-for-agent -label:slice` for everything you can start.
