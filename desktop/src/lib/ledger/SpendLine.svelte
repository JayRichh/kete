<script lang="ts">
  // Today's spend for one session, from the ledger the daemon pushes.
  import { app } from '../store.svelte';
  import { everyMidnight, localDay, spendLine } from './spend';

  let { agent }: { agent: string } = $props();
  // Ticks at local midnight, so yesterday's row stops showing as today's.
  let today = $state(localDay());
  $effect(() => everyMidnight((day) => (today = day)));
  const line = $derived(spendLine(app.spend[agent], today));
</script>

{#if line}
  <span class="text-[12px] text-muted" title="Today's usage from the ledger. Cost is Claude Code's list-price estimate, not a bill.">{line}</span>
{/if}
