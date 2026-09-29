<script lang="ts">
  import type { Snippet } from 'svelte';
  import { pop } from '../design/motion';

  let {
    right = 8,
    left,
    top = 50,
    width = 320,
    origin = 'top right',
    label,
    onclose,
    children,
  }: {
    right?: number;
    left?: number;
    top?: number;
    width?: number;
    origin?: string;
    label: string;
    onclose: () => void;
    children: Snippet;
  } = $props();

  let el = $state<HTMLDivElement>();
  $effect(() => {
    // Move keyboard focus inside so Tab/Esc work without a mouse.
    const first = el?.querySelector<HTMLElement>('[autofocus], button, input, [tabindex="0"]');
    first?.focus({ preventScroll: true });
  });
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose}></div>
<div
  bind:this={el}
  class="popover float"
  role="dialog"
  aria-label={label}
  tabindex="-1"
  style:top="{top}px"
  style:right={left === undefined ? `${right}px` : undefined}
  style:left={left !== undefined ? `${left}px` : undefined}
  style:width="{width}px"
  in:pop={{ origin, from: 0.96, y: -4 }}
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }}
>
  {@render children()}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }
  .popover {
    position: fixed;
    z-index: 51;
    max-height: calc(100vh - 70px);
    overflow: auto;
    padding: var(--space-3);
  }
</style>
