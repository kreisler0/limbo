<script lang="ts">
  // Dev only (never in the app bundle): stands in for a web page, which in
  // Limbo is a WebView2 window above the UI.
  import type { TabInfo } from '../types';

  let { tab }: { tab: TabInfo } = $props();

  const hue = $derived.by(() => {
    let h = 0;
    for (const c of tab.displayHost || tab.url) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  });
</script>

<div class="page" style:--h={hue}>
  <header>{tab.title || tab.displayHost}</header>
  {#if tab.loading}
    <p class="muted">Loading {tab.url}…</p>
  {:else}
    {#each Array(9) as _, i (i)}
      <div class="line" style:width="{Math.min(90, 30 + ((i * 37) % 60))}%"></div>
    {/each}
  {/if}
</div>

<style>
  .page {
    height: 100%;
    background: hsl(var(--h) 30% 97%);
    color: #222;
    overflow: hidden;
  }
  header {
    height: 64px;
    padding: 20px 32px;
    background: hsl(var(--h) 55% 50%);
    color: #fff;
    font-size: 22px;
    font-weight: 600;
  }
  .line {
    height: 12px;
    margin: 22px 32px 0;
    border-radius: 3px;
    background: hsl(var(--h) 15% 82%);
  }
  .muted {
    margin: 24px 32px;
    color: #666;
  }
</style>
