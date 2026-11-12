# 0001: One downloaded binary in a skill bundle

Status: proposed

## Context

The bundle is skill text: nothing to install beyond `git` and `gh`, nothing to run. The second brain ([feature 0002](../features/0002-second-brain.md)) needs file conversion, chunking, CPU embeddings and search, which no skill text can do, and the owner wants no dependencies, API keys or services.

## Decision

One statically linked Rust binary, `brain`, in this repo under `brain/`, released from this repo for linux x64 and arm64 and macOS arm64 and x64. Only the `brain` skill uses it: it downloads the pinned release with `gh release download` on first use, after showing the size and waiting for `go`, verifies the sha256, and caches it in `~/.cache/supercharge/brain/<version>/`. The other skills work without it.

## Alternatives

- A Python or Node package: dependencies, runtimes and versions on the user's machine.
- Embeddings over an API: keys, cost, and the company's documents leaving the machine.
- A separate repo for the binary: two things to install and keep in step.

## Consequences

This repo gets Rust code, Rust CI and releases; "nothing beyond `git` and `gh`" holds for every skill but `brain`, which needs the network once. A platform that can't be built stops the release rather than shipping fewer platforms silently.
