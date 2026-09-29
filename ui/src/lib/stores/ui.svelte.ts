// UI-only state: which layers are open. Esc always closes the topmost one.

import { overlay } from './overlay.svelte';

export type Layer =
  | 'omnibox'
  | 'appMenu'
  | 'contextMenu'
  | 'palette'
  | 'memory'
  | 'downloads'
  | 'bookmark'
  | 'siteInfo'
  | 'extensions'
  | 'autofill'
  | 'overview'
  | 'dialog'
  | 'tabMenu';

class UiStore {
  layers = $state<Layer[]>([]);
  sidebarOpen = $state(false);
  sidebarPinned = $state(false);
  sidebarSection = $state<'tabs' | 'bookmarks' | 'history'>('tabs');
  findOpen = $state(false);
  omniboxFocused = $state(false);
  /** Increments to ask the omnibox to focus and select all. */
  focusOmniboxSeq = $state(0);
  dialog = $state<null | { kind: 'clearData' } | { kind: 'install'; review: import('../types').InstallReview } | { kind: 'import' }>(null);
  tabMenu = $state<null | { id: number; x: number; y: number }>(null);
  /** The page area in CSS px; the host reports page coordinates relative to it. */
  content = $state({ x: 0, y: 0, width: 0, height: 0 });

  top = $derived(this.layers.at(-1) ?? null);

  isOpen(layer: Layer) {
    return this.layers.includes(layer);
  }

  /** Opens a layer over the page (snapshot swap included). */
  async open(layer: Layer, coversPage = true) {
    if (!this.layers.includes(layer)) this.layers.push(layer);
    if (coversPage) await overlay.cover(layer);
  }

  close(layer: Layer) {
    this.layers = this.layers.filter((l) => l !== layer);
    void overlay.uncover(layer);
  }

  toggle(layer: Layer, coversPage = true) {
    if (this.isOpen(layer)) this.close(layer);
    else void this.open(layer, coversPage);
  }

  closeTop(): boolean {
    const top = this.top;
    if (!top) return false;
    this.close(top);
    return true;
  }

  closeAll() {
    for (const l of [...this.layers]) this.close(l);
  }

  focusOmnibox() {
    this.focusOmniboxSeq++;
  }
}

export const ui = new UiStore();
