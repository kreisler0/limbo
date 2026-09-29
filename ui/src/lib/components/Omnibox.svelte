<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import SuggestionList from './SuggestionList.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { pop } from '../design/motion';
  import type { Suggestion } from '../types';

  let input = $state<HTMLInputElement>();
  let focused = $state(false);
  let text = $state('');
  /** What the user actually typed (text minus inline autocomplete). */
  let typed = $state('');
  let rows = $state<Suggestion[]>([]);
  let selected = $state(0);
  let deleting = false;
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let bookmarked = $state(false);

  const t = $derived(browser.active);
  const toast = $derived(browser.toasts[0] ?? null);
  const zoomed = $derived(t && Math.abs(t.zoom - 1) > 0.001 ? Math.round(t.zoom * 100) : null);
  const logins = $derived(t ? (browser.logins[t.id] ?? 0) : 0);
  const storeExt = $derived(t?.storeExtension && !browser.extensions.some((e) => e.id === t.storeExtension?.id) ? t.storeExtension : null);

  // The focused text: search terms on Google results pages, else the full URL.
  function focusText(): string {
    if (!t || t.internal) return '';
    return t.searchTerms ?? t.url;
  }

  // Ask the host whether this page is bookmarked (star state).
  $effect(() => {
    const url = t?.url;
    if (!url || t?.internal) {
      bookmarked = false;
      return;
    }
    void browser.bookmarks;
    api.bookmarks.forUrl(url).then((b) => (bookmarked = b.length > 0)).catch(() => {});
  });

  // Focus requests (Ctrl+L, new tab).
  let lastSeq = 0;
  $effect(() => {
    const seq = ui.focusOmniboxSeq;
    if (seq !== lastSeq) {
      lastSeq = seq;
      input?.focus();
      input?.select();
    }
  });

  // New Tab pages start with the omnibox focused.
  let lastAutoFocus: number | null = null;
  $effect(() => {
    if (t?.internal === 'newTab' && lastAutoFocus !== t.id) {
      lastAutoFocus = t.id;
      queueMicrotask(() => input?.focus());
    }
  });

  // Late Google suggestions for the current text.
  $effect(() => {
    const r = browser.remoteSuggestions;
    untrack(() => {
      if (focused && r && r.text === typed && typed) {
        const keep = rows[selected]?.url;
        rows = r.rows;
        const i = rows.findIndex((x) => x.url === keep);
        selected = i >= 0 ? i : 0;
      }
    });
  });

  function onFocus() {
    focused = true;
    ui.omniboxFocused = true;
    text = focusText();
    typed = text;
    rows = [];
    selected = 0;
    queueMicrotask(() => input?.select());
    void ui.open('omnibox');
  }

  function onBlur() {
    focused = false;
    ui.omniboxFocused = false;
    rows = [];
    ui.close('omnibox');
  }

  function applyAutocomplete() {
    const top = rows[0];
    if (!input || deleting || !top?.completion || document.activeElement !== input) return;
    if (input.selectionStart !== typed.length) return;
    text = typed + top.completion;
    input.value = text;
    input.setSelectionRange(typed.length, text.length);
  }

  function query() {
    const q = typed;
    if (!q.trim()) {
      rows = [];
      return;
    }
    api.suggest
      .query(q, deleting)
      .then((r) => {
        if (q !== typed) return;
        rows = r;
        selected = 0;
        applyAutocomplete();
      })
      .catch(() => {});
  }

  function onInput(e: Event) {
    const ev = e as InputEvent;
    deleting = !!ev.inputType?.startsWith('delete');
    typed = (e.target as HTMLInputElement).value;
    text = typed;
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(query, 80);
  }

  function go(row: Suggestion | null, opts: { newTab?: boolean } = {}) {
    if (!t) return;
    const id = t.id;
    if (row?.kind === 'openTab' && row.tabId) {
      void api.tabs.activate(row.tabId);
    } else if (row) {
      if (opts.newTab) void api.tabs.create(row.url, { typed: true });
      else void api.nav.url(id, row.url, true);
    } else {
      const value = text.trim();
      if (!value) return;
      void api.nav.go(id, value, opts.newTab);
    }
    input?.blur();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (!rows.length) return;
      e.preventDefault();
      selected = (selected + (e.key === 'ArrowDown' ? 1 : -1) + rows.length) % rows.length;
      const row = rows[selected];
      text = row.kind === 'search' ? row.title : selected === 0 ? typed + (row.completion ?? '') : row.url;
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.ctrlKey && !/[.\s/:]/.test(text.trim())) {
        text = `www.${text.trim()}.com`;
        go(null);
        return;
      }
      const autocompleted = text !== typed && rows[0]?.completion && selected === 0;
      const row = selected > 0 || autocompleted ? rows[selected] : null;
      go(row, { newTab: e.altKey });
      return;
    }
    if (e.key === 'Delete' && e.shiftKey) {
      const row = rows[selected];
      if (row && (row.kind === 'history' || row.kind === 'bookmark')) {
        e.preventDefault();
        void api.suggest.remove(row.url);
        rows = rows.filter((r) => r !== row);
        selected = Math.min(selected, rows.length - 1);
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (rows.length) {
        rows = [];
        text = typed;
      } else {
        text = focusText();
        input?.blur();
      }
    }
  }

  async function toggleBookmark() {
    if (!t || t.internal) return;
    if (bookmarked) {
      void ui.open('bookmark');
    } else {
      await api.bookmarks.add(t.title || t.displayHost, t.url);
      bookmarked = true;
      void ui.open('bookmark');
    }
  }

  async function addExtension() {
    if (!storeExt) return;
    try {
      const review = await api.ext.prepareStore(storeExt.store, storeExt.id);
      ui.dialog = { kind: 'install', review };
      void ui.open('dialog');
    } catch (err) {
      browser.toast(`Couldn't get the extension: ${err}`);
    }
  }
