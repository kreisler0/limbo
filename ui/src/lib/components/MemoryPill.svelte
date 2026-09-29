<script lang="ts">
  import { untrack } from 'svelte';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { tween } from '../design/motion';

  let shown = $state(0);
  let cancel: (() => void) | null = null;

  const target = $derived(browser.memory ? browser.memory.totalBytes / 1048576 : 0);
  $effect(() => {
    const to = target;
    cancel?.();
    const from = untrack(() => shown);
    cancel = tween(from, to, 200, (v) => (shown = v));
  });

  const level = $derived(browser.pressure);
</script>

{#if browser.memory}
  <button
    class="pill tabular"
    class:elevated={level === 'elevated'}
    class:critical={level === 'critical'}
    aria-label="Memory: {Math.round(target)} MB. Open memory saver"
    title="Memory used by Limbo (Ctrl+Shift+M)"
    onclick={() => ui.toggle('memory')}
  >
    {Math.round(shown)} MB
  </button>
{/if}

<style>
  .pill {
    height: 24px;
    padding: 0 9px;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    white-space: nowrap;
    transition: background-color 120ms ease-out, color 300ms ease-out;
  }
  .pill:hover {
    background: var(--bg-hover);
    color: var(--text);
    transition-duration: 0ms, 300ms;
  }
  .elevated {
    color: var(--warning);
    background: var(--warning-soft);
  }
  .critical {
    color: var(--danger);
    background: var(--danger-soft);
  }
</style>
