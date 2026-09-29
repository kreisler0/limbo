// A stand-in for the Windows host, used when the UI runs in a normal browser
// (`npm run dev`, Playwright). It behaves like the host closely enough to
// develop and test every flow: tabs, navigation, suggestions, memory, import...
// Never bundled into the app's main chunk (see vite.config.ts).

import type { Backend } from './ipc';
import type {
  AppState,
  Bookmark,
  BookmarkRoots,
  Download,
  ExtensionInfo,
  HistoryEntry,
  LoginSummary,
  MemorySample,
  Policy,
  Settings,
  Suggestion,
  TabId,
  TabInfo,
  Tile,
} from './types';

type Handler = (payload: unknown) => void;

const PRESETS: Record<string, Policy> = {
  balanced: { preset: 'balanced', hiddenToLowMs: 30000, lowToSuspendMs: 300000, suspendToDiscardMs: 1800000, maxAwake: 4, discardPinned: false },
  aggressive: { preset: 'aggressive', hiddenToLowMs: 10000, lowToSuspendMs: 60000, suspendToDiscardMs: 300000, maxAwake: 2, discardPinned: false },
  off: { preset: 'off', hiddenToLowMs: null, lowToSuspendMs: null, suspendToDiscardMs: null, maxAwake: null, discardPinned: false },
};

const SITES: [string, string][] = [
  ['https://www.google.com/', 'Google'],
  ['https://mail.google.com/mail/u/0/', 'Inbox (3) - Gmail'],
  ['https://github.com/', 'GitHub'],
  ['https://www.youtube.com/', 'YouTube'],
  ['https://news.ycombinator.com/', 'Hacker News'],
  ['https://en.wikipedia.org/wiki/Limbo', 'Limbo - Wikipedia'],
  ['https://docs.rs/', 'Docs.rs'],
  ['https://www.notion.so/', 'Notion'],
  ['https://www.rust-lang.org/', 'Rust Programming Language'],
  ['https://calendar.google.com/', 'Google Calendar'],
];

const HUES: Record<string, number> = {};
function hueFor(host: string): number {
  if (!(host in HUES)) {
    let h = 0;
    for (const c of host) h = (h * 31 + c.charCodeAt(0)) % 360;
    HUES[host] = h;
  }
  return HUES[host];
}

