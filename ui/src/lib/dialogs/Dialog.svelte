<script lang="ts">
  import type { Snippet } from 'svelte';
  import { pop, fade } from '../design/motion';

  let {
    label,
    width = 440,
    onclose,
    children,
  }: { label: string; width?: number; onclose?: () => void; children: Snippet } = $props();

  let el = $state<HTMLDivElement>();

  $effect(() => {
    const first = el?.querySelector<HTMLElement>('[autofocus], input, button.primary, button');
    first?.focus({ preventScroll: true });
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && onclose) {
      e.stopPropagation();
      onclose();
    } else if (e.key === 'Tab' && el) {
      // Keep focus inside the dialog.
      const f = [...el.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select, a[href], [tabindex="0"]')];
      if (!f.length) return;
      const first = f[0];
      const last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<div class="scrim" role="presentation" onpointerdown={() => onclose?.()} transition:fade={{ duration: 160 }}></div>
<div class="wrap">
  <div
    bind:this={el}
    class="dialog float"
    role="dialog"
    aria-modal="true"
    aria-label={label}
    tabindex="-1"
    style:width="{width}px"
    onkeydown={onKeydown}
    in:pop={{ origin: 'center', from: 0.95, spring: 'smooth' }}
  >
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: var(--scrim);
  }
  .wrap {
    position: fixed;
    inset: 0;
    z-index: 81;
    display: grid;
    place-items: center;
    padding: 24px;
    pointer-events: none;
  }
  .dialog {
    max-width: 100%;
    max-height: calc(100vh - 48px);
    overflow: auto;
    padding: 24px;
    border-radius: var(--radius-lg);
    pointer-events: auto;
  }
  .dialog :global(h2) {
    margin: 0 0 6px;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
  }
  .dialog :global(.sub) {
    margin: 0 0 18px;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: var(--text-md-lh);
  }
  .dialog :global(.actions) {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 22px;
  }
</style>
