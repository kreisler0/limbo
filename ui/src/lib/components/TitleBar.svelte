<script lang="ts">
  import ActionBar from './ActionBar.svelte';
  import NavButtons from './NavButtons.svelte';
  import Omnibox from './Omnibox.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import TabStrip from './TabStrip.svelte';
  import WindowControls from './WindowControls.svelte';
</script>

<header class="bar" data-tauri-drag-region>
  <NavButtons />
  <TabStrip />
  <Omnibox />
  <div class="spacer" data-tauri-drag-region></div>
  <ActionBar />
  <WindowControls />
  <ProgressBar />
</header>

<style>
  .bar {
    position: relative;
    display: flex;
    align-items: center;
    height: var(--bar-height);
    flex: none;
    background: var(--bar-bg, var(--bg-subtle));
    z-index: 20;
  }
  .spacer {
    flex: 0 1 24px;
    min-width: 4px;
    height: 100%;
  }
</style>
