// App-wide state as Svelte runes: the daemon model the Rust side pushes,
// and what the window is looking at. Where the window is lives in
// `nav` (shell/history.svelte.ts); this file follows it.

import { untrack } from 'svelte';
import { api, type AccountRow, type Launchables, type Model, type Scope, type SessionRef } from './api';
import { pruneScope, repoScope, sameScope, tidy } from './scope';
import { ado, type AdoContextInfo } from './ado/api';
import { nav, type Place } from './shell/history.svelte';
import { applyOverrides } from './shortcuts';
import { diffItem, isDiffMode, parseDiffItem, type DiffMode, type DiffRange } from './diff-api';
import type { Against, ReviewBase } from './diff-base';
import { knownTheme } from './look';
import { look, setLookPrefs } from './look.svelte';
import { refreshLabels } from './accounts/labels.svelte';

/** Workshop's `system`, `light` or `dark`, `one-dark`, `one-light`,
 *  `vscode`, or `import:<id>` (lib/look.ts). */
export type ThemePref = string;
/** What the main column shows when no view is open: the overview or the board. */
export type Home = 'overview' | 'board';
/** What the quick prompt should switch to, once (a rail pick, a duplicate). */
export type QuickPreset = {
  project?: string;
  worktree?: string;
  harness?: string;
  profile?: string;
  model?: string | null;
  effort?: string | null;
  newWorktree?: boolean;
};

const empty: Model = {
  connected: false,
  error: null,
  daemon_pid: 0,
  protocol_version: 0,
  runtime_dir: '',
  projects: [],
  worktrees: [],
  agents: [],
  terminals: [],
  ports: [],
  prs: {},
  subagents: [],
  accounts: {},
  contexts: {},
  stops: {},
  cards: {},
  lanes: { needs: [], running: [], unseen: [], done: [], pr: [] },
  hidden: { needs: 0, running: 0, unseen: 0, done: 0, pr: 0 },
  scope: {},
};

export const app = $state({
  model: empty as Model,
  booted: false,
  home: 'overview' as Home,
  /** The session the Session and Diff views show. */
  session: null as SessionRef | null,
  /** The worktree the Diff view reads, when no session picks it. */
  diffWorktree: null as string | null,
  /** The range the Diff view compares; null: the working tree against HEAD. */
  diffRange: null as DiffRange | null,
  /** What the Diff view reads a working tree against, as the reader picked
   *  it (the header's toggle, the palette); null: the base when there is one. */
  diffAgainst: null as Against | null,
  /** What the Diff view last read against, and the base there is. */
  diffShown: null as { against: Against; base: ReviewBase | null } | null,
  /** The mode for a repo whose Diff view has none saved yet. */
  diffMode: 'side' as DiffMode,
  /** Bumped to ask the quick prompt to take focus. */
  quickFocus: 0,
  /** Harness and profile the quick prompt should switch to, once. */
  quickPreset: null as QuickPreset | null,
  /** Width of the right split (session, diff) in px. */
  splitWidth: 0,
  /** The split's right side (session, diff) has the whole window. */
  paneMax: false,
  themePref: 'system' as ThemePref,
  dark: false,
  now: Date.now(),
  toast: null as string | null,
  launch: { harnesses: [], profiles: [] } as Launchables,
  /** The rail's accounts (`accounts_list`); live usage is `model.accounts`. */
  accounts: [] as AccountRow[],
  /** Per project id: its ADO remote, null when none, absent until asked. */
  adoCtx: {} as Record<string, AdoContextInfo | null>,
  /** A repo picked in the rail, for the overview to scroll to; seq bumps per
   *  pick, and `done` is the last seq the overview scrolled for. */
  reveal: { repo: null as string | null, seq: 0, done: 0 },
  /** Worktree id to the command its Run button starts. */
  runCommands: {} as Record<string, string>,
  /** The overview lists archived sessions too (Shift+A). */
  showArchived: false,
  /** The session whose review the Diff view shows (a Review or Ship card
   *  opened): its verbs ride above the diff while it reads its worktree. */
  review: null as string | null,
});

