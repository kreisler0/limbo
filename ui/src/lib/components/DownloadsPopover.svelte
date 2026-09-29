<script lang="ts">
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { api } from '../ipc';
  import { browser, downloadStatus as status } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { openInternal } from '../menus';

  const list = $derived(browser.downloads.slice(0, 8));
</script>

<Popover label="Downloads" width={340} onclose={() => ui.close('downloads')}>
  <div class="head">
    <h3>Downloads</h3>
    <button class="btn ghost small" onclick={() => { ui.close('downloads'); openInternal('downloads'); }}>Show all</button>
  </div>
  {#if !list.length}
    <p class="empty">Nothing downloaded yet.</p>
  {/if}
  <ul>
    {#each list as d (d.id)}
      {@const pct = d.total ? d.received / d.total : 0}
      <li class:bad={!!d.danger}>
        <button class="file" disabled={d.state !== 'completed'} onclick={() => api.downloads.open(d.path)} title={d.path}>
          <span class="ring" style:--p={d.state === 'inProgress' ? pct : 1}>
            <Icon name={d.danger ? 'alert' : 'file'} size={15} />
          </span>
          <span class="meta">
            <span class="name fade-out">{d.fileName}</span>
            <span class="status tabular">{status(d)}</span>
          </span>
        </button>
        <span class="ctl">
          {#if d.state === 'inProgress'}
            <button class="icon-btn" aria-label="Pause" onclick={() => api.downloads.control(d.id, 'pause')}><Icon name="pause" size={13} /></button>
            <button class="icon-btn" aria-label="Cancel" onclick={() => api.downloads.control(d.id, 'cancel')}><Icon name="x" size={13} /></button>
          {:else if d.state === 'paused'}
            <button class="icon-btn" aria-label="Resume" onclick={() => api.downloads.control(d.id, 'resume')}><Icon name="play" size={13} /></button>
          {:else if d.state === 'completed'}
            <button class="icon-btn" aria-label="Show in folder" title="Show in folder" onclick={() => api.downloads.showInFolder(d.path)}><Icon name="folder" size={14} /></button>
          {/if}
        </span>
      </li>
    {/each}
  </ul>
</Popover>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  h3 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }
  .small {
    height: 24px;
    font-size: var(--text-xs);
  }
  .empty {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  ul {
    margin: 0 -6px;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
  }
  li:hover {
    background: var(--bg-hover);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
    padding: 6px;
    border: 0;
    background: none;
    text-align: left;
  }
  .ring {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: conic-gradient(var(--accent) calc(var(--p) * 1turn), var(--bg-active) 0);
    color: var(--text-muted);
    flex: none;
    position: relative;
    isolation: isolate;
  }
  .ring::before {
    content: '';
    position: absolute;
    inset: 2px;
    z-index: -1;
    border-radius: 50%;
    background: var(--bg-float);
  }
  .meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .status {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .bad .status {
    color: var(--danger);
  }
  .ctl {
    display: flex;
    padding-right: 4px;
  }
</style>
