import { api, emptyQuery } from '../ipc/api';
import type { AppInfo } from '../ipc/bindings/AppInfo';
import type { Category } from '../ipc/bindings/Category';
import type { Counts } from '../ipc/bindings/Counts';
import type { JobInfo } from '../ipc/bindings/JobInfo';
import type { Query } from '../ipc/bindings/Query';
import type { RootInfo } from '../ipc/bindings/RootInfo';
import type { Settings } from '../ipc/bindings/Settings';
import type { TagCount } from '../ipc/bindings/TagCount';

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
  view = $state<View>('timeline');
  scope = $state<Scope>(ALL);
  /** Filters from the search field and the filter panel. */
  filters = $state<Partial<Query>>({});
  infoOpen = $state(false);
  sidebarCollapsed = $state(false);
  paletteOpen = $state(false);
  filtersOpen = $state(false);
  /** Bumped when the library changes, so views reload. */
  revision = $state(0);
  thumbsRevision = $state(0);

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
    } catch (e) {
      this.error = String(e);
    }
    api.on('jobs', (list) => (this.jobs = list));
    api.on('changed', (what) => {
      if (what === 'thumbs') this.thumbsRevision++;
      else if (what === 'settings') void api.getSettings().then((s) => (this.settings = s));
      else void this.refresh();
    });
    this.ready = true;
  }

  async refresh() {
    const [roots, counts, categories, tags] = await Promise.all([api.roots(), api.counts(), api.categories(), api.tagCounts()]);
    this.roots = roots;
    this.counts = counts;
    this.categories = categories;
    this.tags = tags;
    this.revision++;
  }

  async saveSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...this.settings, ...patch };
    this.settings = next;
    if (patch.theme) applyTheme(patch.theme);
    await api.saveSettings(next);
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
