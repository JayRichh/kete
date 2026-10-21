<script lang="ts">
  import { app, toast, triageAvailable } from '../store.svelte';
  import { api } from '../api';
  import { openLine } from '../editors/verbs';
  import {
    allowedTarget,
    defaultTarget,
    nextTarget,
    orchestratorRepo,
    registerInput,
    repoChoices,
    routeText,
    sendingLabel,
    targetLabel,
    type Target,
    type TriageSent,
    type TriageTail,
  } from './triage.svelte';

  // The triage strip, pinned under the orchestrator's session: one line files
  // a note into the repo's inbox and sends it to the triage desk or the
  // orchestrator. Enter files it; Esc leaves the field. The field stays
  // usable while a note is on its way (the desk can take two minutes).
  const POLL_MS = 10_000;
  let text = $state('');
  /** The target the user switched to; null follows the repo. */
  let chosen = $state<Target | null>(null);
  /** The repo the user picked; '' follows the orchestrator's. */
  let picked = $state('');
  let tail = $state<TriageTail | null>(null);
  /** Notes on their way: the target of each. */
  let pending = $state<Target[]>([]);
  /** The last send's words, for the repo they were filed into. */
  let sent = $state<{ repo: string; text: string } | null>(null);
  let field = $state<HTMLInputElement>();

  const choices = $derived(repoChoices(app.model, app.studioRepos));
  const repo = $derived(
    choices.find((c) => c.id === picked)?.id ?? choices.find((c) => c.id === orchestratorRepo(app.model))?.id ?? choices[0]?.id ?? '',
  );
  const shown = $derived(triageAvailable() && !!repo);
  const here = $derived(tail?.repo === repo ? tail : null);
  const target = $derived(allowedTarget(here, chosen ?? defaultTarget(here)));
  const canSwitch = $derived(!!here?.desk);
  const sendable = $derived(!!text.trim() && (!here || !!here.inbox));
  const route = $derived(
    pending.length ? sendingLabel(pending[pending.length - 1]) : sent?.repo === repo ? sent.text : (here?.route ?? ''),
  );

  // The field is `focusTriage`'s while the strip is on screen.
  $effect(() => {
    registerInput(shown ? (field ?? null) : null);
    return () => registerInput(null);
  });

  let seq = 0;
  async function load(id: string) {
    const mine = ++seq;
    try {
      const t = (await api.triageTail(id)) as TriageTail;
      if (mine === seq) tail = t;
    } catch {
      if (mine === seq) tail = null;
    }
  }

  // Poll while the strip is up and the window is visible.
  $effect(() => {
    if (!shown) return;
    const id = repo;
    const tick = () => {
      if (document.visibilityState === 'visible') void load(id);
    };
    tick();
    const timer = setInterval(tick, POLL_MS);
    document.addEventListener('visibilitychange', tick);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', tick);
    };
  });

  async function submit() {
    const body = text.trim();
    if (!body || !sendable) return;
    const id = repo;
    const to = target;
    text = '';
    pending = [...pending, to];
    try {
      const out = (await api.triageSubmit(id, body, to)) as TriageSent;
      sent = { repo: id, text: routeText(out) };
    } catch (e) {
      toast(`Could not file the note: ${e}`);
      if (!text) text = body;
    } finally {
      pending.splice(pending.indexOf(to), 1);
      void load(id);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      field?.blur();
    } else if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      void submit();
    }
  }

  const pick =
    'h-full w-full min-w-0 appearance-none border-0 bg-transparent pr-7 pl-3 text-[13px] text-ink outline-none hover:bg-surface focus-visible:-outline-offset-2';
</script>

{#if shown}
  <section class="flex flex-col border-t border-rule-strong bg-bg" aria-label="Triage" data-testid="triage-strip">
    <div class="flex min-h-11 items-stretch">
      {#if choices.length > 1}
        <span class="relative flex max-w-32 min-w-0 border-r border-rule md:max-w-44">
          <select class={pick} value={repo} onchange={(e) => (picked = e.currentTarget.value)} aria-label="Triage repo" title="The repo the note is filed into">
            {#each choices as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
          <svg class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
            ><path d="M1.5 3.5L5 7l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.4" /></svg
          >
        </span>
      {/if}
      <button
        type="button"
        class="flex-none border-0 border-r border-rule bg-transparent px-3 text-[13px] whitespace-nowrap text-ink hover:bg-surface focus-visible:-outline-offset-2 aria-disabled:text-muted aria-disabled:hover:bg-transparent"
        data-testid="triage-target"
        aria-disabled={!canSwitch}
        title={canSwitch ? `Send to the ${nextTarget(here, target) === 'desk' ? 'triage desk' : 'orchestrator'} instead` : 'No triage desk in this repo, so notes go to the orchestrator'}
        onclick={() => {
          if (canSwitch) chosen = nextTarget(here, target);
        }}>{canSwitch ? '\u21c4 ' : ''}{targetLabel(target)}</button
      >
      {#if here && !here.inbox}
        <p class="min-w-0 flex-1 truncate self-center px-3 text-[13px] text-muted" title={here.error ?? 'This repo names no inbox'}>
          {here.error ?? 'This repo names no inbox, so there is nowhere to file a note.'}
        </p>
      {:else}
        <input
          class="min-w-0 flex-1 border-0 bg-transparent px-3 py-2.5 text-[14px] leading-5 text-ink outline-none placeholder:overflow-hidden placeholder:text-ellipsis placeholder:whitespace-nowrap placeholder:text-faint focus-visible:-outline-offset-2"
          type="text"
          bind:this={field}
          bind:value={text}
          onkeydown={onKey}
          aria-label="Triage note"
          title="Enter files the note and sends it. Esc leaves the field."
          placeholder="Jot a note. Enter files and sends it."
          data-testid="triage-input"
        />
      {/if}
      <button
        type="button"
        class="flex-none border-0 border-l border-rule-strong bg-ink px-4 text-[13px] font-medium text-bg enabled:hover:bg-surface enabled:hover:text-ink disabled:bg-transparent disabled:text-faint focus-visible:-outline-offset-2"
        disabled={!sendable}
        title="File the note and send it (Enter)"
        onclick={() => void submit()}>File</button
      >
    </div>
    {#if route || here?.notes.length}
      <div class="flex min-h-8 flex-wrap items-center gap-x-3 gap-y-0 border-t border-rule px-3 py-1 text-[12px]">
        <p class="min-w-0 flex-1 basis-60 truncate text-muted" role="status" aria-live="polite" title={route} data-testid="triage-route">{route}</p>
        {#each here?.notes ?? [] as n (n.id)}
          <button
            type="button"
            class="max-w-48 min-w-0 truncate border-0 bg-transparent p-0 font-mono text-[12px] text-ink underline decoration-rule-strong underline-offset-2 hover:bg-surface"
            title="Open {n.title} ({n.status})"
            data-testid="triage-note"
            onclick={() => openLine(n.path, null)}>{n.title} ({n.status})</button
          >
        {/each}
      </div>
    {/if}
  </section>
{/if}
