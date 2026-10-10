# Research notes

Collected on 2026-10-10 while writing [PROMPT.md](../PROMPT.md). Prices and
versions change: re-check them before relying on them.

## 1. A real library

The owner's iPhone export was read without opening any image content. Only
file headers were parsed: EXIF, XMP, JPEG frame headers and QuickTime `moov`
boxes.

**Size and layout**

- 27,331 files, 113 GB, from 2010 to 2025 (most from 2018 to 2025).
- Year folders only, with no subfolders.
- Names were rewritten by the export tool: `IMG_<device>_<year> - <n>.jpeg`.
- HEIC was converted to JPEG: 24,516 `.jpeg` files (64 GB) and 2,815 `.mov`
  files (49 GB).

**Photos**

| Signal | Count | Share |
|---|---|---|
| Camera make present | 7,956 | 32% |
| iOS screenshot marker (EXIF `UserComment = "Screenshot"`) | 2,593 | 11% |
| No camera data at all | 13,967 | 57% |
| GPS present | 1,567 | 6% |
| `DateTimeOriginal` and `OffsetTimeOriginal` present | 24,516 | 100% (filled by the export) |
| Apple `ContentIdentifier` (Live Photo still) | 91 | none of them has its video |
| MPF (depth or gain map) | 114 | |

- Several iPhones and an iPad, plus 6 dedicated cameras: about 1,250 photos,
  none with GPS.
- The most common sizes with no camera data:

  | Size | Files |
  |---|---|
  | 854×1590 | 4,122 |
  | 828×1792 (iPhone 11 screen) | 1,464 |
  | 1024×1024 | 854 |
  | 1152×2048 | 689 |
  | 1536×2048 | 431 |
  | 896×1152 | 406 |

  Single sizes like these point to a single source app.
- Software tags are rare. Instagram, Canva, Photoshop, GIMP and Capture One
  appear only a few times.

**Videos**

- Camera make present: 268 (10%). Location present: 69 (2.5%).
- 2,547 have no camera make. They are saved clips at sizes typical of social
  apps: 576×1024, 480×848, 720×1280, 540×960, 1080×1920 and 640×1136.
- Durations: 191 under 4 s, 1,997 from 4 to 30 s, 589 from 30 s to 5 min, 38
  over 5 min.
- Codecs: 2,808 H.264 and 7 HEVC.

**What this means for the design**

- File names can't be relied on.
- The screenshot marker is a free, certain rule.
- About 60% of items need content-based sorting.
- Size and encoder signatures let one decision cover thousands of files.
- Location fixing is a core feature.
- Dates on saved items are save dates, not capture dates.

**Owner's PC:** Windows 11 25H2, i9-9900KF, 32 GB RAM, RTX 2080 Ti (11 GB),
NVMe.

## 2. AI prices on OpenRouter

These come from the public `GET /api/v1/models` (458 models) and
`GET /api/v1/endpoints/zdr` (940 endpoints).

**Cheapest image-capable models with a zero-data-retention endpoint:**

| Model | ZDR provider | $/M input | $/M output |
|---|---|---|---|
| `inclusionai/ling-3.0-flash-vl` | Novita | 0.021 | 0.062 |
| `google/gemma-3-4b-it` | DeepInfra | 0.05 | 0.10 |
| `google/gemma-3-12b-it` | DeepInfra | 0.05 | 0.15 |
| `google/gemma-4-26b-a4b-it` | Io Net | 0.048 | 0.21 |
| `openai/gpt-5-nano` | Azure | 0.05 | 0.40 (plus reasoning tokens) |
| `amazon/nova-lite-v1` | Amazon Bedrock | 0.06 | 0.24 |
| `mistralai/mistral-small-3.2-24b-instruct` | DeepInfra | 0.075 | 0.20 |
| `google/gemini-2.5-flash-lite` | Google Vertex | 0.10 | 0.40 |

Newer Gemini Flash-Lite versions (3.1, 3.5) cost more: $0.25–0.54 per million
input tokens.

**Estimated cost per 100k items**, assuming every item goes to the model, 8
images per request, about 256–330 image tokens, 30 hint tokens, a 400-token
prompt shared by 8 items, and 35 output tokens:

| Model | ≈ $ per 100k |
|---|---|
| Ling 3.0 Flash VL | 1.0 |
| Gemma 3 12B | 2.2 |
| Gemma 4 26B A4B | 2.5 |
| Gemini 2.5 Flash-Lite | 4.8 |

