<script lang="ts">
  // Start orchestrator: harness, model, effort and profile, each starting on
  // the Agents tab's defaults (the quick prompt's harness, its configured
  // model and effort), and the project it runs in and its name, as `kete
  // orchestrator start` takes them. Drawn in the orchestrator view when
  // there is none, and as a dialog from the palette, 0 or F10 elsewhere.
  import { tick } from 'svelte';
  import { app } from '../store.svelte';
  import { ui } from '../ui/ui.svelte';
  import { startOrchestrator } from '../orch.svelte';
  import { canProvision, orchestratorOf, RELOAD_HINT } from '../orchestrator';
  import type { HarnessInfo } from '../api';
  import { initialProfile, profileOptions, stillOffered } from '../accounts/pick';
  import RolePicker from '../roles/RolePicker.svelte';

  let { inline = false }: { inline?: boolean } = $props();

  const harnesses = $derived<HarnessInfo[]>(app.launch.harnesses.length ? app.launch.harnesses : [{ id: 'claude', label: 'Claude' }]);
  let harness = $state('');
  let model = $state('');
  let effort = $state('');
  let profile = $state('');
  let project = $state('');
  let name = $state('');
  let role = $state<string | null>(null);
  let busy = $state(false);
  let box = $state<HTMLElement>();

  const info = $derived(harnesses.find((h) => h.id === harness));
  const models = $derived(info?.choices?.models ?? []);
  const efforts = $derived(info?.choices?.efforts[model] ?? info?.choices?.efforts[info?.choices?.model ?? ''] ?? []);
  const profiles = $derived(app.launch.profiles.filter((p) => p.harness === harness));
  const profileOpts = $derived(profileOptions(profiles, app.launch.spread, null));
  const ok = $derived(canProvision(app.model));
  const existing = $derived(orchestratorOf(app.model));
  const open = $derived(inline || ui.startOrchestrator);

  // Defaults: the quick prompt's harness, then its configured model/effort.
  $effect(() => {
    if (harness && harnesses.some((h) => h.id === harness)) return;
    const def = app.launch.default_harness;
    harness = def && harnesses.some((h) => h.id === def) ? def : harnesses[0].id;
  });
  let modelFor = '';
  $effect(() => {
    const c = info?.choices;
    // Wait for the harness's choices (the launchables load after boot).
    if (!c || modelFor === harness) return;
    modelFor = harness;
    model = c.models.find((m) => m === c.model) ?? c.models[0] ?? '';
    effort = c.effort;
  });
  $effect(() => {
    if (efforts.length && !efforts.includes(effort)) effort = efforts.includes(info?.choices?.effort ?? '') ? info!.choices!.effort : efforts[0];
  });
  // As the quick prompt: auto while a spread is on, else the default.
  let profileFor = '';
  $effect(() => {
    const key = [harness, app.launch.spread ?? '', ...profiles.map((p) => `${p.name}:${p.default}:${p.disabled}`)].join('|');
    if (profileFor !== key || !stillOffered(profile, profiles)) {
      profileFor = key;
      profile = initialProfile(profiles, app.launch.spread);
    }
  });
  $effect(() => {
    if (open && !inline) tick().then(() => box?.querySelector<HTMLElement>('select')?.focus());
  });

  async function start() {
    if (busy || !ok) return;
    busy = true;
    const c = info?.choices;
    await startOrchestrator({
      kind: harness,
      profile: profile || null,
      // The configured pick goes as none, so the Agents tab decides.
      model: model && model !== c?.model ? model : null,
      effort: efforts.length && effort && effort !== c?.effort ? effort : null,
      project: project || null,
      name: name.trim() || null,
      // A role belongs to a repo; its own workspace has none.
      role: project ? role : null,
      // shortcut: `role` rides past the spec's type until orch.svelte.ts names it.
    } as Parameters<typeof startOrchestrator>[0]);
    busy = false;
  }

  function cancel() {
    ui.startOrchestrator = false;
  }

  function onKey(e: KeyboardEvent) {
    if (inline) {
      if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'BUTTON') (e.preventDefault(), start());
      return;
    }
    e.stopPropagation();
    if (e.key === 'Escape') (e.preventDefault(), cancel());
    else if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'BUTTON') (e.preventDefault(), start());
  }

  const field = 'h-8 w-full border border-rule bg-bg px-2 text-[13px] text-ink outline-none hover:border-rule-strong focus:border-rule-strong disabled:text-faint';
  const label = 'text-[12px] font-medium text-muted';
</script>

