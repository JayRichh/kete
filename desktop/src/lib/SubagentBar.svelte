<script lang="ts">
  // Every session's subagents counted, the fold setting, and the one-click
  // cleanup: above the board's lanes and the overview's repos.
  import { app } from './store.svelte';
  import { endedTotal, isEnded } from './subagents';
  import { clearFinishedSubagents, foldingFinished, toggleFoldFinished } from './subagent-actions';

  const running = $derived(app.model.subagents.filter((s) => !isEnded(s)).length);
  const ended = $derived(endedTotal(app.model.subagents));
  const folding = $derived(foldingFinished());
</script>

{#if app.model.subagents.length}
  <div class="flex h-8 flex-none items-center gap-3 border-b border-rule px-4 text-[12px] text-muted" role="toolbar" aria-label="Subagents">
    <span class="font-mono">{running} subagent{running === 1 ? '' : 's'} running{' \u00B7 '}{ended} finished</span>
    <label
      class="ml-auto inline-flex items-center gap-1.5"
      title="Fold each session's done and closed subagents into one count under it (Settings, Sessions, Finished subagents; shared with the terminal app)"
    >
      <input type="checkbox" checked={folding} onchange={toggleFoldFinished} />Fold finished
    </label>
    <button
      class="inline-flex h-6 items-center border border-rule bg-transparent px-2 text-[12px] text-ink hover:border-rule-strong disabled:text-faint"
      title="Remove every finished subagent record (done, failed, or closed with its session); running ones stay"
      disabled={!ended}
      onclick={() => clearFinishedSubagents(null)}>Clear finished</button
    >
  </div>
{/if}