With rules and local models first, 30–50% of items reach the model: about
$0.30–2.50 per 100k.

**Batch API:** about half the price, but not usable for private photos.
Multimodal input in batch is URL-only (public image URLs, not base64), and
OpenRouter stores batch inputs and results as JSONL for 30 days.

**Privacy routing:** `provider.zdr = true` restricts routing to ZDR
endpoints, and `provider.data_collection = "deny"` excludes providers that
collect data. `require_parameters = true` keeps structured outputs working.

**Cost tracking:** every response includes `usage.cost`. The old
`usage: { include: true }` parameter is no longer needed.

## 3. Immich compatibility (v3.3.1, released 2026-10-08)

From `server/src/services/metadata.service.ts`:

- **Tags:**
  - Immich reads `TagsList` first, then `HierarchicalSubject` (with `|`
    turned into `/`), then `Keywords`. Tags are hierarchical with `/`.
  - Sidecar values are merged over the file's own values
    (`{ ...mediaTags, ...videoTags, ...sidecarTags }`).
- **Dates:**
  - Dates come from the sidecar first, including the time zone.
  - The tag order is `SubSecDateTimeOriginal`, `SubSecCreateDate`,
    `DateTimeOriginal`, `CreationDate`, `CreateDate`, `MediaCreateDate`,
    `DateTimeCreated`, `GPSDateTime`, `DateTimeUTC`, and others.
- **Other fields:**
  - Description: `ImageDescription` or `Description`.
  - Rating: `Rating` 1–5.
  - Live Photo link: `ContentIdentifier` or `MediaGroupUUID`.
  - Faces: `RegionInfo` (MWG regions), when face import is on.
- **Sidecar files:**
  - Candidates are `<file>.<ext>.xmp`, then `<file>.xmp`.
  - Immich's own sidecar write job writes `Description`, `ImageDescription`,
    `DateTimeOriginal` (with time zone), `GPSLatitude`, `GPSLongitude`,
    `Rating` and `TagsList`.
- **Reverse geocoding:** local, using GeoNames `cities500.txt`.

From the CLI (`packages/cli/src`):

- `immich upload` uploads the sidecar with each file, preferring
  `<file>.<ext>.xmp` over `<file>.xmp`.
- `--album` creates albums named after each file's parent folder.
  `--album-name <name>` puts everything into one album.
- `--visibility` accepts the asset visibility values: `archive`, `timeline`,
  `hidden` and `locked`.

From the OpenAPI spec (`open-api/immich-openapi-specs.json`, 3.3.0-rc.0 on
`main`), these are the endpoints Roll's sync needs:

| Endpoint | Use |
|---|---|
| `POST /assets` | Upload, with `assetData`, `sidecarData`, `visibility`, `isFavorite`, `livePhotoVideoId`, `fileCreatedAt` and `fileModifiedAt` |
| `POST /assets/bulk-upload-check` | Skip files Immich already has |
| `PUT /assets` | Bulk update of `dateTimeOriginal`, `dateTimeRelative`, `timeZone`, `latitude`, `longitude`, `description`, `rating`, `isFavorite` and `visibility` |
| `DELETE /assets` | Delete by `ids`; without `force`, assets go to Immich's trash |
| `PUT /tags/assets` | Bulk tagging |
| `POST /albums`, `PUT /albums/{id}/assets` | Albums |
| `PUT /albums/{id}/users` | Share an album |

**Models:** Immich publishes ONNX exports of SigLIP 2 on Hugging Face, for
example `immich-app/ViT-B-16-SigLIP2__webli`. Its visual model is 372 MB and
its text model 1.13 GB (fp32). Larger variants up to SO400M and gopt also
exist.

## 4. Local models and runtimes

- **Ollama v0.40.2.**
  - `gemma4`: e2b, e4b, 12b, 26b, 31b.
  - `qwen3-vl`: 2b, 4b, 8b, 30b, 32b, 235b.
  - `qwen3.5`: 0.8b to 122b.
  - `gemma3`: 270m to 27b.
- **llama.cpp v0.6.0.**
- **Rerankers and embeddings:** `Qwen/Qwen3-VL-Reranker-2B` and `-8B`
  (Apache-2.0, January 2026, GGUF builds available), and
  `Qwen/Qwen3-VL-Embedding-2B` and `-8B`.
