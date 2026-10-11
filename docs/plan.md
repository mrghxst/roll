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

- [x] Geo crate: time zones (tzf-rs), importers for GPX, KML, GeoJSON and
      the three Google formats, interpolation with accuracy, coordinate and
      map link parsing, a GeoNames gazetteer
- [x] Fix dates: camera sets with reasons, shift to a reference (or a
      photo from another device), time zone, drift, shift by an amount,
      exact and suggested dates, whole-camera ranges, previews with year and
      folder side effects
- [x] Fix places: suggestions from tracks, own photos and folder names,
      accept, adjust, reject, the time zone follow-up, "Set location…" with
      search, pasted coordinates and links, copy and clear
- [x] Map: time slider (month and week histogram, range handles, wheel
      zoom, year chips, play), clusters with thumbnail pins, dashed rings,
      heatmap, rectangle and lasso selection, side grid, tray with drag
      and drop, offline Natural Earth basemap
- [x] Acceptance: the +1 y 2 h camera set fixed with one reference (4
      actions in the UI test), 500 photos placed from one GPX in one
      accepted suggestion, 100k points at 60 fps on the owner's GPU
- [ ] Opt-in online detail tiles (OpenFreeMap), place names download with
      consent (with the M4 pack manager), event centroids (M6)
## M4 Local AI
## M5 Decision models and cost
## M6 Events, Immich, polish
## M7 Later

## Next

M4: the pack manager, SigLIP 2 embeddings, search, classifiers, Review,
Teach and Discover, duplicates, OCR and the Private guard.
