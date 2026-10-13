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
- [ ] Immich round trip (needs Docker; see M6)
- [x] Rare access violation during thumbnailing: libwebp's encoder setup
      races on Windows (no lock in `WEBP_DSP_INIT`); one warm-up encode
      before parallel use fixes it. Found with the test's fault handler
      under 4× parallel load (1 in about 50 runs before, 0 in 88 after).

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
- [x] Opt-in online detail tiles (OpenFreeMap, own monochrome style,
      nothing fetched while off; tested with the host intercepted), place
      names download with consent (M4 pack manager)
## M4 Local AI

- [x] Review: proposals per category, most doubtful first, keys 1–9, X,
      Enter, Space, Z; accept all at 95% or higher; 1% audit with a
      precision estimate
- [x] Duplicates: exact (BLAKE3 on same-size candidates), smaller copies of
      camera photos, look-alikes (dHash), bursts; keep-best and merge
- [x] Discover by source signature (the 854×1590 fixtures form one
      cluster); assign, new category, split by folder, ignore
- [x] Search: FTS5 index and the deterministic parser, chips on Enter,
      under 300 ms on 100k
- [x] Pack manager: `packs.json` with URL, size, SHA-256 and license;
      consent sheet with size, hosts and license; resume after stop;
      checksum check; Settings › Models and data; place names download
      from the Places page (tested against a local server)
- [ ] First real downloads: waiting for the owner's go-ahead
- [x] Private guard, text part: Luhn-valid card numbers, IBANs (mod 97),
      passport and ID machine-readable lines (check digits); marks items
      Private with a reason that never repeats the number; the owner's own
      "not private" wins; Organize holds Private items until reviewed.
      Runs on descriptions now and on OCR text once the reader is in
- [x] Embedding store (f16 in SQLite), in-memory brute-force index
      (100k × 768 in 12 ms), hybrid ranking: FTS5 BM25 plus image
      similarity fused with RRF, filters applied to both; OCR text table
      feeding the search index
- [ ] SigLIP 2 embeddings (GPU and CPU), semantic search with RRF,
      zero-shot and learned classifiers, Teach, embedding clusters in
      Discover, smart time offset, OCR and the Private guard
## M5 Decision models and cost

- [x] OpenAI-compatible client for local servers (Ollama, LM Studio,
      llama.cpp, found on their default ports, models from `/v1/models`)
      and OpenRouter; retries with jitter on 429/5xx and timeouts
- [x] Model picker: image-capable OpenRouter models with live prices,
      cheapest ZDR endpoint per model, cheapest first, ZDR filter (models
      that keep data only when the owner allows them)
- [x] Decision prompt `decide-1` with hints (never names, paths, places or
      dates), 512 px JPEG without metadata, video strips of 3 frames,
      8 per request (4 or 1 in Settings), JSON schema, temperature 0,
      reasoning off; replies checked, malformed ones retried once then
      unsure
- [x] Runs: estimate and consent text before each run, per-run budget
      reserved per batch so the spend never passes it, live cost in
      Activity, a note when costs run 20% over the estimate, adaptive
      concurrency (4, up to 8, halved on throttling), resumable per item;
      answers become proposals in Review, captions go into search, source
      tags at 80% or more, private documents marked Private
- [x] Model check: a stratified sample of about 150 items (owner-reviewed
      ones come labelled), labelling with keys 1–9 and Enter, up to 3
      models compared on accuracy, cost per 100k and speed, the cheapest
      within 2 points recommended
- [x] Acceptance against a stand-in server (`crates/core/tests/decide_runs.rs`):
      estimate within ±25% on 1,000 items, $2.43 per 100k with the default
      settings and the spec's Gemma 3 12B prices, the cap never passed,
      resume without sending an answered item again
- [ ] The same acceptance against OpenRouter: a 1,000-item run costs
      about $0.03 and needs the owner's go-ahead (spending, data off the
      machine)
- [x] Natural-language search: words the offline parser leaves go to the
      text model on request ("Ask AI about these words"); its answer
      becomes filter chips, unknown places stay as words
- [ ] Optional reranking (Qwen3-VL-Reranker or the decision model)
## M6 Events, Immich, polish

- [x] Events: detection (4 h / 12 h gaps, 50 km from home, 8 items,
      trips merged, file-time and suspicious dates ignored), home from
      night photos or the setting, names from the gazetteer; Events view
      with confirm, rename, merge, split, dismiss and own folder, all
      undoable; on the fixtures: Italy 2019, New York 2021, Lisbon 2022,
      the Dolomites hike, Christmas 2018 at home
- [x] Immich sync through the v3 API (checked against the 3.3.1 OpenAPI
      spec): plan first (send, update, new albums, removals that need a
      tick), checksums before upload, Live Photo pairs, later edits and
      tags pushed, event albums with sharing, Locked for private items,
      Immich's trash only after confirmation, resumable per item, History
      entry. Send to Immich / Keep local only on any selection. CLI script
      export without the key. Guide in docs/immich.md. Tested against a
      stand-in server (crates/core/tests/immich_sync.rs)
- [ ] Immich round trip against a real server in Docker (9.3): waiting for
      the owner's go-ahead to use Docker
- [x] Add photos (4.11): a folder is indexed read-only as a staging set;
      source facts and items already in the library (skipped by default);
      dates against the library by day, smart offset from look-alike phone
      photos, the Fix dates tools on the set; places from the library's
      photos at the same time, one place for the rest or per day; event
      name from the folder or "{Month YYYY} {Place}"; copy (verified) or
      move into the event folder with metadata written, one batch in
      History with undo; drop a folder onto the window to start. On the
      fixture card: dates within 4 min, 40 of 40 placed, own folder;
      a camera card or phone that mounts with a DCIM folder offers Add
      photos; a mini map of the placed items
- [x] Add photos: the event's Immich album (on by default) and who to
      share it with, made at the next sync
- [x] Takeout and WhatsApp importers, through Add photos: a chat export
      (Android or iPhone, either date order, 12- or 24-hour clock) gives
      each file its sending time and sender (`Source/WhatsApp`,
      `From/<name>`); a Takeout export gets its JSON dates in the place's
      time zone, `Album/<name>` tags, and album repeats added once; both
      are written into the copies. Exports are not one event by default:
      sorted items go to their category folders, the rest wait in
      `Imported/<folder>`
- [x] First guide after indexing: seven steps from review to the first
      Immich sync, each opening its view, progress kept in settings, back
      from the command palette
- [x] Accessibility: axe (WCAG 2.1 A/AA) over every view in Playwright,
      light and dark; the photo grid is a multi-select listbox with
      dated item labels and an active descendant; contrast and labels fixed
- [x] Windows installer: per-user NSIS with ExifTool 13.59 and an LGPL
      FFmpeg 8.1 (pinned checksums, `scripts/fetch-tools.ps1`), installed
      to `%LOCALAPPDATA%\Programs\Roll`; installed, run and uninstalled
      silently (docs/checks.md); the pages by hand are pending
- [x] User docs: docs/user-guide.md, docs/immich.md
- [x] Place suggestions from events: when most of an event is geotagged,
      its centre (with the radius that holds 80% of it) for the rest
## M7 Later

## Next

M4 models: waiting for the owner's go-ahead to download SigLIP 2 (1.4 GB),
PP-OCRv6 small (30 MB) and GeoNames (13 MB) through the pack manager, and
ONNX Runtime with the `ort` crate. The store and ranking they feed are
ready.
