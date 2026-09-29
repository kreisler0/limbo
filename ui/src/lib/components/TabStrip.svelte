<script lang="ts">
  import { flip as flipAnimate } from 'svelte/animate';
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser, formatMB } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { pop, springs, prefersReducedMotion } from '../design/motion';
  import type { TabInfo } from '../types';

  const PINNED_W = 36;
  const MAX_W = 200;
  /** The active tab keeps at least this much so its title stays readable. */
  const ACTIVE_MIN = 150;
  /** Below this, background tabs show only their favicon. */
  const COMPACT_BELOW = 64;
  const ICON_MAX = 40;
  const ICON_MIN = 28;
  const GAP = 2;

  interface Layout {
    active: number;
    other: number;
    compact: boolean;
  }

  let strip = $state<HTMLDivElement>();
  let available = $state(600);
  /** Widths frozen while closing tabs with the mouse (the next close button stays put). */
  let frozen = $state<Layout | null>(null);

  let dragId = $state<number | null>(null);
  let dragX = $state(0);
  let grabOffset = 0;
  let startX = 0;
  let pressId: number | null = null;

  $effect(() => {
    if (!strip) return;
    const ro = new ResizeObserver(([e]) => (available = e.contentRect.width));
    ro.observe(strip);
    return () => ro.disconnect();
  });

  const pinned = $derived(browser.tabs.filter((t) => t.pinned).length);
  const normal = $derived(browser.tabs.length - pinned);
  const activeIsNormal = $derived(!!browser.active && !browser.active.pinned);

  // Split the strip: an even share when it fits; otherwise the active tab keeps
  // a readable title and the others shrink, down to favicons (then the tab
  // overview button appears as the way to reach everything).
  const computed = $derived.by((): Layout => {
    const reserved = 36 + (browser.tabs.length > 8 ? 34 : 0); // new tab (+ overview) buttons
    const space = available - reserved - pinned * (PINNED_W + GAP);
    if (normal === 0) return { active: MAX_W, other: MAX_W, compact: false };
    const even = Math.min(MAX_W, Math.floor(space / normal) - GAP);
    if (!activeIsNormal || normal === 1 || even >= ACTIVE_MIN) {
      const w = Math.max(ICON_MIN, even);
      return { active: w, other: w, compact: w < COMPACT_BELOW };
    }
    const active = Math.min(ACTIVE_MIN, space - GAP);
    const other = Math.floor((space - active - GAP) / (normal - 1)) - GAP;
    if (other >= COMPACT_BELOW) return { active, other, compact: false };
    const icon = Math.max(ICON_MIN, Math.min(ICON_MAX, other));
    const rest = space - (normal - 1) * (icon + GAP) - GAP;
    return { active: Math.min(MAX_W, Math.max(icon, rest)), other: icon, compact: true };
  });
  const layout = $derived(frozen ?? computed);
  const compact = $derived(layout.compact);
  const activeTitled = $derived(layout.active >= COMPACT_BELOW);
  const crowded = $derived(browser.tabs.length > 8 && compact);

  function tip(t: TabInfo): string {
    const mem = browser.memoryForTab(t.id);
    const state = t.state === 'suspended' ? ' · sleeping' : t.state === 'discarded' ? ' · unloaded' : '';
    const host = t.internal ? '' : `\n${t.displayHost}`;
    return `${t.title || t.displayHost || 'New Tab'}${host}${mem !== null && mem > 0 ? ` · ${formatMB(mem)}` : ''}${state}`;
  }

  function activate(t: TabInfo) {
    if (t.id !== browser.activeId) void api.tabs.activate(t.id);
  }

  function close(t: TabInfo, e?: Event) {
    e?.stopPropagation();
    // Keep widths so the next close button lands under the pointer.
    if (e && !t.pinned) frozen = computed;
    void api.tabs.close(t.id);
  }

  function onPointerDown(e: PointerEvent, t: TabInfo) {
    if (e.button === 1) {
      e.preventDefault();
      return;
    }
    if (e.button !== 0) return;
    pressId = t.id;
    startX = e.clientX;
    const el = e.currentTarget as HTMLElement;
    grabOffset = e.clientX - el.getBoundingClientRect().left;
    el.setPointerCapture(e.pointerId);
    activate(t);
  }

  function onPointerMove(e: PointerEvent, t: TabInfo) {
    if (pressId !== t.id) return;
    if (dragId === null && Math.abs(e.clientX - startX) < 5) return;
    dragId = t.id;
    const el = e.currentTarget as HTMLElement;
    // Where the tab's slot currently is (without the drag transform).
    const slotLeft = el.getBoundingClientRect().left - dragX;
    dragX = e.clientX - grabOffset - slotLeft;
    // Swap with a neighbor once the tab's center crosses the neighbor's middle.
    const tabs = browser.tabs;
    const i = tabs.findIndex((x) => x.id === t.id);
    const center = e.clientX - grabOffset + el.offsetWidth / 2;
    const siblings = Array.from(strip?.querySelectorAll<HTMLElement>('[data-tab]') ?? []);
    for (const [j, s] of siblings.entries()) {
      if (j === i) continue;
      const other = tabs[j];
      if (!other || other.pinned !== t.pinned) continue;
      const r = s.getBoundingClientRect();
      const mid = r.left + r.width / 2;
      if ((j > i && center > mid) || (j < i && center < mid)) {
        const [moved] = tabs.splice(i, 1);
        tabs.splice(j, 0, moved);
        // Re-anchor so the tab stays under the pointer after its slot moved.
        requestAnimationFrame(() => {
          const newLeft = el.getBoundingClientRect().left - dragX;
          dragX = e.clientX - grabOffset - newLeft;
        });
        break;
      }
    }
  }

  function onPointerUp(e: PointerEvent, t: TabInfo) {
    if (pressId === t.id && dragId === t.id) {
      const index = browser.tabs.findIndex((x) => x.id === t.id);
      void api.tabs.move(t.id, index);
    } else if (e.button === 1 && !t.pinned) {
      close(t);
    }
    pressId = null;
    dragId = null;
    dragX = 0;
  }

  function onAuxClick(e: MouseEvent, t: TabInfo) {
    if (e.button === 1) close(t, e);
  }

  function openMenu(e: MouseEvent, t: TabInfo) {
    e.preventDefault();
    ui.tabMenu = { id: t.id, x: e.clientX, y: e.clientY };
    void ui.open('tabMenu');
  }

  const flipParams = $derived({ duration: prefersReducedMotion() ? 0 : springs.smooth.duration, easing: springs.smooth.ease });
