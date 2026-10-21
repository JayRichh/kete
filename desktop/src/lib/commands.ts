// The window's actions by shortcut id: what a key from the shortcut table
// does, and what the command palette offers besides jumping. App.svelte
// dispatches keys through `runShortcut`; the palette lists `paletteCommands`.

import { api } from './api';
import { nav } from './shell/history.svelte';
import {
  app,
  focusQuickPrompt,
  closeDiff,
  goHome,
  markSeen,
  openDiff,
  openGauntlet,
  openStudio,
  openSession,
  openSettings,
  projectOf,
  revealRepo,
  sessionWorktree,
  setHome,
  setThemePref,
  toast,
  triageAvailable,
} from './store.svelte';
import { nextTheme, themeLabel } from './look';
import { look } from './look.svelte';
import { importVsCodeTheme, lookIsKetes, resetAppearance } from './settings/appearance';
import { keysOf, type ShortcutId } from './shortcuts';
import {
  archiveDone,
  closestTarget,
  isDone,
  localRow,
  markAllSeen,
  menuFor,
  newWorktree,
  openTargetMenu,
  openWakeView,
  parseTarget,
  refreshPrs,
  registryRows,
  runRowKey,
  type MenuOpts,
  type Target,
  type VerbTable,
} from './actions';
import { pickBranchSwitch } from './worktrees/switcher.svelte';
import { ask, openMenu, ui, type MenuEntry, type MenuItem } from './ui/ui.svelte';
import { selection } from './selection.svelte';
import { learn } from './learn.svelte';
import { menuLabel } from './verbs';
import { attentionSessions, stepRing, type PaletteCommand } from './palette';
import { askStartOrchestrator, openOrchestrator, orchestratorCommands } from './orch.svelte';
import { orchestratorOf } from './orchestrator';
import { openAccounts } from './accounts/actions';
import { openWake } from './wake/state.svelte';
import { openAddAccount } from './accounts/add.svelte';
import { focusTriage } from './triage/focus';
import { exportUsage } from './ledger/export';
import { moveStep, rootOf, stepInFor } from './reorder.svelte';
import { railOrder, repoRows } from './overview/repos';
import { inScope, isEverything, scopedRepo, scopedWorktree } from './scope';
import { leafItems } from './menus/model';
import { insertRows, layout } from './menus/layout';
import { exportSettings, importSettings } from './settings/io';
import { bringBack, critters, hiddenNow, hideUntilTomorrow, toggleCritters } from './critters/state.svelte';
import { openCritterLog } from './critters/sheet.svelte';
import { toggleSidebar } from './shell/rail-state.svelte';
import { closeFilesView, openFilesView } from './files/tree.svelte';

/** F10: the orchestrator, or the Start form when there is none. */
function orchestratorKey() {
  if (orchestratorOf(app.model)) openOrchestrator();
  else askStartOrchestrator();
}

export function cycleTheme() {
  setThemePref(nextTheme(app.themePref));
}

export function toggleHome() {
  setHome(app.home === 'overview' ? 'board' : 'overview', nav.current.repo);
}

export function toggleArchived() {
  app.showArchived = !app.showArchived;
  if (app.home !== 'overview' || (nav.current.view !== 'overview' && nav.current.view !== 'board')) setHome('overview', nav.current.repo);
}

/** `.` / `,`: open the next (or previous) session in attention order. */
export function jumpAttention(step: 1 | -1) {
  const s = app.session;
  const next = stepRing(attentionSessions(app.model), s && 'Agent' in s ? s.Agent : null, step);
  if (next) openSession({ Agent: next });
  else toast('No sessions to jump to.');
}

/** The repo the window is on: the one in view, else the scope's, else the
 *  open session's. */
export function currentRepo(): string | undefined {
  return nav.current.repo ?? scopedRepo(app.model) ?? projectOf(sessionWorktree(app.session));
}

/** The session row or card with the focus, if any (past a chip's own
 *  target inside it: the row keys act on the row). */
