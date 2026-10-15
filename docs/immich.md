# Roll and Immich

Your library folder on this computer is the complete archive and the master
copy. Immich gets the part you choose to see on your phone, show friends and
share. Immich keeps its own copy of what it gets, so re-foldering the archive
never changes anything in Immich.

## What goes to Immich

- Items in categories whose Immich default is on: Photos, unless you change
  it in Settings › Categories.
- Confirmed events, each as an album (Immich page › Event albums; switch an
  event off to keep it local).
- Anything you mark **Send to Immich** (selection bar › Immich, or the
  command palette). **Keep local only** keeps an item out even when its
  category or event would send it.
- Private items go only when you mark them Send to Immich, and then in
  Immich's Locked folder.

People you name in Roll go into the files as face regions (MWG). Immich
reads them when its **Import faces** option is on (off by default; under
Administration › Settings, in Metadata or Machine Learning depending on
the Immich version). For items Immich already has, run its metadata
extraction job again.

Tiles of items in Immich carry a small cloud badge, and the filter panel can
show what is sent, pending or local.

## Connecting

1. In Immich: Account settings › API keys › New API key. Give it these
   permissions: `asset.upload`, `asset.update`, `asset.delete`,
   `tag.create`, `tag.asset`, `album.create`, `album.read`,
   `albumAsset.create`, `albumUser.create`, `user.read`.
2. In Roll: Settings › Immich: the server address (for example
   `https://photos.example.com`) and the key. The key is stored in the
   system keychain (Windows Credential Manager) and never shown again.
3. On the Immich page, turn Immich on. Until then Roll does not contact the
   server.

Roll targets Immich 3.x and was checked against the 3.3.1 API.

## A sync

The Immich page shows the plan first: how many items to send (and their
size), how many to update because a date, place, tag, rating or description
changed here, and which albums are new.

- Before uploading, Roll asks the server which files it already has (by
  SHA-1); those are linked, not sent again.
- Live Photos go as a pair: the video first, then the still linked to it.
- Later edits in Roll are pushed on the next sync.
- Items you switch to Keep local only, or move to Roll's trash, leave Immich
  only when you tick **Move them to Immich's trash** for that sync. They go
  to Immich's trash (kept 30 days there), never deleted outright. Files on
  this computer are never touched by a sync.
- A sync runs in the background (Activity shows it), can be stopped, and
  continues where it stopped next time. Each one lands in History.

## Without the API: the CLI script

Immich page › Save a script… writes a script for the Immich command-line
tool (`npm install -g @immich/cli`, then `immich login <address> <key>`).
It sends the folders that hold items chosen for Immich, with their `.xmp`
sidecars, and event folders as albums named after the event. The CLI sends
whole folders, so items marked Keep local only inside them go too. The
script never contains your API key.

## Two things to avoid

- **No external library on the archive.** Pointing an Immich external
  library at your library folder would show everything, screenshots and
  documents included.
- **Leave the Immich app's phone backup off.** With it on, Immich gets the
  whole camera roll, not your selection. New photos reach the archive
  through your usual export and Roll's Inbox, and from there go to Immich
  when they are chosen.
