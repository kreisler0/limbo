<script lang="ts">
  import Icon from './Icon.svelte';
  import Menu, { type MenuItemDef } from './Menu.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { Bookmark } from '../types';

  let menu = $state<null | { x: number; y: number; items: MenuItemDef[] }>(null);
  const items = $derived(browser.bookmarks?.toolbar.children ?? []);

  function open(b: Bookmark, e: MouseEvent) {
    if (!b.url) return;
    const t = browser.active;
    if (e.ctrlKey || e.button === 1 || !t) void api.tabs.create(b.url, { background: true });
    else void api.nav.url(t.id, b.url);
  }

  function folderItems(f: Bookmark): MenuItemDef[] {
    const out: MenuItemDef[] = (f.children ?? []).map((c) =>
      c.kind === 'separator'
        ? 'separator'
        : { label: c.title || c.url || 'Untitled', icon: c.kind === 'folder' ? ('folder' as const) : ('globe' as const), run: () => c.url && browser.active && api.nav.url(browser.active.id, c.url) },
    );
    return out.length ? out : [{ label: '(empty)', disabled: true, run: () => {} }];
  }

  function openFolder(f: Bookmark, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: r.left, y: r.bottom + 4, items: folderItems(f) };
    void ui.open('tabMenu');
  }

  function contextFor(b: Bookmark, e: MouseEvent) {
    e.preventDefault();
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        ...(b.url ? [{ label: 'Open in new tab', icon: 'plus' as const, run: () => api.tabs.create(b.url!, { background: true }) }] : []),
        {
          label: 'Delete',
          icon: 'trash' as const,
          danger: true,
          run: async () => {
            const removed = await api.bookmarks.remove(b.id);
            browser.toast('Bookmark deleted', { label: 'Undo', run: () => void api.bookmarks.restore(removed) });
          },
        },
      ],
    };
    void ui.open('tabMenu');
  }
</script>

<nav class="bar" aria-label="Bookmarks bar">
  {#each items as b (b.id)}
    {#if b.kind === 'separator'}
      <span class="sep"></span>
    {:else if b.kind === 'folder'}
      <button class="bm" onclick={(e) => openFolder(b, e)} oncontextmenu={(e) => contextFor(b, e)}>
        <Icon name="folder" size={13} /> <span>{b.title}</span>
      </button>
    {:else}
      <button class="bm" title={b.url} onclick={(e) => open(b, e)} onauxclick={(e) => open(b, e)} oncontextmenu={(e) => contextFor(b, e)}>
        <Icon name="globe" size={13} /> <span>{b.title}</span>
      </button>
    {/if}
  {:else}
    <span class="empty">Bookmarks you add to the bar appear here (Ctrl+D)</span>
  {/each}
</nav>

{#if menu}
  <Menu items={menu.items} x={menu.x} y={menu.y} onclose={() => { menu = null; ui.close('tabMenu'); }} />
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 32px;
    padding: 0 10px;
    background: var(--bg-subtle);
    overflow: hidden;
    flex: none;
  }
  .bm {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 180px;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .bm span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bm:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .sep {
    width: 1px;
    height: 16px;
    margin: 0 4px;
    background: var(--divider);
  }
  .empty {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
</style>
