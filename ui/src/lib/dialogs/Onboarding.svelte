<script lang="ts">
  // First run: look → bring your stuff from Firefox → an ad blocker → default browser.
  import Icon from '../components/Icon.svelte';
  import Segmented from '../components/Segmented.svelte';
  import ImportWizard from './ImportWizard.svelte';
  import InstallReview from './InstallReview.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { fade, pop } from '../design/motion';
  import mark from '../../assets/limbo-mark.png';
  import type { InstallReview as Review, Theme } from '../types';

  let { onclose }: { onclose: () => void } = $props();

  const UBOL = 'ddkjiahejlhfcafbddmgiahcphecmpfh';
  const ALL_STEPS = ['welcome', 'import', 'extension', 'default'] as const;
  // A portable Limbo can't become the default browser, so that step is skipped.
  const STEPS = $derived(ALL_STEPS.filter((s) => s !== 'default' || !browser.platform?.portable));
  let step = $state<(typeof ALL_STEPS)[number]>('welcome');
  let review = $state<Review | null>(null);
  let preparing = $state(false);

  const index = $derived(STEPS.indexOf(step));
  const ubolInstalled = $derived(browser.extensions.some((e) => e.id === UBOL));

  function next() {
    const i = STEPS.indexOf(step);
    if (i === STEPS.length - 1 || (STEPS[i + 1] === 'default' && browser.isDefaultBrowser)) finish();
    else step = STEPS[i + 1];
  }

  function finish() {
    void browser.updateSettings({ onboardingDone: true });
    onclose();
  }

  async function addUbol() {
    preparing = true;
    review = await api.ext.prepareStore('chromeWebStore', UBOL).catch((e) => (browser.toast(String(e)), null));
    preparing = false;
  }

  async function makeDefault() {
    await api.defaultBrowser.open();
    // Windows Settings opens; check again when the user comes back.
    const check = () => api.defaultBrowser.status().then((d) => (browser.isDefaultBrowser = d));
    window.addEventListener('focus', check, { once: true });
  }
</script>

<div class="screen" transition:fade={{ duration: 200 }}>
  <div class="card float" in:pop={{ origin: 'center', from: 0.97, spring: 'gentle' }}>
    {#key step}
      <div class="content" in:fade={{ duration: 180 }}>
        {#if step === 'welcome'}
          <img class="mark" src={mark} alt="" width="72" height="72" />
          <h1>Welcome to Limbo</h1>
          <p class="sub">A small, fast browser. Tabs you aren’t using go to sleep so your PC stays quick.</p>
          <div class="field">
            <span>Appearance</span>
            <Segmented
              label="Theme"
              value={browser.settings?.theme ?? 'system'}
              options={[{ value: 'system', label: 'System' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }] as { value: Theme; label: string }[]}
              onchange={(v) => browser.updateSettings({ theme: v })}
            />
          </div>
          <div class="actions"><button class="btn primary big" onclick={next}>Get started</button></div>
        {:else if step === 'import'}
          <ImportWizard embedded onclose={next} />
        {:else if step === 'extension'}
          <div class="icon"><Icon name="shield" size={28} /></div>
          <h1>Block ads and trackers</h1>
          <p class="sub">uBlock Origin Lite blocks ads and trackers without slowing pages down. It’s the one extension we recommend.</p>
          <div class="actions">
            <button class="btn ghost" onclick={next}>{ubolInstalled ? 'Continue' : 'Not now'}</button>
            {#if ubolInstalled}
              <span class="added"><Icon name="check" size={15} /> Added</span>
            {:else}
              <button class="btn primary" disabled={preparing} onclick={addUbol}>{preparing ? 'Getting it…' : 'Add uBlock Origin Lite'}</button>
            {/if}
          </div>
        {:else}
          <div class="icon"><Icon name="globe" size={28} /></div>
          <h1>Make Limbo your default browser?</h1>
          <p class="sub">Links you open from other apps will open in Limbo. Windows asks you to confirm this in Settings.</p>
          <div class="actions">
            <button class="btn ghost" onclick={finish}>{browser.isDefaultBrowser ? 'Done' : 'Not now'}</button>
            {#if browser.isDefaultBrowser}
              <span class="added"><Icon name="check" size={15} /> Limbo is your default browser</span>
            {:else}
              <button class="btn primary" onclick={makeDefault}>Open Windows Settings</button>
            {/if}
          </div>
        {/if}
      </div>
    {/key}
    <div class="dots" aria-hidden="true">
      {#each STEPS as s, i (s)}<span class:on={i === index}></span>{/each}
    </div>
  </div>
</div>

{#if review}
  <InstallReview {review} onclose={() => (review = null)} />
{/if}

<style>
  .screen {
    position: fixed;
    inset: var(--bar-height) 0 0 0;
    z-index: 75;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--bg-subtle);
  }
  .card {
    display: flex;
    flex-direction: column;
    width: min(520px, 100%);
    max-height: calc(100vh - var(--bar-height) - 48px);
    overflow: auto;
    padding: 36px 36px 20px;
    border-radius: 18px;
  }
  .content {
    display: flex;
    flex-direction: column;
  }
  .mark {
    align-self: center;
    margin-bottom: 18px;
    border-radius: 16px;
  }
  .icon {
    display: grid;
    place-items: center;
    align-self: center;
    width: 56px;
    height: 56px;
    margin-bottom: 18px;
    border-radius: 16px;
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  h1 {
    margin: 0 0 8px;
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
    text-align: center;
  }
  .content > :global(h2) {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    text-align: center;
  }
  .sub,
  .content > :global(.sub) {
    margin: 0 0 22px;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: var(--text-md-lh);
    text-align: center;
  }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    border-radius: var(--radius-md);
    box-shadow: 0 0 0 1px var(--divider);
    font-size: var(--text-md);
  }
  .actions,
  .content > :global(.actions) {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 24px;
  }
  .big {
    height: 36px;
    padding: 0 20px;
    font-size: var(--text-md);
  }
  .added {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--success);
    font-size: var(--text-md);
  }
  .dots {
    display: flex;
    justify-content: center;
    gap: 6px;
    margin-top: 24px;
  }
  .dots span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--bg-active);
    transition: width var(--dur-smooth) var(--ease-smooth), background-color var(--dur-smooth) var(--ease-smooth);
  }
  .dots span.on {
    width: 18px;
    border-radius: 3px;
    background: var(--accent);
  }
</style>
