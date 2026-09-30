<script lang="ts" module>
  export type Section = 'general' | 'appearance' | 'memory' | 'privacy' | 'passwords' | 'extensions' | 'downloads' | 'import' | 'shortcuts' | 'about';
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import Toggle from '../components/Toggle.svelte';
  import Segmented from '../components/Segmented.svelte';
  import MemoryPanel from '../components/MemoryPanel.svelte';
  import { api } from '../ipc';
  import { browser } from '../stores/browser.svelte';
  import { ui } from '../stores/ui.svelte';
  import { actions } from '../menus';
  import { LABELS } from '../shortcuts';
  import { permissionLabel } from '../permissions';
  import type { IconName } from '../design/icons';
  import mark from '../../assets/limbo-mark.png';
  import type { LoginSummary, Policy, Preset, Settings, SitePermission } from '../types';

  const NAV: { id: Section; label: string; icon: IconName }[] = [
    { id: 'general', label: 'General', icon: 'home' },
    { id: 'appearance', label: 'Appearance', icon: 'sun' },
    { id: 'memory', label: 'Memory saver', icon: 'zap' },
    { id: 'privacy', label: 'Privacy & security', icon: 'shield' },
    { id: 'passwords', label: 'Passwords', icon: 'key' },
    { id: 'extensions', label: 'Extensions', icon: 'puzzle' },
    { id: 'downloads', label: 'Downloads', icon: 'download' },
    { id: 'import', label: 'Import', icon: 'upload' },
    { id: 'shortcuts', label: 'Shortcuts', icon: 'command' },
    { id: 'about', label: 'About Limbo', icon: 'info' },
  ];

  const fromHash = (h: string): Section | null => {
    const id = h.replace(/^#/, '') as Section;
    return NAV.some((n) => n.id === id) ? id : null;
  };

  let { initial }: { initial?: Section } = $props();
  let section = $state<Section>(
    untrack(() => initial ?? fromHash(new URL(browser.active?.url ?? 'limbo://settings').hash) ?? fromHash(location.hash) ?? 'general'),
  );

  let main = $state<HTMLElement>();
  let jumping = false;

  function go(id: Section, smooth = true) {
    section = id;
    const el = main?.querySelector<HTMLElement>(`#${id}`);
    if (!el || !main) return;
    jumping = true;
    main.scrollTo({ top: el.offsetTop - 12, behavior: smooth ? 'smooth' : 'instant' });
    setTimeout(() => (jumping = false), smooth ? 600 : 0);
  }

  // One scrolling page: jump to the requested section, highlight the one in view.
  $effect(() => {
    if (!main || !s) return;
    untrack(() => go(section, false));
    const onScroll = () => {
      if (jumping || !main) return;
      const top = main.scrollTop + 80;
      let current: Section = 'general';
      for (const el of main.querySelectorAll<HTMLElement>('section[id]')) if (el.offsetTop <= top) current = el.id as Section;
      if (main.scrollTop + main.clientHeight >= main.scrollHeight - 4) current = NAV[NAV.length - 1].id;
      section = current;
    };
    const onHash = () => {
      const h = fromHash(location.hash);
      if (h) go(h);
    };
    main.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('hashchange', onHash);
    return () => {
      main?.removeEventListener('scroll', onScroll);
      window.removeEventListener('hashchange', onHash);
    };
  });

  const s = $derived(browser.settings);
  const set = (patch: Partial<Settings>) => browser.updateSettings(patch);

  // --- memory ---
  const TIMERS = [
    { ms: 10_000, label: '10 seconds' },
    { ms: 30_000, label: '30 seconds' },
    { ms: 60_000, label: '1 minute' },
    { ms: 300_000, label: '5 minutes' },
    { ms: 900_000, label: '15 minutes' },
    { ms: 1_800_000, label: '30 minutes' },
    { ms: 3_600_000, label: '1 hour' },
    { ms: 14_400_000, label: '4 hours' },
  ];
  function setPolicy(patch: Partial<Policy>) {
    if (!s) return;
    void set({ memory: { ...s.memory, ...patch, preset: 'custom' } });
  }
  function timerValue(ms: number | null) {
    return ms === null ? 'never' : String(ms);
  }
  function timerParse(v: string) {
    return v === 'never' ? null : Number(v);
  }

  // --- privacy ---
  let perms = $state<SitePermission[]>([]);
  $effect(() => {
    api.sites.permissions().then((p) => (perms = p));
  });

  // --- passwords ---
  let logins = $state<LoginSummary[]>([]);
  let loginFilter = $state('');
  let revealed = $state<Record<number, string>>({});
  // Icons come from the local favicon store; asking a web service would leak
  // which sites have saved passwords.
  let icons = $state<Record<string, string | null>>({});
  $effect(() => {
    api.passwords.list().then((l) => {
      logins = l;
      for (const o of new Set(l.map((x) => x.origin))) api.favicon(o + '/').then((f) => (icons[o] = f));
    });
  });
  const shownLogins = $derived(
    logins.filter((l) => !loginFilter || (l.origin + ' ' + l.username).toLowerCase().includes(loginFilter.toLowerCase())),
  );
  async function reveal(id: number) {
    if (revealed[id] !== undefined) {
      delete revealed[id];
      return;
    }
    try {
      revealed[id] = await api.passwords.reveal(id);
      // Plaintext never lingers on screen.
      setTimeout(() => delete revealed[id], 30_000);
    } catch (e) {
      browser.toast(String(e));
    }
  }
  async function copyPassword(id: number) {
    try {
      const pw = revealed[id] ?? (await api.passwords.reveal(id));
      await navigator.clipboard.writeText(pw);
      browser.toast('Password copied');
    } catch (e) {
      browser.toast(String(e));
    }
  }
  async function removeLogin(l: LoginSummary) {
    await api.passwords.remove(l.id);
    logins = logins.filter((x) => x.id !== l.id);
    browser.toast(`Deleted password for ${l.username || new URL(l.origin).host}`);
  }
  async function importCsv() {
    const r = await api.passwords.importCsv().catch((e) => (browser.toast(String(e)), null));
    if (r) {
      browser.toast(`Imported ${r.imported} passwords${r.failed ? `, ${r.failed} failed` : ''}`);
      logins = await api.passwords.list();
    }
  }
  async function exportCsv() {
    const path = await api.passwords.exportCsv().catch((e) => (browser.toast(String(e)), null));
    if (path) browser.toast('Passwords exported. Delete the file when you are done with it.');
  }

  // --- extensions ---
  async function installFile() {
    const review = await api.ext.prepareFile().catch((e) => (browser.toast(String(e)), null));
    if (review) {
      ui.dialog = { kind: 'install', review };
      void ui.open('dialog');
    }
  }
  async function loadUnpacked() {
    const ext = await api.ext.loadUnpacked().catch((e) => (browser.toast(String(e)), null));
    if (ext) browser.toast(`Loaded ${ext.name}`);
  }
  function openStore() {
    void api.tabs.create('https://chromewebstore.google.com/');
  }

  // --- advanced ---
  let argsText = $state('');
  $effect(() => {
    if (s) argsText = s.experimentalArgs.join(' ');
  });
  let needsRestart = $state(false);

  const SHORTCUTS: [string, string][] = [
    ['New tab', LABELS.newTab!],
    ['New private tab', LABELS.newPrivateTab!],
    ['Close tab', LABELS.closeTab!],
    ['Reopen closed tab', LABELS.reopenClosedTab!],
    ['Next / previous tab', 'Ctrl+Tab / Ctrl+Shift+Tab'],
    ['Go to tab 1–8 / last tab', 'Ctrl+1…8 / Ctrl+9'],
    ['Address bar', `${LABELS.focusOmnibox} / Alt+D / F6`],
    ['Open as new tab from address bar', 'Alt+Enter'],
    ['Add www. and .com', 'Ctrl+Enter'],
    ['Command palette', LABELS.commandPalette!],
    ['Find in page', `${LABELS.find} / F3`],
    ['Bookmark this page', LABELS.bookmark!],
    ['History', LABELS.history!],
    ['Downloads', LABELS.downloads!],
    ['Sidebar', LABELS.toggleSidebar!],
    ['Bookmarks bar', LABELS.toggleBookmarksBar!],
    ['Tab overview', LABELS.tabOverview!],
    ['Memory saver', LABELS.memoryPopover!],
    ['Clear browsing data', LABELS.clearBrowsingData!],
    ['Zoom in / out / reset', 'Ctrl+= / Ctrl+− / Ctrl+0'],
    ['Reload / hard reload', 'Ctrl+R / Ctrl+Shift+R'],
    ['Back / forward', 'Alt+← / Alt+→'],
    ['Full screen', LABELS.fullscreen!],
  ];

  const PRESETS: { value: Preset; label: string }[] = [
    { value: 'balanced', label: 'Balanced' },
    { value: 'aggressive', label: 'Maximum savings' },
    { value: 'off', label: 'Off' },
  ];
  const PRESET_TEXT: Record<Preset, string> = {
    balanced: 'Background tabs slow down after 30 seconds, sleep after 5 minutes and unload after 30 minutes. At most 4 tabs stay awake.',
    aggressive: 'Background tabs slow down after 10 seconds, sleep after 1 minute and unload after 5 minutes. At most 2 tabs stay awake.',
    off: 'Background tabs are never put to sleep. Uses the most memory.',
    custom: 'Your own timers.',
  };
</script>

<div class="settings">
  <nav aria-label="Settings sections">
    <h1>Settings</h1>
    {#each NAV as n (n.id)}
      <button class:on={section === n.id} aria-current={section === n.id ? 'page' : undefined} onclick={() => go(n.id)}>
        <Icon name={n.icon} size={15} />
        {n.label}
      </button>
    {/each}
  </nav>

  <main bind:this={main}>
    {#if s}
      <section id="general">
        <h2>General</h2>
        {#if !browser.isDefaultBrowser && !browser.platform?.portable}
          <div class="card callout">
            <div>
              <strong>Make Limbo your default browser</strong>
              <p>Links from other apps will open here.</p>
            </div>
            <button class="btn primary" onclick={() => api.defaultBrowser.open()}>Open Windows Settings</button>
          </div>
        {/if}
        <div class="card">
          <div class="row">
            <div><strong>Continue where you left off</strong><p>Reopen your tabs when Limbo starts. They load when you click them.</p></div>
            <Toggle label="Continue where you left off" checked={s.restoreTabsOnStartup} onchange={(v) => set({ restoreTabsOnStartup: v })} />
          </div>
          <div class="row">
            <div><strong>Google suggestions</strong><p>Send what you type in the address bar to Google for suggestions. Your history and bookmarks are always matched on this device.</p></div>
            <Toggle label="Google suggestions" checked={s.showGoogleSuggestions} onchange={(v) => set({ showGoogleSuggestions: v })} />
          </div>
          <div class="row">
            <div><strong>Show bookmarks bar</strong><p>{LABELS.toggleBookmarksBar}</p></div>
            <Toggle label="Show bookmarks bar" checked={s.bookmarksBarVisible} onchange={(v) => set({ bookmarksBarVisible: v })} />
          </div>
          <div class="row">
            <div><strong>Search engine</strong><p>Limbo uses Google for search, suggestions and the new tab page.</p></div>
            <span class="value">Google</span>
          </div>
        </div>
      </section>
      <section id="appearance">
        <h2>Appearance</h2>
        <div class="card">
          <div class="row">
            <div><strong>Theme</strong></div>
            <Segmented
              label="Theme"
              value={s.theme}
              options={[{ value: 'system', label: 'System' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]}
              onchange={(v) => set({ theme: v })}
            />
          </div>
          {#if browser.platform?.windows11}
            <div class="row">
              <div><strong>Translucent title bar</strong><p>Uses Windows 11 Mica behind the tabs.</p></div>
              <Toggle label="Translucent title bar" checked={s.mica} onchange={(v) => set({ mica: v })} />
            </div>
          {/if}
          <div class="row">
            <div><strong>Page zoom</strong><p>Default zoom for sites you haven't zoomed yourself.</p></div>
            <select class="input" value={String(s.defaultZoom)} onchange={(e) => set({ defaultZoom: Number(e.currentTarget.value) })}>
              {#each [0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5] as z (z)}
                <option value={String(z)}>{Math.round(z * 100)}%</option>
              {/each}
            </select>
          </div>
        </div>
      </section>
      <section id="memory">
        <h2>Memory saver</h2>
        <p class="lead">Limbo puts tabs you aren't using to sleep so the one you're looking at stays fast. Sleeping tabs keep their place and wake up when you click them. When more tabs are awake than the maximum, the least recently used one goes to sleep; pinned tabs and tabs playing audio or using your camera stay awake.</p>
        <div class="card live"><MemoryPanel /></div>
        <div class="card">
          <div class="row">
            <div><strong>Mode</strong><p>{PRESET_TEXT[s.memory.preset]}</p></div>
            <Segmented label="Memory saver mode" value={s.memory.preset} options={PRESETS} onchange={(v) => api.memory.setPreset(v).then((x) => (browser.settings = x))} />
          </div>
          {#each [{ key: 'hiddenToLowMs', label: 'Slow down background tabs after' }, { key: 'lowToSuspendMs', label: 'Put tabs to sleep after' }, { key: 'suspendToDiscardMs', label: 'Unload sleeping tabs after' }] as const as t (t.key)}
            <div class="row">
              <div><strong>{t.label}</strong></div>
              <select class="input" value={timerValue(s.memory[t.key])} onchange={(e) => setPolicy({ [t.key]: timerParse(e.currentTarget.value) })}>
                {#each TIMERS as o (o.ms)}<option value={String(o.ms)}>{o.label}</option>{/each}
                <option value="never">Never</option>
              </select>
            </div>
          {/each}
          <div class="row">
            <div><strong>Unload pinned tabs too</strong><p>Pinned tabs sleep but normally stay loaded.</p></div>
            <Toggle label="Unload pinned tabs too" checked={s.memory.discardPinned} onchange={(v) => setPolicy({ discardPinned: v })} />
          </div>
          <div class="row">
            <div><strong>Show memory in the toolbar</strong><p>A small live readout next to the menu button. Click it for this panel.</p></div>
            <Toggle label="Show memory in the toolbar" checked={s.showMemoryInToolbar} onchange={(v) => set({ showMemoryInToolbar: v })} />
          </div>
        </div>
      </section>
      <section id="privacy">
        <h2>Privacy & security</h2>
        <div class="card">
          <div class="row">
            <div><strong>Clear browsing data</strong><p>History, cookies, cached files and more.</p></div>
            <button class="btn" onclick={actions.clearData}>Clear…</button>
          </div>
          <div class="row">
            <div><strong>Offer to save passwords</strong></div>
            <Toggle label="Offer to save passwords" checked={s.offerToSavePasswords} onchange={(v) => set({ offerToSavePasswords: v })} />
          </div>
          <div class="row">
            <div>
              <strong>Microsoft Defender SmartScreen</strong>
              <p>Checks pages and downloads against Microsoft's list of known phishing and malware sites. Off by default in Limbo to save memory. Changes apply after a restart.</p>
            </div>
            <Toggle
              label="Microsoft Defender SmartScreen"
              checked={s.smartscreenEnabled}
              onchange={(v) => {
                void set({ smartscreenEnabled: v });
                needsRestart = true;
              }}
            />
          </div>
          <div class="row">
            <div><strong>No telemetry</strong><p>Limbo doesn't send usage data anywhere. Google receives what you search, and suggestion queries if they're on.</p></div>
            <Icon name="check" size={16} />
          </div>
        </div>
        <h3>Site permissions</h3>
        <div class="card">
          {#each perms as p (p.origin + p.kind)}
            <div class="row">
              <div><strong>{p.origin}</strong><p>{permissionLabel(p.kind)}: {p.allow ? 'Allowed' : 'Blocked'}</p></div>
              <button class="btn ghost" onclick={() => api.sites.revoke(p.origin, p.kind).then(() => (perms = perms.filter((x) => x !== p)))}>Reset</button>
            </div>
          {:else}
            <p class="empty">Sites you allow or block (camera, location, notifications…) appear here.</p>
          {/each}
        </div>
      </section>
      <section id="passwords">
        <h2>Passwords</h2>
        <p class="lead">Encrypted on this PC with your Windows account. Revealing or exporting asks for Windows Hello or your PIN.</p>
        <div class="toolbar">
          <input class="input grow" placeholder="Search passwords" bind:value={loginFilter} aria-label="Search passwords" />
          <button class="btn" onclick={importCsv}><Icon name="upload" size={14} /> Import CSV</button>
          <button class="btn" onclick={exportCsv}><Icon name="download" size={14} /> Export</button>
        </div>
        <div class="card">
          {#each shownLogins as l (l.id)}
            <div class="row login">
              {#if icons[l.origin]}<img src={icons[l.origin]} alt="" width="16" height="16" />{:else}<Icon name="globe" size={16} />{/if}
              <div class="grow">
                <strong>{l.origin.replace(/^https?:\/\//, '')}</strong>
                <p>{l.username || '(no username)'}</p>
              </div>
              <code class="pw" class:shown={revealed[l.id] !== undefined}>{revealed[l.id] ?? '••••••••••'}</code>
              <button class="icon-btn" aria-label={revealed[l.id] !== undefined ? 'Hide password' : 'Show password'} onclick={() => reveal(l.id)}>
                <Icon name={revealed[l.id] !== undefined ? 'eyeOff' : 'eye'} size={15} />
              </button>
              <button class="icon-btn" aria-label="Copy password" onclick={() => copyPassword(l.id)}><Icon name="copy" size={15} /></button>
              <button class="icon-btn" aria-label="Delete password" onclick={() => removeLogin(l)}><Icon name="trash" size={15} /></button>
            </div>
          {:else}
            <p class="empty">{loginFilter ? 'No matches' : 'No saved passwords yet. Import them from Firefox or a CSV file.'}</p>
          {/each}
        </div>
      </section>
      <section id="extensions">
        <h2>Extensions</h2>
        <p class="lead">Limbo runs Chrome extensions (Manifest V3). Open an extension's page in the Chrome Web Store or Edge Add-ons and use “Add to Limbo” in the address bar.</p>
        <div class="toolbar">
          <button class="btn primary" onclick={openStore}><Icon name="externalLink" size={14} /> Chrome Web Store</button>
          <button class="btn" onclick={installFile}><Icon name="file" size={14} /> Install from file…</button>
          {#if s.developerMode}<button class="btn" onclick={loadUnpacked}><Icon name="folder" size={14} /> Load unpacked…</button>{/if}
        </div>
        <div class="card">
          {#each browser.extensions as x (x.id)}
            <div class="row ext">
              {#if x.icon}<img src={x.icon} alt="" width="28" height="28" />{:else}<span class="ph"><Icon name="puzzle" size={16} /></span>{/if}
              <div class="grow">
                <strong>{x.name} <span class="ver">{x.version}</span></strong>
                <p>{x.description}</p>
                <div class="links">
                  {#if x.optionsPage}<button class="link" onclick={() => api.ext.openOptions(x.id, x.optionsPage!)}>Options</button>{/if}
                  {#if x.hasAction}<button class="link" onclick={() => api.ext.setPinned(x.id, !x.pinned)}>{x.pinned ? 'Unpin from toolbar' : 'Pin to toolbar'}</button>{/if}
                  <button class="link danger" onclick={() => api.ext.remove(x.id).then(() => browser.toast(`Removed ${x.name}`))}>Remove</button>
                  {#if s.developerMode}<span class="id">{x.id}</span>{/if}
                </div>
              </div>
              <Toggle label={`Enable ${x.name}`} checked={x.enabled} onchange={(v) => api.ext.setEnabled(x.id, v)} />
            </div>
          {:else}
            <p class="empty">No extensions yet. uBlock Origin Lite is a good first one.</p>
          {/each}
        </div>
        <div class="card">
          <div class="row">
            <div><strong>Developer mode</strong><p>Load unpacked extensions and show developer tools (F12).</p></div>
            <Toggle label="Developer mode" checked={s.developerMode} onchange={(v) => set({ developerMode: v })} />
          </div>
        </div>
      </section>
      <section id="downloads">
        <h2>Downloads</h2>
        <div class="card">
          <div class="row">
            <div><strong>Location</strong><p class="path">{s.downloadsFolder ?? 'Downloads'}</p></div>
            <button class="btn" onclick={() => api.downloads.chooseFolder().then((x) => x && (browser.settings = x))}>Change…</button>
          </div>
          <div class="row">
            <div><strong>Ask where to save each file</strong></div>
            <Toggle label="Ask where to save each file" checked={s.askWhereToSave} onchange={(v) => set({ askWhereToSave: v })} />
          </div>
        </div>
      </section>
      <section id="import">
        <h2>Import</h2>
        <div class="card">
          <div class="row">
            <div><strong>Firefox</strong><p>Passwords, history, bookmarks, cookies, form entries, open tabs and a list of your add-ons.</p></div>
            <button class="btn primary" onclick={actions.importFirefox}>Import…</button>
          </div>
          <div class="row">
            <div><strong>Bookmarks file</strong><p>An HTML bookmarks file from any browser.</p></div>
            <button class="btn" onclick={() => api.bookmarks.importHtml().then((n) => n !== null && browser.toast(`Imported ${n} bookmarks`))}>Import…</button>
          </div>
          <div class="row">
            <div><strong>Export bookmarks</strong></div>
            <button class="btn" onclick={() => api.bookmarks.exportHtml().then((p) => p && browser.toast('Bookmarks exported'))}>Export…</button>
          </div>
        </div>
      </section>
      <section id="shortcuts">
        <h2>Keyboard shortcuts</h2>
        <div class="card">
          {#each SHORTCUTS as [label, keys] (label)}
            <div class="row compact"><span>{label}</span><span class="keys">{#each keys.split(' / ') as k, i (i)}{#if i}<span class="or">or</span>{/if}<span class="kbd">{k}</span>{/each}</span></div>
          {/each}
        </div>
      </section>
      <section id="about">
        <h2>About Limbo</h2>
        <div class="card about">
          <img src={mark} alt="" width="56" height="56" />
          <div>
            <strong>Limbo {browser.platform?.version}</strong>
            <p>Engine: Microsoft Edge WebView2 {browser.platform?.engineVersion ?? ''}. Updates come with Windows.</p>
            {#if browser.platform?.portable}<p>Portable edition: your data stays in the package's Data folder.</p>{/if}
          </div>
        </div>
        <h3>Advanced</h3>
        <div class="card">
          <div class="row">
            <div><strong>Experimental engine flags</strong><p>Extra Chromium command-line switches. Can break pages; applies after a restart.</p></div>
            <Toggle
              label="Experimental engine flags"
              checked={s.experimentalArgsEnabled}
              onchange={(v) => {
                void set({ experimentalArgsEnabled: v });
                needsRestart = true;
              }}
            />
          </div>
          {#if s.experimentalArgsEnabled}
            <div class="row">
              <input
                class="input grow mono"
                bind:value={argsText}
                placeholder="--enable-features=…"
                aria-label="Engine flags"
                onchange={() => {
                  void set({ experimentalArgs: argsText.split(/\s+/).filter((a) => a.startsWith('--')) });
                  needsRestart = true;
                }}
              />
            </div>
          {/if}
        </div>
      </section>

      {#if needsRestart}
        <div class="restart card">
          <span>Restart Limbo to apply changes. Your tabs will be restored.</span>
          <button class="btn primary" onclick={() => api.restart()}>Restart now</button>
        </div>
      {/if}
    {/if}
  </main>
</div>

<style>
  .settings {
    display: flex;
    height: 100%;
    background: var(--bg);
    overflow: hidden;
  }
  nav {
    width: 232px;
    flex: none;
    padding: 28px 12px;
    overflow-y: auto;
  }
  nav h1 {
    margin: 0 10px 16px;
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
  }
  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-md);
    text-align: left;
  }
  nav button:hover {
    background: var(--bg-hover);
  }
  nav button.on {
    background: var(--bg-active);
    color: var(--text);
  }
  main {
    flex: 1;
    min-width: 0;
    padding: 28px 40px 64px;
    overflow-y: auto;
  }
  main > :global(*) {
    max-width: 680px;
  }
  h2 {
    margin: 36px 0 16px;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
  }
  h3 {
    margin: 28px 0 10px;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }
  .lead {
    margin: -6px 0 16px;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: var(--text-md-lh);
  }
  .card {
    margin-bottom: 16px;
    border-radius: var(--radius-md);
    box-shadow: 0 0 0 1px var(--divider);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    min-height: 56px;
    padding: 12px 16px;
  }
  .row + .row {
    border-top: 1px solid var(--divider);
  }
  .row.compact {
    min-height: 40px;
    padding: 6px 16px;
    font-size: var(--text-md);
  }
  .row strong {
    display: block;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }
  .row p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
    line-height: var(--text-sm-lh);
  }
  .value {
    color: var(--text-muted);
    font-size: var(--text-md);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .callout {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 16px;
    background: var(--accent-soft);
    box-shadow: none;
  }
  .callout p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  select.input {
    padding-right: 8px;
    color: var(--text);
  }
  .toolbar {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }
  .login {
    justify-content: flex-start;
    gap: 12px;
  }
  .login img {
    border-radius: 3px;
  }
  .pw {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-muted);
    letter-spacing: 0.1em;
  }
  .pw.shown {
    color: var(--text);
    letter-spacing: 0;
    user-select: text;
  }
  .ext {
    justify-content: flex-start;
    align-items: flex-start;
    gap: 14px;
  }
  .ext img,
  .ph {
    flex: none;
    border-radius: 6px;
  }
  .ph {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    background: var(--bg-hover);
    color: var(--text-muted);
  }
  .ver {
    margin-left: 4px;
    color: var(--text-muted);
    font-weight: var(--weight-regular);
    font-size: var(--text-sm);
  }
  .links {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 6px;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font-size: var(--text-sm);
  }
  .link:hover {
    text-decoration: underline;
  }
  .link.danger {
    color: var(--danger);
  }
  .id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }
  .path {
    font-family: var(--font-mono);
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .or {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .about {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
  }
  .about img {
    border-radius: 12px;
  }
  .about p {
    margin: 4px 0 0;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .empty {
    margin: 0;
    padding: 16px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  .live {
    padding: 16px;
  }
  .restart {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
    background: var(--bg-float);
    box-shadow: var(--shadow-float);
    font-size: var(--text-md);
  }
</style>
