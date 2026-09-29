<script lang="ts" module>
  import type { IconName } from '../design/icons';

  export interface MenuEntry {
    label: string;
    icon?: IconName;
    shortcut?: string;
    disabled?: boolean;
    checked?: boolean;
    danger?: boolean;
    run: () => void;
  }
  export type MenuItemDef = MenuEntry | 'separator';
</script>

<script lang="ts">
  import Icon from './Icon.svelte';
  import { pop } from '../design/motion';

  let {
    items,
    x,
    y,
    origin = 'top left',
    minWidth = 200,
    onclose,
  }: { items: MenuItemDef[]; x: number; y: number; origin?: string; minWidth?: number; onclose: () => void } = $props();

  let el = $state<HTMLDivElement>();
  let active = $state(-1);
  let typeahead = '';
  let typeTimer: ReturnType<typeof setTimeout> | null = null;
  let pos = $state({ left: 0, top: 0 });

  const entries = $derived(items.map((it, i) => ({ it, i })).filter((e) => e.it !== 'separator' && !e.it.disabled));

  // Keep the menu inside the window.
  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.max(6, Math.min(x, window.innerWidth - r.width - 6)),
      top: Math.max(6, Math.min(y, window.innerHeight - r.height - 6)),
    };
    el.focus();
  });

  function choose(it: MenuItemDef) {
    if (it === 'separator' || it.disabled) return;
    // Run first: a menu item that opens another layer then covers the page
    // before this menu uncovers it (no flash), and the page context menu
    // answers the host before closing releases it.
    it.run();
    onclose();
  }

  function move(delta: number) {
    if (!entries.length) return;
    const idx = entries.findIndex((e) => e.i === active);
    const next = idx < 0 ? (delta > 0 ? 0 : entries.length - 1) : (idx + delta + entries.length) % entries.length;
    active = entries[next].i;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') move(1);
    else if (e.key === 'ArrowUp') move(-1);
    else if (e.key === 'Home') active = entries[0]?.i ?? -1;
    else if (e.key === 'End') active = entries.at(-1)?.i ?? -1;
    else if (e.key === 'Enter' || e.key === ' ') {
      if (active >= 0) choose(items[active]);
    } else if (e.key === 'Escape' || e.key === 'Tab') {
      onclose();
    } else if (e.key.length === 1 && !e.ctrlKey && !e.altKey) {
      // Type-ahead: jump to the next item starting with what was typed.
      typeahead += e.key.toLowerCase();
      if (typeTimer) clearTimeout(typeTimer);
      typeTimer = setTimeout(() => (typeahead = ''), 600);
      const start = entries.findIndex((en) => en.i === active);
      const ordered = [...entries.slice(start + 1), ...entries.slice(0, start + 1)];
      const hit = ordered.find((en) => (en.it as MenuEntry).label.toLowerCase().startsWith(typeahead));
      if (hit) active = hit.i;
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}></div>
<div
  bind:this={el}
  class="menu float"
  role="menu"
  tabindex="-1"
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  style:min-width="{minWidth}px"
  onkeydown={onKeydown}
  in:pop={{ origin, from: 0.96 }}
>
  {#each items as it, i (i)}
    {#if it === 'separator'}
      <div class="divider" role="separator"></div>
    {:else}
      <button
        class="item"
        class:active={i === active}
        class:danger={it.danger}
        role="menuitem"
        disabled={it.disabled}
        tabindex="-1"
        onpointermove={() => (active = it.disabled ? -1 : i)}
        onpointerleave={() => (active = -1)}
        onclick={() => choose(it)}
      >
        <span class="icon">
          {#if it.checked}
            <Icon name="check" size={14} />
          {:else if it.icon}
            <Icon name={it.icon} size={14} />
          {/if}
        </span>
        <span class="label">{it.label}</span>
        {#if it.shortcut}<span class="shortcut">{it.shortcut}</span>{/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
  }
  .menu {
    position: fixed;
    z-index: 61;
    padding: 5px;
    max-height: calc(100vh - 12px);
    overflow-y: auto;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 10px 0 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
  }
  .item.active {
    background: var(--bg-hover);
  }
  .item:disabled {
    color: var(--text-faint);
  }
  .item.danger {
    color: var(--danger);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--text-muted);
    flex: none;
  }
  .danger .icon {
    color: inherit;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .shortcut {
    margin-left: 24px;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
</style>
