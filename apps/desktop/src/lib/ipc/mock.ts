// An in-memory backend for the browser: development without Tauri and the
// Playwright tests. URL parameters: `?mock=N` items (default 2400),
// `?setup=1` to start with a library already added.

import type { Api, Events } from './api';
import { emptyQuery } from './query';
import type { AssetDetail } from './bindings/AssetDetail';
import type { BatchInfo } from './bindings/BatchInfo';
import type { DateFix } from './bindings/DateFix';
import type { DateSet } from './bindings/DateSet';
import type { PlaceGroup } from './bindings/PlaceGroup';
import type { PlaceHit } from './bindings/PlaceHit';
import type { TrackInfo } from './bindings/TrackInfo';
import { emptyMapPoints, type MapPoints } from './mappoints';
import type { Category } from './bindings/Category';
import type { FolderNode } from './bindings/FolderNode';
import type { JobInfo } from './bindings/JobInfo';
import type { PackStatus } from './bindings/PackStatus';
import type { PlanSummary } from './bindings/PlanSummary';
import type { Query } from './bindings/Query';
import type { RootInfo } from './bindings/RootInfo';
import type { Rule } from './bindings/Rule';
import type { Settings } from './bindings/Settings';
import { Flag, type Timeline } from './timeline';

const params = new URLSearchParams(typeof location !== 'undefined' ? location.search : '');
const N = Number(params.get('mock') ?? 2400);
/** `?geo=1`: every item has a place, spread over Europe (map stress tests). */
const GEO = params.get('geo') === '1';

function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 4294967296;
  };
}

interface MockAsset {
  id: number;
  local: string;
  utc: number;
  aspect: number;
  color: number;
  flags: number;
  name: string;
  dir: string;
  category: string;
  camera: string | null;
  lat: number | null;
  lon: number | null;
  duration: number | null;
  rating: number;
  tags: string[];
  trashed: boolean;
  /** Changed in the library but not yet saved to the file. */
  dirty: boolean;
  /** 1 proposed, 2 auto-accepted, 3 decided by the owner. */
  review: number;
  conf: number;
}

let CATS: [string, string][] = [
  ['photos', 'Photos'],
  ['screenshots', 'Screenshots'],
  ['memes', 'Memes'],
  ['social_media', 'Social media'],
  ['wallpapers', 'Wallpapers'],
  ['documents', 'Documents'],
  ['junk', 'Junk'],
  ['unsorted', 'Unsorted'],
];

