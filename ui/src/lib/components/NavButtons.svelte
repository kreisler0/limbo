<script lang="ts">
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';

  const t = $derived(browser.active);
</script>

<div class="nav">
  <button class="icon-btn" aria-label="Back" title="Back (Alt+Left)" disabled={!t?.canGoBack} onclick={() => t && api.nav.back(t.id)}>
    <Icon name="arrowLeft" />
  </button>
  <button class="icon-btn" aria-label="Forward" title="Forward (Alt+Right)" disabled={!t?.canGoForward} onclick={() => t && api.nav.forward(t.id)}>
    <Icon name="arrowRight" />
  </button>
  {#if t?.loading}
    <button class="icon-btn" aria-label="Stop" title="Stop" onclick={() => t && api.nav.stop(t.id)}>
      <Icon name="x" />
    </button>
  {:else}
    <button
      class="icon-btn"
      aria-label="Reload"
      title="Reload (Ctrl+R)"
      disabled={!t || !!t.internal}
      onclick={(e) => t && api.nav.reload(t.id, e.shiftKey || e.ctrlKey)}
    >
      <Icon name="rotateCw" />
    </button>
  {/if}
</div>

<style>
  .nav {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 var(--space-2) 0 var(--space-2);
    flex: none;
  }
</style>
