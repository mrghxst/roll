# Build "Roll": a local photo and video library that sorts itself

Paste this whole file into Claude Code, in a clone of
[github.com/mrghxst/roll](https://github.com/mrghxst/roll) (or an empty
folder: clone the repo first). It is the complete spec.

---

You are building **Roll**, a desktop app (Windows first, then Linux and
macOS). It turns a messy phone dump of 100,000+ photos and videos into a
clean library sorted by year:

- Camera photos form a timeline.
- Screenshots, memes, social media saves and wallpapers get folders of their
  own.
- Wrong dates and missing places are fixed in bulk.
- Everything is tagged in a way Immich and other photo apps understand.

The folders on the PC are the complete archive. Everything is kept there, and
it's pleasant to browse both in Roll and in Windows Explorer. Immich only gets
a curated part: what the owner wants on the phone, to show friends and to
share.

Roll works on the files where they are and never loses anything. Sorting
100,000 photos costs at most a few dollars of AI, or nothing with local
models.

The bar is the polish of the best photo apps (Apple Photos, Google Photos,
Immich), with sorting faster than in any of them. The design is minimal,
black and white, keyboard-first and instant.

Build all of it, milestone by milestone (section 10), until every acceptance
criterion passes. Don't stop after scaffolding, and don't leave placeholder
features in the UI.

## 0. Working rules

1. **Spec and decisions.** Keep this file as written.
   - Record every deviation, choice between alternatives and open question in
     `docs/decisions.md`, with a one-line reason.
   - Keep `docs/plan.md` current: a milestone checklist with what's done and
     what's next.
   - Create `CLAUDE.md` with the commands, conventions and pitfalls you learn.
2. **Verify before you rely.** Versions and prices below were current on
   2026-10-10. Before using a crate, package, API, model or price, check
   crates.io, npm, the upstream docs or the OpenRouter API. Don't trust
   memory.
3. **Never touch real photos during development.** Use the generated fixture
   library (section 9).
   - Read the owner's real library only after they say so.
   - Write to it only through the app's own plan, apply and undo flow, and on
     a copy first.
4. **Ask the owner before you:**
   - spend money (more than $0.50 of OpenRouter calls, for example);
   - send data off the machine (installing tools and packages is fine);
   - create releases or push tags;
   - do anything that can't be undone.

   Installing toolchains and dev dependencies doesn't need a question.
5. **Commit in small steps** with clear messages. Each milestone ends with:
   - the app running;
   - tests passing in CI on Windows, Linux and macOS;
   - docs updated;
   - a short note to the owner about what to try.
6. **Code.**
   - Small modules, clear names, comments only where the intent isn't
     obvious.
   - No dead code.
   - LF line endings everywhere (already set in `.gitattributes`). Watch
     tools that write CRLF on Windows.
   - Rust 2024 edition, stable toolchain. TypeScript strict.
7. **Copy.**
   - Sentence case, plain words, short sentences, neutral tone.
   - No exclamation marks, no emojis, no jargon in the UI.
   - Before an action happens, say what it will do ("Moves 312 photos to
     2019/Memes. You can undo this.").
   - Use example names like `IMG_1234.HEIC` or `2019-08 Lake Garda`, never
     real people.

## 1. The owner's problem

The owner sorts photos the way most people would like to:

- a folder per year, then subfolders inside it;
- a default `Photos` subfolder for the main timeline (camera photos and
  videos);
- event folders for trips and occasions.

The reality is a phone dump, either one huge folder or one folder per year.
It mixes:

- real camera photos and videos;
- screenshots (Pokémon GO screenshots from years ago, things saved to
  remember);
- memes (images and videos);
- content saved from social media that isn't a meme;
- wallpapers that should be reviewed, then deleted or archived, so they don't
  end up in Immich;
- many other kinds nobody has named yet. Roll must learn new ones.

Older event folders get merged in over time.

Metadata is broken too:

- Photos from dedicated cameras have wrong clocks. If you know when one photo
  of a set was taken, the rest of the set can be shifted by the same amount.
- Many photos have no location, although the owner roughly knows where they
  were taken.

The owner wants:

1. **A complete local archive and a curated Immich.**
   - The folders on the Windows PC keep absolutely everything, sorted. They
     are the backup and the place to browse it all.
   - Immich only gets what the owner wants on the phone: photos to show
     friends, and albums to share (shared albums, partner sharing).
   - Memes, screenshots and the rest stay local unless the owner picks them.
2. **Tags as the source of truth.**
   - Categories, events and sources are tags written into standard metadata,
     readable by Immich.
   - Folders are derived from tags, so the library can be re-foldered at any
     time.
3. **Fast, smart bulk fixes for dates and places.**
4. **A great library.** Timeline, filters, search, viewer, video, and a map
   with a time slider on the left that selects which period is shown.
5. **Cheap, local-first AI.** For a one-time pass over 100k+ items, AI costs
   at most $10 per 100k through the API, ideally under $2. A local model costs
   nothing. Slow is acceptable.
6. **Photos stay where they are.** Roll works on a folder the owner picks,
   with no hidden store. New photos from outside (a camera card, a friend's
   folder, an old event folder) are added to the archive through a guided
   check (4.11).

## 2. What real input looks like

The owner's library is a 27k-file iPhone export: 113 GB, 2010 to 2025, with
flat year folders. Read-only analysis of it found:

| Finding | Number | What it means for Roll |
|---|---|---|
| Names were rewritten by the export tool (`IMG_<device>_<year> - <n>.jpeg`) | 100% | Never rely on names. Use them when they help (other libraries have `IMG-20190812-WA0001.jpg`, `Screenshot_…`, `RPReplay_…`). |
| HEIC was converted to JPEG; only `.jpeg` and `.mov` remain | 24.5k JPEG, 2.8k MOV | Other libraries will have HEIC, PNG, DNG, MP4 and more. Support them all. |
| Photos with camera make and model | 32% | Strong prior for the timeline. |
| iOS screenshots, marked by EXIF `UserComment = "Screenshot"` (survives HEIC to JPEG) | 11% | A free, certain rule. |
| Photos with no camera data at all (saved, received or downloaded) | 57% | This is where AI and clustering matter. |
| The largest group with no camera data shares one size (854×1590) | 4,122 files | Size and encoder signatures reveal the source app, so one decision can cover thousands of files. Others: 1024×1024, 1152×2048, 896×1152, 750×1334. |
| Photos with GPS | 6% | Location fixing is a core feature, not an extra. |
| Photos from 6 dedicated cameras (Nikon, Canon, Fujifilm, Panasonic), none with GPS | about 1,250 | The time offset tool and location-from-neighbors tool are needed. |
| All JPEGs have `DateTimeOriginal` and `OffsetTimeOriginal` | 100% | The export tool filled them. For saved images this is the save date, not the capture date: lower trust for events. |
| Live Photo IDs in stills, but no motion videos left | 91 | Pair Live Photos when both parts exist. Never assume they do. |
| Videos without a camera make (saved clips at social sizes such as 576×1024, 480×848, 720×1280) | 90% | Videos need the same sorting as photos. |
| Video codecs | 99.7% H.264, 7 HEVC | HEVC playback still needs a fallback (see 7.6). |

The owner's main PC runs Windows 11 25H2 on an i9-9900KF (8 cores, 16
threads) with 32 GB RAM, an RTX 2080 Ti (11 GB) and NVMe disks. Local AI is
practical on it, and Roll must still work, more slowly, on a PC without a
GPU.

## 3. Core concepts

- **Library root.** A folder the owner picks: the complete archive.
  - Roll indexes it recursively and organizes inside it.
  - Nothing leaves it unless the owner deletes it.
  - There can be several roots.
- **Immich selection.** Which assets also go to Immich.
  - Defaults come per category and per event (4.10).
  - Any item, folder or event can be switched on or off.
  - The local library always keeps everything.
