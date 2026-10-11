// An in-memory backend for the browser: development without Tauri and the
// Playwright tests. URL parameters: `?mock=N` items (default 2400),
// `?setup=1` to start with a library already added.

import type { Api, Events } from './api';
import type { AssetDetail } from './bindings/AssetDetail';
import type { BatchInfo } from './bindings/BatchInfo';
import type { Category } from './bindings/Category';
import type { FolderNode } from './bindings/FolderNode';
import type { JobInfo } from './bindings/JobInfo';
import type { PlanSummary } from './bindings/PlanSummary';
import type { Query } from './bindings/Query';
import type { RootInfo } from './bindings/RootInfo';
import type { Rule } from './bindings/Rule';
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
  rating: number;
  tags: string[];
  trashed: boolean;
  /** Changed in the library but not yet saved to the file. */
  dirty: boolean;
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
      lat: r() > 0.7 ? 45 + r() * 3 : null,
      lon: r() > 0.7 ? 8 + r() * 4 : null,
      duration: video ? Math.floor(r() * 60_000) + 2000 : null,
      rating: 0,
      tags: [],
      trashed: false,
      dirty: false,
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
    const snap = { category: a.category, flags: a.flags, rating: a.rating, tags: [...a.tags], trashed: a.trashed, dirty: a.dirty, aspect: a.aspect };
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
    review: Math.min(48, assets.length),
    date_issues: 12,
    no_location: assets.filter((a) => a.lat == null).length,
    trash: assets.filter((a) => a.trashed).length,
    duplicates: 0,
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
          if (a.category === k) return false;
          a.category = k;
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
    for (const a of live) {
      if (a.category === 'unsorted') {
        undecided++;
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