- **OCR:** PaddleOCR PP-OCRv5 mobile detection and recognition, with ONNX
  exports.

## 5. Maps

- **Protomaps basemap:** a world extract to zoom 6 is about 46–60 MB. Each
  extra zoom level roughly doubles the size, and the full planet is about
  120 GB. `pmtiles extract` uses HTTP range requests. OpenStreetMap
  attribution (ODbL) is required.
- **OpenFreeMap public instance:** no registration, no API keys, no cookies,
  and no limits on views or requests. Attribution is required, and commercial
  use is allowed.

## 6. Location history formats

| Format | Fields |
|---|---|
| Google Maps phone export (`Timeline.json`) | `semanticSegments[]` with `startTime`, `endTime`, `timelinePath[]` `{ point: "50.0506312°, 14.3439906°", time }`, `visit.topCandidate.placeLocation.latLng`, `activity.start/end.latLng`; `rawSignals[].position` `{ LatLng, accuracyMeters, timestamp }`; `userLocationProfile.frequentPlaces[]` (HOME, WORK) |
| Legacy `Records.json` | `locations[]` with `latitudeE7`, `longitudeE7`, `timestamp` (or `timestampMs`), `accuracy` |
| Semantic Location History | monthly `timelineObjects[]` with `placeVisit.location` (E7) and `activitySegment` (with `waypointPath.waypoints[]` `{ latE7, lngE7 }`) |

## 7. Versions on 2026-10-10

**Rust crates:**

| Crate | Version |
|---|---|
| tauri | 2.12.2 |
| ort | 2.0.0-rc.13 |
| rusqlite | 0.40.2 |
| nom-exif | 3.8.0 |
| libheif-rs | 3.0.0 |
| notify | 8.2.0 |
| trash | 5.2.9 |
| keyring | 4.2.0 |
| image | 0.25.10 |
| fast_image_resize | 6.1.0 |
| zune-jpeg | 0.5.15 |
| blake3 | 1.8.7 |
| rayon | 1.12.0 |
| tokio | 1.53.2 |
| tantivy | 0.26.2 |
| usearch | 2.26.4 |
| ffmpeg-sidecar | 2.6.0 |
| image_hasher | 3.1.1 |
| tzf-rs | 2.1.2 |
| gpx | 0.10.0 |
| pmtiles | 0.24.1 |
| rawler | 0.8.0 |
| oar-ocr | 0.10.0 |
| tauri-specta | 2.0.0-rc.25 |

**npm packages:**

| Package | Version |
|---|---|
| svelte | 5.57.2 |
| vite | 8.3.4 |
| @tauri-apps/api | 2.12.2 |
| @tauri-apps/cli | 2.12.1 |
| maplibre-gl | 6.13.0 |
| pmtiles | 4.5.0 |
| @protomaps/basemaps | 5.7.2 |
| vitest | 5.0.3 |
| @playwright/test | 1.64.0 |
| geist | 1.7.2 |

## Sources

- [OpenRouter Batch API quickstart](https://openrouter.ai/docs/batch-quickstart)
- [OpenRouter provider routing](https://openrouter.ai/docs/features/provider-routing)
- [OpenRouter usage accounting](https://openrouter.ai/docs/use-cases/usage-accounting)
- [OpenRouter models API](https://openrouter.ai/api/v1/models) and
  [ZDR endpoints](https://openrouter.ai/api/v1/endpoints/zdr)
- [Immich metadata service](https://github.com/immich-app/immich/blob/main/server/src/services/metadata.service.ts),
  [CLI upload](https://github.com/immich-app/immich/blob/main/packages/cli/src/commands/asset.ts),
  [XMP sidecars](https://docs.immich.app/features/xmp-sidecars) and
  [tags](https://docs.immich.app/features/tags/)
- [Immich SigLIP 2 ONNX export](https://huggingface.co/immich-app/ViT-B-16-SigLIP2__webli)
- [Qwen3-VL-Reranker-2B](https://huggingface.co/Qwen/Qwen3-VL-Reranker-2B)
- [Protomaps downloads](https://docs.protomaps.com/basemaps/downloads) and
  [getting started](https://docs.protomaps.com/guide/getting-started)
- [OpenFreeMap](https://openfreemap.org/)
- [What's inside your Google Timeline export (Dawarich)](https://dawarich.app/blog/whats-inside-your-google-timeline-export/)