</script>

<div class="omni" class:focused class:private={t?.private}>
  {#if !focused}
    <button
      class="site"
      aria-label="Site information"
      title={t?.security === 'secure' ? 'Connection is secure' : t?.security === 'insecure' ? 'Not secure' : 'Site information'}
      onclick={() => t && !t.internal && ui.toggle('siteInfo')}
    >
      {#if t?.private}
        <Icon name="glasses" size={14} />
      {:else if t?.security === 'secure'}
        <Icon name="lock" size={13} />
      {:else if t?.security === 'insecure'}
        <Icon name="lockOpen" size={13} class="warn" />
      {:else}
        <Icon name="search" size={14} />
      {/if}
    </button>
  {:else}
    <span class="site static"><Icon name="search" size={14} /></span>
  {/if}

  <input
    bind:this={input}
    class="field"
    class:display={!focused}
    type="text"
    spellcheck="false"
    autocomplete="off"
    aria-label="Search Google or type a URL"
    aria-expanded={rows.length > 0}
    aria-controls="suggestions"
    aria-autocomplete="both"
    role="combobox"
    placeholder={focused || !t || t.internal === 'newTab' ? 'Search Google or type a URL' : ''}
    value={focused ? text : t?.internal === 'newTab' ? '' : (t?.displayHost ?? '')}
    onfocus={onFocus}
    onblur={onBlur}
    oninput={onInput}
    onkeydown={onKeydown}
  />

  {#if !focused}
    <div class="chips">
      {#if toast}
        <div class="chip toast" in:pop={{ origin: 'center right', from: 0.9 }}>
          <span>{toast.message}</span>
          {#if toast.action}
            <button class="link" onclick={() => { toast.action?.run(); browser.dismissToast(toast.id); }}>{toast.action.label}</button>
          {/if}
        </div>
      {:else}
        {#if storeExt}
          <button class="chip add" onclick={addExtension} in:pop={{ origin: 'center right' }}>
            <Icon name="plus" size={12} stroke={1.8} /> Add to Limbo
          </button>
        {/if}
        {#if zoomed}
          <button class="chip" title="Reset zoom" onclick={() => t && api.zoom(t.id, 'reset')} in:pop={{ origin: 'center right' }}>
            <Icon name="zoomIn" size={12} /> <span class="tabular">{zoomed}%</span>
          </button>
        {/if}
        {#if logins > 0}
          <button class="icon-btn mini" aria-label="Saved passwords" title="Fill a saved password" onclick={() => { browser.autofillFor = t?.id ?? null; void ui.open('autofill'); }}>
            <Icon name="key" size={14} />
          </button>
        {/if}
        {#if t && !t.internal}
          <button class="icon-btn mini star" class:on={bookmarked} aria-label={bookmarked ? 'Edit bookmark' : 'Bookmark this page'} title="Bookmark (Ctrl+D)" onclick={toggleBookmark}>
            <Icon name="star" size={14} />
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  {#if focused && rows.length}
    <SuggestionList {rows} {selected} {typed} onpick={(row) => go(row)} onhover={(i) => (selected = i)} />
  {/if}
</div>

<style>
  .omni {
    position: relative;
    display: flex;
    align-items: center;
    width: clamp(240px, 26vw, 540px);
    min-width: 200px;
    height: 32px;
    flex: 0 1 auto;
    border-radius: var(--radius-pill);
    background: var(--bg-hover);
    transition:
      background-color 160ms ease-out,
      box-shadow 160ms ease-out;
  }
  .omni:hover:not(.focused) {
    background: var(--bg-active);
  }
  .omni.focused {
    background: var(--bg-float);
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent),
      0 0 0 4px var(--accent-soft);
    z-index: 30;
  }
  .site {
    display: grid;
    place-items: center;
    width: 30px;
    height: 32px;
    margin-left: 3px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-muted);
    flex: none;
  }
  .site:hover:not(.static) {
    color: var(--text);
  }
  .site :global(.warn) {
    color: var(--warning);
  }
  .field {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    padding: 0 8px 0 2px;
    border: 0;
    background: transparent;
    font-size: var(--text-md);
    caret-color: var(--accent);
  }
  .field.display {
    color: var(--text);
    cursor: default;
    text-overflow: ellipsis;
  }
  .field::placeholder {
    color: var(--text-muted);
  }
  .field::selection {
    background: var(--accent-soft);
  }
  .chips {
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: 4px;
    flex: none;
  }
  .mini {
    width: 26px;
    height: 26px;
    border-radius: var(--radius-pill);
  }
  .star.on {
    color: var(--accent);
  }
  .star.on :global(svg) {
    fill: currentColor;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-pill);
    background: var(--bg);
    box-shadow: var(--shadow-tab);
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    white-space: nowrap;
  }
  .chip.add {
    background: var(--accent-strong);
    color: var(--on-accent);
    box-shadow: none;
  }
  .chip.toast {
    max-width: 320px;
    color: var(--text);
  }
  .chip.toast span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    font-weight: var(--weight-semibold);
  }
</style>
