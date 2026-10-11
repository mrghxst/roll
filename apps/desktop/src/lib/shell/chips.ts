// Active filters shown as removable chips in the search field.

import type { Query } from '../ipc/bindings/Query';
import { shortDate } from '../util/format';

export interface Chip {
  key: string;
  label: string;
}

const LOCATION: Record<string, string> = { any: 'Has location', none: 'No location', approximate: 'Approximate location', exact: 'Exact location' };
const MEDIA: Record<string, string> = { photo: 'Photos', video: 'Videos', live: 'Live Photos', burst: 'Bursts', raw: 'RAW' };
const PROBLEMS: Record<number, string> = { 1: 'No date', 2: 'Suspicious date', 3: 'Date problems', 4: 'Date changed', 8: 'No location' };

const day = (ms: number) => shortDate(new Date(ms).toISOString());

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** "2019", "Aug 2019", "Jun – Aug 2019" for local bounds on month starts. */
export function localRange(from: string | null | undefined, to: string | null | undefined): string {
  if (!from || !to) return from ? `From ${from.slice(0, 10)}` : `Until ${to?.slice(0, 10)}`;
  const [fy, fm] = [Number(from.slice(0, 4)), Number(from.slice(5, 7))];
  const end = new Date(Date.UTC(Number(to.slice(0, 4)), Number(to.slice(5, 7)) - 1, Number(to.slice(8, 10))) - 86_400_000);
  const [ty, tm] = [end.getUTCFullYear(), end.getUTCMonth() + 1];
  if (fy === ty && fm === 1 && tm === 12) return String(fy);
  if (fy === ty && fm === tm) return `${MONTHS[fm - 1]} ${fy}`;
  if (fy === ty) return `${MONTHS[fm - 1]} – ${MONTHS[tm - 1]} ${fy}`;
  return `${MONTHS[fm - 1]} ${fy} – ${MONTHS[tm - 1]} ${ty}`;
}

/** `labels` names chips the search parser made ("Near Italy"). */
export function chips(f: Partial<Query>, labels: Record<string, string> = {}): Chip[] {
  const out: Chip[] = [];
  if (f.from_utc != null || f.to_utc != null) {
    const from = f.from_utc != null ? day(f.from_utc) : '…';
    const to = f.to_utc != null ? day(f.to_utc - 1) : '…';
    out.push({ key: 'date', label: from === to ? from : `${from} – ${to}` });
  }
  if (f.from_local || f.to_local) out.push({ key: 'local', label: labels.local ?? localRange(f.from_local, f.to_local) });
  for (const m of f.media ?? []) out.push({ key: `media:${m}`, label: MEDIA[m] ?? m });
  for (const c of f.categories ?? []) out.push({ key: `cat:${c}`, label: `#${c.replace('_', ' ')}` });
  for (const c of f.exclude_categories ?? []) out.push({ key: `xcat:${c}`, label: `Without ${c.replace('_', ' ')}` });
  for (const t of f.tags ?? []) out.push({ key: `tag:${t}`, label: t });
  for (const c of f.cameras ?? []) out.push({ key: `cam:${c}`, label: c });
  for (const l of f.lenses ?? []) out.push({ key: `lens:${l}`, label: l });
  for (const fm of f.formats ?? []) out.push({ key: `fmt:${fm}`, label: fm.toUpperCase() });
  if (f.location) out.push({ key: 'location', label: LOCATION[f.location] ?? f.location });
  if (f.bbox) out.push({ key: 'bbox', label: labels.bbox ?? 'Map area' });
  if (f.min_rating) out.push({ key: 'rating', label: `${f.min_rating}+ stars` });
  if (f.favorite) out.push({ key: 'favorite', label: 'Favorites' });
  if (f.problems) out.push({ key: 'problems', label: PROBLEMS[f.problems] ?? 'Problems' });
  if (f.orientation) out.push({ key: 'orientation', label: f.orientation[0]!.toUpperCase() + f.orientation.slice(1) });
  if (f.immich) out.push({ key: 'immich', label: f.immich === 'sent' ? 'In Immich' : f.immich === 'local' ? 'Local only' : 'Waiting for sync' });
  return out;
}

export function removeChip(f: Partial<Query>, key: string): Partial<Query> {
  const next = { ...f };
  const [kind, value] = key.includes(':') ? [key.slice(0, key.indexOf(':')), key.slice(key.indexOf(':') + 1)] : [key, ''];
  const drop = (arr: string[] | undefined) => (arr ?? []).filter((v) => v !== value);
  switch (kind) {
    case 'date':
      next.from_utc = null;
      next.to_utc = null;
      break;
    case 'local':
      next.from_local = null;
      next.to_local = null;
      break;
    case 'media':
      next.media = drop(f.media);
      break;
    case 'cat':
      next.categories = drop(f.categories);
      break;
    case 'xcat':
      next.exclude_categories = drop(f.exclude_categories);
      break;
    case 'tag':
      next.tags = drop(f.tags);
      break;
    case 'cam':
      next.cameras = drop(f.cameras);
      break;
    case 'lens':
      next.lenses = drop(f.lenses);
      break;
    case 'fmt':
      next.formats = drop(f.formats);
      break;
    case 'location':
      next.location = null;
      break;
    case 'bbox':
      next.bbox = null;
      break;
    case 'rating':
      next.min_rating = null;
      break;
    case 'favorite':
      next.favorite = null;
      break;
    case 'problems':
      next.problems = null;
      break;
    case 'orientation':
      next.orientation = null;
      break;
    case 'immich':
      next.immich = null;
      break;
  }
  return next;
}
