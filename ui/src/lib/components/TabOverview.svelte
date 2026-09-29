<script lang="ts">
  // Ctrl+Shift+A: every tab as a snapshot card, zooming out from the page.
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { ui } from '../stores/ui.svelte';
  import { pop, fade } from '../design/motion';

  let shots = $state<Record<number, string | null>>({});
  let filter = $state('');

  $effect(() => {
    // Snapshots load lazily; the active tab reuses the overlay snapshot.
    let cancelled = false;
    (async () => {
      for (const t of browser.tabs) {
        if (cancelled) break;
        if (t.internal) continue;
        const url = t.id === browser.activeId && overlay.snapshot ? overlay.snapshot : await api.tabs.snapshot(t.id).catch(() => null);
        shots[t.id] = url;
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  const tabs = $derived(browser.tabs.filter((t) => !filter || (t.title + t.url).toLowerCase().includes(filter.toLowerCase())));

  function close() {
    ui.close('overview');
  }

  function pick(id: number) {
    close();
    void api.tabs.activate(id);
  }
</script>

<div class="scrim" role="presentation" onpointerdown={close} transition:fade={{ duration: 180 }}></div>
<section class="overview" aria-label="All tabs" in:pop={{ spring: 'gentle', from: 1.06, origin: 'center' }}>
  <header>
    <h2>{browser.tabs.length} tabs</h2>
    <input class="input" placeholder="Filter" bind:value={filter} aria-label="Filter tabs" />
  </header>
  <div class="grid">
    {#each tabs as t (t.id)}
      <div class="card" class:active={t.id === browser.activeId} role="button" tabindex="0" onclick={() => pick(t.id)} onkeydown={(e) => e.key === 'Enter' && pick(t.id)}>
        <div class="shot">
          {#if shots[t.id]}
            <img src={shots[t.id]} alt="" />
          {:else}
            <div class="blank">{#if t.favicon}<img class="big" src={t.favicon} alt="" />{/if}</div>
          {/if}
          <button class="close icon-btn" aria-label="Close tab" onclick={(e) => { e.stopPropagation(); void api.tabs.close(t.id); }}>
            <Icon name="x" size={12} />
          </button>
        </div>
        <div class="caption">
          {#if t.favicon}<img src={t.favicon} alt="" width="14" height="14" />{/if}
          <span class="fade-out">{t.title || t.displayHost || 'New Tab'}</span>
        </div>
      </div>
    {/each}
  </div>
</section>

<style>
  .scrim {
    position: fixed;
    inset: var(--bar-height) 0 0 0;
    background: var(--bg-subtle);
    z-index: 65;
  }
  .overview {
    position: fixed;
    inset: calc(var(--bar-height) + 16px) 24px 16px;
    z-index: 66;
    display: flex;
    flex-direction: column;
    pointer-events: none;
  }
  header,
  .grid {
    pointer-events: auto;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }
  h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 16px;
    overflow-y: auto;
    padding: 4px;
  }
  .card {
    border-radius: var(--radius-md);
    transition: transform var(--dur-snappy) var(--ease-snappy);
  }
  .card:hover {
    transform: translateY(-2px);
  }
  .card:active {
    transform: scale(0.98);
  }
  .shot {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: var(--radius-md);
    overflow: hidden;
    background: var(--bg);
    box-shadow: var(--shadow-tab);
  }
  .card.active .shot {
    box-shadow: 0 0 0 2px var(--accent), var(--shadow-float);
  }
  .shot > img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: top;
  }
  .blank {
    display: grid;
    place-items: center;
    height: 100%;
  }
  .big {
    width: 32px;
    height: 32px;
    opacity: 0.6;
  }
  .close {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 22px;
    height: 22px;
    background: var(--bg-float);
    box-shadow: var(--shadow-tab);
    opacity: 0;
  }
  .card:hover .close {
    opacity: 1;
  }
  .caption {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    font-size: var(--text-sm);
  }
  .caption img {
    border-radius: 3px;
  }
</style>
