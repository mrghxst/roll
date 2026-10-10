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
100,000 photos take about 7 minutes, well inside 30 minutes. Embeddings,
the map and warm start are measured when those milestones land.
