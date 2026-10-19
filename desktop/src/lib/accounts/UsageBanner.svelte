<script lang="ts">
  // One line at 95% of the 5-hour window: when it resets, and that agents
  // the limit paused carry on by themselves after that.
  import type { UsageWindow } from '../api';
  import { labelFor } from './labels.svelte';

  let { window: w }: { window: UsageWindow | null | undefined } = $props();
  const banner = $derived(labelFor(w)?.banner ?? null);
</script>

{#if banner}
  <div class="flex-none border-b border-rule bg-surface px-4 py-1 text-[12px] text-fail" role="status" data-usage-banner>{banner}</div>
{/if}
