# 0002: Local embeddings and one SQLite index

Status: accepted

## Context

Search has to find an answer that shares no words with the question, and an exact ID, across a million documents in several languages, with no service and no key ([feature 0002](../features/0002-second-brain.md)).

## Decision

`multilingual-e5-small`, int8, compiled into the binary and run on the CPU with ONNX Runtime. One SQLite file per brain holds FTS5 (BM25) and the vectors (int8, plus 1-bit codes for a first pass); hybrid results are fused by rank (RRF). Brute force over the codes, no approximate-nearest-neighbour index. The index is derived: rebuilt whenever the model ID or binary version changes, and kept per machine under `~/.cache/supercharge/index/<root-hash>/`, never inside the brain. Big brains pull an index built once in CI and embed only what changed.

## Alternatives

- Embeddings over an API: ruled out by the owner.
- EmbeddingGemma 2 (text only, 256 dimensions, Apache 2.0): measured on the full eval in [runs/2026-11-22-brain-model-choice.md](../../runs/2026-11-22-brain-model-choice.md). Hybrid recall@5 did not rise (0.917 to 0.911), indexing was about 7 times slower (a 1M-chunk shared-index build about 28 CI runs instead of 4) and the binary 2.2 times bigger; its int8 export needed a rewrite to run on ONNX Runtime 1.22.
- A vector database or an ANN index (HNSW): a service or a graph to build and keep in memory, for a wall brute force already reaches.
- The index inside the brain folder: a synced folder (Dropbox, Drive) would sync and corrupt it.

## Consequences

Indexing runs at about 12 chunks/s on a laptop CPU, so a big brain is only practical with the shared index. Changing the model rebuilds every index. 1M chunks is the wall; beyond it the search design changes.
