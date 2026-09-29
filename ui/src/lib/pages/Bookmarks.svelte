<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import type { Bookmark } from '../types';

  let query = $state('');
  let folderId = $state<number | null>(null);
  let editing = $state<number | null>(null);
  let editTitle = $state('');
  let editUrl = $state('');

  const roots = $derived(browser.bookmarks ? [browser.bookmarks.toolbar, browser.bookmarks.menu, browser.bookmarks.other, browser.bookmarks.mobile] : []);

  function find(nodes: Bookmark[], id: number): Bookmark | null {
    for (const n of nodes) {
      if (n.id === id) return n;
      const hit = n.children && find(n.children, id);
      if (hit) return hit;
    }
    return null;
  }

  function flat(nodes: Bookmark[], out: Bookmark[] = []): Bookmark[] {
    for (const n of nodes) {
      if (n.kind === 'url') out.push(n);
      if (n.children) flat(n.children, out);
    }
    return out;
  }

  function folders(nodes: Bookmark[], depth = 0, out: { b: Bookmark; depth: number }[] = []) {
    for (const n of nodes) {
      if (n.kind !== 'folder') continue;
      out.push({ b: n, depth });
      if (n.children) folders(n.children, depth + 1, out);
    }
    return out;
  }

  const current = $derived(folderId !== null ? find(roots, folderId) : (roots[0] ?? null));
  const items = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (q) return flat(roots).filter((b) => (b.title + ' ' + b.url).toLowerCase().includes(q));
    return (current?.children ?? []).filter((b) => b.kind !== 'separator');
  });

  function open(b: Bookmark, e: MouseEvent) {
    if (b.kind === 'folder') {
      folderId = b.id;
      query = '';
      return;
    }
    if (!b.url) return;
    const t = browser.active;
    if (!t || e.ctrlKey || e.button === 1) void api.tabs.create(b.url, { background: true });
    else void api.nav.url(t.id, b.url);
  }

  function startEdit(b: Bookmark) {
    editing = b.id;
    editTitle = b.title;
    editUrl = b.url ?? '';
  }

  async function saveEdit(b: Bookmark) {
    await api.bookmarks.update(b.id, editTitle, b.kind === 'url' ? editUrl : undefined);
    editing = null;
  }

  async function remove(b: Bookmark) {
    const removed = await api.bookmarks.remove(b.id);
    browser.toast(b.kind === 'folder' ? 'Folder deleted' : 'Bookmark deleted', { label: 'Undo', run: () => void api.bookmarks.restore(removed) });
  }

  async function newFolder() {
    const parent = current?.id;
    const id = await api.bookmarks.add('New folder', null, parent, 'folder');
    await browser.refreshBookmarks();
    const b = find(roots, id);
    if (b) startEdit(b);
  }
</script>

<div class="page">
  <aside>
    <h1>Bookmarks</h1>
    {#each folders(roots) as f (f.b.id)}
      <button class:on={!query && current?.id === f.b.id} style:padding-left="{10 + f.depth * 14}px" onclick={() => { folderId = f.b.id; query = ''; }}>
        <Icon name={f.depth === 0 && f.b.id === roots[0]?.id ? 'star' : 'folder'} size={14} />
        <span class="fade-out">{f.b.title}</span>
      </button>
    {/each}
  </aside>
  <main>
    <header>
      <label class="search">
        <Icon name="search" size={15} />
        <input placeholder="Search bookmarks" bind:value={query} aria-label="Search bookmarks" />
      </label>
      <button class="btn" onclick={newFolder} disabled={!!query}><Icon name="folder" size={14} /> New folder</button>
    </header>
    <h2>{query ? 'Search results' : current?.title}</h2>
    <ul>
      {#each items as b (b.id)}
        <li>
          {#if editing === b.id}
            <form class="edit" onsubmit={(e) => { e.preventDefault(); void saveEdit(b); }}>
              <input class="input" bind:value={editTitle} aria-label="Name" />
              {#if b.kind === 'url'}<input class="input url" bind:value={editUrl} aria-label="URL" />{/if}
              <button class="btn primary" type="submit">Save</button>
              <button class="btn ghost" type="button" onclick={() => (editing = null)}>Cancel</button>
            </form>
          {:else}
            <button class="hit" onclick={(e) => open(b, e)} onauxclick={(e) => open(b, e)} ondblclick={() => startEdit(b)}>
              <Icon name={b.kind === 'folder' ? 'folder' : 'globe'} size={15} />
              <span class="title fade-out">{b.title || b.url}</span>
              {#if b.url}<span class="url">{b.url.replace(/^https?:\/\/(www\.)?/, '').replace(/\/$/, '')}</span>{/if}
            </button>
            <button class="icon-btn" aria-label="Edit" onclick={() => startEdit(b)}><Icon name="settings" size={14} /></button>
            <button class="icon-btn" aria-label="Delete" onclick={() => remove(b)}><Icon name="trash" size={14} /></button>
          {/if}
        </li>
      {:else}
        <p class="empty">{query ? 'No matches' : 'This folder is empty. Press Ctrl+D on any page to bookmark it.'}</p>
      {/each}
    </ul>
  </main>
</div>

<style>
  .page {
    display: flex;
    height: 100%;
    background: var(--bg);
  }
  aside {
    width: 232px;
    flex: none;
    padding: 28px 12px;
    overflow-y: auto;
  }
  h1 {
    margin: 0 10px 16px;
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
  }
  aside button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-md);
    text-align: left;
  }
  aside button:hover {
    background: var(--bg-hover);
  }
  aside button.on {
    background: var(--bg-active);
    color: var(--text);
  }
  main {
    flex: 1;
    min-width: 0;
    padding: 28px 32px 48px;
    overflow-y: auto;
  }
  header {
    display: flex;
    gap: 8px;
    max-width: 760px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
    color: var(--text-muted);
  }
  .search input {
    flex: 1;
    border: 0;
    background: none;
    color: var(--text);
    font-size: var(--text-sm);
  }
  h2 {
    margin: 24px 8px 8px;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }
  ul {
    max-width: 760px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 36px;
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }
  li:hover {
    background: var(--bg-hover);
  }
  .hit {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
    height: 36px;
    padding: 0 6px;
    border: 0;
    background: none;
    color: var(--text);
    font-size: var(--text-md);
    text-align: left;
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
  }
  .url {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  li .icon-btn {
    opacity: 0;
  }
  li:hover .icon-btn,
  li .icon-btn:focus-visible {
    opacity: 1;
  }
  .edit {
    display: flex;
    gap: 6px;
    flex: 1;
    padding: 4px 0;
  }
  .edit .input {
    flex: 1;
  }
  .edit .url {
    flex: 2;
  }
  .empty {
    padding: 32px 8px;
    color: var(--text-muted);
  }
</style>
