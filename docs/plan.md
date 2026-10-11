# Plan

Milestones from PROMPT.md section 10. `[x]` done, `[~]` partly done,
`[ ]` not started.

## M0 Foundations

- [x] Monorepo layout, Rust 2024 workspace, pnpm workspace
- [x] Tauri and Svelte shell with the design system (tokens, Geist fonts,
      buttons, inputs, sheets, sidebar, top bar, empty states)
- [x] First-run flow, light and dark
- [x] Fixture generator with ground-truth manifest and stress mode
- [x] CLAUDE.md, docs/plan.md, docs/decisions.md
- [x] CI workflow on Windows, Linux and macOS (`.github/workflows/ci.yml`);
      not yet run on GitHub: needs the commits pushed

## M1 Library

- [x] Roots, parallel scan (nom-exif plus native parsers), the database
- [x] Re-linking of files moved outside Roll, sidecars, groups
- [x] Thumbnails for every fixture format (JPEG, HEIC, PNG, GIF, RAW, video)
- [x] Timeline: justified rows, month and day headers, sticky month,
      scrubber, badges, selection (click, Ctrl, Shift, band, Ctrl+A, day and
      month headers), row height
- [x] Viewer: preview then full, zoom, pan, filmstrip, info, video with
      proxy fallback, Live Photo playback
- [x] Folders view, filters (facets), file watching, Activity panel
- [x] Stress library measured against the budgets in 7.5 (docs/benchmarks.md)
- [x] Playwright tests against the mock backend, including 100k scrolling
- [x] Scan streams in committed chunks; the grid fills while indexing

## M2 Organize core

- [x] Categories (add, rename, nest, delete), provenance, rules engine with
      defaults, classification after scans and thumbnails
- [x] Folder templates, planner, journal (apply, undo, crash recovery),
      tested with a hash manifest
- [x] Trash and Recycle Bin, ExifTool writes with snapshots (round trip on
      JPEG, HEIC, PNG, MOV, MP4), long paths, case-only conflicts
- [x] Tauri commands and UI: Organize preview, History with Ctrl+Z, batch
      edit actions (category, tag, favorite, rating, rotate, trash), Trash
      view, Settings for categories and rules, Playwright tests
- [x] Acceptance run on a copy of the fixture library, with a real process
      kill during apply (`crates/core/tests/organize_acceptance.rs`)
- [x] Explorer check through the Windows property system (docs/checks.md)
- [ ] Photos app check by hand (docs/checks.md has the steps)
- [ ] Immich round trip (needs Docker; moved to M6 with the Immich work)
- [ ] Rare access violation in the acceptance test during thumbnailing
      (about 1 run in 15, not reproduced in isolation); under investigation

## M3 Dates, places, map
## M4 Local AI
## M5 Decision models and cost
## M6 Events, Immich, polish
## M7 Later

## Next

Pin down the rare crash, then M3: fix dates, fix places, the map with the
time slider, offline gazetteer and time zones.
