# Decisions

Deviations from PROMPT.md, choices between alternatives, and open
questions. One line of reason each. Newest at the bottom of each section.

## Stack and tooling

- **TypeScript 6.0, not 7.0.** svelte-check 4.7 supports TypeScript 5 and 6
  only; TS 7 (the native compiler) is not supported by it yet.
- **ts-rs 12 instead of tauri-specta 2 RC.** ts-rs is stable; command
  wrappers are a small hand-written file (`src/lib/ipc/tauri.ts`) that
  mirrors `commands.rs`.
- **Fonts from `@fontsource-variable/geist` and `geist-mono` 5.3.** The
  `geist` npm package targets Next.js; Fontsource ships plain CSS and the
  same OFL fonts, bundled locally.
- **pnpm 12 via npm needs `--allow-scripts=pnpm`.** Without its install
  scripts, the Windows shim fails when invoked through `cmd` (Tauri's
  `beforeDevCommand`).

## Media

- **HEIC, AVIF and JPEG XL are decoded with FFmpeg, not libheif-rs.**
  libheif is LGPL and needs a dynamic DLL build on Windows (vcpkg); FFmpeg is
  already a bundled helper process (LGPL build), decodes HEIF grids and
  applies `irot`/`imir`, so nothing is rotated twice.
- **RAW previews come from the largest embedded JPEG, not rawler.** Nearly
  every RAW format embeds a full preview; the scan is format-independent and
  needs no extra dependency. TIFF-based files fall back to the image crate.
- **Own dHash instead of image_hasher.** It is 15 lines on the thumbnail and
  avoids a dependency pinned to an older `image`.
- **Thumbnails are 256 px on the short side (long side at most 640), not
  256 px on the long side.** Justified rows reach 300+ CSS px at high DPI; a
  256 px long side looks soft. About 10–20 KB each as WebP.
- **Truncated JPEGs are detected by their missing end marker.** Decoders
  pad missing data silently, so broken files would otherwise look fine.

## Metadata

- **Apple maker notes and QuickTime keys are parsed natively.** nom-exif
  returns the maker note as bytes; reading `ContentIdentifier` and
  `BurstUUID` directly avoids an ExifTool pass over every file.
- **Videos without an Apple creation date store QuickTime time as UTC and
  get the computer's zone as a guessed offset** (flagged `NO_OFFSET`), until
  a location gives a real zone.
- **In-file writes cover JPEG, HEIC, AVIF, PNG, TIFF, WebP, MOV, MP4 and
  3GP.** RAW, GIF, BMP and other video containers get sidecars only, which
  is safer for formats ExifTool rewrites less often.
- **Undo of a metadata write restores values, not bytes.** ExifTool puts
  the previous values back and the image data is unchanged (checked with
  `ImageDataHash`), but GPS degrees are re-encoded as DMS rationals and can
  differ from the original in the 11th decimal (well under a millimetre).
  Undo of moves, by contrast, is byte-identical.
- **Rotation writes EXIF orientation for JPEG, PNG, WebP and TIFF only.**
  HEIF rotation lives in the container (`irot`) and video rotation in the
  track matrix; Explorer and most viewers ignore EXIF orientation there,
  so rotating those would look wrong elsewhere. They are left alone, and
  the UI says so.

## Organize

- **Edits change the database first; files change only when a plan is
  applied.** Each edit records provenance and marks the item unsaved; the
  Organize view shows how many items have unsaved changes and writes them
  together with the moves, in one undoable batch.
- **The owner's rules run before the built-in ones** (priority 5, built-in
  rules start at 10), so a rule the owner writes can override a default.
  Rule patterns are checked when saved; an invalid pattern is refused with
  the regex error instead of silently never matching.
- **Copy to a new library folder is not offered yet.** Move in place is
  implemented and tested; copy mode needs its own root handling and is
  planned for M6 with the import work. The UI does not show the option.
- **Per-category "send to Immich" is stored but not shown** until Immich
  sync exists (M6), so the UI has no setting that does nothing.

## Places and dates

- **The offline basemap is Natural Earth country outlines** (the
  `world-atlas` package, public domain, 0.7 MB) instead of a Protomaps
  PMTiles world file. Getting the PMTiles extract means downloading from a
  build server, which needs the owner's go-ahead; the outlines need nothing
  and match the monochrome style.