- **Asset.** One photo or video file.
- **Asset group.** Assets that belong together and always move together.
  They show as one tile with a badge.

  | Group | How it's matched |
  |---|---|
  | Live Photo (still + motion) | Apple `ContentIdentifier`, else the same stem, else under 1 s apart with a clip under 4 s |
  | RAW + JPEG | Same stem and time |
  | Edited + original | `IMG_E1234` with `IMG_1234`, and `FullSizeRender` |
  | Sidecar files | `.xmp`, `.aae`, Google Takeout `.json` |
  | Burst | Burst UUID, or a rapid sequence |

- **Category.** Exactly one per asset. It drives the folder. The defaults are
  in 4.3, and the owner can add, rename, nest and reorder categories.
- **Tags.** Any number per asset, hierarchical (`Source/Instagram`,
  `Event/2019-08 Lake Garda`, `Category/Memes`). They are written to
  metadata. The category is mirrored as a `Category/…` tag so other apps and
  Immich see it.
- **Event.** A named span of time and place (a trip, a party). Confirmed
  events get tags and, optionally, their own folder.
- **Provenance.** Every editable field records its value and the original
  value from the file. Fields are date and time, time zone, location and
  accuracy, category, tags, description and rating. Each also records:
  - its **source**: file, filename, folder, neighbors, location history, a
    rule (with id), a model (with id), or manual;
  - a **confidence**;
  - a **history** entry for undo.

  The UI always answers "why": "Screenshots, because of the iOS screenshot
  marker." "Date shifted +1 y 2 h from the camera clock (reference: photo
  12)."
- **Plan, apply and journal.** Every file change goes through a plan:
  - The change types are move, rename, create folder, write metadata and
    trash.
  - The owner previews a plan before applying it.
  - Applying is journaled, can be undone, and is safe across crashes.

## 4. Features

### 4.1 Library and browsing

- **Timeline:**
  - justified rows with adjustable row height (Ctrl+scroll or a slider);
  - month and day headers, with a sticky month label;
  - a scrubber on the right edge with years and months, smooth while
    dragging fast;
  - badges for video (with duration), Live Photo, burst, RAW, and metadata
    problems.
- **Selection:** click, Ctrl or Shift click, drag a rectangle, Ctrl+A, and
  select a whole day or month from its header. The selection count and the
  actions on it sit in the top bar.
- **Folders:** the real folder tree under each root, with counts. Drag and
  drop moves files, through a journaled plan. "Show in Explorer" opens the
  folder.
- **Sidebar sections:** categories, tags, events and saved filters, each with
  counts.
- **Viewer:**
  - Opens full window on a black backdrop, with a zoom transition from the
    thumbnail. It preloads neighbors and shows the 1024 px preview first,
    then full resolution.
  - Zoom (wheel, pinch, 1:1) and pan.
  - Filmstrip.
  - Info panel: date with its provenance, a place mini-map, camera, lens,
    file, category, tags, source and description.
  - Video player with scrubbing. Live Photos play on hover or press.
  - Rotate (writes orientation), rating 0–5, favorite (stored as a tag and
    shown as a heart).
  - Copy, show in folder, open with the default app.
- **Quick look** (Space) and a **command palette** (Ctrl+K) for every action.
- **Batch edit on any selection:** category, tags, date and time, location,
  description, rating, favorite, move, delete.
- **Inbox:** new files that appear in a root (watched) are indexed, sorted
  automatically where confident, and listed in the Inbox.
- **History panel:** every applied batch, with undo. Ctrl+Z undoes the last
  batch.
- **Activity panel:** background jobs with progress, ETA, throughput,
  pause and resume, and AI spend so far.

### 4.2 Filters and search

- **One search field:** free text plus filter chips. A deterministic parser
  handles years and months ("2019", "summer 2021", "last year"), media types
  ("videos"), categories ("screenshots", `#memes`), places (offline
  gazetteer), cameras, events, and "no location" or "no date". The rest is
  semantic text.
- **Filter panel,** with facets and counts:
  - date range, media type (photo, video, Live Photo, burst, RAW), category,
    tags, event, source app;
  - camera, lens;
  - location: has location, none, approximate, country or city, map area;
  - resolution, orientation, aspect ratio, duration, format, folder;
  - rating, favorite;
  - duplicates, AI confidence, review state;
  - Immich: sent, local only, waiting for the next sync;
  - metadata problems: no date, suspicious date, date changed, no location.
- **Saved filters** act as smart albums in the sidebar.
- **Semantic search:**
  1. A SigLIP 2 text embedding is matched against the image embeddings.
  2. SQLite FTS5 (BM25) searches OCR text, captions, tags, folder names and
     place names.
  3. The two result lists are fused with Reciprocal Rank Fusion.
  4. Optionally, "Better ranking" reranks the top 50 with a reranker model
     (see 5.1).
- **Natural language:** "videos from Italy in summer 2019 without
  screenshots" becomes filter JSON plus semantic text. The deterministic
  parser runs first. The text model (local or API) only handles what's left,
  and its output is shown as editable chips.
- **Speed:** results in under 300 ms for 100k assets, without reranking.

### 4.3 Sorting

**Default categories** (editable; the definitions are also used in the AI
prompts):

| Category | What belongs | Default folder | Sent to Immich |
|---|---|---|---|
| Photos | Real-life photos and videos: people, places, events, things. Includes personal photos received from friends (tagged `Source/Chat`). | `{year}/Photos`, or the event folder | yes, in the timeline |
| Screenshots | Screen captures and screen recordings from any device. | `{year}/Screenshots` | no |
| Memes | Made to be funny or shared: captions over images, reaction images, comics, joke formats, stickers, GIFs. | `{year}/Memes` | no |
| Social media | Saved posts, stories, reels and pictures from apps and websites that aren't memes: other people's photos, outfits, recipes, products, news, quotes. | `{year}/Social media` | no |
| Wallpapers | Backgrounds, digital art, AI-generated images and design assets with no personal content. | `Wallpapers` | no |
| Documents | Photos or scans of paper, receipts, tickets, IDs, whiteboards, notes and labels, or a screen photographed for its text. | `{year}/Documents` | no |
| Junk | Accidental, black or very blurry shots, pocket shots, test shots, broken files. | `{year}/Junk`, kept until the owner deletes it | no |
| Unsorted | Not decided yet. | Stays where it is | no |

Any item, folder, category or event can be switched to "send to Immich". For
example, a few memes can go there as an album to show friends.

**Privacy guard.** Items that look like IDs, payment cards, passwords,
medical or intimate content get the tag `Private`:

- The tag is assigned locally: zero-shot scores plus OCR patterns
  (Luhn-valid card numbers, IBANs, passport MRZ lines).
- These items are never sent to a cloud model unless the owner allows it.
- They are never moved without review.
- They are never sent to Immich unless the owner picks them, and then they
  go to Immich's locked folder.

**Signals per asset:**

- **Metadata:**
  - camera make, model and serial; lens; software; screenshot marker;
  - GPS; size and aspect ratio; format;
  - for videos, duration, fps and whether there is audio;
  - ICC profile name and a JPEG quantization-table fingerprint (the encoder
    signature);
  - maker notes and XMP presence;
  - file and folder name patterns; Live Photo and burst IDs.
- **Content:**
  - a SigLIP 2 image embedding (for videos, 3 keyframes);
  - OCR text, for screen-like, document-like and meme-like items;
  - cheap image statistics: brightness, blur (Laplacian variance), text area
    ratio, colorfulness.

**Decision stages,** cheapest first. Each later stage only sees what's still
uncertain.

1. **Rules.** Deterministic, explainable, editable in Settings › Rules, and
   stored as data. Defaults include:
   - the iOS screenshot marker, which is certain;
   - Android `Screenshot_*` and iOS `RPReplay*` file names;
   - WhatsApp `IMG-YYYYMMDD-WA*` and `VID-*`, Telegram, Snapchat, `FB_IMG_*`
     and `received_*` names, plus Instagram, Snapchat and Canva software tags,
     which set source tags;
   - camera make present, which gives a strong prior for Photos;
   - GIF, which gives a prior for Memes;
   - tiny or near-uniform images, which become Junk candidates.
