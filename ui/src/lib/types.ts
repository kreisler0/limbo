// Mirrors the host's serde types (src-tauri/src, crates/limbo-core). camelCase.

export type TabId = number;
export type TabState = 'active' | 'hidden' | 'lowMemory' | 'suspended' | 'discarded';
export type InternalPage = 'newTab' | 'settings' | 'history' | 'bookmarks' | 'downloads' | 'extensions' | 'passwords';
export type Security = 'secure' | 'insecure' | 'internal' | 'file';
export type StoreName = 'chromeWebStore' | 'edgeAddons';

export interface TabInfo {
  id: TabId;
  url: string;
  title: string;
  favicon: string | null;
  loading: boolean;
  progress: number;
  canGoBack: boolean;
  canGoForward: boolean;
  audible: boolean;
  muted: boolean;
  pinned: boolean;
  private: boolean;
  state: TabState;
  security: Security;
  internal: InternalPage | null;
  zoom: number;
  crashed: boolean;
  hasSnapshot: boolean;
  displayHost: string;
  searchTerms: string | null;
  storeExtension: { store: StoreName; id: string; installed: boolean } | null;
}

export type Preset = 'balanced' | 'aggressive' | 'off' | 'custom';

export interface Policy {
  preset: Preset;
  hiddenToLowMs: number | null;
  lowToSuspendMs: number | null;
  suspendToDiscardMs: number | null;
  maxAwake: number | null;
  discardPinned: boolean;
}

export type Theme = 'system' | 'light' | 'dark';

export interface Settings {
  theme: Theme;
  accent: string | null;
  memory: Policy;
  showMemoryInToolbar: boolean;
  showGoogleSuggestions: boolean;
  restoreTabsOnStartup: boolean;
  downloadsFolder: string | null;
  askWhereToSave: boolean;
  bookmarksBarVisible: boolean;
  tabLayout: 'top' | 'sidebar';
  defaultZoom: number;
  developerMode: boolean;
  experimentalArgs: string[];
  experimentalArgsEnabled: boolean;
  smartscreenEnabled: boolean;
  mica: boolean;
  onboardingDone: boolean;
  offerToSavePasswords: boolean;
}

export interface WindowState {
  maximized: boolean;
  fullscreen: boolean;
  focused: boolean;
  minimized: boolean;
}

export interface AppState {
  tabs: TabInfo[];
  activeId: TabId | null;
  settings: Settings;
  platform: { windows11: boolean; mica: boolean; version: string; engineVersion: string | null };
  window: WindowState;
  firstRun: boolean;
  isDefaultBrowser: boolean;
}

export type SuggestionKind = 'navigate' | 'search' | 'history' | 'bookmark' | 'openTab' | 'internal';

export interface Suggestion {
  kind: SuggestionKind;
  title: string;
  url: string;
  completion?: string;
  tabId?: TabId;
}

export interface HistoryEntry {
  visitId: number;
  placeId: number;
  url: string;
  title: string;
  visitUs: number;
}

export interface Tile {
  url: string;
  title: string;
  host: string;
  favicon: string | null;
}

export type BookmarkKind = 'url' | 'folder' | 'separator';

export interface Bookmark {
  id: number;
  parentId: number | null;
  kind: BookmarkKind;
  title: string;
  url: string | null;
  position: number;
  addedUs: number;
  modifiedUs: number;
  guid: string;
  children?: Bookmark[];
}

export interface BookmarkRoots {
  toolbar: Bookmark;
  menu: Bookmark;
  other: Bookmark;
  mobile: Bookmark;
}

export interface LoginSummary {
  id: number;
  guid: string;
  origin: string;
  actionOrigin: string | null;
  realm: string | null;
  username: string;
  createdUs: number;
  lastUsedUs: number | null;
  changedUs: number | null;
  timesUsed: number;
  sameSiteOnly?: boolean;
}

export interface ExtensionInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  enabled: boolean;
  pinned: boolean;
  source: string;
  icon: string | null;
  hasAction: boolean;
  popup: string | null;
  actionTitle: string | null;
  optionsPage: string | null;
}

