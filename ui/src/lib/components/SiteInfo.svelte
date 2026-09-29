<script lang="ts">
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { permissionLabel, permissionIcon } from '../permissions';
  import type { SitePermission } from '../types';

  const t = browser.active;
  let perms = $state<SitePermission[]>([]);
  const origin = $derived.by(() => {
    try {
      return t ? new URL(t.url).origin : '';
    } catch {
      return '';
    }
  });

  $effect(() => {
    if (origin) api.sites.permissions(origin).then((p) => (perms = p));
  });

  async function revoke(p: SitePermission) {
    await api.sites.revoke(p.origin, p.kind);
    perms = perms.filter((x) => x !== p);
  }
</script>

<Popover label="Site information" width={320} left={120} origin="top left" onclose={() => ui.close('siteInfo')}>
  <div class="head">
    {#if t?.favicon}<img src={t.favicon} alt="" width="20" height="20" />{/if}
    <div>
      <div class="host">{t?.displayHost}</div>
      <div class="sec" class:bad={t?.security === 'insecure'}>
        <Icon name={t?.security === 'secure' ? 'lock' : 'lockOpen'} size={12} />
        {t?.security === 'secure' ? 'Connection is secure' : t?.security === 'file' ? 'Local file' : 'Not secure: information you send could be read by others'}
      </div>
    </div>
  </div>
  {#if perms.length}
    <div class="label">Permissions</div>
    <ul>
      {#each perms as p (p.kind)}
        <li>
          <Icon name={permissionIcon(p.kind)} size={14} />
          <span class="name">{permissionLabel(p.kind)}</span>
          <span class="state" class:allow={p.allow}>{p.allow ? 'Allowed' : 'Blocked'}</span>
          <button class="btn ghost small" onclick={() => revoke(p)}>Reset</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted">This site hasn't asked for any permissions.</p>
  {/if}
</Popover>

<style>
  .head {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    margin-bottom: 12px;
  }
  .head img {
    border-radius: 4px;
    margin-top: 2px;
  }
  .host {
    font-weight: var(--weight-semibold);
  }
  .sec {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .sec.bad {
    color: var(--warning);
  }
  .label {
    margin: 6px 0 4px;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }
  .name {
    flex: 1;
    color: var(--text);
  }
  .state {
    font-size: var(--text-xs);
  }
  .state.allow {
    color: var(--success);
  }
  .small {
    height: 22px;
    font-size: var(--text-xs);
  }
  .muted {
    color: var(--text-muted);
    font-size: var(--text-sm);
    margin: 0;
  }
</style>
