// The offline basemap: Natural Earth country outlines (public domain, via
// the world-atlas package) in a monochrome style that follows the theme.
// No text layers, so no glyphs have to be fetched.

import type { FeatureCollection } from 'geojson';
import type { Map as MlMap, StyleSpecification } from 'maplibre-gl';
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
