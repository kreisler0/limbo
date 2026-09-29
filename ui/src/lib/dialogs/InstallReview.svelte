<script lang="ts">
  // "Add <extension>?" — what it can do, whether it will work, then install.
  import Dialog from './Dialog.svelte';
  import Icon from '../components/Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import type { InstallReview } from '../types';

  let { review, onclose }: { review: InstallReview; onclose: () => void } = $props();
  let busy = $state(false);

  const blocked = $derived(review.compatibility === 'firefoxOnly');

  async function install() {
    busy = true;
    try {
      const ext = await api.ext.confirm(review.token);
      browser.toast(`${ext.name} added`);
      onclose();
    } catch (e) {
      browser.toast(String(e));
      busy = false;
    }
  }
</script>

<Dialog label={`Add ${review.name}`} width={460} {onclose}>
  <div class="head">
    {#if review.icon}<img src={review.icon} alt="" width="40" height="40" />{:else}<span class="ph"><Icon name="puzzle" size={20} /></span>{/if}
    <div>
      <h2>Add “{review.name}”?</h2>
      <p class="meta">Version {review.version} · {review.source}</p>
    </div>
  </div>
  {#if review.description}<p class="sub">{review.description}</p>{/if}

  {#if review.compatibility === 'mv2Deprecated'}
    <div class="note warn">
      <Icon name="alert" size={16} />
      <span>This extension uses Manifest V2, which the Edge engine no longer runs reliably. It may not work. Look for a Manifest V3 version.</span>
    </div>
  {:else if blocked}
    <div class="note bad">
      <Icon name="alert" size={16} />
      <span>This is a Firefox-only add-on and can’t run in Limbo.</span>
    </div>
  {/if}

  {#if review.warnings.length}
    <h3>It will be able to:</h3>
    <ul>
      {#each review.warnings as w (w)}
        <li><Icon name="check" size={14} /> {w}</li>
      {/each}
    </ul>
  {:else}
    <p class="sub">It doesn’t ask for any special permissions.</p>
  {/if}

  <div class="actions">
    <button class="btn ghost" onclick={onclose}>Cancel</button>
    <button class="btn primary" disabled={busy || blocked} onclick={install}>{busy ? 'Adding…' : 'Add extension'}</button>
  </div>
</Dialog>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 12px;
  }
  .head img,
  .ph {
    flex: none;
    border-radius: 9px;
  }
  .ph {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    background: var(--bg-hover);
    color: var(--text-muted);
  }
  .head h2 {
    margin: 0;
  }
  .meta {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  h3 {
    margin: 4px 0 8px;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 4px 0;
    color: var(--text-muted);
    font-size: var(--text-md);
  }
  li :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  .note {
    display: flex;
    gap: 10px;
    margin-bottom: 14px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    line-height: var(--text-sm-lh);
  }
  .note :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .warn {
    background: var(--warning-soft);
  }
  .bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
</style>
