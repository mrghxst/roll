// The offline basemap: Natural Earth country outlines (public domain, via
// the world-atlas package) on a globe, in a monochrome style that follows
// the theme. No text layers, so no glyphs have to be fetched. MapLibre
// flattens the globe to Web Mercator as you zoom in.

import type { FeatureCollection } from 'geojson';
import type { LayerSpecification, Map as MlMap, SkySpecification, StyleSpecification } from 'maplibre-gl';
import type { GeometryCollection, Topology } from 'topojson-specification';

let countries: Promise<FeatureCollection> | null = null;

/** Country polygons, loaded once and on demand (about 700 KB). */
export function loadCountries(): Promise<FeatureCollection> {
  countries ??= Promise.all([import('topojson-client'), import('world-atlas/countries-50m.json')]).then(([topo, data]) => {
    const t = ((data as { default?: unknown }).default ?? data) as Topology<{ countries: GeometryCollection }>;
    const fc = topo.feature(t, t.objects.countries) as FeatureCollection;
    for (const f of fc.features) {
      const g = f.geometry;
      if (g.type === 'Polygon') g.coordinates = g.coordinates.map(unwrap);
      else if (g.type === 'MultiPolygon') g.coordinates = g.coordinates.map((poly) => poly.map(unwrap));
    }
    return fc;
  });
  return countries;
}

/**
 * Rings that cross the 180° meridian jump from +180 to −180 in one edge.
 * Flat, that edge runs off the side of the map; on the globe it is a
 * circle around the pole that fills the ocean. Continue such rings past
 * ±180 instead (MapLibre wraps them). A ring around a pole (Antarctica)
 * cannot be unwrapped and stays as it is.
 */
export function unwrap(ring: number[][]): number[][] {
  let shift = 0;
  const out = ring.map((p, i) => {
    if (i > 0) {
      const d = p[0]! - ring[i - 1]![0]!;
      if (d > 180) shift -= 360;
      else if (d < -180) shift += 360;
    }
    return shift ? [p[0]! + shift, ...p.slice(1)] : p;
  });
  return shift === 0 ? out : ring;
}

export interface Palette {
  /** Outside the globe. */
  space: string;
  water: string;
  land: string;
  border: string;
  /** The thin glow around the globe. */
  glow: string;
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
  const dark = rgb(bg).reduce((a, b) => a + b, 0) < 384;
  // Land is the lighter surface in both themes, water a step towards the
  // background, so the globe reads as a globe and not as a hole.
  const land = dark ? mix(bg, fg, 0.13) : bg;
  const water = dark ? mix(bg, fg, 0.05) : mix(bg, fg, 0.075);
  return {
    space: dark ? bg : mix(bg, fg, 0.02),
    water,
    land,
    border: dark ? mix(bg, fg, 0.26) : mix(bg, fg, 0.22),
    glow: dark ? mix(bg, fg, 0.35) : mix(bg, fg, 0.12),
    point: fg,
    pointText: bg,
    muted: v('--muted', '#666666'),
    road: mix(land, fg, 0.28),
    minor: mix(land, fg, 0.16),
    building: mix(land, fg, 0.07),
    green: mix(land, water, 0.5),
    label: mix(land, fg, 0.6),
  };
}

/** A soft rim of light around the globe that fades out as you zoom in. */
function sky(p: Palette): SkySpecification {
  return {
    'sky-color': p.space,
    'horizon-color': p.glow,
    'fog-color': p.space,
    'atmosphere-blend': ['interpolate', ['linear'], ['zoom'], 0, 0.55, 4, 0.35, 7, 0],
  };
}

export function baseStyle(world: FeatureCollection, p: Palette): StyleSpecification {
  return {
    version: 8,
    projection: { type: 'globe' },
    sky: sky(p),
    sources: { countries: { type: 'geojson', data: world } },
    layers: [
      { id: 'water', type: 'background', paint: { 'background-color': p.water } },
      { id: 'land', type: 'fill', source: 'countries', paint: { 'fill-color': p.land } },
      { id: 'borders', type: 'line', source: 'countries', paint: { 'line-color': p.border, 'line-width': ['interpolate', ['linear'], ['zoom'], 0, 0.3, 6, 0.9] } },
    ],
  };
}

/** Recolour an existing map after a theme change. */
export function applyPalette(map: MlMap, p: Palette) {
  map.setPaintProperty('water', 'background-color', p.water);
  map.setPaintProperty('land', 'fill-color', p.land);
  map.setPaintProperty('borders', 'line-color', p.border);
  map.setSky(sky(p));
  for (const [source, layers] of [
    [DETAIL, detailLayers],
    [OFFLINE, offlineLayers],
  ] as const) {
    if (!map.getSource(source)) continue;
    for (const l of layers(p)) {
      for (const [k, v] of Object.entries(l.paint ?? {})) {
        if (k.endsWith('color')) map.setPaintProperty(l.id, k as 'fill-color', v);
      }
    }
  }
}

