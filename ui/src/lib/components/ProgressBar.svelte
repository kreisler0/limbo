<script lang="ts">
  import { browser } from '../stores/browser.svelte';

  // A 2px accent line: trickles while loading, then finishes and fades.
  // Pure CSS transforms, and idle when nothing loads (no animation loops).
  const t = $derived(browser.active);
  const loading = $derived(!!t?.loading && !t.internal);
  const target = $derived(loading ? Math.max(0.08, Math.min(0.92, t?.progress ?? 0)) : 1);

  let visible = $state(false);
  let done = $state(false);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (loading) {
      if (hideTimer) clearTimeout(hideTimer);
      visible = true;
      done = false;
    } else if (visible) {
      done = true;
      hideTimer = setTimeout(() => {
        visible = false;
        done = false;
      }, 420);
    }
  });
</script>

<div class="track" aria-hidden="true">
  {#if visible}
    <div class="bar" class:done class:trickle={loading} style:transform="scaleX({target})"></div>
  {/if}
</div>

<style>
  .track {
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 2px;
    pointer-events: none;
    overflow: hidden;
    z-index: 5;
  }
  .bar {
    height: 100%;
    background: var(--accent);
    transform-origin: left center;
    transition:
      transform 600ms cubic-bezier(0.2, 0.7, 0.2, 1),
      opacity 300ms ease-out 120ms;
  }
  .bar.trickle {
    transition-duration: 1800ms, 300ms;
  }
  .bar.done {
    opacity: 0;
    transition-duration: 220ms, 300ms;
  }
</style>
