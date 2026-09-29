// Shortcuts while the UI (not a page) has focus. The same table lives in
// src-tauri/src/shortcuts.rs for when a web page has focus.

import type { Shortcut } from './types';

export function classify(e: KeyboardEvent): Shortcut | null {
  const ctrl = e.ctrlKey || e.metaKey;
  const { shiftKey: shift, altKey: alt } = e;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (ctrl && !shift && !alt) {
    switch (k) {
      case 't': return 'newTab';
      case 'w': case 'F4': return 'closeTab';
      case 'l': case 'e': return 'focusOmnibox';
      case 'k': return 'commandPalette';
      case 'r': return 'reload';
      case 'f': return 'find';
      case 'g': return 'findNext';
      case 'd': return 'bookmark';
      case 'h': return 'history';
      case 'j': return 'downloads';
      case 'b': return 'toggleSidebar';
      case 'Tab': case 'PageDown': return 'nextTab';
      case 'PageUp': return 'prevTab';
      case 'F5': return 'hardReload';
      case '=': case '+': return 'zoomIn';
      case '-': return 'zoomOut';
      case '0': return 'zoomReset';
      case '9': return 'tabLast';
    }
    if (k >= '1' && k <= '8') return `tab${k}` as Shortcut;
    return null;
  }
  if (ctrl && shift && !alt) {
    switch (k) {
      case 't': return 'reopenClosedTab';
      case 'n': return 'newPrivateTab';
      case 'r': return 'hardReload';
      case 'b': return 'toggleBookmarksBar';
      case 'a': return 'tabOverview';
      case 'm': return 'memoryPopover';
      case 'g': return 'findPrev';
      case 'Tab': return 'prevTab';
      case 'Delete': return 'clearBrowsingData';
      case '+': return 'zoomIn';
    }
    return null;
  }
  if (alt && !ctrl && !shift) {
    if (k === 'ArrowLeft') return 'back';
    if (k === 'ArrowRight') return 'forward';
    if (k === 'd') return 'focusOmnibox';
    return null;
  }
  if (!ctrl && !alt) {
    if (!shift && k === 'F5') return 'reload';
    if (!shift && k === 'F6') return 'focusOmnibox';
    if (!shift && k === 'F11') return 'fullscreen';
    if (k === 'F3') return shift ? 'findPrev' : 'findNext';
    if (!shift && k === 'Escape') return 'escape';
  }
  return null;
}

/** Actions the host performs itself (tab management, navigation, zoom). */
export const HOST_ACTIONS: ReadonlySet<Shortcut> = new Set<Shortcut>([
  'newTab', 'newPrivateTab', 'closeTab', 'reopenClosedTab', 'nextTab', 'prevTab',
  'tab1', 'tab2', 'tab3', 'tab4', 'tab5', 'tab6', 'tab7', 'tab8', 'tabLast',
  'reload', 'hardReload', 'back', 'forward', 'zoomIn', 'zoomOut', 'zoomReset', 'fullscreen',
]);

export const LABELS: Partial<Record<Shortcut, string>> = {
  newTab: 'Ctrl+T',
  newPrivateTab: 'Ctrl+Shift+N',
  closeTab: 'Ctrl+W',
  reopenClosedTab: 'Ctrl+Shift+T',
  focusOmnibox: 'Ctrl+L',
  commandPalette: 'Ctrl+K',
  find: 'Ctrl+F',
  bookmark: 'Ctrl+D',
  history: 'Ctrl+H',
  downloads: 'Ctrl+J',
  toggleSidebar: 'Ctrl+B',
  toggleBookmarksBar: 'Ctrl+Shift+B',
  tabOverview: 'Ctrl+Shift+A',
  memoryPopover: 'Ctrl+Shift+M',
  clearBrowsingData: 'Ctrl+Shift+Del',
  zoomIn: 'Ctrl+=',
  zoomOut: 'Ctrl+−',
  zoomReset: 'Ctrl+0',
  fullscreen: 'F11',
  reload: 'Ctrl+R',
};
