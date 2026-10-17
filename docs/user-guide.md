# Roll user guide

Roll turns a big dump of photos and videos into an archive sorted by year,
with dates and places fixed, and sends the part you choose to Immich. It
works on your folders in place, never deletes anything permanently, and every
change can be undone from History.

## Install

Run the installer (`Roll_<version>_x64-setup.exe`). It installs for your user
only, to `%LOCALAPPDATA%\Programs\Roll`, no administrator rights needed, and
brings ExifTool (metadata) and FFmpeg (HEIC, videos) with it. Windows may
warn that the installer is from an unknown publisher: it is not signed yet.

On a Mac with Apple silicon (M1 or later; Intel Macs are not supported),
open the `.dmg` and drag Roll to Applications; ExifTool comes with it, FFmpeg from Homebrew (`brew install ffmpeg`) adds HEIC photos and
videos. The first start needs right-click › Open, since the app is not
notarized yet. On Linux (glibc 2.38 or later: Ubuntu 24.04, Debian 13,
Fedora 39), the `.deb` and the AppImage bring ExifTool and FFmpeg; the
AppImage needs Perl, which nearly every distribution has.

Roll keeps its database, thumbnails and logs in `%LOCALAPPDATA%\Roll`. Your
photos stay where they are. Uninstalling keeps that folder; delete it by hand
to start over.

## First run

1. **Library folder:** the folder with your complete archive. More folders
   can be added later in Settings › Library.
2. **How to save changes:**
   - *In the files and as sidecars* (recommended): Explorer and the Photos
     app see the fixed dates, places and tags; Immich, digiKam and Lightroom
     read the `.xmp` sidecars. Only metadata is rewritten, never the picture.
   - *Sidecars only:* the originals stay byte for byte as they are.
3. **AI:** off, local (Ollama, LM Studio, llama.cpp) or OpenRouter. Rules
   and local models work without it.
4. **Immich** (optional): server address and API key. Nothing is sent until
   you have looked at the first sync plan.

The grid fills while Roll indexes. Afterwards **Get started** walks through
the first sort; it stays in the sidebar until you close it, and the command
palette brings it back.

## Finding your way

- **Timeline:** everything by month and day. The scrubber on the right jumps
  through years. Ctrl+wheel changes the tile size.