</script>

<div
  class="strip"
  class:compact
  bind:this={strip}
  role="tablist"
  aria-label="Tabs"
  tabindex="-1"
  onpointerleave={() => (frozen = null)}
  data-tauri-drag-region
>
  {#each browser.tabs as t (t.id)}
    {@const active = t.id === browser.activeId}
    {@const w = t.pinned ? PINNED_W : active ? layout.active : layout.other}
    <div
      data-tab
      class="tab"
      class:active
      class:pinned={t.pinned}
      class:dragging={dragId === t.id}
      class:asleep={t.state === 'suspended'}
      class:unloaded={t.state === 'discarded' && !t.internal}
      class:private={t.private}
      class:iconOnly={t.pinned || (compact && (!active || !activeTitled))}
      style:width="{w}px"
      style:transform={dragId === t.id ? `translateX(${dragX}px)` : undefined}
      role="tab"
      tabindex={active ? 0 : -1}
      aria-selected={active}
      title={tip(t)}
      animate:flipAnimate={flipParams}
      in:pop={{ from: 0.9, y: 4, origin: 'center left' }}
      onpointerdown={(e) => onPointerDown(e, t)}
      onpointermove={(e) => onPointerMove(e, t)}
      onpointerup={(e) => onPointerUp(e, t)}
      onauxclick={(e) => onAuxClick(e, t)}
      oncontextmenu={(e) => openMenu(e, t)}
      onkeydown={(e) => e.key === 'Enter' && activate(t)}
    >
      <span class="icon">
        {#if t.loading && !t.internal}
          <span class="spinner" aria-hidden="true"></span>
        {:else if t.favicon}
          <img src={t.favicon} alt="" width="16" height="16" />
        {:else if t.internal === 'newTab'}
          <Icon name="plus" size={14} />
        {:else if t.internal}
          <Icon name={t.internal === 'settings' ? 'settings' : t.internal === 'history' ? 'history' : t.internal === 'downloads' ? 'download' : t.internal === 'passwords' ? 'key' : t.internal === 'extensions' ? 'puzzle' : 'bookmark'} size={14} />
        {:else}
          <Icon name="globe" size={14} />
        {/if}
        {#if t.private}
          <span class="badge"><Icon name="glasses" size={9} stroke={1.8} /></span>
        {/if}
      </span>
      {#if !t.pinned && !(compact && (!active || !activeTitled))}
        <span class="title fade-out">{t.title || t.displayHost || 'New Tab'}</span>
      {/if}
      {#if (t.audible || t.muted) && !compact}
        <button
          class="audio"
          aria-label={t.muted ? 'Unmute tab' : 'Mute tab'}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => {
            e.stopPropagation();
            void api.tabs.mute(t.id, !t.muted);
          }}
        >
          <Icon name={t.muted ? 'volumeOff' : 'volume'} size={13} />
        </button>
      {/if}
      {#if !t.pinned && (!compact || active)}
        <button
          class="close"
          aria-label="Close tab"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => close(t, e)}
        >
          <Icon name="x" size={12} stroke={1.6} />
        </button>
      {/if}
    </div>
  {/each}
  <button class="icon-btn new" aria-label="New tab" title="New tab (Ctrl+T)" onclick={() => api.tabs.create()}>
    <Icon name="plus" />
  </button>
  {#if crowded}
    <button class="icon-btn" aria-label="Tab overview" title="All tabs (Ctrl+Shift+A)" onclick={() => ui.toggle('overview')}>
      <Icon name="grid" size={15} />
    </button>
  {/if}
</div>

<style>
  .strip {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    /* Tabs take what's left after the omnibox and buttons, never below ~3 tabs. */
    min-width: 120px;
    height: 100%;
    flex: 1 1 0;
    overflow: hidden;
    padding-right: 4px;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 6px 0 10px;
    border-radius: 8px;
    flex: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    background: transparent;
    transition:
      background-color 120ms ease-out,
      color 120ms ease-out,
      box-shadow 160ms ease-out,
      transform var(--dur-snappy) var(--ease-snappy);
    touch-action: none;
  }
  .tab:hover:not(.active) {
    background: var(--bg-hover);
    color: var(--text);
    transition-duration: 0ms;
  }
  .tab.active {
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-tab);
  }
  .tab.dragging {
    z-index: 2;
    transition: none;
    box-shadow: var(--shadow-float);
  }
  .tab.iconOnly {
    justify-content: center;
    padding: 0;
  }
  .icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    flex: none;
  }
  .icon img {
    width: 16px;
    height: 16px;
    border-radius: 3px;
    image-rendering: auto;
  }
  .asleep .icon img {
    opacity: 0.6;
    transition: opacity 400ms ease;
  }
  .unloaded .icon img {
    opacity: 0.5;
    filter: grayscale(1);
  }
  .badge {
    position: absolute;
    right: -5px;
    bottom: -4px;
    display: grid;
    place-items: center;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    /* Fade inside the tab, not past its edge. */
    margin-right: 0;
  }
  .close,
  .audio {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--text-muted);
    flex: none;
    opacity: 0;
    transition: opacity 120ms ease-out, background-color 120ms ease-out;
  }
  .audio {
    opacity: 1;
  }
  .tab:not(.active) .close {
    position: absolute;
    right: 6px;
  }
  .tab:hover .close,
  .tab.active .close {
    opacity: 1;
    transition-duration: 0ms;
  }
  .close:hover,
  .audio:hover {
    background: var(--bg-active);
    color: var(--text);
  }
  .spinner {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 1.5px solid var(--divider);
    border-top-color: var(--accent);
    animation: spin 700ms linear infinite;
  }
  .new {
    flex: none;
    margin-left: 2px;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 1.6s;
    }
  }
</style>