export function focusedTarget(): Target | null {
  if (typeof document === 'undefined') return null;
  return closestTarget(document.activeElement);
}

/** Still in the model: a remembered row that was deleted is none. */
function live(t: Target | null): Target | null {
  if (!t) return null;
  const m = app.model;
  const there =
    t.kind === 'agent'
      ? m.agents.some((a) => a.id === t.id)
      : t.kind === 'terminal'
        ? m.terminals.some((x) => x.id === t.id)
        : t.kind === 'worktree'
          ? m.worktrees.some((w) => w.id === t.id)
          : m.projects.some((p) => p.id === t.id);
  return there ? t : null;
}

/** The row the keys act on: the focused one, else the one focused last
 *  (a menu, the palette or the verb bar has the focus now). */
export function selectedTarget(): Target | null {
  return focusedTarget() ?? live(selection.target);
}

/** The session the window shows, as a target. */
export function currentTarget(): Target | null {
  const s = app.session;
  if (!s) return null;
  return 'Agent' in s ? { kind: 'agent', id: s.Agent } : { kind: 'terminal', id: s.Terminal };
}

/** The checkout a diff opened from here reads: the focused row's (a
 *  session, card, worktree or repo), the session's in a session view, the
 *  repo's root in its own views (Worktrees, the ADO ones), else the one
 *  last diffed or the open session's. */
export function diffTarget(): string | null {
  const t = focusedTarget();
  const m = app.model;
  const focused =
    t?.kind === 'agent'
      ? m.agents.find((a) => a.id === t.id)?.worktree_id
      : t?.kind === 'terminal'
        ? m.terminals.find((x) => x.id === t.id)?.worktree_id
        : t?.kind === 'worktree'
          ? t.id
          : t?.kind === 'project'
            ? rootOf(t.id)
            : undefined;
  if (focused) return focused;
  const v = nav.current.view;
  if (v === 'files' && nav.current.item) return nav.current.item;
  if (v === 'session' || v === 'orchestrator') return sessionWorktree(app.session) ?? app.diffWorktree;
  const repo = nav.current.repo;
  if (repo && v !== 'overview' && v !== 'board' && v !== 'diff') return rootOf(repo) ?? null;
  // The scope's checkout (or its repo's root) before the last one diffed.
  const scoped = scopedWorktree(app.model);
  if (scoped && v !== 'diff') return scoped;
  const scopeRepo = scopedRepo(app.model);
  return app.diffWorktree ?? sessionWorktree(app.session) ?? (repo ? (rootOf(repo) ?? null) : scopeRepo ? (rootOf(scopeRepo) ?? null) : null);
}

/** D, Ctrl+Shift+D and the header's Diff: the diff for the checkout in
 *  context, or, from the diff, back to where it was opened. A row focused
 *  beside the diff that reads another checkout diffs that one instead. A
 *  board card becomes the session too, as its Diff button makes it. */
export function toggleDiff() {
  const t = focusedTarget();
  const want = diffTarget();
  if (nav.current.view === 'diff' && (!t || want === app.diffWorktree)) return closeDiff();
  if (t?.kind === 'agent' && document.activeElement?.closest('[data-card-sel]')) {
    app.session = { Agent: t.id };
    markSeen(app.session);
  }
  openDiff(want);
}

/** B, Ctrl+Shift+E and the header's Files: the checkout in context as a
 *  tree, or, from the files, back to where they were opened. A row
 *  focused beside them that reads another checkout shows that one. */
export function toggleFiles() {
  const t = focusedTarget();
  const want = diffTarget();
  if (nav.current.view === 'files' && (!t || want === nav.current.item)) return closeFilesView();
  openFilesView(want);
}

/** Views with their own f and / (a filter box, the diff's file list) and
 *  Shift+R (the Branches view's fetch): the window's keys stand down. */
const FINDS_ITSELF = new Set(['diff', 'files', 'tasks', 'branches']);
const REFRESHES_ITSELF = new Set(['branches']);

/** F and /: the window's find, the palette, where the view has no filter
 *  of its own. */
