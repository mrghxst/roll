# Roll

Desktop photo library (Tauri 2, Rust, Svelte 5). PROMPT.md is the spec;
docs/plan.md tracks milestones; docs/decisions.md records deviations.

## Layout

- `crates/meta` metadata read/write, `crates/media` decode/thumbnails/video,
  `crates/core` DB, scan, jobs, queries, Library facade, `crates/geo`,
  `crates/ai`. No Tauri dependency in `crates/*`.
- `apps/desktop/src-tauri` thin commands + `roll://` scheme;
  `apps/desktop/src` Svelte UI. `src/lib/ipc/api.ts` picks the Tauri or mock
  backend (`mock.ts` runs in a plain browser: `?setup=1&mock=N`; add
  `&pics=cc0` to show the CC0 test set from `tools/cc0` instead of
  gradients, which is how the README screenshots are made).
- `tools/fixtures` generates `fixtures-out/` (gitignored) with `manifest.json`.

## Commands

```bash
cargo test --workspace                     # Rust tests (fixture tests skip if fixtures-out is missing)
cargo run -p roll-fixtures                 # fixture library (~2 min; needs ffmpeg with x264/x265 and exiftool)
cargo run -p roll-fixtures -- --stress 100000 --stress-only
cargo test -p roll-core export_bindings    # regenerate TS bindings in src/lib/ipc/bindings
cd apps/desktop && pnpm check && pnpm test # svelte-check, vitest
cd apps/desktop && pnpm tauri dev          # the app; set ROLL_DATA_DIR to keep dev data apart
powershell -File e2e/immich/server.ps1 setup|start|stop|remove   # throwaway Immich in its own WSL distro
# then ROLL_IMMICH_URL/ROLL_IMMICH_KEY (from %LOCALAPPDATA%\RollImmichTest\credentials.json):
cargo test -p roll-core --test immich_real -- --ignored
# model tests need packs: cargo run -p roll-core --example packs -- fixtures-out/packs faces image-model
# then ROLL_PACKS=<absolute path to fixtures-out/packs> (tests run in the crate folder):
cargo test --release -p roll-core --test people_real -- --ignored
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
- `api.ts` picks its backend with a top-level `await import()`; modules it
  loads (`mock.ts`) must not import values from `api.ts` (circular import,
  the page never starts). Shared helpers live in their own modules
  (`ipc/query.ts`).
- MapLibre needs `requestAnimationFrame`; the desktop app's browser pane
  does not run it while hidden, so check the map with Playwright or the
  real app (WebView2 with `--remote-debugging-port`).