export type Compatibility = 'ok' | 'mv2Deprecated' | 'firefoxOnly';

export interface InstallReview {
  token: number;
  id: string | null;
  name: string;
  version: string;
  description: string;
  icon: string | null;
  warnings: string[];
  compatibility: Compatibility;
  source: string;
}

export interface FirefoxProfile {
  name: string;
  path: string;
  isDefault: boolean;
  running: boolean;
  hasPasswords: boolean;
  hasHistory: boolean;
}

export type ImportType = 'history' | 'bookmarks' | 'favicons' | 'passwords' | 'cookies' | 'formHistory' | 'tabs' | 'addons';

export interface FirefoxAddon {
  id: string;
  name: string;
  active: boolean;
  chrome?: { id: string; name: string };
  firefoxOnly: boolean;
  searchUrl: string;
}

export interface ImportSummary {
  history: { pages: number; visits: number } | null;
  bookmarks: number | null;
  favicons: number | null;
  passwords: { imported: number; merged: number; failed: number; skipped: number } | null;
  cookies: number | null;
  formHistory: number | null;
  tabs: number | null;
  addons: FirefoxAddon[];
  errors: { dataType: ImportType; message: string }[];
  notImported: string[];
  needsPrimaryPassword: boolean;
  firefoxRunning: boolean;
}

export interface ImportProgress {
  dataType: ImportType;
  status: 'running' | 'done' | 'failed';
  done: number;
  total: number;
}

export type DownloadState = 'inProgress' | 'paused' | 'completed' | 'interrupted' | 'cancelled';

export interface Download {
  id: number;
  tabId: TabId | null;
  url: string;
  path: string;
  fileName: string;
  received: number;
  total: number | null;
  state: DownloadState;
  danger: string | null;
  canResume: boolean;
}

export interface MemoryRow {
  kind: 'tab' | 'extension' | 'engine';
  id: string;
  bytes: number;
  shared: boolean;
  state?: 'active' | 'awake' | 'sleeping' | 'unloaded';
}

export interface MemorySample {
  totalBytes: number;
  systemAvailBytes: number;
  systemTotalBytes: number;
  awake: number;
  maxAwake: number | null;
  rows: MemoryRow[];
}

export type PressureLevel = 'normal' | 'elevated' | 'critical';

export interface MenuItem {
  id: number;
  label: string;
  kind: 'item' | 'checkbox' | 'separator';
  enabled: boolean;
  checked: boolean;
  shortcut: string | null;
}

export interface ContextMenuRequest {
  menuId: number;
  tabId: TabId;
  x: number;
  y: number;
  items: MenuItem[];
}

export interface PermissionPrompt {
  requestId: number;
  tabId: TabId;
  origin: string;
  kind: string;
}

export interface SavePasswordPrompt {
  promptId: number;
  tabId: TabId;
  origin: string;
  username: string;
  update: boolean;
}

export interface SitePermission {
  origin: string;
  kind: string;
  allow: boolean;
  updatedUs: number;
}

export interface FindResult {
  tabId: TabId;
  matches: number;
  active: number;
}

export type Shortcut =
  | 'newTab'
  | 'newPrivateTab'
  | 'closeTab'
  | 'reopenClosedTab'
  | 'nextTab'
  | 'prevTab'
  | 'tab1'
  | 'tab2'
  | 'tab3'
  | 'tab4'
  | 'tab5'
  | 'tab6'
  | 'tab7'
  | 'tab8'
  | 'tabLast'
  | 'reload'
  | 'hardReload'
  | 'back'
  | 'forward'
  | 'zoomIn'
  | 'zoomOut'
  | 'zoomReset'
  | 'fullscreen'
  | 'focusOmnibox'
  | 'commandPalette'
  | 'find'
  | 'findNext'
  | 'findPrev'
  | 'bookmark'
  | 'history'
  | 'downloads'
  | 'toggleBookmarksBar'
  | 'toggleSidebar'
  | 'tabOverview'
  | 'memoryPopover'
  | 'clearBrowsingData'
  | 'escape';

export interface Insets {
  top: number;
  left: number;
  right: number;
  bottom: number;
}