function find(): boolean {
  if (FINDS_ITSELF.has(nav.current.view)) return false;
  ui.help = false;
  ui.palette = true;
  return true;
}

/** Shift+R: ask GitHub again for the pull requests. */
function refresh(): boolean {
  if (REFRESHES_ITSELF.has(nav.current.view)) return false;
  refreshPrs();
  return true;
}

/** 1-9: the Nth repo on the rail, as a click on it. */
function railRepo(n: number): boolean {
  const r = railOrder(repoRows(app.model))[n - 1];
  if (!r) return false;
  revealRepo(r.project.id);
  return true;
}

const split = () => nav.current.view === 'session' || nav.current.view === 'diff' || nav.current.view === 'files' || nav.current.view === 'orchestrator';

/** Z, Ctrl+Shift+M and the panes' Full pane button: the split's right side
 *  takes the window, or comes back down. Nothing to do with no split up. */
export function toggleMaximize(): boolean {
  if (!split()) return false;
  app.paneMax = !app.paneMax;
  return true;
}

/** P, N, Ctrl+Shift+N: the quick prompt, on the focused row's repo when a
 *  row, card or repo header has the focus. */
function quickPrompt() {
  const repo = focusedRepo();
  focusQuickPrompt(repo ? { project: repo } : undefined);
}

/** What the verbs menu acts on with no row selected: the diff's
 *  checkout in the diff, the shown session in its view, else the repo in
 *  view. */
function viewTarget(): Target | null {
  const v = nav.current.view;
  if (v === 'diff' && app.diffWorktree) return { kind: 'worktree', id: app.diffWorktree };
  if (v === 'files' && nav.current.item) return { kind: 'worktree', id: nav.current.item };
  if (v === 'session') return currentTarget();
  const repo = nav.current.repo;
  return repo ? { kind: 'project', id: repo } : null;
}

/** The verbs menu's title: what its verbs act on. */
export function targetName(t: Target): string {
  const m = app.model;
  switch (t.kind) {
    case 'agent':
      return `session ${m.agents.find((a) => a.id === t.id)?.name ?? ''}`;
    case 'terminal':
      return `terminal ${m.terminals.find((x) => x.id === t.id)?.name ?? ''}`;
    case 'worktree': {
      const w = m.worktrees.find((x) => x.id === t.id);
      return `${nav.current.view === 'diff' ? 'diff' : nav.current.view === 'files' ? 'files' : w?.is_main ? 'checkout' : 'worktree'} ${w?.branch ?? ''}`;
    }
    case 'project':
      return `repo ${m.projects.find((p) => p.id === t.id)?.name ?? ''}`;
    case 'account':
      return `account ${t.id}`;
  }
}

/** The verbs menu's last row: every key (the full help, F1). */
export function allKeysItem(): MenuItem {
  return { label: 'All keys\u2026', run: () => ((ui.palette = false), (ui.help = true)), keys: keysOf('help_f1'), pinned: true };
}

/** The sessions the board's scope admits, live ones. */
function scopedSessions() {
  return app.model.agents.filter((a) => !a.archived && inScope(app.model, a.id));
}

/** The background's menu (\u00a75.15): the window's own verbs, for a
 *  right-click on empty space and `?` with nothing selected. View \u25b8 ticks
 *  the home in use and the archived sessions shown; Mark all seen and
 *  Archive done sessions work on the board's scope. */
