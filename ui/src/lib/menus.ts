// Actions shared by the app menu, tab menu, command palette and shortcuts.

import { api } from './ipc';
import { browser } from './stores/browser.svelte';
import { ui } from './stores/ui.svelte';
import { HOST_ACTIONS, LABELS } from './shortcuts';
import type { MenuItemDef } from './components/Menu.svelte';
import type { IconName } from './design/icons';
import type { Shortcut, TabId } from './types';

const activeId = () => browser.activeId;

export function openInternal(page: 'settings' | 'history' | 'bookmarks' | 'downloads' | 'extensions' | 'passwords', section?: string) {
  const url = `limbo://${page}${section ? `#${section}` : ''}`;
  const existing = browser.tabs.find((t) => t.internal === page);
  if (existing) {
    void api.tabs.activate(existing.id, false);
    if (section) location.hash = section;
    return;
  }
  const t = browser.active;
  if (t?.internal === 'newTab') void api.nav.url(t.id, url);
  else void api.tabs.create(url);
}

export const actions = {
  newTab: () => api.tabs.create(),
  newPrivateTab: () => api.tabs.create(undefined, { private: true }),
  reopenClosed: () => api.tabs.reopenClosed(),
  history: () => openInternal('history'),
  downloads: () => ui.toggle('downloads'),
  passwords: () => openInternal('settings', 'passwords'),
  extensions: () => openInternal('settings', 'extensions'),
  settings: () => openInternal('settings'),
  memory: () => ui.toggle('memory'),
  importFirefox: () => {
    ui.dialog = { kind: 'import' };
    void ui.open('dialog');
  },
  clearData: () => {
    ui.dialog = { kind: 'clearData' };
    void ui.open('dialog');
  },
  find: () => {
    const t = browser.active;
    if (!t || t.internal) return;
    ui.findOpen = true;
  },
  toggleBookmarksBar: () => browser.updateSettings({ bookmarksBarVisible: !browser.settings?.bookmarksBarVisible }),
  toggleSidebar: () => {
    ui.sidebarOpen = !ui.sidebarOpen;
  },
  overview: () => ui.toggle('overview'),
  palette: () => ui.toggle('palette'),
  print: () => {
    const id = activeId();
    if (id !== null) void api.print(id);
  },
  zoom: (step: 'in' | 'out' | 'reset') => {
    const id = activeId();
    if (id !== null) void api.zoom(id, step);
  },
  fullscreen: () => api.window.toggleFullscreen(),
  devtools: () => {
    const id = activeId();
    if (id !== null) void api.devtools(id);
  },
  bookmarkPage: () => {
    const t = browser.active;
    if (!t || t.internal) return;
    void api.bookmarks.forUrl(t.url).then(async (b) => {
      if (!b.length) await api.bookmarks.add(t.title || t.displayHost, t.url);
      void ui.open('bookmark');
    });
  },
};

