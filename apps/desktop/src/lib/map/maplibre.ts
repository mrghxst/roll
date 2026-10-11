// Loads MapLibre once, with its worker bundled by Vite. MapLibre otherwise
// looks for the worker next to its own module, which Vite moves, and finds
// nothing on non-http schemes such as tauri:// on macOS.

import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url';

let lib: Promise<typeof import('maplibre-gl')> | null = null;

export function maplibre(): Promise<typeof import('maplibre-gl')> {
  lib ??= import('maplibre-gl').then((m) => {
    m.setWorkerUrl(new URL(workerUrl, location.href).href);
    return m;
  });
  return lib;
}
