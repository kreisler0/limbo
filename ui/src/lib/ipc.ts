// Typed access to the host. Inside Limbo this is Tauri IPC; in a normal
// browser (`npm run dev`, Playwright) a mock backend stands in, so the whole UI
// can be developed and tested without Windows.

import type {
  AppState,
  Bookmark,
  BookmarkKind,
  BookmarkRoots,
  Download,
  ExtensionInfo,
  FirefoxProfile,
  HistoryEntry,
  ImportSummary,
  ImportType,
  Insets,
  InstallReview,
  LoginSummary,
  Preset,
  Settings,
  Shortcut,
  SitePermission,
  StoreName,
  Suggestion,
  TabId,
  Tile,
} from './types';

export interface Backend {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): () => void;
}

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let backendPromise: Promise<Backend> | null = null;

function backend(): Promise<Backend> {
  backendPromise ??= inTauri
    ? import('@tauri-apps/api/core').then(async (core) => {
        const ev = await import('@tauri-apps/api/event');
        return {
          invoke: <T>(cmd: string, args?: Record<string, unknown>) => core.invoke<T>(cmd, args),
          listen: <T>(event: string, handler: (payload: T) => void) => {
            let unlisten: (() => void) | null = null;
            let cancelled = false;
            ev.listen<T>(event, (e) => handler(e.payload)).then((u) => {
              if (cancelled) u();
              else unlisten = u;
            });
            return () => {
              cancelled = true;
              unlisten?.();
            };
          },
        } satisfies Backend;
      })
    : import('./mock').then((m) => m.createMockBackend());
  return backendPromise;
}

/** Test hook: install a backend before the app starts. */
export function setBackend(b: Backend) {
  backendPromise = Promise.resolve(b);
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (await backend()).invoke<T>(cmd, args);
}

export function on<T>(event: string, handler: (payload: T) => void): () => void {
  let off: (() => void) | null = null;
  let cancelled = false;
  backend().then((b) => {
    if (!cancelled) off = b.listen<T>(event, handler);
  });
  return () => {
    cancelled = true;
    off?.();
  };
}

/** Binary responses (JPEG snapshots) become object URLs. */
async function blobUrl(cmd: string, args?: Record<string, unknown>): Promise<string | null> {
  const data = await call<ArrayBuffer | number[] | string>(cmd, args);
  if (typeof data === 'string') return data || null; // mock returns data URLs
  const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data);
  if (bytes.length === 0) return null;
  return URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
}

export interface CreateOptions {
  background?: boolean;
  index?: number;
  private?: boolean;
  opener?: TabId;
  typed?: boolean;
  pinned?: boolean;
}

