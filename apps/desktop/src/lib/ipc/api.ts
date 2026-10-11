// The backend interface the UI uses. In the app it is Tauri; in a plain
// browser (development, Playwright) it is an in-memory mock.

import type { AppInfo } from './bindings/AppInfo';
import type { AssetDetail } from './bindings/AssetDetail';
import type { BatchInfo } from './bindings/BatchInfo';
import type { Category } from './bindings/Category';
import type { CategoryPatch } from './bindings/CategoryPatch';
import type { Counts } from './bindings/Counts';
import type { EditAction } from './bindings/EditAction';
import type { Facets } from './bindings/Facets';
import type { FolderNode } from './bindings/FolderNode';
import type { JobInfo } from './bindings/JobInfo';
import type { PlanOptions } from './bindings/PlanOptions';
import type { PlanSummary } from './bindings/PlanSummary';
import type { Query } from './bindings/Query';
import type { RootInfo } from './bindings/RootInfo';
import type { Rule } from './bindings/Rule';
import type { ScanReport } from './bindings/ScanReport';
import type { Settings } from './bindings/Settings';
import type { TagCount } from './bindings/TagCount';
import type { TileInfo } from './bindings/TileInfo';
import type { Timeline } from './timeline';

export type ChangeKind = 'assets' | 'thumbs' | 'roots' | 'settings' | string;

export interface Events {
  jobs: (list: JobInfo[]) => void;
  changed: (what: ChangeKind) => void;
  scanned: (report: ScanReport) => void;
}

export interface Api {
  readonly mock: boolean;
  appInfo(): Promise<AppInfo>;
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<void>;
  pickFolder(title: string): Promise<string | null>;
  roots(): Promise<RootInfo[]>;
  addRoot(path: string): Promise<RootInfo>;
  removeRoot(id: number): Promise<void>;
  rescan(): Promise<void>;
  timeline(query: Query): Promise<Timeline>;
  queryIds(query: Query): Promise<number[]>;
  tiles(ids: number[]): Promise<TileInfo[]>;
  assetDetail(id: number): Promise<AssetDetail | null>;
  folders(): Promise<FolderNode[]>;
  categories(): Promise<Category[]>;
  tagCounts(): Promise<TagCount[]>;
  counts(): Promise<Counts>;
  facets(query: Query): Promise<Facets>;
  jobs(): Promise<JobInfo[]>;
  jobControl(id: number, action: 'pause' | 'resume' | 'cancel'): Promise<void>;
  showInFolder(id: number): Promise<void>;
  openFolder(rootId: number, path: string): Promise<void>;
  openWithDefault(id: number): Promise<void>;
  clearLogs(): Promise<void>;
  /** Store an API key in the OS keychain. It can never be read back here. */
  setSecret(name: 'openrouter' | 'immich', value: string): Promise<void>;
  hasSecret(name: 'openrouter' | 'immich'): Promise<boolean>;
  deleteSecret(name: 'openrouter' | 'immich'): Promise<void>;
  /** One action on a selection; returns how many items changed. */
  edit(ids: number[], action: EditAction): Promise<number>;
  setTrashed(ids: number[], trashed: boolean): Promise<void>;
  /** Send trashed items to the system Recycle Bin (all when `ids` is null). */
  emptyTrash(ids: number[] | null): Promise<number>;
  plan(options: PlanOptions): Promise<PlanSummary>;
  /** Asks where to save; false when the owner cancelled. */
  savePlanCsv(batch: number): Promise<boolean>;
  applyPlan(batch: number): Promise<void>;
  undo(batch: number): Promise<void>;
  /** Undo the most recent change; returns its label. */
  undoLast(): Promise<string | null>;
  history(limit: number): Promise<BatchInfo[]>;
  /** Items with changes not yet saved to their files. */
  unsaved(): Promise<number>;
  addCategory(name: string, parent: string | null, definition: string): Promise<string>;
  updateCategory(key: string, patch: Partial<CategoryPatch>): Promise<void>;
  deleteCategory(key: string): Promise<void>;
  rules(): Promise<Rule[]>;
  saveRule(rule: Rule): Promise<number>;
  deleteRule(id: number): Promise<void>;
  on<K extends keyof Events>(event: K, cb: Events[K]): () => void;
  thumbUrl(id: number, v: number): string;
  previewUrl(id: number, v: number): string;
  fullUrl(id: number, v: number): string;
  videoUrl(id: number, native: boolean): string;
  showWindow(): Promise<void>;
  setWindowTheme(theme: 'light' | 'dark' | null): Promise<void>;
}

export function emptyQuery(): Query {
  return {
    text: null,
    from_utc: null,
    to_utc: null,
    media: [],
    categories: [],
    exclude_categories: [],
    tags: [],
    event_id: null,
    cameras: [],
    lenses: [],
    formats: [],
    location: null,
    bbox: null,
    root_id: null,
    folder: null,
    recursive: false,
    min_rating: null,
    favorite: null,
    problems: null,
    review: [],
    immich: null,
    orientation: null,
    private: null,
    inbox: null,
    trashed: false,
    missing: false,
    ids: null,
  };
}

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const api: Api = isTauri ? (await import('./tauri')).tauriApi : (await import('./mock')).mockApi;
