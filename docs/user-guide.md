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
  with country outlines; Settings › Network › Online map tiles adds
  streets and place names from OpenFreeMap.
- **Search** (top bar, or press /): words like `videos summer 2019 italy`, file names or
  tags. Roll turns what it understands into filter chips. The filter panel
  adds categories, cameras, places, ratings and more. When AI is on,
  words Roll did not understand can go to the AI model ("Ask AI about
  these words"); only those words are sent, never photos.
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

- **"ExifTool was not found":** the installer brings it; for a development
  build, put `exiftool.exe` on the PATH or set `ROLL_EXIFTOOL`.
- **A drive is missing:** items on it show as missing until it is back; Roll
  does not forget them.
- **Roll crashed:** `%LOCALAPPDATA%\Roll\logs` has a `crash-…txt` report.
  Interrupted file operations are finished or rolled back on the next start.
