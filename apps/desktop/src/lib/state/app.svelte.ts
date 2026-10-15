import { api, emptyQuery } from '../ipc/api';
import type { AppInfo } from '../ipc/bindings/AppInfo';
import type { Category } from '../ipc/bindings/Category';
import type { Counts } from '../ipc/bindings/Counts';
import type { JobInfo } from '../ipc/bindings/JobInfo';
import type { PackStatus } from '../ipc/bindings/PackStatus';
import type { Query } from '../ipc/bindings/Query';
import type { RootInfo } from '../ipc/bindings/RootInfo';
import type { Settings } from '../ipc/bindings/Settings';
import type { TagCount } from '../ipc/bindings/TagCount';
import type { ZoneProposal } from '../ipc/bindings/ZoneProposal';
import { toast } from './toast.svelte';

export type View =
  | 'timeline'
  | 'folders'
  | 'activity'
  | 'settings'
  | 'inbox'
  | 'review'
  | 'discover'
  | 'duplicates'
  | 'dates'
  | 'places'
  | 'map'
  | 'events'
  | 'add'
  | 'immich'
  | 'ai'
  | 'guide'
  | 'history'
  | 'trash'
  | 'organize';

/** What the grid shows: a sidebar scope (all, a category, a folder…). */
export interface Scope {
  key: string;
  label: string;
  query: Partial<Query>;
}

export const ALL: Scope = { key: 'all', label: 'Timeline', query: {} };

const emptyCounts: Counts = { total: 0, inbox: 0, review: 0, date_issues: 0, no_location: 0, trash: 0, duplicates: 0 };

class AppState {
  ready = $state(false);
  error = $state<string | null>(null);
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);
  roots = $state<RootInfo[]>([]);
  counts = $state<Counts>(emptyCounts);
  categories = $state<Category[]>([]);
  tags = $state<TagCount[]>([]);
  jobs = $state<JobInfo[]>([]);
  /** Downloadable packs; refreshed as downloads progress. */
  packs = $state<PackStatus[]>([]);
  /** The pack the download confirmation is open for. */
  packAsk = $state<string | null>(null);
  view = $state<View>('timeline');
  scope = $state<Scope>(ALL);
  /** Filters from the search field and the filter panel. */
  filters = $state<Partial<Query>>({});
  /** Chip labels the search parser chose ("Near Italy", "Summer 2019"). */
  filterLabels = $state<Record<string, string>>({});
  infoOpen = $state(false);
  sidebarCollapsed = $state(false);
  paletteOpen = $state(false);
  filtersOpen = $state(false);
  /** Items the tag sheet is open for. */
  tagging = $state<number[] | null>(null);
  /** Items given as examples in the Teach sheet. */
  teaching = $state<number[] | null>(null);
  /** The best matches sheet for the current search. */
  bestOpen = $state(false);
  /** Trashed items waiting for confirmation to go to the Recycle Bin. */
  recycling = $state<number[] | 'all' | null>(null);
  /** Time zones to offer after places were set. */
  zones = $state<ZoneProposal[] | null>(null);
  /** Items the "Set location…" sheet is open for. */
  locating = $state<number[] | null>(null);
  /** Items the "Change date…" sheet is open for. */
  dating = $state<number[] | null>(null);
  /** A place copied from one item, to give to others. */
  copiedPlace = $state<{ lat: number; lon: number; accuracy_m: number; label: string } | null>(null);
  /** Items with changes not yet saved to their files. */
  unsaved = $state(0);
  /** Bumped when the library changes, so views reload. */
  revision = $state(0);
  thumbsRevision = $state(0);
  historyRevision = $state(0);
  importsRevision = $state(0);
  aiRevision = $state(0);
  /** The import the Add photos view shows. */
  importing = $state<number | null>(null);

  query = $derived<Query>(mergeQuery(this.scope.query, this.filters));

  async init() {
    try {
      const [info, settings, roots] = await Promise.all([api.appInfo(), api.getSettings(), api.roots()]);
      this.info = info;
      this.settings = settings;
      this.roots = roots;
      applyTheme(settings.theme);
      await this.refresh();
      this.jobs = await api.jobs();
      await this.loadPacks();
    } catch (e) {
      this.error = String(e);
    }
    api.on('jobs', (list) => (this.jobs = list));
    // A folder dropped onto the window, or a camera card connected: Add photos.
    api.on('drop', (paths) => {
      if (paths[0]) void this.addPhotos(paths[0]).catch((e) => toast.error(e));
    });
    api.on('card', (path) =>
      toast.show(`A camera card or phone is connected (${path}).`, { action: { label: 'Add photos from it', run: () => void this.addPhotos(path).catch((e) => toast.error(e)) } }),
    );
    api.on('changed', (what) => {
      if (what === 'thumbs') this.thumbsRevision++;
      else if (what === 'settings') void api.getSettings().then((s) => (this.settings = s));
      else if (what === 'history') this.historyRevision++;
      else if (what === 'packs') void this.loadPacks();
      else if (what === 'imports') this.importsRevision++;
      else if (what === 'ai') this.aiRevision++;
      else void this.refresh();
    });
    this.ready = true;
  }

  async refresh() {
    const [roots, counts, categories, tags, unsaved] = await Promise.all([
      api.roots(),
      api.counts(),
      api.categories(),
      api.tagCounts(),
      api.unsaved(),
    ]);
    this.roots = roots;
    this.counts = counts;
    this.categories = categories;
    this.tags = tags;
    this.unsaved = unsaved;
    this.revision++;
    this.historyRevision++;
  }

  async loadPacks() {
    this.packs = await api.packs();
  }

  pack(id: string): PackStatus | undefined {
    return this.packs.find((p) => p.id === id);
  }

  async saveSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...this.settings, ...patch };
    this.settings = next;
    if (patch.theme) applyTheme(patch.theme);
    await api.saveSettings($state.snapshot(next));
  }

  /** Check a folder to add: `path`, or one the owner picks. */
  async addPhotos(path: string | null = null) {
    const id = await api.importStart(path);
    if (id == null) return;
    this.importing = id;
    this.go('add');
  }

  go(view: View, scope?: Scope) {
    this.view = view;
    if (scope) this.scope = scope;
  }

  get activeJobs() {
    return this.jobs.filter((j) => j.state === 'running' || j.state === 'queued' || j.state === 'paused');
  }
}

export function mergeQuery(...parts: Partial<Query>[]): Query {
  const q = emptyQuery();
  for (const p of parts) {
    for (const [k, v] of Object.entries(p) as [keyof Query, never][]) {
      const cur = q[k] as unknown;
      if (Array.isArray(cur) && Array.isArray(v)) (q[k] as unknown) = [...cur, ...(v as unknown[])];
      else if (v !== undefined) (q[k] as unknown) = v;
    }
  }
  return q;
}

export function applyTheme(theme: string) {
  const root = document.documentElement;
  if (theme === 'light' || theme === 'dark') root.dataset.theme = theme;
  else delete root.dataset.theme;
  void api.setWindowTheme(theme === 'light' || theme === 'dark' ? theme : null).catch(() => undefined);
}

export const app = new AppState();
