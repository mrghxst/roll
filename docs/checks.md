# Checks

Results of acceptance checks that are not plain unit tests. Automated ones
name their test; checks by hand say who ran them and when.

## M2 Organize core

Automated in `crates/core/tests/organize_acceptance.rs`, on a copy of the
fixture library (884 files):

- **Rule-decidable fixtures land in the right folders.** All 608 items the
  manifest marks rule-decidable get the expected category without review
  and end up in `{year}/{Category}` after apply.
- **Killing the app during apply.** The apply runs in a child process that
  is killed after about a third of the 629 operations. After restart,
  recovery finishes the batch: every operation is done, every asset and
  sidecar is where the database says, and the multiset of file hashes is
  unchanged (nothing lost, duplicated or altered; no partial copies left).
- **Undo of moves restores a byte-identical tree**: same paths, same
  BLAKE3 hashes, same folders, including folders removed when they emptied.
- **Undo of metadata writes** on JPEG, HEIC, PNG, MOV and MP4: rating,
  tags, date and GPS read back after apply; after undo the previous values
  are back and `ImageDataHash` is unchanged throughout. GPS can differ in
  the 11th decimal (see decisions).

### Explorer (2026-10-11, automated through the Windows property system)

Roll's writer was run on copies of fixture files
(`cargo run -p roll-meta --example write_check -- <folder>`) with date
2019-08-12 11:00 +02:00, tags `Trips/Lake Garda` and `Category/Photos`,
rating 4, a description and a location. The values were then read through
the Shell property system, which is what Explorer's Details pane and
columns use:

| File | Date taken / Media created | Tags | Rating | GPS | Title |
|---|---|---|---|---|---|
| JPEG | 2019-08-12 11:00 local | Lake Garda; Photos | 4 stars | 45°40′44″ N, 10°39′15″ E | yes |
| HEIC | 2019-08-12 11:00 local | Lake Garda; Photos | 4 stars | same | yes |
| PNG | 2019-08-12 11:00 local | Lake Garda; Photos | 4 stars | not shown (Windows reads no GPS from PNG) | yes |
| MP4 | Media created 2019-08-12 11:00 local | not shown | not shown | same | no |

Explorer shows tags and rating from XMP only for image formats; section 6
asks for "Media created" on videos, which is shown. This check found that
PNG files kept an older `XMP-photoshop:DateCreated` from an editor; Roll
now writes the XMP date and GPS fields into photos as well as EXIF.

### Photos app (by hand, pending)

Not yet checked: access to the Photos app was not granted for this
session. To check:

1. `cargo run -p roll-meta --example write_check -- %TEMP%\roll-check`
2. Open `IMG_1001.JPG` and `IMG_1002.HEIC` from that folder in Photos and
   open the info pane (`Alt+Enter`).
3. Expect: date 12 August 2019 11:00, the location near Lake Garda on the
   map, and the description "Written by Roll".

### Immich round trip (pending)

Needs a local test Immich in Docker, which is not installed on this
machine yet.

## M3 Dates, places, map

Automated in `crates/core/tests/places_acceptance.rs` on the fixture
library:

- **Camera set +1 y 2 h.** The NIKON D750 set is listed with "The folder
  says 2019, but the photos are dated 2020". One reference with its true
  time and zone shifts the set by −1 y 2 h; every wrong-clock photo lands on
  its true local time to the second.
- **500 photos from one GPX.** Importing `dolomites-hike-2022-07-16.gpx`
  gives one suggestion with all 500 Sony photos; accepting it places every
  one within 150 m of its true position.
- **Home time zone.** The Canon set on home time is flagged against the
  phones' UTC−4.

In the real app (2026-10-11, driven over WebView2's DevTools port with the
fixture library): Dates opens on the NIKON set; picking a photo, typing its
true time, choosing UTC+2 and pressing Fix fixed 32 items (28 photos and 4
RAW partners), four actions in all. The map shows the fixture clusters
(New York, Lisbon, Lake Garda) with thumbnail pins and no console errors.

UI tests (`e2e/places.spec.ts`, mock backend): map pins and tray, drag
and drop onto the map, accepting a suggestion with the time zone
follow-up, "Set location…" by search and by coordinates, "Change date…",
and the four-step camera fix. Map speed: docs/benchmarks.md.

## M6 Windows installer

`scripts/fetch-tools.ps1` then
`pnpm tauri build --config src-tauri/tauri.bundle.json` (2026-10-13):

- `Roll_0.1.0_x64-setup.exe`, 66 MB, per-user NSIS installer. 7-Zip's
  listing shows `roll-desktop.exe`, `exiftool\exiftool.exe` with its
  `exiftool_files`, and `ffmpeg\` with `ffmpeg.exe`, `ffprobe.exe` and the
  LGPL DLLs (no `ffplay.exe`).
- Tauri's per-user default folder is `%LOCALAPPDATA%\Roll`, which is
  Roll's data folder. `src-tauri/windows/hooks.nsh` moves the default to
  `%LOCALAPPDATA%\Programs\Roll`. A silent install (`/S`) put the app, the
  tools and `uninstall.exe` there, nothing in `%LOCALAPPDATA%\Roll`, and
  added a Start menu entry.
- The installed copy, started with `PATH` reduced to `C:\Windows` and an
  empty data folder, reported ExifTool and FFmpeg from its own folder
  (`app_info` over WebView2's DevTools port).
- `uninstall.exe /S` removed the folder, the shortcut and the registry
  entry.
- One build attempt failed with rustc exiting `STATUS_ACCESS_VIOLATION`
  while compiling `roll-desktop` with thin LTO; the retry passed. If it
  repeats, close other builds or set `CARGO_PROFILE_RELEASE_LTO=false`.

By hand, pending: the installer's pages (the directory page should show
`...\Programs\Roll`), and starting Roll from the Start menu.