export function appMenuItems(): MenuItemDef[] {
  const dev = browser.settings?.developerMode;
  const t = browser.active;
  const zoom = t ? Math.round(t.zoom * 100) : 100;
  const items: MenuItemDef[] = [
    { label: 'New tab', icon: 'plus', shortcut: LABELS.newTab, run: actions.newTab },
    { label: 'New private tab', icon: 'glasses', shortcut: LABELS.newPrivateTab, run: actions.newPrivateTab },
    'separator',
    { label: 'History', icon: 'history', shortcut: LABELS.history, run: actions.history },
    { label: 'Downloads', icon: 'download', shortcut: LABELS.downloads, run: actions.downloads },
    { label: browser.settings?.bookmarksBarVisible ? 'Hide bookmarks bar' : 'Show bookmarks bar', icon: 'bookmark', shortcut: LABELS.toggleBookmarksBar, run: actions.toggleBookmarksBar },
    { label: 'Passwords', icon: 'key', run: actions.passwords },
    { label: 'Extensions', icon: 'puzzle', run: actions.extensions },
    'separator',
    { label: `Zoom in (${zoom}%)`, icon: 'zoomIn', shortcut: LABELS.zoomIn, disabled: !t || !!t.internal, run: () => actions.zoom('in') },
    { label: 'Zoom out', icon: 'minus', shortcut: LABELS.zoomOut, disabled: !t || !!t.internal, run: () => actions.zoom('out') },
    { label: 'Full screen', icon: 'maximize', shortcut: LABELS.fullscreen, run: actions.fullscreen },
    { label: 'Find in page…', icon: 'search', shortcut: LABELS.find, disabled: !t || !!t.internal, run: actions.find },
    { label: 'Print…', icon: 'printer', shortcut: 'Ctrl+P', disabled: !t || !!t.internal, run: actions.print },
    'separator',
    { label: 'Memory saver', icon: 'zap', shortcut: LABELS.memoryPopover, run: actions.memory },
    { label: 'Import from Firefox…', icon: 'upload', run: actions.importFirefox },
    { label: 'Clear browsing data…', icon: 'trash', shortcut: LABELS.clearBrowsingData, run: actions.clearData },
    { label: 'Settings', icon: 'settings', run: actions.settings },
  ];
  if (dev) items.push({ label: 'Developer tools', icon: 'command', shortcut: 'F12', disabled: !t || !!t.internal, run: actions.devtools });
  return items;
}

export function tabMenuItems(id: TabId): MenuItemDef[] {
  const t = browser.tab(id);
  if (!t) return [];
  const idx = browser.tabs.indexOf(t);
  const others = browser.tabs.filter((x) => x.id !== id && !x.pinned);
  const right = browser.tabs.slice(idx + 1).filter((x) => !x.pinned);
  return [
    { label: 'New tab to the right', icon: 'plus', run: () => api.tabs.create(undefined, { index: idx + 1 }) },
    'separator',
    { label: 'Reload', icon: 'rotateCw', disabled: !!t.internal, run: () => api.nav.reload(id) },
    { label: 'Duplicate', icon: 'copy', run: () => api.tabs.duplicate(id) },
    { label: t.pinned ? 'Unpin' : 'Pin', icon: 'pin', run: () => api.tabs.pin(id, !t.pinned) },
    { label: t.muted ? 'Unmute site' : 'Mute site', icon: t.muted ? 'volume' : 'volumeOff', run: () => api.tabs.mute(id, !t.muted) },
    'separator',
    { label: 'Put to sleep', icon: 'moon', disabled: t.id === browser.activeId || t.state === 'suspended' || t.state === 'discarded', run: () => api.memory.sleepTab(id) },
    { label: 'Unload', icon: 'refreshOff', disabled: t.id === browser.activeId || t.state === 'discarded', run: () => api.memory.unloadTab(id) },
    'separator',
    { label: 'Close', icon: 'x', shortcut: LABELS.closeTab, run: () => api.tabs.close(id) },
    { label: 'Close other tabs', disabled: !others.length, run: () => others.forEach((o) => api.tabs.close(o.id)) },
    { label: 'Close tabs to the right', disabled: !right.length, run: () => right.forEach((o) => api.tabs.close(o.id)) },
  ];
}

export interface Command {
  id: string;
  label: string;
  icon: IconName;
  hint?: string;
  run: () => unknown;
}