2. **Local models (free).**
   - Zero-shot scores per category, from prompt ensembles.
   - k-nearest-neighbor and logistic-regression classifiers on SigLIP
     embeddings plus metadata features, trained on the owner's decisions and
     retrained in seconds.
   - Cluster propagation (see Discover).

   The outputs are calibrated probabilities.
3. **Decision model** (optional). A small vision-language model, local or
   through OpenRouter, sees only the items that are still below the
   auto-accept threshold. See section 5.
4. **Review.** The owner decides the rest. A random 1% audit of auto-accepted
   items estimates accuracy ("estimated 97.8% correct, ±1.1%").

**Auto-accept.** Each category gets a threshold so that measured precision on
reviewed items stays at or above 98%. The owner can change it. Until there
are enough reviews, nothing is auto-accepted except certain rules.

**Review screen:**

- One tab per proposed category, sorted by confidence ascending, so the most
  doubtful items come first.
- Each tile shows its "why".
- Keyboard: 1–9 assign a category, X marks Junk, Enter accepts the page,
  arrows move, Space shows a large preview, Z undoes.
- "Accept all at 95% or higher" is a single action.
- Progress: "Reviewed 1,240 · Auto-accepted 18,422 · Left 312".

**Teach (new types).**

1. Name a new category or subcategory and write one line about what belongs.
2. Add 10–30 examples: search, "find similar", or pick from the grid.
3. Roll trains a classifier, scores the library and shows proposals in
   Review.

Each correction improves the classifier, and new categories also become
available to the decision model. Example: `Screenshots/Pokémon GO`.

Subcategories are tags only by default. The owner can give one its own folder
(`{year}/Screenshots/Pokémon GO`).

**Discover.** Roll clusters unsorted items by embedding (HDBSCAN, or k-means
plus merging) and by metadata signature (same size, same encoder, no camera).
It shows the largest clusters first, with 12 sample tiles each.

- Actions: assign a category to all, create a category or subcategory, split
  the cluster, ignore it.
- Clusters get a suggested name from the text or decision model: one cheap
  call per cluster with 9 thumbnails.
- This is how 4,122 files at 854×1590 get sorted with one decision.

**Junk detection** (local): black or white frames, low variance, heavy blur,
pocket shots (dark with motion blur), images under 200 px, and broken files.

### 4.4 Organize

- **Folder templates** per category. Tokens: `{year}`, `{month}` (01–12),
  `{month_name}`, `{date}` (YYYY-MM-DD), `{event}`, `{category}`,
  `{subcategory}`, `{source}`, `{camera}`, `{country}`, `{city}`.
  - The year is that of the date taken, in local time.
  - Renaming files is opt-in, with its own template (for example
    `{date} {time} {camera}`). Names are kept by default.
- **Default layout:**

  ```
  <Library>/                  the complete archive: everything is kept
    2023/
      Photos/                 camera photos and videos not in an event
      2023-07 Lake Garda/     events the owner confirmed
      Screenshots/
      Memes/
      Social media/
      Documents/
      Junk/                   kept until the owner deletes it
    Wallpapers/
  ```

- **Readable in Windows Explorer.**
  - Folder and file names sort correctly (`2023-07 …` before `2023-08 …`).
  - Metadata is written so that Explorer and the Photos app show the right
    date, tags, rating and location (section 6).

- **Modes:**
  - **Move in place.** Instant on the same drive (rename).
  - **Copy to a new library folder.** Originals are untouched. Roll copies,
    verifies each copy with BLAKE3, and checks free space first.
- **Plan preview:**
  - the target tree with counts and sample tiles;
  - "unchanged", "moved" and "renamed" totals;
  - conflicts;
  - items without a decision, which stay put;
  - export of the plan as CSV.
- **Apply,** with a journal:
  1. Each operation is recorded as pending, performed, then marked done.
  2. On start, pending operations are reconciled against the file system.
  3. Cross-volume moves are copy, verify, then delete the source.
  4. Existing files are never overwritten: a conflict gets " (2)".
  5. Groups and sidecars move together.
  6. Case-only collisions are handled (Windows and macOS file systems are
     case-insensitive).
- **Undo** of a whole batch restores the exact previous tree. Tests verify
  this with a hash manifest.
- **Re-folder at any time.** When tags or categories change, Organize shows
  only the difference. "Keep folders in sync automatically" is an option,
  off by default.
- **Paths:** paths longer than 260 characters work on Windows (test them).
  Long generated names are shortened. Hidden and system files (`Thumbs.db`,
  `desktop.ini`, `.DS_Store`, `._*`) are ignored.
- **Placeholders:** OneDrive and iCloud files that aren't downloaded are
  detected and skipped, with a note.

### 4.5 Fix dates

**Detection** builds a list of suspect sets:

- **Camera clocks:**
  - Group by camera (make, model and serial) and by continuous time runs.
  - Flag sets that disagree with phone photos of the same scenes, using
    visual pairing (below).
  - Flag sets that fall outside plausible ranges: before the camera model
    existed, in the future, or default dates like 2000-01-01 or 1970.
- **Time zones:** camera photos without an offset, taken where the phone's
  GPS shows another zone.
- **Missing or untrusted dates:** candidates come from file name patterns,
  folder names, the camera's file counter sequence (between `IMG_1233` and
  `IMG_1235`) and, as a last resort, file times. Each candidate carries a
  confidence.

**Fixing a set:**

1. Select a set, either detected or any selection.
2. Pick a reference photo and enter its true date and time with time zone,
   or pick a phone photo of the same moment.
3. Roll computes the offset and shows a before and after strip: the camera
   row against the phone row, plus the new date range. It lists side
   effects, such as a year change that moves files to another year folder.
4. Options:
   - shift (the default);
   - set the time zone without shifting (re-label local time);
   - correct drift with two references (linear);
   - apply to every photo from this camera within a date range (cameras keep
     a wrong clock for months).
5. Apply. Dates are written (section 6), history is recorded, and any needed
   re-folder plan is shown.

**Smart offset.** For camera sets that overlap phone photos within ±48 h,
Roll finds visually similar pairs (same scene, high SigLIP similarity). It
proposes the median offset, with the pairs shown as evidence. One click
accepts it.

**Shift a selection by an amount:** ± years, days, hours, minutes or seconds.
**Set an exact date** on items with none.

### 4.6 Fix places

**No location** is a first-class view: counts by day, event and camera, and
grouped suggestions such as "312 photos, 3–10 Aug 2019: Rome area (from 41
phone photos, ±3 km)". Suggestion sources:

1. **Time neighbors.** The owner's own geotagged photos and videos close in
   time. If there are neighbors before and after within N minutes (default
   30) and they are close together, Roll interpolates. Confidence drops with
   the time gap and distance.
2. **Location history imports:**

   | Format | Key fields |
   |---|---|
   | GPX, KML, GeoJSON tracks | track points |
   | Google Maps Timeline phone export (`Timeline.json`) | `semanticSegments` with `timelinePath` points (`"50.0506312°, 14.3439906°"` strings), `visit`, `activity`, plus `rawSignals.position` |
   | Legacy Takeout `Records.json` | `locations[]` with `latitudeE7`, `longitudeE7`, `timestamp` |
   | Semantic Location History | monthly `timelineObjects` with `placeVisit` and `activitySegment` |

   Detect the format from its top-level keys. Build a fixture for each.
3. **Events.** If most of an event is geotagged, propose its centroid for the
   rest.
4. **Visual matches.** Geotagged photos of the same place or landmark, shown
   as evidence.
5. **Names.** Folder and file names geocoded offline. "Italy 2019" gives
   Italy at country accuracy.

**Accept** a group in bulk, **adjust** it (drag the pin, search), or
**reject** it.

**Manual tools,** available anywhere:

- drag a selection onto the map;
- "Set location…" with offline search (GeoNames cities, regions and
  countries; online search is optional);
- paste coordinates or a maps link;
- copy the location from another photo;
- clear the location.

**Accuracy radius on every location:** about 10 m for GPS, computed for
interpolations, 5 km for a city, 200 km for a country.

