<script lang="ts">
  // What shows in the page area when there's no live page: a crashed tab, or
  // an unloaded tab while it reloads (its last snapshot, blurred).
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import type { TabInfo } from '../types';

  let { tab }: { tab: TabInfo } = $props();
  let shot = $state<string | null>(null);

  $effect(() => {
    const id = tab.id;
    let url: string | null = null;
    if (tab.hasSnapshot && !tab.crashed) {
      api.tabs.snapshot(id).then((u) => {
        url = u;
        shot = u;
      });
    }
    return () => {
      if (url?.startsWith('blob:')) URL.revokeObjectURL(url);
      shot = null;
    };
  });
</script>

<div class="wrap">
  {#if tab.crashed}
    <div class="crashed">
      <Icon name="alert" size={28} />
      <h2>This page crashed</h2>
      <p>Something went wrong while showing {tab.displayHost || 'this page'}. Other tabs are fine.</p>
      <button class="btn primary" onclick={() => api.nav.reload(tab.id)}>Reload</button>
    </div>
  {:else if shot}
    <img src={shot} alt="" />
    <div class="waking"><span class="dot"></span>Waking up…</div>
  {:else}
    <div class="waking plain"><span class="dot"></span>Loading…</div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    height: 100%;
    overflow: hidden;
    background: var(--bg);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: top left;
    filter: saturate(0.6) blur(1px);
    opacity: 0.7;
  }
  .waking {
    position: absolute;
    top: 16px;
    left: 50%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: var(--radius-pill);
    background: var(--bg-float);
    box-shadow: var(--shadow-float);
    color: var(--text-muted);
    font-size: var(--text-sm);
    transform: translateX(-50%);
  }
  .plain {
    box-shadow: none;
    background: none;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    from {
      opacity: 0.3;
    }
  }
  .crashed {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    color: var(--text-muted);
    text-align: center;
  }
  .crashed h2 {
    margin: 8px 0 0;
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
  }
  .crashed p {
    max-width: 380px;
    margin: 0 0 12px;
    font-size: var(--text-md);
  }
</style>
