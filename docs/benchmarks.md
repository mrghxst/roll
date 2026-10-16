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

### Globe, 2026-10-13

The same run with the globe projection. Locally the perf project now runs
in Edge on the graphics card (the WebView2 engine); without vsync in
headless mode the frames are shorter than a 60 Hz refresh:

| Renderer | Projection | Median frame | 95th percentile |
|---|---|---|---|
| Edge, RTX 2080 Ti, Direct3D 11 | globe | 4.2 ms | 5.7 ms |
| Edge, RTX 2080 Ti, Direct3D 11 | flat | 3.6 ms | 3.8 ms |
| Headless software WebGL (SwiftShader) | globe | 90.9 ms | 128.1 ms |
| Headless software WebGL (SwiftShader) | flat | 24.3 ms | 35.0 ms |

The globe's subdivided tiles cost little on a GPU and a lot in software,
so CI's limit went from 200 to 400 ms (a Windows runner measured 260 ms).

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

OpenRouter, `google/gemma-4-26b-a4b-it` through a zero-data-retention
endpoint, same prompt, 2026-10-13:

| Set | Items | Right | Spent | Estimate |
|---|---|---|---|---|
| CC0 set | 157 | 145, 92.4% (junk 10 of 15, wallpapers 7 of 14, everything else all right) | $0.0068 | $0.0054 |
| CC0 set and fixture library | 980 | (fixture "photos" are generated test patterns; the model calls them wallpapers, which they look like) | $0.0375 | $0.0337 |

That is about $4.30 per 100,000 items, 52 s for 157 items. Estimates ran
10–26% low on the first run with a model; Roll keeps the measured token
counts per model, so later estimates use them.

## Local image model and text reader (M4), 2026-10-14

SigLIP 2 ViT-B/16 (ONNX, Immich's export), RTX 2080 Ti through DirectML;
CC0 set (157 items) as a library, `crates/core/tests/vision_real.rs`:

| Step | Result |
|---|---|
| Zero-shot on full-size images, mean of descriptions | 82.2% right |
| Zero-shot, best description per category | 89.2% right; top 60% by margin 98.9% right |
| In the library (256 px thumbnails), auto-accepted | 68% of items, 97.2% right (3 wrong: two photos taken for junk, a post for a photo) |
| Scan, thumbnails, embeddings and sorting | 11 s for 157 items |
| With the text reader as well | 39 s; text in 70 items |
| Image model alone, full-size JPEGs from disk | 18–21 items/s |

Text reader (PP-OCRv6 small), per image:

| Image | CPU | DirectML |
|---|---|---|
| Chat screenshot, 8 lines | 180 ms | 1,015 ms |
| Screenshot, 14 lines | 236 ms | 1,397 ms |
| Meme, 2 lines | 229 ms | 232 ms |
| 1899 receipt, 24 lines | 294 ms | 1,144 ms |

Search, text to picture similarity on the CC0 set: relevant pictures score
0.09–0.17, the median item 0.01–0.04; "chat" finds cats (French).

## Best matches and better ranking, 2026-10-15

`crates/core/tests/rerank_real.rs` on the 157-item CC0 set with neutral
file names; relevance is a title word from Openverse (rough: many
screenshots and documents have no title). Precision of the first 5 best
matches over 8 searches (cat, dog, food on a plate, beach, city street,
birthday party, handwritten letter, mountains):

| Order | Precision at 5 |
|---|---|
| Before the fix below | 58% |
| Fused text and picture ranks | 64% |
| Rated by qwen3-vl:8b-instruct (local) | 64% |

- The run found a search bug: Roll's own tags (`Category/Social media`)
  were indexed with their root, so "cat" matched 46 items through
  "Category". Roots of `Category/`, `Event/`, `Source/` and `From/` tags
  are now left out of the index; "cat" matches 15 items, 3 of them by
  text.
- The model moved two more cats into the top 5 for "cat" and one food
  picture out for "food on a plate"; the other orders stayed. On a set
  this small the fused order is already good; better ranking is for long
  result lists.
- Speed with the local model: about 1.5 s a picture (8 per request) on
  the RTX 2080 Ti, so 50 pictures take about 75 s; the first request after
  loading the model takes longer.

## Faces, 2026-10-15

YuNet 2023mar and SFace 2021dec (`crates/ai/examples/faces.rs`) on CC0
family photos at 1024 px, RTX 2080 Ti machine:

| Step | Time |
|---|---|
| Find faces (YuNet, 640 px input, CPU) | 24–34 ms a photo |
| Face numbers (SFace, DirectML) | 2 ms a face |

- 22 faces in 6 photos (portraits at 133–168 px, group photos at 37–80
  px); a 1915 photo's only face, 20 px wide, is kept but too small to
  recognize.
- The same three people in the two "family picnic at Chinde Point" photos
  pair up at 0.44–0.51; two different faces within one blurry family
  picnic photo (sharpness 11–18) reached 0.44, which is why two faces in
  one item are never one person.

## Roll's local AI, 2026-10-15

`crates/core/tests/real_models.rs bundled`: the `local-ai` pack (llama.cpp
b11372 Vulkan, Qwen3-VL 4B Q4_K_M with the Q8_0 projector) started by
Roll on the first request, RTX 2080 Ti, the 157-item CC0 set:

| Model | Accuracy | Speed |
|---|---|---|
| Qwen3-VL 4B, Roll's local AI | 91.1% (143 of 157) | 0.95 items/s |
| Qwen3-VL 8B, Ollama (M5) | 95.5% | 0.48 items/s |

- Starting the server and loading the model took about 10 s; the run
  took 165 s, and the server stopped with the library.
- Its mistakes: wallpapers taken for photos (6) or junk (3), junk taken
  for photos (3). Documents, memes, screenshots and posts were all right.

## Offline map areas, 2026-10-15

`crates/core/tests/offline_real.rs`, Protomaps build 20261015, zoom 0 to
13, six range requests at a time:

| Area | Tiles | Stored | Time |
|---|---|---|---|
| Central Lisbon | 46 | 4.4 MB (95 KB a tile) | 16 s |
| Lake Garda region | 326 | 17.2 MB (53 KB a tile) | 14 s (91–120 s while the server's cache was cold) |

Finding the tiles in the archive's directories took 3.8 s for Lake Garda
(6 leaf directories).
