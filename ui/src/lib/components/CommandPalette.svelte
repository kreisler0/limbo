<script lang="ts">
  // Ctrl+K: tabs, bookmarks, history, commands and settings in one list.
  // Enter on something that matches nothing searches Google.
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { commands } from '../menus';
  import { pop, fade } from '../design/motion';
  import type { IconName } from '../design/icons';
  import type { Bookmark, HistoryEntry } from '../types';

  interface Row {
    key: string;
    group: string;
    label: string;
    detail?: string;
    icon?: IconName;
    img?: string | null;
    hint?: string;
    run: () => unknown;
  }

  let query = $state('');
  let selected = $state(0);
  let history = $state<HistoryEntry[]>([]);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLUListElement>();

  function close() {
    ui.close('palette');
  }

  function flatBookmarks(): Bookmark[] {
    const out: Bookmark[] = [];
    const walk = (b: Bookmark) => {
      if (b.kind === 'url') out.push(b);
      b.children?.forEach(walk);
    };
    const r = browser.bookmarks;
    if (r) [r.toolbar, r.menu, r.other, r.mobile].forEach(walk);
    return out;
  }

  $effect(() => {
    const q = query.trim();
    if (!q) {
      history = [];
      return;
    }
    const handle = setTimeout(() => api.history.search(q, undefined, 6).then((h) => (history = h)), 60);
    return () => clearTimeout(handle);
  });

  const match = (s: string, q: string) => s.toLowerCase().includes(q);

  const rows = $derived.by((): Row[] => {
    const q = query.trim().toLowerCase();
    const out: Row[] = [];
    for (const t of browser.tabs) {
      if (!q || match(t.title + ' ' + t.url, q))
        out.push({ key: `t${t.id}`, group: 'Tabs', label: t.title || t.displayHost, detail: t.displayHost, img: t.favicon, icon: 'globe', run: () => api.tabs.activate(t.id) });
    }
    for (const c of commands()) {
      if (!q || match(c.label, q)) out.push({ key: c.id, group: 'Commands', label: c.label, icon: c.icon, hint: c.hint, run: c.run });
    }
    if (q) {
      for (const b of flatBookmarks().filter((b) => match(b.title + ' ' + b.url, q)).slice(0, 5)) {
        out.push({ key: `b${b.id}`, group: 'Bookmarks', label: b.title, detail: b.url ?? '', icon: 'star', run: () => b.url && open(b.url) });
      }
      for (const h of history) {
        out.push({ key: `h${h.visitId}`, group: 'History', label: h.title || h.url, detail: h.url, icon: 'history', run: () => open(h.url) });
      }
      out.push({ key: 'google', group: 'Search', label: `Search Google for “${query.trim()}”`, icon: 'search', run: () => open(query.trim(), true) });
    }
    return out.slice(0, 40);
  });

  function open(urlOrQuery: string, search = false) {
    const t = browser.active;
    if (!t) return;
    if (search) void api.nav.go(t.id, urlOrQuery);
    else void api.nav.url(t.id, urlOrQuery);
  }

  function run(r: Row | undefined) {
    if (!r) return;
    close();
    queueMicrotask(() => void r.run());
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!rows.length) return;
      selected = (selected + (e.key === 'ArrowDown' ? 1 : -1) + rows.length) % rows.length;
      listEl?.children[selected]?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(rows[selected]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  $effect(() => {
    void query;
    selected = 0;
  });

  $effect(() => {
    input?.focus();
  });
</script>

<div class="scrim" role="presentation" onpointerdown={close} transition:fade={{ duration: 140 }}></div>
<div class="palette float" role="dialog" aria-label="Command palette" in:pop={{ origin: 'top center', from: 0.97, y: -8 }}>
  <div class="search">
    <Icon name="command" size={16} />
    <input
      bind:this={input}
      bind:value={query}
      placeholder="Search tabs, bookmarks, history and commands"
      aria-label="Command"
      onkeydown={onKeydown}
    />
    <span class="kbd">Esc</span>
  </div>
  <ul bind:this={listEl} role="listbox">
    {#each rows as r, i (r.key)}
      {#if i === 0 || rows[i - 1].group !== r.group}
        <li class="group" role="presentation">{r.group}</li>
      {/if}
      <!-- Keyboard is handled by the text field (combobox pattern). -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        role="option"
        aria-selected={i === selected}
        class:selected={i === selected}
        onpointermove={() => (selected = i)}
        onpointerdown={(e) => e.preventDefault()}
        onclick={() => run(r)}
      >
        <span class="ic">
          {#if r.img}<img src={r.img} alt="" width="16" height="16" />{:else if r.icon}<Icon name={r.icon} size={15} />{/if}
        </span>
        <span class="label fade-out">{r.label}</span>
        {#if r.detail}<span class="detail">{r.detail}</span>{/if}
        {#if r.hint}<span class="kbd">{r.hint}</span>{/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    z-index: 70;
  }
  .palette {
    position: fixed;
    top: 72px;
    left: 50%;
    width: min(620px, calc(100vw - 32px));
    margin-left: calc(min(620px, calc(100vw - 32px)) / -2);
    z-index: 71;
    overflow: hidden;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 50px;
    padding: 0 16px;
    border-bottom: 1px solid var(--divider);
    color: var(--text-muted);
  }
  input {
    flex: 1;
    border: 0;
    background: none;
    font-size: var(--text-lg);
    color: var(--text);
  }
  input::placeholder {
    color: var(--text-faint);
  }
  ul {
    margin: 0;
    padding: 6px;
    list-style: none;
    max-height: min(420px, calc(100vh - 160px));
    overflow-y: auto;
  }
  .group {
    padding: 8px 10px 4px;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
  }
  li[role='option'] {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
  }
  li.selected {
    background: var(--bg-hover);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--text-muted);
    flex: none;
  }
  .ic img {
    border-radius: 3px;
  }
  .label {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 60%;
  }
  .detail {
    flex: 1;
    min-width: 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
