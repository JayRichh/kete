<script lang="ts">
  // The orchestrator has no card on the board, so its subagents (the
  // teammates it runs) are listed here, above the lanes: label, model and
  // effort, state. Finished ones follow the same fold setting as a card's.
  import { app } from './store.svelte';
  import Status from './Status.svelte';
  import { subagentTone, age } from './status';
  import { offBoardSubagents, isEnded, movedAt, subagentRun, subagentWord } from './subagents';
  import { foldingFinished } from './subagent-actions';

  const all = $derived(offBoardSubagents(app.model));
  const folding = $derived(foldingFinished());
  const shown = $derived(folding ? all.filter((s) => !isEnded(s)) : all);
  const ended = $derived(all.filter(isEnded).length);
</script>

{#if all.length}
  <ul class="m-0 flex max-h-40 flex-none list-none flex-col gap-1 overflow-y-auto border-0 border-b border-rule px-4 py-2" aria-label="Orchestrator teammates" data-teammates>
    {#each shown as s (s.parent_agent + s.id)}
      <li class="flex min-w-0 items-center gap-2 text-[12px]" title="{[s.kind || 'subagent', subagentRun(s)].filter(Boolean).join(' \u00b7 ')}: {s.description || 'no task seen'}">
        <Status tone={subagentTone(s)} word={false} />
        <span class="min-w-0 truncate {isEnded(s) ? 'text-faint' : 'text-ink'}">{s.description || s.kind || 'subagent'}</span>
        <span class="flex-none font-mono text-[11px] text-faint">{subagentRun(s)}</span>
        <span class="ml-auto flex-none font-mono text-[11px] text-muted">{subagentWord(s)} {age(movedAt(s), app.now)}</span>
      </li>
    {/each}
    {#if folding && ended}
      <li class="font-mono text-[11px] text-faint">{ended} finished</li>
    {/if}
  </ul>
{/if}
