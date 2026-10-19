<script lang="ts">
  // The session view's side rail: where the session runs, its context,
  // what its checkout has changed, its model and effort, and how its
  // children stand. Facts only; the header keeps the verbs.
  import { onMount } from 'svelte';
  import type { Agent } from './api';
  import { api } from './api';
  import { app, openDiff, toast } from './store.svelte';
  import ContextBar from './accounts/ContextBar.svelte';
  import UsageMeter from './accounts/UsageMeter.svelte';
  import { labelFor } from './accounts/labels.svelte';
  import { accountKey, sessionHarness } from './accounts/usage';
  import { rows, watchGit } from './worktrees/rows.svelte';
  import { changedWords, railFacts } from './sessionRail';

  let { agent }: { agent: Agent } = $props();
  // The rail counts the checkout's changes, so it keeps them fresh itself:
  // with the pane full, no overview or board is mounted to.
  onMount(watchGit);
  const f = $derived(railFacts(app.model, agent, rows.git));
  const plan = $derived(app.model.accounts?.[accountKey(agent.profile, sessionHarness(agent))]?.windows.find((x) => x.kind === 'session') ?? null);
  const kids = $derived(f.children.working + f.children.needs + f.children.done);

  const DOT = ` ${String.fromCharCode(0xb7)} `;
  const LABEL = 'm-0 text-[11px] uppercase tracking-wide text-muted';
</script>

<aside class="flex w-56 flex-none flex-col gap-3 overflow-y-auto border-l border-rule px-3 py-3 text-[13px]" aria-label="Session details">
  <section class="flex flex-col gap-1">
    <h3 class={LABEL}>Runs</h3>
    {#if f.place.kind === 'cloud'}
      {@const url = f.place.url}
      <span class="text-ink">In the cloud</span>
      <button
        class="self-start truncate border-0 bg-transparent p-0 text-left font-mono text-[12px] text-ink underline"
        title={url}
        onclick={() => api.openUrl(url).catch((e) => toast(`Could not open the session: ${e}`))}>Open in the browser</button
      >
    {:else}
      <span class="text-ink">On this machine</span>
      <span class="font-mono text-[12px] text-ink">{f.place.project} / {f.place.branch}</span>
      <span class="truncate font-mono text-[11px] text-muted" title={f.place.path}>{f.place.path}</span>
    {/if}
  </section>

  {#if plan}
    <section class="flex flex-col gap-1">
      <h3 class={LABEL}>Plan usage</h3>
      <UsageMeter window={plan} />
      {#if labelFor(plan)?.countdown}<span class="font-mono text-[11px] text-muted">{labelFor(plan)?.countdown}</span>{/if}
      {#if labelFor(plan)?.banner}<span class="text-[12px] text-fail" data-usage-banner>{labelFor(plan)?.banner}</span>{/if}
    </section>
  {/if}

  {#if f.context && f.context.percent != null}
    <section class="flex flex-col gap-1">
      <h3 class={LABEL}>Context</h3>
      <ContextBar context={f.context} wide />
    </section>
  {/if}

  {#if f.changed != null}
    <section class="flex flex-col gap-1">
      <h3 class={LABEL}>Checkout</h3>
      {#if f.changed > 0}
        <button
          class="self-start border-0 bg-transparent p-0 text-left text-ink underline"
          data-rail="changed"
          title="Open the diff of this worktree"
          onclick={() => openDiff(agent.worktree_id)}>{changedWords(f.changed)}</button
        >
      {:else}
        <span class="text-muted" data-rail="changed">{changedWords(0)}</span>
      {/if}
    </section>
  {/if}

  {#if f.model || f.effort}
    <section class="flex flex-col gap-1">
      <h3 class={LABEL}>Model</h3>
      <span class="font-mono text-[12px] text-ink">{[f.model, f.effort && `${f.effort} effort`].filter(Boolean).join(DOT)}</span>
    </section>
  {/if}

  {#if kids}
    <section class="flex flex-col gap-1">
      <h3 class={LABEL}>Started from here</h3>
      {#if f.children.needs}<span class="text-ember-ink">{f.children.needs} {f.children.needs === 1 ? 'needs' : 'need'} you</span>{/if}
      {#if f.children.working}<span class="text-ink">{f.children.working} working</span>{/if}
      {#if f.children.done}<span class="text-muted">{f.children.done} done</span>{/if}
    </section>
  {/if}
</aside>