export function backgroundMenu(): MenuEntry[] {
  const repo = currentRepo();
  const view = nav.current.view;
  const t: VerbTable = {
    quick_prompt: { run: quickPrompt },
    add_project: { run: () => addRepo() },
    command_palette: { run: () => ((ui.help = false), (ui.palette = true)) },
    home_overview: { run: () => setHome('overview', nav.current.repo), checked: app.home === 'overview', radio: true },
    home_board: { run: () => setHome('board', nav.current.repo), checked: app.home === 'board', radio: true },
    toggle_orchestrator_view: { run: orchestratorKey, checked: view === 'orchestrator', radio: true },
    accounts: { run: openAccounts, checked: view === 'accounts', radio: true },
    wake_schedule: { run: openWakeView, checked: view === 'whakaoho', radio: true },
    toggle_archived: { run: toggleArchived, checked: app.showArchived },
    settings: { run: () => openSettings() },
  };
  // The critters' own toggle (the shared `critters` setting), ticked while
  // on, once they have been found (menu matrix 5.15).
  // Studio mode (0.60): the rows show once a repo has it; Repo and Session
  // rows (studio_autonomy, renew_orchestrator) are in actions.ts.
  if (app.studioRepos.length) {
    t.studio_board = { run: () => openStudio(repo) };
    t.gauntlet_view = { run: () => openGauntlet(repo) };
  }
  if (triageAvailable()) t.triage_note = { run: focusTriage };
  t.usage_csv = { run: () => void exportUsage() };
  if (critters.found) t.critters = { run: () => void toggleCritters(), checked: critters.prefs.on };
  const where = isEverything(app.model.scope) ? 'across every repo' : 'in the scope';
  if (scopedSessions().some((a) => a.unseen)) t.mark_seen = { run: () => markAllSeen(scopedSessions()) };
  if (scopedSessions().some(isDone)) t.archive_done = { run: () => archiveDone(scopedSessions, where) };
  let rows = registryRows('global', t);
  // The repo in view's Worktrees table, a view of its own in the window.
  if (repo)
    rows = insertRows(
      rows,
      localRow(
        'local:view.worktrees',
        'go',
        { label: 'Worktrees', run: () => nav.go({ view: 'worktrees', repo }), checked: view === 'worktrees', radio: true },
        { id: 'view', title: 'View', label: 'Worktrees' },
      ),
      { after: ['wake_schedule', 'accounts', 'toggle_orchestrator_view', 'home_board'] },
    );
  return layout(rows);
}

/** `?`: the verbs of the focused row (or of what the view shows), each
 *  with its key, searchable, All keys last. */
export function openVerbs(): boolean {
  const jump = !!document.activeElement?.closest('[data-menu-jump]');
  const { items, title } = verbsMenu(selectedTarget() ?? viewTarget(), { jump });
  const row = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-target], [data-verb-bar]');
  const at = row ?? { x: Math.max(8, Math.round(window.innerWidth / 2 - 180)), y: 96 };
  openMenu(items, at, title, { search: true });
  return true;
}

/** The verbs menu for `t` (the window's own verbs for none): its menu's
 *  rows, then All keys. */
export function verbsMenu(t: Target | null, opts: MenuOpts = {}): { items: MenuEntry[]; title: string } {
  const items: MenuEntry[] = t ? menuFor(t, opts) : backgroundMenu();
  return { items: [...items, 'sep', allKeysItem()], title: t ? targetName(t) : 'Verbs' };
}

