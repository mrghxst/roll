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
  and match the monochrome style. Detailed online tiles are still to come
  as an opt-in.
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
