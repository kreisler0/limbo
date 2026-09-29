<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import { browser, downloadStatus } from '../stores/browser.svelte';

  let query = $state('');
  const list = $derived(
    browser.downloads.filter((d) => !query || (d.fileName + ' ' + d.url).toLowerCase().includes(query.toLowerCase())),
  );

  function source(url: string) {
    try {
      return new URL(url).host.replace(/^www\./, '');
    } catch {
      return '';
    }
  }

  async function clearAll() {
    await api.downloads.clear();
    browser.downloads = browser.downloads.filter((d) => d.state === 'inProgress' || d.state === 'paused');
  }
</script>

<div class="page">
  <header>
    <h1>Downloads</h1>
    <div class="tools">
      <label class="search">
        <Icon name="search" size={15} />
        <input placeholder="Search downloads" bind:value={query} aria-label="Search downloads" />
      </label>
      <button class="btn" disabled={!browser.downloads.length} onclick={clearAll}>Clear list</button>
    </div>
  </header>
  <ul>
    {#each list as d (d.id)}
      {@const pct = d.total ? Math.round((d.received / d.total) * 100) : null}
      <li class:bad={!!d.danger}>
        <span class="ic"><Icon name={d.danger ? 'alert' : 'file'} size={18} /></span>
        <div class="meta">
          {#if d.state === 'completed'}
            <button class="name link" onclick={() => api.downloads.open(d.path)}>{d.fileName}</button>
          {:else}
            <span class="name" class:strike={d.state === 'cancelled'}>{d.fileName}</span>
          {/if}
          <span class="sub tabular">{source(d.url)} · {downloadStatus(d)}</span>
          {#if d.state === 'inProgress' || d.state === 'paused'}
            <span class="bar"><span style:width="{pct ?? 30}%" class:indeterminate={pct === null}></span></span>
          {/if}
        </div>
        <div class="ctl">
          {#if d.state === 'inProgress'}
            <button class="btn ghost" onclick={() => api.downloads.control(d.id, 'pause')}>Pause</button>
            <button class="btn ghost" onclick={() => api.downloads.control(d.id, 'cancel')}>Cancel</button>
          {:else if d.state === 'paused' || (d.state === 'interrupted' && d.canResume)}
            <button class="btn ghost" onclick={() => api.downloads.control(d.id, 'resume')}>Resume</button>
            <button class="btn ghost" onclick={() => api.downloads.control(d.id, 'cancel')}>Cancel</button>
          {:else if d.state === 'completed'}
            <button class="btn ghost" onclick={() => api.downloads.showInFolder(d.path)}>Show in folder</button>
          {:else if !d.danger}
            <button class="btn ghost" onclick={() => api.tabs.create(d.url, { background: true })}>Retry</button>
          {/if}
          <button class="icon-btn" aria-label="Remove from list" onclick={() => api.downloads.remove(d.id).then(() => (browser.downloads = browser.downloads.filter((x) => x.id !== d.id)))}>
            <Icon name="x" size={13} />
          </button>
        </div>
      </li>
    {:else}
      <p class="empty">{query ? 'No matches' : 'Files you download appear here.'}</p>
    {/each}
  </ul>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    background: var(--bg);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    max-width: 820px;
    margin: 0 auto;
    padding: 28px 32px 16px;
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
    width: 260px;
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
  ul {
    max-width: 820px;
    margin: 0 auto;
    padding: 0 24px 48px;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 8px;
    border-radius: var(--radius-sm);
  }
  li + li {
    border-top: 1px solid var(--divider);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    background: var(--bg-hover);
    color: var(--text-muted);
    flex: none;
  }
  .bad .ic {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
  }
  .link:hover {
    text-decoration: underline;
  }
  .strike {
    color: var(--text-muted);
    text-decoration: line-through;
  }
  .sub {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .bad .sub {
    color: var(--danger);
  }
  .bar {
    height: 3px;
    margin-top: 6px;
    border-radius: 2px;
    background: var(--bg-active);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
    transition: width 300ms ease-out;
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .empty {
    padding: 48px 8px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
