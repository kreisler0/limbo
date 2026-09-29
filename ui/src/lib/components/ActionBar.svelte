<script lang="ts">
  import Icon from './Icon.svelte';
  import MemoryPill from './MemoryPill.svelte';
  import Menu, { type MenuItemDef } from './Menu.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { ExtensionInfo } from '../types';
  import { appMenuItems } from '../menus';

  const MAX_VISIBLE = 3;
  const enabled = $derived(browser.extensions.filter((e) => e.enabled && e.hasAction));
  const visible = $derived(enabled.filter((e) => e.pinned).slice(0, MAX_VISIBLE));
  const showMemory = $derived(browser.settings?.showMemoryInToolbar ?? true);

  const activeDownloads = $derived(browser.downloads.filter((d) => d.state === 'inProgress'));
  const progress = $derived.by(() => {
    const withTotal = activeDownloads.filter((d) => d.total);
    if (!withTotal.length) return null;
    const got = withTotal.reduce((s, d) => s + d.received, 0);
    const total = withTotal.reduce((s, d) => s + (d.total ?? 0), 0);
    return total ? got / total : null;
  });
  const recentDownloads = $derived(browser.downloads.length > 0);

  let extMenu = $state<null | { x: number; y: number; items: MenuItemDef[] }>(null);
  let appMenu = $state<null | { x: number; y: number }>(null);

  async function openPopup(e: ExtensionInfo, target: HTMLElement) {
    if (!e.popup) {
      browser.toast(`${e.name} has no popup`);
      return;
    }
    const r = target.getBoundingClientRect();
    await api.ext.openPopup(e.id, e.popup, { x: r.left, y: r.top, width: r.width, height: r.height });
  }

  function extensionContext(ev: MouseEvent, e: ExtensionInfo) {
    ev.preventDefault();
    const items: MenuItemDef[] = [
      { label: e.pinned ? 'Unpin from toolbar' : 'Pin to toolbar', icon: 'pin', run: () => api.ext.setPinned(e.id, !e.pinned) },
    ];
    if (e.optionsPage) items.push({ label: 'Options', icon: 'settings', run: () => api.ext.openOptions(e.id, e.optionsPage!) });
    items.push('separator', { label: `Remove ${e.name}`, icon: 'trash', danger: true, run: () => api.ext.remove(e.id) });
    extMenu = { x: ev.clientX, y: ev.clientY, items };
    void ui.open('tabMenu');
  }

  function overflow(ev: MouseEvent) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItemDef[] = enabled.map((e) => ({
      label: e.name,
      icon: 'puzzle' as const,
      run: () => void openPopup(e, ev.currentTarget as HTMLElement),
    }));
    if (items.length) items.push('separator');
    items.push({ label: 'Manage extensions', icon: 'settings', run: () => api.tabs.create('limbo://extensions') });
    items.push({ label: 'Get extensions', icon: 'externalLink', run: () => api.tabs.create('https://chromewebstore.google.com/category/extensions') });
    extMenu = { x: r.right - 220, y: r.bottom + 6, items };
    void ui.open('tabMenu');
  }

  function openAppMenu(ev: MouseEvent) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    appMenu = { x: r.right - 250, y: r.bottom + 6 };
    void ui.open('appMenu');
  }
</script>

<div class="actions">
  {#each visible as e (e.id)}
    <button
      class="icon-btn ext"
      aria-label={e.actionTitle ?? e.name}
      title={e.actionTitle ?? e.name}
      onclick={(ev) => openPopup(e, ev.currentTarget)}
      oncontextmenu={(ev) => extensionContext(ev, e)}
    >
      {#if e.icon}<img src={e.icon} alt="" width="16" height="16" />{:else}<Icon name="puzzle" />{/if}
    </button>
  {/each}
  {#if browser.extensions.length > 0}
    <button class="icon-btn" aria-label="Extensions" title="Extensions" onclick={overflow}>
      <Icon name="puzzle" />
    </button>
  {/if}
  {#if showMemory}
    <MemoryPill />
  {/if}
  {#if recentDownloads}
    <button class="icon-btn dl" aria-label="Downloads" title="Downloads (Ctrl+J)" onclick={() => ui.toggle('downloads')}>
      <Icon name="download" />
      {#if progress !== null}
        <svg class="ring" viewBox="0 0 28 28" aria-hidden="true">
          <circle cx="14" cy="14" r="12" pathLength="100" stroke-dasharray="{progress * 100} 100" />
        </svg>
      {/if}
    </button>
  {/if}
  <button class="icon-btn" aria-label="Menu" title="Menu" onclick={openAppMenu}>
    <Icon name="ellipsis" />
  </button>
</div>

{#if extMenu}
  <Menu items={extMenu.items} x={extMenu.x} y={extMenu.y} onclose={() => { extMenu = null; ui.close('tabMenu'); }} />
{/if}
{#if appMenu && ui.isOpen('appMenu')}
  <Menu
    items={appMenuItems()}
    x={appMenu.x}
    y={appMenu.y}
    minWidth={250}
    origin="top right"
    onclose={() => { appMenu = null; ui.close('appMenu'); }}
  />
{/if}

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px;
    flex: none;
  }
  .ext img {
    border-radius: 3px;
  }
  .dl {
    position: relative;
  }
  .ring {
    position: absolute;
    inset: 0;
    transform: rotate(-90deg);
    pointer-events: none;
  }
  .ring circle {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
    transition: stroke-dasharray 300ms ease-out;
  }
</style>
