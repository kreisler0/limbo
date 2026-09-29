<script lang="ts">
  import Popover from './Popover.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { Bookmark } from '../types';

  let bookmark = $state<Bookmark | null>(null);
  let title = $state('');
  let folder = $state<number | null>(null);

  const t = browser.active;

  $effect(() => {
    if (!t) return;
    api.bookmarks.forUrl(t.url).then((b) => {
      bookmark = b[0] ?? null;
      title = bookmark?.title ?? t.title;
      folder = bookmark?.parentId ?? null;
    });
  });

  const folders = $derived.by(() => {
    const out: { id: number; title: string; depth: number }[] = [];
    const walk = (b: Bookmark, depth: number) => {
      if (b.kind !== 'folder') return;
      out.push({ id: b.id, title: b.title, depth });
      b.children?.forEach((c) => walk(c, depth + 1));
    };
    const r = browser.bookmarks;
    if (r) [r.toolbar, r.other, r.menu].forEach((root) => walk(root, 0));
    return out;
  });

  async function done() {
    if (bookmark) {
      if (title !== bookmark.title) await api.bookmarks.update(bookmark.id, title);
      if (folder !== null && folder !== bookmark.parentId) await api.bookmarks.move(bookmark.id, folder);
    }
    ui.close('bookmark');
  }

  async function remove() {
    if (!bookmark) return;
    const removed = await api.bookmarks.remove(bookmark.id);
    ui.close('bookmark');
    browser.toast('Bookmark removed', { label: 'Undo', run: () => void api.bookmarks.restore(removed) });
  }
</script>

<Popover label="Bookmark" width={320} right={140} onclose={done}>
  <h3>Bookmarked</h3>
  <label class="row">
    <span>Name</span>
    <input class="input" bind:value={title} onkeydown={(e) => e.key === 'Enter' && done()} />
  </label>
  <label class="row">
    <span>Folder</span>
    <select class="input" bind:value={folder}>
      {#each folders as f (f.id)}
        <option value={f.id}>{'  '.repeat(f.depth)}{f.title}</option>
      {/each}
    </select>
  </label>
  <div class="buttons">
    <button class="btn ghost" onclick={remove}>Remove</button>
    <button class="btn primary" onclick={done}>Done</button>
  </div>
</Popover>

<style>
  h3 {
    margin: 0 0 10px;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }
  .row {
    display: grid;
    grid-template-columns: 56px 1fr;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 12px;
  }
</style>
