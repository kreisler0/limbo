<script lang="ts">
  // Import from Firefox: pick a profile → choose what → progress → summary
  // (with add-on suggestions). Also embedded in onboarding (`embedded`).
  import Dialog from './Dialog.svelte';
  import InstallReview from './InstallReview.svelte';
  import Icon from '../components/Icon.svelte';
  import { api, on } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import type { FirefoxAddon, FirefoxProfile, ImportProgress, ImportSummary, ImportType, InstallReview as Review } from '../types';

  let { onclose, embedded = false, ondone }: { onclose: () => void; embedded?: boolean; ondone?: (s: ImportSummary | null) => void } = $props();

  const TYPES: { id: ImportType; label: string }[] = [
    { id: 'passwords', label: 'Passwords' },
    { id: 'history', label: 'History' },
    { id: 'bookmarks', label: 'Bookmarks' },
    { id: 'cookies', label: 'Cookies (stay signed in)' },
    { id: 'formHistory', label: 'Form entries' },
    { id: 'favicons', label: 'Site icons' },
    { id: 'tabs', label: 'Open tabs' },
    { id: 'addons', label: 'Add-ons list' },
  ];

  let step = $state<'pick' | 'running' | 'password' | 'done'>('pick');
  let profiles = $state<FirefoxProfile[] | null>(null);
  let profile = $state<FirefoxProfile | null>(null);
  // Open tabs are imported by default (owner's choice, see docs/DECISIONS.md).
  let chosen = $state<Record<ImportType, boolean>>({ passwords: true, history: true, bookmarks: true, cookies: true, formHistory: true, favicons: true, tabs: true, addons: true });
  let progress = $state<Partial<Record<ImportType, ImportProgress>>>({});
  let summary = $state<ImportSummary | null>(null);
  let primaryPassword = $state('');
  let review = $state<Review | null>(null);
  let adding = $state<string | null>(null);

  $effect(() => {
    api.import.detect().then((p) => {
      profiles = p;
      profile = p.find((x) => x.isDefault) ?? p[0] ?? null;
    });
    return on<ImportProgress>('import:progress', (p) => (progress[p.dataType] = p));
  });

  async function choose() {
    const p = await api.import.chooseFolder().catch((e) => (browser.toast(String(e)), null));
    if (p) {
      profiles = [...(profiles ?? []).filter((x) => x.path !== p.path), p];
      profile = p;
    }
  }

  async function run(types: ImportType[], password?: string) {
    if (!profile) return;
    step = 'running';
    progress = {};
    try {
      const s = await api.import.run(profile.path, types, password);
      summary = summary && password ? { ...summary, passwords: s.passwords, needsPrimaryPassword: s.needsPrimaryPassword } : s;
      step = s.needsPrimaryPassword ? 'password' : 'done';
      if (step === 'done') ondone?.(summary);
    } catch (e) {
      browser.toast(String(e));
      step = 'pick';
    }
  }

  const selectedTypes = $derived(TYPES.map((t) => t.id).filter((t) => chosen[t]));

  async function addAddon(a: FirefoxAddon) {
    if (!a.chrome) return;
    adding = a.id;
    review = await api.ext.prepareStore('chromeWebStore', a.chrome.id).catch((e) => (browser.toast(String(e)), null));
    adding = null;
  }

  const installed = $derived(new Set(browser.extensions.map((e) => e.id)));

  function n(x: number) {
    return x.toLocaleString();
  }
</script>

