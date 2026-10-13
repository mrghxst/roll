<script lang="ts">
  import 'maplibre-gl/dist/maplibre-gl.css';
  import type { Feature, FeatureCollection, Point, Polygon } from 'geojson';
  import type { GeoJSONSource, Map as MlMap, Marker } from 'maplibre-gl';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { api } from '../ipc/api';
  import type { MapPoints } from '../ipc/mappoints';
  import { app } from '../state/app.svelte';
  import { applyPalette, baseStyle, loadCountries, metresPerPixel, palette, ring, setDetail } from './basemap';
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
    if (el) el.style.background = p.space;
    if (!map) return;
    applyPalette(map, p);
    map.setPaintProperty('clusters', 'circle-color', p.point);
    map.setPaintProperty('clusters', 'circle-stroke-color', p.land);
    map.setPaintProperty('dots', 'circle-color', p.point);
    map.setPaintProperty('dots', 'circle-stroke-color', p.land);
    map.setPaintProperty('rings', 'line-color', p.point);
    map.setPaintProperty('rings-fill', 'fill-color', p.point);
  }

  onMount(() => {
    let cancelled = false;
    void (async () => {
      const [lib, world] = await Promise.all([maplibre(), loadCountries()]);
      if (cancelled || !el) return;
      ml = lib;
      const p = palette();
      el.style.background = p.space;
      const m = new lib.Map({
        container: el,
        style: baseStyle(world, p),
        center: [10, 30],
        zoom: 1.6,
        minZoom: 0,
        maxZoom: 18,
        boxZoom: false,
        dragRotate: false,
        pitchWithRotate: false,
        touchPitch: false,
        attributionControl: false,
        fadeDuration: 0,
      });
      m.touchZoomRotate.disableRotation();
      // Bottom left: the time slider takes the right side.
      m.addControl(new lib.AttributionControl({ compact: true, customAttribution: 'Natural Earth' }), 'bottom-left');
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
        // Approximate places: a faint disc with a dashed edge, once it is big enough to read.
        m.addLayer({ id: 'rings-fill', type: 'fill', source: 'rings', minzoom: 6, paint: { 'fill-color': p.point, 'fill-opacity': 0.04 } });
        m.addLayer({
          id: 'rings',
          type: 'line',
          source: 'rings',
          minzoom: 6,
          paint: { 'line-color': p.point, 'line-opacity': 0.35, 'line-width': 1, 'line-dasharray': [2, 2] },
        });
        m.addLayer({
          id: 'clusters',
          type: 'circle',
          source: 'items',
          filter: ['has', 'point_count'],
          layout: { visibility: heatmap ? 'none' : 'visible' },
          paint: {
            'circle-color': p.point,
            'circle-opacity': 0.9,
            // Smaller than a pin, which sits right on top of the biggest ones.
            'circle-radius': ['step', ['get', 'point_count'], 7, 10, 9, 100, 11, 1000, 13],
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
          paint: { 'circle-color': p.point, 'circle-radius': 4.5, 'circle-stroke-width': 1.5, 'circle-stroke-color': p.land },
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

  // Online detail tiles follow Settings › Network.
  $effect(() => {
    const on = !!app.settings?.network.online_tiles;
    if (!ready || !map) return;
    untrack(() => setDetail(map!, on, palette(), 'heatmap'));
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
    if (ready) untrack(() => fit());
  });

  function fit(duration = 0) {
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
      { padding: 80, maxZoom: 12, duration },
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
    // queryRenderedFeatures with a box finds nothing on the globe, so take
    // the source's clusters and keep those on screen and facing us.
    const canvas = map.getCanvas();
    const [w, h] = [canvas.clientWidth, canvas.clientHeight];
    const c = map.getCenter();
    const keyOf = (f: { properties: Record<string, unknown> | null }) => {
      const p = f.properties ?? {};
      return p.cluster_id != null ? `c${p.cluster_id}` : `p${p.id}`;
    };
    const unique = new Map<string, ReturnType<MlMap['querySourceFeatures']>[number]>();
    for (const f of map.querySourceFeatures('items')) {
      const [lon, lat] = (f.geometry as Point).coordinates as [number, number];
      if (!facing(c.lat, c.lng, lat, lon)) continue;
      const s = map.project([lon, lat]);
      if (s.x < 0 || s.y < 0 || s.x > w || s.y > h) continue;
      unique.set(keyOf(f), f);
    }
    const seen = new Set<string>();
    const list = [...unique.values()]
      .map((f) => ({ f, n: Number(f.properties?.point_count ?? 1) }))
      .sort((a, b) => b.n - a.n)
      .slice(0, 36);
    for (const { f, n } of list) {
      const p = f.properties ?? {};
      const key = keyOf(f);
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
      // Centred on the cluster, which it covers.
      const mk = new ml.Marker({ element: node, anchor: 'center' }).setLngLat(geom.coordinates as [number, number]).addTo(map);
      markers.set(key, mk);
    }
    for (const [k, mk] of markers) {
      if (!seen.has(k)) {
        mk.remove();
        markers.delete(k);
      }
    }
  }

  /** Show every point in the time window. */
  export function fitAll() {
    fit(600);
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

  /** False for points on the far side of the globe, which project onto the visible side. */
  function facing(cLat: number, cLon: number, lat: number, lon: number) {
    const r = Math.PI / 180;
    const cos = Math.sin(cLat * r) * Math.sin(lat * r) + Math.cos(cLat * r) * Math.cos(lat * r) * Math.cos((lon - cLon) * r);
    return cos > 0.05;
  }

  function onpointerup() {
    if (!map || (!band && !lasso)) return;
    map.dragPan.enable();
    const picked: number[] = [];
    // Both in screen space, so they are right on the globe too.
    const poly: [number, number][] | null = band
      ? [
          [band.x0, band.y0],
          [band.x1, band.y0],
          [band.x1, band.y1],
          [band.x0, band.y1],
        ]
      : lasso && lasso.length > 2
        ? lasso
        : null;
    if (poly) {
      const xs = poly.map((p) => p[0]);
      const ys = poly.map((p) => p[1]);
      const [minX, maxX, minY, maxY] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
      const c = map.getCenter();
      for (const i of indices()) {
        const [la, lo] = [points.lats[i]!, points.lons[i]!];
        if (!facing(c.lat, c.lng, la, lo)) continue;
        const s = map.project([lo, la]);
        if (s.x >= minX && s.x <= maxX && s.y >= minY && s.y <= maxY && (band || inside(poly, s.x, s.y))) picked.push(points.ids[i]!);
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
  /* MapLibre positions markers absolutely (and sets their transform); that
     also anchors the badge. */
  :global(.roll-pin) {
    display: block;
    width: 46px;
    height: 46px;
    padding: 0;
    border: 2px solid var(--bg);
    border-radius: 11px;
    background: var(--surface);
    box-shadow:
      0 1px 2px rgba(0, 0, 0, 0.18),
      0 6px 16px rgba(0, 0, 0, 0.16);
    cursor: pointer;
  }
  :global(.roll-pin img) {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 9px;
    transition: transform 150ms var(--ease);
  }
  :global(.roll-pin:hover img) {
    transform: scale(1.06);
  }
  :global(.roll-pin:focus-visible) {
    outline: 2px solid var(--fg);
    outline-offset: 2px;
  }
  :global(.roll-pin span) {
    position: absolute;
    right: -9px;
    bottom: -9px;
    min-width: 22px;
    height: 20px;
    padding: 0 5px;
    border: 2px solid var(--bg);
    border-radius: 10px;
    background: var(--fg);
    color: var(--bg);
    font: 600 10px/16px var(--mono);
    text-align: center;
  }
  /* The attribution and the canvas in the app's colours. */
  :global(.maplibregl-ctrl-attrib.maplibregl-compact) {
    margin: 0 0 10px 10px !important;
    font-size: 10px;
    border: 1px solid var(--border);
    background: var(--bg) !important;
    color: var(--muted);
  }
  :global(.maplibregl-ctrl-attrib a) {
    color: var(--muted) !important;
  }
  :global(:root[data-theme='dark'] .maplibregl-ctrl-attrib-button) {
    filter: invert(1);
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light']) .maplibregl-ctrl-attrib-button) {
      filter: invert(1);
    }
  }
</style>
