A preview of Roll: a local photo and video library that sorts itself.
It is in active development; see the README for what works today and
docs/plan.md for what is next. Roll never deletes anything permanently and
every change can be undone, but please try it on a copy of your photos first.

## New in 0.3.1

- **More map at a click:** the layers icon on the map opens Map detail,
  with three ways to more than country outlines: the online map (streets
  and every building everywhere, from OpenFreeMap), a world map to keep
  on your computer (about 63 MB: lakes, large rivers, motorways, main
  roads and cities for the whole world) and offline map areas. While the
  map shows only outlines, it points you there.

## New in 0.3

- **The map down to single buildings:** offline map areas now keep every
  building and footpath for a city or a region up to the size of a large
  lake (central Lisbon is 18 MB), so you can tell one building of a campus
  from the next. Larger areas keep streets. The map zooms closer, and when
  you zoom in where it has no streets it offers to keep the area offline.
- **Skip blinks in best shots:** with the eye models (about 6 MB, offered
  on the Duplicates page), every face in a burst or series is checked and
  frames with closed eyes rank last, noted "Eyes closed". On hand-labelled
  portraits it found 4 of 5 closed eyes and almost never mistook open
  ones.

## New in 0.2

Everything new runs on your computer; the models are optional downloads
in Settings › Models and data.

- **Image understanding** (SigLIP 2): sorting suggestions, search by what a
  picture shows, Teach a category with examples, Find similar, look-alike
  groups in Discover, and camera and phone photos of one scene paired to
  fix clocks.
- **Text in photos** (PP-OCRv6): the words in screenshots, documents and
  memes are searchable and can mark an item private.
- **People** (YuNet and SFace): faces grouped into people you name, merge
  or correct; names are searchable and written into the files as face
  regions, which Immich can import.
- **Duplicates:** merge every exact copy in one step (your own folders win
  over downloads and backups), and the best shots of a burst or series,
  ranked by sharpness, exposure and faces.
- **Search:** best matches first, and "Better ranking" by your decision
  model. A search for "cat" no longer finds every sorted item.
- **Local AI in one click:** Roll downloads llama.cpp's server with
  Qwen3-VL 4B (about 3 GB) and runs it itself when needed; no Ollama or
  other install. 91% right on the test set, about one item a second.
- **Discover** can ask the decision model for a group's name.
- **Offline map areas:** keep the streets, water and place names of an
  area on this computer (from Protomaps' OpenStreetMap build), for a map
  without online tiles.
- Immich sync checked again against a real Immich 3.3.1: re-foldering
  between syncs, and the same metadata as Immich's own CLI.

## Downloads

| System | File | Notes |
|---|---|---|
| Windows 10 and 11 | `Roll_*_x64-setup.exe` | Installs for your user only, no administrator needed. ExifTool and FFmpeg are included. |
| macOS 11 or later on Apple silicon (M1 and later) | `Roll_*_aarch64.dmg` | ExifTool is included. For HEIC photos and videos, add FFmpeg: `brew install ffmpeg`. |
| Debian 13, Ubuntu 24.04 or later | `Roll_*_amd64.deb` | ExifTool and FFmpeg are included. |
| Other Linux with glibc 2.38 or later (Fedora 39, Arch) | `Roll_*_amd64.AppImage` | ExifTool and FFmpeg are included; needs Perl, which nearly every distribution has. |

`SHA256SUMS` lists the checksum of every file. Intel Macs and older Linux
distributions are not supported: ONNX Runtime, which runs the image,
text and face models, has no build for them.

## Not signed yet

The builds are not code-signed, so the systems warn on first start:

- **Windows:** SmartScreen says "Windows protected your PC". Choose More info, then Run anyway.
- **macOS:** the app is signed ad hoc, not notarized. Open it once with right-click › Open, or allow it in System Settings › Privacy & Security.

There is no automatic update yet; new versions are published here.