/** Run the action a shortcut names. False when it does nothing here. */
export function runShortcut(id: ShortcutId): boolean {
  const go: Partial<Record<ShortcutId, () => unknown>> = {
    palette: () => ((ui.help = false), (ui.palette = true)),
    palette_alt: () => ((ui.help = false), (ui.palette = true)),
    verbs: openVerbs,
    help: () => ((ui.palette = false), (ui.help = !ui.help)),
    help_f1: () => ((ui.palette = false), (ui.help = !ui.help)),
    whats_new: () => ((ui.palette = false), (ui.help = false), (learn.whatsNew = true)),
    settings: () => openSettings(),
    find,
    refresh,
    back: () => nav.back(),
    forward: () => nav.forward(),
    home_board: () => setHome('board', nav.current.repo),
    home_overview: () => setHome('overview', nav.current.repo),
    toggle_home: toggleHome,
    toggle_sidebar: toggleSidebar,
    open_session: () => app.session && openSession(app.session),
    open_diff: toggleDiff,
    diff: toggleDiff,
    open_files: toggleFiles,
    files: toggleFiles,
    repo_1: () => railRepo(1),
    repo_2: () => railRepo(2),
    repo_3: () => railRepo(3),
    repo_4: () => railRepo(4),
    repo_5: () => railRepo(5),
    repo_6: () => railRepo(6),
    repo_7: () => railRepo(7),
    repo_8: () => railRepo(8),
    repo_9: () => railRepo(9),
    quick_prompt: quickPrompt,
    quick_prompt_alt: quickPrompt,
    cycle_theme: cycleTheme,
    go_home: () => goHome(nav.current.repo),
    go_session: () => app.session && openSession(app.session),
    go_diff: () => openDiff(app.diffWorktree ?? sessionWorktree(app.session)),
    orchestrator: orchestratorKey,
    orchestrator_f10: orchestratorKey,
    accounts: openAccounts,
    close_split: () => {
      if (!split() && nav.current.view !== 'settings' && nav.current.view !== 'accounts' && nav.current.view !== 'whakaoho') return false;
      // A full pane comes back down first; the next Esc closes the split.
      if (app.paneMax && split()) return void (app.paneMax = false);
      if (nav.current.view === 'diff') return closeDiff();
      if (nav.current.view === 'files') return closeFilesView();
      goHome(nav.current.repo);
    },
    toggle_archived: toggleArchived,
    maximize: toggleMaximize,
    maximize_z: toggleMaximize,
    next_attention: () => jumpAttention(1),
    prev_attention: () => jumpAttention(-1),
    new_worktree: () => newWorktree(focusedRepo() ?? currentRepo()),
  };
  const f = go[id];
  if (f) return f() !== false;
  // A chip with a menu of its own (a pull request, a port) under the Menu
  // key: the key's own contextmenu follows, and the router opens the
  // chip's menu, not its row's.
  if (id === 'row_menu') {
    const near = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-target]');
    if (near && !parseTarget(near.dataset.target)) return false;
  }
  // Row keys act on the focused row or card.
  const t = focusedTarget();
  if (!t) return false;
  if (id === 'move_up' || id === 'move_down') return moveStep(t, id === 'move_up' ? -1 : 1, stepInFor(document.activeElement));
  // The Menu key: the row's menu, named for it, as a right-click opens it.
  if (id === 'row_menu') {
    const el = document.activeElement as HTMLElement;
    openTargetMenu(t, el, { jump: !!el.closest('[data-menu-jump]') });
    return true;
  }
  return runRowKey(id, t);
}

function focusedRepo(): string | undefined {
  const t = focusedTarget();
  if (!t) return undefined;
  if (t.kind === 'project') return t.id;
  if (t.kind === 'worktree') return projectOf(t.id);
  if (t.kind === 'agent') return projectOf(app.model.agents.find((a) => a.id === t.id)?.worktree_id ?? null);
  return projectOf(app.model.terminals.find((x) => x.id === t.id)?.worktree_id ?? null);
}

type Command = PaletteCommand & { run: () => unknown };

/** With the Diff view up on a worktree that has a base branch: compare with
 *  the other one (the header's toggle). No key of its own. */
export function diffAgainstCommand(
  cmd: (id: string, label: string, group: string, run: () => unknown, key?: ShortcutId, words?: string) => Command,
): Command[] {
  const shown = app.diffShown;
  if (nav.current.view !== 'diff' || app.diffRange || !shown?.base) return [];
  const toBase = shown.against === 'head';
  const label = toBase ? `Diff: compare with ${shown.base.branch}` : 'Diff: compare with HEAD';
  return [cmd('diff-against', label, 'Window', () => (app.diffAgainst = toBase ? 'base' : 'head'), undefined, 'merge-base base head against branch committed')];
}

/** The `critters` verb (on or off, the one shared setting), and while
 *  they're on, the way to hide them for the day (or bring them back) and
 *  the log: the critter menu's rows for the keyboard. */
