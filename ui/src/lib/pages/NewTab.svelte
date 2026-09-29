<script lang="ts">
  // The new tab page: a quiet greeting, a search box that hands off to the
  // omnibox (so there is one place to type), and your most visited sites.
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { Tile } from '../types';

  let tiles = $state<Tile[]>([]);
  let now = $state(new Date());

  $effect(() => {
    api.topSites(8).then((t) => (tiles = t));
    const timer = setInterval(() => (now = new Date()), 15_000);
    return () => clearInterval(timer);
  });

  const greeting = $derived.by(() => {
    const h = now.getHours();
    if (h < 5) return 'Good night';
    if (h < 12) return 'Good morning';
    if (h < 18) return 'Good afternoon';
    return 'Good evening';
  });

  const isPrivate = $derived(!!browser.active?.private);

  function open(t: Tile, e: MouseEvent) {
    const tab = browser.active;
    if (!tab || e.ctrlKey || e.button === 1) void api.tabs.create(t.url, { background: true });
    else void api.nav.url(tab.id, t.url);
  }

  function fakebox(e: Event) {
    e.preventDefault();
    ui.focusOmnibox();
  }
</script>

<main class="page">
  <div class="center">
    {#if isPrivate}
      <div class="private">
        <Icon name="glasses" size={28} />
        <h1>Private tab</h1>
        <p>Limbo won't keep this tab's history, cookies, or form entries after you close it. Downloads and bookmarks you make are kept.</p>
      </div>
    {:else}
      <p class="clock tabular">{now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</p>
      <h1>{greeting}</h1>
    {/if}

    <button class="fakebox" onclick={fakebox} onfocus={fakebox}>
      <Icon name="search" size={16} />
      <span>Search Google or type a URL</span>
      <span class="kbd">Ctrl L</span>
    </button>

    {#if !isPrivate && tiles.length}
      <nav class="tiles" aria-label="Most visited">
        {#each tiles as t (t.url)}
          <a class="tile" href={t.url} onclick={(e) => { e.preventDefault(); open(t, e); }} onauxclick={(e) => { e.preventDefault(); open(t, e); }}>
            <span class="icon">
              {#if t.favicon}<img src={t.favicon} alt="" width="24" height="24" />{:else}<span class="letter">{t.host.replace(/^www\./, '')[0]?.toUpperCase()}</span>{/if}
            </span>
            <span class="name fade-out">{t.title || t.host}</span>
          </a>
        {/each}
      </nav>
    {/if}
  </div>
</main>

<style>
  .page {
    display: grid;
    place-items: start center;
    height: 100%;
    overflow-y: auto;
    background: var(--bg);
  }
  .center {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: min(640px, calc(100% - 48px));
    padding-top: clamp(48px, 18vh, 180px);
  }
  .clock {
    margin: 0 0 4px;
    color: var(--text-faint);
    font-size: var(--text-md);
  }
  h1 {
    margin: 0 0 28px;
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: var(--weight-semibold);
    letter-spacing: -0.01em;
  }
  .private {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    color: var(--accent-text);
  }
  .private h1 {
    margin: 10px 0 8px;
    color: var(--text);
  }
  .private p {
    max-width: 440px;
    margin: 0 0 28px;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: var(--text-md-lh);
  }
  .fakebox {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 48px;
    padding: 0 16px;
    border: 1px solid var(--divider);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow-tab);
    color: var(--text-muted);
    font-size: var(--text-lg);
    text-align: left;
    transition: box-shadow var(--dur-snappy) var(--ease-snappy);
  }
  .fakebox:hover {
    box-shadow: var(--shadow-float);
  }
  .fakebox span:nth-child(2) {
    flex: 1;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    width: 100%;
    margin-top: 32px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 14px 8px 10px;
    border-radius: var(--radius-md);
    color: var(--text);
    text-decoration: none;
    transition: background var(--dur-snappy) var(--ease-snappy), transform var(--dur-snappy) var(--ease-snappy);
  }
  .tile:hover {
    background: var(--bg-hover);
  }
  .tile:active {
    transform: scale(0.97);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: var(--bg-subtle);
    box-shadow: var(--shadow-tab);
  }
  .icon img {
    border-radius: 5px;
  }
  .letter {
    font-weight: var(--weight-semibold);
    color: var(--text-muted);
  }
  .name {
    max-width: 100%;
    font-size: var(--text-sm);
  }
</style>