/** Narrow the window to `s` (`{}`: everything). The Tauri side holds it
 *  and pushes the model back narrowed; `model.scope` is the truth. */
export function setScope(s: Scope) {
  if (sameScope(s, app.model.scope)) return;
  api.boardScope(tidy(s)).catch((e) => toast(`Could not set the scope: ${e}`));
}

/** The window's own preferences, saved under `desktop` in config.json. */
export type DesktopPref = 'theme' | 'diff_mode' | 'home';

/** Save one preference. A failed write says so; the window keeps the value. */
export function saveDesktopPref(key: DesktopPref, value: string) {
  api.settingsSet({ scope: 'desktop', key }, value).catch((e) => toast(`Could not save the preference: ${e}`));
}

/** The theme, changed by the user: applied and saved. */
export function setThemePref(p: ThemePref) {
  app.themePref = p;
  saveDesktopPref('theme', p);
}

/** Overview or board, picked by the user: shown and saved. A session or
 *  diff open beside the list stays open: the pick takes the split's left
 *  side (Esc closes the split). */
export function setHome(h: Home, repo?: string) {
  app.home = h;
  saveDesktopPref('home', h);
  if (nav.current.view === 'session' || nav.current.view === 'diff' || nav.current.view === 'files') return;
  goHome(repo);
}

/** Read the saved preferences (before any command-line override). */
export async function loadDesktopPrefs() {
  api
    .configProblem()
    .then((p) => p && toast(`Settings are read-only until config.json is fixed: ${p}`))
    .catch(() => {});
  const p = await api.desktopPrefs().catch(() => null);
  if (!p) return null;
  setLookPrefs(p);
  if (knownTheme(p.theme, look.prefs.themes)) app.themePref = p.theme;
  if (isDiffMode(p.diff_mode)) app.diffMode = p.diff_mode;
  if (p.home === 'overview' || p.home === 'board') {
    app.home = p.home;
    if (nav.current.view === 'overview' || nav.current.view === 'board') nav.replace({ ...nav.current, view: p.home });
  }
  return p;
}

/** Lay the window's own key bindings (Settings, Hotkeys) over the
 *  defaults. Without them the defaults stand. */
export function loadDesktopKeys() {
  api
    .actionsList()
    .then((l) => applyOverrides(l.overrides))
    .catch(() => {});
}

/** Ask which worktrees have a run command (after a settings change too). */
export function refreshRunCommands() {
  api
    .runCommands()
    .then((r) => (app.runCommands = r))
    .catch(() => {});
}

/** Ask again what the quick prompt offers (after a settings change). */
export function refreshLaunchables() {
  api
    .launchables()
    .then((l) => (app.launch = l))
    .catch(() => {});
  refreshAccounts();
}

/** Ask again for the rail's accounts (after a profile edit or toggle). */
export function refreshAccounts() {
  api
    .accounts()
    .then((a) => {
      app.accounts = a;
      refreshUsageLabels();
    })
    .catch(() => {});
}

/** The Settings view, on a tab. */
export function openSettings(tab?: string) {
  nav.go({ view: 'settings', item: tab ?? (nav.current.view === 'settings' ? nav.current.item : undefined) });
}

export function sessionKey(s: SessionRef | null): string {
  if (!s) return '';
  return 'Agent' in s ? `a:${s.Agent}` : `t:${s.Terminal}`;
}

export function sessionFromKey(k: string | undefined): SessionRef | null {
  if (!k) return null;
  if (k.startsWith('a:')) return { Agent: k.slice(2) };
  if (k.startsWith('t:')) return { Terminal: k.slice(2) };
  return null;
}

