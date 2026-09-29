<script lang="ts">
  import { icons, type IconName } from '../design/icons';

  let { name, size = 16, stroke = 1.5, class: cls = '' }: { name: IconName; size?: number; stroke?: number; class?: string } =
    $props();
</script>

<svg
  class={cls}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={(stroke * 24) / size}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html icons[name]}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
