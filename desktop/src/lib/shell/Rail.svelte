<script lang="ts">
  import { app, openGauntlet, openSettings, openStudio, revealRepo, setThemePref, toast } from '../store.svelte';
  import { nextTheme, themeLabel } from '../look';
  import { look } from '../look.svelte';
  import { api } from '../api';
  import { nav, type View } from './history.svelte';
  import { railOrder, repoRows, tally } from '../overview/repos';
  import { canProvision, descendantCounts, orchestratorOf, orchestratorTree } from '../orchestrator';
  import { askStartOrchestrator, openOrchestrator } from '../orch.svelte';
  import { toneOf } from '../status';
  import Status from '../Status.svelte';
  import AccountsSection from '../accounts/AccountsSection.svelte';
  import { openAccounts } from '../accounts/actions';
  import { openWake } from '../wake/state.svelte';
  import { showStudioRail } from '../studio/types';
  import VersionPlate from './VersionPlate.svelte';
  import { drag, dropProject, endDrag, leftFor, moveOnto, offsetIn, rootOf, startDrag } from '../reorder.svelte';
  import { dropZone, type Zone } from '../order';
  import { withKey, type ShortcutId } from '../shortcuts';
  import { discover } from '../critters/state.svelte';

  /** Icons only, for a narrow window: the repos as their initials. */
  let { compact = false }: { compact?: boolean } = $props();

  const rows = $derived(repoRows(app.model));
  const orch = $derived(orchestratorOf(app.model));
  const orchNeeds = $derived(descendantCounts(orchestratorTree(app.model)).needs ?? 0);
  /** Projects in the user's own order: the rail is a map, not a ranking. */
  const projects = $derived(railOrder(rows));
  /** A repo's digit key, for the first nine. */
  const digit = (i: number): ShortcutId[] => (i < 9 ? [`repo_${i + 1}` as ShortcutId] : []);
  const here = $derived(nav.current.repo);
  const inSettings = $derived(nav.current.view === 'settings');
  const inWake = $derived(nav.current.view === 'whakaoho');
  const inStudio = $derived(nav.current.view === 'studio');
  const inGauntlet = $derived(nav.current.view === 'gauntlet');
  const studio = $derived(showStudioRail(app.studioRepos));
  const STUDIO_TITLE = 'Studio: the board and the idle-note queue of the repos with studio mode';
  const GAUNTLET_TITLE = 'Gauntlet: the sign-offs and verdicts of the repos with studio mode';
  /** Whakaoho, a middle dot, Wake-up (the dot is built, not typed: sources stay ASCII). */
  const WAKE_TITLE = `Whakaoho ${String.fromCharCode(0xb7)} Wake-up: start each account's 5-hour window earlier`;
  let adding = $state(false);
  let addPath = $state('');

  /** Under every repo: its checkouts as one table. */
  const REPO_VIEWS: { view: View; label: string }[] = [{ view: 'worktrees', label: 'Worktrees' }];
  const ADO_VIEWS: { view: View; label: string }[] = [
    { view: 'tasks', label: 'Tasks' },
    { view: 'branches', label: 'Branches' },
    { view: 'prs', label: 'Pull requests' },
  ];

  // A repo drags to reorder the repos; a session dropped on one moves to
  // its root checkout (asked first), as onto a worktree's header.
  let mark = $state<{ id: string; zone: Zone } | null>(null);
  function over(e: DragEvent, id: string) {
    const d = drag.item;
    let zone: Zone | null = null;
    if (d?.kind === 'project' && d.id !== id) {
      const [y, h] = offsetIn(e);
      zone = dropZone(y, h, { insert: true, onto: false });
    } else if (d?.kind === 'agent' && rootOf(id) && rootOf(id) !== d.worktree) zone = 'onto';
    if (!zone) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    mark = { id, zone };
  }
  function drop(e: DragEvent, id: string) {
    const d = drag.item;
    const at = mark;
    mark = null;
    if (!d || !at || at.id !== id) return;
    e.preventDefault();
    endDrag();
    const root = rootOf(id);
    if (d.kind === 'project' && at.zone !== 'onto') void dropProject(d.id, id, at.zone);
    else if (d.kind === 'agent' && at.zone === 'onto' && root) void moveOnto(d.id, root);
  }
  /** The drag wiring a repo's rail button takes. */
  const dnd = (id: string) => ({
    draggable: true,
    ondragstart: (e: DragEvent) => startDrag(e, { kind: 'project', id }),
    ondragend: () => ((mark = null), endDrag()),
    ondragenter: (e: DragEvent) => over(e, id),
    ondragover: (e: DragEvent) => over(e, id),
    ondragleave: (e: DragEvent) => leftFor(e) && mark?.id === id && (mark = null),
    ondrop: (e: DragEvent) => drop(e, id),
  });
  const reorderHint = $derived(withKey('drag to reorder', ['move_up', 'move_down']));

  async function addProject(e: SubmitEvent) {
    e.preventDefault();
    if (!addPath.trim()) return;
    try {
      await api.addProject(addPath);
      addPath = '';
      adding = false;
    } catch (err) {
      toast(String(err));
    }
  }

  function cycleTheme() {
    setThemePref(nextTheme(app.themePref));
  }
  /** What the theme tile says: the theme worn, and that it cycles Workshop's three. */
  const themeWord = $derived(themeLabel(app.themePref, look.prefs.themes));
  const themeTip = $derived(`Theme: ${themeWord}. Press to cycle Workshop: system, light, dark (more themes in Settings, Desktop, Appearance)`);

  // The critters' secret: five quick clicks on the kete mark, and a kiwi
  // climbs out of the basket (and the critters turn on).
  let markClicks: number[] = [];
  function markClick(e: MouseEvent) {
    const now = Date.now();
    markClicks = [...markClicks.filter((t) => now - t < 2500), now];
    if (markClicks.length < 5) return;
    markClicks = [];
    discover('mark', (e.currentTarget as Element).getBoundingClientRect());
  }

  const item =
    'flex w-full min-w-0 items-center gap-2 border-0 px-4 py-1.5 text-left text-[14px] hover:bg-surface focus-visible:-outline-offset-2';
  const heading = 'px-4 pt-5 pb-1.5 text-[12px] font-medium text-muted';
  /** A compact rail's square button. */
  const tile =
    'relative mx-auto grid size-9 place-items-center border border-transparent p-0 font-mono text-[12px] hover:border-rule focus-visible:-outline-offset-2';
  /** A repo's initials for the compact rail: `kete-desktop` reads `KD`. */
  const initials = (name: string) => {
    const words = name.split(/[^A-Za-z0-9]+/).filter(Boolean);
    return (words.length > 1 ? words[0][0] + words[1][0] : name.slice(0, 2)).toUpperCase();
  };
