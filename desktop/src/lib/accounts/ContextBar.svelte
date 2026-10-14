<script lang="ts">
  // A session's context window: a short bar and its percent, with a
  // compaction mark (a square tick and the count) once it was compacted.
  // Nothing at all while the session has no reading.
  import type { ContextUsage } from '../api';
  import { app } from '../store.svelte';
  import UsageBar from './UsageBar.svelte';
  import { contextTitle } from './usage';

  let { context, wide = false }: { context: ContextUsage | null | undefined; wide?: boolean } = $props();
</script>

{#if context && context.percent != null}
  <span
    class="inline-flex flex-none items-center gap-1.5"
    title={contextTitle(context, app.now)}
    aria-label="Context {Math.round(context.percent)}%{context.compactions ? `, compacted ${context.compactions} times` : ''}"
  >
    {#if wide}<span class="text-[12px] text-muted">context</span>{/if}
    <UsageBar percent={context.percent} width={wide ? 72 : 28} />
    {#if context.compactions > 0}
      <span class="inline-flex items-center gap-0.5 font-mono text-[11px] leading-none text-muted"
        ><span class="size-[5px] border border-current" aria-hidden="true"></span>{context.compactions}</span
      >
    {/if}
  </span>
{/if}