function critterCommands(cmd: (id: string, label: string, group: string, run: () => unknown, key?: ShortcutId, words?: string) => Command): Command[] {
  const words = 'critters easter egg pixel visitors animals';
  const out = [cmd('critters', critters.prefs.on ? 'Turn critters off' : 'Turn critters on', 'Window', toggleCritters, undefined, words)];
  if (!critters.prefs.on) return out;
  out.push(
    hiddenNow()
      ? cmd('critters-back', 'Bring the critters back today', 'Window', bringBack, undefined, words)
      : cmd('critters-hide', 'Hide critters until tomorrow', 'Window', hideUntilTomorrow, undefined, words),
    cmd('critter-log', 'Critter log\u2026', 'Window', openCritterLog, undefined, `${words} collection log seen`),
  );
  return out;
}

/** Everything the palette offers besides jumping: the window's actions,
 *  then the shown session's own verbs. */
export function paletteCommands(): Command[] {
  const cmd = (id: string, label: string, group: string, run: () => unknown, key?: ShortcutId | ShortcutId[], words?: string): Command => ({
    id,
    label,
    group,
    run,
    keys: key ? [key].flat().flatMap((k) => keysOf(k)) : undefined,
    words,
  });
  const out: Command[] = [
    cmd('new-session', 'New session', 'Window', () => focusQuickPrompt(), 'quick_prompt', 'quick prompt start agent'),
    cmd('new-worktree', 'New worktree\u2026', 'Window', () => newWorktree(currentRepo()), 'new_worktree', 'branch'),
    cmd('overview', 'Show the overview', 'Window', () => setHome('overview', nav.current.repo), 'home_overview'),
    cmd('board', 'Show the board', 'Window', () => setHome('board', nav.current.repo), 'home_board', 'kanban'),
    cmd('home', 'Home', 'Window', () => goHome(nav.current.repo), 'go_home', 'overview board view'),
    ...(app.session ? [cmd('session', 'Show the session', 'Window', () => app.session && openSession(app.session), ['open_session', 'go_session'], 'view')] : []),
    cmd('archived', app.showArchived ? 'Hide archived sessions' : 'Show archived sessions', 'Window', toggleArchived, 'toggle_archived'),
    // The background's bulk verbs, on the board's scope.
    ...(scopedSessions().some((a) => a.unseen)
      ? [cmd('mark-all-seen', menuLabel('mark_seen', 'global'), 'Window', () => markAllSeen(scopedSessions()), undefined, 'read unread inbox triage')]
      : []),
    ...(scopedSessions().some(isDone)
      ? [
          cmd(
            'archive-done',
            menuLabel('archive_done', 'global'),
            'Window',
            () => archiveDone(scopedSessions, isEverything(app.model.scope) ? 'across every repo' : 'in the scope'),
            undefined,
            'finished clean up done lane',
          ),
        ]
      : []),
    cmd('next-attention', 'Next session needing you', 'Window', () => jumpAttention(1), 'next_attention', 'attention jump feedback'),
    cmd('prev-attention', 'Previous session needing you', 'Window', () => jumpAttention(-1), 'prev_attention', 'attention jump feedback'),
    cmd('diff', 'Diff the worktree', 'Window', () => openDiff(diffTarget()), ['diff', 'open_diff']),
    cmd('files', 'Files of the worktree', 'Window', () => openFilesView(diffTarget()), ['files', 'open_files'], 'tree explorer browse folder'),
    ...diffAgainstCommand(cmd),
    ...(split() ? [cmd('maximize', app.paneMax ? 'Bring the pane back down' : 'Full pane', 'Window', toggleMaximize, ['maximize_z', 'maximize'], 'maximize zoom full screen')] : []),
    cmd('sidebar', 'Collapse or expand the sidebar', 'Window', toggleSidebar, 'toggle_sidebar', 'rail panel hide show'),
    cmd('theme', `Cycle Workshop theme (now ${themeLabel(app.themePref, look.prefs.themes)})`, 'Window', cycleTheme, 'cycle_theme', 'dark light system'),
    cmd('import-theme', 'Import a VS Code theme\u2026', 'Window', () => void importVsCodeTheme(), undefined, 'appearance colours colors one dark vscode'),
    ...(lookIsKetes() ? [] : [cmd('reset-look', 'Reset appearance to Kete defaults', 'Window', () => void resetAppearance(), undefined, 'appearance theme font workshop default')]),
    cmd('settings', 'Settings', 'Window', () => openSettings(), 'settings', 'preferences config'),
    cmd('presets', 'Agent presets', 'Window', () => openSettings('presets'), undefined, 'prefix postfix'),
    cmd('export-settings', menuLabel('export_settings', 'global'), 'Window', exportSettings, undefined, 'backup config save file'),
    cmd('import-settings', menuLabel('import_settings', 'global'), 'Window', importSettings, undefined, 'restore config load file'),
    cmd('accounts', 'Accounts', 'Window', openAccounts, 'accounts', 'profiles login usage spread subscription'),
    cmd('whakaoho', 'Whakaoho \u00b7 Wake-up', 'Window', () => openWake(), undefined, 'wake wake-up morning heartbeat 5-hour window reset schedule'),
    cmd('add-account', 'Add an account\u2026', 'Window', () => openAddAccount(), undefined, 'profile login new subscription'),
    cmd('profiles', 'Profiles', 'Window', openAccounts, undefined, 'accounts login'),
    cmd('help', 'Keyboard shortcuts', 'Window', () => (ui.help = true), 'help_f1', 'help keys'),
    cmd('whats-new', "What's new", 'Window', () => (learn.whatsNew = true), 'whats_new', 'changes release notes changelog keys moved'),
    cmd('add-repo', 'Add a repo\u2026', 'Window', () => addRepo(), undefined, 'project folder'),
    ...critterCommands(cmd),
    ...orchestratorCommands(),
    // With a repo in view these come from its menu, below.
    ...(currentRepo()
      ? []
      : [
          cmd('refresh-prs', 'Refresh pull requests', 'Window', refreshPrs, 'refresh', 'pr github merged reload'),
          cmd('switch-branch', 'Switch branch\u2026', 'Window', () => pickBranchSwitch(), 'switch_branch', 'checkout git'),
        ]),
  ];
  // The selection's verbs first, ahead of every session (palette.ts
  // pins them).
  const sel = selectedTarget();
  if (sel) {
    let i = 0;
    for (const it of leafItems(menuFor(sel))) {
      if (it.disabled) continue;
      out.push({ id: `sel-${i++}`, label: it.full ?? it.label, group: targetName(sel), keys: it.keys, run: it.run, pinned: true });
    }
  }
  const shown = currentTarget();
  const t = shown && !(sel && sel.kind === shown.kind && sel.id === shown.id) ? shown : null;
  if (t) {
    const name =
      t.kind === 'agent' ? app.model.agents.find((a) => a.id === t.id)?.name : app.model.terminals.find((x) => x.id === t.id)?.name;
    let i = 0;
    for (const it of leafItems(menuFor(t))) {
      if (it.disabled) continue;
      out.push({ id: `session-${i++}`, label: it.full ?? it.label, group: `Session ${name ?? ''}`.trim(), keys: it.keys, run: it.run });
    }
  }
  const repo = currentRepo();
  if (repo) {
    const name = app.model.projects.find((p) => p.id === repo)?.name ?? 'repo';
    let i = 0;
    for (const it of leafItems(menuFor({ kind: 'project', id: repo }))) {
      if (it.disabled) continue;
      out.push({ id: `repo-${i++}`, label: it.full ?? it.label, group: `Repo ${name}`, keys: it.keys, run: it.run });
    }
  }
  return out;
}

/** Ask for a folder and add it as a repo (the rail's Add a repo, the
 *  palette's, the empty window's). */
export async function addRepo() {
  const v = await ask.prompt({
    title: 'Add a repo',
    fields: [{ key: 'path', label: 'Folder', placeholder: 'C:\\path\\to\\repo', mono: true }],
    confirm: 'Add',
  });
  if (v?.path.trim()) await api.addProject(v.path).catch((e) => toast(String(e)));
}
