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
- [ ] CI on Windows, Linux and macOS

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
- [ ] Stress library measured against the budgets in 7.5
- [ ] Playwright tests against the mock backend

## M2 Organize core

- [ ] Categories, provenance, rules engine with defaults
- [ ] Folder templates, plan preview, apply with journal, undo, recovery
- [ ] Trash, ExifTool writes, long paths, conflicts

## M3 Dates, places, map
## M4 Local AI
## M5 Decision models and cost
## M6 Events, Immich, polish
## M7 Later

## Next

Measure the stress library, add Playwright and CI, then start M2.
