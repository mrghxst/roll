// An in-memory backend for the browser: development without Tauri and the
// Playwright tests. URL parameters: `?mock=N` items (default 2400),
// `?setup=1` to start with a library already added.

import type { MapArea } from './bindings/MapArea';
import type { ModelOffer } from './bindings/ModelOffer';
import type { LocalModelInfo } from './bindings/LocalModelInfo';
import type { RunInfo } from './bindings/RunInfo';
import type { CheckItem } from './bindings/CheckItem';
import type { CheckModel } from './bindings/CheckModel';
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
import type { EventInfo } from './bindings/EventInfo';
import type { ImportInfo } from './bindings/ImportInfo';
import type { ImportPlan } from './bindings/ImportPlan';
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

interface Picture {
  url: string;
  category: string;
  aspect: number;
}

/**
 * `?pics=cc0`: real pictures instead of gradients, from the CC0 test set
 * the dev server serves at /__cc0/ (public domain, never the owner's
 * library). Used for README screenshots; empty when the set is missing.
 */
async function loadPictures(): Promise<Map<string, Picture[]>> {
  const out = new Map<string, Picture[]>();
  const m = (await fetch('/__cc0/manifest.json')
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null)) as { items: { file: string; category: string }[] } | null;
  const sized = await Promise.all(
    (m?.items ?? []).map(
      (it) =>
        new Promise<Picture | null>((resolve) => {
          const img = new Image();
          img.onload = () => resolve({ url: `/__cc0/${it.file}`, category: it.category, aspect: img.naturalWidth / img.naturalHeight });
          img.onerror = () => resolve(null);
          img.src = `/__cc0/${it.file}`;
        }),
    ),
  );
  for (const p of sized) if (p) out.set(p.category, [...(out.get(p.category) ?? []), p]);
  return out;
}

const PICS = params.get('pics') === 'cc0' ? await loadPictures() : null;

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
  /** The staging root of an import, while it is being checked. */
  staged?: number;
  /** "send", "local" or "default". */
  immich?: string;
  immichId?: string;
  /** `?pics=cc0`: the picture shown for it. */
  pic?: string;
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
  if (PICS?.size) {
    const all = [...PICS.values()].flat();
    const used = new Map<string, number>();
    for (const a of out) {
      const list = PICS.get(a.category) ?? all;
      const k = used.get(a.category) ?? 0;
      used.set(a.category, k + 1);
      const p = list[k % list.length]!;
      a.pic = p.url;
      a.aspect = p.aspect;
    }
  }
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
  ai_decision_model: params.get('model'),
  ai_text_model: null,
  ai_budget_usd: 5,
  ai_batch: 8,
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
  guide_steps: [],
};

const setup = params.get('setup') === '1';
// `?guide=1` keeps the first guide open after setup.
// `?ai=local` starts with local AI on.
let settings: Settings = {
  ...defaults,
  first_run_done: setup,
  guide_done: setup && params.get('guide') !== '1',
  ai_mode: params.get('ai') === 'local' ? 'local' : defaults.ai_mode,
};
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
/** Two bursts of four frames, for the best-shot picker. */
let bursts: number[][] | null = null;
function burstSeries() {
  if (!bursts) {
    const photos = assets.filter((a) => !a.trashed && a.category === 'photos').slice(60, 68).map((a) => a.id);
    bursts = [photos.slice(0, 4), photos.slice(4, 8)].filter((b) => b.length === 4);
  }
  return bursts
    .map((ids) => ids.filter((id) => !byId(id)?.trashed))
    .filter((ids) => ids.length > 1)
    .map((ids) => ({ key: `burst:${ids.join(',')}`, kind: 'burst', ids, why: 'The original camera file.' }));
}

/** People: four people over the photos, one face each in some of them. */
interface MockPerson {
  id: number;
  name: string | null;
  hidden: boolean;
}
interface MockFace {
  id: number;
  asset: number;
  x: number;
  y: number;
  w: number;
  h: number;
  person: number | null;
  rejected: number[];
}
// `?maps=1`: the Lake Garda area from fixtures-out/maps, served by the dev server.
let mapAreaList: MapArea[] =
  params.get('maps') === '1'
    ? [{ id: 'garda', name: 'Lake Garda', bbox: [10.5, 45.5, 11.0, 46.0], max_zoom: 13, tiles: 326, bytes: 17_178_624, state: 'ready', build: '20261015', error: null }]
    : [];
let people: MockPerson[] = [];
let faces: MockFace[] = [];
let peopleReady = false;
function ensurePeople() {
  if (peopleReady) return;
  peopleReady = true;
  people = [
    { id: 1, name: 'Anna', hidden: false },
    { id: 2, name: null, hidden: false },
    { id: 3, name: null, hidden: false },
    { id: 4, name: null, hidden: false },
  ];
  const photos = assets.filter((a) => a.category === 'photos' && a.duration == null).slice(0, 80);
  let id = 0;
  photos.forEach((a, i) => {
    const person = [1, 2, 3, 4, null][i % 5]!;
    faces.push({ id: ++id, asset: a.id, x: 0.38, y: 0.22, w: 0.2, h: 0.28, person, rejected: [] });
    if (i % 3 === 0) faces.push({ id: ++id, asset: a.id, x: 0.7, y: 0.3, w: 0.12, h: 0.18, person: i % 2 ? 2 : null, rejected: [] });
  });
}
const faceBox = (f: MockFace) => ({ id: f.id, asset: f.asset, x: f.x, y: f.y, w: f.w, h: f.h, person: f.person, ratio: byId(f.asset)?.aspect ?? 1.5 });
const liveFaces = () => faces.filter((f) => !byId(f.asset)?.trashed);
/** A people change as one undoable batch. */
function peopleChange(label: string, fn: () => void) {
  const before = { people: people.map((p) => ({ ...p })), faces: faces.map((f) => ({ ...f, rejected: [...f.rejected] })) };
  fn();
  const id = ++batchSeq;
  customUndo.set(id, () => {
    people = before.people;
    faces = before.faces;
    emit('changed', 'people');
  });
  pushBatch(id, 'edit', label, 1);
  emit('changed', 'people');
  emit('changed', 'history');
}

/** The mock's stand-in for image similarity: the closest colours. */
function lookAlike(ids: number[], limit: number): number[] {
  const rgb = (c: number) => [(c >> 16) & 255, (c >> 8) & 255, c & 255];
  const from = assets.filter((a) => ids.includes(a.id)).map((a) => rgb(a.color));
  if (!from.length) return [];
  const centre = [0, 1, 2].map((i) => from.reduce((s, c) => s + c[i]!, 0) / from.length);
  return assets
    .filter((a) => !a.trashed && !ids.includes(a.id))
    .map((a) => ({ id: a.id, d: rgb(a.color).reduce((s, v, i) => s + (v - centre[i]!) ** 2, 0) }))
    .sort((a, b) => a.d - b.d || a.id - b.id)
    .slice(0, limit)
    .map((x) => x.id);
}

