<script lang="ts">
  import Dialog from './Dialog.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';

  let { onclose }: { onclose: () => void } = $props();

  type Kind = 'history' | 'cookies' | 'cache' | 'downloads' | 'passwords' | 'autofill';
  const KINDS: { id: Kind; label: string; detail: string }[] = [
    { id: 'history', label: 'Browsing history', detail: 'Pages you visited and their icons' },
    { id: 'cookies', label: 'Cookies and site data', detail: 'Signs you out of most sites' },
    { id: 'cache', label: 'Cached images and files', detail: 'Sites may load slower next time' },
    { id: 'downloads', label: 'Download list', detail: 'Files stay on your PC' },
    { id: 'autofill', label: 'Form entries', detail: 'Things you typed into forms' },
    { id: 'passwords', label: 'Saved passwords', detail: 'Can’t be undone' },
  ];
  const RANGES = [
    { label: 'Last hour', ms: 3_600_000 },
    { label: 'Last 24 hours', ms: 86_400_000 },
    { label: 'Last 7 days', ms: 7 * 86_400_000 },
    { label: 'Last 4 weeks', ms: 28 * 86_400_000 },
    { label: 'All time', ms: null },
  ];

  let range = $state(1);
  let kinds = $state<Record<Kind, boolean>>({ history: true, cookies: true, cache: true, downloads: false, passwords: false, autofill: false });
  let busy = $state(false);

  async function clear() {
    busy = true;
    const ms = RANGES[range].ms;
    const since = ms === null ? 0 : (Date.now() - ms) * 1000;
    try {
      await api.history.clear(since, kinds);
      browser.toast('Browsing data cleared');
      if (kinds.downloads) browser.downloads = browser.downloads.filter((d) => d.state === 'inProgress' || d.state === 'paused');
      onclose();
    } catch (e) {
      browser.toast(String(e));
      busy = false;
    }
  }
</script>

<Dialog label="Clear browsing data" {onclose}>
  <h2>Clear browsing data</h2>
  <p class="sub">Choose what to remove from this PC.</p>
  <label class="range">
    Time range
    <select class="input" bind:value={range}>
      {#each RANGES as r, i (r.label)}<option value={i}>{r.label}</option>{/each}
    </select>
  </label>
  <div class="list">
    {#each KINDS as k (k.id)}
      <label class="item">
        <input type="checkbox" bind:checked={kinds[k.id]} />
        <span>
          <strong>{k.label}</strong>
          <small class:warn={k.id === 'passwords'}>{k.detail}</small>
        </span>
      </label>
    {/each}
  </div>
  <div class="actions">
    <button class="btn ghost" onclick={onclose}>Cancel</button>
    <button class="btn primary" disabled={busy || !Object.values(kinds).some(Boolean)} onclick={clear}>{busy ? 'Clearing…' : 'Clear data'}</button>
  </div>
</Dialog>

<style>
  .range {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    font-size: var(--text-md);
  }
  select {
    color: var(--text);
  }
  .list {
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    box-shadow: 0 0 0 1px var(--divider);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
  }
  .item + .item {
    border-top: 1px solid var(--divider);
  }
  .item input {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-strong);
  }
  .item span {
    display: flex;
    flex-direction: column;
  }
  strong {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }
  small {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  small.warn {
    color: var(--danger);
  }
</style>