// Opt-in detail tiles (Settings › Network › Online map tiles, or Map ›
// Map detail › Online map): OpenFreeMap's
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
  // Main roads stay a full pixel wide when zoomed out, so they read on the world map.
  const main: unknown = ['interpolate', ['exponential', 1.6], ['zoom'], 4, 0.6, 8, 1.1, 16, 8];
  return [
    // The world outlines are coarse at a coast: from zoom 8 the tiles' own
    // water (every sea and lake) is drawn on land instead.
    { id: 'd-land', type: 'background', minzoom: 8, paint: { 'background-color': p.land } },
    // Parks and woods only close up: zoomed out they blot the land like lakes.
    { id: 'd-green', type: 'fill', ...s, 'source-layer': 'park', minzoom: 10, paint: { 'fill-color': p.green } },
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
      paint: { 'line-color': p.road, 'line-width': main as number },
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
      paint: { 'line-color': p.minor, 'line-width': 1, 'line-dasharray': [4, 2] },
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

// Offline areas and the world map (Map › Map detail): Protomaps tiles that
// Roll serves itself, in the Protomaps schema, drawn like the online tiles.
// Only while the online tiles are off. Roll answers up to zoom 15 (areas
// that stop at 13, and the world map at 6, are enlarged by Roll); the map
// enlarges past that.

const OFFLINE = 'offline';
const GREEN = ['park', 'forest', 'wood', 'nature_reserve', 'national_park', 'grass', 'grassland', 'meadow', 'garden'];

function offlineLayers(p: Palette): LayerSpecification[] {
  const s = { source: OFFLINE };
  const width = (a: number, b: number): unknown => ['interpolate', ['exponential', 1.6], ['zoom'], 6, a, 16, b];
  // Main roads stay a full pixel wide when zoomed out, so they read on the world map.
  const main: unknown = ['interpolate', ['exponential', 1.6], ['zoom'], 4, 0.6, 8, 1.1, 16, 8];
  return [
    // The area's own land, finer than the world outlines at a coast.
    { id: 'o-earth', type: 'fill', ...s, 'source-layer': 'earth', paint: { 'fill-color': p.land } },
    {
      id: 'o-green',
      type: 'fill',
      ...s,
      'source-layer': 'landuse',
      minzoom: 10,
      filter: ['in', ['get', 'kind'], ['literal', GREEN]],
      paint: { 'fill-color': p.green },
    },
    { id: 'o-water', type: 'fill', ...s, 'source-layer': 'water', filter: ['==', ['geometry-type'], 'Polygon'], paint: { 'fill-color': p.water } },
    {
      id: 'o-river',
      type: 'line',
      ...s,
      'source-layer': 'water',
      minzoom: 8,
      filter: ['==', ['geometry-type'], 'LineString'],
      paint: { 'line-color': p.water, 'line-width': width(0.5, 3) as number },
    },
    { id: 'o-building', type: 'fill', ...s, 'source-layer': 'buildings', minzoom: 14, paint: { 'fill-color': p.building } },
    {
      id: 'o-minor',
      type: 'line',
      ...s,
      'source-layer': 'roads',
      minzoom: 12,
      filter: ['in', ['get', 'kind'], ['literal', ['minor_road', 'path', 'other']]],
      paint: { 'line-color': p.minor, 'line-width': width(0.3, 4) as number },
    },
    {
      id: 'o-road',
      type: 'line',
      ...s,
      'source-layer': 'roads',
      minzoom: 5,
      filter: ['in', ['get', 'kind'], ['literal', ['highway', 'major_road', 'medium_road']]],
      paint: { 'line-color': p.road, 'line-width': main as number },
    },
    {
      id: 'o-rail',
      type: 'line',
      ...s,
      'source-layer': 'roads',
      minzoom: 10,
      filter: ['==', ['get', 'kind'], 'rail'],
      paint: { 'line-color': p.road, 'line-width': 1, 'line-dasharray': [3, 3] },
    },
    // The world map's land covers the outlines' borders: its own instead.
    {
      id: 'o-border',
      type: 'line',
      ...s,
      'source-layer': 'boundaries',
      filter: ['==', ['get', 'kind'], 'country'],
      paint: { 'line-color': p.border, 'line-width': ['interpolate', ['linear'], ['zoom'], 0, 0.3, 6, 0.9] },
    },
    {
      id: 'o-boundary',
      type: 'line',
      ...s,
      'source-layer': 'boundaries',
      minzoom: 5,
      filter: ['==', ['get', 'kind'], 'region'],
      paint: { 'line-color': p.minor, 'line-width': 1, 'line-dasharray': [4, 2] },
    },
    {
      id: 'o-place',
      type: 'symbol',
      ...s,
      'source-layer': 'places',
      filter: ['all', ['==', ['get', 'kind'], 'locality'], ['>=', ['zoom'], ['coalesce', ['get', 'min_zoom'], 0]]],
      layout: {
        'text-field': ['get', 'name'],
        'text-font': FONT,
        'text-size': ['match', ['get', 'kind_detail'], 'city', 14, 'town', 12, 11],
      },
      paint: { 'text-color': p.label, 'text-halo-color': p.land, 'text-halo-width': 1.2 },
    },
  ];
}

/** Show the offline areas (`bounds`: west, south, east, north of them all),
 * or take them away with `tiles` null. */
export function setOffline(map: MlMap, tiles: string | null, fonts: string | null, bounds: number[] | null, p: Palette, below?: string) {
  if (map.getSource(OFFLINE)) {
    for (const l of offlineLayers(p)) if (map.getLayer(l.id)) map.removeLayer(l.id);
    map.removeSource(OFFLINE);
  }
  if (!tiles || !bounds) return;
  if (fonts) map.setGlyphs(fonts);
  map.addSource(OFFLINE, {
    type: 'vector',
    tiles: [tiles],
    minzoom: 0,
    maxzoom: 15,
    bounds: bounds as [number, number, number, number],
    attribution: '© OpenStreetMap contributors, Protomaps',
  });
  const before = below && map.getLayer(below) ? below : undefined;
  for (const l of offlineLayers(p)) map.addLayer(l, before);
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
