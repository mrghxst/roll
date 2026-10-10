// An in-memory backend for the browser: development without Tauri and the
// Playwright tests. URL parameters: `?mock=N` items (default 2400),
// `?setup=1` to start with a library already added.

import type { Api, Events } from './api';
import type { AssetDetail } from './bindings/AssetDetail';
import type { Category } from './bindings/Category';
import type { FolderNode } from './bindings/FolderNode';
import type { JobInfo } from './bindings/JobInfo';
import type { Query } from './bindings/Query';
import type { RootInfo } from './bindings/RootInfo';
import type { Settings } from './bindings/Settings';
import { Flag, type Timeline } from './timeline';

const params = new URLSearchParams(typeof location !== 'undefined' ? location.search : '');
const N = Number(params.get('mock') ?? 2400);

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
}

const CATS: [string, string][] = [
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
    const hue = Math.floor(r() * 360);
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
      lat: r() > 0.7 ? 45 + r() * 3 : null,
      lon: r() > 0.7 ? 8 + r() * 4 : null,
      duration: video ? Math.floor(r() * 60_000) + 2000 : null,
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
    if (q.from_utc != null && a.utc < q.from_utc) return false;
    if (q.to_utc != null && a.utc >= q.to_utc) return false;
    if (q.location === 'none' && a.lat != null) return false;
    if (q.location === 'any' && a.lat == null) return false;
    if (q.favorite && !(a.flags & Flag.FAVORITE)) return false;
    if (q.cameras.length && !q.cameras.includes(a.camera ?? '')) return false;
    if (q.folder != null && q.folder !== '' && !(q.recursive ? a.dir.startsWith(q.folder) : a.dir === q.folder)) return false;
    if (q.ids && !q.ids.includes(a.id)) return false;
    if (q.text && !a.name.toLowerCase().includes(q.text.toLowerCase())) return false;
    return true;
  });
}

function svg(a: MockAsset | undefined, w: number): string {
  const c = a ? `rgb(${(a.color >> 16) & 255},${(a.color >> 8) & 255},${a.color & 255})` : '#888';
  const h = Math.round(w / (a?.aspect ?? 1));
  const body = `<svg xmlns='http://www.w3.org/2000/svg' width='${w}' height='${h}' viewBox='0 0 ${w} ${h}'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='${c}'/><stop offset='1' stop-color='#222'/></linearGradient></defs><rect width='100%' height='100%' fill='url(#g)'/><circle cx='${w * 0.7}' cy='${h * 0.3}' r='${Math.min(w, h) * 0.12}' fill='rgba(255,255,255,0.7)'/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(body)}`;
}

function byId(id: number) {
  return assets.find((a) => a.id === id);
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
      tags: a.category === 'photos' ? [] : [`Category/${CATS.find((c) => c[0] === a.category)?.[1]}`],
      rating: 0,
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
      trashed: false,
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
      count: assets.filter((a) => a.category === key).length,
    })),
  tagCounts: async () => [],
  counts: async () => ({ total: assets.length, inbox: 0, review: Math.min(48, assets.length), date_issues: 12, no_location: assets.filter((a) => a.lat == null).length, trash: 0, duplicates: 0 }),
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