/** The worktree a session lives in. */
export function sessionWorktree(s: SessionRef | null): string | null {
  if (!s) return null;
  const m = app.model;
  if ('Agent' in s) return m.agents.find((a) => a.id === s.Agent)?.worktree_id ?? null;
  return m.terminals.find((t) => t.id === s.Terminal)?.worktree_id ?? null;
}

/** The project a worktree belongs to. */
export function projectOf(worktree: string | null): string | undefined {
  if (!worktree) return undefined;
  return app.model.worktrees.find((w) => w.id === worktree)?.project_id;
}

/** The session the window shows or selects: seen, so its unread clears. */
export function markSeen(s: SessionRef | null) {
  if (s && 'Agent' in s) api.markSeen(s.Agent).catch(() => {});
}

export function openSession(s: SessionRef) {
  app.session = s;
  nav.go({ view: 'session', repo: projectOf(sessionWorktree(s)), item: sessionKey(s) });
  if ('Agent' in s) api.markSeen(s.Agent).catch(() => {});
}

export function openDiff(worktree: string | null) {
  app.review = null;
  app.diffWorktree = worktree;
  app.diffRange = null;
  nav.go({ view: 'diff', repo: projectOf(worktree), item: worktree ?? undefined });
}

/** The Diff view on a range (`base` and `tip` as refs or SHAs), read in `worktree`. */
export function openDiffRange(worktree: string, range: DiffRange) {
  app.review = null;
  app.diffWorktree = worktree;
  app.diffRange = { base: range.base, tip: range.tip };
  nav.go({ view: 'diff', repo: projectOf(worktree), item: diffItem(worktree, range) });
}

/** A Review (or Ship) card opened: the board stays on the left, the
 *  session's worktree diffed against its base on the right, the card's
 *  verbs (reply, PR, merge, archive, delete worktree) above the diff. The
 *  session is read, as opening it would. */
export function openReview(agent: string) {
  const a = app.model.agents.find((x) => x.id === agent);
  if (!a) return;
  app.session = { Agent: agent };
  markSeen(app.session);
  app.diffAgainst = 'base';
  openDiff(a.worktree_id);
  app.review = agent;
}

/** Leave the Diff view for wherever it was opened from (the orchestrator,
 *  a session), or home when it was the first place. */
export function closeDiff() {
  const repo = nav.current.repo;
  const home = app.home;
  // Past every diff place (a worktree picked, a range opened) to the view
  // the diff was opened from.
  while (nav.canBack && nav.current.view === 'diff') nav.back();
  if (nav.current.view === 'diff') goHome(repo);
  // Home switched while the diff was up (Overview | Board keeps the
  // split): land on the home picked, not the one the diff was opened from.
  else if ((nav.current.view === 'overview' || nav.current.view === 'board') && nav.current.view !== home)
    nav.replace({ ...nav.current, view: home });
}

/** Back to the overview (or the board), closing any split. */
export function goHome(repo?: string) {
  nav.go({ view: app.home, repo });
}

/** A repo picked in the rail (or by its digit): narrow the scope to it, go
 *  home and scroll its row into view. */
export function revealRepo(repo: string) {
  setScope(repoScope(app.model.scope, repo));
  goHome(repo);
  app.reveal.repo = repo;
  app.reveal.seq += 1;
}

/** Make the window's selection follow a place reached by back or forward. */
export function follow(p: Place) {
  if (p.view === 'overview' || p.view === 'board') app.home = p.view;
  if (p.view === 'orchestrator') {
    // The orchestrator view shows its session in the split's pane.
    const o = app.model.agents.find((a) => a.role === 'orchestrator' && !a.archived);
    if (o) {
      // A plain object, never read back from `app.session`: this runs in
      // an effect, and reading what it writes would loop.
      const s: SessionRef = { Agent: o.id };
      if (sessionKey(untrack(() => app.session)) !== sessionKey(s)) app.session = s;
      markSeen(s);
    }
  } else if (p.view === 'session') {
    const s = sessionFromKey(p.item);
    if (s) app.session = s;
    // Back or forward onto a session shows it again: it is seen.
    markSeen(s);
  } else if (p.view === 'diff') {
    const d = parseDiffItem(p.item);
    app.diffWorktree = d.worktree;
    app.diffRange = d.range;
  }
}

