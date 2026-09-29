<script lang="ts">
  let {
    checked,
    label,
    disabled = false,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button class="toggle" role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    flex: none;
    width: 34px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--bg-active);
    transition: background-color var(--dur-snappy) var(--ease-snappy);
  }
  .toggle[aria-checked='true'] {
    background: var(--accent-strong);
  }
  .toggle:disabled {
    opacity: 0.5;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform var(--dur-snappy) var(--ease-snappy);
  }
  .toggle[aria-checked='true'] .knob {
    transform: translateX(14px);
  }
  .toggle:active:not(:disabled) .knob {
    width: 17px;
  }
  .toggle[aria-checked='true']:active:not(:disabled) .knob {
    transform: translateX(11px);
  }
</style>
