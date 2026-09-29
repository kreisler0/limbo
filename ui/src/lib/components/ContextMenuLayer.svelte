<script lang="ts">
  // The page's context menu: WebView2's own is suppressed, the host sends the
  // curated items, we draw them and answer with the chosen item id.
  import Menu, { type MenuItemDef } from './Menu.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { IconName } from '../design/icons';

  const req = $derived(browser.contextMenu);
  let answered = false;

  $effect(() => {
    if (req) {
      answered = false;
      void ui.open('contextMenu');
    }
  });

  function icon(label: string): IconName | undefined {
    const l = label.toLowerCase();
    if (l.startsWith('open link in private')) return 'glasses';
    if (l.startsWith('open')) return 'arrowUpRight';
    if (l.startsWith('search google')) return 'search';
    if (l.startsWith('copy')) return 'copy';
    if (l.startsWith('save')) return 'download';
    if (l.startsWith('print')) return 'printer';
    if (l === 'back') return 'arrowLeft';
    if (l === 'forward') return 'arrowRight';
    if (l.startsWith('reload')) return 'rotateCw';
    if (l.startsWith('inspect')) return 'command';
    return undefined;
  }

  const items = $derived.by((): MenuItemDef[] => {
    if (!req) return [];
    const menuId = req.menuId;
    return req.items.map((it) =>
      it.kind === 'separator'
        ? 'separator'
        : {
            label: it.label,
            icon: icon(it.label),
            shortcut: it.shortcut ?? undefined,
            disabled: !it.enabled,
            checked: it.kind === 'checkbox' ? it.checked : undefined,
            run: () => {
              answered = true;
              void api.respond(menuId, { itemId: it.id });
            },
          },
    );
  });

  // The host reports the click in physical pixels inside the page area.
  const pos = $derived.by(() => {
    if (!req) return { x: 0, y: 0 };
    const dpr = window.devicePixelRatio || 1;
    return { x: ui.content.x + req.x / dpr, y: ui.content.y + req.y / dpr };
  });

  function close() {
    if (req && !answered) void api.respond(req.menuId, { itemId: null });
    browser.contextMenu = null;
    ui.close('contextMenu');
  }
</script>

{#if req}
  <Menu {items} x={pos.x} y={pos.y} minWidth={220} onclose={close} />
{/if}
