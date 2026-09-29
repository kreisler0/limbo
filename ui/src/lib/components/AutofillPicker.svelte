<script lang="ts">
  import Icon from './Icon.svelte';
  import Popover from './Popover.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import type { LoginSummary } from '../types';

  let logins = $state<LoginSummary[]>([]);
  const tabId = browser.autofillFor ?? browser.activeId;

  $effect(() => {
    if (tabId !== null) api.passwords.forTab(tabId).then((l) => (logins = l));
  });

  function close() {
    browser.autofillFor = null;
    ui.close('autofill');
  }

  async function fill(l: LoginSummary) {
    if (tabId === null) return;
    // Logins from another subdomain need an explicit confirmation.
    if (l.sameSiteOnly && !confirm(`Use your ${new URL(l.origin).host} password on this page?`)) return;
    close();
    try {
      await api.passwords.fill(tabId, l.id);
    } catch (e) {
      browser.toast(String(e));
    }
  }
</script>

<Popover label="Saved passwords" width={300} right={200} onclose={close}>
  <h3>Fill a saved password</h3>
  <ul>
    {#each logins as l (l.id)}
      <li>
        <button onclick={() => fill(l)}>
          <Icon name="key" size={14} />
          <span class="user">{l.username || '(no username)'}</span>
          <span class="origin">{new URL(l.origin).host}</span>
        </button>
      </li>
    {:else}
      <li class="empty">No saved passwords for this site.</li>
    {/each}
  </ul>
</Popover>

<style>
  h3 {
    margin: 0 0 6px;
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--text-muted);
  }
  ul {
    margin: 0 -6px;
    padding: 0;
    list-style: none;
  }
  button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 34px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    color: var(--text-muted);
  }
  button:hover {
    background: var(--bg-hover);
  }
  .user {
    flex: 1;
    color: var(--text);
    font-size: var(--text-sm);
  }
  .origin {
    font-size: var(--text-xs);
  }
  .empty {
    padding: 8px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