- **Online detail tiles use Roll's own style over OpenFreeMap's planet
  tiles** (OpenMapTiles schema: water, parks, roads, rail, buildings,
  boundaries, place names in Noto Sans from OpenFreeMap's font server)
  instead of one of OpenFreeMap's ready-made styles, so they match the
  monochrome palette and follow the theme. They draw over the offline
  outlines, below the items. The CSP allows `https://tiles.openfreemap.org`
  for `connect-src` at all times; the map only asks it while Settings ›
  Network › Online map tiles is on. Offline regions remain a later
  milestone.
- **Place names install from GeoNames files the owner picks** (Settings
  has no download yet): `cities500.txt`, `admin1CodesASCII.txt` and
  `countryInfo.txt`. A download with consent comes with the pack manager
  (M4). The fixtures ship a small gazetteer in the same format.
- **Items without a stated UTC offset are matched to tracks on local time.**
  Their wall-clock time is what the camera showed; their UTC is a guess
  from the computer's zone, which is wrong when travelling. Track points are
  converted to local time with the zone at each point.
- **Date fixes shift by a fixed duration**, not a calendar amount: a clock
  that runs 366 days and 2 hours ahead is 366 days and 2 hours ahead for
  every photo. "Shift by an amount" uses calendar years because that is
  what the owner types.
- **Camera sets list every dedicated camera**, flagged or not; phones are
  listed only when something looks wrong. Visual pairing with phone photos
  (the smart offset) needs embeddings and lands in M4.
- **MapLibre's worker is bundled by Vite** and set with `setWorkerUrl`.
  MapLibre otherwise looks for it next to its own module, which bundling
  moves, and finds nothing on `tauri://` (macOS).

## Fixtures

- **HEIC fixtures are built by the generator itself.** FFmpeg has no HEIF
  muxer; the generator encodes HEVC to MP4 with FFmpeg and rewraps `hvcC`
  and the frame into HEIF, with a thumbnail item and an Exif item. The
  `iref` box must exist from the start: ExifTool mixes 16- and 32-bit item
  ids when it has to create one, which FFmpeg then rejects.
- **Basic EXIF in fixtures is written by a small TIFF encoder**, ExifTool
  writes XMP, QuickTime keys and RAW stand-ins. This keeps generation under
  two minutes and makes the 100k stress library possible.
- **HEIC fixtures come from a raw HEVC stream, not an MP4.** FFmpeg 9's
  MP4 muxer (Homebrew build on macOS) stopped accepting libx265's single
  frame ("not yet implemented"). The generator now asks FFmpeg for Annex B
  output and builds `hvcC` from the VPS, SPS and PPS itself.
- **Synthetic Apple maker notes always carry `MakerNoteVersion`.** An empty
  maker note directory makes ExifTool refuse to rewrite the file, which no
  real iPhone file triggers.
- **WhatsApp copies of camera photos are not rule-decidable.** Their name
  only sets `Source/WhatsApp`; the category follows from the camera
  original through duplicate detection (M4).

## UI

- **Click selects, double click opens** (desktop convention, as in Apple
  Photos on macOS and Windows Explorer). Enter opens, Space is quick look.
- **Short days share a row**, each with its own small header. Sparse
  libraries otherwise waste most of the width under day headers.

## Packs and models

- **PP-OCRv6 small instead of PP-OCRv5 mobile.** PaddleOCR released
  PP-OCRv6 in June 2026 and `oar-ocr` (0.7+) has a preset for it. The small
  pair (det 9.4 MiB, rec 20.2 MiB, 18,708-character dictionary) is the
  closest to v5 mobile in size, from the oar-ocr v0.7.0 release, whose
  GitHub asset digests are the SHA-256 values in `packs.json`.
- **GeoNames files carry no checksum.** GeoNames regenerates its dumps
  daily and keeps no dated copies, so a fixed SHA-256 would fail the next
  day. The place-names pack caps each file at four times its listed size,
  restarts instead of resuming, and the gazetteer build refuses files with
  no places. The SHA-256 of what was downloaded is still recorded in the
  pack's `pack.json`. A pinned alternative would be a processed
  `places.tsv` attached to a Roll release, which needs the owner's go-ahead
  for releases.
- **Downloads run on their own thread, not in the job queue.** Jobs run one
  at a time; a 1.4 GB model must not hold up scans and thumbnails. Packs
  report progress through a `packs` change event and show in Settings ›
  Models and data.
- **`packs.json` is compiled into the app.** The catalog and its checksums
  ship with each version, so a compromised server cannot change what Roll
  accepts. A pack is installed once its `pack.json` is written, after every
  file matched and the install step (the gazetteer build) succeeded.
- **The SigLIP 2 export is pinned to a commit** of
  `immich-app/ViT-B-16-SigLIP2__webli` (5f3fc29). The text model is 1.1 GB
  in fp32 because of the 256k-token Gemma vocabulary.
- **The Windows crash report uses an unhandled-exception filter**, not a
  vectored handler: vectored handlers also see first-chance exceptions that
  libraries catch on purpose. The report is written synchronously because
  the log writer's thread may not run again.
- **Private guard reasons never repeat what was found.** History says
  "Payment card number ending 4242" or "Machine-readable lines of a
  passport", so the database, the logs and an exported history hold no
  card, account or passport numbers. A passport name line alone (`P<UTO…`
  with `<<` fillers) counts, because OCR often loses the second line.
- **Private items move only after review** (review state 3, which a
  manual category sets). The plan counts them as "Private, not reviewed".
- **One WebP encode runs before any parallel ones.** libwebp picks its
  encoder routines on first use and, on Windows, does so without a lock;
  threads encoding at the same moment could run on pointers another thread
  was rewriting (a crash in `VP8PutBit`). A `Once` warm-up in
  `roll_media::thumb::webp` finishes that setup first.

## Add photos

- **Staged items are `missing = 2`.** The folder being added is a root of
  kind `staging`; its items are on disk but not part of the archive. Every
  archive view already filters on `missing = 0`, so they stay out without a
  new condition; indexing (thumbnails, hashes, groups, sorting rules) uses
  `missing != 1`. `Query.staged` shows one import's items.
- **Smart offset pairs by dHash until the image model is installed.**
  Look-alikes are phone photos within ±48 h whose 64-bit dHash is within
  10 bits; the offset is the median of the pairs that agree within 3 min.
  On the fixture card it proposes +9 h 28 min (true: 9 h 30 min; the camera
  shots follow the phone shots by 20 s to 3.5 min). SigLIP 2 will replace
  the pairing; the rest stays.
- **Imports land where Organize would put them.** The event folder follows
  the planner's rule (`{year}/{yyyy-mm} {event}`), so a later plan leaves
  the items alone; a free template for imports is not offered.
- **Applying an import is the owner's decision for its unsorted items.**
  Items sorted as something other than photos go to their category
  folders; everything else goes to the event folder and becomes Photos,
  recorded in the batch with the reason.
- **"Already in the library"** means the same BLAKE3, or a dHash within
  2 bits with the same aspect ratio and an archive copy at least as large.
  Missing hashes on either side are computed when the import is checked.
- **Undoing an import removes a copy only when it still matches the
  original byte for byte.** A copy changed since goes to the Recycle Bin.
  An applied import keeps its staging root after closing, so the batch
  stays undoable; undo reopens it for checking.
- **WhatsApp exports: the chat text is the date source.** Media in a chat
  export has no capture date; the chat line that attached it gives the
  sending time (to the minute on Android, the second on iPhone), recorded
  as a date edit with source `chat` and the sender in the reason. The date
  order (day or month first) comes from values above 12, else from the
  dates in the file names. Senders become `From/<name>` tags, so the owner
  can find what a person sent; a file sent as a document keeps its camera
  date.
- **Takeout JSON values are written into the copies** (dates in the zone
  of the JSON place, GPS), so the JSON files are not copied along; without
  ExifTool they are, so nothing is lost.
- **Exports are not one event by default.** A Takeout or chat export spans
  years: items sorted as something go to their category folders, items not
  sorted yet wait in `Imported/<folder>` for Review and Organize. The
  owner can still add any import as one event.

## Immich

- **`PATCH /assets` for later edits.** Immich 3.x marks `PUT /assets`
  deprecated in favor of an unlisted `PATCH` with the same body; Roll uses
  the `PATCH` and falls back to `PUT` on a 404 or 405.
- **What was pushed is stored per item** (`assets.immich_synced`, JSON) and
  compared as text: parsed floats can differ in the last bit, which made
  every geotagged item look changed.
- **After an upload, saved metadata counts as pushed.** A file whose
  changes are saved (and its `.xmp` sidecar, which is uploaded with it)
  already tells Immich the same; items with unsaved changes get the update
  step in the same sync.
- **Items in Roll's trash leave Immich too**, with the same confirmation as
  Keep local only. Both go to Immich's trash, never a forced delete.
- **The CLI script names folders one by one** (no `--recursive`): Roll
  knows which folders hold chosen items, and categories nest folders.
  Event folders use `--album-name <event>`.
- **The server address is a setting; only the API key is in the keychain.**
  The address is not a secret and the UI shows it; PROMPT 4.10 puts both in
  the keychain.
- **Albums are shared as viewers.** The people the owner picks can see and
  download, not change the album.

## Installer

- **The installer is per-user NSIS only** (no MSI): it needs no
  administrator rights. It installs to `%LOCALAPPDATA%\Programs\Roll`, not
  Tauri's default `%LOCALAPPDATA%\Roll`, which is the data folder section 7
  fixes; `src-tauri/windows/hooks.nsh` changes the default. Tauri's
  uninstaller option "Delete the application data" removes
  `%LOCALAPPDATA%\io.github.mrghxst.roll`, which Roll does not use, so
  uninstalling always keeps the library database.
- **Bundled tools come from pinned downloads at build time**, not from the
  repository: ExifTool 13.59 (Windows 64-bit package from SourceForge, the
  checksum from exiftool.org) and BtbN's LGPL shared FFmpeg 8.1 build
  (GitHub's asset digest). `ffplay.exe` is left out. The normal
  `tauri.conf.json` bundles nothing, so `pnpm tauri build` without
  `src-tauri/tauri.bundle.json` still works for development.
- **The installer is not signed and has no updater yet.** Signing needs a
  certificate (money) and releases need the owner's go-ahead (9.4).

## Decision models (M5)

- **A crash can cost one batch per worker twice.** Items are marked sent
  before their request and done with the reply in one transaction. Answered
  items are never sent again; items still marked sent after a crash go
  again, because OpenRouter's generation id (to look up a charge) only
  arrives with the reply. At most 8 batches are in flight. Stopping from
  Activity waits for the batches in flight, so a stop costs nothing twice.
- **"The cap stops the run exactly" means it is never passed.** Before a
  batch is sent its worst case (twice the image and hint tokens, every
  output token up to `max_tokens`) is reserved against the budget; the run
  stops when the next batch could pass it, so it may end a few cents
  under the cap.
- **OpenRouter routing:** `provider: {zdr: true, data_collection: "deny",
  require_parameters: true, sort: "price"}`; `zdr` is left out only when
  the owner allows providers that keep data. The estimate uses the
  cheapest ZDR endpoint's price from `/endpoints/zdr`, which `sort: "price"`
  routes to. `reasoning: {effort: "none", exclude: true}`, retried with
  `"minimal"` when a model rejects "none".
- **Model answers are proposals.** They go to Review (never auto-accepted),
  replace a rule's proposal only with higher confidence, and never touch
  owner decisions or certain rules. Source tags (`Source/Instagram`…) are
  added at 80% confidence or more; a "private" answer marks the item
  Private through the same path as the text guard, so the owner's own
  "not private" wins.
- **Local servers need no Network switch.** They answer on the loopback
  address; nothing leaves the machine. Detection tries Ollama (11434), LM
  Studio (1234) and llama.cpp (8080).
- **The model check compares batch sizes by hand.** It runs at the
  "Items per request" setting and the report shows the batch size per
  model; checking a model again at 4 or 1 shows whether batching hurts it.
  There is no automatic fallback yet.
- **The acceptance numbers come from a stand-in server** that charges by
  token counts like OpenRouter. The real 1,000-item check needs the
  owner's go-ahead to spend money and send thumbnails.
