// The snapshot technique (plan 5.2): a tab webview is a native window above
// this UI, so anything that must cover the page first swaps the live page for
// a picture of it:
//   capture (JPEG) -> show it exactly where the page is -> hide the webview
//   -> animate the overlay -> on close, show the webview and drop the picture.

import { api } from '../ipc';
import { browser } from './browser.svelte';

const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

class OverlayController {
  /** Object URL of the page snapshot while an overlay is open. */
  snapshot = $state<string | null>(null);
  /** Snapshot shown and the live page hidden: dim/blur it. */
  covering = $state(false);
  private open = new Set<string>();
  private busy: Promise<void> = Promise.resolve();

  /** Is a live web page currently showing under the chrome? */
  private pageVisible(): boolean {
    const t = browser.active;
    return !!t && !t.internal && !t.crashed && t.state !== 'discarded';
  }

  isOpen(kind: string) {
    return this.open.has(kind);
  }

  /** Call before showing an overlay that covers the page area. */
  cover(kind: string): Promise<void> {
    const first = this.open.size === 0;
    this.open.add(kind);
    if (!first || !this.pageVisible()) return this.busy;
    this.busy = (async () => {
      const started = performance.now();
      const url = await api.overlay.capture().catch(() => null);
      if (!this.open.has(kind)) {
        if (url) URL.revokeObjectURL(url);
        return;
      }
      if (url) {
        this.snapshot = url;
        // Make sure the picture is decoded and painted before the page goes.
        const img = new Image();
        img.src = url;
        await img.decode().catch(() => {});
        await nextFrame();
        await nextFrame();
      }
      await api.overlay.hideContent();
      this.covering = true;
      const ms = performance.now() - started;
      if (ms > 50 && import.meta.env.DEV) console.debug(`snapshot swap took ${ms.toFixed(0)} ms`);
    })();
    return this.busy;
  }

  /** Call when an overlay closes. */
  async uncover(kind: string) {
    if (!this.open.delete(kind) || this.open.size > 0) return;
    await this.busy;
    if (this.open.size > 0) return;
    this.covering = false;
    if (this.snapshot) {
      await api.overlay.showContent();
      const url = this.snapshot;
      // Keep the picture one more frame so there's never a gap.
      await nextFrame();
      await nextFrame();
      if (this.open.size === 0) {
        this.snapshot = null;
        URL.revokeObjectURL(url);
      }
    } else {
      await api.overlay.showContent();
    }
  }

  /** Close everything (Esc stack emptied, tab switched). */
  async reset() {
    for (const k of [...this.open]) await this.uncover(k);
  }
}

export const overlay = new OverlayController();