export function mockFavicon(host: string): string {
  const letter = (host.replace(/^www\./, '')[0] || '?').toUpperCase();
  const hue = hueFor(host);
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'><rect width='32' height='32' rx='7' fill='hsl(${hue} 60% 48%)'/><text x='16' y='22' font-family='Segoe UI,sans-serif' font-size='17' font-weight='600' fill='white' text-anchor='middle'>${letter}</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}

function classify(input: string): { url: string; internal: boolean } {
  const t = input.trim();
  if (/^limbo:\/\//i.test(t)) return { url: t.toLowerCase(), internal: true };
  if (/^https?:\/\//i.test(t)) return { url: t, internal: false };
  if (!/\s/.test(t) && /\.[a-z]{2,}(\/|$)/i.test(t)) return { url: `https://${t}${t.includes('/') ? '' : '/'}`, internal: false };
  return { url: `https://www.google.com/search?q=${encodeURIComponent(t).replace(/%20/g, '+')}`, internal: false };
}

const INTERNAL: Record<string, TabInfo['internal']> = {
  'limbo://newtab': 'newTab',
  'limbo://settings': 'settings',
  'limbo://history': 'history',
  'limbo://bookmarks': 'bookmarks',
  'limbo://downloads': 'downloads',
  'limbo://extensions': 'extensions',
  'limbo://passwords': 'passwords',
};

const TITLES: Record<string, string> = {
  newTab: 'New Tab',
  settings: 'Settings',
  history: 'History',
  bookmarks: 'Bookmarks',
  downloads: 'Downloads',
  extensions: 'Extensions',
  passwords: 'Passwords',
};

export function createMockBackend(): Backend {
  const handlers = new Map<string, Set<Handler>>();
  const emit = (event: string, payload: unknown) => {
    for (const h of handlers.get(event) ?? []) queueMicrotask(() => h(payload));
  };

  let nextId = 0;
  let activeId: TabId | null = null;
  const tabs: TabInfo[] = [];
  const closed: string[] = [];
  const responses: { id: number; value: unknown }[] = [];
  const history: HistoryEntry[] = SITES.map(([url, title], i) => ({
    visitId: i + 1,
    placeId: i + 1,
    url,
    title,
    visitUs: (Date.now() - i * 3_600_000 * (i + 1)) * 1000,
  }));
  let settings: Settings = {
    theme: 'system',
    accent: null,
    memory: { ...PRESETS.balanced },
    showMemoryInToolbar: true,
    showGoogleSuggestions: true,
    restoreTabsOnStartup: true,
    downloadsFolder: null,
    askWhereToSave: false,
    bookmarksBarVisible: false,
    tabLayout: 'top',
    defaultZoom: 1,
    developerMode: false,
    experimentalArgs: [],
    experimentalArgsEnabled: false,
    smartscreenEnabled: false,
    mica: true,
    onboardingDone: new URLSearchParams(location.search).get('onboarding') !== '1',
    offerToSavePasswords: true,
  };
  const downloads: Download[] = [
    { id: 1, tabId: null, url: 'https://example.com/report.pdf', path: 'C:\\Users\\Yida\\Downloads\\report.pdf', fileName: 'report.pdf', received: 2_400_000, total: 2_400_000, state: 'completed', danger: null, canResume: false },
  ];
  let bookmarkId = 100;
  const bm = (title: string, url: string | null, children?: Bookmark[]): Bookmark => ({
    id: ++bookmarkId,
    parentId: null,
    kind: url ? 'url' : 'folder',
    title,
    url,
    position: 0,
    addedUs: Date.now() * 1000,
    modifiedUs: Date.now() * 1000,
    guid: `g${bookmarkId}`,
    children,
  });
  const roots: BookmarkRoots = {
    toolbar: bm('Bookmarks bar', null, [bm('GitHub', 'https://github.com/'), bm('Read later', null, [bm('Rust', 'https://www.rust-lang.org/')]), bm('HN', 'https://news.ycombinator.com/')]),
    menu: bm('Bookmarks menu', null, []),
    other: bm('Other bookmarks', null, [bm('Docs.rs', 'https://docs.rs/')]),
    mobile: bm('Mobile bookmarks', null, []),
  };
  const logins: LoginSummary[] = [
    { id: 1, guid: 'a', origin: 'https://github.com', actionOrigin: null, realm: null, username: 'yida', createdUs: 0, lastUsedUs: null, changedUs: null, timesUsed: 4 },
    { id: 2, guid: 'b', origin: 'https://accounts.google.com', actionOrigin: null, realm: null, username: 'yida@gmail.com', createdUs: 0, lastUsedUs: null, changedUs: null, timesUsed: 12 },
  ];
  const extensions: ExtensionInfo[] = [
    { id: 'ddkjiahejlhfcafbddmgiahcphecmpfh', name: 'uBlock Origin Lite', version: '2025.1.1', description: 'An efficient content blocker.', enabled: true, pinned: true, source: 'chrome-web-store', icon: mockFavicon('ublock'), hasAction: true, popup: 'popup.html', actionTitle: 'uBlock Origin Lite', optionsPage: 'dashboard.html' },
  ];

  const info = (t: TabInfo) => ({ ...t });

  function makeTab(url: string, opts: { private?: boolean; pinned?: boolean } = {}): TabInfo {
    const internal = INTERNAL[url.split(/[?#]/)[0].toLowerCase()] ?? null;
    const host = hostOf(url);
    return {
      id: ++nextId,
      url,
      title: internal ? TITLES[internal] : '',
      favicon: internal ? null : mockFavicon(host),
      loading: !internal,
      progress: internal ? 0 : 0.1,
      canGoBack: false,
      canGoForward: false,
      audible: false,
      muted: false,
      pinned: !!opts.pinned,
      private: !!opts.private,
      state: 'hidden',
      security: internal ? 'internal' : url.startsWith('https') ? 'secure' : 'insecure',
      internal,
      zoom: 1,
      crashed: false,
      hasSnapshot: false,
      displayHost: internal ? TITLES[internal] : host.replace(/^www\./, ''),
      searchTerms: url.includes('google.com/search') ? new URL(url).searchParams.get('q') : null,
      storeExtension: null,
    };
  }

  function finishLoad(t: TabInfo) {
    const steps = [0.4, 0.75, 1];
    steps.forEach((p, i) =>
      setTimeout(() => {
        t.progress = p;
        if (p === 1) {
          t.loading = false;
          const site = SITES.find(([u]) => u === t.url);
          t.title = site?.[1] ?? (t.searchTerms ? `${t.searchTerms} - Google Search` : t.displayHost);
          history.unshift({ visitId: history.length + 1, placeId: history.length + 1, url: t.url, title: t.title, visitUs: Date.now() * 1000 });
        }
        emit('tab:updated', info(t));
      }, 180 * (i + 1)),
    );
  }

  function states() {
    emit(
      'tab:states',
      tabs.map((t) => [t.id, t.state]),
    );
  }

  function activate(id: TabId) {
    const prev = tabs.find((t) => t.id === activeId);
    if (prev && prev.id !== id) prev.state = prev.internal ? 'discarded' : 'hidden';
    const t = tabs.find((x) => x.id === id);
    if (!t) return;
    t.state = 'active';
    activeId = id;
    emit('tab:activated', { id });
    states();
  }

  function create(url: string | undefined, opts: Record<string, unknown> = {}): TabId {
    const t = makeTab(url ?? 'limbo://newtab', { private: !!opts.private, pinned: !!opts.pinned });
    const anchor = tabs.findIndex((x) => x.id === (opts.opener ?? activeId));
    const index = typeof opts.index === 'number' ? opts.index : anchor >= 0 ? anchor + 1 : tabs.length;
    tabs.splice(index, 0, t);
    emit('tab:created', { tab: info(t), index });
    if (!t.internal) finishLoad(t);
    if (!opts.background) activate(t.id);
    return t.id;
  }

  // A realistic session to start from.
  const initial = [SITES[0][0], SITES[2][0], SITES[4][0]];
  for (const u of initial) {
    const t = makeTab(u);
    t.loading = false;
    t.progress = 1;
    t.title = SITES.find(([x]) => x === u)?.[1] ?? '';
    tabs.push(t);
  }
  tabs[1].state = 'suspended';
  tabs[2].state = 'discarded';
  activeId = tabs[0].id;
  tabs[0].state = 'active';

  function memorySample(): MemorySample {
    const rows: MemorySample['rows'] = tabs.filter((t) => !t.internal).map((t) => {
      const base = t.state === 'discarded' ? 0 : t.state === 'suspended' ? 9 : t.state === 'active' ? 58 : 24;
      const jitter = t.state === 'discarded' ? 0 : Math.round(Math.random() * 4);
      return {
        kind: 'tab' as const,
        id: String(t.id),
        bytes: (base + jitter) * 1048576,
        shared: false,
        state: t.state === 'active' ? 'active' : t.state === 'suspended' ? 'sleeping' : t.state === 'discarded' ? 'unloaded' : 'awake',
      };
    });
    rows.push({ kind: 'extension', id: extensions[0].id, bytes: 11 * 1048576, shared: false });
    rows.push({ kind: 'engine', id: 'engine', bytes: (96 + Math.round(Math.random() * 3)) * 1048576, shared: false });
    rows.sort((a, b) => b.bytes - a.bytes);
    const total = rows.reduce((s, r) => s + r.bytes, 0);
    const awake = tabs.filter((t) => t.state === 'active' || t.state === 'hidden' || t.state === 'lowMemory').length;
    return { totalBytes: total, systemAvailBytes: 1.45 * 1024 ** 3, systemTotalBytes: 3.8 * 1024 ** 3, awake, maxAwake: settings.memory.maxAwake, rows };
  }

  let samplingTimer: ReturnType<typeof setInterval> | null = null;

  function renderSnapshot(id: TabId | null = activeId): string {
    // Draw something page-like for the snapshot overlay.
    const t = tabs.find((x) => x.id === id);
    const c = document.createElement('canvas');
    const w = Math.max(1, window.innerWidth);
    const h = Math.max(1, window.innerHeight - 44);
    c.width = w;
    c.height = h;
    const g = c.getContext('2d');
    if (g && t) {
      const hue = hueFor(hostOf(t.url));
      g.fillStyle = `hsl(${hue} 30% 97%)`;
      g.fillRect(0, 0, w, h);
      g.fillStyle = `hsl(${hue} 55% 50%)`;
      g.fillRect(0, 0, w, 64);
      g.fillStyle = '#fff';
      g.font = '600 22px Segoe UI, sans-serif';
      g.fillText(t.title || t.displayHost, 32, 41);
      g.fillStyle = `hsl(${hue} 15% 80%)`;
      for (let i = 0; i < 9; i++) g.fillRect(32, 110 + i * 34, Math.min(w - 64, 280 + ((i * 97) % 420)), 12);
    }
    return c.toDataURL('image/jpeg', 0.8);
  }

  const commands: Record<string, (a: Record<string, any>) => unknown> = {
    app_state: (): AppState => ({
      tabs: tabs.map(info),
      activeId,
      settings,
      platform: { windows11: true, mica: false, version: '0.1.0-mock', engineVersion: '140.0.0.0' },
      window: { maximized: false, fullscreen: false, focused: true, minimized: false },
      firstRun: !settings.onboardingDone,
      isDefaultBrowser: false,
    }),
    app_restart: () => location.reload(),
    app_ready: () => {
      samplingTimer ??= setInterval(() => emit('memory:sample', memorySample()), 2000);
      setTimeout(() => emit('memory:sample', memorySample()), 300);
    },
    prompt_respond: ({ id, value }) => {
      responses.push({ id, value });
      return true;
    },
    // The host performs these itself (never echo them back: the UI would
    // forward them again, forever).
    shortcut: ({ action }) => {
      const i = tabs.findIndex((t) => t.id === activeId);
      const at = (n: number) => tabs[(n + tabs.length) % tabs.length];
      const zoom = (step: string) => activeId !== null && commands.zoom_step({ id: activeId, step });
      switch (action as string) {
        case 'newTab':
        case 'newPrivateTab':
          create(undefined, { private: action === 'newPrivateTab' });
          return emit('shortcut', 'focusOmnibox');
        case 'closeTab': return activeId !== null && commands.tabs_close({ id: activeId });
        case 'reopenClosedTab': return commands.tabs_reopen_closed({});
        case 'nextTab': return commands.tabs_activate({ id: at(i + 1).id });
        case 'prevTab': return commands.tabs_activate({ id: at(i - 1).id });
        case 'tabLast': return commands.tabs_activate({ id: at(-1).id });
        case 'reload':
        case 'hardReload': return activeId !== null && commands.nav_reload({ id: activeId });
        case 'zoomIn': return zoom('in');
        case 'zoomOut': return zoom('out');
        case 'zoomReset': return zoom('reset');
        default: {
          const m = /^tab(\d)$/.exec(action);
          if (m && tabs[Number(m[1]) - 1]) return commands.tabs_activate({ id: tabs[Number(m[1]) - 1].id });
        }
      }
    },

    tabs_create: ({ url, options }) => create(url, options ?? {}),
    tabs_close: ({ id }) => {
      const i = tabs.findIndex((t) => t.id === id);
      if (i < 0) return;
      const [t] = tabs.splice(i, 1);
      if (!t.internal) closed.push(t.url);
      emit('tab:closed', { id });
      if (tabs.length === 0) create(undefined);
      else if (activeId === id) activate((tabs[i] ?? tabs[i - 1]).id);
    },
    tabs_activate: ({ id }) => {
      const t = tabs.find((x) => x.id === id);
      if (t && (t.state === 'discarded' || t.state === 'suspended') && !t.internal) {
        t.loading = true;
        t.progress = 0.3;
        emit('tab:updated', info(t));
        finishLoad(t);
      }
      activate(id);
    },
    tabs_move: ({ id, index }) => {
      const i = tabs.findIndex((t) => t.id === id);
      const [t] = tabs.splice(i, 1);
      tabs.splice(index, 0, t);
      emit('tabs:order', tabs.map((x) => x.id));
    },
    tabs_pin: ({ id, on }) => {
      const t = tabs.find((x) => x.id === id);
      if (!t) return;
      t.pinned = on;
      const i = tabs.indexOf(t);
      tabs.splice(i, 1);
      const pinned = tabs.filter((x) => x.pinned).length;
      tabs.splice(pinned, 0, t);
      emit('tab:updated', info(t));
      emit('tabs:order', tabs.map((x) => x.id));
    },
    tabs_mute: ({ id, on }) => {
      const t = tabs.find((x) => x.id === id);
      if (t) {
        t.muted = on;
        emit('tab:updated', info(t));
      }
    },
    tabs_duplicate: ({ id }) => create(tabs.find((t) => t.id === id)?.url, { opener: id }),
    tabs_reopen_closed: () => (closed.length ? create(closed.pop()) : null),
    tabs_snapshot: ({ id }) => renderSnapshot(id),

    nav_go: ({ id, input, newTab }) => {
      const c = classify(input);
      if (newTab) return create(c.url);
      commands.nav_url({ id, url: c.url });
    },
    nav_url: ({ id, url }) => {
      const t = tabs.find((x) => x.id === id);
      if (!t) return;
      const fresh = makeTab(url);
      Object.assign(t, { ...fresh, id: t.id, state: t.state, pinned: t.pinned, canGoBack: true });
      emit('tab:updated', info(t));
      if (!t.internal) finishLoad(t);
    },
    nav_back: () => {},
    nav_forward: () => {},
    nav_reload: ({ id }) => {
      const t = tabs.find((x) => x.id === id);
      if (t && !t.internal) {
        t.loading = true;
        t.progress = 0.1;
        t.crashed = false;
        emit('tab:updated', info(t));
        finishLoad(t);
      }
    },
    nav_stop: () => {},
    layout_set_insets: () => {},
    overlay_capture: () => renderSnapshot(),
    overlay_hide_content: () => {},
    overlay_show_content: () => {},

    suggest_query: ({ text }): Suggestion[] => {
      const q = String(text).trim().toLowerCase();
      if (!q) return [];
      const c = classify(q);
      const top: Suggestion = c.url.includes('google.com/search')
        ? { kind: 'search', title: text, url: c.url }
        : { kind: 'navigate', title: c.url, url: c.url };
      const hostHit = SITES.find(([u]) => hostOf(u).replace(/^www\./, '').startsWith(q));
      const rows: Suggestion[] = [];
      if (hostHit && !q.includes(' ')) {
        const bare = hostOf(hostHit[0]).replace(/^www\./, '') + '/';
        rows.push({ kind: 'navigate', title: bare, url: hostHit[0], completion: bare.slice(q.length) });
      } else rows.push(top);
      for (const [u, t] of SITES) {
        if (rows.length >= 4) break;
        if ((u + t).toLowerCase().includes(q) && !rows.some((r) => r.url === u)) rows.push({ kind: 'history', title: t, url: u });
      }
      const localCount = rows.length;
      setTimeout(() => {
        const remote = [q, `${q} meaning`, `${q} near me`, `${q} 2026`].slice(0, 8 - localCount);
        const merged = [
          ...rows,
          ...remote
            .filter((r) => !(rows[0].kind === 'search' && r === q))
            .map((r) => ({ kind: 'search' as const, title: r, url: `https://www.google.com/search?q=${encodeURIComponent(r)}` })),
        ].slice(0, 8);
        emit('suggest:results', { text, rows: merged });
      }, 120);
      return rows;
    },
    suggest_remove: () => {},
    history_search: ({ query }) =>
      history.filter((h) => !query || (h.url + h.title).toLowerCase().includes(String(query).toLowerCase())),
    history_delete: ({ visitIds }) => {
      for (const id of visitIds) {
        const i = history.findIndex((h) => h.visitId === id);
        if (i >= 0) history.splice(i, 1);
      }
    },
    clear_browsing_data: () => {},
    top_sites: (): Tile[] =>
      SITES.slice(0, 8).map(([url, title]) => ({ url, title, host: hostOf(url).replace(/^www\./, ''), favicon: mockFavicon(hostOf(url)) })),
    favicon_for: ({ url }) => mockFavicon(hostOf(url)),

    bookmarks_tree: () => roots,
    bookmarks_add: ({ title, url }) => {
      const b = bm(title, url);
      roots.toolbar.children!.push(b);
      emit('bookmarks:changed', null);
      return b.id;
    },
    bookmarks_update: () => emit('bookmarks:changed', null),
    bookmarks_move: () => emit('bookmarks:changed', null),
    bookmarks_remove: ({ id }) => {
      const walk = (n: Bookmark): Bookmark | null => {
        const i = n.children?.findIndex((c) => c.id === id) ?? -1;
        if (i >= 0) return n.children!.splice(i, 1)[0];
        for (const c of n.children ?? []) {
          const r = walk(c);
          if (r) return r;
        }
        return null;
      };
      const removed = walk(roots.toolbar) ?? walk(roots.other);
      emit('bookmarks:changed', null);
      return removed;
    },
    bookmarks_restore: ({ subtree }) => {
      roots.toolbar.children!.push(subtree);
      emit('bookmarks:changed', null);
      return subtree.id;
    },
    bookmarks_for_url: ({ url }) => {
      const out: Bookmark[] = [];
      const walk = (n: Bookmark) => {
        if (n.url === url) out.push(n);
        n.children?.forEach(walk);
      };
      Object.values(roots).forEach(walk);
      return out;
    },
    bookmarks_import_html: () => 0,
    bookmarks_export_html: () => null,

    passwords_list: () => logins,
    passwords_for_tab: ({ id }) => logins.filter((l) => tabs.find((t) => t.id === id)?.url.startsWith(l.origin)),
    passwords_reveal: () => 'correct horse battery staple',
    passwords_delete: ({ id }) => {
      const i = logins.findIndex((l) => l.id === id);
      if (i >= 0) logins.splice(i, 1);
    },
    passwords_export: () => null,
    passwords_import_csv: () => null,
    autofill_fill: () => {},

    ext_list: () => extensions,
    ext_prepare_store: ({ id }) => ({
      token: 1,
      id,
      name: 'Dark Reader',
      version: '4.9.99',
      description: 'Dark mode for every website.',
      icon: mockFavicon('darkreader'),
      warnings: ['Read and change all your data on all websites'],
      compatibility: 'ok',
      source: 'chrome-web-store',
    }),
    ext_prepare_file: () => null,
    ext_confirm: () => {
      const e: ExtensionInfo = { id: 'eimadpbcbfnmbkopoojfekhnkhdbieeh', name: 'Dark Reader', version: '4.9.99', description: '', enabled: true, pinned: true, source: 'chrome-web-store', icon: mockFavicon('darkreader'), hasAction: true, popup: 'ui/popup/index.html', actionTitle: 'Dark Reader', optionsPage: null };
      extensions.push(e);
      emit('ext:changed', null);
      return e;
    },
    ext_load_unpacked: () => null,
    ext_remove: ({ id }) => {
      const i = extensions.findIndex((e) => e.id === id);
      if (i >= 0) extensions.splice(i, 1);
      emit('ext:changed', null);
    },
    ext_set_enabled: ({ id, on }) => {
      const e = extensions.find((x) => x.id === id);
      if (e) e.enabled = on;
      emit('ext:changed', null);
    },
    ext_set_pinned: ({ id, on }) => {
      const e = extensions.find((x) => x.id === id);
      if (e) e.pinned = on;
      emit('ext:changed', null);
    },
    ext_open_popup: () => {},
    ext_close_popup: () => {},
    ext_open_options: () => {},

    import_detect: () => [
      { name: 'default-release', path: 'C:\\Users\\Yida\\AppData\\Roaming\\Mozilla\\Firefox\\Profiles\\abcd.default-release', isDefault: true, running: false, hasPasswords: true, hasHistory: true },
    ],
    import_choose_folder: () => null,
    import_run: async ({ args }) => {
      const types: string[] = args.types;
      for (const t of types) {
        emit('import:progress', { dataType: t, status: 'running', done: 0, total: 100 });
        await new Promise((r) => setTimeout(r, 350));
        emit('import:progress', { dataType: t, status: 'done', done: 100, total: 100 });
      }
      const summary = {
        history: types.includes('history') ? { pages: 18342, visits: 91230 } : null,
        bookmarks: types.includes('bookmarks') ? 214 : null,
        favicons: types.includes('favicons') ? 1203 : null,
        passwords: types.includes('passwords') ? { imported: 57, merged: 0, failed: 0, skipped: 1 } : null,
        cookies: types.includes('cookies') ? 1840 : null,
        formHistory: types.includes('formHistory') ? 320 : null,
        tabs: types.includes('tabs') ? 6 : null,
        addons: types.includes('addons')
          ? [
              { id: 'uBlock0@raymondhill.net', name: 'uBlock Origin', active: true, chrome: { id: 'ddkjiahejlhfcafbddmgiahcphecmpfh', name: 'uBlock Origin Lite' }, firefoxOnly: false, searchUrl: '' },
              { id: 'addon@darkreader.org', name: 'Dark Reader', active: true, chrome: { id: 'eimadpbcbfnmbkopoojfekhnkhdbieeh', name: 'Dark Reader' }, firefoxOnly: false, searchUrl: '' },
              { id: '@testpilot-containers', name: 'Firefox Multi-Account Containers', active: true, firefoxOnly: true, searchUrl: '' },
            ]
          : [],
        errors: [],
        notImported: ['Site permissions and settings', 'Downloads list', 'Certificates', 'Search engines (Limbo always uses Google)'],
        needsPrimaryPassword: false,
        firefoxRunning: false,
      };
      emit('import:finished', summary);
      return summary;
    },

    downloads_list: () => downloads,
    downloads_choose_folder: () => null,
    downloads_control: () => {},
    downloads_open: () => {},
    downloads_show_in_folder: () => {},
    downloads_remove: ({ id }) => {
      const i = downloads.findIndex((d) => d.id === id);
      if (i >= 0) downloads.splice(i, 1);
    },
    downloads_clear: () => {
      const n = downloads.length;
      downloads.length = 0;
      return n;
    },
    site_permissions: () => [],
    site_permission_revoke: () => {},
    settings_get: () => settings,
    settings_set: ({ patch }) => {
      settings = { ...settings, ...patch };
      emit('settings:changed', settings);
      return settings;
    },
    find_start: ({ id, text }) => emit('find:result', { tabId: id, matches: text ? 3 + (text.length % 5) : 0, active: text ? 1 : 0 }),
    find_step: ({ id }) => emit('find:result', { tabId: id, matches: 4, active: 2 }),
    find_stop: () => {},
    zoom_step: ({ id, step }) => {
      const t = tabs.find((x) => x.id === id);
      if (!t) return;
      const levels = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];
      const i = levels.indexOf(t.zoom);
      t.zoom = step === 'reset' ? 1 : levels[Math.max(0, Math.min(levels.length - 1, i + (step === 'in' ? 1 : -1)))];
      emit('tab:updated', info(t));
      emit('zoom', { id, factor: t.zoom, default: 1 });
    },
    window_minimize: () => {},
    window_toggle_maximize: () => {},
    window_close: () => {},
    window_toggle_fullscreen: () => {},
    window_snap_layouts: () => {},
    memory_set_sampling: ({ mode }) => {
      if (samplingTimer) clearInterval(samplingTimer);
      samplingTimer = mode === 'off' ? null : setInterval(() => emit('memory:sample', memorySample()), mode === 'popover' ? 2000 : 10000);
      emit('memory:sample', memorySample());
    },
    memory_sleep_tab: ({ id }) => {
      const t = tabs.find((x) => x.id === id);
      if (t && t.id !== activeId) t.state = 'suspended';
      states();
    },
    memory_unload_tab: ({ id }) => {
      const t = tabs.find((x) => x.id === id);
      if (t && t.id !== activeId) t.state = 'discarded';
      states();
    },
    memory_sleep_all: () => {
      tabs.forEach((t) => t.id !== activeId && t.state !== 'discarded' && (t.state = 'suspended'));
      states();
    },
    memory_set_max_awake: ({ max }) => {
      settings = { ...settings, memory: { ...settings.memory, maxAwake: max, preset: 'custom' } };
      emit('settings:changed', settings);
      return settings;
    },
    memory_set_preset: ({ preset }) => {
      settings = { ...settings, memory: { ...(PRESETS[preset] ?? PRESETS.balanced) } };
      emit('settings:changed', settings);
      return settings;
    },
    print_page: () => {},
    devtools_open: () => {},
    default_browser_open: () => {},
    default_browser_status: () => false,
  };

  // Dev/test hook: lets Playwright (and the console) play the host's part.
  (window as unknown as { __limboMock: unknown }).__limboMock = {
    emit,
    tabs,
    responses,
    settings: () => settings,
    invoke: (cmd: string, args: Record<string, unknown> = {}) => commands[cmd](args),
  };

  return {
    async invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
      const f = commands[cmd];
      if (!f) throw new Error(`mock: unknown command ${cmd}`);
      return (await f(args as Record<string, any>)) as T;
    },
    listen<T>(event: string, handler: (payload: T) => void) {
      let set = handlers.get(event);
      if (!set) handlers.set(event, (set = new Set()));
      set.add(handler as Handler);
      return () => set!.delete(handler as Handler);
    },
  };
}