- Approximate locations also write `GPSHPositioningError`.
- The map shows them with a dashed ring.
- Filters can include or exclude them.

**Time zone follow-up.** After setting a location, Roll offers to set the time
zone (offline lookup) on items without an offset.

### 4.7 Map with time slider

- **Left: a vertical time slider** over the library's whole date range.
  - It shows a histogram of item counts per month, or per week when zoomed
    in.
  - Two handles select a range, and dragging the range moves it.
  - Scrolling over the slider zooms the time axis.
  - Year chips jump to a year.
  - A play button animates the window through time, with a speed control.
- **Center: the map** (MapLibre).
  - Clusters show the best item's thumbnail and a count. Clicking a cluster
    opens its items in a side grid.
  - Rectangle and lasso selection, and a heatmap toggle.
  - Approximate locations have dashed rings.
- **Bottom tray:** items in the selected time range without a location. Drag
  them onto the map.
- **Shared state:** the map, timeline and filters share selection and
  filters. "Show in timeline" jumps to the selection.
- **Basemap:**
  - An offline world map (Protomaps PMTiles, zoom 0–6, about 60 MB) ships
    with the app, with a monochrome style that matches the UI.
  - Detailed tiles are opt-in: OpenFreeMap online (with a privacy note), or
    downloaded offline regions for the places where the owner's photos are
    (later milestone).
- **Speed:** 100k points stay interactive at 60 fps.

### 4.8 Duplicates

- **Groups:**
  - exact copies (BLAKE3);
  - visual near-duplicates (perceptual hash plus embedding similarity);
  - lower-quality copies of a photo the owner has in full quality (the
    WhatsApp copy of a camera photo);
  - bursts.
- **Keep-best suggestion:** original camera file, then largest resolution,
  then richest metadata, then already in the right folder.
- **Merge:** tags, dates and locations move to the kept item. The others go
  to the app's Trash.
- **Trash:** a reversible list inside Roll. Files are untouched until the
  owner empties it, and emptying sends them to the system Recycle Bin.
  Roll never deletes permanently.

### 4.9 Events

- **Detection,** on Photos only, ignoring "saved on" dates:
  - Split by time gaps: more than 4 h at home, more than 12 h away.
  - Use distance from home: more than 50 km counts as away.
  - Require at least 8 items.
  - Days away from home merge into one trip.
  - Home is the most frequent night location, or the owner sets it.
- **Names:** "{Month YYYY} {Place}", with the place from reverse geocoding:
  the city if the event stays in one, otherwise the region or country. The
  text model can refine it from captions ("Hiking in the Dolomites").
- **Owner actions:** confirm, rename, merge, split, or dismiss.
- **Confirmed events** get the tag `Event/<name>` and, optionally, a folder
  `{year}/{yyyy-mm} {name}`. An event sent to Immich becomes an album there
  and can be shared with chosen Immich users.

### 4.10 Immich

Immich is the curated, social part: what the owner wants on the phone, to show
friends, and to share. The local library stays the complete archive and the
master copy. Immich keeps its own copy of what it gets, so re-foldering the
local archive never affects it.

- **What goes to Immich:**
  - Defaults per category (table in 4.3): Photos yes, everything else no.
  - Confirmed events: yes, each as an album.
  - Overrides on any item, selection, folder, category or event: "Send to
    Immich" or "Keep local only". The state shows as a small badge on tiles
    and as a filter.
  - Private items go only when the owner picks them, and then with `locked`
    visibility.
