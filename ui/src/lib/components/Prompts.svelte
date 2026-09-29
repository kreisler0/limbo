<script lang="ts">
  // Requests from pages that need an answer: permission prompts and "Save
  // password?". Anchored under the omnibox; they cover the page, so they use
  // the snapshot overlay while visible.
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { pop } from '../design/motion';
  import { permissionAsk, permissionIcon } from '../permissions';

  const perm = $derived(browser.permissionPrompts.find((p) => p.tabId === browser.activeId) ?? null);
  const save = $derived(perm ? null : (browser.savePrompts.find((p) => p.tabId === browser.activeId) ?? null));
  const showing = $derived(!!perm || !!save);
  let remember = $state(true);

  $effect(() => {
    if (showing) void overlay.cover('prompt');
    else void overlay.uncover('prompt');
  });

  function host(origin: string) {
    try {
      return new URL(origin).host.replace(/^www\./, '');
    } catch {
      return origin;
    }
  }

  function answerPermission(allow: boolean) {
    if (!perm) return;
    void api.respond(perm.requestId, { allow, remember });
    browser.permissionPrompts = browser.permissionPrompts.filter((p) => p !== perm);
  }

  function answerSave(action: 'save' | 'never' | 'notNow') {
    if (!save) return;
    void api.respond(save.promptId, { action });
    browser.savePrompts = browser.savePrompts.filter((p) => p !== save);
  }
</script>

{#if perm}
  {#key perm.requestId}
    <div class="prompt float" role="alertdialog" aria-label="Permission request" in:pop={{ origin: 'top left', y: -6 }}>
      <div class="title">
        <span class="ic"><Icon name={permissionIcon(perm.kind)} size={16} /></span>
        <span><b>{host(perm.origin)}</b> wants to {permissionAsk(perm.kind)}</span>
      </div>
      <label class="check"><input type="checkbox" bind:checked={remember} /> Remember for this site</label>
      <div class="buttons">
        <button class="btn" onclick={() => answerPermission(false)}>Block</button>
        <button class="btn primary" onclick={() => answerPermission(true)}>Allow</button>
      </div>
    </div>
  {/key}
{:else if save}
  {#key save.promptId}
    <div class="prompt float" role="alertdialog" aria-label="Save password" in:pop={{ origin: 'top left', y: -6 }}>
      <div class="title">
        <span class="ic"><Icon name="key" size={16} /></span>
        <span>{save.update ? 'Update' : 'Save'} password for <b>{host(save.origin)}</b>?</span>
      </div>
      <div class="user">{save.username || 'No username'}</div>
      <div class="buttons">
        <button class="btn ghost" onclick={() => answerSave('never')}>Never for this site</button>
        <span class="grow"></span>
        <button class="btn" onclick={() => answerSave('notNow')}>Not now</button>
        <button class="btn primary" onclick={() => answerSave('save')}>{save.update ? 'Update' : 'Save'}</button>
      </div>
    </div>
  {/key}
{/if}

<style>
  .prompt {
    position: fixed;
    top: 50px;
    left: max(12px, calc(50vw - 360px));
    width: 340px;
    padding: 14px;
    z-index: 55;
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    font-size: var(--text-md);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-text);
    flex: none;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 10px 0 0 38px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .user {
    margin: 8px 0 0 38px;
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
    font-size: var(--text-sm);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 14px;
  }
  .grow {
    flex: 1;
  }
</style>
