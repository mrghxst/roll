# Benchmarks

Budgets from PROMPT.md section 7.5, measured on the 100k stress library.

## How to run

```bash
cargo run --release -p roll-fixtures -- --stress 100000 --stress-only
cargo run --release -p roll-core --example bench -- fixtures-out/stress
cd apps/desktop && npx playwright test e2e/perf.spec.ts
```

The stress library has cheap 256×192 pixels and realistic metadata shares
(32% camera, 11% screenshots, saved images at social sizes). Thumbnail
times for real 12 MP photos are higher; see the estimate below.

## Results, 2026-10-10

Owner's class of machine (16 threads, NVMe), release build.

| Step | Took | Budget | RSS |
|---|---|---|---|
| First items in the database (first chunk) | 0.38 s | thumbnails visible < 10 s | 57 MB |
| Metadata index, 100,000 files | 10.7 s | < 10 min | 57 MB |
| Groups | 0.64 s | | 93 MB |
| Timeline payload, 100k items (1.9 MB) | 0.16 s | | 131 MB |
| Text search | 0.08 s | < 300 ms | 196 MB |
| Filtered count | 0.03 s | < 300 ms | 228 MB |
| 256 px thumbnails, 100,000 | 92 s | < 30 min | 199 MB |

Thumbnail store: 250 MB, 2.4 KB per item for the stress pixels (real photos
average 10–20 KB).

UI, headless Chromium, mock backend with 100,000 items:

| What | Measured | Budget |
|---|---|---|
| Scrolling, median frame | 16.7 ms | 60 fps |
| Scrolling, 95th percentile frame | 18.4 ms | |
| Scrubber jump to placeholders | 73 ms | < 100 ms |
| Justified layout of 100k items (worker) | < 250 ms (unit test) | |

The fixture library (837 files of every format, HEIC and video through
FFmpeg): scan 0.1 s, all thumbnails 3.6 s, HEVC playback proxy 0.5 s.

## Estimates for real photos

A 12 MP JPEG decodes in about 60 ms on one core; with 14 worker threads
100,000 photos take about 7 minutes, well inside 30 minutes. Embeddings
and warm start are measured when those milestones land.

## Map (M3), 2026-10-11

100,000 placed items (mock backend, `?mock=100000&geo=1`), clustered
MapLibre source, panning and zooming one step per frame
(`e2e/perf.spec.ts`), 1440×900:

| Renderer | Median frame | 95th percentile |
|---|---|---|
| Owner's GPU (RTX 2080 Ti, Direct3D 11) | 16.6 ms | 17.6 ms |
| Headless software WebGL (SwiftShader, CI) | 24.7 ms | 37.1 ms |

The budget (60 fps) holds on the owner's PC. Thumbnail pins are DOM nodes
and are rebuilt only once the map rests; rebuilding them every frame cost
about 17 ms. The CI test asserts a looser 34 ms because CI has no GPU.

## Search (M4), 2026-10-11

100,000 stress items, release build, `cargo run --release -p roll-core
--example bench -- fixtures-out/stress --no-thumbs`:

| What | Measured | Budget |
|---|---|---|
| Full search index rebuild (background job) | 0.69 s | |
| "img", matching all 100,000, to timeline payload | 0.26 s | < 300 ms |
| Parsed query ("videos summer 2019") to timeline | < 0.01 s | < 300 ms |

The metadata index took 62 s in this run against 10.7 s earlier; the stress
files were not in the operating system's file cache this time.

## Vector search

`cargo test -p roll-ai --release -- --ignored --nocapture`: brute force
over 100,000 normalized 768-dimension rows (SigLIP 2 B/16 size), top 200,
on the owner's PC (rayon, all cores): 12 ms.

## Decision models on the CC0 set (M5), 2026-10-13

`tools/cc0/build.mjs` builds 157 labelled items in `fixtures-out/cc0`:
photos, documents and wallpapers from Openverse (CC0 and the Public Domain
Mark, checked by eye; corrections in `tools/cc0/review.json`), plus
screenshots, memes, social media posts and junk shots rendered from them
and our own text. `crates/core/tests/real_models.rs` runs the decision
model through the Library on every item and compares with the labels.

Local, Ollama 0.34.4, `qwen3-vl:8b-instruct` (Q4_K_M, 6.1 GB) on an
RTX 2080 Ti (11 GB), prompt `decide-2`:

| Category | Right |
|---|---|
| Photos | 47 of 47 |
| Screenshots | 25 of 25 |
| Memes | 25 of 25 |
| Social media | 20 of 20 |
| Documents | 11 of 11 |
| Junk | 12 of 15 (3 taken for photos) |
| Wallpapers | 10 of 14 (4 space images taken for photos) |
| **All** | **150 of 157, 95.5%**, none unsure |

0.48 items per second with Ollama's default 4,096-token context: Qwen3-VL
takes about 1,100 tokens per 512 px image, so Roll halves the batches to
2 images per request. Nothing left the machine. A 16,384-token context
(`OLLAMA_CONTEXT_LENGTH`, 8 images per request, all on the card) was much
slower: stopped after 25 minutes against 5.5. The default context is the
better setting on this card.

What the runs found:

- The plain `qwen3-vl:8b` tag is the thinking variant. Its reasoning used
  up the output allowance, every answer came back empty, and none of the
  "reasoning off" switches (`reasoning_effort`, `think`, `reasoning`) turn
  it off through Ollama's OpenAI endpoint. The `-instruct` tags answer
  directly; the model list now points to them and marks thinking variants.
- With prompt `decide-1` (the spec's starting point) the same model scored
  75%: chat and settings screenshots were taken for social media. The
  sharper definitions and one line in the prompt (`decide-2`) fixed that.