/** Ask the quick prompt for focus, optionally switching harness and profile. */
export function focusQuickPrompt(preset?: QuickPreset) {
  if (preset) app.quickPreset = preset;
  app.quickFocus += 1;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;
export function toast(msg: string) {
  app.toast = msg;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (app.toast = null), 5000);
}

/** A failed ado.json write as a toast. The ADO views ask when they open. */
export function checkAdoStore() {
  ado
    .storeError()
    .then((e) => e && toast(e))
    .catch(() => {});
}

/** Look up each project's ADO remote once (each lookup runs git). */
const asked = new Set<string>();
/** A failed lookup is asked again, at most this often. */
const RETRY_MS = 30_000;
const failedAt = new Map<string, number>();
function askAdo(m: Model) {
  for (const p of m.projects) {
    if (asked.has(p.id)) continue;
    if (Date.now() - (failedAt.get(p.id) ?? 0) < RETRY_MS) continue;
    const main =
      m.worktrees.find((w) => w.project_id === p.id && w.is_main) ?? m.worktrees.find((w) => w.project_id === p.id);
    if (!main) continue;
    asked.add(p.id);
    ado
      .context(main.id)
      .then((ctx) => {
        app.adoCtx[p.id] = ctx;
        if (ctx) nav.seed(p.id, ctx.ui);
      })
      .catch(() => {
        // A failed lookup is not "no ADO remote": ask again later.
        asked.delete(p.id);
        failedAt.set(p.id, Date.now());
        app.adoCtx[p.id] = null;
      });
  }
}

/** View state goes to ado.json for repos that live on ADO; others keep it in memory. */
nav.setSaver((view, repo, state) => {
  const ctx = app.adoCtx[repo];
  if (ctx) ado.saveUi(ctx.repoKey, view, state).catch(() => {});
});

let worktreeSig = '';
function take(m: Model) {
  app.model = m;
  refreshUsageLabels();
  askAdo(m);
  // A scope naming a repo, worktree or session that is gone lets it go.
  const pruned = pruneScope(m);
  if (pruned) setScope(pruned);
  // A session open on screen that turns unread again (a new turn finished
  // while you watch) is seen at once, as the TUI's pane does.
  seeOpenSession();
  const sig = m.worktrees.map((w) => w.id).join(',');
  if (sig !== worktreeSig) {
    worktreeSig = sig;
    refreshRunCommands();
  }
}

/** Mark the session on screen seen when it reads unread and the window has focus. */
export function seeOpenSession() {
  const s = app.session;
  if ((nav.current.view !== 'session' && nav.current.view !== 'orchestrator') || !s || !('Agent' in s)) return;
  if (typeof document !== 'undefined' && !document.hasFocus()) return;
  if (app.model.agents.find((a) => a.id === s.Agent)?.unseen) markSeen(s);
}

/** Reword every window on screen (the live push and the rail's list) with
 *  the shared formatter, as of now: the reset in local time and the countdown. */
export function refreshUsageLabels() {
  const windows = [
    ...Object.values(app.model.accounts ?? {}).flatMap((u) => u.windows),
    ...app.accounts.flatMap((r) => r.usage?.windows ?? []),
  ];
  void refreshLabels(windows, Date.now());
}

export async function boot() {
  try {
    take(await api.state());
  } catch {
    /* the push below catches up */
  }
  await api.onState(take);
  app.booted = true;
  refreshLaunchables();
  window.addEventListener('focus', seeOpenSession);
  setInterval(() => {
    app.now = Date.now();
    refreshUsageLabels();
  }, 15_000);
}