export const api = {
  appState: () => call<AppState>('app_state'),
  appReady: () => call<void>('app_ready'),
  restart: () => call<void>('app_restart'),
  respond: (id: number, value: unknown) => call<boolean>('prompt_respond', { id, value }),
  shortcut: (action: Shortcut) => call<void>('shortcut', { action }),

  tabs: {
    create: (url?: string, options?: CreateOptions) => call<TabId>('tabs_create', { url, options }),
    close: (id: TabId) => call<void>('tabs_close', { id }),
    activate: (id: TabId, focusPage = true) => call<void>('tabs_activate', { id, focusPage }),
    move: (id: TabId, index: number) => call<void>('tabs_move', { id, index }),
    pin: (id: TabId, on: boolean) => call<void>('tabs_pin', { id, on }),
    mute: (id: TabId, on: boolean) => call<void>('tabs_mute', { id, on }),
    duplicate: (id: TabId) => call<TabId | null>('tabs_duplicate', { id }),
    reopenClosed: () => call<TabId | null>('tabs_reopen_closed'),
    snapshot: (id: TabId) => blobUrl('tabs_snapshot', { id }),
  },

  nav: {
    go: (id: TabId, input: string, newTab = false) => call<void>('nav_go', { id, input, newTab }),
    url: (id: TabId, url: string, typed = false) => call<void>('nav_url', { id, url, typed }),
    back: (id: TabId) => call<void>('nav_back', { id }),
    forward: (id: TabId) => call<void>('nav_forward', { id }),
    reload: (id: TabId, hard = false) => call<void>('nav_reload', { id, hard }),
    stop: (id: TabId) => call<void>('nav_stop', { id }),
  },

  layout: {
    setInsets: (insets: Insets) => call<void>('layout_set_insets', { insets }),
  },

  overlay: {
    capture: () => blobUrl('overlay_capture'),
    hideContent: () => call<void>('overlay_hide_content'),
    showContent: () => call<void>('overlay_show_content'),
  },

  suggest: {
    query: (text: string, deleting = false) => call<Suggestion[]>('suggest_query', { text, deleting }),
    remove: (url: string) => call<void>('suggest_remove', { url }),
  },

  history: {
    search: (query?: string, before?: number, limit = 100) =>
      call<HistoryEntry[]>('history_search', { query, before, limit }),
    remove: (visitIds: number[]) => call<void>('history_delete', { visitIds }),
    clear: (sinceUs: number, kinds: Record<'history' | 'cookies' | 'cache' | 'downloads' | 'passwords' | 'autofill', boolean>) =>
      call<void>('clear_browsing_data', { sinceUs, kinds }),
  },

  topSites: (limit = 8) => call<Tile[]>('top_sites', { limit }),
  favicon: (url: string) => call<string | null>('favicon_for', { url }),

  bookmarks: {
    tree: () => call<BookmarkRoots>('bookmarks_tree'),
    add: (title: string, url: string | null, parentId?: number, kind: BookmarkKind = 'url', index?: number) =>
      call<number>('bookmarks_add', { parentId, kind, title, url, index }),
    update: (id: number, title?: string, url?: string) => call<void>('bookmarks_update', { id, title, url }),
    move: (id: number, parentId: number, index?: number) => call<void>('bookmarks_move', { id, parentId, index }),
    remove: (id: number) => call<Bookmark>('bookmarks_remove', { id }),
    restore: (subtree: Bookmark) => call<number>('bookmarks_restore', { subtree }),
    forUrl: (url: string) => call<Bookmark[]>('bookmarks_for_url', { url }),
    importHtml: () => call<number | null>('bookmarks_import_html'),
    exportHtml: () => call<string | null>('bookmarks_export_html'),
  },

  passwords: {
    list: () => call<LoginSummary[]>('passwords_list'),
    forTab: (id: TabId) => call<LoginSummary[]>('passwords_for_tab', { id }),
    reveal: (id: number) => call<string>('passwords_reveal', { id }),
    remove: (id: number) => call<void>('passwords_delete', { id }),
    exportCsv: () => call<string | null>('passwords_export'),
    importCsv: () => call<{ imported: number; merged: number; failed: number } | null>('passwords_import_csv'),
    fill: (tabId: TabId, loginId: number) => call<void>('autofill_fill', { tabId, loginId }),
  },

  ext: {
    list: () => call<ExtensionInfo[]>('ext_list'),
    prepareStore: (store: StoreName, id: string) => call<InstallReview>('ext_prepare_store', { store, id }),
    prepareFile: () => call<InstallReview | null>('ext_prepare_file'),
    confirm: (token: number) => call<ExtensionInfo>('ext_confirm', { token }),
    loadUnpacked: () => call<ExtensionInfo | null>('ext_load_unpacked'),
    remove: (id: string) => call<void>('ext_remove', { id }),
    setEnabled: (id: string, on: boolean) => call<void>('ext_set_enabled', { id, on }),
    setPinned: (id: string, on: boolean) => call<void>('ext_set_pinned', { id, on }),
    openPopup: (id: string, popup: string, anchor: { x: number; y: number; width: number; height: number }) =>
      call<void>('ext_open_popup', { id, popup, anchor }),
    closePopup: () => call<void>('ext_close_popup'),
    openOptions: (id: string, page: string) => call<void>('ext_open_options', { id, page }),
  },

  import: {
    detect: () => call<FirefoxProfile[]>('import_detect'),
    chooseFolder: () => call<FirefoxProfile | null>('import_choose_folder'),
    run: (profilePath: string, types: ImportType[], primaryPassword?: string) =>
      call<ImportSummary>('import_run', { args: { profilePath, types, primaryPassword } }),
  },

  downloads: {
    list: () => call<Download[]>('downloads_list'),
    chooseFolder: () => call<Settings | null>('downloads_choose_folder'),
    control: (id: number, action: 'pause' | 'resume' | 'cancel') => call<void>('downloads_control', { id, action }),
    open: (path: string) => call<void>('downloads_open', { path }),
    showInFolder: (path: string) => call<void>('downloads_show_in_folder', { path }),
    remove: (id: number) => call<void>('downloads_remove', { id }),
    clear: () => call<number>('downloads_clear'),
  },

  sites: {
    permissions: (origin?: string) => call<SitePermission[]>('site_permissions', { origin }),
    revoke: (origin: string, kind: string) => call<void>('site_permission_revoke', { origin, kind }),
  },

  settings: {
    get: () => call<Settings>('settings_get'),
    set: (patch: Partial<Settings>) => call<Settings>('settings_set', { patch }),
  },

  find: {
    start: (id: TabId, text: string, matchCase = false) => call<void>('find_start', { id, text, matchCase }),
    step: (id: TabId, forward: boolean) => call<void>('find_step', { id, forward }),
    stop: (id: TabId) => call<void>('find_stop', { id }),
  },

  zoom: (id: TabId, step: 'in' | 'out' | 'reset') => call<void>('zoom_step', { id, step }),

  window: {
    minimize: () => call<void>('window_minimize'),
    toggleMaximize: () => call<void>('window_toggle_maximize'),
    close: () => call<void>('window_close'),
    toggleFullscreen: () => call<void>('window_toggle_fullscreen'),
    snapLayouts: () => call<void>('window_snap_layouts'),
  },

  memory: {
    setSampling: (mode: 'popover' | 'pill' | 'off') => call<void>('memory_set_sampling', { mode }),
    sleepTab: (id: TabId) => call<void>('memory_sleep_tab', { id }),
    unloadTab: (id: TabId) => call<void>('memory_unload_tab', { id }),
    sleepAll: () => call<void>('memory_sleep_all'),
    setMaxAwake: (max: number | null) => call<Settings>('memory_set_max_awake', { max }),
    setPreset: (preset: Preset) => call<Settings>('memory_set_preset', { preset }),
  },

  print: (id: TabId) => call<void>('print_page', { id }),
  devtools: (id: TabId) => call<void>('devtools_open', { id }),
  defaultBrowser: {
    open: () => call<void>('default_browser_open'),
    status: () => call<boolean>('default_browser_status'),
  },
};