- **Sync through the Immich API.** The server URL and API key live in the OS
  keychain. Target Immich v3.x, and check the OpenAPI spec of the owner's
  server version.
  - **Upload** new selected items with `POST /assets` (the file plus
    `sidecarData`). Check checksums first with
    `POST /assets/bulk-upload-check`, so nothing is uploaded twice. Link Live
    Photos (`livePhotoVideoId`).
  - **Store** the Immich asset id for every uploaded file.
  - **Push later edits** from Roll with `PUT /assets` (date and time, time
    zone, location, description, rating, favorite, visibility) and
    `PUT /tags/assets`.
  - **Albums:** create albums for events (`POST /albums`,
    `PUT /albums/{id}/assets`) and share them with chosen users
    (`PUT /albums/{id}/users`).
  - **Remove:** after a confirmation, items switched to "Keep local only" go
    to Immich's trash (`DELETE /assets` without `force`). A sync never
    touches local files.
  - **Plan first:** every sync shows a plan before it runs ("Uploads 412,
    updates 38, removes 3 from Immich"). It runs as a resumable job and lands
    in History.
- **Script export** as a fallback: an Immich CLI script per destination.
  - Timeline folders use plain `immich upload --recursive`. Never use
    `--album` there: it names albums after the parent folder, so every year
    would land in one "Photos" album.
  - Event folders use `--album`.
  - The CLI uploads `<file>.<ext>.xmp` sidecars and prefers them over
    `<file>.xmp`.
- **No external library on the archive.** Pointing an Immich external
  library at the archive would show everything. The docs say so.
- **Phone backup.** If the Immich app's automatic phone backup is on, Immich
  gets the whole camera roll, not the selection. The Immich guide in `docs/`
  explains this and recommends leaving it off. New photos then reach the
  archive through the usual export and Roll's Inbox.

### 4.11 Add photos (import an event)

A guided flow for bringing a folder from outside the archive into it, usually
one event: a camera card, a friend's photos, an old trip folder, a phone
export. Start it with "Add photos…", by dragging a folder onto the window, or
when a camera card or phone is connected.

1. **Pick the source.** Roll indexes it read-only, in place, as a staging set.
   It shows:
   - the count, date range, cameras and formats;
   - how many items are already in the archive (BLAKE3 and visual match).
     These are skipped by default.
2. **Check the dates.** The set is shown on the archive's timeline, next to
   the photos the owner already has from that time.
   - Roll flags a camera clock that looks wrong: dates in the future,
     defaults like 2000-01-01, no overlap with anything plausible.
   - It runs the smart offset against the archive's phone photos (4.5) and
     proposes a shift, with matching pairs as evidence.
   - It has the same reference-photo, time zone and shift tools as 4.5,
     applied to the staging set.
   - Nothing is accepted silently. The step ends with "Dates look right" or a
     fix.
3. **Check the places.**
   - Roll proposes locations from archive photos taken at the same time (4.6),
     from location history, and from the folder name ("2019 Lake Garda").
   - Items without a suggestion can get one pin for the whole set, or per
     day.
   - A mini map shows the result, with approximate locations as dashed rings.
4. **Sort the set.** Rules and models sort the set (4.3). It is proposed as one
   event:
   - the name comes from the folder name, or "{Month YYYY} {Place}";
   - screenshots and memes inside it get their usual categories;
   - a set spanning a long time can be split into several events.
5. **Preview the plan.**
   - **Copy** (the default: the source stays untouched, each copy is verified
     with BLAKE3) or **move**.
   - The target is `{year}/{yyyy-mm} {name}`, or another template.
   - Metadata is written as in section 6.
   - Immich: send the event as an album (on by default for events), and
     optionally share it with chosen users.
6. **Apply.** The files join the archive and the timeline in the right place,
   and the batch lands in History with undo. The Immich part runs as a normal
   sync with its own plan (4.10).

The same flow handles folders already inside the library that were never
checked, such as an event folder copied in by hand. They show up in the Inbox
as "Not checked yet".

### 4.12 Settings, privacy and first run

- **First run:**
  1. Welcome.
  2. Choose the library folder (the complete archive).
  3. Choose how to save changes:
     - **In the files and as sidecars** (recommended):
       - Windows Explorer and the Photos app show the fixed dates, places and
         tags.
       - Immich, digiKam and Lightroom read the `.xmp` sidecars.
       - Only metadata is rewritten, never the image data.
     - **Sidecars only:** originals stay untouched byte for byte, but
       Explorer and the Photos app don't see the changes.
  4. Choose AI:
     - **Local:** detects Ollama, LM Studio or llama.cpp.
     - **OpenRouter:** paste an API key, stored in the OS keychain.
     - **Off:** rules and local embeddings only.
  5. Connect Immich (optional): server URL and API key. Nothing is sent until
     the owner has reviewed the first sync plan.
  6. Start. The grid fills while indexing runs.

  After indexing, a short guide walks through: review automatic decisions,
  Discover, fix dates, fix places, Organize preview, Apply, and the first
  Immich sync.
- **Settings:**
  - library roots;
  - categories with their folder templates and Immich defaults;
  - Immich: server, API key, what to send, sharing, sync;
  - rules;
  - metadata write mode;
  - AI: endpoints, models, privacy and budget;
  - network;
  - appearance: theme and grid density;
  - performance: thread and GPU limits, "only when idle", "pause on
    battery";
  - shortcuts;
  - about: licenses and attributions;
  - advanced: export or import the app state, rebuild the index.
- **Network panel.** It lists every feature that can reach the internet and
  whether it's on:
  - the owner's Immich server;
  - the AI provider;
  - downloads of models, maps and geodata;
  - online map tiles;
  - online place search;
  - update checks.

  Everything is off until the owner turns it on, except what they chose in
  the first run.
- **No telemetry.** Logs stay local, rotate, and never contain image content.
  "Clear logs" deletes them.

## 5. AI and cost

### 5.1 Model roles

| Role | Default | Runs |
|---|---|---|
| Vision embeddings (similarity, zero-shot, classifiers, search, duplicates, clustering) | SigLIP 2 ViT-B/16 (multilingual), the ONNX export at `immich-app/ViT-B-16-SigLIP2__webli`. "Best" option: SO400M/16-384. | Always local (ONNX Runtime) |
| OCR (screens, documents, memes, search) | PP-OCRv5 mobile detection and recognition, ONNX (Latin and multilingual) | Local |
| Decision model (classify uncertain items, name clusters) | A small vision-language model; the owner picks it after the model check | Local endpoint or OpenRouter |
| Text model (search query to filters, event and cluster names) | A small text model | Local endpoint or OpenRouter |
| Reranker ("Better ranking", optional) | `Qwen/Qwen3-VL-Reranker-2B` (Apache-2.0) in GGUF via llama.cpp, or the decision model ranking thumbnails | Local or API |

- **Local endpoints:** any OpenAI-compatible server (Ollama, LM Studio,
  llama.cpp `llama-server`).
  - Detect them on their default ports.
  - List models through `/v1/models`.
  - Suggest models for the available VRAM: for 8–12 GB, `gemma4:e4b`,
    `qwen3-vl:4b` or `qwen3-vl:8b`; for 4 GB or less, `qwen3-vl:2b`.
- **Bundled llama.cpp** (Vulkan or Metal) as a one-click local AI is a later
  milestone.

### 5.2 OpenRouter

- **Models and prices:** live from `GET https://openrouter.ai/api/v1/models`.
  Per-provider prices and zero data retention (ZDR) come from
  `GET /api/v1/endpoints/zdr` and `/api/v1/models/{id}/endpoints`.
  - The picker shows only image-capable models.
  - It sorts by estimated cost per 100k items.
  - It hides endpoints without ZDR unless the owner allows them.
- **Requests:** synchronous `POST /api/v1/chat/completions` with images as
  base64 data URLs. Body settings:
  - `provider: { "zdr": true, "data_collection": "deny", "require_parameters": true }`;
  - `response_format` set to a JSON schema;
  - temperature 0;
  - reasoning off or at minimum effort;
  - `max_tokens` capped.
- **No Batch API.** OpenRouter's Batch API is half price, but images must be
  public URLs and inputs are stored for 30 days. That rules it out for
  private photos.
- **Real cost:** every response carries `usage.cost`. Sum it, store it per
  asset and per job, and show it live.
- **What leaves the machine:**
  - A thumbnail of at most 512 px, JPEG quality 80, with all metadata
    stripped.
  - Short hints (has camera data, size, video duration).
  - Never file names, paths, GPS or Private items.
  - Videos send one image strip of 3 keyframes.
- **Batching:** 8 images per request by default, numbered in the prompt. The
  model check confirms batching doesn't hurt accuracy, and falls back to 4 or
  1 if it does.
- **Throughput:** adaptive concurrency (start at 4, back off on 429 and 5xx),
  retries with jitter, and an idempotent per-asset state machine in SQLite
  (pending, sent, done, failed, each with model id and prompt version). Runs
  resume after a crash without paying twice.
- **Guardrails:**
  - A cost estimate before every run.
  - A per-run budget cap (default $5) that stops the run exactly.
  - A warning when measured cost runs 20% over the estimate.
  - A clear consent screen: "3,412 thumbnails (512 px, no metadata) will be
    sent to <provider> through OpenRouter. Estimated cost $0.41."

### 5.3 Cost math

Assumptions per item:

- tokens:
  - 256–330 image tokens (it depends on the model, so measure it in the
    model check);
  - 30 tokens of hints;
  - a 400-token prompt shared by 8 items;
  - 35 output tokens of compact JSON;
- cost = (image + hint + prompt share) × input price + output × output price.

Snapshot from 2026-10-10, with ZDR endpoints, assuming every item goes to the
model:

| Model | Provider | $/M in | $/M out | ≈ $ per 100k items |
|---|---|---|---|---|
| `inclusionai/ling-3.0-flash-vl` | Novita | 0.021 | 0.062 | 1.0 |
| `google/gemma-3-12b-it` | DeepInfra | 0.05 | 0.15 | 2.2 |
| `google/gemma-4-26b-a4b-it` | Io Net | 0.048 | 0.21 | 2.5 |
| `google/gemini-2.5-flash-lite` | Google Vertex | 0.10 | 0.40 | 4.8 |

With rules and local models deciding first, typically 30–50% of items reach
the decision model, so the real cost is about $0.30–2.50 per 100k. That's
far under the $10 ceiling, and $0 with a local model.

A local 4B model on the owner's RTX 2080 Ti should handle about 1 item per
second, so about 30k uncertain items run overnight. Measure this and show
real rates in the Activity panel. Reranking with the API costs under
$0.001 per search.

### 5.4 Model check (calibration)

Before the first big run:

1. The owner labels about 150 items in Review: a stratified sample, about 10
   minutes with the keyboard.
2. Roll evaluates the local stage and up to 3 candidate decision models on
   that sample.
3. It shows each model's accuracy, cost per 100k and speed, and recommends
   the cheapest model within 2 points of the best accuracy.

The check costs under $0.10. Re-run it when models or prices change.

### 5.5 Decision prompt (starting point; version it)

System:

```
You sort a personal photo library. For each numbered image choose exactly one
category from the list, using the definitions and the hints. If none fits or
you can't tell, answer "unsure". Be calibrated: confidence is the probability
that your category is right. Reply with JSON only.

Categories:
- photos: <definition>
- screenshots: <definition>
- ... (owner's categories, including custom ones, with their one-line definitions)
Sources (optional): instagram, tiktok, snapchat, whatsapp, telegram, reddit, x, facebook, pinterest, youtube, other
```

User: for each item, the text `Image 3: camera=no, screenshot_marker=no,
size=1080x1350, video=no` followed by the image.

Schema:

```json
{"items":[{"i":3,"category":"social_media","confidence":0.86,"source":"instagram",
           "caption":"recipe card for pasta","has_text":true,"private":false}]}
```

Validate every reply against the schema. Retry malformed replies once, then
mark the item as unsure. Keep captions to 12 words or fewer, and store them
for search.

## 6. Metadata

- **Reading.**
  - Fast native reading with `nom-exif` (JPEG, HEIF and HEIC, TIFF, PNG,
    MOV, MP4 and more), in parallel.
  - ExifTool as a fallback, and for what nom-exif doesn't cover: Apple maker
    notes (`ContentIdentifier`), XMP, QuickTime keys, RAW.
  - Read existing `.xmp` sidecars and Google Takeout `.json` sidecars
    (`photoTakenTime`, `geoData`, `description`).
- **Writing.** ExifTool in `-stay_open` mode, bundled.
  - On Windows always pass `-charset filename=utf8` and use UTF-8 argument
    files, or non-ASCII names break.
  - The default mode writes into the file and to a `<file>.<ext>.xmp`
    sidecar, merged with existing content. Sidecar-only mode is the
    alternative.
  - Writing into a file rewrites metadata only. Check ExifTool's
    `ImageDataHash` before and after: it must not change. The journal keeps
    the previous values, so undo can restore them.
  - Write what Windows shows, and check it in Explorer and the Photos app on
    Windows:
    - EXIF `DateTimeOriginal`: Explorer's "Date taken";
    - EXIF GPS;
    - `XMP-dc:Subject` plus `IPTC:Keywords`: Explorer's "Tags";
    - `XMP-xmp:Rating` plus EXIF `Rating` and `RatingPercent`;
    - QuickTime `CreateDate` for videos: "Media created".
- **Time.**
  - EXIF `DateTimeOriginal` is local time. `OffsetTimeOriginal` carries the
    zone if present.
  - QuickTime `CreateDate` is UTC by spec, but some cameras write local
    time. Handle that per camera and test it.
  - DST boundaries must round-trip.
- **Orientation.** Apply EXIF orientation to thumbnails. libheif already
  applies HEIF transforms, so never rotate twice. Apply the video rotation
  matrix too.

Immich mapping, verified against the Immich v3.3.1 source
(`server/src/services/metadata.service.ts`):

| Roll field | Sidecar (XMP) written | In the file (default mode) | What Immich reads |
|---|---|---|---|
| Tags | `XMP-digiKam:TagsList` (`A/B`), `XMP-lr:HierarchicalSubject` (`A\|B`), `XMP-dc:Subject` (leaf names) | same, plus `IPTC:Keywords` for JPEG | `TagsList`, else `HierarchicalSubject`, else `Keywords` |
| Date taken | `XMP-exif:DateTimeOriginal` with offset, `XMP-photoshop:DateCreated`, `XMP-xmp:CreateDate` | `DateTimeOriginal`, `CreateDate`, `OffsetTime*`, `SubSec*`; for video QuickTime `CreateDate` (UTC) and `Keys:CreationDate` | Sidecar date first. Then in order: `SubSecDateTimeOriginal`, `SubSecCreateDate`, `DateTimeOriginal`, `CreationDate`, `CreateDate`, `MediaCreateDate`, … |
| Location | `XMP-exif:GPSLatitude`, `GPSLongitude`, `GPSAltitude`, `GPSHPositioningError` | EXIF GPS; for video QuickTime `GPSCoordinates` and `Keys:LocationISO6709` | `GPSLatitude`, `GPSLongitude`; reverse geocoded with GeoNames `cities500` |
| Description | `XMP-dc:Description` | `ImageDescription` | `ImageDescription` or `Description` |
| Rating | `XMP-xmp:Rating` | same | `Rating` 1–5 |
| People (later) | `XMP-mwg-rs:RegionInfo` | – | `RegionInfo`, when face import is on |
| Live Photo link | never changed | – | `ContentIdentifier` or `MediaGroupUUID` |

Sidecar fields override the file's own fields in Immich.

## 7. Architecture and stack

### 7.1 Stack

These versions were current on 2026-10-10. Re-check them.

| Area | Choice | Notes |
|---|---|---|
| App shell | Tauri 2.12 | Small, fast and secure. WebView2 on Windows, WebKitGTK on Linux, WKWebView on macOS. |
| UI | Svelte 5.57 (runes), TypeScript, Vite 8 | The same stack as the owner's other tools. |
| Rust | Edition 2024: tokio 1.53, rayon 1.12 | Async I/O plus a CPU pool. |
| Database | SQLite via rusqlite 0.40 (bundled) | WAL, FTS5. One writer plus a reader pool. Migrations. |
| Vectors | f16 vectors in SQLite blobs, plus an in-memory SIMD brute-force index | Fine up to about 500k assets. Move to `usearch` HNSW only if that's measured to be needed. |
| Metadata | nom-exif 3.8 to read; ExifTool 13.x (bundled) to read and write | See section 6. |
| Images | zune-jpeg or turbojpeg (scaled DCT decode), libheif-rs 3 (HEIC and AVIF, LGPL, dynamic), `image` 0.25 (PNG, GIF, WebP, TIFF, BMP), jxl-oxide (JPEG XL), rawler (RAW previews), fast_image_resize 6 | Use embedded thumbnails for the first paint. |
| Video | FFmpeg 8 and ffprobe, an LGPL build, as sidecar binaries | Posters, keyframes, metadata, preview proxies. For proxies use hardware H.264 encoders or SVT-AV1 or libvpx, never GPL x264. |
| ML | ort 2.0 (ONNX Runtime): DirectML on Windows, CoreML on macOS, CPU everywhere, CUDA optional on Linux; `tokenizers` | Check the current DirectML or Windows ML status first. |
| OCR | PP-OCRv5 mobile through `oar-ocr` (uses ort) | |
| Geo | GeoNames `cities500` plus admin1 and country info (CC BY 4.0, offline), tzf-rs (time zones), `gpx`, own Google Timeline parsers | |
| Hashing | blake3, image_hasher (pHash and dHash), ThumbHash placeholders | |
| File system | notify 8 (watch), trash 5 (Recycle Bin) | |
| Secrets | keyring 4 (Windows Credential Manager, macOS Keychain, Secret Service) | API keys never reach the WebView. |
| Map | MapLibre GL JS 6, pmtiles 4, @protomaps/basemaps 5 | Monochrome style, offline world basemap. |
| Types | tauri-specta 2 (release candidate) or ts-rs | Typed commands and events. |
| Tests | cargo test, vitest 5, Playwright 1.64, tauri-driver smoke test on Windows | |

### 7.2 Repository layout

```
apps/desktop/           Tauri app: src-tauri/ (thin Rust layer) and src/ (Svelte UI)
crates/core/            data model, DB and migrations, scan, rules, planner, journal, jobs
crates/media/           decoding, thumbnails, video (FFmpeg), hashing, image stats
crates/meta/            metadata read and write, sidecars, Immich mapping
crates/ai/              embeddings, classifiers, clustering, OCR, model clients, cost tracking
crates/geo/             gazetteer, time zones, tracks and importers, interpolation
tools/fixtures/         fixture library generator with ground truth
e2e/                    UI tests and the Immich round-trip test
docs/                   plan, decisions, architecture, privacy, ai-and-cost, immich, shortcuts
packs.json              downloadable packs: URL, size, SHA-256, license
```

`crates/*` have no Tauri dependency and are tested on their own.

### 7.3 Process and data

- **One process.** Async commands, a priority job system, and helper
  processes for ExifTool (stay-open pool), FFmpeg and ffprobe, and optional
  llama.cpp.
- **App data** lives in `%LOCALAPPDATA%\Roll\` (and the platform equivalents):
  - `library.db`;
  - `thumbs.db` (thumbnail blobs);
  - `models/`, `maps/`, `geodata/` (downloaded packs, verified by SHA-256
    from `packs.json`);
  - `logs/`.

  Inside photo folders Roll only moves files and writes their metadata and
  sidecars. It never puts databases, caches or hidden folders there.
- **Main tables:**
  - `assets`: path relative to the root, size, mtime, quick hash, BLAKE3,
    type, size in pixels, duration, orientation, date taken (UTC and local)
    with offset, location with accuracy, camera, lens, software, markers,
    group, signature, category, rating, favorite, description, and a
    `missing` flag;
  - `groups`, `categories`, `tags` and `asset_tags`, `events`;
  - `field_history` (provenance and undo);
  - `embeddings`, `ocr`, an FTS5 index, `ai_results` (model, prompt version,
    output, cost);
  - `jobs`, `ops` and `batches` (the journal), `rules`, `settings`.
- **File identity.** Files moved outside Roll are re-linked by size and quick
  hash, then confirmed with BLAKE3, so AI results and edits survive.
- **Jobs and priorities:** visible thumbnails, then metadata, then other
  thumbnails, then embeddings, then OCR, then AI. Jobs are persistent,
  pausable and resumable, and a resource governor sets threads, GPU use and
  "only when idle".

### 7.4 UI and backend boundary

- **The front end never loads 100k asset objects.** It asks for timeline
  sections: per month, the counts plus packed arrays of ids, aspect ratios
  and flags, sent as binary `tauri::ipc::Response` (typed arrays). Details
  are fetched lazily per visible range.
- **Thumbnails and previews** are served by an asynchronous custom URI scheme
  (`register_asynchronous_uri_scheme_protocol`), read from `thumbs.db`.
  - Long cache lifetimes, keyed by content version.
  - Build URLs with Tauri's helper. On Windows custom schemes appear as
    `http://<scheme>.localhost/`.
- **Layout:** the justified layout is computed in a Web Worker. DOM nodes are
  recycled with absolute positioning. A ThumbHash placeholder shows
  instantly, then the 256 px thumbnail, then 1024 px in the viewer.
- **Thumbnail sizes:** 256 px (WebP, about 10 KB, about 1 GB per 100k
  assets) and 1024 px on demand in an LRU cache (default 5 GB, configurable).
  Embedded EXIF and HEIF thumbnails give the first paint.
- **Security:**
  - A strict CSP: no remote origins except the optional tile server.
  - All network calls (AI, downloads) go through Rust.
  - Tauri capabilities are least-privilege.

### 7.5 Performance budgets (100k assets, owner's PC)

| What | Budget |
|---|---|
| First thumbnails visible after adding the library | under 10 s |
| Metadata index | under 10 min |
| All 256 px thumbnails | under 30 min |
| Embeddings | under 30 min on GPU, under 3 h on CPU only |
| Timeline scrolling | 60 fps |
| Scrubber jump | placeholders under 100 ms, thumbnails under 300 ms |
| Search | under 300 ms |
| Map | 100k points at 60 fps |
| Warm start | under 2 s |
| Memory | under 1.5 GB RSS, excluding model weights |

Build a 100k synthetic stress library (9.1) and measure every budget in CI or
in a documented local benchmark.

### 7.6 Video

- **Posters and hover previews:** a poster at 10% of the duration (skipping
  black frames) and 3 keyframes for AI and hover scrubbing.
- **Playback:** use the WebView natively when
  `canPlayType`/`MediaSource.isTypeSupported` says yes. Otherwise transcode a
  preview proxy on demand (H.264 through hardware encoders, or AV1 or VP9),
  cache it, and stream it with range requests through the custom scheme.

## 8. Design

The design is the same family as the owner's other tools: Vercel-like black
and white, the Geist and Geist Mono fonts (OFL), and color only from photos,
maps and the danger state.

```css
:root {
  --bg: #ffffff; --fg: #000000; --muted: #666666; --border: #eaeaea;
  --surface: #fafafa; --border-strong: #c9c9c9; --danger: #ee0000;
  --overlay: rgba(0, 0, 0, 0.12);
  --sans: 'Geist', ui-sans-serif, system-ui, 'Segoe UI', sans-serif;
  --mono: 'Geist Mono', ui-monospace, Consolas, monospace;
  --t12: 12px; --t13: 13px; --t14: 14px; --t16: 16px; --t24: 24px;
  --r-1: 4px; --r-btn: 6px; --r-sheet: 8px;
  --ease: cubic-bezier(0.16, 1, 0.3, 1);
  color-scheme: light;
}
/* Same dark values in both blocks: System (OS is dark) and Dark (chosen). */
@media (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) {
    --bg: #000000; --fg: #ffffff; --muted: #a1a1a1; --border: #333333;
    --surface: #0a0a0a; --border-strong: #4d4d4d; --overlay: rgba(0, 0, 0, 0.6);
    color-scheme: dark;
  }
}
:root[data-theme='dark'] { /* the same dark values */ }
```

- **Theme:** System, Light or Dark. The viewer always uses a black backdrop.
- **Layout:**
  - native window frame, themed (keep Snap Layouts working);
  - top bar 48 px with search and filter chips, view options and selection
    actions;
  - sidebar 232 px, collapsible to 56 px;
  - info panel 320 px, toggled with I.
- **Grid:** 2 px gaps and square corners. Selection is a 2 px inset ring in
  `--fg` with an 18 px check badge. Hover shows the select circle and
  favorite.
- **Typography and controls:**
  - UI text 13–14 px; numbers and metadata in Geist Mono with tabular
    figures.
  - Buttons 32 and 36 px, list rows 32 px.
  - Icons: Lucide, 16 px, 1.5 px stroke.
- **Motion:** 120–200 ms with `--ease`. The viewer opens with a FLIP zoom
  from the tile. Respect reduced motion.
- **States:**
  - Empty states have one sentence and one action.
  - Loading states use skeleton tiles and ThumbHash, never spinners in the
    grid.
  - Errors say what happened and what to do.
- **Map:** a custom monochrome Protomaps style, light and dark, with
  thumbnail cluster markers in a 1 px border.
- **Accessibility:** AA contrast, visible focus rings, everything reachable
  by keyboard, and labels for screen readers. Alt text is the caption when
  there is one.

Wireframe, main window:

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Roll   [ Search photos, places, text…                ]  Filters  Ctrl K  │
├─────────────┬──────────────────────────────────────────────┬─────────────┤
│ Timeline    │ July 2023 · Lake Garda                  312  │ Info        │
│ Map         │ ▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢  2023 │ 14 Jul 2023 │
│ Folders     │ ▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢   Jul │ 18:42 +2:00 │
│ Events      │ 14 July                                      │ Camera      │
│ ─ Sort ─    │ ▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢  2022 │ Place       │
│ Inbox   312 │ ▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢▢   ··· │ Tags        │
│ Review   48 │                                    scrubber ↑│ Category    │
│ Discover    │                                              │ Why         │
│ Duplicates  │                                              │             │
│ Dates    12 │                                              │             │
│ Places 1.2k │                                              │             │
│ ─ Library ─ │                                              │             │
│ Photos      │                                              │             │
│ Screenshots │                                              │             │
│ Memes       │                                              │             │
│ …           │                                              │             │
│ Activity    │                                              │             │
└─────────────┴──────────────────────────────────────────────┴─────────────┘
```

Map view (left: time slider with the selected range; right: the clicked cluster):

```
┌──────┬──────────────────────────────────────────────────────┬────────────┐
│ 2025 │                                                      │ 312 items  │
│ ▁▂▅▃ │            map: clusters with thumbnails             │ ▢▢▢▢▢▢▢▢▢▢ │
│ 2024 │                                                      │ ▢▢▢▢▢▢▢▢▢▢ │
│ ▃█▆▂ │                   ▣ 24        ▣ 312                  │ ▢▢▢▢▢▢▢▢▢▢ │
│┌2023┐│                                                      │            │
││▅▇▃▁││          ▣ 8                                         │            │
│└2022┘│                                                      │            │
│ ▂▁▁▃ │                                                      │            │
│  ▶   ├──────────────────────────────────────────────────────┤            │
│      │ No location in this range (1,204)  ▢▢▢▢▢▢▢▢ drag →   │            │
└──────┴──────────────────────────────────────────────────────┴────────────┘
```

Keyboard: Ctrl+K palette, / search, Space quick look, Enter open, Esc back, I
info, Ctrl+Z undo, Del to Trash, F favorite, 0–5 rating (viewer), 1–9
category (Review), T tag, D date, L location, Ctrl+A select all.

## 9. Quality

### 9.1 Fixtures

`tools/fixtures` generates a fixture library with a ground-truth manifest,
using ExifTool and FFmpeg. Nothing comes from the owner's photos. It
contains:

- camera JPEGs and HEICs from several cameras, some with wrong clocks (+1 y
  2 h, a time zone left at home, drift);
- phone photos with GPS along a fake trip, and a matching GPX and Google
  Timeline file (all three formats);
- Live Photo pairs (`ContentIdentifier`), RAW+JPEG pairs, edited and original
  pairs, `.aae` and `.xmp` sidecars, Takeout `.json` sidecars;
- iOS screenshots (PNG with the XMP marker, and JPEG with the EXIF marker),
  Android screenshots, screen recordings;
- meme-like, social-like (1080×1350, no EXIF), wallpaper-like, document-like
  and junk images (black, blurred);
- duplicates: exact copies, resized and re-encoded copies, WhatsApp-style
  copies;
- videos: H.264, HEVC, rotated, with QuickTime metadata;
- awkward paths: Unicode names, long paths over 260 characters, case-only
  collisions, read-only files.

Add a small CC0 or public-domain set of real photos (memes, screenshots,
landscapes, documents) for model-based tests, with licenses recorded.

A stress mode creates 100k assets from templates, with varied metadata and
cheap pixels, for the performance budgets.

### 9.2 Tests

- **Unit tests:**
  - planner, templates and journal, including crash recovery (kill during
    apply, then restart);
  - undo (verified with a hash manifest);
  - time math (offsets, DST, drift), interpolation, format detection;
  - the tag serializer, the cost estimator, the rules engine, the cluster
    logic.
- **Metadata round trip:** write, then read back with ExifTool and with
  nom-exif, for JPEG, HEIC, PNG, MOV, MP4 and sidecars, on Windows, Linux and
  macOS.
- **Accuracy:** rule-decidable fixtures must be 100% correct. Model-based
  accuracy is reported, not asserted, except for documented floors on the
  CC0 set.
- **UI:**
  - Playwright against the Svelte app with a mocked IPC layer: grid, viewer,
    review keyboard flow, map slider, organize preview, settings;
  - a tauri-driver smoke test on Windows CI.

### 9.3 Immich round trip

`e2e/immich` starts Immich v3.x with Docker Compose. It uploads the fixtures
twice, once with the Immich CLI (with sidecars) and once with Roll's API sync.
It then asserts through the API that:

- tags (hierarchical), date and time zone, GPS, description and rating match;
- Live Photos are linked;
- only selected items are in Immich, events are albums, and nothing is
  uploaded twice;
- after the next sync, a later date, place or tag change in Roll shows up in
  Immich;
- re-foldering the local archive changes nothing in Immich;
- "Keep local only" moves the item to Immich's trash and leaves the local
  file untouched.

Run it before every milestone that touches metadata or sync, and in CI where
Docker is available.

### 9.4 CI and releases

- **GitHub Actions** on windows-2025 (primary), ubuntu-24.04 and macos-15:
  `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `pnpm check`,
  `pnpm test`, Playwright, and a build.
- **Releases** (on a tag, after asking the owner):
  - Windows NSIS installer (per-user, no admin), then AppImage, deb and dmg;
  - SHA256SUMS;
  - Tauri updater metadata, signed with the owner's minisign key.

## 10. Milestones

Each milestone ends with its acceptance criteria met, CI green, docs updated
and a short note to the owner.

**M0 Foundations**
- Monorepo layout, toolchains, CI on 3 OSes.
- Tauri and Svelte shell with the design system (tokens, fonts, buttons,
  inputs, sheets, sidebar, top bar, empty states).
- Fixture generator, `CLAUDE.md`, `docs/plan.md`, `docs/decisions.md`.
- *Accept:* the app opens to the first-run screen in light and dark, CI
  passes, and fixtures generate on Windows.

**M1 Library**
- Roots, a parallel scan with nom-exif, the DB.
- Thumbnails for every format, including HEIC, RAW previews and video.
- Timeline with scrubber, viewer with video (HEVC fallback), info panel,
  folders view.
- Basic filters (date, type, camera, has location), file watching, Activity
  panel.
- *Accept:* every fixture format displays correctly. The 100k stress library
  meets the budgets in 7.5 for indexing, thumbnails, scrolling and memory.

**M2 Organize core**
- Categories, tags and provenance; the rules engine with defaults.
- Folder templates, plan preview, apply with journal, undo and crash
  recovery.
- Recycle Bin trash, sidecar and embedded writes through ExifTool, long
  paths, conflicts.
- *Accept:*
  - Rule-decidable fixtures land in the right folders.
  - Undo of moves restores a byte-identical tree.
  - Undo of metadata writes restores the previous values, with an unchanged
    `ImageDataHash`.
  - On Windows, Explorer and the Photos app show the written date, tags,
    rating and GPS. This is checked by hand and documented.
  - Killing the app during apply, then restarting, gives a consistent state.
  - The Immich round trip passes for tags, dates and GPS.

**M3 Dates, places, map**
- Fix dates: detection, reference shift, time zone, drift, missing-date
  inference.
- Fix places: time neighbors, all location-history formats, events, names,
  manual tools, accuracy radius, time zone follow-up.
- Map with the left time slider, clusters with thumbnails, and the
  no-location tray.
- Offline gazetteer and time zones.
- *Accept:*
  - The fixture camera set with +1 y 2 h is fixed in 5 actions or fewer.
  - 500 photos are geotagged from GPX in one action.
  - The map holds 60 fps with the stress library.

**M4 Local AI**
- A pack manager (consent, size, SHA-256).
- SigLIP 2 embeddings on GPU and CPU.
- Hybrid search; zero-shot and learned classifiers.
- Review, Teach and Discover.
- Smart time offset by visual pairing.
- Duplicates, junk detection, OCR, the Private guard.
- *Accept:*
  - On the CC0 set, local-only sorting auto-accepts at least 60% at 97% or
    better measured precision.
  - Search finds the documented queries.
  - Discover groups the size-signature fixtures in one cluster each.

**M5 Decision models and cost**
- An OpenAI-compatible client for local servers and OpenRouter.
- A model picker with live prices and a ZDR filter.
- The model check.
- Batching, JSON schema, estimate, budget cap, live cost, consent,
  resumable runs.
- Captions, natural-language queries, optional reranking.
- *Accept:*
  - Estimates land within ±25% of the actual cost on a 1,000-item run.
  - Projected cost is $3 or less per 100k with the default settings.
  - The cap stops the run exactly.
  - A crash mid-run resumes without double charges.

**M6 Events, Immich, polish**
- Event detection and naming; event folders and albums.
- Immich sync through the API: selection, upload, albums and sharing,
  pushing metadata changes, and removal with confirmation. Plus the CLI script
  export and the Immich guide.
- Add photos (4.11): the guided import of an event, with date and place
  checks against the archive and the Immich album.
- Importers for Google Takeout JSON and WhatsApp exports.
- Onboarding and the guided first sort; full Settings.
- Accessibility, performance and copy passes; Windows installer; user docs.
- *Accept:* on a copy of the owner's library, with their permission, the
  owner can go from a dump to a sorted, reviewed and applied archive, plus a
  first sync of only the selected items to a test Immich, in one evening of
  work (the machine may run overnight). A fixture camera card with a wrong
  clock and no GPS gets imported as an event: correct dates and places, its
  own folder and an Immich album, in under 2 minutes of the owner's time.

**M7 Later**
- Linux and macOS packaging polish.
- People: face detection and clustering with Apache-licensed models (for
  example OpenCV Zoo YuNet and SFace), naming, and MWG regions written for
  Immich face import.
- One-click local AI (bundled llama.cpp).
- Offline region map downloads.
- Auto-updates, code signing.

## 11. Manual steps for the owner

- Optional: an OpenRouter API key, entered in Roll's settings. Never paste it
  into chat, code or the repo. Add $5 of credit.
- Optional: install Ollama and pull a suggested vision model (Roll shows the
  command).
- The Immich server URL and an API key with permission for assets, albums and
  tags. Development uses a local test Immich in Docker. The real server is
  only used after the owner says so.
- When the first release is ready: a minisign key pair for updates
  (`pnpm tauri signer generate`), stored as GitHub secrets, and a decision
  on Windows code signing (SmartScreen warns on unsigned installers).
- Before M6: point Roll at a copy of the real library, or confirm that
  in-place organizing with undo is fine.
