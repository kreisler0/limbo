<script lang="ts" generics="T extends string">
  let {
    value,
    options,
    label,
    onchange,
  }: { value: T; options: { value: T; label: string }[]; label: string; onchange: (v: T) => void } = $props();

  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));
</script>

<div class="seg" role="radiogroup" aria-label={label} style:--n={options.length} style:--i={index}>
  <span class="thumb" class:none={!options.some((o) => o.value === value)}></span>
  {#each options as o (o.value)}
    <button role="radio" aria-checked={o.value === value} onclick={() => onchange(o.value)}>{o.label}</button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), 1fr);
    padding: 2px;
    border-radius: 8px;
    background: var(--bg-hover);
  }
  .thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc((100% - 4px) / var(--n));
    border-radius: 6px;
    background: var(--bg-float);
    box-shadow: var(--shadow-tab);
    transform: translateX(calc(100% * var(--i)));
    transition: transform var(--dur-smooth) var(--ease-smooth);
  }
  .thumb.none {
    opacity: 0;
  }
  button {
    position: relative;
    height: 26px;
    padding: 0 12px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    white-space: nowrap;
  }
  button[aria-checked='true'] {
    color: var(--text);
  }
</style>
