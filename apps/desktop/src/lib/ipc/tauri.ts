import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Api, Events } from './api';
import { decodeTimeline } from './timeline';

const url = (path: string) => convertFileSrc(path, 'roll');

export const tauriApi: Api = {
  mock: false,
  appInfo: () => invoke('app_info'),
  getSettings: () => invoke('get_settings'),
  saveSettings: (settings) => invoke('save_settings', { settings }),
  pickFolder: (title) => invoke('pick_folder', { title }),
  roots: () => invoke('roots'),
  addRoot: (path) => invoke('add_root', { path }),
  removeRoot: (id) => invoke('remove_root', { id }),
  rescan: () => invoke('rescan'),
  timeline: async (query) => decodeTimeline(await invoke<ArrayBuffer>('timeline', { query })),
  queryIds: (query) => invoke('query_ids', { query }),
  tiles: (ids) => invoke('tiles', { ids }),
  assetDetail: (id) => invoke('asset_detail', { id }),
  folders: () => invoke('folders'),
  categories: () => invoke('categories'),
  tagCounts: () => invoke('tag_counts'),
  counts: () => invoke('counts'),
  facets: (query) => invoke('facets', { query }),
  jobs: () => invoke('jobs'),
  jobControl: (id, action) => invoke('job_control', { id, action }),
  showInFolder: (id) => invoke('show_in_folder', { id }),
  openFolder: (rootId, path) => invoke('open_folder', { rootId, path }),
  openWithDefault: (id) => invoke('open_with_default', { id }),
  clearLogs: () => invoke('clear_logs'),
  setSecret: (name, value) => invoke('set_secret', { name, value }),
  hasSecret: (name) => invoke('has_secret', { name }),
  deleteSecret: (name) => invoke('delete_secret', { name }),
  on<K extends keyof Events>(event: K, cb: Events[K]) {
    const un = listen(event, (e) => (cb as (p: unknown) => void)(e.payload));
    return () => void un.then((f) => f());
  },
  thumbUrl: (id, v) => `${url(`thumb/${id}`)}?v=${v}`,
  previewUrl: (id, v) => `${url(`preview/${id}`)}?v=${v}`,
  fullUrl: (id, v) => `${url(`full/${id}`)}?v=${v}`,
  videoUrl: (id, native) => `${url(`video/${id}`)}?native=${native ? 1 : 0}`,
  showWindow: () => getCurrentWindow().show(),
  setWindowTheme: (theme) => getCurrentWindow().setTheme(theme),
};
