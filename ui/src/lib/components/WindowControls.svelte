<script lang="ts">
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';

  // Windows 11 shows snap layouts when hovering the maximize button. Our button
  // is HTML inside WebView2, so after a short hover we ask for the flyout.
  let snapTimer: ReturnType<typeof setTimeout> | null = null;
  function hoverMax() {
    if (!browser.platform?.windows11) return;
    snapTimer = setTimeout(() => void api.window.snapLayouts(), 650);
  }
  function leaveMax() {
    if (snapTimer) clearTimeout(snapTimer);
    snapTimer = null;
  }
</script>

<div class="controls">
  <button class="ctl" aria-label="Minimize" onclick={() => api.window.minimize()}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
  </button>
  <button
    class="ctl"
    aria-label={browser.win.maximized ? 'Restore' : 'Maximize'}
    onclick={() => {
      leaveMax();
      void api.window.toggleMaximize();
    }}
    onpointerenter={hoverMax}
    onpointerleave={leaveMax}
  >
    {#if browser.win.maximized}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
        <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
      </svg>
    {:else}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" rx="1" /></svg>
    {/if}
  </button>
  <button class="ctl close" aria-label="Close" onclick={() => api.window.close()}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg>
  </button>
</div>

<style>
  .controls {
    display: flex;
    height: var(--bar-height);
    flex: none;
  }
  .ctl {
    display: grid;
    place-items: center;
    width: 46px;
    height: var(--bar-height);
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--text);
    transition: background-color 120ms ease-out, color 120ms ease-out;
  }
  .ctl svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
    shape-rendering: crispEdges;
  }
  .ctl:hover {
    background: var(--bg-hover);
    transition-duration: 0ms;
  }
  .ctl:active {
    background: var(--bg-active);
  }
  .close:hover {
    background: var(--danger);
    color: #fff;
  }
  .close:active {
    background: color-mix(in srgb, var(--danger) 80%, black);
  }
  :global(:root:not([data-focused='true'])) .ctl {
    color: var(--text-muted);
  }
</style>
