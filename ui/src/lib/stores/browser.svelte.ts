// The UI's picture of the browser, kept in sync by host events.

import { api, on } from '../ipc';
import type {
  AppState,
  BookmarkRoots,
  ContextMenuRequest,
  Download,
  ExtensionInfo,
  FindResult,
  MemorySample,
  PermissionPrompt,
  PressureLevel,
  SavePasswordPrompt,
  Settings,
  Shortcut,
  Suggestion,
  TabId,
  TabInfo,
  TabState,
  WindowState,
} from '../types';

export interface Toast {
  id: number;
  message: string;
  action?: { label: string; run: () => void };
  icon?: string;
}

class BrowserStore {
  tabs = $state<TabInfo[]>([]);
  activeId = $state<TabId | null>(null);
  settings = $state<Settings | null>(null);
  platform = $state<AppState['platform'] | null>(null);
  win = $state<WindowState>({ maximized: false, fullscreen: false, focused: true, minimized: false });
  firstRun = $state(false);
  isDefaultBrowser = $state(true);
  ready = $state(false);

  memory = $state<MemorySample | null>(null);
  pressure = $state<PressureLevel>('normal');
  downloads = $state<Download[]>([]);
  extensions = $state<ExtensionInfo[]>([]);
  bookmarks = $state<BookmarkRoots | null>(null);
  /** Saved logins per tab (omnibox key icon). */
  logins = $state<Record<TabId, number>>({});
  find = $state<FindResult | null>(null);
  toasts = $state<Toast[]>([]);
  permissionPrompts = $state<PermissionPrompt[]>([]);
  savePrompts = $state<SavePasswordPrompt[]>([]);
  contextMenu = $state<ContextMenuRequest | null>(null);
  /** Late (Google) suggestion rows for the text they belong to. */
  remoteSuggestions = $state<{ text: string; rows: Suggestion[] } | null>(null);
  /** Autofill picker requested by the in-page chip. */
  autofillFor = $state<TabId | null>(null);
  lastZoom = $state<{ id: TabId; factor: number; at: number } | null>(null);

  active = $derived(this.tabs.find((t) => t.id === this.activeId) ?? null);
  pinnedCount = $derived(this.tabs.filter((t) => t.pinned).length);

  private shortcutHandler: ((s: Shortcut) => void) | null = null;
  private toastSeq = 0;

  onShortcut(handler: (s: Shortcut) => void) {
    this.shortcutHandler = handler;
  }

  async init() {
    this.listen();
    const s = await api.appState();
    this.tabs = s.tabs;
    this.activeId = s.activeId;
    this.settings = s.settings;
    this.platform = s.platform;
    this.win = s.window;
    this.firstRun = s.firstRun;
    this.isDefaultBrowser = s.isDefaultBrowser;
    this.ready = true;
    void this.refreshExtensions();
    void this.refreshBookmarks();
    void api.downloads.list().then((d) => (this.downloads = d));
  }

  private upsert(tab: TabInfo, index?: number) {
    const i = this.tabs.findIndex((t) => t.id === tab.id);
    if (i >= 0) this.tabs[i] = tab;
    else if (index !== undefined) this.tabs.splice(Math.min(index, this.tabs.length), 0, tab);
    else this.tabs.push(tab);
  }

