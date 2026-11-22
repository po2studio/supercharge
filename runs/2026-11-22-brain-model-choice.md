# The default model, picked on the full eval (2026-11-22)

Slice 7 of [feature 0002](../docs/features/0002-second-brain.md), the last slice. The default `multilingual-e5-small` (int8) against EmbeddingGemma 2 (text only, 256 dimensions), on the full eval: both formats, decisions, notes, Icelandic, duplicates, project docs. The rule was: switch only if recall@5 rises by 5 points or more, and indexing a 1M-document brain is still possible with the shared index.

**Result: no switch.** Recall@5 of the search users get (hybrid) did not rise: 0.917 to 0.911. The candidate's best mode (meaning only) rose 2.2 points. It indexes about 7 times slower, searches 6 to 11 times slower in-process (1.65 times from the command line, where loading the model dominates), and makes the binary 2.2 times bigger. Nothing in `brain/` changes, and `brain.json` still pins 0.6.0.

## The candidate, verified

| | |
|---|---|
| Exists as named | Yes: `google/embeddinggemma-2` (Hugging Face, created 2026-09-14; launch blog and model card dated 2026-10-06). Multimodal (text, image, video, audio), 740M parameters of which the text model is 270M; native 768 dimensions, Matryoshka cuts at 512, 256 and 128; 8K context. |
| Licence | Apache 2.0, in the model card on ai.google.dev and in the Hugging Face metadata of both `google/embeddinggemma-2` and the ONNX export. (EmbeddingGemma 1, `embeddinggemma-300m`, is under the Gemma Terms of Use; not the candidate.) The card also says "Deployments must adhere to the Gemma Prohibited Use Policy": a use policy on top of the licence, not read further here, flagged for the owner. Had the rule said switch, this was the one thing to settle before. |
| An int8 ONNX export | `onnx-community/embeddinggemma-2-ONNX` at revision `daa72c51243991dfcaf9f9137d2c573d8f7790c0`: `onnx/model_quantized.onnx` plus `model_quantized.onnx_data` (314 MB, 8-bit weights, one graph for all modalities taking `image_features`, `video_features`, `audio_features` inputs besides the text ones), and a 32 MB `tokenizer.json`. Text only is the same file; the vision and audio encoders are separate files that are not needed. fp16 is not an option (the card: it returns NaN). |
| Runs on the pinned ONNX Runtime 1.22 | **Not as published**, two things: (1) the graph's embedding lookup is a `GatherBlockQuantized` node with a `bits` attribute that ORT 1.22 does not know (`Unrecognized attribute: bits`); (2) its 146 `MatMulNBits` nodes are 8-bit, which 1.22's CPU kernel runs only on the packed path, selected by `accuracy_level=4`; without it every run fails (`Only 4b quantization is supported for unpacked compute`). Rewritten for the measurement: the one gather became three `Gather`s and a blocked `DequantizeLinear` (same numbers, weights file untouched) and each `MatMulNBits` got `accuracy_level=4` (activations quantised to int8). With both, it loads and runs on this CPU. **Not tested on the other three platforms** (the 8-bit packed kernel is ISA-specific). A switch would have to host that rewritten file, since `build.rs` pins a Hugging Face file. |

## Machine and method

- **Machine:** AMD Ryzen 7 5700G (6 cores, no SMT), 15 GB RAM, Linux, shared with the owner's other work (load average 3.4 to 8.5 during the runs, so every speed below is noisy; the ratios hold in every run). Both models were measured here, one after the other, the evals interleaved (e5, Gemma, e5, ...) three times. Throwaway cache, nothing under `~/.config` or `~/.cache/supercharge/`.
- **Binaries:** both built from the same tree (`brain` 0.6.0), same container (`ubuntu:22.04`, capped at 6 GB, one job at a time), same profile (dependencies at opt-level 3, the `brain` crate at 1 because of the host's memory; the models' speed is ORT's). The Gemma build peaked at 3.9 GB. The Gemma variant is a copy of the tree with only `src/embed.rs` changed (not committed): the model and tokenizer compiled in (the weights file through ORT's in-memory external initialisers), prefixes `task: search result | query: ` and `title: none | text: ` (the chunk's own context header follows), the model's pooled `sentence_embedding` cut to the first 256 dimensions and made unit length again, `<pad>` 0, chunk sizes counted with Gemma's own tokenizer (about 400 tokens, so chunk boundaries move: 195 chunks against 192). `center.f32` (the 1-bit codes' centre) was left at zero: a brain under 50 000 chunks never uses the codes.
- **Eval:** `brain eval` over `brain/eval`: 88 questions, 138 files, two sources. The corpus did not grow since slice 6, so the baseline file is unchanged. Recall@5 and MRR do not move between runs.
- **Latency:** in-process hybrid search p50 on the eval brain (the question embedded each time); and `brain bench --chunks 20000 --embed-sample 256` for the command line's `brain search` (the model loaded for every question) and indexing speed on its real-looking text.