{#snippet body()}
  {#if step === 'pick'}
    <h2>Import from Firefox</h2>
    <p class="sub">Limbo copies your data and never changes your Firefox profile.</p>
    {#if profiles === null}
      <p class="muted">Looking for Firefox…</p>
    {:else if profiles.length === 0}
      <p class="muted">Firefox isn’t installed for this Windows account. If you have a profile folder elsewhere, choose it.</p>
    {:else}
      <div class="profiles" role="radiogroup" aria-label="Firefox profile">
        {#each profiles as p (p.path)}
          <label class="profile" class:on={profile?.path === p.path}>
            <input type="radio" name="profile" checked={profile?.path === p.path} onchange={() => (profile = p)} />
            <span>
              <strong>{p.name}{p.isDefault ? ' (default)' : ''}</strong>
              <small>{p.path}</small>
            </span>
          </label>
        {/each}
      </div>
      {#if profile?.running}
        <div class="note"><Icon name="info" size={15} /> Firefox is open. Import works anyway, but closing Firefox first gets your very latest tabs and cookies.</div>
      {/if}
    {/if}
    <button class="btn ghost choose" onclick={choose}><Icon name="folder" size={14} /> Choose a profile folder…</button>

    {#if profile}
      <div class="types">
        {#each TYPES as t (t.id)}
          <label><input type="checkbox" bind:checked={chosen[t.id]} /> {t.label}</label>
        {/each}
      </div>
    {/if}
    <div class="actions">
      <button class="btn ghost" onclick={onclose}>{embedded ? 'Skip' : 'Cancel'}</button>
      <button class="btn primary" disabled={!profile || !selectedTypes.length} onclick={() => run(selectedTypes)}>Import</button>
    </div>
  {:else if step === 'running'}
    <h2>Importing…</h2>
    <p class="sub">This takes a few seconds. Big histories take a little longer.</p>
    <ul class="progress">
      {#each TYPES.filter((t) => chosen[t.id] || progress[t.id]) as t (t.id)}
        {@const p = progress[t.id]}
        <li>
          <span class="state" class:done={p?.status === 'done'} class:failed={p?.status === 'failed'}>
            {#if p?.status === 'done'}<Icon name="check" size={13} />{:else if p?.status === 'failed'}<Icon name="x" size={13} />{:else if p}<span class="spin"></span>{/if}
          </span>
          {t.label}
          {#if p?.status === 'running' && p.total > 0}<span class="count tabular">{Math.round((p.done / p.total) * 100)}%</span>{/if}
        </li>
      {/each}
    </ul>
  {:else if step === 'password'}
    <h2>Firefox primary password</h2>
    <p class="sub">Your Firefox passwords are protected with a primary password. Enter it to import them. Limbo doesn’t keep it.</p>
    <form onsubmit={(e) => { e.preventDefault(); const pw = primaryPassword; primaryPassword = ''; void run(['passwords'], pw); }}>
      <input class="input wide" type="password" autocomplete="off" bind:value={primaryPassword} aria-label="Primary password" />
      <div class="actions">
        <button type="button" class="btn ghost" onclick={() => { step = 'done'; ondone?.(summary); }}>Skip passwords</button>
        <button type="submit" class="btn primary" disabled={!primaryPassword}>Import passwords</button>
      </div>
    </form>
  {:else if summary}
    <h2>Imported from Firefox</h2>
    <ul class="summary">
      {#if summary.passwords}<li><Icon name="key" size={15} /> {n(summary.passwords.imported)} passwords{summary.passwords.merged ? `, ${n(summary.passwords.merged)} already saved` : ''}{summary.passwords.failed ? ` (${n(summary.passwords.failed)} couldn’t be read)` : ''}</li>{/if}
      {#if summary.history}<li><Icon name="history" size={15} /> {n(summary.history.pages)} pages of history</li>{/if}
      {#if summary.bookmarks !== null}<li><Icon name="star" size={15} /> {n(summary.bookmarks)} bookmarks</li>{/if}
      {#if summary.cookies !== null}<li><Icon name="globe" size={15} /> {n(summary.cookies)} cookies</li>{/if}
      {#if summary.formHistory !== null}<li><Icon name="clipboard" size={15} /> {n(summary.formHistory)} form entries</li>{/if}
      {#if summary.tabs !== null}<li><Icon name="grid" size={15} /> {n(summary.tabs)} open tabs</li>{/if}
    </ul>
    {#each summary.errors as e (e.dataType)}
      <div class="note bad"><Icon name="alert" size={15} /> {TYPES.find((t) => t.id === e.dataType)?.label}: {e.message}</div>
    {/each}

    {#if summary.addons.length}
      <h3>Your add-ons</h3>
      <ul class="addons">
        {#each summary.addons as a (a.id)}
          <li>
            <span class="grow">
              <strong>{a.name}</strong>
              <small>{a.chrome ? (a.chrome.name !== a.name ? `Chrome version: ${a.chrome.name}` : 'Available for Chrome') : a.firefoxOnly ? 'Firefox only' : 'No known Chrome version'}</small>
            </span>
            {#if a.chrome && installed.has(a.chrome.id)}
              <span class="added"><Icon name="check" size={14} /> Added</span>
            {:else if a.chrome}
              <button class="btn" disabled={adding === a.id} onclick={() => addAddon(a)}>{adding === a.id ? '…' : 'Add'}</button>
            {:else if !a.firefoxOnly}
              <button class="btn ghost" onclick={() => api.tabs.create(`https://chromewebstore.google.com/search/${encodeURIComponent(a.name)}`, { background: true })}>Search</button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if summary.notImported.length}
      <p class="muted small">Not imported: {summary.notImported.join(', ')}.</p>
    {/if}
    <div class="actions">
      <button class="btn primary" onclick={onclose}>{embedded ? 'Continue' : 'Done'}</button>
    </div>
  {/if}
{/snippet}

{#if embedded}
  {@render body()}
{:else}
  <Dialog label="Import from Firefox" width={500} onclose={step === 'running' ? undefined : onclose}>
    {@render body()}
  </Dialog>
{/if}

{#if review}
  <InstallReview {review} onclose={() => (review = null)} />
{/if}

<style>
  .muted {
    color: var(--text-muted);
    font-size: var(--text-md);
  }
  .small {
    font-size: var(--text-sm);
    margin-top: 14px;
  }
  .profiles {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .profile {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    box-shadow: 0 0 0 1px var(--divider);
  }
  .profile.on {
    box-shadow: 0 0 0 1.5px var(--accent);
    background: var(--accent-soft);
  }
  .profile input,
  .types input {
    accent-color: var(--accent-strong);
  }
  .profile span {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  strong {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }
  small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .choose {
    margin-top: 8px;
    padding-left: 6px;
  }
  .types {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px 16px;
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--divider);
    font-size: var(--text-md);
  }
  .types label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
    font-size: var(--text-sm);
    line-height: var(--text-sm-lh);
  }
  .note :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .note.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .progress,
  .summary,
  .addons {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .progress li {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    font-size: var(--text-md);
  }
  .state {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--bg-hover);
    color: var(--on-accent);
  }
  .state.done {
    background: var(--success);
  }
  .state.failed {
    background: var(--danger);
  }
  .spin {
    width: 12px;
    height: 12px;
    border: 2px solid var(--accent);
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }
  .count {
    margin-left: auto;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .summary li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 0;
    font-size: var(--text-md);
  }
  .summary :global(svg) {
    color: var(--text-muted);
  }
  h3 {
    margin: 18px 0 6px;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }
  .addons li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
  }
  .addons li + li {
    border-top: 1px solid var(--divider);
  }
  .grow {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .added {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--success);
    font-size: var(--text-sm);
  }
  .wide {
    width: 100%;
  }
</style>
