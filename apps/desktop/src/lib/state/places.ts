// Place edits shared by the map, Fix places and "Set location…". Each one
// is an undoable batch; afterwards Roll offers time zones for items whose
// files state none.

import { api } from '../ipc/api';
import type { LocationResult } from '../ipc/bindings/LocationResult';
import { app } from './app.svelte';
import { undoLast } from './edits';
import { toast } from './toast.svelte';

const plural = (n: number) => (n === 1 ? '1 item' : `${n.toLocaleString('en-US')} items`);

function done(r: LocationResult, text: string) {
  toast.show(text, { action: { label: 'Undo', run: () => void undoLast() } });
  if (r.zones.length) app.zones = r.zones;
}

export async function setPlace(ids: number[], place: { lat: number; lon: number; accuracy_m: number; name: string | null }) {
  if (!ids.length) return;
  try {
    const name = place.name ?? (await api.placeName(place.lat, place.lon));
    const r = await api.setLocation(ids, { ...place, name });
    done(r, `Set the location of ${plural(r.count)}${name ? ` to ${name}` : ''}`);
  } catch (e) {
    toast.error(e);
  }
}

export async function clearPlace(ids: number[]) {
  try {
    const r = await api.setLocation(ids, null);
    done(r, `Cleared the location of ${plural(r.count)}`);
  } catch (e) {
    toast.error(e);
  }
}

export async function acceptGroup(key: string) {
  try {
    const r = await api.acceptPlaces(key);
    done(r, `Set the location of ${plural(r.count)}`);
  } catch (e) {
    toast.error(e);
  }
}

/** UTC+2, UTC−4, UTC+5:30. */
export function offsetLabel(m: number) {
  const sign = m < 0 ? '−' : '+';
  const a = Math.abs(m);
  return `UTC${sign}${Math.floor(a / 60)}${a % 60 ? `:${String(a % 60).padStart(2, '0')}` : ''}`;
}

/** "±40 m", "±5 km". */
export function radius(m: number) {
  return m < 1000 ? `±${Math.round(m / 10) * 10} m` : `±${Math.round(m / 1000)} km`;
}