## The comparison

| | multilingual-e5-small int8 (384 d) | EmbeddingGemma 2 int8 (256 d) |
|---|---|---|
| recall@5, hybrid (what `brain search` returns) | 0.917 | 0.911 |
| MRR, hybrid | 0.849 | 0.863 |
| recall@5, meaning only (`--about`) | 0.917 | 0.939 |
| MRR, meaning only | 0.889 | 0.880 |
| recall@5, full-text only | 0.803 | 0.803 (no model involved) |
| search p50, in-process, hybrid (3 runs) | 5.0, 7.6, 8.6 ms | 52.6, 57.4, 61.0 ms |
| `brain search` from the command line (bench) | 831 ms | 1 367 ms |
| indexing, eval corpus (3 runs) | 23.1, 17.7, 17.0 chunks/s | 2.9, 2.5, 3.0 chunks/s |
| indexing, `brain bench` text | 14.1 chunks/s | 1.8 chunks/s |
| binary (this build; the released 0.6.0 linux-x64 is 176.8 MB) | 176.5 MB | 387.6 MB |
| model compiled in | 118 MB | 314 MB weights + 32 MB tokenizer |

Sensitivity: at the model's native 768 dimensions (also built and run once) hybrid recall@5 is 0.917 (MRR 0.868), meaning only 0.951 (MRR 0.898), 3.4 points over e5 in its best mode, 3.4 chunks/s on the eval corpus. Still short of 5 points where users search.

Per question, hybrid: Gemma finds none that e5 misses in the top 5 and misses one that e5 finds (`back-out`). In meaning-only search Gemma finds three e5 misses (`dec-adr-superseded`, `is-hotel-cap`, `overseas-job`) and misses one (`back-out`); on the others the ranks swap about evenly. Full-text carries the exact terms in both, which is why hybrid hides most of what a better embedding adds.

## The decision

The rule needs both conditions; the first fails, which settles it, and the second is shown only as a risk.

1. **Recall@5 rises 5 points or more:** no. Hybrid, the mode the `brain` skill's search uses: -0.6 points. The most favourable reading, meaning-only search: +2.2 points at 256 dimensions, +3.4 at 768.
2. **Indexing a 1M-document brain still possible with the shared index:** not proven either way, and made much worse. The shared index's CI publishes partial indexes and restarts until done, so a build this slow is possible, not impossible. These are extrapolations from the 256-chunk bench, not measurements. On this machine 1M chunks take about 20 hours with e5 (14.1 chunks/s) and about 6.4 days with Gemma (1.8 chunks/s). The shared index's CI job stops at 330 minutes: that embeds about 280 000 chunks with e5, about 36 000 with Gemma, so a first build is about 4 runs against about 28. Slice 6's acceptance target of at least 12 chunks/s is also missed by 6.7 times, on a machine that was busy for both models.

The switch would also have meant a release with two platforms' int8 kernels unproven, a rewritten model file to host, and a binary of 388 MB instead of 177 MB for every user of the skill. None of that is bought by this recall. So e5 stays and ADR 0002 stands; there is no new ADR.

## What went wrong

- **The published int8 file did not load.** Found only by running it. [INFERENCE] The export targets a newer ONNX Runtime than the one the bundle pins (the pin is because later releases have no macOS x64 build, see `brain-release.yml`).
- **First variant build was killed by the OOM killer** with `-j 2` at 6 GB: the 314 MB `include_bytes!` is what changed from the e5 build, which fit at `-j 2`. With `-j 1` it peaked at 3.9 GB and passed, so the candidate does build inside the cap.
- **Zero-length feature inputs are refused** by the `ort` crate; the three modality inputs get one zero row each, which the graph ignores when the text has no image, video or audio tokens.
- **The committed baseline differs slightly from this machine's e5 numbers** (hybrid recall@5 0.900 and about MRR 0.882 there, 0.917 and 0.889 here, same 88 questions and 138 files). [INFERENCE] ORT kernels differ between the CI runner's CPU and this one. Left alone: the corpus did not grow, and the CI check allows 1.5 points.

## Changes made afterwards

None. The variant is not in the repository; to repeat the measurement, change `src/embed.rs` as described above, with the model rewritten as in the table.
