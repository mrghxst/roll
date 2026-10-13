// The backend interface the UI uses. In the app it is Tauri; in a plain
// browser (development, Playwright) it is an in-memory mock.

import type { AppInfo } from './bindings/AppInfo';
import type { AssetDetail } from './bindings/AssetDetail';
import type { BatchInfo } from './bindings/BatchInfo';
import type { Candidate } from './bindings/Candidate';
import type { DateFix } from './bindings/DateFix';
import type { DatePreview } from './bindings/DatePreview';
import type { DateSet } from './bindings/DateSet';
import type { Cluster } from './bindings/Cluster';
import type { DupeGroup } from './bindings/DupeGroup';
import type { ParsedSearch } from './bindings/ParsedSearch';
import type { ReviewItem } from './bindings/ReviewItem';
import type { ReviewSummary } from './bindings/ReviewSummary';
import type { LocationResult } from './bindings/LocationResult';
import type { PlaceGroup } from './bindings/PlaceGroup';
import type { PlaceHit } from './bindings/PlaceHit';
import type { PlacesInfo } from './bindings/PlacesInfo';
import type { TrackInfo } from './bindings/TrackInfo';
import type { MapPoints } from './mappoints';
import type { Category } from './bindings/Category';
import type { CategoryPatch } from './bindings/CategoryPatch';
import type { Counts } from './bindings/Counts';
import type { EditAction } from './bindings/EditAction';
import type { Facets } from './bindings/Facets';
import type { FolderNode } from './bindings/FolderNode';
import type { JobInfo } from './bindings/JobInfo';
import type { PlanOptions } from './bindings/PlanOptions';
import type { PlanSummary } from './bindings/PlanSummary';
import type { EventInfo } from './bindings/EventInfo';
import type { AlbumInfo } from './bindings/AlbumInfo';
import type { AiStatus } from './bindings/AiStatus';
import type { CheckItem } from './bindings/CheckItem';
import type { CheckReport } from './bindings/CheckReport';
import type { DecidePlan } from './bindings/DecidePlan';
import type { ModelOffer } from './bindings/ModelOffer';
import type { RunInfo } from './bindings/RunInfo';
import type { ImmichPlan } from './bindings/ImmichPlan';
import type { ImmichStatus } from './bindings/ImmichStatus';
import type { ImmichUser } from './bindings/ImmichUser';
import type { ImportInfo } from './bindings/ImportInfo';
import type { ImportOptions } from './bindings/ImportOptions';
import type { ImportPlan } from './bindings/ImportPlan';
import type { PackStatus } from './bindings/PackStatus';
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
  /** Files or folders dropped onto the window. */
  drop: (paths: string[]) => void;
  /** A camera card or phone with a DCIM folder was connected. */
  card: (path: string) => void;
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
  dateSets(): Promise<DateSet[]>;
  datePreview(fix: DateFix): Promise<DatePreview>;
  /** Returns how many items changed. */
  dateApply(fix: DateFix): Promise<number>;
  dateCandidates(ids: number[]): Promise<Candidate[]>;
  /** Hide a place suggestion, a date set or a duplicate group. */
  dismiss(kind: 'place' | 'date' | 'dupe', key: string): Promise<void>;
  placesInfo(): Promise<PlacesInfo>;
  /** Asks for the folder with GeoNames files; null when cancelled. */
  installPlaces(): Promise<PlacesInfo | null>;
  placeSearch(query: string): Promise<PlaceHit[]>;
  /** Detects events again, then lists proposed and confirmed ones. */
  events(): Promise<EventInfo[]>;
  /** Tags the items Event/<name>; a new name renames first. */
  eventConfirm(id: number, name: string | null): Promise<void>;
  eventRename(id: number, name: string): Promise<void>;
  /** A proposed event is hidden; a confirmed one loses its tags. */
  eventDismiss(id: number): Promise<void>;
  eventMerge(ids: number[]): Promise<void>;
  /** `at` and later items become a new event; false when `at` is first. */
  eventSplit(id: number, at: number, name: string): Promise<boolean>;
  eventOwnFolder(id: number, on: boolean): Promise<void>;
  /** Start checking a folder to add (one the owner picks when `path` is
   * null). Returns the import, or null when cancelled. */
  importStart(path: string | null): Promise<number | null>;
  /** Open imports, newest first. */
  imports(): Promise<ImportInfo[]>;
  importInfo(id: number): Promise<ImportInfo>;
  /** The event name; null goes back to the proposed one. */
  importName(id: number, name: string | null): Promise<void>;
  /** "Dates look right" and "Places look right". */
  importMark(id: number, datesOk: boolean | null, placesOk: boolean | null): Promise<void>;
  importLeaveOut(id: number, ids: number[], out: boolean): Promise<void>;
  /** Place suggestions for the import's items (accept with acceptPlaces). */
  importPlaces(id: number): Promise<PlaceGroup[]>;
  importPlan(id: number, opts: ImportOptions): Promise<ImportPlan>;
  /** Copy or move into the library; returns the History batch. */
  importApply(id: number, opts: ImportOptions): Promise<number>;
  /** Forget the import; the source files stay where they are. */
  importClose(id: number): Promise<void>;
  /** The text model's reading of search words the parser left. */
  aiSearch(text: string): Promise<ParsedSearch>;
  /** The AI mode, local servers found on their default ports, and what is missing. */
  aiStatus(): Promise<AiStatus>;
  /** Image-capable OpenRouter models with live prices, cheapest first. */
  aiOffers(): Promise<ModelOffer[]>;
  /** Use a model for decisions; an OpenRouter offer keeps its price for estimates. */
  aiPick(model: string, offer: ModelOffer | null): Promise<void>;
  /** The uncertain items, the estimate and the consent text. */
  aiPlan(): Promise<DecidePlan>;
  /** Start a decision run; returns its id. */
  aiStart(budget: number | null): Promise<number>;
  /** Continue a stopped or capped run. */
  aiResume(run: number, budget: number | null): Promise<void>;
  aiRuns(): Promise<RunInfo[]>;
  /** A new stratified sample of about 150 items for the model check. */
  aiCheckSample(): Promise<number>;
  aiCheckItems(): Promise<CheckItem[]>;
  /** The owner's category for a sample item (null clears it). */
  aiCheckLabel(id: number, key: string | null): Promise<void>;
  /** Runs 1–3 models on the labelled sample. */
  aiCheckStart(models: string[]): Promise<void>;
  aiCheckReport(): Promise<CheckReport>;
  /** Settings, and what the server says when Immich is on. */
  immichStatus(): Promise<ImmichStatus>;
  /** What a sync would do; from the library alone. */
  immichPlan(): Promise<ImmichPlan>;
  /** Start a sync; `remove` confirms moving items to Immich's trash. */
  immichSync(remove: boolean): Promise<void>;
  /** Other users on the server, to share albums with. */
  immichUsers(): Promise<ImmichUser[]>;
  immichAlbums(): Promise<AlbumInfo[]>;
  /** Whether an event becomes an album, and who it is shared with. */
  immichAlbum(event: number, send: boolean, share: string[]): Promise<void>;
  /** Asks where to save the Immich CLI script; false when cancelled. */
  saveImmichScript(): Promise<boolean>;
  /** Downloadable place names and models, with their state. */
  packs(): Promise<PackStatus[]>;
  /** Starts a download the owner has confirmed. */
  downloadPack(id: string): Promise<void>;
  /** Stops a download (the part on disk is kept) or clears a failure. */
  cancelPack(id: string): Promise<void>;
  removePack(id: string): Promise<void>;
  placeName(lat: number, lon: number): Promise<string | null>;
  tracks(): Promise<TrackInfo[]>;
  /** Asks for files to import; empty when cancelled. */
  importTracks(): Promise<TrackInfo[]>;
  deleteTrack(id: number): Promise<void>;
  placeGroups(): Promise<PlaceGroup[]>;
  acceptPlaces(key: string): Promise<LocationResult>;
  /** A place for the items, or null to clear it. */
  setLocation(ids: number[], place: { lat: number; lon: number; accuracy_m: number; name: string | null } | null): Promise<LocationResult>;
  applyZone(ids: number[], offsetMin: number, zone: string): Promise<number>;
  mapPoints(query: Query): Promise<MapPoints>;
  reviewSummary(): Promise<ReviewSummary>;
  /** A category key or "audit"; most doubtful first. */
  reviewItems(key: string, limit: number): Promise<ReviewItem[]>;
  acceptAbove(key: string, min: number): Promise<number>;
  /** Clusters of undecided items; `split` breaks one up by folder. */
  discover(split: string | null): Promise<Cluster[]>;
  discoverIgnore(key: string): Promise<void>;
  /** What the search parser understands in the text. */
  parseSearch(text: string): Promise<ParsedSearch>;
  dupeGroups(): Promise<DupeGroup[]>;
  /** Keep one, move the others to Roll's trash. */
  mergeDupes(keep: number, others: number[]): Promise<number>;
  on<K extends keyof Events>(event: K, cb: Events[K]): () => void;
  thumbUrl(id: number, v: number): string;
  previewUrl(id: number, v: number): string;
  fullUrl(id: number, v: number): string;
  videoUrl(id: number, native: boolean): string;
  showWindow(): Promise<void>;
  setWindowTheme(theme: 'light' | 'dark' | null): Promise<void>;
}

export { emptyQuery } from './query';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const api: Api = isTauri ? (await import('./tauri')).tauriApi : (await import('./mock')).mockApi;
