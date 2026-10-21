<script lang="ts">
  // How an orchestrator renewal ended, as a notice that waits: it never takes
  // the focus (you may be typing in a session) and stays until dismissed.
  // Also draws the renew form, which App.svelte does not mount itself.
  import { dismissRenewNote, fileName, openRenewNote, renewNote } from './renew';
  import RenewForm from './RenewForm.svelte';
</script>

<RenewForm />

{#if $renewNote}
  {@const n = $renewNote}
  <div class="fixed top-12 right-4 z-30 w-[min(460px,90vw)]" role="status" aria-live="polite">
    <div class="flex items-center gap-3 border bg-bg py-1.5 pr-1.5 pl-3 text-[13px] text-ink {n.kind === 'failed' ? 'border-fail' : 'border-rule-strong'}">
      {#if n.kind === 'done'}
        <span class="min-w-0 flex-1">
          Orchestrator renewed{n.note ? ' from ' : ''}{#if n.note}<span class="font-mono text-[12px]" title={n.note}>{fileName(n.note)}</span>{/if}
        </span>
        {#if n.note}
          <button class="h-7 flex-none border-0 bg-ink px-3 font-medium text-bg" title="Open the note in its default app" onclick={openRenewNote}>Open note</button>
        {/if}
      {:else}
        <span class="flex min-w-0 flex-1 flex-col">
          <span class="font-medium">Renewal failed: {n.reason}</span>
          <span class="text-muted">The old orchestrator is still in charge.</span>
        </span>
      {/if}
      <button
        class="grid size-7 flex-none place-items-center border-0 bg-transparent text-muted hover:text-ink"
        onclick={dismissRenewNote}
        aria-label="Dismiss"
        title="Dismiss"
      >
        <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="1.4" /></svg>
      </button>
    </div>
  </div>
{/if}
