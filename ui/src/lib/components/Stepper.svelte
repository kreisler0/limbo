<script lang="ts">
  import Icon from './Icon.svelte';

  // A number stepper where one step past `max` means "No limit" (null).
  let {
    value,
    min,
    max,
    onchange,
  }: { value: number | null; min: number; max: number; onchange: (v: number | null) => void } = $props();

  function step(delta: number) {
    if (value === null) {
      if (delta < 0) onchange(max);
      return;
    }
    const next = value + delta;
    if (next > max) onchange(null);
    else onchange(Math.max(min, next));
  }
</script>

<div class="stepper" role="group" aria-label="Maximum awake tabs">
  <button class="icon-btn" aria-label="Fewer" disabled={value === min} onclick={() => step(-1)}>
    <Icon name="minus" size={13} />
  </button>
  <output class="tabular" aria-live="polite">{value === null ? 'No limit' : value}</output>
  <button class="icon-btn" aria-label="More" disabled={value === null} onclick={() => step(1)}>
    <Icon name="plus" size={13} />
  </button>
</div>

<style>
  .stepper {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
  }
  .icon-btn {
    width: 24px;
    height: 24px;
  }
  output {
    min-width: 54px;
    text-align: center;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
</style>