export function commands(): Command[] {
  return [
    { id: 'new-tab', label: 'New tab', icon: 'plus', hint: LABELS.newTab, run: actions.newTab },
    { id: 'private', label: 'New private tab', icon: 'glasses', hint: LABELS.newPrivateTab, run: actions.newPrivateTab },
    { id: 'reopen', label: 'Reopen closed tab', icon: 'rotateCw', hint: LABELS.reopenClosedTab, run: actions.reopenClosed },
    { id: 'history', label: 'Open history', icon: 'history', hint: LABELS.history, run: actions.history },
    { id: 'downloads', label: 'Show downloads', icon: 'download', hint: LABELS.downloads, run: actions.downloads },
    { id: 'bookmarks-bar', label: 'Toggle bookmarks bar', icon: 'bookmark', hint: LABELS.toggleBookmarksBar, run: actions.toggleBookmarksBar },
    { id: 'sidebar', label: 'Toggle sidebar', icon: 'sidebar', hint: LABELS.toggleSidebar, run: actions.toggleSidebar },
    { id: 'overview', label: 'Tab overview', icon: 'grid', hint: LABELS.tabOverview, run: actions.overview },
    { id: 'memory', label: 'Memory saver', icon: 'zap', hint: LABELS.memoryPopover, run: actions.memory },
    { id: 'sleep-all', label: 'Sleep all background tabs', icon: 'moon', run: () => api.memory.sleepAll() },
    { id: 'passwords', label: 'Passwords', icon: 'key', run: actions.passwords },
    { id: 'extensions', label: 'Extensions', icon: 'puzzle', run: actions.extensions },
    { id: 'import', label: 'Import from Firefox', icon: 'upload', run: actions.importFirefox },
    { id: 'clear', label: 'Clear browsing data', icon: 'trash', hint: LABELS.clearBrowsingData, run: actions.clearData },
    { id: 'find', label: 'Find in page', icon: 'search', hint: LABELS.find, run: actions.find },
    { id: 'zoom-in', label: 'Zoom in', icon: 'zoomIn', hint: LABELS.zoomIn, run: () => actions.zoom('in') },
    { id: 'zoom-out', label: 'Zoom out', icon: 'minus', hint: LABELS.zoomOut, run: () => actions.zoom('out') },
    { id: 'zoom-reset', label: 'Reset zoom', icon: 'zoomIn', hint: LABELS.zoomReset, run: () => actions.zoom('reset') },
    { id: 'fullscreen', label: 'Full screen', icon: 'maximize', hint: LABELS.fullscreen, run: actions.fullscreen },
    { id: 'print', label: 'Print', icon: 'printer', hint: 'Ctrl+P', run: actions.print },
    { id: 'theme-light', label: 'Theme: Light', icon: 'sun', run: () => browser.updateSettings({ theme: 'light' }) },
    { id: 'theme-dark', label: 'Theme: Dark', icon: 'moon', run: () => browser.updateSettings({ theme: 'dark' }) },
    { id: 'theme-system', label: 'Theme: System', icon: 'monitor', run: () => browser.updateSettings({ theme: 'system' }) },
    { id: 'default', label: 'Make Limbo the default browser', icon: 'globe', run: () => api.defaultBrowser.open() },
    { id: 'settings', label: 'Settings', icon: 'settings', run: actions.settings },
  ];
}

/** Handles a shortcut from the host (page focused) or the UI (keydown). */
export function runShortcut(s: Shortcut) {
  if (HOST_ACTIONS.has(s)) {
    void api.shortcut(s);
    return;
  }
  switch (s) {
    case 'focusOmnibox':
      ui.closeAll();
      ui.focusOmnibox();
      break;
    case 'commandPalette':
      actions.palette();
      break;
    case 'find':
      actions.find();
      break;
    case 'findNext':
    case 'findPrev': {
      const id = activeId();
      if (id !== null && ui.findOpen) void api.find.step(id, s === 'findNext');
      else actions.find();
      break;
    }
    case 'bookmark':
      actions.bookmarkPage();
      break;
    case 'history':
      actions.history();
      break;
    case 'downloads':
      actions.downloads();
      break;
    case 'toggleBookmarksBar':
      void actions.toggleBookmarksBar();
      break;
    case 'toggleSidebar':
      actions.toggleSidebar();
      break;
    case 'tabOverview':
      actions.overview();
      break;
    case 'memoryPopover':
      actions.memory();
      break;
    case 'clearBrowsingData':
      actions.clearData();
      break;
    case 'escape':
      if (!ui.closeTop()) {
        if (ui.findOpen) ui.findOpen = false;
        else if (ui.sidebarOpen && !ui.sidebarPinned) ui.sidebarOpen = false;
      }
      break;
  }
}