function makeAssets(n: number): MockAsset[] {
  const r = rng(42);
  const out: MockAsset[] = [];
  const start = Date.UTC(2015, 0, 1);
  const span = Date.UTC(2025, 11, 31) - start;
  for (let i = 0; i < n; i++) {
    const utc = start + Math.floor(r() * span);
    const d = new Date(utc + 3600_000);
    const local = d.toISOString().slice(0, 19);
    const kind = r();
    const video = kind > 0.93;
    const aspect = video ? 16 / 9 : ([4 / 3, 3 / 4, 1, 16 / 9, 9 / 19.5, 3 / 2][Math.floor(r() * 6)] ?? 1);
    let flags = video ? Flag.VIDEO : 0;
    if (!video && r() > 0.95) flags |= Flag.LIVE;
    if (r() > 0.97) flags |= Flag.FAVORITE;
    const hue = Math.floor(r() * 24) * 15;
    const [rr, gg, bb] = hslToRgb(hue / 360, 0.45, 0.55);
    const cat = CATS[Math.floor(Math.pow(r(), 2.2) * 7)]![0];
    out.push({
      id: i + 1,
      local,
      utc,
      aspect,
      color: (rr << 16) | (gg << 8) | bb,
      flags,
      name: video ? `IMG_${1000 + i}.MOV` : `IMG_${1000 + i}.HEIC`,
      dir: `${local.slice(0, 4)}`,
      category: cat,
      camera: cat === 'photos' ? 'iPhone 12' : null,
      lat: GEO ? 36 + r() * 24 : r() > 0.7 ? 45 + r() * 3 : null,
      lon: GEO ? -9 + r() * 38 : r() > 0.7 ? 8 + r() * 4 : null,
      duration: video ? Math.floor(r() * 60_000) + 2000 : null,
      rating: 0,
      tags: [],
      trashed: false,
      dirty: false,
      review: r() > 0.6 ? 1 : 2,
      conf: 0.5 + r() * 0.5,
    });
  }
  out.sort((a, b) => (a.local < b.local ? 1 : a.local > b.local ? -1 : b.id - a.id));
  return out;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const f = (n: number) => {
    const k = (n + h * 12) % 12;
    const a = s * Math.min(l, 1 - l);
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return [f(0), f(8), f(4)];
}

const defaults: Settings = {
  first_run_done: false,
  write_mode: 'file_and_sidecar',
  theme: 'system',
  row_height: 200,
  ai_mode: 'off',
  ai_local_endpoint: null,
  ai_decision_model: null,
  ai_text_model: null,
  ai_budget_usd: 5,
  ai_allow_private_cloud: false,
  ai_allow_non_zdr: false,
  immich_url: null,
  network: { immich: false, ai: false, downloads: false, online_tiles: false, online_search: false, update_checks: false },
  threads: null,
  use_gpu: true,
  only_when_idle: false,
  pause_on_battery: true,
  preview_cache_gb: 5,
  rename_files: false,
  rename_template: '{date} {time} {camera}',
  organize_mode: 'move',
  copy_target: null,
  keep_folders_in_sync: false,
  home: null,
  auto_precision: 0.98,
  interpolation_minutes: 30,
  quicktime_local_models: [],
  guide_done: false,
};

const setup = params.get('setup') === '1';
let settings: Settings = { ...defaults, first_run_done: setup };
let roots: RootInfo[] = setup ? [{ id: 1, path: 'C:\\Users\\Example\\Pictures', kind: 'library', count: N }] : [];
let assets: MockAsset[] = setup ? makeAssets(N) : [];
let jobs: JobInfo[] = [];
const secrets = new Set<string>();
let history: BatchInfo[] = [];
/** Undo data per batch: the asset fields before the change. */
const undoData = new Map<number, Map<number, Partial<MockAsset>>>();
let batchSeq = 0;
let currentPlan: PlanSummary | null = null;
let rules: Rule[] = [
  rule(1, 'ios_screenshot', 'iOS screenshot marker', 'screenshot', 'eq', true, 'screenshots', 1, true, 'the iOS screenshot marker'),
  rule(2, 'screenshot_name', 'Screenshot file name', 'name', 'regex', '(?i)^(screenshot|screen shot)[ _-]', 'screenshots', 0.99, true, 'a screenshot file name'),
  rule(3, 'whatsapp_name', 'WhatsApp file name', 'name', 'regex', '^(IMG|VID)-[0-9]{8}-WA[0-9]+', null, 1, true, 'a WhatsApp file name', ['Source/WhatsApp']),
  rule(4, 'camera_make', 'Camera make present', 'has_camera', 'eq', true, 'photos', 0.8, false, 'a camera make in the file'),
  rule(5, 'gif', 'GIF image', 'format', 'eq', 'gif', 'memes', 0.6, false, 'a GIF'),
];

function rule(
  id: number,
  key: string | null,
  name: string,
  field: string,
  op: Rule['def']['when'][number]['op'],
  value: string | number | boolean,
  category: string | null,
  confidence: number,
  certain: boolean,
  because: string,
  tags: string[] = [],
): Rule {
  return { id, key, name, enabled: true, priority: id * 10, builtin: key != null, def: { when: [{ field, op, value }], then: { category, confidence, certain, tags, because } } };
}

const plural = (n: number) => (n === 1 ? '1 item' : `${n} items`);

function pushBatch(id: number, kind: string, label: string, ops: number) {
  history = [{ id, kind, label, state: 'applied', created_at: Date.now(), applied_at: Date.now(), ops, failed: 0, summary: null }, ...history];
}

/** Record a change as an undoable batch, like the library does. */
function change(kind: string, label: string, ids: number[], fn: (a: MockAsset) => boolean, dirty = true): number {
  const before = new Map<number, Partial<MockAsset>>();
  let n = 0;
  for (const id of ids) {
    const a = byId(id);
    if (!a) continue;
    const snap = { category: a.category, flags: a.flags, rating: a.rating, tags: [...a.tags], trashed: a.trashed, dirty: a.dirty, aspect: a.aspect, local: a.local, utc: a.utc, lat: a.lat, lon: a.lon, review: a.review };
    if (fn(a)) {
      before.set(id, snap);
      if (dirty) a.dirty = true;
      n++;
    }
  }
  if (n > 0) {
    const id = ++batchSeq;
    undoData.set(id, before);
    pushBatch(id, kind, label, n);
    assets = [...assets];
    emit('changed', 'assets');
    emit('changed', 'history');
  }
  return n;
}

const PLACES: PlaceHit[] = [
  { label: 'Rome, Latium, Italy', name: 'Rome', kind: 'city', lat: 41.8919, lon: 12.5113, accuracy_m: 5000 },
  { label: 'Riva del Garda, Trentino-Alto Adige, Italy', name: 'Riva del Garda', kind: 'city', lat: 45.8858, lon: 10.841, accuracy_m: 5000 },
  { label: 'Zürich, Zurich, Switzerland', name: 'Zürich', kind: 'city', lat: 47.3667, lon: 8.55, accuracy_m: 5000 },
  { label: 'Lisbon, Lisbon, Portugal', name: 'Lisbon', kind: 'city', lat: 38.7167, lon: -9.1333, accuracy_m: 5000 },
  { label: 'New York City, New York, United States', name: 'New York City', kind: 'city', lat: 40.7143, lon: -74.006, accuracy_m: 5000 },
  { label: 'Italy', name: 'Italy', kind: 'country', lat: 43.0, lon: 12.0, accuracy_m: 200000 },
];
let placesInstalled = setup && params.get('places') !== '0';
const pack = (id: string, title: string, size: number, host: string, license: string, verified = true): PackStatus => ({
  id,
  title,
  purpose: `What ${title.toLowerCase()} is for.`,
  license,
  source: host,
  attribution: `${title}, ${license}`,
  hosts: [host],
  size,
  done: 0,
  state: 'available',
  error: null,
  verified,
});
const packs: PackStatus[] = [
  pack('places', 'Place names', 14_055_527, 'download.geonames.org', 'CC BY 4.0', false),
  pack('image-model', 'Image understanding', 1_535_520_638, 'huggingface.co', 'Apache-2.0'),
  pack('text-reader', 'Text in photos', 31_114_837, 'github.com', 'Apache-2.0'),
];
const packTimers = new Map<string, ReturnType<typeof setInterval>>();
let tracks: TrackInfo[] = [];
const dismissed = new Set<string>();
let dupePairs: number[][] | null = null;

const fmtLocal = (ms: number) => new Date(ms).toISOString().slice(0, 19);
const parseLocal = (s: string) => Date.parse(`${s}Z`);

/** A camera set for the Fix dates view: two dozen photos a year off. */
function mockCameraSet(): MockAsset[] {
  return assets.filter((a) => !a.trashed && a.category === 'photos' && a.local.startsWith('2020-0')).slice(0, 24);
}

function nearest(lat: number, lon: number) {
  let best = PLACES[0]!;
  let d = Infinity;
  for (const p of PLACES) {
    const e = (p.lat - lat) ** 2 + (p.lon - lon) ** 2;
    if (e < d && p.kind === 'city') {
      d = e;
      best = p;
    }
  }
  return best;
}

function mockGroups(): PlaceGroup[] {
  const none = assets.filter((a) => !a.trashed && a.lat == null).sort((a, b) => (a.local < b.local ? -1 : 1));
  const byMonth = new Map<string, MockAsset[]>();
  for (const a of none) {
    const k = a.local.slice(0, 7);
    byMonth.set(k, [...(byMonth.get(k) ?? []), a]);
  }
  return [...byMonth.entries()]
    .sort((a, b) => b[1].length - a[1].length)
    .slice(0, 4)
    .map(([month, list], i) => {
      const p = PLACES[i % 5]!;
      return {
        key: `neighbors::${month}`,
        source: i === 0 ? 'track' : 'neighbors',
        evidence: i === 0 ? 'lake-garda-2019.gpx' : 'your geotagged photos taken around the same time',
        place: p.label,
        count: list.length,
        from_local: list[0]!.local,
        to_local: list[list.length - 1]!.local,
        lat: p.lat,
        lon: p.lon,
        accuracy_m: i === 0 ? 40 : 1200,
        ids: list.map((a) => a.id),
      };
    })
    .filter((g) => !dismissed.has(g.key));
}

function shifted(fix: DateFix, a: MockAsset): string | null {
  const ref = fix.reference ? byId(fix.reference.asset_id) : undefined;
  switch (fix.mode) {
    case 'shift': {
      if (!ref || !fix.reference) return null;
      return fmtLocal(parseLocal(a.local) + parseLocal(fix.reference.local) - parseLocal(ref.local));
    }
    case 'amount': {
      const m = fix.amount;
      if (!m) return null;
      const d = new Date(parseLocal(a.local));
      d.setUTCFullYear(d.getUTCFullYear() + m.years);
      return fmtLocal(d.getTime() + (((m.days * 24 + m.hours) * 60 + m.minutes) * 60 + m.seconds) * 1000);
    }
    case 'exact':
      return fix.exact?.local ?? null;
    case 'zone':
      return a.local;
    default:
      return null;
  }
}

function targetDir(a: MockAsset) {
  const name = CATS.find((c) => c[0] === a.category)?.[1] ?? a.category;
  return a.category === 'wallpapers' ? name : `${a.local.slice(0, 4)}/${name}`;
}
const listeners: { [K in keyof Events]: Set<Events[K]> } = { jobs: new Set(), changed: new Set(), scanned: new Set() };

function emit<K extends keyof Events>(k: K, ...args: Parameters<Events[K]>) {
  for (const cb of listeners[k]) (cb as (...a: unknown[]) => void)(...args);
}

function filter(q: Query): MockAsset[] {
  return assets.filter((a) => {
    if (q.media.length) {
      const v = (a.flags & Flag.VIDEO) !== 0;
      if (!q.media.some((m) => (m === 'video' ? v : m === 'photo' ? !v : m === 'live' ? (a.flags & Flag.LIVE) !== 0 : false))) return false;
    }
    if (q.categories.length && !q.categories.includes(a.category)) return false;
    if (q.exclude_categories.includes(a.category)) return false;
    if (q.from_local != null && a.local < q.from_local) return false;
    if (q.to_local != null && a.local >= q.to_local) return false;
    if (q.from_utc != null && a.utc < q.from_utc) return false;
    if (q.to_utc != null && a.utc >= q.to_utc) return false;
    if (q.location === 'none' && a.lat != null) return false;
    if (q.location === 'any' && a.lat == null) return false;
    if (q.favorite && !(a.flags & Flag.FAVORITE)) return false;
    if (q.cameras.length && !q.cameras.includes(a.camera ?? '')) return false;
    if (q.folder != null && q.folder !== '' && !(q.recursive ? a.dir.startsWith(q.folder) : a.dir === q.folder)) return false;
    if (q.ids && !q.ids.includes(a.id)) return false;
    if (a.trashed !== q.trashed) return false;
    if (q.min_rating != null && a.rating < q.min_rating) return false;
    if (q.tags.length && !q.tags.every((t) => a.tags.includes(t))) return false;
    if (q.text && !a.name.toLowerCase().includes(q.text.toLowerCase())) return false;
    return true;
  });
}

const svgCache = new Map<string, string>();

/** Placeholder pictures, cached by color and shape so scrolling stays cheap. */
function svg(a: MockAsset | undefined, w: number): string {
  const key = `${a?.color ?? 0}:${a?.aspect ?? 1}:${w}`;
  let url = svgCache.get(key);
  if (!url) {
    url = makeSvg(a, w);
    svgCache.set(key, url);
  }
  return url;
}

function makeSvg(a: MockAsset | undefined, w: number): string {
  const c = a ? `rgb(${(a.color >> 16) & 255},${(a.color >> 8) & 255},${a.color & 255})` : '#888';
  const h = Math.round(w / (a?.aspect ?? 1));
  const body = `<svg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}' viewBox='0 0 ${w} ${h}'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='${c}'/><stop offset='1' stop-color='#222'/></linearGradient></defs><rect width='100%' height='100%' fill='url(#g)'/><circle cx='${w * 0.7}' cy='${h * 0.3}' r='${Math.min(w, h) * 0.12}' fill='rgba(255,255,255,0.7)'/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(body)}`;
}

let index = new Map<number, MockAsset>();
let indexed: MockAsset[] = [];
function byId(id: number) {
  if (indexed !== assets) {
    index = new Map(assets.map((a) => [a.id, a]));
    indexed = assets;
  }
  return index.get(id);
}

function tree(): FolderNode[] {
  if (!roots.length) return [];
  const years = new Map<string, number>();
  for (const a of assets) years.set(a.dir, (years.get(a.dir) ?? 0) + 1);
  const children = [...years.entries()].sort().map(([y, n]) => ({ root_id: 1, path: y, name: y, count: n, total: n, children: [] }));
  return [{ root_id: 1, path: '', name: 'Pictures', count: 0, total: assets.length, children }];
}

function simulateScan() {
  const job: JobInfo = { id: 1, kind: 'scan', label: 'Indexing the library', state: 'running', done: 0, total: N, rate: 0, eta_secs: null, cost_usd: 0, error: null };
  jobs = [job];
  const all = makeAssets(N);
  let done = 0;
  const step = () => {
    done = Math.min(N, done + Math.ceil(N / 8));
    assets = all.slice(all.length - done);
    job.done = done;
    job.rate = 4000;
    emit('jobs', [...jobs]);
    emit('changed', 'assets');
    if (done < N) setTimeout(step, 120);
    else {
      job.state = 'done';
      roots = roots.map((r) => ({ ...r, count: N }));
      emit('jobs', [...jobs]);
      emit('changed', 'roots');
    }
  };
  setTimeout(step, 100);
}

export const mockApi: Api = {
  mock: true,
  appInfo: async () => ({ version: '0.1.0', data_dir: 'C:\\Users\\Example\\AppData\\Local\\Roll', exiftool: 'exiftool', ffmpeg: 'ffmpeg', os: 'windows' }),
  getSettings: async () => structuredClone(settings),
  saveSettings: async (s) => {
    settings = structuredClone(s);
    emit('changed', 'settings');
  },
  pickFolder: async () => 'C:\\Users\\Example\\Pictures',
  roots: async () => roots,
  addRoot: async (path) => {
    const r = { id: roots.length + 1, path, kind: 'library', count: 0 };
    roots = [...roots, r];
    simulateScan();
    return r;
  },
  removeRoot: async (id) => {
    roots = roots.filter((r) => r.id !== id);
    if (!roots.length) assets = [];
    emit('changed', 'roots');
  },
  rescan: async () => undefined,
  timeline: async (q): Promise<Timeline> => {
    const list = filter(q);
    const n = list.length;
    const t = {
      count: n,
      ids: new Uint32Array(n),
      days: new Uint32Array(n),
      aspects: new Float32Array(n),
      colors: new Uint32Array(n),
      flags: new Uint16Array(n),
      versions: new Uint16Array(n).fill(1),
    };
    list.forEach((a, i) => {
      t.ids[i] = a.id;
      t.days[i] = Number(a.local.slice(0, 10).replaceAll('-', ''));
      t.aspects[i] = a.aspect;
      t.colors[i] = a.color;
      t.flags[i] = a.flags;
    });
    return t;
  },
  queryIds: async (q) => filter(q).map((a) => a.id),
  tiles: async (ids) =>
    ids.map((id) => {
      const a = byId(id);
      return { id, name: a?.name ?? '', thumb_v: 1, thumbhash: null, duration_ms: a?.duration ?? null, caption: null };
    }),
  assetDetail: async (id): Promise<AssetDetail | null> => {
    const a = byId(id);
    if (!a) return null;
    const video = (a.flags & Flag.VIDEO) !== 0;
    return {
      id,
      root_id: 1,
      path: `C:\\Users\\Example\\Pictures\\${a.dir}\\${a.name}`,
      rel_path: `${a.dir}/${a.name}`,
      name: a.name,
      kind: video ? 'video' : 'photo',
      format: video ? 'mov' : 'heic',
      size: 2_400_000,
      width: Math.round(3000 * Math.min(1, a.aspect)),
      height: Math.round(3000 / Math.max(1, a.aspect)),
      duration_ms: a.duration,
      fps: video ? 30 : null,
      codec: video ? 'h264' : null,
      has_audio: video ? true : null,
      taken_utc: a.utc,
      taken_local: a.local,
      tz_offset: 60,
      date_source: 'exif_original',
      lat: a.lat,
      lon: a.lon,
      loc_acc: a.lat != null ? 10 : null,
      loc_source: a.lat != null ? 'file' : null,
      place: null,
      make: a.camera ? 'Apple' : null,
      model: a.camera,
      lens: a.camera ? 'iPhone 12 back camera 4.2mm f/1.6' : null,
      software: null,
      category: a.category,
      category_name: CATS.find((c) => c[0] === a.category)?.[1] ?? null,
      category_conf: 0.97,
      review: 2,
      tags: [...(a.category === 'photos' || a.category === 'unsorted' ? [] : [`Category/${CATS.find((c) => c[0] === a.category)?.[1]}`]), ...a.tags],
      rating: a.rating,
      favorite: (a.flags & Flag.FAVORITE) !== 0,
      description: null,
      private: false,
      immich: 'default',
      immich_id: null,
      group_kind: a.flags & Flag.LIVE ? 'live' : null,
      group_role: a.flags & Flag.LIVE ? 'primary' : null,
      group_members: [],
      problems: 0,
      thumb_v: 1,
      why: [
        { field: 'date', source: 'exif_original', text: "Date from the camera's date taken.", conf: null, at: 0 },
      ],
      sidecars: [],
      missing: false,
      trashed: a.trashed,
    };
  },
  folders: async () => tree(),
  categories: async (): Promise<Category[]> =>
    CATS.map(([key, name], i) => ({
      id: i + 1,
      key,
      name,
      parent_id: null,
      definition: '',
      folder_template: key === 'unsorted' ? null : `{year}/${name}`,
      own_folder: true,
      immich_default: key === 'photos',
      auto_threshold: null,
      builtin: true,
      count: assets.filter((a) => a.category === key && !a.trashed).length,
    })),
  tagCounts: async () => {
    const m = new Map<string, number>();
    for (const a of assets) for (const t of a.tags) if (!a.trashed) m.set(t, (m.get(t) ?? 0) + 1);
    return [...m.entries()].sort().map(([path, count], i) => ({ id: i + 1, path, count }));
  },
  counts: async () => ({
    total: assets.length,
    inbox: 0,
    review: assets.filter((a) => !a.trashed && a.review === 1).length,
    date_issues: 12,
    no_location: assets.filter((a) => a.lat == null).length,
    trash: assets.filter((a) => a.trashed).length,
    duplicates: 20,
  }),
  facets: async (q) => {
    const list = filter(q);
    const count = <T extends string>(f: (a: MockAsset) => T | null) => {
      const m = new Map<string, number>();
      for (const a of list) {
        const k = f(a);
        if (k) m.set(k, (m.get(k) ?? 0) + 1);
      }
      return [...m.entries()].sort((a, b) => b[1] - a[1]).map(([key, n]) => ({ key, label: key, count: n }));
    };
    return {
      categories: count((a) => a.category).map((f) => ({ ...f, label: CATS.find((c) => c[0] === f.key)?.[1] ?? f.key })),
      media: count((a) => (a.flags & Flag.VIDEO ? 'video' : 'photo')).map((f) => ({ ...f, label: f.key === 'video' ? 'Videos' : 'Photos' })),
      cameras: count((a) => a.camera),
      lenses: [],
      formats: count((a) => (a.flags & Flag.VIDEO ? 'mov' : 'heic')).map((f) => ({ ...f, label: f.key.toUpperCase() })),
      years: count((a) => a.local.slice(0, 4)).sort((a, b) => (a.key < b.key ? 1 : -1)),
      location: count((a) => (a.lat == null ? 'none' : 'exact')).map((f) => ({ ...f, label: f.key === 'none' ? 'No location' : 'Exact' })),
      problems: [],
      tags: [],
      immich: [],
      orientation: count((a) => (a.aspect > 1 ? 'landscape' : a.aspect < 1 ? 'portrait' : 'square')).map((f) => ({ ...f, label: f.key[0]!.toUpperCase() + f.key.slice(1) })),
    };
  },
  jobs: async () => jobs,
  jobControl: async (id, action) => {
    jobs = jobs.map((j) => (j.id === id ? { ...j, state: action === 'pause' ? 'paused' : action === 'resume' ? 'running' : 'cancelled' } : j));
    emit('jobs', jobs);
  },
  showInFolder: async () => undefined,
  openFolder: async () => undefined,
  openWithDefault: async () => undefined,
  clearLogs: async () => undefined,
  setSecret: async (name, value) => {
    if (value) secrets.add(name);
    else secrets.delete(name);
  },
  hasSecret: async (name) => secrets.has(name),
  deleteSecret: async (name) => void secrets.delete(name),
  edit: async (ids, action) => {
    const name = (k: string | null) => CATS.find((c) => c[0] === k)?.[1] ?? 'Unsorted';
    const items = plural(ids.length);
    switch (action.kind) {
      case 'category':
        return change('edit', action.key ? `Move ${items} to ${name(action.key)}` : `Mark ${items} as unsorted`, ids, (a) => {
          const k = action.key ?? 'unsorted';
          if (a.category === k && a.review === 3) return false;
          a.category = k;
          a.review = k === 'unsorted' ? 0 : 3;
          return true;
        });
      case 'accept':
        return change('edit', `Accept ${items}`, ids, (a) => {
          if (a.review === 3) return false;
          a.review = 3;
          return true;
        });
      case 'add_tag':
        return change('edit', `Tag ${items} ${action.path}`, ids, (a) => {
          if (a.tags.includes(action.path)) return false;
          a.tags = [...a.tags, action.path];
          return true;
        });
      case 'remove_tag':
        return change('edit', `Remove ${action.path} from ${items}`, ids, (a) => {
          if (!a.tags.includes(action.path)) return false;
          a.tags = a.tags.filter((t) => t !== action.path);
          return true;
        });
      case 'rating':
        return change('edit', `Rate ${items} ${action.rating}`, ids, (a) => {
          if (a.rating === action.rating) return false;
          a.rating = action.rating;
          return true;
        });
      case 'favorite':
        return change('edit', action.on ? `Favorite ${items}` : `Remove ${items} from favorites`, ids, (a) => {
          if (((a.flags & Flag.FAVORITE) !== 0) === action.on) return false;
          a.flags ^= Flag.FAVORITE;
          return true;
        });
      case 'rotate':
        return change('edit', `Rotate ${items} ${action.turns < 0 ? 'left' : 'right'}`, ids, (a) => {
          if (a.flags & Flag.VIDEO) return false;
          if (action.turns % 2) a.aspect = 1 / a.aspect;
          return true;
        });
      default:
        return 0;
    }
  },
  setTrashed: async (ids, trashed) => {
    const items = plural(ids.length);
    const label = trashed ? `Move ${items} to the trash` : `Restore ${items} from the trash`;
    change(
      trashed ? 'trash' : 'restore',
      label,
      ids,
      (a) => {
        if (a.trashed === trashed) return false;
        a.trashed = trashed;
        return true;
      },
      false,
    );
  },
  emptyTrash: async (ids) => {
    const gone = new Set(assets.filter((a) => a.trashed && (!ids || ids.includes(a.id))).map((a) => a.id));
    assets = assets.filter((a) => !gone.has(a.id));
    pushBatch(++batchSeq, 'empty_trash', `Send ${plural(gone.size)} to the Recycle Bin`, gone.size);
    emit('changed', 'assets');
    emit('changed', 'history');
    return gone.size;
  },
  plan: async (options) => {
    const scope = options.ids.length ? new Set(options.ids) : null;
    const live = assets.filter((a) => !a.trashed && (!scope || scope.has(a.id)));
    const folders = new Map<string, { count: number; samples: number[] }>();
    let undecided = 0;
    let unchanged = 0;
    let privateHeld = 0;
    for (const a of live) {
      if (a.category === 'unsorted') {
        undecided++;
        continue;
      }
      if (a.flags & Flag.PRIVATE && a.review < 3) {
        privateHeld++;
        continue;
      }
      const dir = targetDir(a);
      if (a.dir === dir) {
        unchanged++;
        continue;
      }
      const f = folders.get(dir) ?? { count: 0, samples: [] };
      f.count++;
      if (f.samples.length < 12) f.samples.push(a.id);
      folders.set(dir, f);
    }
    const moved = [...folders.values()].reduce((s, f) => s + f.count, 0);
    const writes = options.write_metadata ? live.filter((a) => a.dirty).length : 0;
    const first = live.find((a) => a.category !== 'unsorted');
    currentPlan = {
      batch_id: ++batchSeq,
      moved,
      renamed: options.rename ? moved : 0,
      unchanged,
      undecided,
      private_held: privateHeld,
      writes,
      new_folders: folders.size,
      files: moved,
      conflicts: first && moved > 3 ? [{ asset_id: first.id, wanted: `${targetDir(first)}/${first.name}`, got: `${targetDir(first)}/${first.name.replace('.', ' (2).')}` }] : [],
      folders: [...folders.entries()].sort().map(([path, f]) => ({ root_id: 1, path, count: f.count, samples: f.samples, new: true })),
    };
    return structuredClone(currentPlan);
  },
  savePlanCsv: async () => true,
  applyPlan: async (batch) => {
    const p = currentPlan;
    if (!p || p.batch_id !== batch) throw new Error('This plan is out of date. Preview it again.');
    const before = new Map<number, Partial<MockAsset>>();
    for (const a of assets) {
      if (a.trashed || a.category === 'unsorted') continue;
      const dir = targetDir(a);
      if (a.dir !== dir || a.dirty) {
        before.set(a.id, { dir: a.dir, dirty: a.dirty });
        a.dir = dir;
        a.dirty = false;
      }
    }
    undoData.set(batch, before);
    pushBatch(batch, 'organize', p.moved ? `Organize ${plural(p.moved)}` : `Save changes to ${p.writes} files`, p.files + p.writes);
    currentPlan = null;
    assets = [...assets];
    emit('changed', 'assets');
    emit('changed', 'history');
  },
  undo: async (batch) => {
    const b = history.find((h) => h.id === batch);
    if (!b || b.state !== 'applied') throw new Error('Only applied changes can be undone.');
    if (b.kind === 'empty_trash') throw new Error('Items sent to the Recycle Bin can be restored from the Recycle Bin.');
    for (const [id, snap] of undoData.get(batch) ?? []) {
      const a = byId(id);
      if (a) Object.assign(a, snap);
    }
    history = history.map((h) => (h.id === batch ? { ...h, state: 'undone' } : h));
    assets = [...assets];
    emit('changed', 'assets');
    emit('changed', 'history');
  },
  undoLast: async () => {
    const last = history.find((h) => h.state === 'applied' && h.kind !== 'empty_trash');
    if (!last) return null;
    await mockApi.undo(last.id);
    return last.label;
  },
  history: async (limit) => history.slice(0, limit),
  unsaved: async () => assets.filter((a) => a.dirty && !a.trashed).length,
  addCategory: async (name, parent) => {
    const key = (parent ? `${parent}.` : '') + name.toLowerCase().replace(/[^a-z0-9]+/g, '_');
    if (CATS.some((c) => c[0] === key)) throw new Error(`A category named ${name} already exists here.`);
    CATS = [...CATS.slice(0, -1), [key, name], CATS[CATS.length - 1]!];
    emit('changed', 'categories');
    return key;
  },
  updateCategory: async (key, patch) => {
    const name = patch.name;
    if (name) CATS = CATS.map((c) => (c[0] === key ? [key, name] : c));
    emit('changed', 'categories');
  },
  deleteCategory: async (key) => {
    CATS = CATS.filter((c) => c[0] !== key);
    for (const a of assets) if (a.category === key) a.category = 'unsorted';
    emit('changed', 'categories');
  },
  rules: async () => structuredClone(rules),
  saveRule: async (r) => {
    if (r.id > 0) {
      rules = rules.map((x) => (x.id === r.id ? structuredClone(r) : x));
      return r.id;
    }
    const id = Math.max(0, ...rules.map((x) => x.id)) + 1;
    rules = [...rules, { ...structuredClone(r), id }];
    return id;
  },
  deleteRule: async (id) => {
    if (rules.find((r) => r.id === id)?.builtin) throw new Error('Built-in rules can be turned off but not deleted.');
    rules = rules.filter((r) => r.id !== id);
  },
  dateSets: async (): Promise<DateSet[]> => {
    const set = mockCameraSet();
    if (!set.length || dismissed.has('camera:NIKON D750')) return [];
    const sorted = [...set].sort((a, b) => (a.local < b.local ? -1 : 1));
    const off = sorted[0]!.local.startsWith('2020');
    return [
      {
        key: 'camera:NIKON D750',
        kind: 'camera',
        label: 'NIKON D750',
        count: set.length,
        from_local: sorted[0]!.local,
        to_local: sorted[sorted.length - 1]!.local,
        reasons: off ? ['The folder says 2019, but the photos are dated 2020.'] : [],
        has_offset: false,
        ids: set.map((a) => a.id),
      },
    ];
  },
  datePreview: async (fix) => {
    const list = fix.ids.map((id) => byId(id)).filter((a): a is MockAsset => !!a);
    const pairs = list.map((a) => [a, a.local, shifted(fix, a)] as const).filter((p) => p[2] != null) as [MockAsset, string, string][];
    const before = pairs.map((p) => p[1]).sort();
    const after = pairs.map((p) => p[2]).sort();
    const ref = fix.reference ? byId(fix.reference.asset_id) : undefined;
    const deltaH = ref && fix.reference ? Math.round((parseLocal(fix.reference.local) - parseLocal(ref.local)) / 3_600_000) : 0;
    return {
      count: pairs.length,
      label: fix.mode === 'shift' ? `${deltaH < 0 ? '−' : '+'}${Math.abs(deltaH)} h` : fix.mode === 'zone' ? 'time zone' : 'set',
      before_from: before[0] ?? '',
      before_to: before[before.length - 1] ?? '',
      after_from: after[0] ?? '',
      after_to: after[after.length - 1] ?? '',
      year_changes: pairs.filter((p) => p[1].slice(0, 4) !== p[2].slice(0, 4)).length,
      folder_changes: pairs.filter((p) => p[1].slice(0, 4) !== p[2].slice(0, 4) && p[0].category !== 'unsorted').length,
      samples: pairs.slice(0, 8).map(([a, b, c]) => ({ id: a.id, before: b, after: c })),
    };
  },
  dateApply: async (fix) => {
    const list = fix.ids.map((id) => byId(id)).filter((a): a is MockAsset => !!a);
    const n = change('edit', `Shift ${plural(list.length)}`, fix.ids, (a) => {
      const next = shifted(fix, a);
      if (!next || next === a.local) return false;
      a.utc += parseLocal(next) - parseLocal(a.local);
      a.local = next;
      return true;
    });
    if (n) dismissed.add('camera:NIKON D750');
    assets.sort((a, b) => (a.local < b.local ? 1 : a.local > b.local ? -1 : b.id - a.id));
    return n;
  },
  dateCandidates: async (ids) => ids.map((id) => ({ id, local: byId(id)?.local ?? '2020-01-01T12:00:00', source: 'filename', conf: 0.7, why: 'The file name contains the date.' })),
  dismiss: async (_kind, key) => {
    dismissed.add(key);
    emit('changed', 'places');
  },
  placesInfo: async () => ({ installed: placesInstalled, count: placesInstalled ? PLACES.length : 0 }),
  installPlaces: async () => {
    placesInstalled = true;
    emit('changed', 'places');
    return { installed: true, count: PLACES.length };
  },
  packs: async () => packs.map((p) => ({ ...p })),
  downloadPack: async (id) => {
    if (!settings.network.downloads) throw new Error('Downloads are off. Turn them on in Settings › Network.');
    const p = packs.find((x) => x.id === id);
    if (!p || packTimers.has(id)) return;
    p.state = 'downloading';
    p.error = null;
    emit('changed', 'packs');
    packTimers.set(
      id,
      setInterval(() => {
        p.done = Math.min(p.size, p.done + Math.ceil(p.size / 25));
        if (p.done >= p.size) {
          clearInterval(packTimers.get(id));
          packTimers.delete(id);
          p.state = 'installed';
          if (id === 'places') {
            placesInstalled = true;
            emit('changed', 'places');
          }
        }
        emit('changed', 'packs');
      }, 80),
    );
  },
  cancelPack: async (id) => {
    const p = packs.find((x) => x.id === id);
    clearInterval(packTimers.get(id));
    packTimers.delete(id);
    if (p && p.state !== 'installed') {
      p.state = 'available';
      p.error = null;
    }
    emit('changed', 'packs');
  },
  removePack: async (id) => {
    const p = packs.find((x) => x.id === id);
    if (!p) return;
    p.state = 'available';
    p.done = 0;
    if (id === 'places') {
      placesInstalled = false;
      emit('changed', 'places');
    }
    emit('changed', 'packs');
  },
  placeSearch: async (q) => {
    const m = q.match(/^\s*(-?\d+(?:\.\d+)?)\s*[, ]\s*(-?\d+(?:\.\d+)?)\s*$/);
    const out: PlaceHit[] = [];
    if (m) {
      const lat = Number(m[1]);
      const lon = Number(m[2]);
      out.push({ label: nearest(lat, lon).label, name: `${lat.toFixed(5)}, ${lon.toFixed(5)}`, kind: 'city', lat, lon, accuracy_m: 10 });
    }
    const t = q.trim().toLowerCase();
    if (t && placesInstalled) out.push(...PLACES.filter((p) => p.label.toLowerCase().includes(t)));
    return out;
  },
  placeName: async (lat, lon) => (placesInstalled ? nearest(lat, lon).label : null),
  tracks: async () => tracks,
  importTracks: async () => {
    const t: TrackInfo = { id: tracks.length + 1, name: 'lake-garda-2019.gpx', format: 'GPX', count: 2161, start_utc: Date.UTC(2019, 7, 3, 6), end_utc: Date.UTC(2019, 7, 9, 18), imported_at: Date.now() };
    tracks = [...tracks, t];
    emit('changed', 'places');
    return [t];
  },
  deleteTrack: async (id) => {
    tracks = tracks.filter((t) => t.id !== id);
    emit('changed', 'places');
  },
  placeGroups: async () => mockGroups(),
  acceptPlaces: async (key) => {
    const g = mockGroups().find((x) => x.key === key);
    if (!g) throw new Error('This suggestion is out of date. Refresh the list.');
    change('edit', `Set the location of ${plural(g.ids.length)}`, g.ids, (a) => {
      a.lat = g.lat + (Math.random() - 0.5) * 0.01;
      a.lon = g.lon + (Math.random() - 0.5) * 0.01;
      return true;
    });
    return { count: g.ids.length, zones: [{ zone: 'Europe/Rome', offset_min: 120, count: Math.ceil(g.ids.length / 3), ids: g.ids.slice(0, Math.ceil(g.ids.length / 3)) }] };
  },
  setLocation: async (ids, place) => {
    const n = change('edit', place ? `Set the location of ${plural(ids.length)}` : `Clear the location of ${plural(ids.length)}`, ids, (a) => {
      if (!place) {
        if (a.lat == null) return false;
        a.lat = null;
        a.lon = null;
        return true;
      }
      a.lat = place.lat;
      a.lon = place.lon;
      return true;
    });
    return { count: n, zones: [] };
  },
  applyZone: async (ids) => ids.length,
  mapPoints: async (q): Promise<MapPoints> => {
    const list = filter(q).filter((a) => a.lat != null && a.lon != null);
    if (!list.length) return emptyMapPoints();
    list.sort((a, b) => (a.local < b.local ? -1 : 1));
    return {
      count: list.length,
      ids: Uint32Array.from(list.map((a) => a.id)),
      days: Uint32Array.from(list.map((a) => Number(a.local.slice(0, 10).replaceAll('-', '')))),
      lats: Float32Array.from(list.map((a) => a.lat!)),
      lons: Float32Array.from(list.map((a) => a.lon!)),
      accs: Float32Array.from(list.map((a) => (a.id % 7 === 0 ? 5000 : 10))),
      flags: Uint16Array.from(list.map((a) => a.flags & (Flag.VIDEO | Flag.FAVORITE))),
    };
  },
  reviewSummary: async () => {
    const live = assets.filter((a) => !a.trashed);
    const proposed = live.filter((a) => a.review === 1);
    const tabs = CATS.map(([key, name]) => ({ key, name, count: proposed.filter((a) => a.category === key).length })).filter((t) => t.count > 0);
    tabs.sort((a, b) => b.count - a.count);
    return {
      reviewed: live.filter((a) => a.review === 3).length,
      auto_accepted: live.filter((a) => a.review === 2).length,
      left: proposed.length,
      tabs,
      precision: null,
      margin: null,
      audited: 0,
    };
  },
  reviewItems: async (key, limit) =>
    assets
      .filter((a) => !a.trashed && a.review === 1 && a.category === key)
      .sort((a, b) => a.conf - b.conf)
      .slice(0, limit)
      .map((a) => ({ id: a.id, conf: a.conf, why: `${CATS.find((c) => c[0] === a.category)?.[1]}, because of camera data in the file.`, thumb_v: 1 })),
  acceptAbove: async (key, min) =>
    change('edit', 'Accept proposals', assets.filter((a) => a.review === 1 && a.category === key && a.conf >= min).map((a) => a.id), (a) => {
      a.review = 3;
      return true;
    }),
  discover: async (split) => {
    const pool = assets.filter((a) => !a.trashed && a.review < 2 && a.category !== 'photos');
    const bySig = new Map<string, MockAsset[]>();
    for (const a of pool) {
      const k = split ? `${split}|${a.dir}` : `jpeg:${Math.round(a.aspect * 100)}:nocam:q`;
      if (split && !split.startsWith('jpeg:')) continue;
      bySig.set(k, [...(bySig.get(k) ?? []), a]);
    }
    return [...bySig.entries()]
      .filter(([k, v]) => (split ? true : v.length >= 5) && !dismissed.has(k))
      .sort((a, b) => b[1].length - a[1].length)
      .slice(0, 8)
      .map(([key, v]) => ({
        key,
        label: split ? `Same size, in ${key.split('|')[1]}` : `${Math.round(1000 * v[0]!.aspect)} × 1000 JPEG, no camera data, one encoder`,
        count: v.length,
        samples: v.slice(0, 12).map((a) => a.id),
        ids: v.map((a) => a.id),
        proposed: v[0]!.category,
      }));
  },
  discoverIgnore: async (key) => void dismissed.add(key),
  parseSearch: async (text) => {
    const words = text.toLowerCase().split(/\s+/).filter(Boolean);
    const query = { ...emptyQuery() };
    const understood: string[] = [];
    const rest: string[] = [];
    for (const w of words) {
      if (/^(19|20)\d\d$/.test(w)) {
        query.from_local = `${w}-01-01T00:00:00`;
        query.to_local = `${Number(w) + 1}-01-01T00:00:00`;
        understood.push(w);
      } else if (w === 'videos' || w === 'video') {
        query.media = ['video'];
        understood.push('Videos');
      } else if (w === 'memes') {
        query.categories = ['memes'];
        understood.push('Memes');
      } else rest.push(w);
    }
    return { query, rest: rest.join(' '), understood };
  },
  dupeGroups: async () => {
    // Fixed pairs, chosen once, so merging one group leaves the others.
    if (!dupePairs) {
      const photos = assets.filter((a) => !a.trashed && a.category === 'photos');
      dupePairs = [];
      for (let i = 0; i + 1 < Math.min(photos.length, 40); i += 4) dupePairs.push([photos[i]!.id, photos[i + 1]!.id]);
    }
    return dupePairs
      .filter((ids) => ids.every((id) => !byId(id)?.trashed))
      .map((ids, i) => ({ key: `exact:${ids.join(',')}`, kind: i % 2 === 0 ? 'exact' : 'copy', ids, why: i % 2 === 0 ? 'Identical; the first one found.' : 'The original camera file.' }))
      .filter((g) => !dismissed.has(g.key));
  },
  mergeDupes: async (keep, others) =>
    change('edit', `Merge ${plural(others.length)}`, others.filter((o) => o !== keep), (a) => {
      a.trashed = true;
      return true;
    }),
  on(event, cb) {
    listeners[event].add(cb as never);
    return () => listeners[event].delete(cb as never);
  },
  thumbUrl: (id) => svg(byId(id), 320),
  previewUrl: (id) => svg(byId(id), 1024),
  fullUrl: (id) => svg(byId(id), 2048),
  videoUrl: () => '',
  showWindow: async () => undefined,
  setWindowTheme: async () => undefined,
};
