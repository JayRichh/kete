<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { app, boot, checkAdoStore, focusQuickPrompt, follow, loadDesktopKeys, loadDesktopPrefs, openDiff, openSession, sessionWorktree, setHome } from './lib/store.svelte';
  import { shortcutFor, withKey } from './lib/shortcuts';
  import { currentRepo, focusedTarget, runShortcut, toggleArchived, toggleDiff } from './lib/commands';
  import { layerOpen, ui } from './lib/ui/ui.svelte';
  import Icon from './lib/ui/Icon.svelte';
  import { LAYER_SEL, focusBack } from './lib/ui/focus';
  import Menu from './lib/ui/Menu.svelte';
  import Dialog from './lib/ui/Dialog.svelte';
  import Palette from './lib/Palette.svelte';
  import Help from './lib/Help.svelte';
  import { api } from './lib/api';
  import { isDiffMode } from './lib/diff-api';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { nav } from './lib/shell/history.svelte';
  import Sidebar from './lib/shell/Sidebar.svelte';
  import { loadSidebar } from './lib/shell/rail-state.svelte';
  import Breadcrumb from './lib/shell/Breadcrumb.svelte';
  import UsageMeter from './lib/accounts/UsageMeter.svelte';
  import UsageBanner from './lib/accounts/UsageBanner.svelte';
  import { headline } from './lib/accounts/usage';
  import ScopeChip from './lib/shell/ScopeChip.svelte';
  import ShipSheet from './lib/ship/ShipSheet.svelte';
  import AttentionStrip from './lib/overview/AttentionStrip.svelte';
  import Overview from './lib/overview/Overview.svelte';
  import Board from './lib/Board.svelte';
  import Session from './lib/Session.svelte';
  import Diff from './lib/Diff.svelte';
  import QuickPrompt from './lib/QuickPrompt.svelte';
  import Tasks from './lib/ado/Tasks.svelte';
  import Branches from './lib/ado/Branches.svelte';
  import Prs from './lib/ado/Prs.svelte';
  import Settings from './lib/settings/Settings.svelte';
  import OrchestratorPanel from './lib/orchestrator/OrchestratorPanel.svelte';
  import StartOrchestrator from './lib/orchestrator/StartOrchestrator.svelte';
  import ReviewSheet from './lib/orchestrator/ReviewSheet.svelte';
  import BranchSwitch from './lib/worktrees/BranchSwitch.svelte';
  import ReviewNotices from './lib/orchestrator/ReviewNotices.svelte';
  import { onReviewOpened, openOrchestrator } from './lib/orch.svelte';
  import { opensOnOrchestrator, orchestratorOf, placeKey } from './lib/orchestrator';
  import FileSheet from './lib/files/FileSheet.svelte';
  import FilesView from './lib/files/FilesView.svelte';
  import FilesToggle from './lib/files/FilesToggle.svelte';
  import AccountsView from './lib/accounts/AccountsView.svelte';
  import WakeView from './lib/wake/WakeView.svelte';
  import AddAccount from './lib/accounts/AddAccount.svelte';
  import WorktreesView from './lib/worktrees/WorktreesView.svelte';
  import { loadCollapsed } from './lib/worktrees/rows.svelte';
  import { listenAlerts } from './lib/alerts';
  import { noteFocus } from './lib/selection.svelte';
  import { installMenus } from './lib/menus/builtin';
  import { startEditors } from './lib/editors/store.svelte';
  import { installEditorMenus } from './lib/editors/menus';
  import VerbBar from './lib/VerbBar.svelte';
  import TipBar from './lib/TipBar.svelte';
  import WhatsNew from './lib/WhatsNew.svelte';
  import { loadLearn, offerTip } from './lib/learn.svelte';
  import { selection } from './lib/selection.svelte';
  import { isEverything } from './lib/scope';
  import Critters from './lib/critters/Critters.svelte';
  import CritterLog from './lib/critters/CritterLog.svelte';
  import { look, paint, refreshLive } from './lib/look.svelte';
  import { knownTheme, zoomOf } from './lib/look';

  const media = window.matchMedia('(prefers-color-scheme: dark)');
  let osDark = $state(media.matches);
  media.addEventListener('change', (e) => (osDark = e.matches));

  // The look: Workshop's blocks, One's, or an inline palette (VS Code's,
  // an imported one), and the fonts, on <html>; then the terminals and
  // the diff editor re-read it (`look.seq`).
  $effect(() => {
    app.dark = paint(app.themePref, osDark);
    void look.prefs;
    void look.live;
    untrack(() => look.seq++);
  });
  // The native title bar follows the theme too, not only the OS: the OS
  // itself while Workshop follows it, else the theme's own light or dark.
  $effect(() => {
    const follow = app.themePref === 'system';
    const dark = app.dark;
    getCurrentWindow()
      .setTheme(follow ? null : dark ? 'dark' : 'light')
      .catch(() => {});
  });
  // `vscode` follows VS Code: its settings.json is stat'ed every few
  // seconds and whenever the window takes focus, and read again when the
  // OS flips light or dark (VS Code's auto-detect follows that).
  $effect(() => {
    if (app.themePref !== 'vscode') return;
    const osLight = !osDark;
    void refreshLive(osLight, true);
    const beat = setInterval(() => void refreshLive(osLight), 2500);
    const onFocus = () => void refreshLive(osLight);
    window.addEventListener('focus', onFocus);
    return () => {
      clearInterval(beat);
      window.removeEventListener('focus', onFocus);
    };
  });
  // The Interface size scales the whole window (every size in it is px).
  $effect(() => {
    const scale = zoomOf(look.prefs.sizes);
    api.lookZoom(scale).catch(() => {});
  });

  // Back and forward land on a place; the selection follows it.
  $effect(() => follow(nav.current));

  // LEARN IN PLACE: a concept's first-use tip the first time it is on
  // screen (the board offers its lanes and drag). Each effect reads the
  // tip state too, so one that waited on another tip tries again.
  $effect(() => {
    if (app.review) offerTip('review');
  });
  $effect(() => {
    if (!isEverything(app.model.scope) || (app.home === 'board' && app.model.projects.length > 1)) offerTip('scope');
  });
  $effect(() => {
    if (selection.target) offerTip('verbs');
  });

  const view = $derived(nav.current.view);
  const usageHead = $derived(headline(app.model.accounts, app.now));

  // A narrow window (900px and under): the rail goes to icons, and a split
  // shows one side at a time, the session or diff pane unless the list is
  // asked for (the header's List button). Opening anything puts the pane
  // back.
  const narrowQuery = window.matchMedia('(max-width: 900px)');
  let narrow = $state(narrowQuery.matches);
  narrowQuery.addEventListener('change', (e) => (narrow = e.matches));
  let listShown = $state(false);
  $effect(() => {
    placeKey(nav.current);
    untrack(() => (listShown = false));
  });
  const orch = $derived(orchestratorOf(app.model));
  // The orchestrator view is a split too: its tree on the left, its own
  // session in the shared pane on the right (one pane per session, so the
  // terminal is never attached twice).
  const orchView = $derived(view === 'orchestrator');
  const split = $derived(view === 'session' || view === 'diff' || view === 'files' || (orchView && !!orch));
  // Full pane (Z, Ctrl+Shift+M): the right side takes the window. Leaving the
  // split brings it back down, so the next split opens beside the list.
  const full = $derived(split && app.paneMax);
  $effect(() => {
    if (!split) app.paneMax = false;
  });
  // The orchestrator view keeps the chat primary: a wider right side.
  const rightWidth = $derived(full ? 'auto' : app.splitWidth ? `${app.splitWidth}px` : orchView ? '62%' : '56%');
  // An orchestrator started (or pinned) while its view is up: show it.
  $effect(() => {
    if (orchView && orch && !(app.session && 'Agent' in app.session && app.session.Agent === orch.id)) app.session = { Agent: orch.id };
  });
  const adoView = $derived(view === 'tasks' || view === 'branches' || view === 'prs');
  const ADO_TITLE: Record<string, string> = { tasks: 'Tasks', branches: 'Branches', prs: 'Pull requests' };

  // Each ADO view, as it opens for a repo, asks whether ado.json failed to
  // save. Keyed on view and repo, so picking a row inside does not ask.
  const adoOpen = $derived(adoView ? `${view}\u0000${nav.current.repo ?? ''}` : '');
  $effect(() => {
    if (adoOpen) checkAdoStore();
  });

  // Session and Diff stay mounted once opened, so a terminal keeps its
  // scrollback and the diff editor its place while you look elsewhere.
  let sessionEverOpened = $state(false);
  let diffEverOpened = $state(false);
  $effect(() => {
    if (view === 'session' || (orchView && orch)) sessionEverOpened = true;
    if (view === 'diff') diffEverOpened = true;
  });

  let body: HTMLDivElement;
  function startResize(e: PointerEvent) {
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const box = body.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      app.splitWidth = Math.round(Math.max(380, Math.min(box.width - 360, box.right - ev.clientX)));
    };
    const up = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }
  function resizeKey(e: KeyboardEvent) {
    const w = app.splitWidth || Math.round(body.getBoundingClientRect().width * 0.56);
    if (e.key === 'ArrowLeft') app.splitWidth = w + 40;
    else if (e.key === 'ArrowRight') app.splitWidth = Math.max(380, w - 40);
  }

  /** A button or link of its own inside a row or card: not the row (a
   *  `data-target`) and not a board card's own face (`data-card-sel`). */
  function ownControl(t: Element): boolean {
    const c = t.closest('button, a[href], [role="button"], summary');
    return !!c && !c.matches('[data-target], [data-card-sel]') && !!c.closest('[data-target]');
  }

  // Every key goes through the one shortcut table (lib/shortcuts.ts),
  // which the help overlay lists too.
  function onKey(e: KeyboardEvent) {
    if (layerOpen()) return;
    const t = e.target instanceof Element ? e.target : document.body;
    const inTerminal = !!t.closest('.xterm');
    const typing = inTerminal || !!t.closest('input, textarea, select, .monaco-editor');
    // Space on a button inside a row (a card's Archive, a tree row's
    // toggle) presses that button: the row's own Space (the follow-up)
    // is for the row itself.
    if (e.key === ' ' && ownControl(t)) return;
    const s = shortcutFor(e, { inTerminal, inEditor: !!t.closest('.monaco-editor'), typing, onRow: !!focusedTarget() });
    if (s && runShortcut(s.id)) e.preventDefault();
  }

  // The focus outside the layers, given back when the last one closes
  // (ui/focus.ts), so Esc out of a menu leaves its row keys working.
  let lastOutside: HTMLElement | null = null;
  function onFocusIn(e: FocusEvent) {
    if (e.target instanceof HTMLElement && !e.target.closest(LAYER_SEL)) {
      lastOutside = e.target;
      // The row the verb bar, `?` and the palette act on.
      noteFocus(e.target);
    }
  }
  let layered = false;
  $effect(() => {
    if (layerOpen()) layered = true;
    else if (layered) {
      layered = false;
      tick().then(() => focusBack(lastOutside, document.activeElement, document.body)?.focus());
    }
  });

  /** The mouse's back and forward buttons. */
  function onMouseUp(e: MouseEvent) {
    if (e.button === 3) (e.preventDefault(), nav.back());
    else if (e.button === 4) (e.preventDefault(), nav.forward());
  }

  /** Command-line `--view`, `--session`, `--theme`, `--diff`. */
  async function applyStartup() {
    // Saved preferences first; the command line overrides them for this
    // run only and is never written back.
    loadDesktopKeys();
    const prefs = await loadDesktopPrefs();
    loadCollapsed(prefs?.collapsed);
    loadSidebar(prefs);
    const s = await api.startup().catch(() => null);
    // Flags that need no daemon apply at once.
    if (s?.theme && knownTheme(s.theme, look.prefs.themes)) app.themePref = s.theme;
    if (isDiffMode(s?.diff_mode)) app.diffMode = s.diff_mode;
    if (s?.view === 'board') {
      app.home = 'board';
      nav.replace({ view: 'board' });
    }
    // The rest waits for the daemon, then moves the window only if the
    // user has not gone somewhere in the meantime.
    const orchestrator = opensOnOrchestrator(prefs?.startup, s);
    const session = !orchestrator && !!s && (!!s.session || s.view === 'session' || s.view === 'diff');
    if (!orchestrator && !session) return;
    const before = placeKey(nav.current);
    for (let i = 0; i < 100 && !app.model.connected; i++) await new Promise((r) => setTimeout(r, 100));
    if (placeKey(nav.current) !== before) return;
    // Open on the orchestrator (Desktop tab) unless the command line picks
    // a view or session for this run.
    if (orchestrator) return openOrchestrator();
    if (s && session) {
      // Let the daemon's snapshot settle before resolving names.
      await new Promise((r) => setTimeout(r, 300));
      if (placeKey(nav.current) !== before) return;
      const m = app.model;
      const want = s.session?.toLowerCase();
      const agent = want ? m.agents.find((a) => a.id.toLowerCase() === want || a.name.toLowerCase() === want) : undefined;
      const term = want && !agent ? m.terminals.find((t) => t.id.toLowerCase() === want || t.name.toLowerCase() === want) : undefined;
      if (agent) openSession({ Agent: agent.id });
      else if (term) openSession({ Terminal: term.id });
    }
    if (s?.view === 'diff') openDiff(sessionWorktree(app.session));
  }

  onMount(() => {
    // Every right-click: Kete's menu for what is under it, never WebView2's.
    const stopMenus = installMenus();
    // VS Code and Visual Studio: found once, followed per checkout.
    installEditorMenus();
    const stopEditors = startEditors();
    api.onReview(onReviewOpened).catch(() => {});
    listenAlerts().catch(() => {});
    boot();
    applyStartup();
    loadLearn();
    return () => {
      stopMenus();
      stopEditors();
    };
  });