- **Folders, Map, Events:** the same items by folder, by place and by
  event. The map is a globe that flattens as you zoom in. The time slider
  on its right picks the period: drag the handles or the range, click a
  year, scroll to zoom the time axis, or press play. It works offline
  with country outlines; **Map detail** (the layers icon) adds a world
  map, offline areas down to every building, or the online map (see
  [Map detail](#map-detail)).
- **Search** (top bar, or press /): words like `videos summer 2019 italy`, file names or
  tags. Roll turns what it understands into filter chips. The filter panel
  adds categories, cameras, places, ratings and more. When AI is on,
  words Roll did not understand can go to the AI model ("Ask AI about
  these words"); only those words are sent, never photos.
- **Best matches:** results stay in date order; the list icon next to the
  search shows the 50 that match the words best, by text and by what the
  pictures show. With a decision model, "Better ranking" has it rate each
  of them (the words and 320 px pictures without metadata; about $0.002 a
  search on OpenRouter, free with a local model).
- **Command palette:** Ctrl+K. Every view and action is there.
- **Viewer:** double-click a tile or press Enter. Arrow keys move, 1–5
  rate (0 clears), F favorites, T tags, Del moves to Roll's trash, Esc
  closes.

Keys in the grid: arrows move, Enter opens, Space shows a quick look,
Shift+arrows and Shift-click extend the selection, Ctrl-click adds to it,
Ctrl+A selects all, Esc clears it. On a selection: F favorites, T tags, L
sets the location, D changes the date, Del moves to Roll's trash. I shows
the details panel, Ctrl+Z undoes the last change.

## Sorting

Every item gets a category: Photos, Screenshots, Memes, Social media,
Wallpapers, Documents, Junk or Unsorted, plus your own.

- **Rules** decide what is certain (the iOS screenshot marker, WhatsApp file
  names, a camera make). See and change them in Settings › Rules.
- **Review** shows the proposals the sorting is least sure about first. Keys
  1–9 pick a category, Enter accepts, X skips, Z undoes. "Accept all at 95%
  or higher" takes the rest, and a small audit keeps an eye on precision.
- **Discover** groups undecided images that share a source (same size and
  encoder, usually one app): one decision sorts the whole group.
- **Sort with AI** (when AI is on in Settings › AI) sends what is still
  uncertain to a decision model, which proposes a category, a short
  caption (searchable) and whether an item shows a private document. The
  proposals wait in Review. With OpenRouter you see the estimate and
  exactly what is sent (512 px thumbnails without metadata) before
  anything goes, and the run stops before it would pass its budget. A
  stopped run continues where it stopped, without paying twice for items
  already answered. The **model check** compares up to three models on
  about 150 of your own items and recommends the cheapest good one.
- **Local AI in one click** (Settings › AI or Sort with AI): Roll downloads
  llama.cpp's server and Qwen3-VL 4B (about 3 GB, from GitHub and Hugging
  Face) and uses it as the decision model. Nothing else needs installing.
  Roll starts the server when a model is needed, stops it after 10 minutes
  without requests and when Roll closes, and it listens only on this
  computer. Ollama or LM Studio still work if you prefer them: enter their
  address in Settings › AI.
- **Duplicates** finds exact copies, smaller copies (the WhatsApp copy of a
  camera photo), look-alikes and bursts, and suggests which to keep. Merging
  keeps the best one and moves the others to Roll's trash.

## Dates

**Dates** lists camera sets that look wrong (a clock that was never set, a
year that disagrees with the folder, no time zone) and items without a
trusted date.

- Pick a reference photo and enter its true time: the whole set shifts.
  Two references correct a clock that drifts.
- Set a time zone without shifting, shift by an amount, or set an exact
  date.
- The preview shows the new date range and which items change year (and
  folder).

## Places

**Places** lists items without a place and suggests one from your location
history (GPX, KML, GeoJSON, Google Timeline and Takeout files: Import
location history…), from your own geotagged photos taken at the same time,
and from folder names. Accept, adjust or reject each group. "Set location…"
works on any selection: search a town (after downloading the place names in
Settings › Models and data), paste coordinates or a maps link.

After setting places Roll offers to set the time zone of items without one.


### Map detail

Out of the box the map draws only country outlines, so nothing is fetched
without your say. **Map detail** (the layers icon on the map, or the
button when the map says it has only outlines or no streets) has three
ways to more:

- **Online map**: streets and every building everywhere, loaded from
  OpenFreeMap as you look; OpenFreeMap sees which areas you look at. The
  same switch is Settings › Network › Online map tiles.
- **World map**: lakes, large rivers, motorways, main roads and cities for
  the whole world, kept on this computer (about 63 MB, a few seconds to a
  minute to download). Zoomed in past a region, it gets coarse; that is
  where offline areas come in.
- **Offline map areas**, below.

### Offline map areas

For streets, buildings, water and place names without the online map,
keep the area on screen in **Map detail** (or press **Keep this area
offline** when the map says it has no streets where you zoomed in): Roll
copies it from Protomaps' OpenStreetMap build. A city or a region up to about
the size of a large lake has every building and footpath, so you can
tell one building of a campus from the next (central Lisbon is 18 MB);
a bigger area has streets and larger buildings (Switzerland is about
500 MB). The download runs in Activity; the area shows while Settings ›
Network › Online map tiles is off. Remove areas in the same sheet. With
the online map on, the map shows every building everywhere.

## Events

**Events** proposes trips (more than 50 km from home) and busy days at home.
Confirm, rename, merge or split them. A confirmed event tags its items
`Event/<name>`, gets its own folder when you organize, and becomes an Immich
album.

## Adding photos

**Add photos** (or drop a folder onto the window, or connect a camera card)
brings a folder from outside the library in:

1. Roll reads it without changing it and shows what's in it and what you
   already have (left out by default).
2. **Dates:** if your phone photos from the same moments look like these,
   Roll proposes the camera's clock offset; the days are shown next to your
   library. Nothing is accepted until you say the dates look right.
3. **Places:** from your photos taken at the same time, one place for the
   rest, or per day.
4. **Event:** the name comes from the folder or the dates and place. Exports
   (Google Takeout, WhatsApp chats) are not one event by default.
5. **Add:** copy (each copy is checked against its original) or move into
   the event folder, with dates, places and tags written. Undo takes the
   copies back out.

A WhatsApp chat export gives every file the time it was sent and its sender
(`From/<name>` tags). A Google Takeout export gets the dates and places from
Google's JSON files and its albums as `Album/<name>` tags.

## Organize

**Organize** shows where every sorted item would go (`2023/Photos`,
`2023/2023-07 Lake Garda`, `Screenshots`…), which folders are new, and any
name conflicts, before anything moves. Apply moves the files and saves the
metadata changes. You can stop at any time; Roll continues where it stopped,
also after a crash.

Items that are not sorted yet stay where they are. Items marked Private wait
until you have looked at them.

## History and undo

Every change is one entry in **History**: an edit, a date fix, an organize
run, an import. Undo puts files and values back as they were. Ctrl+Z undoes
the last change. **Trash** is Roll's own: items there are still on disk;
emptying it sends them to the Windows Recycle Bin.

## Immich

See [immich.md](immich.md): what goes to Immich, the API key's permissions,
how a sync works, and why the Immich app's phone backup should stay off.

## Privacy

- Everything that can reach the internet is listed in Settings › Network,
  and all of it is off until you turn it on.
- Downloads (place names, models) show their size, source and license first.
- API keys live in the Windows Credential Manager, never in files.
- No telemetry. Logs stay in `%LOCALAPPDATA%\Roll\logs`; Settings ›
  Advanced › Clear logs deletes them.
- Items that look like card numbers, IBANs or passports are marked Private:
  they stay out of cloud AI and Immich unless you pick them.

## When something goes wrong

- **"ExifTool was not found":** the installers bring it; for a development
  build, put `exiftool` on the PATH or set `ROLL_EXIFTOOL` (`ROLL_FFMPEG`
  for FFmpeg).
- **A drive is missing:** items on it show as missing until it is back; Roll
  does not forget them.
- **Roll crashed:** `%LOCALAPPDATA%\Roll\logs` has a `crash-…txt` report.
  Interrupted file operations are finished or rolled back on the next start.

## Models on this computer

Settings › Models and data downloads two models that run on this computer,
nothing leaves it:

- **Image understanding** (SigLIP 2, 1.5 GB): Roll looks at every photo
  and video once, then sorts undecided items into categories (only when it
  is at least 97% sure; the rest waits in Review with the reason), finds
  pictures by what they show ("dog", "beach", "receipt"), and suggests the
  clock error of a camera from phone photos of the same scenes in Dates.
- **Text in photos** (30 MB): reads screenshots, documents, memes and saved
  posts, so their words are searchable and cards or IDs in them are marked
  Private.

With the image model you can also:

- **Find similar:** select items and press the find-similar button in the
  selection bar.
- **Teach a category:** select 10–30 examples, press the teach button, and
  name a new category or subcategory (for example Pokémon GO inside
  Screenshots). Roll proposes items that look like them in Review; every
  correction teaches it more.
- **Discover** shows groups of undecided items that look alike, so one
  decision sorts the whole group.

## People

People (sidebar) needs the face models (about 39 MB, downloaded from the
People page or Settings › Models and data). Roll looks for faces in
photos after their thumbnails are made and groups them by who they look
like; someone shows up once three photos of them are found.

- **Name** someone by clicking "Add a name" under their face. The name is
  searchable and goes into the photos' files as a face region at the next
  metadata write (Organize), which Immich imports when its "Import faces"
  setting is on.
- **Merge** two or more groups of the same person: tick them and choose
  Merge.
- **Not this person:** the person icon on a card shows all their faces;
  pick the wrong ones. They are never grouped with that person again.
- **Hide** people you don't care about (strangers in the background).
- In a photo's info panel (I), People shows who is in it; "Who is this?"
  names a face that belongs to nobody yet.

Every change is one step in History and can be undone. Faces and their
numbers stay on this computer.

## Duplicates and best shots

- **Merge all exact copies** shows first what happens: how many identical
  files go to Roll's trash, how much space that frees, and for each group
  which copy stays (the one in your own folders, not in Downloads,
  backups or with names like "(1)" or "copy"). One undo brings everything
  back; files are only deleted when you empty the trash.
- **Bursts and look-alike series** are ranked best first: the sharpest,
  best exposed frame (and, with the image model, the one that looks best)
  is suggested. Click frames to choose which to keep, even several; the
  rest go to Roll's trash in one step.
- **Skip blinks:** with the eye models (about 6 MB, and the face models if
  they are not there yet; **Download** on the Duplicates page or in
  Settings › Models and data), Roll checks every face in a series and
  ranks a frame where someone's eyes are closed below the others, with
  the note "Eyes closed". On portraits labelled by hand it found 4 of 5
  closed eyes and almost never mistook open eyes for closed; a broad
  smile that squeezes the eyes shut can count as closed. Without the eye
  models the image model only guesses ("Eyes may be closed").
