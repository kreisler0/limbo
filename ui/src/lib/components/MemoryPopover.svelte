<script lang="ts">
  import MemoryPanel from './MemoryPanel.svelte';
  import Popover from './Popover.svelte';
  import { ui } from '../stores/ui.svelte';
</script>

<Popover label="Memory saver" width={360} onclose={() => ui.close('memory')}>
  <MemoryPanel />
</Popover>
