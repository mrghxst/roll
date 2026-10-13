// The offline basemap: Natural Earth country outlines (public domain, via
// the world-atlas package) in a monochrome style that follows the theme.
// No text layers, so no glyphs have to be fetched.

import type { FeatureCollection } from 'geojson';
import type { LayerSpecification, Map as MlMap, StyleSpecification } from 'maplibre-gl';
import type { GeometryCollection, Topology } from 'topojson-specification';

let countries: Promise<FeatureCollection> | null = null;

/** Country polygons, loaded once and on demand (about 700 KB). */
export function loadCountries(): Promise<FeatureCollection> {
  countries ??= Promise.all([import('topojson-client'), import('world-atlas/countries-50m.json')]).then(([topo, data]) => {
    const t = ((data as { default?: unknown }).default ?? data) as Topology<{ countries: GeometryCollection }>;
    return topo.feature(t, t.objects.countries) as FeatureCollection;
  });
  return countries;
}

export interface Palette {
  water: string;
  land: string;
  border: string;
  point: string;
  pointText: string;
  muted: string;
  /** Detail tiles: roads, minor roads, buildings, parks, place names. */
  road: string;
  minor: string;
  building: string;
  green: string;
  label: string;
}

function rgb(c: string): [number, number, number] {
  const h = c.trim().replace('#', '');
  if (/^[0-9a-f]{6}$/i.test(h)) return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
  if (/^[0-9a-f]{3}$/i.test(h)) return [0, 1, 2].map((i) => parseInt(h[i]! + h[i]!, 16)) as [number, number, number];
  const m = c.match(/\d+(\.\d+)?/g);
  return m && m.length >= 3 ? [Number(m[0]), Number(m[1]), Number(m[2])] : [128, 128, 128];
}

/** `a` with `t` of `b` mixed in, as a hex colour MapLibre can parse. */
function mix(a: string, b: string, t: number): string {
  const [x, y] = [rgb(a), rgb(b)];
  return `#${x.map((v, i) => Math.round(v + (y[i]! - v) * t).toString(16).padStart(2, '0')).join('')}`;
}

export function palette(): Palette {
  const css = getComputedStyle(document.documentElement);
  const v = (n: string, d: string) => css.getPropertyValue(n).trim() || d;
  const bg = v('--bg', '#ffffff');
  const fg = v('--fg', '#000000');
  return {
    water: mix(bg, fg, 0.1),
    land: bg,
    border: mix(bg, fg, 0.3),
    point: fg,
    pointText: bg,
    muted: v('--muted', '#666666'),
    road: mix(bg, fg, 0.28),
    minor: mix(bg, fg, 0.16),
    building: mix(bg, fg, 0.07),
    green: mix(bg, fg, 0.04),
    label: mix(bg, fg, 0.6),
  };
}

export function baseStyle(world: FeatureCollection, p: Palette): StyleSpecification {
  return {
    version: 8,
    sources: { countries: { type: 'geojson', data: world } },
    layers: [
      { id: 'water', type: 'background', paint: { 'background-color': p.water } },
      { id: 'land', type: 'fill', source: 'countries', paint: { 'fill-color': p.land } },
      { id: 'borders', type: 'line', source: 'countries', paint: { 'line-color': p.border, 'line-width': ['interpolate', ['linear'], ['zoom'], 0, 0.4, 6, 1] } },
    ],
  };
}

/** Recolour an existing map after a theme change. */
export function applyPalette(map: MlMap, p: Palette) {
  map.setPaintProperty('water', 'background-color', p.water);
  map.setPaintProperty('land', 'fill-color', p.land);
  map.setPaintProperty('borders', 'line-color', p.border);
  if (!map.getSource(DETAIL)) return;
  for (const l of detailLayers(p)) {
    for (const [k, v] of Object.entries(l.paint ?? {})) {
      if (k.endsWith('color')) map.setPaintProperty(l.id, k as 'fill-color', v);
    }
  }
}

// Opt-in detail tiles (Settings › Network › Online map tiles): OpenFreeMap's
// planet tiles in the OpenMapTiles schema, drawn in the same monochrome
// style over the offline outlines. Nothing is fetched while it is off.

const DETAIL = 'detail';
const OFM = 'https://tiles.openfreemap.org';
const FONT = ['Noto Sans Regular'];
const ROADS = ['motorway', 'trunk', 'primary', 'secondary', 'tertiary'];
const MINOR = ['minor', 'service', 'track', 'path'];

