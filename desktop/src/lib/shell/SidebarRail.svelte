<script lang="ts">
  // The sidebar folded: a 44 px column of section icons, each with a
  // tooltip. A click opens the sidebar again and goes to the section.
  import { app, openGauntlet, openSettings, openStudio, setHome } from '../store.svelte';
  import { showStudioRail } from '../studio/types';
  import { nav } from './history.svelte';
  import { canProvision, descendantCounts, orchestratorOf, orchestratorTree } from '../orchestrator';
  import { askStartOrchestrator, openOrchestrator } from '../orch.svelte';
  import { openAccounts } from '../accounts/actions';
  import { openWake } from '../wake/state.svelte';
  import { withKey } from '../shortcuts';
  import { setSidebarCollapsed } from './rail-state.svelte';

  const orch = $derived(orchestratorOf(app.model));
  const needs = $derived(descendantCounts(orchestratorTree(app.model)).needs ?? 0);
  const view = $derived(nav.current.view);
  const studio = $derived(showStudioRail(app.studioRepos));

  /** Open the sidebar again, then go. */
  function go(then: () => void) {
    setSidebarCollapsed(false);
    then();
  }

  const tile =
    'relative mx-auto grid size-9 place-items-center border border-transparent p-0 hover:border-rule focus-visible:-outline-offset-2';
  const on = (active: boolean) => (active ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink');
  const WAKE_TITLE = `Whakaoho ${String.fromCharCode(0xb7)} Wake-up: start each account's 5-hour window earlier`;
</script>

<aside class="flex h-full min-h-0 w-full flex-col border-r border-rule bg-bg" aria-label="Sidebar, folded" data-sidebar-rail>
  <div class="flex h-10 flex-none items-center justify-center border-b border-rule">
    <button
      class="grid size-7 place-items-center border-0 bg-transparent p-0 text-muted hover:bg-surface hover:text-ink"
      title={withKey('Open the sidebar', 'toggle_sidebar')}
      aria-label="Open the sidebar"
      data-sidebar-expand
      onclick={() => setSidebarCollapsed(false)}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <path d="M2.5 2.5h11v11h-11zM6.5 2.5v11M9.5 6l2 2-2 2" fill="none" stroke="currentColor" stroke-width="1.3" />
      </svg>
    </button>
  </div>
  <nav class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto py-2" aria-label="Sections">
    <button
      class="{tile} {on(view === 'orchestrator')}"
      data-rail="orchestrator"
      title={orch ? `The orchestrator, ${orch.name}${needs ? `: ${needs} need you` : ''}` : 'Start an orchestrator'}
      aria-label="Orchestrator"
      onclick={() => go(() => (orch || canProvision(app.model) ? openOrchestrator() : askStartOrchestrator()))}
    >
      <span class="font-mono text-[12px]" aria-hidden="true">{orch ? '\u25c6' : '\u25c7'}</span>
      {#if needs}<span class="absolute top-1 right-1 size-1.5 bg-ember" aria-hidden="true"></span>{/if}
    </button>
    <button
      class="{tile} {on(view === 'overview' || view === 'board')}"
      data-rail="repos"
      title="Repos: the overview of every repo and session"
      aria-label="Repos"
      onclick={() => go(() => setHome(app.home, nav.current.repo))}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <path d="M1.5 3.5h4.5l1.5 1.5h7v8.5h-13zM1.5 7h13" fill="none" stroke="currentColor" stroke-width="1.3" />
      </svg>
    </button>
    <button
      class="{tile} {on(view === 'accounts')}"
      data-rail="accounts"
      title={withKey('Accounts: every login, its use and its sessions', 'accounts')}
      aria-label="Accounts"
      onclick={() => go(openAccounts)}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <path d="M8 2.5a2.7 2.7 0 1 0 0 5.4 2.7 2.7 0 0 0 0-5.4zM2.5 13.5c0-3 2.5-4.6 5.5-4.6s5.5 1.6 5.5 4.6z" fill="none" stroke="currentColor" stroke-width="1.3" />
      </svg>
    </button>
    <button
      class="{tile} {on(view === 'whakaoho')}"
      data-rail="whakaoho"
      title={WAKE_TITLE}
      aria-label="Whakaoho, wake-up"
      onclick={() => go(() => openWake())}
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <path d="M5.2 10.5a2.8 2.8 0 0 1 5.6 0M8 4.7v1.8M3.9 6.4l1.3 1.3M12.1 6.4l-1.3 1.3" fill="none" stroke="currentColor" stroke-width="1.2" />
        <path d="M3.5 10.5h9l-1.4 4H4.9zM4.2 12.5h7.6" fill="none" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>
    {#if studio}
      <button
        class="{tile} {on(view === 'studio')}"
        data-rail="studio"
        title="Studio: the board and the idle-note queue of the repos with studio mode"
        aria-label="Studio"
        onclick={() => go(() => openStudio(nav.current.repo))}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M2.5 2.5h3.2v11H2.5zM6.4 2.5h3.2v7H6.4zM10.3 2.5h3.2v9h-3.2z" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
      <button
        class="{tile} {on(view === 'gauntlet')}"
        data-rail="gauntlet"
        title="Gauntlet: the sign-offs and verdicts of the repos with studio mode"
        aria-label="Gauntlet"
        onclick={() => go(() => openGauntlet(nav.current.repo))}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M2.5 8.5l3.2 3.2 7.8-7.8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="square" />
        </svg>
      </button>
    {/if}
  </nav>
  <div class="flex flex-none flex-col gap-1 border-t border-rule py-2">
    <button
      class="{tile} {on(view === 'settings')}"
      data-rail="settings"
      title={withKey('Settings', 'settings')}
      aria-label="Settings"
      onclick={() => go(() => openSettings())}
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
  </div>
</aside>