  private listen() {
    on<{ tab: TabInfo; index: number }>('tab:created', ({ tab, index }) => this.upsert(tab, index));
    on<TabInfo>('tab:updated', (tab) => this.upsert(tab));
    on<{ id: TabId }>('tab:closed', ({ id }) => {
      this.tabs = this.tabs.filter((t) => t.id !== id);
      delete this.logins[id];
    });
    on<{ id: TabId }>('tab:activated', ({ id }) => {
      this.activeId = id;
      if (this.find && this.find.tabId !== id) this.find = null;
    });
    on<TabId[]>('tabs:order', (ids) => {
      const byId = new Map(this.tabs.map((t) => [t.id, t]));
      this.tabs = ids.map((id) => byId.get(id)).filter((t): t is TabInfo => !!t);
    });
    on<[TabId, TabState][]>('tab:states', (states) => {
      for (const [id, state] of states) {
        const t = this.tabs.find((x) => x.id === id);
        if (t && t.state !== state) t.state = state;
      }
    });
    on<{ text: string; rows: Suggestion[] }>('suggest:results', (r) => (this.remoteSuggestions = r));
    on<MemorySample>('memory:sample', (s) => (this.memory = s));
    on<PressureLevel>('memory:pressure', (p) => (this.pressure = p));
    on<Download>('download:progress', (d) => {
      const i = this.downloads.findIndex((x) => x.id === d.id);
      if (i >= 0) this.downloads[i] = d;
      else this.downloads.unshift(d);
    });
    on<WindowState>('window:state', (w) => (this.win = w));
    on<Settings>('settings:changed', (s) => (this.settings = s));
    on<null>('bookmarks:changed', () => void this.refreshBookmarks());
    on<null>('ext:changed', () => void this.refreshExtensions());
    on<{ message: string }>('toast', ({ message }) => this.toast(message));
    on<Shortcut>('shortcut', (s) => this.shortcutHandler?.(s));
    on<FindResult>('find:result', (r) => (this.find = r));
    on<{ id: TabId; factor: number }>('zoom', ({ id, factor }) => (this.lastZoom = { id, factor, at: Date.now() }));
    on<{ tabId: TabId; count: number }>('autofill:available', ({ tabId, count }) => (this.logins[tabId] = count));
    on<{ tabId: TabId }>('autofill:choose', ({ tabId }) => (this.autofillFor = tabId));
    on<PermissionPrompt>('perm:request', (p) => this.permissionPrompts.push(p));
    on<SavePasswordPrompt>('passwords:save-prompt', (p) => this.savePrompts.push(p));
    on<ContextMenuRequest>('context-menu', (m) => {
      // A newer menu replaces an unanswered one (which must still be released).
      if (this.contextMenu) void api.respond(this.contextMenu.menuId, { itemId: null });
      this.contextMenu = m;
    });
  }

  async refreshExtensions() {
    this.extensions = await api.ext.list().catch(() => []);
  }

  async refreshBookmarks() {
    this.bookmarks = await api.bookmarks.tree().catch(() => null);
  }

  toast(message: string, action?: Toast['action'], icon?: string) {
    const t: Toast = { id: ++this.toastSeq, message, action, icon };
    this.toasts = [t]; // one at a time
    setTimeout(() => {
      this.toasts = this.toasts.filter((x) => x.id !== t.id);
    }, 4000);
  }

  dismissToast(id: number) {
    this.toasts = this.toasts.filter((x) => x.id !== id);
  }

  async updateSettings(patch: Partial<Settings>) {
    this.settings = await api.settings.set(patch);
  }

  tab(id: TabId | null | undefined): TabInfo | undefined {
    return this.tabs.find((t) => t.id === id);
  }

  memoryForTab(id: TabId): number | null {
    const row = this.memory?.rows.find((r) => r.kind === 'tab' && r.id === String(id));
    return row ? row.bytes : null;
  }
}

export const browser = new BrowserStore();

export function formatMB(bytes: number): string {
  return `${Math.round(bytes / 1048576)} MB`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(bytes < 10485760 ? 1 : 0)} MB`;
  return `${(bytes / 1073741824).toFixed(1)} GB`;
}

export function downloadStatus(d: Download): string {
  switch (d.state) {
    case 'inProgress':
      return d.total ? `${formatBytes(d.received)} of ${formatBytes(d.total)}` : formatBytes(d.received);
    case 'paused':
      return `Paused · ${formatBytes(d.received)}`;
    case 'completed':
      return d.total ? formatBytes(d.total) : 'Done';
    case 'cancelled':
      return 'Cancelled';
    default:
      return d.danger ? 'Blocked: this file may be harmful' : 'Failed';
  }
}
