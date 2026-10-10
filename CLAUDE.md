# Roll

Desktop photo library (Tauri 2, Rust, Svelte 5). PROMPT.md is the spec;
docs/plan.md tracks milestones; docs/decisions.md records deviations.

## Layout

- `crates/meta` metadata read/write, `crates/media` decode/thumbnails/video,
  `crates/core` DB, scan, jobs, queries, Library facade, `crates/geo`,
  `crates/ai`. No Tauri dependency in `crates/*`.
- `apps/desktop/src-tauri` thin commands + `roll://` scheme;
  `apps/desktop/src` Svelte UI. `src/lib/ipc/api.ts` picks the Tauri or mock
  backend (`mock.ts` runs in a plain browser: `?setup=1&mock=N`).
- `tools/fixtures` generates `fixtures-out/` (gitignored) with `manifest.json`.

## Commands

```bash
cargo test --workspace                     # Rust tests (fixture tests skip if fixtures-out is missing)
cargo run -p roll-fixtures                 # fixture library (~2 min; needs ffmpeg with x264/x265 and exiftool)
cargo run -p roll-fixtures -- --stress 100000 --stress-only
cargo test -p roll-core export_bindings    # regenerate TS bindings in src/lib/ipc/bindings
cd apps/desktop && pnpm check && pnpm test # svelte-check, vitest
cd apps/desktop && pnpm tauri dev          # the app; set ROLL_DATA_DIR to keep dev data apart
```

## Conventions and pitfalls

- Rust 2024, `cargo fmt`, `clippy -D warnings`. LF endings everywhere.
- Never touch real photos in development; use `fixtures-out/`.
- UI copy: sentence case, no exclamation marks, say what an action does.
- Svelte 5 effects: never read and write the same `$state` inside one
  effect; wrap such calls in `untrack` (the grid hit this twice).
- quick-xml 0.42 names are `&str`, entity references arrive as
  `Event::GeneralRef`.
- parking_lot `Condvar` must be used with a single mutex.
- SQL: `NOT (x IN …)` is NULL for NULL `x`; use `coalesce` for category
  filters.
- pnpm installed with npm needs `npm i -g --allow-scripts=pnpm pnpm`, or its
  Windows shim breaks under `cmd`.
- Timeline payload layout is shared by `crates/core/src/query.rs` and
  `src/lib/ipc/timeline.ts`; change both together.
