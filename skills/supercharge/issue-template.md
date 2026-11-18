# Issue template

The default for every issue `supercharge` creates. Replace it whole with `docs/issue-template.md`. Whatever a template says, the body starts with the build command, a ticket has acceptance criteria, and a feature issue links its feature doc and copies no criteria. Write for a human first and keep it short: enough to understand what is being done, no more.

## Ticket

~~~md
/build-slice <this issue's URL>

## Why
<The problem and who has it.>

## What changes
<What is different for them once it's done.>

## Acceptance criteria
- <What someone can observe. No files or modules.>

## How to check
<What a human does to see it work.>

## Out of scope
<What this leaves alone.>

## Touches
<Screens, tables, modules, rules: for the builder.>
~~~

## Feature issue

~~~md
/drive-feature <this issue's URL>

## Why
<The problem and who has it.>

## What changes
<What is different for them once it's all done.>

## Feature doc
<Link to docs/features/NNNN-slug.md on `main`. The doc holds the deliverables and criteria.>
~~~
