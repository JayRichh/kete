<script lang="ts">
  import { app, openSettings, revealRepo, setThemePref, toast, type ThemePref } from '../store.svelte';
  import { api } from '../api';
  import { nav, type View } from './history.svelte';
  import { repoRows, tally } from '../overview/repos';
  import AccountsSection from '../accounts/AccountsSection.svelte';

  const rows = $derived(repoRows(app.model));
  /** Projects in the user's own order: the rail is a map, not a ranking. */
  const projects = $derived([...rows].sort((a, b) => a.project.sort_order - b.project.sort_order));
  const here = $derived(nav.current.repo);
  const inSettings = $derived(nav.current.view === 'settings');
  let adding = $state(false);
  let addPath = $state('');

  const ADO_VIEWS: { view: View; label: string }[] = [
    { view: 'tasks', label: 'Tasks' },
    { view: 'branches', label: 'Branches' },
    { view: 'prs', label: 'Pull requests' },
  ];

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
    const order: ThemePref[] = ['system', 'light', 'dark'];
    setThemePref(order[(order.indexOf(app.themePref) + 1) % order.length]);
  }
  const THEME_WORD: Record<ThemePref, string> = { system: 'System', light: 'Light', dark: 'Dark' };

  const item =
    'flex w-full min-w-0 items-center gap-2 border-0 px-4 py-1.5 text-left text-[14px] hover:bg-surface focus-visible:-outline-offset-2';
  const heading = 'px-4 pt-5 pb-1.5 text-[12px] font-medium text-muted';
</script>

<aside class="flex min-h-0 flex-col border-r border-rule bg-bg">
  <div class="flex h-10 flex-none items-center gap-2 border-b border-rule px-4">
    <svg class="flex-none text-ink" width="20" height="20" viewBox="0 0 64 64" aria-hidden="true">
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
    <h2 class="m-0 {heading}">Repos</h2>
    {#each projects as r (r.project.id)}
      {@const c = tally([r])}
      {@const needs = c.needs?.length ?? 0}
      {@const running = c.running?.length ?? 0}
      {@const on = here === r.project.id}
      {@const ctx = app.adoCtx[r.project.id]}
      <button
        class="{item} {on ? 'bg-surface font-semibold text-ink' : 'bg-transparent text-ink'}"
        onclick={() => revealRepo(r.project.id)}
        title={r.project.repo_path}
        aria-current={on ? 'true' : undefined}
      >
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
      {#if ctx}
        <ul class="m-0 list-none p-0" aria-label="{r.name} on Azure DevOps">
          {#each ADO_VIEWS as v (v.view)}
            {@const cur = nav.current.view === v.view && here === r.project.id}
            <li>
              <button
                class="{item} pl-8 text-[13px] {cur ? 'bg-surface text-ink' : 'bg-transparent text-muted hover:text-ink'}"
                onclick={() => nav.go({ view: v.view, repo: r.project.id })}
                aria-current={cur ? 'page' : undefined}>{v.label}</button
              >
            </li>
          {/each}
        </ul>
      {/if}
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
  </nav>

  <div class="flex h-10 flex-none items-center border-t border-rule px-2">
    <button
      class="inline-flex h-7 items-center gap-2 border-0 px-2 text-[13px] hover:bg-surface hover:text-ink {inSettings
        ? 'bg-surface text-ink'
        : 'bg-transparent text-muted'}"
      onclick={() => openSettings()}
      title="Settings (Ctrl+,)"
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
      title="Theme: {THEME_WORD[app.themePref]}. Press to cycle system, light, dark (Ctrl+Shift+L)"
    >
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <rect x="2.5" y="2.5" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.3" />
        {#if app.themePref === 'system'}
          <path d="M8 2.5v11h5.5v-11z" fill="currentColor" />
        {:else if app.themePref === 'dark'}
          <rect x="2.5" y="2.5" width="11" height="11" fill="currentColor" />
        {/if}
      </svg>
      Theme: {THEME_WORD[app.themePref]}
    </button>
  </div>
</aside>
