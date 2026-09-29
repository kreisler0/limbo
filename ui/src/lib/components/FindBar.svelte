<script lang="ts">
  // Find in page. It must not cover the page (highlights update live), so it
  // takes a slim strip from the page area instead of using the overlay.
  import Icon from './Icon.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';

  let text = $state('');
  let matchCase = $state(false);
  let input = $state<HTMLInputElement>();
  const id = browser.activeId;

  $effect(() => {
    input?.focus();
    input?.select();
  });

  $effect(() => {
    const q = text;
    const c = matchCase;
    if (id === null) return;
    const h = setTimeout(() => (q ? api.find.start(id, q, c) : api.find.stop(id)), 60);
    return () => clearTimeout(h);
  });

  function close() {
    if (id !== null) void api.find.stop(id);
    browser.find = null;
    ui.findOpen = false;
  }

  const result = $derived(browser.find && browser.find.tabId === id ? browser.find : null);
</script>

<div class="find" role="search">
  <Icon name="search" size={14} />
  <input
    bind:this={input}
    bind:value={text}
    placeholder="Find in page"
    aria-label="Find in page"
    onkeydown={(e) => {
      if (e.key === 'Enter' && id !== null) {
        e.preventDefault();
        void api.find.step(id, !e.shiftKey);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    }}
  />
  <span class="count tabular">{text && result ? (result.matches ? `${result.active} of ${result.matches}` : 'No results') : ''}</span>
  <button class="icon-btn toggle" class:on={matchCase} aria-pressed={matchCase} title="Match case" onclick={() => (matchCase = !matchCase)}>
    <span class="aa">Aa</span>
  </button>
  <button class="icon-btn" aria-label="Previous match" disabled={!result?.matches} onclick={() => id !== null && api.find.step(id, false)}>
    <Icon name="chevronDown" size={14} class="up" />
  </button>
  <button class="icon-btn" aria-label="Next match" disabled={!result?.matches} onclick={() => id !== null && api.find.step(id, true)}>
    <Icon name="chevronDown" size={14} />
  </button>
  <button class="icon-btn" aria-label="Close find" onclick={close}><Icon name="x" size={14} /></button>
</div>

<style>
  .find {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 40px;
    padding: 0 10px 0 16px;
    border-top: 1px solid var(--divider);
    background: var(--bg-subtle);
    color: var(--text-muted);
    flex: none;
  }
  input {
    flex: 0 1 280px;
    height: 28px;
    border: 0;
    background: transparent;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .count {
    flex: 1;
    font-size: var(--text-xs);
  }
  .toggle.on {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .aa {
    font-size: 12px;
    font-weight: var(--weight-semibold);
  }
  :global(.up) {
    transform: rotate(180deg);
  }
</style>
