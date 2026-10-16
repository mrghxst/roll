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
- [x] Immich round trip (see M6)
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
- [x] First real downloads (2026-10-14, approved by the owner): SigLIP 2
      (1.5 GB), PP-OCRv6 small (30 MB) and GeoNames (14 MB) through the
      pack manager with checksums (`crates/core/examples/packs.rs`)
- [x] Private guard, text part: Luhn-valid card numbers, IBANs (mod 97),
      passport and ID machine-readable lines (check digits); marks items
      Private with a reason that never repeats the number; the owner's own
      "not private" wins; Organize holds Private items until reviewed.
      Runs on descriptions now and on OCR text once the reader is in
- [x] Embedding store (f16 in SQLite), in-memory brute-force index
      (100k × 768 in 12 ms), hybrid ranking: FTS5 BM25 plus image
      similarity fused with RRF, filters applied to both; OCR text table
      feeding the search index
- [x] ONNX Runtime through `ort` (DirectML on Windows, Core ML on macOS,
      CPU on Linux); SigLIP 2 image and text towers, embeddings from the
      256 px thumbnails in a background job
- [x] Image-model sorting: zero-shot from category descriptions (best
      matching description per category) plus the owner's sorted items
      (k nearest neighbours), calibrated confidence, auto-accept at the
      category threshold (97%)
- [x] **Acceptance:** CC0 set, local only: 68% sorted on their own at
      97.2% precision (target 60% at 97%); 157 items in 11 s on the
      owner's GPU (`crates/core/tests/vision_real.rs`)
- [x] Semantic search: pictures that match the words (similarity 0.09 or
      more, within 0.035 of the best) join the text matches; "dog",
      "receipt", "nebula" and a word only in a chat screenshot are found
- [x] Text reader: PP-OCRv6 small (detection, line boxes, CTC) on the CPU
      for screenshots, documents, memes and saved posts; feeds search and
      the Private guard
- [x] Teach: examples of a new or existing (sub)category train a linear
      classifier; proposals in Review, retrained on corrections; Find
      similar
- [x] Discover: look-alike groups of undecided items next to the
      signature clusters, named by the closest category description
- [x] Smart time offset by look: camera and phone photos of the same
      scene pair by embedding (0.88) as well as dHash; in Dates and Add
      photos
- [x] Cluster names from the decision model: "Suggest a name" in Discover,
      one call with up to 9 pictures (320 px, no metadata); a new category
      with the name and its definition, or the existing one that fits
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
- [x] Real local run (docs/benchmarks.md): qwen3-vl:8b-instruct on the
      RTX 2080 Ti, 95.5% on the 157-item CC0 set, 0.48 items/s
- [x] CC0 test set (PROMPT 9.1): `tools/cc0/build.mjs`, licenses in its
      manifest, reviewed labels; more documents still to add
- [x] The same acceptance against OpenRouter (2026-10-13, owner's
      short-lived key): Gemma 4 26B with zero data retention, 980 items
      for $0.0375 (estimate within 10%, under the $0.10 cap); 92.4% on the
      CC0 set (docs/benchmarks.md)
- [x] Natural-language search: words the offline parser leaves go to the
      text model on request ("Ask AI about these words"); its answer
      becomes filter chips, unknown places stay as words
- [x] Best matches and "Better ranking" (2026-10-15): the 50 best matches
      of a search (RRF of text and picture ranks) in a sheet; the decision
      model rates them 0–10, 8 pictures (320 px) per request. Measured on
      the CC0 set (docs/benchmarks.md), which also found and fixed tag roots
      like `Category/` matching searches
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
- [x] Immich round trip against a real server (9.3): Immich 3.3.1 in its
      own WSL distro with Docker (`e2e/immich/server.ps1`, removable in one
      step), `crates/core/tests/immich_real.rs`: uploads, Live Photo link,
      tags, date, place, shared event album, no double uploads, later
      rating, tag, description, place and date edits, trash only after
      confirmation with the local file untouched. Found and fixed: linking
      a known still skipped its Live Photo video, and an item sent again
      after "Keep local only" stayed in Immich's trash