</script>

{#snippet sunrise()}
  <!-- Whakaoho's mark: a half sun with three rays rising from behind a kete (a basket). -->
  <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
    <path d="M5.2 10.5a2.8 2.8 0 0 1 5.6 0M8 4.7v1.8M3.9 6.4l1.3 1.3M12.1 6.4l-1.3 1.3" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M3.5 10.5h9l-1.4 4H4.9zM4.2 12.5h7.6" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="miter" />
  </svg>
{/snippet}

<aside class="flex min-h-0 flex-col border-r border-rule bg-bg">
  {#if compact}
    <div class="flex h-10 flex-none items-center justify-center border-b border-rule" title={app.model.connected ? `kete: daemon pid ${app.model.daemon_pid}` : 'kete: not connected to the daemon'}>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <svg class="flex-none text-ink" width="20" height="20" viewBox="0 0 64 64" aria-hidden="true" onclick={markClick}>
        <path d="M11 31h42l-6.5 26h-29z" fill="currentColor" />
        <path d="M21 31V22a11 11 0 0 1 22 0a5.5 5.5 0 0 1 -11 0" fill="none" stroke="currentColor" stroke-width="6" />
        <circle class={app.model.connected ? 'fill-ember' : 'fill-fail'} cx="32" cy="22" r="3.2" />
      </svg>
    </div>
    <nav class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto py-2" aria-label="Repos">
      <button
        class="{tile} {nav.current.view === 'orchestrator' ? 'bg-surface text-ink' : 'bg-transparent text-ink'}"
        data-target="orchestrator:rail"
        onclick={() => (orch || canProvision(app.model) ? openOrchestrator() : askStartOrchestrator())}
        title={orch ? `The orchestrator, ${orch.name}${orchNeeds ? `: ${orchNeeds} need you` : ''} (0 or F10)` : 'Start an orchestrator (0 or F10)'}
        aria-label="Orchestrator"
        aria-current={nav.current.view === 'orchestrator' ? 'page' : undefined}
      >
        <span aria-hidden="true">{orch ? '\u25c6' : '\u25c7'}</span>
        {#if orchNeeds}<span class="absolute top-1 right-1 size-1.5 bg-ember" aria-hidden="true"></span>{/if}
      </button>
      <span class="mx-3 my-1 h-px flex-none bg-rule" aria-hidden="true"></span>
      {#each projects as r, i (r.project.id)}
        {@const c = tally([r])}
        {@const needs = c.needs?.length ?? 0}
        {@const running = c.running?.length ?? 0}
        {@const on = here === r.project.id}
        <button
          class="{tile} {on ? 'bg-surface font-semibold text-ink' : 'bg-transparent text-muted hover:text-ink'} {mark?.id === r.project.id && mark.zone === 'onto' && drag.item ? 'outline-2 -outline-offset-2 outline-ember' : ''}"
          data-target="project:{r.project.id}"
          {...dnd(r.project.id)}
          onclick={() => revealRepo(r.project.id)}
          title="{withKey(r.name, digit(i))}{needs ? `: ${needs} ${needs === 1 ? 'needs' : 'need'} you` : running ? `: ${running} running` : ''}. {reorderHint}; drop a session to move it here"
          aria-label={r.name}
          aria-current={on ? 'true' : undefined}
        >
          {#if mark?.id === r.project.id && mark.zone !== 'onto' && drag.item}
            <span class="pointer-events-none absolute inset-x-0 z-10 h-px bg-ember {mark.zone === 'before' ? '-top-px' : '-bottom-px'}" aria-hidden="true"></span>
          {/if}
          {initials(r.name)}
          {#if needs}<span class="absolute top-1 right-1 size-1.5 bg-ember" aria-hidden="true"></span>
          {:else if running}<span class="absolute top-1 right-1 size-1.5 bg-running" aria-hidden="true"></span>{/if}
        </button>
        {#if on}
          <!-- The repo's views, under the repo in view: Worktrees, and the
               Azure DevOps ones where it has that remote. -->
          {#each [...REPO_VIEWS, ...(app.adoCtx[r.project.id] ? ADO_VIEWS : [])] as v (v.view)}
            {@const cur = nav.current.view === v.view}
            <button
              class="{tile} text-[11px] {cur ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
              onclick={() => nav.go({ view: v.view, repo: r.project.id })}
              title="{r.name}: {v.label}"
              aria-label="{r.name} {v.label}"
              aria-current={cur ? 'page' : undefined}
            >
              <span aria-hidden="true">{v.view === 'worktrees' ? '\u21b3' : v.view === 'prs' ? 'PR' : v.label.slice(0, 2)}</span>
            </button>
          {/each}
        {/if}
      {/each}
    </nav>
    <div class="flex flex-none flex-col gap-1 border-t border-rule py-2">
      <!-- The rail's Accounts section has no room here: its view does. -->
      {#if studio}
        <button
          class="{tile} text-[11px] {inStudio ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
          data-rail="studio"
          onclick={() => openStudio(nav.current.repo)}
          title={STUDIO_TITLE}
          aria-label="Studio"
          aria-current={inStudio ? 'page' : undefined}
        >
          <span aria-hidden="true">St</span>
        </button>
        <button
          class="{tile} text-[11px] {inGauntlet ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
          data-rail="gauntlet"
          onclick={() => openGauntlet(nav.current.repo)}
          title={GAUNTLET_TITLE}
          aria-label="Gauntlet"
          aria-current={inGauntlet ? 'page' : undefined}
        >
          <span aria-hidden="true">Ga</span>
        </button>
      {/if}
      <button
        class="{tile} text-[11px] {nav.current.view === 'accounts' ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
        onclick={openAccounts}
        title={withKey('Accounts: every login, its use and its sessions', 'accounts')}
        aria-label="Accounts"
        aria-current={nav.current.view === 'accounts' ? 'page' : undefined}
      >
        <span aria-hidden="true">Ac</span>
      </button>
      <button
        class="{tile} {inWake ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
        onclick={() => openWake()}
        title={WAKE_TITLE}
        aria-label="Whakaoho, wake-up"
        aria-current={inWake ? 'page' : undefined}
      >
        {@render sunrise()}
      </button>
      <button
        class="{tile} {inSettings ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
        onclick={() => openSettings()}
        title={withKey('Settings', 'settings')}
        aria-label="Settings"
        aria-current={inSettings ? 'page' : undefined}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M6.7 1.5h2.6l.4 1.9 1.3.6 1.7-1 1.8 1.8-1 1.7.6 1.3 1.9.4v2.6l-1.9.4-.6 1.3 1 1.7-1.8 1.8-1.7-1-1.3.6-.4 1.9H6.7l-.4-1.9-1.3-.6-1.7 1-1.8-1.8 1-1.7-.6-1.3-1.9-.4V6.7l1.9-.4.6-1.3-1-1.7 1.8-1.8 1.7 1 1.3-.6z"
            fill="none"
            stroke="currentColor"
            stroke-width="1.2"
          />
          <rect x="6" y="6" width="4" height="4" fill="none" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
      <button
        class="{tile} bg-transparent text-muted hover:text-ink"
        onclick={cycleTheme}
        title={withKey(themeTip, 'cycle_theme')}
        aria-label="Theme: {themeWord}"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
          <rect x="2.5" y="2.5" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.3" />
          {#if app.themePref === 'system'}
            <path d="M8 2.5v11h5.5v-11z" fill="currentColor" />
          {:else if app.dark}
            <rect x="2.5" y="2.5" width="11" height="11" fill="currentColor" />
          {/if}
        </svg>
      </button>
    </div>
  {:else}
  <div class="flex h-10 flex-none items-center gap-2 border-b border-rule px-4">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <svg class="flex-none text-ink" width="20" height="20" viewBox="0 0 64 64" aria-hidden="true" onclick={markClick}>
      <path d="M11 31h42l-6.5 26h-29z" fill="currentColor" />
      <path d="M21 31V22a11 11 0 0 1 22 0a5.5 5.5 0 0 1 -11 0" fill="none" stroke="currentColor" stroke-width="6" />
      <circle class="fill-ember" cx="32" cy="22" r="3.2" />
    </svg>
    <span class="text-[15px] font-semibold tracking-tight text-ink">kete</span>
    <span
      class="ml-auto inline-flex items-center gap-1.5 font-mono text-[12px] {app.model.connected ? 'text-muted' : 'text-fail'}"
      title={app.model.connected ? `daemon pid ${app.model.daemon_pid}` : 'Not connected to the daemon'}
    >
      <span class="size-1.5 {app.model.connected ? 'bg-done' : 'bg-fail'}"></span>{app.model.connected ? 'daemon' : 'offline'}
    </span>
  </div>

  <nav class="min-h-0 flex-1 overflow-y-auto pb-4" aria-label="Repos">
    <!-- The orchestrator, pinned above the repos: the place most work starts. -->
    <div class="border-b border-rule py-1.5">
      {#if orch}
        {@const onOrch = nav.current.view === 'orchestrator'}
        <button
          class="{item} {onOrch ? 'bg-surface font-semibold text-ink' : 'bg-transparent text-ink'}"
          data-target="orchestrator:rail"
          onclick={openOrchestrator}
          title="The orchestrator, {orch.name} (0 or F10); right-click for its menu"
          aria-current={onOrch ? 'page' : undefined}
        >
          <span class="flex-none font-mono text-[12px]" aria-hidden="true">&#9670;</span>
          <span class="min-w-0 flex-1 truncate">Orchestrator</span>
          {#if orchNeeds}
            <span class="inline-flex flex-none items-center gap-1.5 text-[12px] text-ember-ink"
              ><span class="size-1.5 bg-ember" aria-hidden="true"></span><span class="font-bold">{orchNeeds}</span> {orchNeeds === 1 ? 'needs you' : 'need you'}</span
            >
          {:else}
            <Status tone={toneOf(orch)} class="font-normal" />
          {/if}
        </button>
      {:else}
        <button
          class="{item} bg-transparent text-[13px] text-muted hover:text-ink"
          data-target="orchestrator:rail"
          onclick={() => (canProvision(app.model) ? openOrchestrator() : askStartOrchestrator())}
          title="Start an orchestrator: one session that coordinates the others (0 or F10)"
        >
          <span class="flex-none font-mono text-[12px]" aria-hidden="true">&#9671;</span>
          <span class="min-w-0 flex-1 truncate">Start orchestrator</span>
        </button>
      {/if}
    </div>
    <h2 class="m-0 {heading}">Repos</h2>
    {#each projects as r, i (r.project.id)}
      {@const c = tally([r])}
      {@const needs = c.needs?.length ?? 0}
      {@const running = c.running?.length ?? 0}
      {@const on = here === r.project.id}
      {@const ctx = app.adoCtx[r.project.id]}
      <button
        class="{item} relative {on ? 'bg-surface font-semibold text-ink' : 'bg-transparent text-ink'} {mark?.id === r.project.id && mark.zone === 'onto' && drag.item ? 'outline-2 -outline-offset-2 outline-ember' : ''}"
        data-target="project:{r.project.id}"
        {...dnd(r.project.id)}
        onclick={() => revealRepo(r.project.id)}
        title="{withKey(r.project.repo_path, digit(i))}. {reorderHint}; drop a session to move it to the root checkout"
        aria-current={on ? 'true' : undefined}
      >
        {#if mark?.id === r.project.id && mark.zone !== 'onto' && drag.item}
          <span class="pointer-events-none absolute inset-x-0 z-10 h-px bg-ember {mark.zone === 'before' ? '-top-px' : '-bottom-px'}" aria-hidden="true"></span>
        {/if}
        <span class="min-w-0 flex-1 truncate">{r.name}</span>
        {#if needs}
          <span class="inline-flex flex-none items-center gap-1.5 text-[12px] text-ember-ink"
            ><span class="size-1.5 bg-ember" aria-hidden="true"></span><span class="font-bold">{needs}</span> {needs === 1 ? 'needs you' : 'need you'}</span
          >
        {:else if running}
          <span class="inline-flex flex-none items-center gap-1.5 text-[12px] font-normal text-muted"
            ><span class="size-1.5 bg-running" aria-hidden="true"></span>{running} running</span
          >
        {/if}
      </button>
      <ul class="m-0 list-none p-0" aria-label="{r.name} views">
        {#each [...REPO_VIEWS, ...(ctx ? ADO_VIEWS : [])] as v (v.view)}
          {@const cur = nav.current.view === v.view && here === r.project.id}
          <li>
            <button
              class="{item} pl-8 text-[13px] {cur ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
              onclick={() => nav.go({ view: v.view, repo: r.project.id })}
              title={v.view === 'worktrees' ? `${r.name}: every checkout, its git state and sessions` : undefined}
              aria-current={cur ? 'page' : undefined}>{v.label}</button
            >
          </li>
        {/each}
      </ul>
    {/each}
    {#if !projects.length}
      <p class="m-0 px-4 py-1.5 text-[13px] text-muted">No repos yet.</p>
    {/if}
    <div class="px-4 pt-2">
      {#if adding}
        <form onsubmit={addProject}>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="w-full border border-rule-strong bg-bg px-2 py-1 font-mono text-[12px] text-ink outline-none"
            bind:value={addPath}
            placeholder="C:\path\to\repo"
            aria-label="Repo folder"
            autofocus
            onkeydown={(e) => e.key === 'Escape' && (adding = false)}
          />
        </form>
      {:else}
        <button
          class="border-0 bg-transparent p-0 text-[13px] text-muted enabled:hover:text-ink enabled:hover:underline enabled:hover:underline-offset-4 disabled:text-faint"
          onclick={() => (adding = true)}
          disabled={!app.model.connected}>Add a repo</button
        >
      {/if}
    </div>

    <AccountsSection {heading} />
    <!-- Whakaoho sits with the accounts it wakes. -->
    <div class="pt-1">
      <button
        class="{item} text-[13px] {inWake ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
        onclick={() => openWake()}
        title={WAKE_TITLE}
        aria-current={inWake ? 'page' : undefined}
      >
        {@render sunrise()}
        <span class="min-w-0 flex-1 truncate">Whakaoho</span>
      </button>
      {#if studio}
        <button
          class="{item} text-[13px] {inStudio ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
          data-rail="studio"
          onclick={() => openStudio(nav.current.repo)}
          title={STUDIO_TITLE}
          aria-current={inStudio ? 'page' : undefined}
        >
          <span class="min-w-0 flex-1 truncate">Studio</span>
        </button>
        <button
          class="{item} text-[13px] {inGauntlet ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
          data-rail="gauntlet"
          onclick={() => openGauntlet(nav.current.repo)}
          title={GAUNTLET_TITLE}
          aria-current={inGauntlet ? 'page' : undefined}
        >
          <span class="min-w-0 flex-1 truncate">Gauntlet</span>
        </button>
      {/if}
    </div>
  </nav>

  <VersionPlate />

  <div class="flex h-10 flex-none items-center border-t border-rule px-2">
    <button
      class="inline-flex h-7 items-center gap-2 border-0 px-2 text-[13px] hover:bg-surface hover:text-ink {inSettings
        ? 'bg-surface text-ink'
        : 'bg-transparent text-muted'}"
      onclick={() => openSettings()}
      title={withKey('Settings', 'settings')}
      aria-current={inSettings ? 'page' : undefined}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M6.7 1.5h2.6l.4 1.9 1.3.6 1.7-1 1.8 1.8-1 1.7.6 1.3 1.9.4v2.6l-1.9.4-.6 1.3 1 1.7-1.8 1.8-1.7-1-1.3.6-.4 1.9H6.7l-.4-1.9-1.3-.6-1.7 1-1.8-1.8 1-1.7-.6-1.3-1.9-.4V6.7l1.9-.4.6-1.3-1-1.7 1.8-1.8 1.7 1 1.3-.6z"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
          stroke-linejoin="miter"
        />
        <rect x="6" y="6" width="4" height="4" fill="none" stroke="currentColor" stroke-width="1.2" />
      </svg>
      Settings
    </button>
    <span class="flex-1"></span>
    <button
      class="inline-flex h-7 items-center gap-2 border-0 bg-transparent px-2 text-[13px] text-muted hover:bg-surface hover:text-ink"
      onclick={cycleTheme}
      title={withKey(themeTip, 'cycle_theme')}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <rect x="2.5" y="2.5" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.3" />
        {#if app.themePref === 'system'}
          <path d="M8 2.5v11h5.5v-11z" fill="currentColor" />
        {:else if app.dark}
          <rect x="2.5" y="2.5" width="11" height="11" fill="currentColor" />
        {/if}
      </svg>
      <span class="min-w-0 truncate">Theme: {themeWord}</span>
    </button>
  </div>
  {/if}
</aside>
