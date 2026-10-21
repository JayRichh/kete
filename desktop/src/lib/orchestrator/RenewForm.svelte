<script lang="ts">
  // Renew orchestrator: the handover note the fresh session starts on, which
  // account it runs under, and when the swap happens. Opened by the session
  // menu's Renew orchestrator; drawn by RenewNotice.
  import { tick } from 'svelte';
  import { api, type AutoPick } from '../api';
  import { app } from '../store.svelte';
  import { orchestratorOf } from '../orchestrator';
  import { closeRenew, renewForm, submitRenew } from './renew';

  const KEEP = '';
  const PICK = '@@pick';

  let note = $state('');
  let choice = $state(KEEP);
  let keepOld = $state(false);
  let now = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let pick = $state<AutoPick | null>(null);
  let box = $state<HTMLElement>();

  const kind = $derived(orchestratorOf(app.model)?.kind ?? 'claude');
  const profiles = $derived(app.launch.profiles.filter((p) => p.harness === kind && !p.disabled));

  // Each opening starts clean, on the note field.
  $effect(() => {
    if (!$renewForm) return;
    note = '';
    choice = KEEP;
    keepOld = false;
    now = false;
    error = null;
    busy = false;
    pick = null;
    const asked = kind;
    api
      .accountPick(asked, null)
      .then((p) => (pick = p?.profile ? p : null))
      .catch(() => {});
    tick().then(() => box?.querySelector<HTMLElement>('input')?.focus());
  });

  async function submit() {
    if (busy) return;
    if (!note.trim()) {
      error = 'Name the handover note first.';
      return;
    }
    busy = true;
    error = null;
    const profile = choice === PICK ? (pick?.profile ?? null) : choice || null;
    error = await submitRenew(note, profile, keepOld, now);
    busy = false;
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Escape') (e.preventDefault(), closeRenew());
    else if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'INPUT' && (e.target as HTMLInputElement).type === 'text') (e.preventDefault(), submit());
  }

  const field = 'h-8 w-full border border-rule bg-bg px-2 text-[13px] text-ink outline-none hover:border-rule-strong focus:border-rule-strong disabled:text-faint';
  const label = 'text-[12px] font-medium text-muted';
</script>

{#if $renewForm}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="fixed inset-0 z-40 bg-[rgb(28_25_23/0.18)]" onmousedown={closeRenew}></div>
  <div
    bind:this={box}
    class="fixed top-[16%] left-1/2 z-50 w-[min(520px,92vw)] -translate-x-1/2 border border-rule-strong bg-bg text-ink"
    role="dialog"
    aria-modal="true"
    aria-labelledby="renew-title"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="renew-title" class="m-0 border-b border-rule px-4 py-2.5 text-[15px] font-semibold">Renew the orchestrator</h2>
    <div class="flex flex-col gap-3 px-4 py-3">
      <p class="m-0 text-[13px] text-muted">A fresh orchestrator starts on your handover note. The old one is archived once the new one reports.</p>
      <label class="flex flex-col gap-1">
        <span class={label}>Handover note</span>
        <input
          class="{field} font-mono text-[12px]"
          type="text"
          bind:value={note}
          placeholder="Full path to the note file"
          aria-label="Handover note"
          title="The file the new orchestrator reads first: a full path"
          spellcheck="false"
        />
      </label>
      <label class="flex flex-col gap-1">
        <span class={label}>Account</span>
        <select class={field} bind:value={choice} aria-label="Account" title="Which login the new orchestrator runs under">
          <option value={KEEP}>Keep the current account</option>
          {#if pick}<option value={PICK}>Let Kete pick: {pick.profile}</option>{/if}
          {#each profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
      </label>
      <label class="flex items-center gap-2 text-[13px] text-ink">
        <input type="checkbox" class="size-3.5 accent-[var(--color-ink)]" bind:checked={keepOld} />
        Keep old session
        <span class="text-muted">(it stays as a worker)</span>
      </label>
      <label class="flex items-center gap-2 text-[13px] text-ink">
        <input type="checkbox" class="size-3.5 accent-[var(--color-ink)]" bind:checked={now} />
        Renew now
        <span class="text-muted">(unchecked: waits for its turn to end)</span>
      </label>
      {#if error}
        <p class="m-0 border border-fail px-3 py-2 text-[13px] text-fail" role="alert">{error}</p>
      {/if}
    </div>
    <div class="flex items-center justify-end gap-2 border-t border-rule px-4 py-2.5">
      <button class="h-7 border border-rule bg-transparent px-3 text-[13px] text-ink hover:border-rule-strong" title="Esc" onclick={closeRenew}>Cancel</button>
      <button
        class="h-7 border-0 bg-ink px-3 text-[13px] font-medium text-bg disabled:bg-transparent disabled:text-faint"
        title="Enter"
        disabled={busy}
        onclick={submit}>{busy ? 'Renewing\u2026' : 'Renew'}</button
      >
    </div>
  </div>
{/if}
