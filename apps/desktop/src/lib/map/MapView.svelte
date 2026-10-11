<script lang="ts">
  import 'maplibre-gl/dist/maplibre-gl.css';
  import type { Feature, FeatureCollection, Point, Polygon } from 'geojson';
  import type { GeoJSONSource, Map as MlMap, Marker } from 'maplibre-gl';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { api } from '../ipc/api';
  import type { MapPoints } from '../ipc/mappoints';
  import { applyPalette, baseStyle, loadCountries, metresPerPixel, palette, ring } from './basemap';
  import { maplibre } from './maplibre';

  type Props = {
    points: MapPoints;
    /** Indices into `points` to show (the time window). */
    visible?: Uint32Array | null;
    heatmap?: boolean;
    /** A cluster or point was clicked. */
    onopen?: (ids: number[]) => void;
    /** Items were dragged from the tray onto the map. */
    ondropids?: (ids: number[], lat: number, lon: number, accuracyM: number) => void;
    /** Shift-drag rectangle or Alt-drag lasso. */
    onselect?: (ids: number[]) => void;
    fitKey?: string;
  };

  let { points, visible = null, heatmap = false, onopen, ondropids, onselect, fitKey = '' }: Props = $props();

  let el: HTMLDivElement | undefined = $state();
  let map: MlMap | null = $state(null);
  let ready = $state(false);
  let ml: typeof import('maplibre-gl') | null = null;
  let markers = new Map<string, Marker>();
  let leafCache = new Map<number, number>();
  let band = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  let lasso = $state<[number, number][] | null>(null);
  let dropping = $state(false);

  function indices(): number[] {
    if (visible) return Array.from(visible);
    return Array.from({ length: points.count }, (_, i) => i);
  }

  function collection(): FeatureCollection<Point> {
    const features: Feature<Point>[] = [];
    for (const i of indices()) {
      features.push({ type: 'Feature', id: points.ids[i], properties: { id: points.ids[i] }, geometry: { type: 'Point', coordinates: [points.lons[i]!, points.lats[i]!] } });
    }
    return { type: 'FeatureCollection', features };
  }

  /** Dashed rings for approximate places (1 km or more). */
  function rings(): FeatureCollection<Polygon> {
    const features: Feature<Polygon>[] = [];
    const seen = new Set<string>();
    for (const i of indices()) {
      const acc = points.accs[i]!;
      if (acc < 1000) continue;
      const k = `${points.lats[i]!.toFixed(3)},${points.lons[i]!.toFixed(3)},${Math.round(acc)}`;
      if (seen.has(k) || features.length > 2000) continue;
      seen.add(k);
      features.push({ type: 'Feature', properties: {}, geometry: { type: 'Polygon', coordinates: [ring(points.lats[i]!, points.lons[i]!, acc)] } });
    }
    return { type: 'FeatureCollection', features };
  }

  function colors() {
    const p = palette();
    if (!map) return;
    applyPalette(map, p);
    map.setPaintProperty('clusters', 'circle-color', p.point);
    map.setPaintProperty('clusters', 'circle-stroke-color', p.land);
    map.setPaintProperty('dots', 'circle-color', p.point);
    map.setPaintProperty('dots', 'circle-stroke-color', p.land);
    map.setPaintProperty('rings', 'line-color', p.muted);
  }

  onMount(() => {
    let cancelled = false;
    void (async () => {
      const [lib, world] = await Promise.all([maplibre(), loadCountries()]);
      if (cancelled || !el) return;
      ml = lib;
      const p = palette();
      const m = new lib.Map({
        container: el,
        style: baseStyle(world, p),
        center: [10, 45],
        zoom: 3,
        minZoom: 0.5,
        maxZoom: 18,
        boxZoom: false,
        dragRotate: false,
        pitchWithRotate: false,
        touchPitch: false,
        attributionControl: { compact: true, customAttribution: 'Natural Earth' },
        fadeDuration: 0,
      });
      m.touchZoomRotate.disableRotation();
      if (import.meta.env.DEV) (window as unknown as { __rollMap: MlMap }).__rollMap = m;
      m.on('load', () => {
        m.addSource('items', { type: 'geojson', data: collection(), cluster: true, clusterMaxZoom: 16, clusterRadius: 48 });
        m.addSource('heat', { type: 'geojson', data: collection() });
        m.addSource('rings', { type: 'geojson', data: rings() });
        m.addLayer({
          id: 'heatmap',
          type: 'heatmap',
          source: 'heat',
          layout: { visibility: heatmap ? 'visible' : 'none' },
          paint: {
            'heatmap-radius': ['interpolate', ['linear'], ['zoom'], 0, 4, 12, 24],
            'heatmap-intensity': ['interpolate', ['linear'], ['zoom'], 0, 0.6, 12, 2],
            'heatmap-color': ['interpolate', ['linear'], ['heatmap-density'], 0, 'rgba(0,0,0,0)', 0.3, '#7a7a7a', 0.7, '#3d3d3d', 1, '#000000'],
          },
        });
        m.addLayer({ id: 'rings', type: 'line', source: 'rings', paint: { 'line-color': p.muted, 'line-width': 1, 'line-dasharray': [3, 3] } });
        m.addLayer({
          id: 'clusters',
          type: 'circle',
          source: 'items',
          filter: ['has', 'point_count'],
          layout: { visibility: heatmap ? 'none' : 'visible' },
          paint: {
            'circle-color': p.point,
            'circle-opacity': 0.85,
            'circle-radius': ['step', ['get', 'point_count'], 10, 10, 13, 100, 16, 1000, 20],
            'circle-stroke-width': 2,
            'circle-stroke-color': p.land,
          },
        });
        m.addLayer({
          id: 'dots',
          type: 'circle',
          source: 'items',
          filter: ['!', ['has', 'point_count']],
          layout: { visibility: heatmap ? 'none' : 'visible' },
          paint: { 'circle-color': p.point, 'circle-radius': 5, 'circle-stroke-width': 1.5, 'circle-stroke-color': p.land },
        });
        for (const layer of ['clusters', 'dots']) {
          m.on('click', layer, (e) => {
            const f = e.features?.[0];
            if (f) void open(f);
          });
          m.on('mouseenter', layer, () => (m.getCanvas().style.cursor = 'pointer'));
          m.on('mouseleave', layer, () => (m.getCanvas().style.cursor = ''));
        }
        // Pins are DOM nodes: rebuild them only once the map rests.
        m.on('movestart', () => clearTimeout(pinTimer));
        m.on('moveend', schedulePins);
        m.on('sourcedata', (e) => {
          if (e.sourceId === 'items' && e.isSourceLoaded) schedulePins();
        });
        map = m;
        if (import.meta.env.DEV) (window as unknown as { __rollMap: MlMap }).__rollMap = m;
        ready = true;
        fit();
      });
    })();
    const obs = new MutationObserver(() => colors());
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    const mq = matchMedia('(prefers-color-scheme: dark)');
    const onScheme = () => colors();
    mq.addEventListener('change', onScheme);
    return () => {
      cancelled = true;
      obs.disconnect();
      mq.removeEventListener('change', onScheme);
    };
  });

  onDestroy(() => {
    for (const mk of markers.values()) mk.remove();
    map?.remove();
  });

  // New data or a new time window.
  $effect(() => {
    void points;
    void visible;
    if (!ready) return;
    untrack(() => {
      const data = collection();
      (map?.getSource('items') as GeoJSONSource | undefined)?.setData(data);
      (map?.getSource('heat') as GeoJSONSource | undefined)?.setData(data);
      (map?.getSource('rings') as GeoJSONSource | undefined)?.setData(rings());
      leafCache = new Map();
    });
  });

  $effect(() => {
    const h = heatmap;
    if (!ready || !map) return;
    map.setLayoutProperty('heatmap', 'visibility', h ? 'visible' : 'none');
    for (const l of ['clusters', 'dots']) map.setLayoutProperty(l, 'visibility', h ? 'none' : 'visible');
    untrack(refreshPins);
  });

  $effect(() => {
    void fitKey;
    if (ready) untrack(fit);
  });

  function fit() {
    if (!map || !points.count) return;
    let [s, w, n, e] = [90, 180, -90, -180];
    for (const i of indices()) {
      s = Math.min(s, points.lats[i]!);
      n = Math.max(n, points.lats[i]!);
      w = Math.min(w, points.lons[i]!);
      e = Math.max(e, points.lons[i]!);
    }
    if (s > n) return;
    map.fitBounds(
      [
        [w, s],
        [e, n],
      ],
      { padding: 60, maxZoom: 12, duration: 0 },
    );
  }

  async function leaves(clusterId: number, limit: number): Promise<number[]> {
    const src = map?.getSource('items') as GeoJSONSource | undefined;
    if (!src) return [];
    const fs = await src.getClusterLeaves(clusterId, limit, 0);
    return fs.map((f) => Number(f.properties?.id));
  }

  async function open(f: { properties: Record<string, unknown> | null }) {
    const p = f.properties ?? {};
    if (p.cluster_id != null) onopen?.(await leaves(Number(p.cluster_id), Infinity));
    else if (p.id != null) onopen?.([Number(p.id)]);
  }

  let pinTimer = 0;
  function schedulePins() {
    clearTimeout(pinTimer);
    pinTimer = window.setTimeout(() => void refreshPins(), 120);
  }

  /** Thumbnail pins on the biggest clusters in view. */
  async function refreshPins() {
    if (!map || !ml || heatmap) {
      for (const mk of markers.values()) mk.remove();
      markers.clear();
      return;
    }
    const features = map.queryRenderedFeatures({ layers: ['clusters', 'dots'] });
    const seen = new Set<string>();
    const list = features
      .map((f) => ({ f, n: Number(f.properties?.point_count ?? 1) }))
      .sort((a, b) => b.n - a.n)
      .slice(0, 36);
    for (const { f, n } of list) {
      const p = f.properties ?? {};
      const key = p.cluster_id != null ? `c${p.cluster_id}` : `p${p.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      if (markers.has(key)) continue;
      let id = Number(p.id);
      if (p.cluster_id != null) {
        const cid = Number(p.cluster_id);
        id = leafCache.get(cid) ?? (await leaves(cid, 1))[0] ?? 0;
        leafCache.set(cid, id);
      }
      if (!map || !ml) return;
      const geom = f.geometry as Point;
      const node = document.createElement('button');
      node.className = 'roll-pin';
      node.setAttribute('aria-label', n > 1 ? `${n} items` : 'One item');
      node.innerHTML = `<img alt="" draggable="false" src="${api.thumbUrl(id, 1)}">${n > 1 ? `<span>${n >= 1000 ? `${Math.round(n / 100) / 10}k` : n}</span>` : ''}`;
      node.addEventListener('click', (ev) => {
        ev.stopPropagation();
        void open(f);
      });
      const mk = new ml.Marker({ element: node, anchor: 'bottom' }).setLngLat(geom.coordinates as [number, number]).addTo(map);
      markers.set(key, mk);
    }
    for (const [k, mk] of markers) {
      if (!seen.has(k)) {
        mk.remove();
        markers.delete(k);
      }
    }
  }

  export function flyTo(lat: number, lon: number, zoom = 11) {
    map?.flyTo({ center: [lon, lat], zoom });
  }

  // Selection: Shift-drag draws a rectangle, Alt-drag a lasso.
  function onpointerdown(e: PointerEvent) {
    if (!map || (!e.shiftKey && !e.altKey)) return;
    e.preventDefault();
    e.stopPropagation();
    map.dragPan.disable();
    const r = el!.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    if (e.shiftKey) band = { x0: x, y0: y, x1: x, y1: y };
    else lasso = [[x, y]];
    el!.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!band && !lasso) return;
    const r = el!.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    if (band) band = { ...band, x1: x, y1: y };
    if (lasso) lasso = [...lasso, [x, y]];
  }

  function inside(poly: [number, number][], x: number, y: number) {
    let hit = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [xi, yi] = poly[i]!;
      const [xj, yj] = poly[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit;
    }
    return hit;
  }

  function onpointerup() {
    if (!map || (!band && !lasso)) return;
    map.dragPan.enable();
    const picked: number[] = [];
    if (band) {
      const [x0, x1] = [Math.min(band.x0, band.x1), Math.max(band.x0, band.x1)];
      const [y0, y1] = [Math.min(band.y0, band.y1), Math.max(band.y0, band.y1)];
      const a = map.unproject([x0, y1]);
      const b = map.unproject([x1, y0]);
      for (const i of indices()) {
        const la = points.lats[i]!;
        const lo = points.lons[i]!;
        if (la >= a.lat && la <= b.lat && lo >= a.lng && lo <= b.lng) picked.push(points.ids[i]!);
      }
    } else if (lasso && lasso.length > 2) {
      const poly = lasso;
      const xs = poly.map((p) => p[0]);
      const ys = poly.map((p) => p[1]);
      const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
      for (const i of indices()) {
        const s = map.project([points.lons[i]!, points.lats[i]!]);
        if (s.x >= minX && s.x <= maxX && s.y >= minY && s.y <= maxY && inside(poly, s.x, s.y)) picked.push(points.ids[i]!);
      }
    }
    band = null;
    lasso = null;
    onselect?.(picked);
  }

  function ondragover(e: DragEvent) {
    if (!e.dataTransfer?.types.includes('application/x-roll-ids')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    dropping = true;
  }

  function ondrop(e: DragEvent) {
    dropping = false;
    const raw = e.dataTransfer?.getData('application/x-roll-ids');
    if (!raw || !map) return;
    e.preventDefault();
    const ids = JSON.parse(raw) as number[];
    const r = el!.getBoundingClientRect();
    const ll = map.unproject([e.clientX - r.left, e.clientY - r.top]);
    // The pin is as precise as the zoom: about 16 screen pixels.
    const acc = Math.min(200_000, Math.max(10, metresPerPixel(ll.lat, map.getZoom()) * 16));
    ondropids?.(ids, ll.lat, ll.lng, acc);
  }
</script>

<div
  class="map"
  class:dropping
  bind:this={el}
  role="application"
  aria-label="Map"
  onpointerdowncapture={onpointerdown}
  {onpointermove}
  {onpointerup}
  {ondragover}
  ondragleave={() => (dropping = false)}
  {ondrop}
  data-testid="map"
>
  {#if band}
    <div
      class="band"
      style:left="{Math.min(band.x0, band.x1)}px"
      style:top="{Math.min(band.y0, band.y1)}px"
      style:width="{Math.abs(band.x1 - band.x0)}px"
      style:height="{Math.abs(band.y1 - band.y0)}px"
    ></div>
  {/if}
  {#if lasso}
    <svg class="lasso" aria-hidden="true"><polygon points={lasso.map((p) => p.join(',')).join(' ')} /></svg>
  {/if}
</div>

<style>
  .map {
    position: absolute;
    inset: 0;
    background: var(--surface);
  }
  .map.dropping {
    outline: 2px dashed var(--fg);
    outline-offset: -6px;
  }
  .band {
    position: absolute;
    z-index: 5;
    border: 1px solid var(--fg);
    background: color-mix(in srgb, var(--fg) 8%, transparent);
    pointer-events: none;
  }
  .lasso {
    position: absolute;
    inset: 0;
    z-index: 5;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .lasso polygon {
    fill: color-mix(in srgb, var(--fg) 8%, transparent);
    stroke: var(--fg);
    stroke-width: 1;
    stroke-dasharray: 4 3;
  }
  /* MapLibre positions markers absolutely; that also anchors the badge. */
  :global(.roll-pin) {
    display: block;
    width: 44px;
    height: 44px;
    padding: 2px;
    border: 0;
    border-radius: 6px;
    background: var(--bg);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);
    cursor: pointer;
  }
  :global(.roll-pin img) {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
    background: var(--surface);
  }
  :global(.roll-pin span) {
    position: absolute;
    top: -8px;
    right: -8px;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    border-radius: 10px;
    background: var(--fg);
    color: var(--bg);
    font: 600 11px/20px var(--mono);
    text-align: center;
  }
  :global(.maplibregl-ctrl-attrib) {
    font-size: 10px;
  }
</style>
