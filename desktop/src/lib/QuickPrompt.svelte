<script lang="ts">
  import { app, openSession, projectOf, sessionWorktree, toast } from './store.svelte';
  import { api, type HarnessInfo, type Preset } from './api';
  import { nav } from './shell/history.svelte';

  // The quick prompt: pinned under the main column. Pick where and with
  // what, type the task, Enter starts it (Shift+Enter for a new line). P
  // focuses it from anywhere. While it has the focus (or holds text) a
  // second row opens with the launch's options: model, effort, profile,
  // agent preset, and a new worktree cut for it \u2014 each starting on the
  // Settings defaults, as the terminal app's quick prompt does.
  const TERMINAL = 'terminal';
  const NEW_WT = '__new';
  let project = $state('');
  let worktree = $state('');
  let harness = $state('claude');
  let profile = $state('');
  let model = $state('');
  let effort = $state('');
  let preset = $state('');
  let prompt = $state('');
  let busy = $state(false);
  let focused = $state(false);
  let presets = $state<Preset[]>([]);
  let box = $state<HTMLTextAreaElement>();
  let form = $state<HTMLFormElement>();

  const harnesses = $derived<HarnessInfo[]>(app.launch.harnesses.length ? app.launch.harnesses : [{ id: 'claude', label: 'Claude' }]);
  const info = $derived(harnesses.find((h) => h.id === harness));
  const worktrees = $derived(
    app.model.worktrees
      .filter((w) => w.project_id === project)
      .sort((a, b) => Number(b.is_main) - Number(a.is_main) || a.sort_order - b.sort_order),
  );
  const profiles = $derived(app.launch.profiles.filter((p) => p.harness === harness));
  const models = $derived(info?.choices?.models ?? []);
  const efforts = $derived(info?.choices?.efforts[model] ?? info?.choices?.efforts[info?.choices?.model ?? ''] ?? []);
  const isTerminal = $derived(harness === TERMINAL);
  const expanded = $derived(focused || !!prompt.trim());
  const usablePresets = $derived(presets.filter((p) => p.usable !== false));

  function loadPresets() {
    api
      .presets()
      .then((p) => (presets = p))
      .catch(() => {});
  }
  $effect(loadPresets);

  // Start on the Agents tab's quick prompt harness, and step off one the
  // settings have since switched off (as the TUI's quick prompt does).
  let seeded = false;
  $effect(() => {
    const list = harnesses;
    const def = app.launch.default_harness;
    if (!seeded && def && list.some((h) => h.id === def)) {
      seeded = true;
      if (harness !== TERMINAL) harness = def;
      if (app.launch.new_worktree) worktree = NEW_WT;
      return;
    }
    if (harness !== TERMINAL && !list.some((h) => h.id === harness)) {
      harness = def && list.some((h) => h.id === def) ? def : list[0].id;
    }
  });

  // Model and effort follow the harness's configured defaults until picked.
  let modelFor = '';
  $effect(() => {
    const c = info?.choices;
    if (!c || modelFor === harness) return;
    modelFor = harness;
    model = c.models.find((m) => m === c.model) ?? c.models[0] ?? '';
    effort = c.effort;
  });
  $effect(() => {
    if (efforts.length && !efforts.includes(effort)) effort = efforts.includes(info?.choices?.effort ?? '') ? info!.choices!.effort : efforts[0];
  });

  const ready = $derived(app.model.connected && !!worktree && !busy);

  // Follow the window (the repo in view, else the open session's) until
  // the user picks a repo by hand; that pick holds until a session starts.
  let picked = false;
  $effect(() => {
    const repo = nav.current.repo;
    if (repo && !picked) project = repo;
  });
  $effect(() => {
    if (app.model.projects.find((p) => p.id === project)) return;
    project = projectOf(sessionWorktree(app.session)) ?? app.model.projects[0]?.id ?? '';
  });
  $effect(() => {
    if (worktree !== NEW_WT && !worktrees.find((w) => w.id === worktree)) worktree = worktrees[0]?.id ?? '';
  });
  $effect(() => {
    const def = profiles.find((p) => p.default);
    if (!profiles.find((p) => p.name === profile)) profile = def?.name ?? '';
  });

  // A focus request, maybe with a preset from the rail, a repo row or a
  // duplicated session.
  let seen = 0;
  $effect(() => {
    const n = app.quickFocus;
    if (n === seen) return;
    seen = n;
    const p = app.quickPreset;
    app.quickPreset = null;
    if (p?.project) {
      project = p.project;
      picked = true;
    }
    if (p?.harness) harness = p.harness;
    queueMicrotask(() => {
      if (p?.worktree) worktree = p.worktree;
      if (p?.newWorktree !== undefined && !p.worktree) worktree = p.newWorktree ? NEW_WT : (worktrees[0]?.id ?? '');
      if (p?.profile) profile = p.profile;
      if (p?.model) model = p.model;
      if (p?.effort) effort = p.effort;
    });
    loadPresets();
    box?.focus();
  });

  /** A preset picked: its harness, model and effort (the text goes on at Start). */
  function pickPreset() {
    const p = presets.find((x) => x.name === preset);
    if (!p) return;
    harness = p.harness;
    queueMicrotask(() => {
      modelFor = harness;
      const c = harnesses.find((h) => h.id === p.harness)?.choices;
      model = p.model ?? c?.model ?? 'default';
      effort = p.effort ?? c?.effort ?? '';
    });
  }

  function grow() {
    if (!box) return;
    box.style.height = 'auto';
    box.style.height = `${Math.min(box.scrollHeight, 160)}px`;
  }

  async function submit() {
    if (!ready) return;
    busy = true;
    try {
      const c = info?.choices;
      const out = await api.quickLaunch({
        project,
        worktree: worktree === NEW_WT ? '' : worktree,
        newWorktree: worktree === NEW_WT,
        kind: harness,
        profile: profile || null,
        prompt,
        // The configured pick goes as none, so the Agents tab decides.
        model: !isTerminal && model && model !== c?.model ? model : null,
        effort: !isTerminal && efforts.length && effort && effort !== c?.effort ? effort : null,
        preset: !isTerminal && preset ? preset : null,
      });
      const follow = app.launch.follow !== false;
      if (out.agent) {
        if (follow) openSession({ Agent: out.agent });
        else toast('Started. It is on the overview.');
        // Which account the spread gave a launch that named none, and why.
        if (out.picked) toast(follow ? out.picked : `Started on ${out.picked}. It is on the overview.`);
      } else if (out.terminal) openSession({ Terminal: out.terminal });
      if (worktree === NEW_WT && !app.launch.new_worktree) worktree = out.worktree;
      prompt = '';
      preset = '';
      picked = false;
      queueMicrotask(grow);
      box?.blur();
    } catch (e) {
      toast(String(e));
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      box?.blur();
    } else if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  function onFocusOut(e: FocusEvent) {
    if (!form?.contains(e.relatedTarget as Node | null)) focused = false;
  }

  const pick =
    'h-full w-full min-w-0 appearance-none border-0 bg-transparent pr-7 pl-3 text-[13px] text-ink outline-none hover:bg-surface focus-visible:-outline-offset-2 disabled:text-faint';
  const optLabel = 'pl-3 font-mono text-[11px] text-muted';
</script>

{#snippet chevron()}
  <svg class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
    ><path d="M1.5 3.5L5 7l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.4" /></svg
  >
{/snippet}

<form
  bind:this={form}
  class="relative flex flex-none flex-col border-t border-rule-strong bg-bg"
  aria-label="Start a session"
  onfocusin={() => (focused = true)}
  onfocusout={onFocusOut}
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  {#if expanded}
    <!-- Over the column, not in it: opening it must not shrink the split
         (a session terminal would refit and redraw on every focus). -->
    <div
      class="absolute right-0 bottom-full left-0 z-10 flex h-8 items-stretch border-t border-rule-strong bg-bg"
      role="group"
      aria-label="Launch options"
    >
      <span class="relative flex max-w-60 min-w-0 items-center border-r border-rule">
        <span class={optLabel}>model</span>
        <select class="{pick} font-mono text-[12px]" bind:value={model} disabled={isTerminal || !models.length} aria-label="Model">
          {#each models as m (m)}<option value={m}>{m}{m === info?.choices?.model ? ' (set)' : ''}</option>{/each}
        </select>
        {@render chevron()}
      </span>
      <span class="relative flex max-w-52 min-w-0 items-center border-r border-rule">
        <span class={optLabel}>effort</span>
        <select class="{pick} font-mono text-[12px]" bind:value={effort} disabled={isTerminal || !efforts.length} aria-label="Effort">
          {#if !efforts.length}<option value={effort}>n/a</option>{/if}
          {#each efforts as x (x)}<option value={x}>{x}{x === info?.choices?.effort ? ' (set)' : ''}</option>{/each}
        </select>
        {@render chevron()}
      </span>
      <span class="relative flex max-w-44 min-w-0 items-center border-r border-rule">
        <span class={optLabel}>profile</span>
        <select class={pick} bind:value={profile} disabled={isTerminal || !profiles.length} aria-label="Profile">
          <option value="">{profiles.length ? 'none' : 'default login'}</option>
          {#each profiles as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
        {@render chevron()}
      </span>
      <span class="relative flex max-w-52 min-w-0 items-center border-r border-rule">
        <span class={optLabel}>preset</span>
        <select class={pick} bind:value={preset} onchange={pickPreset} disabled={isTerminal || !usablePresets.length} aria-label="Agent preset">
          <option value="">{usablePresets.length ? 'none' : 'no presets'}</option>
          {#each usablePresets as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
        {@render chevron()}
      </span>
      <label class="flex items-center gap-2 border-r border-rule px-3 text-[12px] text-ink">
        <input
          type="checkbox"
          class="size-3.5 accent-[var(--color-ink)]"
          checked={worktree === NEW_WT}
          onchange={(e) => (worktree = (e.currentTarget as HTMLInputElement).checked ? NEW_WT : (worktrees[0]?.id ?? ''))}
        />
        new worktree
      </label>
      <span class="flex-1"></span>
      <span class="flex items-center pr-3 font-mono text-[11px] text-muted">Enter start &middot; Shift+Enter new line</span>
    </div>
  {/if}
  <div class="flex min-h-11 items-stretch">
    <span class="relative flex max-w-40 min-w-0 border-r border-rule">
      <select class={pick} bind:value={project} onchange={() => (picked = true)} aria-label="Repo">
        {#each app.model.projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      {@render chevron()}
    </span>
    <span class="relative flex max-w-44 min-w-0 border-r border-rule">
      <select class="{pick} font-mono text-[12px]" bind:value={worktree} aria-label="Worktree">
        {#each worktrees as w (w.id)}<option value={w.id}>{w.branch}</option>{/each}
        <option value={NEW_WT}>+ new worktree</option>
      </select>
      {@render chevron()}
    </span>
    <span class="relative flex max-w-32 min-w-0 border-r border-rule">
      <select class={pick} bind:value={harness} aria-label="Harness">
        {#each harnesses as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
        <option value={TERMINAL}>Terminal</option>
      </select>
      {@render chevron()}
    </span>
    <textarea
      class="min-w-0 flex-1 resize-none self-center border-0 bg-transparent px-3 py-2.5 text-[14px] leading-5 text-ink outline-none placeholder:text-faint disabled:text-faint"
      rows="1"
      bind:this={box}
      bind:value={prompt}
      oninput={grow}
      onkeydown={onKey}
      disabled={isTerminal}
      aria-label="Prompt"
      placeholder={isTerminal
        ? 'Opens a shell in this worktree. Enter to start.'
        : preset
          ? `Task for ${preset} (optional). Enter to start.`
          : 'What should it do? Empty opens the CLI. P to focus, Enter to start.'}
    ></textarea>
    <button
      type="submit"
      class="flex-none border-0 border-l border-rule-strong bg-ink px-4 text-[13px] font-medium text-bg enabled:hover:bg-surface enabled:hover:text-ink disabled:bg-transparent disabled:text-faint"
      disabled={!ready}>{busy ? 'Starting\u2026' : 'Start'}</button
    >
  </div>
</form>
