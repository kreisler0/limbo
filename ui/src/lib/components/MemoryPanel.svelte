<script lang="ts" module>
  // Several panels may be open at once (popover + Settings): sample fast while any is.
  import { api as host } from '../ipc';
  let users = 0;
  function acquire() {
    if (users++ === 0) void host.memory.setSampling('popover');
  }
  function release() {
    if (--users === 0) void host.memory.setSampling('pill');
  }
</script>

<script lang="ts">
  // The memory readout: total, share of system RAM, per-tab rows with
  // Sleep/Unload, and the maximum-awake-tabs stepper. Shown in the memory
  // popover and embedded in Settings → Memory saver.
  import { untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import Stepper from './Stepper.svelte';
  import { api } from '../ipc';
  import { browser, formatMB } from '../stores/browser.svelte';
  import type { MemoryRow } from '../types';

  let history = $state<number[]>([]);

  // Sample every 2 s while shown (the pill alone samples every 10 s).
  $effect(() => {
    acquire();
    return release;
  });

  $effect(() => {
    const m = browser.memory;
    if (!m) return;
    history = [...untrack(() => history).slice(-29), m.totalBytes];
  });

  const m = $derived(browser.memory);
  const total = $derived(m?.systemTotalBytes ?? 1);
  const appShare = $derived(m ? m.totalBytes / total : 0);
  const freeShare = $derived(m ? m.systemAvailBytes / total : 0);
  const otherShare = $derived(Math.max(0, 1 - appShare - freeShare));

  const spark = $derived.by(() => {
    // Newest sample on the right; the line grows leftwards as samples arrive.
    if (history.length < 4) return '';
    const max = Math.max(...history) * 1.1;
    const min = Math.min(...history) * 0.9;
    const span = Math.max(1, max - min);
    const n = history.length;
    return history
      .map((v, i) => `${i === 0 ? 'M' : 'L'}${(120 - ((n - 1 - i) / 29) * 120).toFixed(1)},${(24 - ((v - min) / span) * 22 - 1).toFixed(1)}`)
      .join(' ');
  });

  function label(r: MemoryRow): { title: string; icon: string | null } {
    if (r.kind === 'engine') return { title: 'Browser engine', icon: null };
    if (r.kind === 'extension') {
      const e = browser.extensions.find((x) => x.id === r.id);
      return { title: e?.name ?? 'Extension', icon: e?.icon ?? null };
    }
    const t = browser.tab(Number(r.id));
    return { title: t?.title || t?.displayHost || 'Tab', icon: t?.favicon ?? null };
  }

  const stateLabel: Record<string, string> = { active: 'Active', awake: 'Awake', sleeping: 'Sleeping', unloaded: 'Unloaded' };
  const maxAwake = $derived(browser.settings?.memory.maxAwake ?? null);
  const overLimit = $derived(!!m && m.maxAwake !== null && m.awake > m.maxAwake);
</script>

<div class="panel">
  <header>
    <div>
      <div class="big tabular">{m ? formatMB(m.totalBytes) : '—'}</div>
      <div class="muted">used by Limbo</div>
    </div>
    <svg class="spark" width="120" height="24" viewBox="0 0 120 24" aria-hidden="true">
      <path d={spark} />
    </svg>
  </header>
  <div class="bar" aria-hidden="true">
    <span class="app" style:flex={appShare}></span>
    <span class="other" style:flex={otherShare}></span>
    <span class="free" style:flex={freeShare}></span>
  </div>
  <div class="legend muted">
    <span><i class="app"></i>Limbo</span>
    <span><i class="other"></i>Other apps</span>
    <span><i class="free"></i>{m ? formatMB(m.systemAvailBytes) : ''} free</span>
  </div>

  <ul class="rows">
    {#each m?.rows ?? [] as r (r.kind + r.id)}
      {@const l = label(r)}
      <li>
        <span class="ic">
          {#if l.icon}<img src={l.icon} alt="" width="16" height="16" />{:else if r.kind === 'engine'}<Icon name="memory" size={15} />{:else}<Icon name="globe" size={15} />{/if}
        </span>
        <span class="name fade-out">{l.title}</span>
        {#if r.state}
          <span class="dot {r.state}" title={stateLabel[r.state]}></span>
        {/if}
        {#if r.kind === 'tab' && r.state !== 'active'}
          <span class="actions">
            {#if r.state === 'awake'}
              <button class="btn ghost small" onclick={() => api.memory.sleepTab(Number(r.id))}>Sleep</button>
            {/if}
            {#if r.state !== 'unloaded'}
              <button class="btn ghost small" onclick={() => api.memory.unloadTab(Number(r.id))}>Unload</button>
            {/if}
          </span>
        {/if}
        <span class="mb tabular" title={r.shared ? 'Shared with other tabs of the same site' : undefined}>
          {r.bytes > 0 ? formatMB(r.bytes) : '—'}{r.shared ? '*' : ''}
        </span>
      </li>
    {/each}
  </ul>

  <footer>
    <div class="limit">
      <div>
        <div class="strong">Maximum awake tabs</div>
        <div class="muted small-text">{m ? `${m.awake} of ${maxAwake ?? m.awake} tabs awake` : ''}</div>
      </div>
      <Stepper value={maxAwake} min={1} max={20} onchange={(v) => api.memory.setMaxAwake(v).then((s) => (browser.settings = s))} />
    </div>
    {#if overLimit}
      <p class="hint">Some tabs stay awake because they're pinned, playing audio, recording, or downloading.</p>
    {/if}
    <button class="btn wide" onclick={() => api.memory.sleepAll()}>
      <Icon name="moon" size={14} /> Sleep all background tabs
    </button>
  </footer>
</div>

<style>
  header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .big {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    line-height: var(--text-xl-lh);
    font-weight: var(--weight-semibold);
  }
  .muted {
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .spark path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linejoin: round;
  }
  .bar {
    display: flex;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: var(--bg-hover);
  }
  .bar span {
    transition: flex 400ms var(--ease-smooth);
  }
  .app {
    background: var(--accent);
  }
  .other {
    background: color-mix(in srgb, var(--text) 25%, transparent);
  }
  .free {
    background: transparent;
  }
  .legend {
    display: flex;
    gap: 12px;
    margin: 6px 0 10px;
  }
  .legend i {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 5px;
    border-radius: 2px;
    vertical-align: 0;
  }
  .legend i.free {
    box-shadow: inset 0 0 0 1px var(--divider);
  }
  .rows {
    margin: 0 -6px;
    padding: 0;
    list-style: none;
    max-height: 280px;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 6px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }
  li:hover {
    background: var(--bg-hover);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--text-muted);
    flex: none;
  }
  .ic img {
    border-radius: 3px;
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex: none;
    transition: background-color 400ms ease;
  }
  .dot.active {
    background: var(--success);
  }
  .dot.awake {
    background: var(--accent);
  }
  .dot.sleeping {
    background: var(--warning);
  }
  .dot.unloaded {
    background: var(--text-faint);
  }
  .actions {
    display: none;
    gap: 2px;
  }
  li:hover .actions {
    display: flex;
  }
  li:hover .mb {
    display: none;
  }
  .small {
    height: 22px;
    padding: 0 7px;
    font-size: var(--text-xs);
  }
  .mb {
    width: 56px;
    text-align: right;
    color: var(--text-muted);
    flex: none;
  }
  footer {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--divider);
  }
  .limit {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  .strong {
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
  }
  .small-text {
    margin-top: 1px;
  }
  .hint {
    margin: -2px 0 10px;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }
  .wide {
    width: 100%;
  }
</style>