- [x] Round trip, rest of 9.3 (2026-10-15): re-foldering the archive
      with metadata writes between syncs sends nothing again and links
      nothing new; the official Immich CLI (3.2.4) uploading the same
      archive for a second user gives the same rating, description, place,
      date and tag as Roll's sync
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
- [x] Map polish (owner, 2026-10-13): minimal globe, time slider as a
      floating panel on the right, pins centred on their clusters and
      showing the best item (favourite photos first), no more polar fill
      artifacts; README with screenshots from the mock backend and the
      CC0 set

## M7 Later

Owner requests (2026-10-13), to build after M4–M6:

- [x] **Deduplicate identical photos in bulk** (2026-10-14): "Merge all
      exact copies" with a preview (groups, copies, space, sample paths);
      among identical files the copy in the owner's folders wins (not
      Downloads, backups or chat exports, no " (1)", "copy" or "Kopie"),
      then the shortest path; one batch, one undo; dates, places and tags
      move to the kept file. Originals stay untouched
- [x] **Pick the best shots of a series** (2026-10-14): bursts and
      look-alike groups ranked on their previews by sharpness (Laplacian),
      exposure (brightness, clipping) and, with the image model, look and
      clearly closed eyes; the best one or two suggested, any frames can be
      kept, the rest to Roll's trash with undo. Never re-encodes
- [x] **People** (PROMPT M7, 2026-10-15): the faces pack (YuNet and SFace
      from the OpenCV Zoo, 39 MB, MIT and Apache-2.0) finds faces on the
      previews; a face joins the person it is most like (0.42 to their
      mean), new people need three alike faces in three items, two faces
      in one item are never one person. People view: name, merge, hide,
      "Not this person" (never grouped with them again), their photos in
      the timeline; the info panel shows who is in a photo and asks "Who is
      this?". All undoable. Names are searchable and go into the files as
      MWG regions (relative to the stored pixels, merged with regions
      other apps wrote, taken out again on undo) for Immich's face import.
      On the CC0 family photos: 22 faces, the Chinde Point family as people
      across photos, regions checked with ExifTool
      (`crates/core/tests/people_real.rs`)
- [x] Faces for the best shot: with the faces pack, how sharp the faces
      are counts as much as the whole frame's sharpness ("Sharpest faces")
- [x] **One-click local AI** (PROMPT M7, 2026-10-15): the `local-ai` pack
      per system (llama.cpp b11372: Vulkan on Windows and Linux, Metal on
      macOS; Qwen3-VL 4B Q4_K_M and its projector, 3 GB, checksums
      pinned); "Set up local AI" in Settings › AI and Sort with AI makes it
      the decision model; Roll starts `llama-server` on 127.0.0.1:8766 when a
      model is needed and stops it when idle or on exit
- [x] Packaging polish for macOS and Linux (2026-10-15): the `.dmg` brings
      ExifTool, the `.deb` and AppImage bring ExifTool and an LGPL static
      FFmpeg (`scripts/fetch-tools.sh`, pinned checksums); Roll finds tools
      in the app's Resources and in `lib/Roll`. macOS FFmpeg stays Homebrew's
      (no LGPL build to ship)
- [x] **Offline map areas** (PROMPT M7, 2026-10-15): "Offline map areas" on
      the Map copies the area on screen (zoom 0 to 13, up to 20,000 tiles)
      from the newest Protomaps build with range requests (a PMTiles reader
      in `crates/geo`, six requests at a time) into `<data>/maps/<id>.db`,
      with Noto Sans glyphs (pinned checksums); Roll serves them at
      `roll://maptile` and the map draws them in its own style while the
      online tiles are off. Lake Garda: 326 tiles, 17 MB, 14 s
- [ ] Signed builds with automatic updates (needs certificates and an
      update key from the owner)
- [ ] Open or closed eyes from a landmark model (the image model's guess
      stays), and duplicates across imports before they are added

## Next

M4 to M6 and M7 are in, except signed builds with updates (needs
certificates and keys from the owner) and open or closed eyes. 0.2.0 is
published as a pre-release (2026-10-16) for Windows, Linux and Apple
silicon Macs. Left by hand: the checks in docs/checks.md (the macOS dmg
has not been started on a Mac yet).
