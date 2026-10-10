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

## Fixtures

- **HEIC fixtures are built by the generator itself.** FFmpeg has no HEIF
  muxer; the generator encodes HEVC to MP4 with FFmpeg and rewraps `hvcC`
  and the frame into HEIF, with a thumbnail item and an Exif item. The
  `iref` box must exist from the start: ExifTool mixes 16- and 32-bit item
  ids when it has to create one, which FFmpeg then rejects.
- **Basic EXIF in fixtures is written by a small TIFF encoder**, ExifTool
  writes XMP, QuickTime keys and RAW stand-ins. This keeps generation under
  two minutes and makes the 100k stress library possible.

## UI

- **Click selects, double click opens** (desktop convention, as in Apple
  Photos on macOS and Windows Explorer). Enter opens, Space is quick look.
- **Short days share a row**, each with its own small header. Sparse
  libraries otherwise waste most of the width under day headers.
