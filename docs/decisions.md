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
- **The world map is a download, not part of the installer** (PROMPT
  §4.7 asks for a Protomaps world file, zoom 0–6, shipped with the app).
  Map › Map detail fetches it from the Protomaps build with the owner's
  click (63 MB, zoom 0–6: lakes, large rivers, motorways and trunk roads,
  cities), so the installer stays at its size and nothing is fetched
  unasked. Zoom 7 would add main roads but is 285 MB. The map offers it
  while it has only the outlines. Parks and woods are drawn from zoom 10
  only: further out they blot the land like lakes.
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
- **Search asks the text model only on request.** After Enter, words the
  offline parser left show an "Ask AI about these words" button; only
  those words, today's date and the category names are sent. Without a
  separate text model the decision model answers. These requests cost a
  fraction of a cent and are not counted in run totals.
- **Model lists are sorted by quality, highest first** (owner's request,
  2026-10-13). Quality is the model check's accuracy on the owner's items
  when there is one; otherwise a coarse tier (Best, Very good, Good,
  Basic) from a curated table in `crates/ai/src/catalog.rs`, built from
  the official model cards where they publish figures (Gemma 3 MMMU,
  Gemma 4 MMMU-Pro) and reported Qwen3-VL figures. Vendors report
  different benchmarks, so tiers are deliberately coarse and each row
  says where its figure comes from. Models Roll does not know are "Not
  rated" and listed after rated ones, cheapest first. Local models show
  their download size and the graphics memory they need; ones larger than
  the card go last. Cloud models show their price per 100,000 items and
  how the provider handles data (zero retention or not, the provider's
  country and privacy policy from OpenRouter's `/providers`).
- **Batches shrink when a server's context is too small.** A 400 that says
  the request exceeds the context halves the batch and lowers the batch
  size for the rest of the run. Ollama's default context (4,096 tokens)
  holds 2 Qwen3-VL images. Local requests also get 2,048 extra output
  tokens, so a model that reasons before answering still finishes.
- **Thinking variants rank one tier lower.** They give the same answers
  several times slower, and Ollama cannot turn their reasoning off through
  the OpenAI endpoint. Qwen3-VL's plain tags are thinking variants; the
  list suggests the `-instruct` tags.
- **Prompt `decide-2` and sharper built-in definitions.** The first real
  run took chat screenshots for social media; the definitions now say
  what each category looks like (a whole-screen capture, a post in an
  app's frame, a caption added to a photo). Migration M11 updates only
  definitions the owner has not changed.
- **The CC0 set is built, not committed.** `tools/cc0/build.mjs` fetches
  from Openverse (CC0 and Public Domain Mark only) and renders the rest;
  only the script and the reviewed corrections (public image URLs) are in
  git. Each item's source, creator and license are in its manifest.
- **The map is a globe and the time slider sits on the right** (PROMPT 4.7
  says left). The owner suggested both (2026-10-13). The slider is a
  floating panel over the right edge of the map, so the app's sidebar is
  the only column on the left; year labels sit on the time axis. MapLibre
  flattens the globe to Web Mercator as you zoom in. Natural Earth rings
  that cross the 180° meridian are unwrapped before they reach MapLibre,
  or they fill the polar oceans on the globe. Box queries return nothing
  on the globe, so thumbnail pins come from the source's own features.
- **First releases are unsigned and without the updater** (PROMPT 9.4
  asks for Tauri updater metadata signed with the owner's minisign key).
  There is no key yet, so `release.yml` publishes the installers and
  SHA256SUMS as a pre-release; Windows shows SmartScreen and the Mac app
  is signed ad hoc, not notarized. The updater follows once the owner has
  a key.
- **Only the Windows installer bundles ExifTool and FFmpeg.** The deb
  depends on the distribution's packages, the AppImage and the Mac app use
  the system's (Homebrew) tools. Roll also looks in `/opt/homebrew/bin`,
  `/usr/local/bin` and `/usr/bin`, because apps started from the Dock or a
  menu do not get the shell's `PATH`.
- **The Immich round trip runs on a throwaway server in its own WSL
  distro** (`e2e/immich/server.ps1`) rather than Docker Desktop: no
  administrator rights, nothing added to the owner's other distros, and
  `remove` deletes all of it. Machine learning is left out. The test
  asserts through Immich's API and only adds data, so it can run again.
- **Image embeddings come from the 256 px thumbnails, and videos from
  their poster frame** (PROMPT 5.1 asks for 3 keyframes per video). The
  thumbnails exist already, so embedding 100k items reads no originals;
  SigLIP 2 sees 224 px. Keyframes for videos can follow.
- **Image-model sorting is zero-shot plus nearest neighbours, not
  logistic regression per category.** Zero-shot uses the best-matching
  description per category (89% on the CC0 set against 82% for the mean);
  once the owner has sorted 20 items, their 9 nearest sorted neighbours
  vote too. Logistic regression is used for taught categories (Teach),
  where examples are few and specific.
- **The confidence curve was calibrated on the CC0 set** it is then
  measured on (68% auto-accepted at 97.2%). There is no second labelled
  set yet; the model check (5.4) on the owner's own items is the honest
  measure and should replace it.
- **Search results stay in date order.** The image side joins the text
  side as a set (similarity 0.09 or more and within 0.035 of the best
  match) instead of an RRF-ranked list, because the timeline, map and
  facets all filter one query. `embeddings::hybrid` (RRF) is kept for a
  "best matches" order later.
- **The text reader runs on the CPU.** DirectML was 4–6× slower for the
  small, variable-width OCR models (180–500 ms per screen on the CPU).
  The image model uses the graphics card.
- **DirectML.dll ships next to Roll.exe**; ONNX Runtime itself is linked
  in (`ort` download-binaries), so there is no separate runtime install.
  `scripts/fetch-tools.ps1` copies it into `bundle-tools`: as a resource
  in `target/release` it was copied onto itself (os error 32 in CI).
- **macOS builds are Apple silicon only, Linux builds need glibc 2.38**
  (Ubuntu 24.04, Debian 13, Fedora 39): `ort`'s prebuilt ONNX Runtime has
  no Intel Mac library, and its Linux library uses glibc 2.38 symbols
  (`__isoc23_strtol`), so the Linux packages are built on Ubuntu 24.04.
  Building ONNX Runtime from source for both would take about an hour a
  release; Intel Macs are no longer sold.
- **Best matches are a separate list.** The timeline keeps date order
  (above); the list icon next to the search opens the 50 best matches
  (`embeddings::hybrid`, RRF of the text index and picture similarity,
  limited to what the search shows). "Better ranking" uses the decision
  model, not Qwen3-VL-Reranker: it is already set up, and a GGUF reranker
  would need llama.cpp in the installer. 8 pictures per request at 320 px;
  the model gives each 0–10. On OpenRouter that is about $0.002 for 50
  pictures with Gemma 4 26B ($0.000038 a picture measured in M5), over the
  spec's $0.001; ranking 25 would meet it, but 50 is what the spec asks
  to rerank.
- **People from YuNet and SFace.** The OpenCV Zoo models the spec names
  (MIT and Apache-2.0, 39 MB, 25 ms to find faces and 2 ms a face on the
  CPU/GPU). Thresholds: a face joins a person at 0.42 cosine to the mean
  of their faces, new people start from faces 0.45 alike; OpenCV's own
  "same person" threshold is 0.363, so grouping is a little stricter and
  the owner merges what it misses. Three faces in three items make a
  person (as in Immich). Faces under 36 px on the 1024 px preview are
  kept but not recognized.
- **Face regions are written relative to the stored pixels** (MWG; Immich
  turns them by the EXIF orientation on import). Roll keeps every region
  other apps wrote; it replaces a region that covers one of its named
  faces, and one on a face it found whose name it once wrote
  (`people_names`), so a name taken away or undone also leaves the file.
- **People changes are a history field** (`people`: each face's person,
  how it got there, and that person's name), so naming, merging and "Not
  this person" undo like any edit. A merged person keeps its row so undo
  can bring it back.
- **Search ignores Roll's own tag roots.** `Category/`, `Event/`,
  `Source/` and `From/` are not indexed as words; "cat" matched every
  sorted item through "Category" before.
- **Local AI is llama.cpp's own server**, run by Roll, not a library built
  into Roll: the official builds cover Vulkan (any graphics card on Windows
  and Linux) and Metal, need no compiler toolchain in CI, and keep the
  installer small (the server comes with the 3 GB model, which is a
  download anyway). Packs can now be listed once per system under one id.
  The server is started on demand and stopped after 10 idle minutes (its
  `/slots` endpoint tells whether a request is still running) so it does
  not hold graphics memory while Roll is idle. Qwen3-VL 4B rather than 8B:
  it fits 6 GB cards and is twice as fast (91.1% at 0.95 items/s against
  95.5% at 0.48 for 8B on the CC0 set); 8B stays the better choice
  through Ollama.
- **Offline map areas come from the Protomaps builds**, read with range
  requests (their documented use, like `pmtiles extract`), not from
  OpenFreeMap, whose free server should not be crawled tile by tile.
  An area is capped at 20,000 tiles (about 1 GB), since a tile is about
  50 KB, and goes down to zoom 15 (every building and footpath, the
  builds' deepest level) when that fits, else to 14 or 13 (streets and
  larger buildings: a zoom 13 tile in Lisbon has 12 of the 850 buildings
  zoom 15 has). One map source serves all areas up to zoom 15: Roll
  enlarges an area's deepest tile for the zooms it lacks
  (`roll_geo::mvt::overzoom`: the child's quarter, coordinates scaled,
  features outside it dropped, the map clips the rest). Each area is its
  own SQLite file, so removing one never touches another. The tiles' own
  land (and, online, their seas from zoom 8) is drawn over the Natural
  Earth outlines, which put coastal city centres like Lisbon's in the
  water.
  Labels need glyphs: Noto Sans for Latin, Greek and Cyrillic (OFL,
  560 KB, pinned checksums) comes with the first area; other scripts stay
  unlabelled offline.
- **Closed eyes come from MediaPipe's blendshapes**, run as ONNX
  (Google's Face Mesh V2 and face blendshape models, Apache-2.0, converted
  with a parity check by the QtMeshEditor project and pinned by commit and
  checksum), on the faces YuNet already finds. A face counts as closed
  when its two `eyeBlink` scores average 0.5 or more: on 99 labelled
  faces that beat the eye aspect ratio from the same mesh (36 of 49) and
  either eye alone (33 of 49) at one false alarm in 50. Other options
  were weaker: no eye-state model in the OpenCV Zoo, InsightFace's
  landmarks are for non-commercial use, and the image model's whole-frame
  guess cannot say whose eyes are closed. It stays as the fallback
  without the pack.
- **Jev (TypeSafe) is not used yet** (2026-10-16). It answers typed
  questions with calibrated probabilities, which would suit sorting, but
  images are an experimental "danger zone" feature, it needs its own
  TypeSafe account and key (OpenRouter lists only a router on top of it),
  and zero data retention is for enterprise customers only.