function change(kind: string, label: string, ids: number[], fn: (a: MockAsset) => boolean, dirty = true): number {
  const before = new Map<number, Partial<MockAsset>>();
  let n = 0;
  for (const id of ids) {
    const a = byId(id);
    if (!a) continue;
    const snap = { category: a.category, flags: a.flags, rating: a.rating, tags: [...a.tags], trashed: a.trashed, dirty: a.dirty, aspect: a.aspect, local: a.local, utc: a.utc, lat: a.lat, lon: a.lon, review: a.review, immich: a.immich };
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
  pack('faces', 'Faces', 38_941_385, 'huggingface.co', 'MIT (YuNet), Apache-2.0 (SFace)'),
  pack('local-ai', 'Local AI', 2_984_528_827, 'github.com', 'MIT (llama.cpp), Apache-2.0 (Qwen3-VL)'),
];
// `?models=1`: the image model, text reader and faces are installed (Teach, Find similar, People).
if (params.get('models') === '1') {
  for (const p of packs) if (p.id !== 'places' && p.id !== 'local-ai') Object.assign(p, { state: 'installed', done: p.size });
}
const packTimers = new Map<string, ReturnType<typeof setInterval>>();

// Events: quarters with 8 or more photos stand in for detection (the
// mock library spreads its photos thinly over eleven years).
interface MockEvent {
  id: number;
  name: string;
  state: 'proposed' | 'confirmed' | 'dismissed';
  ids: number[];
  own_folder: boolean;
  custom: boolean;
  /** Merged or split by the owner: detection leaves it alone. */
  shaped: boolean;
  immichOff?: boolean;
  share?: string[];
  album?: string;
}
let mockEvents: MockEvent[] = [];
let eventSeq = 0;
const MONTH_NAMES = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

function detectEvents() {
  const held = new Set(mockEvents.filter((e) => e.state !== 'proposed' || e.shaped).flatMap((e) => e.ids));
  const byMonth = new Map<string, MockAsset[]>();
  for (const a of assets) {
    if (a.trashed || a.category !== 'photos' || held.has(a.id)) continue;
    const k = `${a.local.slice(0, 4)}-${Math.floor((Number(a.local.slice(5, 7)) - 1) / 3)}`;
    byMonth.set(k, [...(byMonth.get(k) ?? []), a]);
  }
  const old = new Map(mockEvents.filter((e) => e.state === 'proposed' && !e.shaped).map((e) => [e.ids[0], e]));
  const next: MockEvent[] = mockEvents.filter((e) => e.state !== 'proposed' || e.shaped);
  for (const [k, list] of byMonth) {
    if (list.length < 8) continue;
    list.sort((a, b) => a.utc - b.utc);
    const located = list.find((a) => a.lat != null);
    const place = located && placesInstalled ? ` ${nearest(located.lat!, located.lon!).name}` : '';
    const prev = old.get(list[0]!.id);
    next.push({
      id: prev?.id ?? ++eventSeq,
      name: prev?.custom ? prev.name : `${MONTH_NAMES[Number(list[0]!.local.slice(5, 7)) - 1]} ${k.slice(0, 4)}${place}`,
      state: 'proposed',
      ids: list.map((a) => a.id),
      own_folder: true,
      custom: prev?.custom ?? false,
      shaped: false,
    });
  }
  mockEvents = next;
}

function eventInfo(e: MockEvent): EventInfo {
  const items = e.ids.map((id) => byId(id)).filter((a): a is MockAsset => !!a);
  const step = Math.max(1, Math.floor(items.length / 8));
  return {
    id: e.id,
    name: e.name,
    state: e.state,
    start_local: items[0]?.local ?? '',
    end_local: items[items.length - 1]?.local ?? '',
    count: items.length,
    place: null,
    away: (() => {
      const located = items.filter((a) => a.lat != null);
      const far = located.filter((a) => Math.hypot(a.lat! - 47.37, (a.lon! - 8.54) * 0.68) > 0.45);
      return located.length > 0 && far.length * 2 > located.length;
    })(),
    own_folder: e.own_folder,
    samples: items
      .filter((_, i) => i % step === 0)
      .slice(0, 8)
      .map((a) => a.id),
  };
}

function retagEvent(ids: number[], remove: string | null, add: string | null) {
  for (const id of ids) {
    const a = byId(id);
    if (!a) continue;
    if (remove) a.tags = a.tags.filter((t) => t !== `Event/${remove}`);
    if (add && !a.tags.includes(`Event/${add}`)) a.tags.push(`Event/${add}`);
  }
  emit('changed', 'assets');
}

const eventById = (id: number) => {
  const e = mockEvents.find((x) => x.id === id);
  if (!e) throw new Error('This event is gone. Open Events again.');
  return e;
};
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
    const k = `${a.local.slice(0, 4)}-${Math.floor((Number(a.local.slice(5, 7)) - 1) / 3)}`;
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
    case 'amount':
    case 'smart': {
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
const listeners: { [K in keyof Events]: Set<Events[K]> } = { jobs: new Set(), changed: new Set(), scanned: new Set(), drop: new Set(), card: new Set() };

// Add photos: a simulated camera card. The clock is 9 h 30 min behind, it
// has no GPS, and two of its photos are already in the library.
interface MockImport {
  id: number;
  root: number;
  source: string;
  state: 'checking' | 'applied';
  name: string | null;
  added: number[];
  datesOk: boolean;
  placesOk: boolean;
  leftOut: number[];
  batch: number | null;
  orig: Map<number, string>;
}
let imports: MockImport[] = [];
let staged: MockAsset[] = [];
let importSeq = 0;
const importGroups = new Map<string, PlaceGroup>();
const customUndo = new Map<number, () => void>();
/** What phone photos of the same moments suggest, in seconds. */
const CARD_OFFSET = 9 * 3600 + 28 * 60 + 5;
const MONTHS_LONG = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

function makeCard(root: number): MockAsset[] {
  const r = rng(7 + root);
  const out: MockAsset[] = [];
  let t = Date.UTC(2022, 4, 19, 22, 37, 35);
  for (let k = 0; k < 42; k++) {
    t += k === 20 ? 9 * 3600_000 : Math.floor(60_000 + r() * 900_000);
    const local = fmtLocal(t);
    const hue = Math.floor(r() * 24) * 15;
    const [rr, gg, bb] = hslToRgb(hue / 360, 0.5, 0.5);
    out.push({
      id: 900_000 + root * 1000 + k,
      local,
      utc: t - 2 * 3600_000,
      aspect: 3 / 2,
      color: (rr << 16) | (gg << 8) | bb,
      flags: 0,
      name: `IMG_${String(k + 1).padStart(4, '0')}.JPG`,
      dir: 'DCIM/100CANON',
      category: 'photos',
      camera: 'Canon EOS R6',
      lat: null,
      lon: null,
      duration: null,
      rating: 0,
      tags: [],
      trashed: false,
      dirty: false,
      review: 2,
      conf: 0.98,
      staged: root,
    });
  }
  return out;
}

const stagedOf = (imp: MockImport) => staged.filter((a) => a.staged === imp.root);

// Immich: what goes there follows the item choice, the category default
// (Photos) and confirmed events.
const IMMICH_USERS = [
  { id: 'u1', name: 'Alex', email: 'alex@example.com' },
  { id: 'u2', name: 'Sam', email: 'sam@example.com' },
];
let immichSeq = 0;
function immichSelected(a: MockAsset) {
  if (a.trashed || a.staged != null) return false;
  if (a.immich === 'send') return true;
  if (a.immich === 'local') return false;
  if (mockEvents.some((e) => e.state === 'confirmed' && !e.immichOff && e.ids.includes(a.id))) return true;
  return a.category === 'photos';
}
const immichReady = () => settings.network.immich && !!settings.immich_url && secrets.has('immich');
const importById = (id: number) => {
  const imp = imports.find((i) => i.id === id);
  if (!imp) throw new Error('This import is gone. Open Add photos again.');
  return imp;
};

function diffLabel(secs: number) {
  if (Math.abs(secs) < 1) return 'no change';
  const s = Math.abs(secs);
  const parts = [[Math.floor(s / 86400), 'd'], [Math.floor((s % 86400) / 3600), 'h'], [Math.floor((s % 3600) / 60), 'min'], [s % 60, 's']]
    .filter(([v]) => (v as number) > 0)
    .map(([v, u]) => `${v} ${u}`);
  return `${secs < 0 ? '−' : '+'}${parts.join(' ')}`;
}

function importInfo(imp: MockImport): ImportInfo {
  const list = stagedOf(imp).sort((a, b) => (a.local < b.local ? -1 : 1));
  const exact = list.slice(-2).map((a) => a.id);
  const first = list[0];
  const shiftedBy = first ? Math.round((parseLocal(first.local) - parseLocal(imp.orig.get(first.id) ?? first.local)) / 1000) : 0;
  const remaining = CARD_OFFSET - shiftedBy;
  const located = list.filter((a) => a.lat != null);
  const place = located.length && placesInstalled ? 'Lisbon' : null;
  const proposed = first ? `${MONTHS_LONG[Number(first.local.slice(5, 7)) - 1]} ${first.local.slice(0, 4)}${place ? ` ${place}` : ''}` : 'New photos';
  const phones = assets.filter((a) => a.category === 'photos').slice(0, 4);
  return {
    id: imp.id,
    root_id: imp.root,
    source: imp.source,
    state: imp.state,
    kind: 'folder',
    facts: [],
    name: imp.name ?? proposed,
    custom_name: imp.name != null,
    count: list.length,
    photos: list.length,
    videos: 0,
    from_local: first?.local ?? '',
    to_local: list[list.length - 1]?.local ?? '',
    cameras: [{ label: 'Canon EOS R6', count: list.length }],
    formats: [{ label: 'JPEG', count: list.length }],
    exact,
    similar: [],
    twins: [],
    left_out: [...imp.leftOut],
    pending: 0,
    date_sets: [
      {
        key: `camera:Canon EOS R6:${first?.local.slice(0, 10) ?? ''}`,
        kind: 'camera',
        label: 'Canon EOS R6',
        count: list.length,
        from_local: first?.local ?? '',
        to_local: list[list.length - 1]?.local ?? '',
        reasons: Math.abs(remaining) > 120 ? ['Phone photos from the same days are in UTC+1; this camera states no time zone and is read as UTC+2.'] : [],
        has_offset: false,
        ids: list.map((a) => a.id),
      },
    ],
    smart: phones.length
      ? {
          seconds: remaining,
          label: diffLabel(remaining),
          offset_min: 60,
          matched: 29,
          compared: list.length,
          pairs: list.slice(0, 4).map((a, i) => ({
            item: a.id,
            phone: phones[i]!.id,
            item_local: a.local,
            phone_local: fmtLocal(parseLocal(a.local) + remaining * 1000),
          })),
        }
      : null,
    no_date: 0,
    no_place: list.length - located.length,
    place,
    categories: [{ label: 'Photos', count: list.length }],
    dates_ok: imp.datesOk,
    places_ok: imp.placesOk,
    ids: list.map((a) => a.id),
    added: imp.added,
    batch_id: imp.batch,
  };
}

function importPlan(imp: MockImport, skipKnown: boolean): ImportPlan {
  const info = importInfo(imp);
  const skip = new Set([...imp.leftOut, ...(skipKnown ? info.exact : [])]);
  const bring = info.ids.filter((id) => !skip.has(id));
  const start = byId(bring[0] ?? 0)?.local ?? info.from_local;
  return {
    root_id: roots[0]?.id ?? 1,
    root_path: roots[0]?.path ?? '',
    event: info.name,
    files: bring.length,
    sidecars: 0,
    skipped: skip.size,
    bytes: bring.length * 6_200_000,
    free: 512_000_000_000,
    folders: [{ root_id: roots[0]?.id ?? 1, path: `${start.slice(0, 4)}/${start.slice(0, 7)} ${info.name}`, count: bring.length, samples: bring.slice(0, 12), new: true }],
    writes: true,
  };
}

function emit<K extends keyof Events>(k: K, ...args: Parameters<Events[K]>) {
  for (const cb of listeners[k]) (cb as (...a: unknown[]) => void)(...args);
}

function filter(q: Query): MockAsset[] {
  const base = q.staged != null ? staged.filter((a) => a.staged === q.staged).sort((a, b) => (a.local < b.local ? 1 : -1)) : assets;
  return base.filter((a) => {
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
    if (q.event_id != null && !mockEvents.find((e) => e.id === q.event_id)?.ids.includes(a.id)) return false;
    if (q.text && !a.name.toLowerCase().includes(q.text.toLowerCase())) return false;
    if (q.people?.length) {
      ensurePeople();
      if (!q.people.every((p) => faces.some((f) => f.asset === a.id && f.person === p))) return false;
    }
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
  return index.get(id) ?? (id >= 900_000 ? staged.find((a) => a.id === id) : undefined);
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

// Decision models: a simulated OpenRouter catalogue, a local Ollama when
// the mode is local, and runs that answer 64 items per tick.

const perItem = (inM: number, outM: number) => (380 * inM + 35 * outM) / 1e6;
const OFFERS: ModelOffer[] = (
  [
    ['google/gemini-2.5-flash', 'Gemini 2.5 Flash', 'Google Vertex', 0.3, 2.5, true, true, 'Best', 'Gemini 2.5 Flash; strong multimodal scores in Google\'s model card'],
    ['google/gemma-4-26b-a4b-it', 'Gemma 4 26B A4B', 'Io Net', 0.048, 0.21, true, true, 'Best', 'Gemma 4 26B A4B; MMMU-Pro 73.8 (Gemma 4 model card)'],
    ['google/gemini-2.5-flash-lite', 'Gemini 2.5 Flash Lite', 'Google Vertex', 0.1, 0.4, true, true, 'Very good', "Gemini 2.5 Flash-Lite; Google's smallest 2.5 model"],
    ['google/gemma-3-12b-it', 'Gemma 3 12B', 'DeepInfra', 0.05, 0.15, true, true, 'Good', 'Gemma 3 12B; MMMU 59.6 (Gemma 3 model card)'],
    ['inclusionai/ling-3.0-flash-vl', 'Ling 3.0 Flash VL', 'Novita', 0.021, 0.062, true, true, 'Not rated', null],
    ['example/keeps-data-vl', 'Example VL', null, 0.01, 0.03, false, false, 'Not rated', null],
  ] as const
).map(([id, name, provider, inM, outM, zdr, structured, quality, basis], k) => ({
  id,
  name,
  provider,
  prompt_per_m: inM,
  completion_per_m: outM,
  image_usd: 0,
  zdr,
  structured,
  per_100k_usd: perItem(inM, outM) * 100_000,
  quality,
  basis,
  measured: null,
  best: k === 0,
  data: zdr ? 'Keeps no data: zero data retention' : "May keep requests for a while; Roll only routes to providers that say they don't collect or train on data",
  country: provider ? 'US' : null,
  policy_url: provider ? 'https://example.com/privacy' : null,
}));
const LOCAL_MODELS: LocalModelInfo[] = (
  [
    ['qwen3-vl:8b-instruct', false, 6.1, 8, 'Very good', 'MMMU 69.6 as reported from the model card', true],
    ['gemma4:e4b', true, 6.6, 8, 'Very good', 'MMMU-Pro 52.6 (Gemma 4 model card)', true],
    ['qwen3-vl:4b-instruct', true, 3.3, 5, 'Good', 'MMMU 67.4 as reported from the model card', true],
    ['qwen3-vl:2b-instruct', false, 1.9, 3, 'Basic', 'Qwen3-VL 2B, the smallest', true],
    ['gemma4:26b', false, 16, 19, 'Best', 'MMMU-Pro 73.8 (Gemma 4 model card)', false],
  ] as const
).map(([name, installed, download_gb, memory_gb, quality, basis, fits], k) => ({
  name,
  installed,
  download_gb,
  memory_gb,
  quality,
  basis,
  measured: null,
  fits,
  best: k === 0,
}));
const pickedOffers = new Map<string, ModelOffer>();
interface MockRun extends RunInfo {
  ids: number[];
  timer?: ReturnType<typeof setTimeout>;
  jobId: number;
}
let aiRunList: MockRun[] = [];
let aiSeq = 0;
let checkItemsList: CheckItem[] = [];

function aiProblem(model?: string): string | null {
  const s = settings;
  if (s.ai_mode === 'off') return 'AI is off. Choose Local or OpenRouter in Settings › AI.';
  if (s.ai_mode === 'openrouter' && !secrets.has('openrouter')) return 'Add an OpenRouter API key in Settings › AI.';
  if (s.ai_mode === 'openrouter' && !s.network.ai) return 'The AI provider is off in Settings › Network.';
  if (!(model ?? s.ai_decision_model)) return 'Pick a decision model.';
  return null;
}

function aiPrice(model: string): number {
  if (settings.ai_mode !== 'openrouter') return 0;
  const o = pickedOffers.get(model) ?? OFFERS.find((x) => x.id === model);
  return o ? perItem(o.prompt_per_m, o.completion_per_m) : 0;
}

const uncertain = () => assets.filter((a) => !a.trashed && !a.staged && a.review < 2 && a.conf < 0.95 && !(a.tags.includes('Private') && settings.ai_mode === 'openrouter' && !settings.ai_allow_private_cloud));

function startMockRun(kind: string, model: string, ids: number[], budget: number | null): MockRun {
  const price = aiPrice(model);
  const cloud = settings.ai_mode === 'openrouter';
  const run: MockRun = {
    id: ++aiSeq,
    kind,
    provider: cloud ? 'openrouter' : 'http://127.0.0.1:11434/v1',
    model,
    state: 'running',
    items: ids.length,
    done: 0,
    failed: 0,
    unsure: 0,
    proposed: 0,
    estimate_usd: price * ids.length,
    budget_usd: cloud ? budget : null,
    spent_usd: 0,
    seconds: 0,
    note: null,
    created_at: Date.now(),
    ids,
    jobId: 1000 + aiSeq,
  };
  aiRunList.unshift(run);
  continueMockRun(run);
  return run;
}

function continueMockRun(run: MockRun) {
  const price = aiPrice(run.model);
  const job: JobInfo = { id: run.jobId, kind: 'decide', label: 'Sorting uncertain items with AI', state: 'running', done: run.done, total: run.items, rate: 400, eta_secs: null, cost_usd: run.spent_usd, error: null };
  jobs = [job, ...jobs.filter((j) => j.id !== run.jobId)];
  run.state = 'running';
  const step = () => {
    // Reserve a batch's worst case (about three items' worth) before sending.
    const batch = run.ids.slice(run.done, run.done + 64);
    let n = 0;
    for (const id of batch) {
      if (run.budget_usd != null && run.spent_usd + price * 3 > run.budget_usd) break;
      run.spent_usd += price * (0.85 + ((id * 37) % 30) / 100);
      run.done++;
      n++;
      const a = byId(id);
      if (!a) continue;
      if (id % 10 === 9) {
        run.unsure++;
        continue;
      }
      run.proposed++;
      if (run.kind === 'decide') {
        a.review = 1;
        a.conf = 0.86 + (id % 13) / 100;
      }
    }
    run.seconds += 0.15;
    job.done = run.done;
    job.cost_usd = run.spent_usd;
    const capped = n < batch.length;
    if (run.done >= run.items || capped) {
      run.state = capped ? 'capped' : 'done';
      job.state = capped ? 'failed' : 'done';
      job.error = capped ? 'The run stopped at its budget. Raise the budget and start it again to continue.' : null;
      assets = [...assets];
      emit('changed', 'assets');
    } else run.timer = setTimeout(step, 150);
    emit('jobs', [...jobs]);
    emit('changed', 'ai');
  };
  run.timer = setTimeout(step, 150);
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
      case 'immich':
        return change(
          'edit',
          action.mode === 'send' ? `Send ${items} to Immich` : action.mode === 'local' ? `Keep ${items} local only` : `Use the default Immich choice for ${items}`,
          ids,
          (a) => {
            if ((a.immich ?? 'default') === action.mode) return false;
            a.immich = action.mode;
            return true;
          },
          false,
        );
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
    customUndo.get(batch)?.();
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
  // The NIKON set runs a year and two hours ahead; photos from the same
  // weeks stand in for the phone photos of the same moments.
  dateSmart: async (ids) => {
    const set = mockCameraSet().filter((a) => ids.includes(a.id));
    if (set.length < 3 || !set.every((a) => a.local.startsWith('2020'))) return null;
    const seconds = -(366 * 24 + 2) * 3600;
    const others = assets.filter((a) => a.category === 'photos' && !ids.includes(a.id)).slice(0, 4);
    return {
      seconds,
      label: diffLabel(seconds),
      offset_min: 120,
      matched: Math.min(set.length, 18),
      compared: set.length,
      pairs: set.slice(0, others.length).map((a, i) => ({
        item: a.id,
        phone: others[i]!.id,
        item_local: a.local,
        phone_local: fmtLocal(parseLocal(a.local) + seconds * 1000),
      })),
    };
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
  events: async () => {
    detectEvents();
    return mockEvents
      .filter((e) => e.state !== 'dismissed')
      .map(eventInfo)
      .sort((a, b) => (a.start_local < b.start_local ? 1 : -1));
  },
  eventConfirm: async (id, name) => {
    const e = eventById(id);
    if (name) {
      e.name = name;
      e.custom = true;
    }
    e.state = 'confirmed';
    retagEvent(e.ids, null, e.name);
  },
  eventRename: async (id, name) => {
    const e = eventById(id);
    if (e.state === 'confirmed') retagEvent(e.ids, e.name, name);
    e.name = name;
    e.custom = true;
    emit('changed', 'assets');
  },
  eventDismiss: async (id) => {
    const e = eventById(id);
    if (e.state === 'confirmed') retagEvent(e.ids, e.name, null);
    e.state = 'dismissed';
    emit('changed', 'assets');
  },
  eventMerge: async (ids) => {
    const list = ids.map(eventById).sort((a, b) => (byId(a.ids[0]!)?.utc ?? 0) - (byId(b.ids[0]!)?.utc ?? 0));
    const [first, ...rest] = list;
    if (!first) return;
    const confirmed = list.find((e) => e.state === 'confirmed');
    for (const e of rest) {
      if (e.state === 'confirmed') retagEvent(e.ids, e.name, null);
      first.ids.push(...e.ids);
    }
    mockEvents = mockEvents.filter((e) => !rest.includes(e));
    first.ids.sort((a, b) => (byId(a)?.utc ?? 0) - (byId(b)?.utc ?? 0));
    if (confirmed) {
      first.name = confirmed.name;
      first.state = 'confirmed';
      retagEvent(first.ids, null, first.name);
    }
    first.custom = true;
    first.shaped = true;
    emit('changed', 'assets');
  },
  eventSplit: async (id, at, name) => {
    const e = eventById(id);
    const pos = e.ids.indexOf(at);
    if (pos <= 0) return false;
    const later = e.ids.splice(pos);
    e.shaped = true;
    mockEvents.push({ id: ++eventSeq, name, state: e.state, ids: later, own_folder: e.own_folder, custom: true, shaped: true });
    if (e.state === 'confirmed') retagEvent(later, e.name, name);
    emit('changed', 'assets');
    return true;
  },
  eventOwnFolder: async (id, on) => {
    eventById(id).own_folder = on;
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
  // A stand-in for the text model: a few words it "understands".
  openLink: async (url) => void window.open(url, '_blank', 'noopener'),
  bestMatches: async (q) => {
    const words = (q.text ?? '').trim();
    if (!words) return { ids: [], scores: [], cost_usd: 0, words };
    const ids = filter(q).slice(0, 50).map((a) => a.id);
    return { ids, scores: ids.map(() => null), cost_usd: 0, words };
  },
  rerank: async (q) => {
    const problem = aiProblem();
    if (problem) throw new Error(problem);
    const m = await mockApi.bestMatches(q);
    // The model's stand-in: brighter pictures match better.
    const score = (id: number) => {
      const c = byId(id)?.color ?? 0;
      return Math.round((((c >> 16) & 255) + ((c >> 8) & 255) + (c & 255)) / 76.5);
    };
    const ids = [...m.ids].sort((a, b) => score(b) - score(a) || m.ids.indexOf(a) - m.ids.indexOf(b));
    return { ...m, ids, scores: ids.map(score), cost_usd: settings.ai_mode === 'openrouter' ? 0.0008 : 0 };
  },
  aiSearch: async (text) => {
    const problem = aiProblem();
    if (problem) throw new Error(problem);
    const query = { ...emptyQuery() };
    const understood: string[] = [];
    const rest: string[] = [];
    const year = new Date().getFullYear() - 1;
    for (const w of text.toLowerCase().split(/\s+/).filter(Boolean)) {
      if (w === 'christmas') {
        query.from_local = `${year}-12-24T00:00:00`;
        query.to_local = `${year}-12-27T00:00:00`;
        understood.push(`From ${year}-12-24 to before ${year}-12-27`);
      } else if (w === 'clips' || w === 'footage') {
        query.media = ['video'];
        understood.push('Videos');
      } else if (w === 'funny') {
        query.categories = ['memes'];
        understood.push('Memes');
      } else if (!['the', 'at', 'of', 'from', 'last'].includes(w)) rest.push(w);
    }
    return { query, rest: rest.join(' '), understood };
  },
  aiStatus: async () => ({
    mode: settings.ai_mode,
    local: settings.ai_mode === 'local' ? [{ name: 'Ollama', base: 'http://127.0.0.1:11434/v1', models: LOCAL_MODELS }] : [],
    gpu_memory_gb: 11,
    endpoint: settings.ai_mode === 'local' ? (settings.ai_local_endpoint ?? 'http://127.0.0.1:11434/v1') : null,
    model: settings.ai_decision_model,
    has_key: secrets.has('openrouter'),
    network_on: settings.network.ai,
    problem: aiProblem(),
  }),
  aiOffers: async () => {
    if (!settings.network.ai) throw new Error('The AI provider is off. Turn it on in Settings › Network.');
    return OFFERS;
  },
  aiPick: async (model, offer) => {
    if (offer) pickedOffers.set(model, offer);
    settings = { ...settings, ai_decision_model: model };
    emit('changed', 'settings');
    emit('changed', 'ai');
  },
  aiPlan: async () => {
    const problem = aiProblem();
    if (problem) throw new Error(problem);
    const model = settings.ai_decision_model!;
    const cloud = settings.ai_mode === 'openrouter';
    const done = new Set(aiRunList.filter((r) => r.kind === 'decide' && r.model === model).flatMap((r) => r.ids.slice(0, r.done)));
    const all = assets.filter((a) => !a.trashed && !a.staged && a.review < 2 && a.conf < 0.95);
    const items = uncertain().filter((a) => !done.has(a.id));
    const price = aiPrice(model);
    const offer = pickedOffers.get(model) ?? OFFERS.find((o) => o.id === model);
    const n = items.length.toLocaleString('en-US');
    return {
      provider: cloud ? 'OpenRouter' : 'Ollama',
      host: cloud ? 'openrouter.ai' : '127.0.0.1:11434',
      model,
      cloud,
      items: items.length,
      private_skipped: all.length - uncertain().length,
      already_done: all.filter((a) => done.has(a.id)).length,
      estimate_usd: price * items.length,
      per_100k_usd: price * 100_000,
      budget_usd: cloud ? settings.ai_budget_usd : null,
      batch: settings.ai_batch,
      measured: false,
      consent: cloud
        ? `${n} thumbnails (512 px, no metadata) will be sent to ${offer?.provider ?? 'a provider that keeps no data,'} through OpenRouter. Estimated cost $${(price * items.length).toFixed(2)}.`
        : `${n} thumbnails go to Ollama on this computer (127.0.0.1:11434). Nothing leaves the machine.`,
      samples: items.slice(0, 12).map((a) => a.id),
    };
  },
  aiStart: async (budget) => {
    const problem = aiProblem();
    if (problem) throw new Error(problem);
    const model = settings.ai_decision_model!;
    const done = new Set(aiRunList.filter((r) => r.kind === 'decide' && r.model === model).flatMap((r) => r.ids.slice(0, r.done)));
    const ids = uncertain()
      .filter((a) => !done.has(a.id))
      .map((a) => a.id);
    if (!ids.length) throw new Error('Nothing is uncertain: every item is decided or proposed with 95% or more.');
    return startMockRun('decide', model, ids, budget ?? settings.ai_budget_usd).id;
  },
  aiResume: async (id, budget) => {
    const run = aiRunList.find((r) => r.id === id);
    if (!run) throw new Error('Unknown run');
    if (budget != null) run.budget_usd = budget;
    continueMockRun(run);
  },
  aiRuns: async () => aiRunList.map(({ ids: _ids, timer: _t, jobId: _j, ...r }) => r),
  aiCheckSample: async () => {
    const byCat = new Map<string, MockAsset[]>();
    for (const a of assets.filter((x) => !x.trashed && !x.staged)) byCat.set(a.category, [...(byCat.get(a.category) ?? []), a]);
    const total = Math.max(1, assets.length);
    checkItemsList = [];
    for (const [cat, list] of [...byCat.entries()].sort()) {
      const want = Math.min(list.length, Math.max(5, Math.round((list.length / total) * 150)));
      for (const a of list.slice(0, want)) checkItemsList.push({ id: a.id, truth: a.review === 3 ? cat : null, proposed: cat });
    }
    emit('changed', 'ai');
    return checkItemsList.length;
  },
  aiCheckItems: async () => [...checkItemsList].sort((a, b) => Number(a.truth != null) - Number(b.truth != null)),
  aiCheckLabel: async (id, key) => {
    const it = checkItemsList.find((x) => x.id === id);
    if (it) it.truth = key;
  },
  aiCheckStart: async (models) => {
    if (!models.length || models.length > 3) throw new Error('Pick one to three models.');
    const ids = checkItemsList.filter((x) => x.truth).map((x) => x.id);
    if (ids.length < 20) throw new Error('Label at least 20 items first.');
    for (const m of models) startMockRun('check', m, ids, settings.ai_budget_usd);
    emit('changed', 'ai');
  },
  aiCheckReport: async () => {
    const labeled = checkItemsList.filter((x) => x.truth);
    const local = labeled.filter((x) => (byId(x.id)?.review ?? 0) >= 1 && (byId(x.id)?.review ?? 0) < 3);
    const latest = new Map<string, MockRun>();
    for (const r of aiRunList.filter((r) => r.kind === 'check').reverse()) latest.set(r.model, r);
    // Simulated accuracy: larger and pricier models are a little better.
    const skill = (m: string) => 0.8 + (aiPrice(m) * 1e6) / 400 + (m.length % 5) / 100;
    const models: CheckModel[] = [...latest.values()].map((r) => {
      const answered = r.done;
      const correct = Math.round(answered * Math.min(0.98, skill(r.model)));
      return {
        run: r.id,
        provider: r.provider,
        model: r.model,
        state: r.state,
        batch: settings.ai_batch,
        answered,
        correct,
        unsure: r.unsure,
        accuracy: correct / Math.max(1, answered),
        per_100k_usd: (r.spent_usd / Math.max(1, answered)) * 100_000,
        items_per_sec: answered / Math.max(0.15, r.seconds),
        recommended: false,
      };
    });
    const best = Math.max(0, ...models.map((m) => m.accuracy));
    const pick = models.filter((m) => m.answered && m.accuracy >= best - 0.02).sort((a, b) => a.per_100k_usd - b.per_100k_usd)[0];
    if (pick) pick.recommended = true;
    return {
      sample: checkItemsList.length,
      labeled: labeled.length,
      local_coverage: local.length / Math.max(1, labeled.length),
      local_accuracy: local.length ? local.filter((x) => byId(x.id)?.category === x.truth).length / local.length : 0,
      models,
    };
  },
  immichStatus: async () => ({
    on: settings.network.immich,
    url: settings.immich_url,
    has_key: secrets.has('immich'),
    server: immichReady() ? '3.3.1' : null,
    user: immichReady() ? 'You' : null,
    error: null,
  }),
  immichPlan: async () => {
    const want = assets.filter(immichSelected);
    const up = want.filter((a) => !a.immichId);
    const gone = assets.filter((a) => a.immichId && !immichSelected(a));
    const evs = mockEvents.filter((e) => e.state === 'confirmed' && !e.immichOff && e.ids.some((id) => byId(id) && immichSelected(byId(id)!)));
    return {
      uploads: up.length,
      upload_bytes: up.length * 3_400_000,
      updates: want.filter((a) => a.immichId && a.dirty).length,
      removes: gone.length,
      new_albums: evs.filter((e) => !e.album).length,
      album_updates: evs.filter((e) => e.album).length,
      upload_samples: up.slice(0, 12).map((a) => a.id),
      remove_samples: gone.slice(0, 12).map((a) => a.id),
      in_sync: want.filter((a) => a.immichId && !a.dirty).length,
    };
  },
  immichSync: async (remove) => {
    if (!immichReady()) throw new Error('Immich is off. Turn it on in Settings › Network.');
    let sent = 0;
    let removed = 0;
    for (const a of assets) {
      if (immichSelected(a) && !a.immichId) {
        a.immichId = `im${++immichSeq}`;
        a.flags |= Flag.IMMICH;
        sent++;
      } else if (remove && a.immichId && !immichSelected(a)) {
        a.immichId = undefined;
        a.flags &= ~Flag.IMMICH;
        removed++;
      }
    }
    let albums = 0;
    for (const e of mockEvents.filter((x) => x.state === 'confirmed' && !x.immichOff)) {
      e.album ??= `album${++immichSeq}`;
      albums++;
    }
    const parts = [sent && `sent ${sent}`, removed && `moved ${removed} to Immich's trash`, albums && `${albums} album${albums === 1 ? '' : 's'}`].filter(Boolean);
    pushBatch(++batchSeq, 'immich', parts.length ? `Immich: ${parts.join(', ')}` : 'Immich: nothing to change', sent + removed);
    assets = [...assets];
    emit('changed', 'assets');
    emit('changed', 'history');
    emit('changed', 'immich');
  },
  immichUsers: async () => (immichReady() ? IMMICH_USERS : []),
  immichAlbums: async () =>
    mockEvents
      .filter((e) => e.state === 'confirmed')
      .map((e) => ({
        event_id: e.id,
        name: e.name,
        send: !e.immichOff,
        items: e.ids.filter((id) => byId(id) && immichSelected(byId(id)!)).length,
        album_id: e.album ?? null,
        share: e.share ?? [],
      })),
  immichAlbum: async (event, send, share) => {
    const e = eventById(event);
    e.immichOff = !send;
    e.share = share;
    emit('changed', 'immich');
  },
  saveImmichScript: async () => true,
  importStart: async (path) => {
    const source = path ?? 'E:\\';
    const open = imports.find((i) => i.source === source);
    if (open) return open.id;
    const id = ++importSeq;
    const root = 100 + id;
    const card = makeCard(root);
    staged = [...staged, ...card];
    imports = [{ id, root, source, state: 'checking', name: null, added: [], datesOk: false, placesOk: false, leftOut: [], batch: null, orig: new Map(card.map((a) => [a.id, a.local])) }, ...imports];
    emit('changed', 'imports');
    return id;
  },
  imports: async () => imports.map(importInfo),
  importInfo: async (id) => importInfo(importById(id)),
  importName: async (id, name) => {
    importById(id).name = name?.trim() || null;
    emit('changed', 'imports');
  },
  importMark: async (id, datesOk, placesOk) => {
    const imp = importById(id);
    if (datesOk != null) imp.datesOk = datesOk;
    if (placesOk != null) imp.placesOk = placesOk;
    emit('changed', 'imports');
  },
  importLeaveOut: async (id, ids, out) => {
    const imp = importById(id);
    imp.leftOut = out ? [...new Set([...imp.leftOut, ...ids])] : imp.leftOut.filter((x) => !ids.includes(x));
    emit('changed', 'imports');
  },
  importPlaces: async (id) => {
    const imp = importById(id);
    const missing = stagedOf(imp).filter((a) => a.lat == null);
    if (!missing.length) return [];
    const sorted = missing.sort((a, b) => (a.local < b.local ? -1 : 1));
    const g: PlaceGroup = {
      key: `import${imp.root}:neighbors::${sorted[0]!.local.slice(0, 10)}:${sorted[sorted.length - 1]!.local.slice(0, 10)}`,
      source: 'neighbors',
      evidence: 'your geotagged photos taken around the same time',
      place: 'Lisbon, Lisbon, Portugal',
      count: missing.length,
      from_local: sorted[0]!.local,
      to_local: sorted[sorted.length - 1]!.local,
      lat: 38.7167,
      lon: -9.1333,
      accuracy_m: 40,
      ids: missing.map((a) => a.id),
    };
    importGroups.set(g.key, g);
    return [g];
  },
  importPlan: async (id, opts) => importPlan(importById(id), opts.skip_known),
  importApply: async (id, opts) => {
    const imp = importById(id);
    if (!imp.datesOk) throw new Error('Check the dates first.');
    if (!imp.placesOk) throw new Error('Check the places first.');
    const plan = importPlan(imp, opts.skip_known);
    const info = importInfo(imp);
    const skip = new Set([...imp.leftOut, ...(opts.skip_known ? info.exact : [])]);
    const bring = new Set(info.ids.filter((x) => !skip.has(x)));
    if (!bring.size) throw new Error('Nothing to add: every item is left out or already in the archive.');
    const moved = stagedOf(imp).filter((a) => bring.has(a.id));
    const name = plan.event;
    const dir = plan.folders[0]!.path;
    const before = new Map(moved.map((a) => [a.id, { dir: a.dir, tags: [...a.tags], dirty: a.dirty }]));
    for (const a of moved) {
      a.staged = undefined;
      a.dirty = false;
      a.dir = dir;
      a.tags = [...a.tags, `Event/${name}`];
    }
    staged = staged.filter((a) => !bring.has(a.id));
    assets = [...assets, ...moved].sort((a, b) => (a.local < b.local ? 1 : a.local > b.local ? -1 : b.id - a.id));
    const ev: MockEvent = {
      id: ++eventSeq,
      name,
      state: 'confirmed',
      ids: moved.map((a) => a.id),
      own_folder: true,
      custom: true,
      shaped: true,
      immichOff: !opts.immich_album,
      share: [...opts.immich_share],
    };
    mockEvents.push(ev);
    const batch = ++batchSeq;
    pushBatch(batch, 'import', `${opts.mode === 'move' ? 'Move' : 'Copy'} ${plural(moved.length)} into ${name}`, moved.length);
    imp.state = 'applied';
    imp.batch = batch;
    imp.added = moved.map((a) => a.id);
    customUndo.set(batch, () => {
      for (const a of moved) {
        Object.assign(a, before.get(a.id));
        a.staged = imp.root;
      }
      assets = assets.filter((a) => !bring.has(a.id));
      staged = [...staged, ...moved];
      mockEvents = mockEvents.filter((e) => e !== ev);
      imp.state = 'checking';
      imp.batch = null;
      imp.added = [];
      emit('changed', 'imports');
    });
    emit('changed', 'assets');
    emit('changed', 'history');
    emit('changed', 'imports');
    return batch;
  },
  importClose: async (id) => {
    const imp = importById(id);
    staged = staged.filter((a) => a.staged !== imp.root);
    imports = imports.filter((i) => i !== imp);
    emit('changed', 'imports');
  },
  placeGroups: async () => mockGroups(),
  acceptPlaces: async (key) => {
    const g = importGroups.get(key) ?? mockGroups().find((x) => x.key === key);
    importGroups.delete(key);
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
      flags: Uint16Array.from(list.map((a) => (a.flags & (Flag.VIDEO | Flag.FAVORITE)) | (a.category === 'photos' ? Flag.PHOTO : 0))),
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
  mapAreas: async () => mapAreaList,
  mapEstimate: async (bbox) => {
    // Web Mercator tile count from zoom 0 to 13, as the library counts it.
    let tiles = 0;
    for (let z = 0; z <= 13; z++) {
      const n = 2 ** z;
      const x = (lon: number) => Math.min(n - 1, Math.max(0, Math.floor(((lon + 180) / 360) * n)));
      const y = (lat: number) => {
        const r = (Math.max(-85.05, Math.min(85.05, lat)) * Math.PI) / 180;
        return Math.min(n - 1, Math.max(0, Math.floor(((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * n)));
      };
      tiles += (x(bbox[2]) - x(bbox[0]) + 1) * (y(bbox[1]) - y(bbox[3]) + 1);
    }
    return { tiles, bytes: tiles * 50_000, too_big: tiles > 20_000 };
  },
  addMapArea: async (name, bbox) => {
    if (!settings.network.downloads) throw new Error('Downloads are off. Turn them on in Settings › Network.');
    const est = await mockApi.mapEstimate(bbox);
    if (est.too_big) throw new Error('This area is too large to keep offline. Zoom in to a country or smaller.');
    const id = `area-${Date.now()}`;
    mapAreaList = [...mapAreaList, { id, name, bbox, max_zoom: 13, tiles: est.tiles, bytes: est.bytes, state: 'downloading', build: '', error: null }];
    emit('changed', 'maps');
    setTimeout(() => {
      mapAreaList = mapAreaList.map((a) => (a.id === id ? { ...a, state: 'ready', build: '20261015' } : a));
      emit('changed', 'maps');
    }, 1500);
    return id;
  },
  removeMapArea: async (id) => {
    mapAreaList = mapAreaList.filter((a) => a.id !== id);
    emit('changed', 'maps');
  },
  mapTileUrl: () => (params.get('maps') === '1' ? `${location.origin}/__maps/tile/{z}/{x}/{y}` : null),
  mapFontUrl: () => (params.get('maps') === '1' ? `${location.origin}/__maps/font/{fontstack}/{range}.pbf` : null),
  setupLocalAi: async () => {
    const missing = packs.find((p) => p.id === 'local-ai')?.state !== 'installed';
    if (missing && !settings.network.downloads) throw new Error('Downloads are off. Turn them on in Settings › Network.');
    settings = { ...settings, ai_mode: 'local', ai_local_endpoint: 'http://127.0.0.1:8766/v1', ai_decision_model: 'qwen3-vl-4b-instruct' };
    emit('changed', 'settings');
    if (packs.find((p) => p.id === 'local-ai')?.state !== 'installed') await mockApi.downloadPack('local-ai');
  },
  people: async () => {
    ensurePeople();
    const live = liveFaces();
    return people
      .map((p) => {
        const mine = live.filter((f) => f.person === p.id);
        return { id: p.id, name: p.name, hidden: p.hidden, faces: mine.length, items: new Set(mine.map((f) => f.asset)).size, cover: mine[0] ? faceBox(mine[0]) : null };
      })
      .filter((p) => p.faces > 0)
      .sort((a, b) => Number(a.name == null) - Number(b.name == null) || b.faces - a.faces || a.id - b.id);
  },
  personFaces: async (person) => {
    ensurePeople();
    return liveFaces().filter((f) => f.person === person).map(faceBox);
  },
  facesIn: async (asset) => {
    ensurePeople();
    return faces.filter((f) => f.asset === asset).map(faceBox);
  },
  namePerson: async (person, name) => {
    ensurePeople();
    const n = name?.trim() || null;
    peopleChange(n ? `Named ${n}` : 'Took a name away', () => {
      people = people.map((p) => (p.id === person ? { ...p, name: n } : p));
    });
  },
  mergePeople: async (into, others) => {
    ensurePeople();
    const name = people.find((p) => p.id === into)?.name ?? people.find((p) => others.includes(p.id) && p.name)?.name ?? null;
    peopleChange(`Merged ${others.length + 1} people${name ? ` into ${name}` : ''}`, () => {
      faces = faces.map((f) => (f.person != null && others.includes(f.person) ? { ...f, person: into } : f));
      people = people.map((p) => (p.id === into ? { ...p, name } : p));
    });
  },
  notPerson: async (ids) => {
    ensurePeople();
    peopleChange('Not this person', () => {
      faces = faces.map((f) => (ids.includes(f.id) && f.person != null ? { ...f, rejected: [...f.rejected, f.person], person: null } : f));
    });
  },
  assignFaces: async (ids, person, name) => {
    ensurePeople();
    let target = person;
    if (target == null) {
      target = Math.max(0, ...people.map((p) => p.id)) + 1;
      people = [...people, { id: target, name: name?.trim() || null, hidden: false }];
    }
    const label = people.find((p) => p.id === target)?.name;
    peopleChange(label ? `This is ${label}` : 'Grouped faces', () => {
      faces = faces.map((f) => (ids.includes(f.id) ? { ...f, person: target } : f));
    });
  },
  hidePerson: async (person, hidden) => {
    ensurePeople();
    people = people.map((p) => (p.id === person ? { ...p, hidden } : p));
    emit('changed', 'people');
  },
  nameCluster: async (ids) => {
    const shots = ids.filter((id) => byId(id)?.category === 'screenshots').length;
    return shots * 2 > ids.length
      ? { name: 'Screenshots', definition: '', existing: 'screenshots', cost_usd: 0.0002 }
      : { name: 'Concert tickets', definition: 'Photos and screenshots of tickets for concerts and shows.', existing: null, cost_usd: 0.0002 };
  },
  teach: async (key, ids) => {
    const name = CATS.find((c) => c[0] === key)?.[1] ?? key;
    const examples = change('category', `Move ${ids.length} items to ${name}`, ids, (a) => {
      a.category = key;
      a.review = 3;
      return true;
    });
    // Proposals, not decisions: no history entry, as in the app.
    const like = new Set(lookAlike(ids, 40));
    let proposed = 0;
    for (const a of assets) {
      if (!like.has(a.id) || a.review === 3) continue;
      a.category = key;
      a.review = 1;
      a.conf = 0.72;
      proposed++;
    }
    assets = [...assets];
    emit('changed', 'assets');
    emit('changed', 'review');
    return { examples, proposed };
  },
  similarItems: async (ids, limit) => lookAlike(ids, limit),
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
    // Each pair keeps its kind (by its place in the fixed list).
    return dupePairs
      .map((ids, i) => ({ key: `exact:${ids.join(',')}`, kind: i % 2 === 0 ? 'exact' : 'copy', ids, why: i % 2 === 0 ? 'Identical; the first one found.' : 'The original camera file.' }))
      .concat(burstSeries())
      .filter((g) => g.ids.every((id) => !byId(id)?.trashed))
      .filter((g) => !dismissed.has(g.key));
  },
  bestShots: async (ids) => {
    const lum = (id: number) => {
      const c = byId(id)?.color ?? 0;
      return ((c >> 16) & 255) + ((c >> 8) & 255) + (c & 255);
    };
    const max = Math.max(1, ...ids.map(lum));
    const ranked = [...ids].sort((a, b) => lum(b) - lum(a) || a - b);
    return ranked.map((id, i) => ({
      id,
      score: lum(id) / max,
      keep: i === 0,
      notes: i === 0 ? ['Sharpest', 'Best exposed'] : lum(id) < max * 0.5 ? ['Eyes may be closed'] : [],
    }));
  },
  exactPlan: async () => {
    const groups = (await mockApi.dupeGroups()).filter((g) => g.kind === 'exact');
    return {
      groups: groups.length,
      copies: groups.reduce((s, g) => s + g.ids.length - 1, 0),
      bytes: groups.reduce((s, g) => s + (g.ids.length - 1) * 3_400_000, 0),
      samples: groups.slice(0, 8).map((g) => g.ids.map((id, i) => `${byId(id)?.dir ?? ''}/${i ? `Downloads/${byId(id)?.name}` : byId(id)?.name}`)),
    };
  },
  mergeAllExact: async () => {
    const groups = (await mockApi.dupeGroups()).filter((g) => g.kind === 'exact');
    const others = groups.flatMap((g) => g.ids.slice(1));
    return change('edit', `Merge ${others.length} exact copies in ${groups.length} groups`, others, (a) => {
      a.trashed = true;
      return true;
    });
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
  thumbUrl: (id) => byId(id)?.pic ?? svg(byId(id), 320),
  previewUrl: (id) => byId(id)?.pic ?? svg(byId(id), 1024),
  fullUrl: (id) => svg(byId(id), 2048),
  videoUrl: () => '',
  showWindow: async () => undefined,
  setWindowTheme: async () => undefined,
};