function detailLayers(p: Palette): LayerSpecification[] {
  const s = { source: DETAIL };
  const width = (a: number, b: number): unknown => ['interpolate', ['exponential', 1.6], ['zoom'], 6, a, 16, b];
  return [
    { id: 'd-green', type: 'fill', ...s, 'source-layer': 'park', paint: { 'fill-color': p.green } },
    { id: 'd-water', type: 'fill', ...s, 'source-layer': 'water', paint: { 'fill-color': p.water } },
    { id: 'd-river', type: 'line', ...s, 'source-layer': 'waterway', minzoom: 8, paint: { 'line-color': p.water, 'line-width': width(0.5, 3) as number } },
    { id: 'd-building', type: 'fill', ...s, 'source-layer': 'building', minzoom: 14, paint: { 'fill-color': p.building } },
    {
      id: 'd-minor',
      type: 'line',
      ...s,
      'source-layer': 'transportation',
      minzoom: 12,
      filter: ['in', ['get', 'class'], ['literal', MINOR]],
      paint: { 'line-color': p.minor, 'line-width': width(0.3, 4) as number },
    },
    {
      id: 'd-road',
      type: 'line',
      ...s,
      'source-layer': 'transportation',
      minzoom: 5,
      filter: ['in', ['get', 'class'], ['literal', ROADS]],
      paint: { 'line-color': p.road, 'line-width': width(0.4, 8) as number },
    },
    {
      id: 'd-rail',
      type: 'line',
      ...s,
      'source-layer': 'transportation',
      minzoom: 10,
      filter: ['==', ['get', 'class'], 'rail'],
      paint: { 'line-color': p.road, 'line-width': 1, 'line-dasharray': [3, 3] },
    },
    {
      id: 'd-boundary',
      type: 'line',
      ...s,
      'source-layer': 'boundary',
      filter: ['all', ['<=', ['get', 'admin_level'], 4], ['!=', ['get', 'maritime'], 1]],
      paint: { 'line-color': p.border, 'line-width': 1, 'line-dasharray': [4, 2] },
    },
    {
      id: 'd-place',
      type: 'symbol',
      ...s,
      'source-layer': 'place',
      filter: ['in', ['get', 'class'], ['literal', ['city', 'town', 'village', 'suburb']]],
      layout: {
        'text-field': ['coalesce', ['get', 'name:latin'], ['get', 'name']],
        'text-font': FONT,
        'text-size': ['match', ['get', 'class'], 'city', 14, 'town', 12, 11],
      },
      paint: { 'text-color': p.label, 'text-halo-color': p.land, 'text-halo-width': 1.2 },
    },
  ];
}

/** Turn the online detail tiles on or off, below the item layers. */
export function setDetail(map: MlMap, on: boolean, p: Palette, below?: string) {
  const has = !!map.getSource(DETAIL);
  if (on === has) return;
  const layers = detailLayers(p);
  if (!on) {
    for (const l of layers) if (map.getLayer(l.id)) map.removeLayer(l.id);
    map.removeSource(DETAIL);
    return;
  }
  map.setGlyphs(`${OFM}/fonts/{fontstack}/{range}.pbf`);
  map.addSource(DETAIL, { type: 'vector', url: `${OFM}/planet`, attribution: 'OpenFreeMap © OpenMapTiles © OpenStreetMap contributors' });
  const before = below && map.getLayer(below) ? below : undefined;
  for (const l of layers) map.addLayer(l, before);
}

/** A circle around a point as a polygon ring, for approximate locations. */
export function ring(lat: number, lon: number, radiusM: number, steps = 48): [number, number][] {
  const out: [number, number][] = [];
  const dLat = radiusM / 111_320;
  const dLon = radiusM / (111_320 * Math.max(0.01, Math.cos((lat * Math.PI) / 180)));
  for (let i = 0; i <= steps; i++) {
    const a = (i / steps) * 2 * Math.PI;
    out.push([lon + dLon * Math.cos(a), lat + dLat * Math.sin(a)]);
  }
  return out;
}

/** Metres per screen pixel at a latitude and zoom (Web Mercator, 512 px tiles). */
export function metresPerPixel(lat: number, zoom: number): number {
  return (40_075_016.686 * Math.cos((lat * Math.PI) / 180)) / (512 * 2 ** zoom);
}
