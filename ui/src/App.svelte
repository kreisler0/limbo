<script lang="ts">
  import TitleBar from './lib/components/TitleBar.svelte';
  import BookmarksBar from './lib/components/BookmarksBar.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import FindBar from './lib/components/FindBar.svelte';
  import Menu from './lib/components/Menu.svelte';
  import MemoryPopover from './lib/components/MemoryPopover.svelte';
  import DownloadsPopover from './lib/components/DownloadsPopover.svelte';
  import BookmarkPopover from './lib/components/BookmarkPopover.svelte';
  import SiteInfo from './lib/components/SiteInfo.svelte';
  import AutofillPicker from './lib/components/AutofillPicker.svelte';
  import Prompts from './lib/components/Prompts.svelte';
  import ContextMenuLayer from './lib/components/ContextMenuLayer.svelte';
  import CommandPalette from './lib/components/CommandPalette.svelte';
  import TabOverview from './lib/components/TabOverview.svelte';
  import NewTab from './lib/pages/NewTab.svelte';
  import Settings from './lib/pages/Settings.svelte';
  import History from './lib/pages/History.svelte';
  import Downloads from './lib/pages/Downloads.svelte';
  import Bookmarks from './lib/pages/Bookmarks.svelte';
  import PagePlaceholder from './lib/pages/PagePlaceholder.svelte';
  import ClearData from './lib/dialogs/ClearData.svelte';
  import InstallReview from './lib/dialogs/InstallReview.svelte';
  import ImportWizard from './lib/dialogs/ImportWizard.svelte';
  import Onboarding from './lib/dialogs/Onboarding.svelte';
  import { api, inTauri } from './lib/ipc';
  import { browser } from './lib/stores/browser.svelte';
  import { overlay } from './lib/stores/overlay.svelte';
  import { ui } from './lib/stores/ui.svelte';
  import { classify } from './lib/shortcuts';
  import { runShortcut, tabMenuItems } from './lib/menus';

  let content = $state<HTMLDivElement>();
  let onboarding = $state(false);

  const t = $derived(browser.active);
  const s = $derived(browser.settings);
  const fullscreen = $derived(browser.win.fullscreen);

  browser.onShortcut(runShortcut);

  // --- the page area -----------------------------------------------------------------------------
  // The host lays the active tab's webview exactly over `.content`; tell it
  // whenever the chrome around it changes size.
  let sent = '';
  function reportInsets() {
    if (!content) return;
    const r = content.getBoundingClientRect();
    ui.content = { x: r.left, y: r.top, width: r.width, height: r.height };
    const insets = {
      top: Math.round(r.top),
      left: Math.round(r.left),
      right: Math.round(window.innerWidth - r.right),
      bottom: Math.round(window.innerHeight - r.bottom),
    };
    const key = JSON.stringify(insets);
    if (key !== sent) {
      sent = key;
      void api.layout.setInsets(insets);
    }
  }

  $effect(() => {
    if (!content) return;
    const ro = new ResizeObserver(reportInsets);
    ro.observe(content);
    window.addEventListener('resize', reportInsets);
    reportInsets();
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', reportInsets);
    };
  });

  // --- theme, private tabs, Mica ------------------------------------------------------------------
  $effect(() => {
    const root = document.documentElement;
    if (!s || s.theme === 'system') root.removeAttribute('data-theme');
    else root.dataset.theme = s.theme;
    root.dataset.private = String(!!t?.private);
    root.dataset.mica = String(!!(s?.mica && browser.platform?.mica));
    root.dataset.fullscreen = String(fullscreen);
  });

  // --- layers that open from host events ------------------------------------------------------------
  $effect(() => {
    if (browser.autofillFor !== null) void ui.open('autofill');
  });

  $effect(() => {
    // Switching tabs closes page-bound layers (their snapshot is stale).
    void browser.activeId;
    ui.findOpen = false;
    for (const l of ['siteInfo', 'bookmark', 'autofill'] as const) if (ui.isOpen(l)) ui.close(l);
  });

  $effect(() => {
    if (!browser.ready || !s || s.onboardingDone || onboarding) return;
    onboarding = true;
    void overlay.cover('onboarding');
  });

  function endOnboarding() {
    onboarding = false;
    void overlay.uncover('onboarding');
  }

  function closeDialog() {
    ui.dialog = null;
    ui.close('dialog');
  }

  // --- keyboard and mouse ----------------------------------------------------------------------------
  function onKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    const shortcut = classify(e);
    if (!shortcut) return;
    // Let text fields keep their own Escape behaviour.
    if (shortcut === 'escape' && !ui.top && !ui.findOpen && !ui.sidebarOpen) return;
    e.preventDefault();
    runShortcut(shortcut);
  }

  function onMouseUp(e: MouseEvent) {
    // Mouse back/forward buttons over the chrome.
    if (e.button === 3) runShortcut('back');
    else if (e.button === 4) runShortcut('forward');
  }

  function internalPage(page: NonNullable<typeof t>['internal']) {
    return page;
  }
