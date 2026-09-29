<script lang="ts">
  // Ctrl+B: tabs, bookmarks and history down the left edge. It docks beside
  // the page (the page area shrinks) rather than floating over it, so no
  // snapshot swap is needed and the page stays live.
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { slide } from '../design/motion';
  import type { Bookmark, HistoryEntry } from '../types';

  let history = $state<HistoryEntry[]>([]);
  let open = $state<Record<number, boolean>>({});

  $effect(() => {
    if (ui.sidebarSection === 'history') api.history.search(undefined, undefined, 40).then((h) => (history = h));
  });

  function go(url: string | null, e?: MouseEvent) {
    if (!url) return;
    const t = browser.active;
    if (!t || e?.ctrlKey || e?.button === 1) void api.tabs.create(url, { background: !!t });
    else void api.nav.url(t.id, url);
  }

  const sections = [
    { id: 'tabs', label: 'Tabs', icon: 'grid' },
    { id: 'bookmarks', label: 'Bookmarks', icon: 'star' },
    { id: 'history', label: 'History', icon: 'history' },
  ] as const;

  function time(us: number) {
    return new Date(us / 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  }
</script>

{#snippet tree(nodes: Bookmark[], depth: number)}
  {#each nodes as b (b.id)}
    {#if b.kind === 'folder'}
      <button class="row" style:padding-left="{10 + depth * 14}px" onclick={() => (open[b.id] = !open[b.id])} aria-expanded={!!open[b.id]}>
        <span class="chev" class:open={open[b.id]}><Icon name="chevronRight" size={12} /></span>
        <Icon name="folder" size={14} />
        <span class="fade-out">{b.title}</span>
      </button>
      {#if open[b.id] && b.children}
        {@render tree(b.children, depth + 1)}
      {/if}
    {:else if b.kind === 'url'}
      <button class="row" style:padding-left="{28 + depth * 14}px" title={b.url} onclick={(e) => go(b.url, e)} onauxclick={(e) => go(b.url, e)}>
        <Icon name="globe" size={14} />
        <span class="fade-out">{b.title || b.url}</span>
      </button>
    {/if}
  {/each}
{/snippet}

<aside class="sidebar" aria-label="Sidebar" transition:slide={{ x: -24 }}>
  <div class="tabs" role="tablist">
    {#each sections as s (s.id)}
      <button
        class="icon-btn"
        role="tab"
        aria-selected={ui.sidebarSection === s.id}
        class:on={ui.sidebarSection === s.id}
        title={s.label}
        onclick={() => (ui.sidebarSection = s.id)}
      >
        <Icon name={s.icon} size={15} />
      </button>
    {/each}
    <span class="grow"></span>
    <button class="icon-btn" class:on={ui.sidebarPinned} title={ui.sidebarPinned ? 'Unpin sidebar' : 'Keep sidebar open'} onclick={() => (ui.sidebarPinned = !ui.sidebarPinned)}>
      <Icon name="pin" size={14} />
    </button>
    <button class="icon-btn" aria-label="Close sidebar" onclick={() => (ui.sidebarOpen = false)}><Icon name="x" size={14} /></button>
  </div>

  <div class="list">
    {#if ui.sidebarSection === 'tabs'}
      {#each browser.tabs as t (t.id)}
        <div class="row tab" class:active={t.id === browser.activeId} class:asleep={t.state === 'suspended' || t.state === 'discarded'}>
          <button class="hit" onclick={() => api.tabs.activate(t.id)}>
            {#if t.favicon}<img src={t.favicon} alt="" width="14" height="14" />{:else}<Icon name="globe" size={14} />{/if}
            <span class="fade-out">{t.title || t.displayHost || 'New Tab'}</span>
          </button>
          <button class="icon-btn x" aria-label="Close tab" onclick={() => api.tabs.close(t.id)}><Icon name="x" size={12} /></button>
        </div>
      {/each}
      <button class="row new" onclick={() => api.tabs.create()}><Icon name="plus" size={14} /> New tab</button>
    {:else if ui.sidebarSection === 'bookmarks'}
      {#if browser.bookmarks}
        {@render tree([browser.bookmarks.toolbar, browser.bookmarks.menu, browser.bookmarks.other].filter((r) => r.children?.length), 0)}
      {/if}
    {:else}
      {#each history as h (h.visitId)}
        <button class="row" title={h.url} onclick={(e) => go(h.url, e)} onauxclick={(e) => go(h.url, e)}>
          <span class="time tabular">{time(h.visitUs)}</span>
          <span class="fade-out">{h.title || h.url}</span>
        </button>
      {:else}
        <p class="empty">No history yet</p>
      {/each}
      <button class="row new" onclick={() => go('limbo://history')}><Icon name="history" size={14} /> All history</button>
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    width: var(--sidebar-width);
    flex: none;
    background: var(--bg-subtle);
    border-right: 1px solid var(--divider);
    overflow: hidden;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 8px;
  }
  .grow {
    flex: 1;
  }
  .on {
    background: var(--bg-active);
    color: var(--text);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 2px 6px 12px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.tab {
    padding: 0 4px 0 0;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 10px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .hit img {
    border-radius: 3px;
  }
  .row.active {
    background: var(--bg);
    box-shadow: var(--shadow-tab);
  }
  .asleep .hit {
    color: var(--text-muted);
  }
  .x {
    width: 22px;
    height: 22px;
    opacity: 0;
  }
  .row.tab:hover .x,
  .row.active .x {
    opacity: 1;
  }
  .new {
    color: var(--text-muted);
  }
  .chev {
    display: inline-flex;
    color: var(--text-muted);
    transition: transform var(--dur-snappy) var(--ease-snappy);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .time {
    flex: none;
    width: 58px;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .empty {
    padding: 12px 10px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
