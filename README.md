# Roll

A local photo and video library that sorts itself.

> **Status: blueprint.** There's no code yet. [PROMPT.md](PROMPT.md) is the
> full spec, written to be handed to Claude Code.

Roll turns a phone dump of 100,000+ photos and videos into a library sorted by
year:

- **A complete archive on your PC.** Camera photos go to `2023/Photos`,
  confirmed events to `2023/2023-07 Lake Garda`. Screenshots, memes, social
  media saves, documents and wallpapers get folders of their own. Everything
  is kept and readable in Windows Explorer.
- **A curated Immich.** Immich only gets what you pick for your phone and for
  sharing, by default your photos and events. Roll keeps that in sync: later
  fixes, albums and shared albums.
- **Dates and places, fixed in bulk.** A wrong camera clock is corrected from
  one reference photo. Missing places are filled from nearby photos, location
  history, or a pin on the map.
- **A full library.** Timeline, map with a time slider, filters, search, and a
  viewer for photos and videos.
- **Tags as the source of truth.** Everything is saved as standard metadata
  tags that Immich reads, and folders can be rebuilt from tags at any time.

It works on your folders in place, never deletes anything permanently, and
can undo every change.

## AI, local or for cents

- Rules and local models (SigLIP 2 embeddings, OCR) sort most items for free.
- The rest goes to a small vision model, either running locally (Ollama, LM
  Studio or llama.cpp, free) or through OpenRouter with zero data retention,
  at about $0.30–2.50 per 100,000 items.
- Only 512 px thumbnails without metadata leave your machine, and only if you
  turn that on.

## Stack

Tauri 2, Rust, Svelte 5, SQLite, ONNX Runtime, ExifTool, FFmpeg and MapLibre.
Windows first, then Linux and macOS.

## Build it with Claude Code

```bash
git clone https://github.com/mrghxst/roll
```

Open Claude Code in the `roll` folder and paste the contents of
[PROMPT.md](PROMPT.md), or say "Build Roll as specified in PROMPT.md".

The research behind the spec (AI prices, Immich compatibility and a survey of
a real library) is in [docs/research.md](docs/research.md).

## License

MIT
