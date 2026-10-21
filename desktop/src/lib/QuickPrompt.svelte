<script lang="ts">
  import { app, openSession, projectOf, sessionWorktree, toast } from './store.svelte';
  import { api, type AutoPick, type HarnessInfo, type Preset } from './api';
  import { initialProfile, profileOptions, stillOffered } from './accounts/pick';
  import { projectAccount } from './accounts/view';
  import { nav } from './shell/history.svelte';
  import { ORCH_TARGET, orchestratorOf, quickPromptTarget } from './orchestrator';
  import { scopedRepo, scopedWorktree } from './scope';
  import { sendToOrchestrator } from './orch.svelte';
  import { whileCutting } from './worktrees/rows.svelte';
  import { recentCheckouts, type RecentCheckout } from './recent';
  import RolePicker from './roles/RolePicker.svelte';

  // The quick prompt: pinned under the main column. Pick where and with
  // what, type the task, Enter starts it (Shift+Enter for a new line). P
  // focuses it from anywhere. While it has the focus (or holds text) a
  // second row opens with the launch's options: model, effort, profile,
  // agent preset, and a new worktree cut for it \u2014 each starting on the
  // Settings defaults, as the terminal app's quick prompt does.
  const TERMINAL = 'terminal';
  const NEW_WT = '__new';
  /** The "to orchestrator" target: the text goes to it as a follow-up turn. */
  const ORCH = ORCH_TARGET;
  let project = $state('');
  let worktree = $state('');
  let harness = $state('claude');
  /** A profile name, or '' for auto: none named, the spread chooses. */
  let profile = $state('');
  let autoPick = $state<AutoPick | null>(null);
  let model = $state('');
  let effort = $state('');
  let preset = $state('');
  /** A role card of the repo, or null. */
  let role = $state<string | null>(null);
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
  /** The repo's default account for this harness, which comes before the spread. */
  const projectDefault = $derived(projectAccount(app.launch, app.model.projects.find((p) => p.id === project)?.repo_path, harness));
  const profileOpts = $derived(profileOptions(profiles, app.launch.spread, autoPick));
  const models = $derived(info?.choices?.models ?? []);
  const efforts = $derived(info?.choices?.efforts[model] ?? info?.choices?.efforts[info?.choices?.model ?? ''] ?? []);
  const orch = $derived(orchestratorOf(app.model));
  const toOrch = $derived(project === ORCH);
  const isTerminal = $derived(harness === TERMINAL && !toOrch);
  const expanded = $derived((focused || !!prompt.trim()) && !toOrch);
  const usablePresets = $derived(presets.filter((p) => p.usable !== false));
  /** The checkouts sessions last ran in, one click each. */
  const recent = $derived(recentCheckouts(app.model));

  /** A Recent pick: its repo and checkout, held like a repo picked by hand. */
  function pickRecent(r: RecentCheckout) {
    project = r.project;
    picked = true;
    // The worktree list follows the repo; set the checkout once it has.
    queueMicrotask(() => (worktree = r.worktree));
    box?.focus();
  }

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

  // A role belongs to its repo: a new repo starts with none.
  $effect(() => {
    void project;
    role = null;
  });

  const ready = $derived(app.model.connected && !busy && (toOrch ? !!orch && !!prompt.trim() : !!worktree));

  // Follow the window (the repo in view, else the open session's) until
  // the user picks a repo by hand; that pick holds until a session starts.
  let picked = false;
  let lastView = '';
  $effect(() => {
    // On the orchestrator's view the prompt talks to it, until a repo is
    // picked; leaving that view, the target it set goes back to a repo.
    const view = nav.current.view;
    const left = lastView === 'orchestrator' && view !== 'orchestrator';
    lastView = view;
    const fallback = projectOf(sessionWorktree(app.session)) ?? app.model.projects[0]?.id ?? '';
    // The repo in view, else the scope's.
    const repo = nav.current.repo ?? scopedRepo(app.model) ?? undefined;
    project = quickPromptTarget(project, { repo, view, left, picked, hasOrchestrator: !!orch, fallback });
  });
  // A scope on one worktree (or session) starts the prompt there, in its
  // repo, each time the scope or the repo changes.
  let scopedFor = '';
  $effect(() => {
    const wt = scopedWorktree(app.model);
    const key = `${wt ?? ''}|${project}`;
    if (key === scopedFor) return;
    scopedFor = key;
    if (wt && worktrees.some((w) => w.id === wt)) worktree = wt;
  });
  $effect(() => {
    if (app.model.projects.find((p) => p.id === project) || (project === ORCH && orch)) return;
    project = projectOf(sessionWorktree(app.session)) ?? app.model.projects[0]?.id ?? '';
  });
  $effect(() => {
    if (worktree !== NEW_WT && !worktrees.find((w) => w.id === worktree)) worktree = worktrees[0]?.id ?? '';
  });
  // The profile starts where initialProfile says for each harness (auto
  // while a spread is on) and again when the list or the spread changes,
  // and steps back there when its pick is gone.
  let profileFor = '';
  $effect(() => {
    const key = [harness, app.launch.spread ?? '', projectDefault ?? '', ...profiles.map((p) => `${p.name}:${p.default}:${p.disabled}`)].join('|');
    if (profileFor !== key || !stillOffered(profile, profiles)) {
      profileFor = key;
      profile = initialProfile(profiles, app.launch.spread, projectDefault);
    }
  });
  // What auto would pick, asked again on each opening (usage moves).
  $effect(() => {
    if (!expanded || isTerminal || !profiles.length) {
      autoPick = null;
      return;
    }
    const kind = harness;
    api
      .accountPick(kind, project && project !== ORCH ? project : null)
      .then((p) => {
        if (kind === harness) autoPick = p;
      })
      .catch(() => (autoPick = null));
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
    if (toOrch) {
      busy = true;
      const sent = await sendToOrchestrator(prompt);
      busy = false;
      if (sent) {
        toast(`Sent to ${orch?.name ?? 'the orchestrator'}.`);
        prompt = '';
        queueMicrotask(grow);
      }
      return;
    }
    busy = true;
    try {
      const c = info?.choices;
      const launch = () => api.quickLaunch({
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
        role: !isTerminal && role ? role : null,
        // shortcut: `role` rides past QuickLaunch's type until api.ts names it.
      } as Parameters<typeof api.quickLaunch>[0]);
      // A new worktree shows as a placeholder in its repo while git cuts it.
      const out = await (worktree === NEW_WT ? whileCutting(project, 'new worktree', launch) : launch());
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
      role = null;
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
  data-critter-floor
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
    <div class="absolute right-0 bottom-full left-0 z-10 flex flex-col">
      {#if recent.length && !toOrch}
        <div class="flex h-8 items-center gap-1 overflow-x-auto border-t border-rule-strong bg-bg px-3" role="group" aria-label="Recent checkouts">
          <span class="pr-1 font-mono text-[11px] text-muted">recent</span>
          {#each recent as r (r.worktree)}
            <button
              type="button"
              class="h-6 flex-none border border-rule px-2 font-mono text-[12px] whitespace-nowrap hover:bg-surface {r.worktree === worktree
                ? 'text-ink'
                : 'text-muted'}"
              aria-pressed={r.worktree === worktree}
              title="Start in {r.label}"
              onclick={() => pickRecent(r)}>{r.label}</button
            >
          {/each}
        </div>
      {/if}
      <div
        class="@container flex min-h-8 flex-wrap items-stretch border-t border-rule-strong bg-bg *:h-8"
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
        <span class="relative flex max-w-60 min-w-0 items-center border-r border-rule">
          <span class={optLabel}>profile</span>
          <select
            class={pick}
            bind:value={profile}
            disabled={isTerminal || !profiles.length}
            aria-label="Profile"
            title={profileOpts.find((o) => o.value === profile)?.title}
          >
            {#each profileOpts as o (o.value)}<option value={o.value} title={o.title}>{o.label}</option>{/each}
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
        {#if !isTerminal}<RolePicker repo={project} bind:value={role} bar />{/if}
        <label class="flex items-center gap-2 border-r border-rule px-3 text-[12px] whitespace-nowrap text-ink">
          <input
            type="checkbox"
            class="size-3.5 accent-[var(--color-ink)]"
            checked={worktree === NEW_WT}
            onchange={(e) => (worktree = (e.currentTarget as HTMLInputElement).checked ? NEW_WT : (worktrees[0]?.id ?? ''))}
          />
          new worktree
        </label>
        <span class="flex-1"></span>
        <!-- Gives way first in a narrow row: the prompt's placeholder names Enter too. -->
        <span class="flex flex-none items-center pr-3 font-mono text-[11px] whitespace-nowrap text-muted @max-[1100px]:hidden">Enter start &middot; Shift+Enter new line</span>
      </div>
    </div>
  {/if}
  <div class="flex min-h-11 items-stretch">
    <span class="relative flex max-w-28 min-w-0 border-r border-rule md:max-w-40">
      <select class={pick} bind:value={project} onchange={() => (picked = true)} aria-label="Repo">
        {#if orch}<option value={ORCH}>&#9670; Orchestrator</option>{/if}
        {#each app.model.projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      {@render chevron()}
    </span>
    <span class="relative flex max-w-32 min-w-0 border-r border-rule md:max-w-44">
      <select class="{pick} font-mono text-[12px]" bind:value={worktree} aria-label="Worktree" disabled={toOrch}>
        {#each worktrees as w (w.id)}<option value={w.id}>{w.branch}</option>{/each}
        <option value={NEW_WT}>+ new worktree</option>
      </select>
      {@render chevron()}
    </span>
    <span class="relative flex max-w-28 min-w-0 border-r border-rule md:max-w-32">
      <select class={pick} bind:value={harness} aria-label="Harness" disabled={toOrch}>
        {#each harnesses as h (h.id)}<option value={h.id}>{h.label}</option>{/each}
        <option value={TERMINAL}>Terminal</option>
      </select>
      {@render chevron()}
    </span>
    <textarea
      class="min-w-0 flex-1 resize-none self-center border-0 bg-transparent px-3 py-2.5 text-[14px] leading-5 text-ink outline-none placeholder:overflow-hidden placeholder:text-ellipsis placeholder:whitespace-nowrap placeholder:text-faint disabled:text-faint"
      rows="1"
      bind:this={box}
      bind:value={prompt}
      oninput={grow}
      onkeydown={onKey}
      disabled={isTerminal}
      aria-label="Prompt"
      placeholder={toOrch
        ? `Tell ${orch?.name ?? 'the orchestrator'} what you want. Enter sends it as a follow-up.`
        : isTerminal
        ? 'Opens a shell in this worktree. Enter to start.'
        : preset
          ? `Task for ${preset} (optional). Enter to start.`
          : 'What should it do? Empty opens the CLI. P to focus, Enter to start.'}
    ></textarea>
    <button
      type="submit"
      class="flex-none border-0 border-l border-rule-strong bg-ink px-4 text-[13px] font-medium text-bg enabled:hover:bg-surface enabled:hover:text-ink disabled:bg-transparent disabled:text-faint"
      disabled={!ready}>{busy ? (toOrch ? 'Sending\u2026' : 'Starting\u2026') : toOrch ? 'Send' : 'Start'}</button
    >
  </div>
</form>
