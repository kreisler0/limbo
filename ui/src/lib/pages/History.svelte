<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { actions } from '../menus';
  import type { HistoryEntry } from '../types';

  const PAGE = 100;
  let query = $state('');
  let rows = $state<HistoryEntry[]>([]);
  let done = $state(false);
  let loading = false;
  let selected = $state<Set<number>>(new Set());
  let sentinel = $state<HTMLDivElement>();

  async function load(reset: boolean) {
    if (loading) return;
    loading = true;
    const before = reset ? undefined : rows.at(-1)?.visitUs;
    const page = await api.history.search(query.trim() || undefined, before, PAGE).catch(() => []);
    rows = reset ? page : [...rows, ...page];
    done = page.length < PAGE;
    loading = false;
  }

  $effect(() => {
    void query;
    const h = setTimeout(() => {
      selected = new Set();
      void load(true);
    }, 120);
    return () => clearTimeout(h);
  });

  $effect(() => {
    if (!sentinel) return;
    const io = new IntersectionObserver((e) => e[0].isIntersecting && !done && void load(false), { rootMargin: '400px' });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  const groups = $derived.by(() => {
    const out: { day: string; items: HistoryEntry[] }[] = [];
    const today = new Date().toDateString();
    const yesterday = new Date(Date.now() - 86_400_000).toDateString();
    for (const r of rows) {
      const d = new Date(r.visitUs / 1000);
      const key = d.toDateString();
      const label = key === today ? 'Today' : key === yesterday ? 'Yesterday' : d.toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric' });
      if (out.at(-1)?.day !== label) out.push({ day: label, items: [] });
      out.at(-1)!.items.push(r);
    }
    return out;
  });

  function host(url: string) {
    try {
      return new URL(url).host.replace(/^www\./, '');
    } catch {
      return url;
    }
  }

  function open(url: string, e: MouseEvent) {
    const t = browser.active;
    if (!t || e.ctrlKey || e.button === 1) void api.tabs.create(url, { background: true });
    else void api.nav.url(t.id, url);
  }

  function toggle(id: number) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  async function removeSelected() {
    const ids = [...selected];
    await api.history.remove(ids);
    rows = rows.filter((r) => !selected.has(r.visitId));
    selected = new Set();
    browser.toast(`Removed ${ids.length} ${ids.length === 1 ? 'page' : 'pages'} from history`);
  }
</script>

<div class="page">
  <header>
    <h1>History</h1>
    <div class="tools">
      <label class="search">
        <Icon name="search" size={15} />
        <input placeholder="Search history" bind:value={query} aria-label="Search history" />
      </label>
      {#if selected.size}
        <button class="btn danger" onclick={removeSelected}>Delete {selected.size}</button>
        <button class="btn ghost" onclick={() => (selected = new Set())}>Cancel</button>
      {:else}
        <button class="btn" onclick={actions.clearData}>Clear browsing data…</button>
      {/if}
    </div>
  </header>

  <div class="list">
    {#each groups as g (g.day)}
      <h2>{g.day}</h2>
      {#each g.items as r (r.visitId)}
        <div class="row" class:sel={selected.has(r.visitId)}>
          <input type="checkbox" checked={selected.has(r.visitId)} onchange={() => toggle(r.visitId)} aria-label="Select" />
          <span class="time tabular">{new Date(r.visitUs / 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</span>
          <a href={r.url} onclick={(e) => { e.preventDefault(); open(r.url, e); }} onauxclick={(e) => { e.preventDefault(); open(r.url, e); }}>
            <span class="title fade-out">{r.title || r.url}</span>
            <span class="host">{host(r.url)}</span>
          </a>
          <button class="icon-btn" aria-label="Remove from history" onclick={() => api.history.remove([r.visitId]).then(() => (rows = rows.filter((x) => x !== r)))}>
            <Icon name="x" size={13} />
          </button>
        </div>
      {/each}
    {:else}
      <p class="empty">{query ? 'No matches' : 'Pages you visit appear here.'}</p>
    {/each}
    <div bind:this={sentinel}></div>
  </div>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    background: var(--bg);
  }
  header {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    max-width: 820px;
    margin: 0 auto;
    padding: 28px 32px 12px;
    background: var(--bg);
  }
  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
  }
  .tools {
    display: flex;
    gap: 8px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 280px;
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
  .list {
    max-width: 820px;
    margin: 0 auto;
    padding: 0 24px 48px;
  }
  h2 {
    margin: 20px 8px 6px;
    color: var(--text-muted);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 36px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
  }
  .row:hover,
  .row.sel {
    background: var(--bg-hover);
  }
  .row input {
    opacity: 0;
    accent-color: var(--accent-strong);
  }
  .row:hover input,
  .row input:checked,
  .row input:focus-visible {
    opacity: 1;
  }
  .time {
    width: 64px;
    flex: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  a {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex: 1;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
    font-size: var(--text-md);
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
  }
  .host {
    flex: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .row .icon-btn {
    opacity: 0;
  }
  .row:hover .icon-btn {
    opacity: 1;
  }
  .empty {
    padding: 48px 8px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
