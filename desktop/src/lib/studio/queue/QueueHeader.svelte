<script lang="ts">
  // One row over the board: autonomy, the runner's state, and Send now.
  import { app, toast } from '../../store.svelte';
  import AutonomyControl from './AutonomyControl.svelte';
  import { queueState, stateLine } from './queue.svelte';

  let props: { repo: string | null } = $props();

  const q = queueState(() => props.repo);
  const info = $derived(app.studioRepos.find((r) => r.repo === props.repo));
  const row = $derived(q.row);
  const line = $derived(q.error ?? (q.loaded ? stateLine(row, Date.now()) : 'Reading the queue'));
</script>

{#if props.repo}
  <div class="flex min-w-0 flex-wrap items-start gap-x-4 gap-y-2 border border-rule px-3 py-2" data-queue-header>
    {#if row || !q.loaded}
      <AutonomyControl
        repoName={row?.name ?? info?.name ?? props.repo}
        level={row?.autonomy ?? info?.autonomy ?? 'A1'}
        onchange={(to) => (row ? q.setLevel(to) : toast('The queue has not loaded yet'))}
      />
    {/if}
    <p class="m-0 min-w-0 flex-1 basis-[200px] self-center text-[13px] {q.error ? 'text-ember-ink' : 'text-muted'}" data-queue-state>{line}</p>
    {#if row}
      <p class="m-0 self-center text-[13px] text-muted">
        {row.open} open
        <span class={row.to_jay ? 'font-medium text-ember-ink' : ''}>{row.to_jay} for you</span>
      </p>
    {/if}
    <button
      class="h-7 self-center border border-rule bg-transparent px-3 text-[13px] text-ink hover:bg-surface disabled:cursor-default disabled:text-faint disabled:hover:bg-transparent"
      title="Send the idle note now, if the orchestrator is idle"
      disabled={q.sending || !row || row.unannounced === 0}
      onclick={() => q.sendNow()}>{q.sending ? 'Sending' : 'Send now'}</button
    >
  </div>
{/if}