</script>

<svelte:window onkeydown={onKeydown} onmouseup={onMouseUp} />

<div class="app">
  {#if !fullscreen}
    <TitleBar />
    {#if s?.bookmarksBarVisible}<BookmarksBar />{/if}
  {/if}

  <div class="body">
    {#if ui.sidebarOpen && !fullscreen}<Sidebar />{/if}
    <div class="column">
      <div class="content" bind:this={content}>
        {#if t}
          {@const page = internalPage(t.internal)}
          {#key t.id}
            {#if page === 'newTab'}
              <NewTab />
            {:else if page === 'settings'}
              <Settings />
            {:else if page === 'extensions' || page === 'passwords'}
              <Settings initial={page} />
            {:else if page === 'history'}
              <History />
            {:else if page === 'downloads'}
              <Downloads />
            {:else if page === 'bookmarks'}
              <Bookmarks />
            {:else if t.crashed || t.state === 'discarded'}
              <PagePlaceholder tab={t} />
            {:else if !inTauri}
              {#await import('./lib/dev/MockPage.svelte') then m}
                <m.default tab={t} />
              {/await}
            {/if}
          {/key}
        {/if}

        {#if overlay.snapshot}
          <!-- The page's picture while something covers it (see overlay.svelte.ts). -->
          <img class="snapshot" src={overlay.snapshot} alt="" />
        {/if}
      </div>
      {#if ui.findOpen && t && !t.internal}
        {#key t.id}<FindBar />{/key}
      {/if}
    </div>
  </div>
</div>

{#if ui.isOpen('memory')}<MemoryPopover />{/if}
{#if ui.isOpen('downloads')}<DownloadsPopover />{/if}
{#if ui.isOpen('bookmark')}<BookmarkPopover />{/if}
{#if ui.isOpen('siteInfo')}<SiteInfo />{/if}
{#if ui.isOpen('autofill')}<AutofillPicker />{/if}
{#if ui.tabMenu && ui.isOpen('tabMenu')}
  <Menu
    items={tabMenuItems(ui.tabMenu.id)}
    x={ui.tabMenu.x}
    y={ui.tabMenu.y}
    onclose={() => {
      ui.tabMenu = null;
      ui.close('tabMenu');
    }}
  />
{/if}
<ContextMenuLayer />
<Prompts />
{#if ui.isOpen('overview')}<TabOverview />{/if}
{#if ui.isOpen('palette')}<CommandPalette />{/if}

{#if ui.dialog && ui.isOpen('dialog')}
  {#if ui.dialog.kind === 'clearData'}
    <ClearData onclose={closeDialog} />
  {:else if ui.dialog.kind === 'install'}
    <InstallReview review={ui.dialog.review} onclose={closeDialog} />
  {:else if ui.dialog.kind === 'import'}
    <ImportWizard onclose={closeDialog} />
  {/if}
{/if}

{#if onboarding}<Onboarding onclose={endOnboarding} />{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .column {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .content {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--bg);
    overflow: hidden;
  }
  .snapshot {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    pointer-events: none;
  }
</style>
