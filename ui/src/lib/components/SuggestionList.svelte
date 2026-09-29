<script lang="ts">
  import Icon from './Icon.svelte';
  import { pop } from '../design/motion';
  import type { Suggestion } from '../types';

  let {
    rows,
    selected,
    typed,
    onpick,
    onhover,
  }: { rows: Suggestion[]; selected: number; typed: string; onpick: (row: Suggestion) => void; onhover: (i: number) => void } =
    $props();

  function host(url: string): string {
    try {
      const u = new URL(url);
      return u.host.replace(/^www\./, '') + (u.pathname !== '/' ? u.pathname : '');
    } catch {
      return url;
    }
  }

  /** Bold the part the user didn't type (Chrome-style emphasis). */
  function split(title: string): [string, string] {
    const q = typed.trim().toLowerCase();
    if (q && title.toLowerCase().startsWith(q)) return [title.slice(0, q.length), title.slice(q.length)];
    return ['', title];
  }
</script>

<ul id="suggestions" class="list float" role="listbox" in:pop={{ origin: 'top center', from: 0.98, y: -4 }}>
  {#each rows as row, i (row.url + row.kind + i)}
    {@const [head, tail] = split(row.kind === 'search' ? row.title : row.title)}
    <!-- Keyboard is handled by the text field (combobox pattern). -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <li
      role="option"
      aria-selected={i === selected}
      class:selected={i === selected}
      onpointerdown={(e) => e.preventDefault()}
      onclick={() => onpick(row)}
      onpointermove={() => i !== selected && onhover(i)}
    >
      <span class="kind">
        {#if row.kind === 'search'}
          <Icon name="search" size={14} />
        {:else if row.kind === 'bookmark'}
          <Icon name="star" size={14} />
        {:else if row.kind === 'openTab'}
          <Icon name="arrowUpRight" size={14} />
        {:else if row.kind === 'history'}
          <Icon name="history" size={14} />
        {:else}
          <Icon name="globe" size={14} />
        {/if}
      </span>
      <span class="text">
        {#if row.kind === 'search'}
          <span class="main">{head}<b>{tail}</b></span>
        {:else if row.kind === 'navigate'}
          <span class="main">{row.completion ? typed + row.completion : host(row.url)}</span>
        {:else}
          <span class="main">{row.title}</span>
          <span class="sub">— {host(row.url)}</span>
        {/if}
      </span>
      {#if row.kind === 'openTab'}
        <span class="hint">Switch to tab</span>
      {:else if i === selected && row.kind === 'search'}
        <span class="hint">Google Search</span>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .list {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    margin: 0;
    padding: 6px;
    list-style: none;
    z-index: 40;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--text-md);
  }
  li.selected {
    background: var(--bg-hover);
  }
  .kind {
    display: grid;
    place-items: center;
    width: 18px;
    color: var(--text-muted);
    flex: none;
  }
  .text {
    display: flex;
    gap: 6px;
    min-width: 0;
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
  }
  .main {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .main b {
    font-weight: var(--weight-semibold);
  }
  .sub {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }
  .hint {
    color: var(--text-muted);
    font-size: var(--text-xs);
    flex: none;
  }
</style>