{#snippet form()}
  <div class="grid grid-cols-2 gap-x-4 gap-y-3">
    <label class="flex flex-col gap-1">
      <span class={label}>Harness</span>
      <select class={field} bind:value={harness} aria-label="Harness">
        {#each harnesses as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class={label}>Profile</span>
      <select class={field} bind:value={profile} disabled={!profiles.length} aria-label="Profile">
        {#each profileOpts as o (o.value)}<option value={o.value} title={o.title}>{o.label}</option>{/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class={label}>Model</span>
      <select class="{field} font-mono text-[12px]" bind:value={model} disabled={!models.length} aria-label="Model">
        {#if !models.length}<option value="">default</option>{/if}
        {#each models as m (m)}<option value={m}>{m}{m === info?.choices?.model ? ' (set)' : ''}</option>{/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class={label}>Effort</span>
      <select class="{field} font-mono text-[12px]" bind:value={effort} disabled={!efforts.length} aria-label="Effort">
        {#if !efforts.length}<option value={effort}>n/a</option>{/if}
        {#each efforts as x (x)}<option value={x}>{x}{x === info?.choices?.effort ? ' (set)' : ''}</option>{/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class={label}>Project</span>
      <select class={field} bind:value={project} aria-label="Project" title="Where it runs: a repo's root checkout, or a workspace of its own under kete's data dir">
        <option value="">Its own workspace</option>
        {#each app.model.projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class={label}>Name</span>
      <input class={field} bind:value={name} placeholder="The daemon names it" aria-label="Name" spellcheck="false" />
    </label>
    {#if project}<RolePicker repo={project} bind:value={role} />{/if}
  </div>
  {#if !ok}
    <p class="m-0 border border-fail px-3 py-2 text-[13px] text-fail" role="status">
      {app.model.connected ? RELOAD_HINT : 'Not connected to the kete daemon.'}
    </p>
  {:else if existing}
    <p class="m-0 text-[13px] text-muted">{existing.name} is the orchestrator now. Starting another makes it an ordinary session; it keeps running.</p>
  {/if}
{/snippet}

{#if inline}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <section class="flex h-full min-h-0 flex-1 items-start justify-center overflow-y-auto px-6 pt-[12vh]" onkeydown={onKey}>
    <div class="w-[min(560px,100%)] border border-rule-strong bg-bg">
      <header class="border-b border-rule px-5 py-4">
        <h1 class="m-0 text-[22px] font-semibold tracking-tight text-ink">Start the orchestrator</h1>
        <p class="m-0 mt-1.5 max-w-[60ch] text-[14px] text-muted">
          One session that watches every repo for you: it reads the others, sends them follow-ups, starts workers in fresh
          worktrees, and asks you to review when something is ready. You talk to it; it does the clicking.
        </p>
      </header>
      <div class="flex flex-col gap-4 px-5 py-4">{@render form()}</div>
      <footer class="flex items-center justify-between gap-3 border-t border-rule px-5 py-3">
        <span class="font-mono text-[11px] text-muted"
          >{project ? 'Runs in the repo\u2019s root checkout' : 'Runs in its own workspace'} &middot; one per machine</span
        >
        <button
          class="h-8 border-0 bg-ink px-4 text-[13px] font-medium text-bg disabled:bg-transparent disabled:text-faint"
          disabled={!ok || busy}
          onclick={start}>{busy ? 'Starting\u2026' : 'Start orchestrator'}</button
        >
      </footer>
    </div>
  </section>
{:else if open}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="fixed inset-0 z-40 bg-[rgb(28_25_23/0.18)]" onmousedown={cancel}></div>
  <div
    bind:this={box}
    class="fixed top-[16%] left-1/2 z-50 w-[min(560px,92vw)] -translate-x-1/2 border border-rule-strong bg-bg text-ink"
    role="dialog"
    aria-modal="true"
    aria-labelledby="start-orch-title"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="start-orch-title" class="m-0 border-b border-rule px-4 py-2.5 text-[15px] font-semibold">Start the orchestrator</h2>
    <div class="flex flex-col gap-3 px-4 py-3">{@render form()}</div>
    <div class="flex items-center justify-end gap-2 border-t border-rule px-4 py-2.5">
      <button class="h-7 border border-rule bg-transparent px-3 text-[13px] text-ink hover:border-rule-strong" onclick={cancel}>Cancel</button>
      <button class="h-7 border-0 bg-ink px-3 text-[13px] font-medium text-bg disabled:bg-transparent disabled:text-faint" disabled={!ok || busy} onclick={start}
        >{busy ? 'Starting\u2026' : 'Start'}</button
      >
    </div>
  </div>
{/if}
