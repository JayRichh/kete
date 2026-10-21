<script lang="ts">
  // Today's spend for one session, from the ledger the daemon pushes.
  import { app } from '../store.svelte';
  import { spendLine } from './spend';

  let { agent }: { agent: string } = $props();
  const line = $derived(spendLine(app.spend[agent]));
</script>

{#if line}
  <span class="text-[12px] text-muted" title="Today's usage from the ledger. Cost is Claude Code's list-price estimate, not a bill.">{line}</span>
{/if}