</script>

<svelte:window onkeydown={onKey} onmouseup={onMouseUp} onfocusin={onFocusIn} />

<div class="grid h-full grid-cols-[auto_minmax(0,1fr)]">
  <Sidebar {narrow} />
  <main class="relative flex min-h-0 min-w-0 flex-col">
    <header class="@container flex h-10 flex-none items-center border-b border-rule pl-2 {narrow ? 'gap-3 pr-2' : 'gap-3 pr-4'}">
      <Breadcrumb />
      <span class="flex-1"></span>
      {#if narrow && split}
        <button
          class="h-6 flex-none border px-2 text-[12px] {listShown ? 'border-ink bg-ink text-bg' : 'border-rule bg-transparent text-muted hover:text-ink'}"
          aria-pressed={listShown}
          title={listShown ? 'Back to the open pane' : 'Show the list: the window is too narrow for the list and the pane side by side'}
          onclick={() => (listShown = !listShown)}>{listShown ? 'Pane' : 'List'}</button
        >
      {/if}
      <UsageMeter window={usageHead?.window} />
      <ScopeChip compact={narrow} />
      {#if !narrow}<AttentionStrip tight={split} />{/if}
      {#if app.home === 'overview' && !adoView && view !== 'settings' && view !== 'worktrees' && view !== 'accounts' && view !== 'whakaoho' && !orchView}
        <button
          class="h-6 flex-none border px-2 text-[12px] {app.showArchived ? 'border-ink bg-ink text-bg' : 'border-rule bg-transparent text-muted hover:text-ink'}"
          aria-pressed={app.showArchived}
          title={withKey('Show archived sessions in the overview', 'toggle_archived')}
          onclick={toggleArchived}>Archived</button
        >
      {/if}
      <!-- Diff and the home toggle in every view: the diff for the checkout
           in context (pressed: back to where it was opened), and Overview |
           Board, which keeps an open session or diff in the split. -->
      <button
        class="inline-flex h-6 flex-none items-center gap-1.5 border px-2 text-[12px] {view === 'diff'
          ? 'border-ink bg-ink text-bg'
          : 'border-rule bg-transparent text-muted hover:text-ink'}"
        aria-pressed={view === 'diff'}
        title={withKey(view === 'diff' ? 'Close the diff: back to where it was opened' : 'Diff the checkout in context: the focused row, the session, the repo', ['diff', 'open_diff'])}
        onclick={toggleDiff}><Icon name="diff" size={12} /><span class="hidden @min-[700px]:inline">Diff</span></button
      >
      <FilesToggle />
      <div class="flex h-6 flex-none border border-rule" role="radiogroup" aria-label="Home view">
        {#each [{ id: 'overview', label: 'Overview' }, { id: 'board', label: 'Board' }] as h, i (h.id)}
          {@const on = app.home === h.id}
          <button
            role="radio"
            aria-checked={on}
            class="border-0 px-2 text-[12px] {i ? 'border-l border-rule' : ''} {on ? 'bg-ink text-bg' : 'bg-transparent text-muted hover:text-ink'}"
            title={withKey(h.label, h.id === 'board' ? ['home_board', 'toggle_home'] : ['home_overview', 'toggle_home'])}
            onclick={() => {
              setHome(h.id as 'overview' | 'board', nav.current.repo);
              // A narrow window shows the list it was asked for.
              if (narrow && split) listShown = true;
            }}>{h.label}</button
          >
        {/each}
      </div>
      <div class="flex flex-none items-center gap-1">
        <!-- The palette's button gives way first in a narrow window (Ctrl+K stays). -->
        <button
          class="hidden size-7 place-items-center border-0 bg-transparent text-muted hover:bg-surface hover:text-ink @min-[760px]:grid"
          title={withKey('Command palette: jump anywhere, run any action', ['palette', 'palette_alt', 'find'])}
          aria-label="Command palette"
          onclick={() => ((ui.help = false), (ui.palette = true))}><Icon name="palette" /></button
        >
        <button
          class="grid size-7 place-items-center border-0 bg-transparent text-muted hover:bg-surface hover:text-ink"
          title={withKey('Keyboard shortcuts', ['help', 'help_f1'])}
          aria-label="Keyboard shortcuts"
          onclick={() => ((ui.palette = false), (ui.help = true))}><Icon name="help" /></button
        >
      </div>
      <!-- The one primary verb, always in view: the quick prompt, on the
           repo in view (or the open session's). -->
      <button
        class="inline-flex h-7 flex-none items-center gap-1.5 border border-ink bg-ink px-2.5 text-[13px] font-medium whitespace-nowrap text-bg hover:bg-transparent hover:text-ink"
        title={withKey('New session: the quick prompt, on this repo', ['quick_prompt', 'quick_prompt_alt'])}
        aria-label="New session"
        onclick={() => {
          const repo = currentRepo();
          focusQuickPrompt(repo ? { project: repo } : undefined);
        }}><Icon name="plus" size={12} /><span class="hidden @min-[1000px]:inline">New session</span></button
      >
    </header>
    <UsageBanner window={usageHead?.window} />

    <div class="relative flex min-h-0 flex-1" bind:this={body}>
      {#if view === 'tasks' && nav.current.repo}
        {#key nav.current.repo}<Tasks repo={nav.current.repo} />{/key}
      {:else if view === 'branches' && nav.current.repo}
        {#key nav.current.repo}<Branches repo={nav.current.repo} />{/key}
      {:else if view === 'worktrees' && nav.current.repo}
        {#key nav.current.repo}<WorktreesView repo={nav.current.repo} />{/key}
      {:else if view === 'prs'}
        <Prs />
      {:else if view === 'settings'}
        <Settings />
      {:else if view === 'accounts'}
        <AccountsView />
      {:else if view === 'whakaoho'}
        <WakeView />
      {:else if orchView && !orch}
        <StartOrchestrator inline />
      {:else if adoView}
        <section class="min-w-0 flex-1 px-6 pt-6">
          <h1 class="m-0 text-[28px] font-semibold tracking-tight text-ink">{ADO_TITLE[view]}</h1>
          <p class="mt-2 max-w-[60ch] text-[14px] text-muted">
            This view is not in this build yet. The repo's Azure DevOps remote is set up, so it will appear here once it ships.
          </p>
        </section>
      {:else}
        <div class="relative flex min-w-0 flex-1 flex-col {full || (narrow && split && !listShown) ? 'hidden' : ''}">
          <div class="relative min-h-0 flex-1">
            {#if orchView}
              <OrchestratorPanel />
            {:else if app.home === 'board'}
              <Board active={!split || (narrow && listShown)} />
            {:else}
              <Overview active={!split || (narrow && listShown)} />
            {/if}
          </div>
          <!-- The focused row's verbs, with their keys. -->
          {#if !orchView}<VerbBar />{/if}
        </div>
        {#if split && !full && !narrow}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div class="relative z-10 w-px flex-none bg-rule-strong">
            <div
              class="absolute inset-y-0 -left-[3px] w-[7px] cursor-col-resize hover:bg-rule focus-visible:bg-rule"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize the split (arrow keys)"
              tabindex="0"
              onpointerdown={startResize}
              onkeydown={resizeKey}
            ></div>
          </div>
        {/if}
        <div
          class="relative min-w-0 {split && !(narrow && listShown) ? '' : 'hidden'} {full || narrow ? 'flex-1' : 'flex-none'}"
          style={full || narrow ? undefined : `width: ${rightWidth}`}
        >
          {#if sessionEverOpened}
            <div class="absolute inset-0 {view === 'session' || orchView ? 'z-10' : 'hidden'}"><Session /></div>
          {/if}
          {#if diffEverOpened}
            <div class="absolute inset-0 {view === 'diff' ? 'z-10' : 'hidden'}"><Diff /></div>
          {/if}
          {#if view === 'files'}<div class="absolute inset-0 z-10"><FilesView /></div>{/if}
        </div>
      {/if}
    </div>

    <!-- The first-use tip, one line, while one is up. -->
    <TipBar />

    <QuickPrompt />

    {#if !app.model.connected && app.booted && app.model.projects.length}
      <div
        class="absolute bottom-14 left-1/2 flex max-w-[80%] -translate-x-1/2 items-center gap-3 border border-fail bg-bg py-1.5 pr-1.5 pl-3 text-[13px] text-ink"
        role="status"
      >
        Lost the daemon: {app.model.error ?? 'reconnecting'}
        <button class="border-0 bg-fail px-2.5 py-1 font-medium text-bg" onclick={() => api.retry()}>Start it</button>
      </div>
    {/if}
  </main>
</div>

<Menu />
<Dialog />
<Palette />
<Help />
<WhatsNew />
<StartOrchestrator />
<ReviewSheet />
<BranchSwitch />
<ReviewNotices />
<FileSheet />
<AddAccount />
<ShipSheet />
<CritterLog />
<!-- The critters: over the window and the toast, under the scrims. -->
<Critters />

{#if app.toast}
  <div
    class="fixed right-4 bottom-14 z-30 max-w-[460px] border border-rule-strong bg-bg px-3 py-2 text-[13px] text-ink select-text"
    role="alert"
  >
    {app.toast}
  </div>
{/if}
