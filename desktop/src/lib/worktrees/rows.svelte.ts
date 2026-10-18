// The window-side state behind the overview's worktree groups and the
// Worktrees view: which groups are folded (saved under `desktop` in
// config.json), each checkout's git state (read in the background,
// cached, again on focus, as a session's turn ends there, as soon as its
// git stamp moves, and every so often while a view shows it), and
// the checkouts git is still cutting, drawn as placeholders until the
// daemon lists them.

import { untrack } from 'svelte';
import { api } from '../api';
import { app, toast } from '../store.svelte';
import { worktreeApi, type WorktreeGit } from './api';
import { pruneKeys, repoKey, setKey } from './groups';
import { turnMarks, turnedWorktrees, type TurnMarks } from './turns';
import { STAMP_MS, changedGit, dueRunning, movedStamps } from './watch';

export const rows = $state({
  /** Folded groups: `repo:<id>`, `wt:<id>`. */
  collapsed: [] as string[],
  /** Worktree id to its git state, as last read. */
  git: {} as Record<string, WorktreeGit>,
  /** Checkouts being made: shown as placeholder rows in their repo. */
  pending: [] as { key: number; project: string; branch: string }[],
});

// ---- folding ----

export function isCollapsed(key: string): boolean {
  return rows.collapsed.includes(key);
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
export function setCollapsed(key: string, on: boolean) {
  rows.collapsed = setKey(rows.collapsed, key, on);
  // A repo opened again reads its checkouts now, not at the next poll.
  if (!on) refreshGit(shownIds());
  clearTimeout(saveTimer);
  // Folding a few in a row writes config.json once.
  saveTimer = setTimeout(() => {
    const keys = app.model.connected ? pruneKeys(rows.collapsed, app.model) : rows.collapsed;
    api.desktopSetCollapsed(keys).catch((e) => toast(`Could not save the fold: ${e}`));
  }, 400);
}

export function toggleCollapsed(key: string) {
  setCollapsed(key, !isCollapsed(key));
}

/** The saved folds, at startup. */
export function loadCollapsed(keys: unknown) {
  if (Array.isArray(keys)) rows.collapsed = keys.filter((k): k is string => typeof k === 'string');
}

// ---- git state ----

/** The least time between two reads of one checkout. */
const MIN_GAP_MS = 4_000;
/** How often the views read every checkout again while one is up: the
 *  backstop behind the stamps (watch.ts). */
const POLL_MS = 30_000;
const readAt = new Map<string, number>();
let reading = false;
/** Forced reads asked for during one: run as it ends. */
const queued = new Set<string>();

/** Read the checkouts' git state, skipping any read in the last few
 *  seconds unless `force`. One read at a time; a call during one waits
 *  for the next tick of the poll, or, forced, for that read to end. */
export async function refreshGit(ids: string[], force = false) {
  if (!ids.length) return;
  if (reading) {
    if (force) for (const id of ids) queued.add(id);
    return;
  }
  const now = Date.now();
  const due = ids.filter((id) => force || now - (readAt.get(id) ?? 0) >= MIN_GAP_MS);
  if (!due.length) return;
  reading = true;
  for (const id of due) readAt.set(id, now);
  try {
    const got = await worktreeApi.git(due);
    // Only what changed is written: an unchanged row keeps its object.
    for (const [id, g] of Object.entries(changedGit(rows.git, got))) rows.git[id] = g;
  } catch {
    /* the next poll tries again */
  } finally {
    reading = false;
  }
  if (queued.size) {
    const next = [...queued];
    queued.clear();
    await refreshGit(next, true);
  }
}

let watchers = 0;
let timer: ReturnType<typeof setInterval> | undefined;
let stampTimer: ReturnType<typeof setInterval> | undefined;
/** Each checkout's stamp as last taken. */
let stamps: Record<string, string> = {};
let stamping = false;

/** Take the shown checkouts' stamps and read the ones that moved, and,
 *  while the window has focus, any checkout a session is working in whose
 *  slower beat is due. Stamps are file metadata alone, so they are taken
 *  unfocused too: a commit made in another window shows behind it. */
async function stampTick() {
  if (stamping) return;
  const shown = shownIds();
  if (!shown.length) return;
  stamping = true;
  try {
    const next = await worktreeApi.stamps(shown);
    const moved = movedStamps(stamps, next);
    // The first look only records: watchGit has just read them all.
    const first = !Object.keys(stamps).length;
    stamps = next;
    const due = document.hasFocus() ? dueRunning(app.model.agents, shown, readAt, Date.now()) : [];
    const ids = [...new Set([...(first ? [] : moved), ...due])];
    if (ids.length) await refreshGit(ids, true);
  } catch {
    /* the next tick tries again */
  } finally {
    stamping = false;
  }
}
/** Checkouts in repos the overview has open (a folded repo's are not
 *  read), and every checkout with a card on the board, folded or not: the
 *  board's Review and Ship lanes read the same git state (src-tauri keeps
 *  the last read). */
export function shownIds(): string[] {
  const onBoard = new Set<string>();
  const byId = new Map(app.model.agents.map((a) => [a.id, a.worktree_id]));
  for (const ids of Object.values(app.model.lanes ?? {})) for (const id of ids) {
    const w = byId.get(id);
    if (w) onBoard.add(w);
  }
  return app.model.worktrees
    .filter((w) => onBoard.has(w.id) || !rows.collapsed.includes(repoKey(w.project_id)))
    .map((w) => w.id);
}
const onFocus = () => refreshGit(shownIds(), true);

let stopTurns: (() => void) | undefined;
/** Read a checkout again as soon as a session there changes status (a
 *  turn that ends has usually committed): the Review card's commit count
 *  and what Ship takes come off that read, and the poll is half a minute
 *  away. */
function watchTurns(): () => void {
  let marks: TurnMarks | null = null;
  return $effect.root(() => {
    $effect(() => {
      const agents = app.model.agents;
      const turned = turnedWorktrees(marks, agents);
      marks = turnMarks(agents);
      const shown = new Set(untrack(shownIds));
      const ids = turned.filter((id) => shown.has(id));
      if (ids.length) refreshGit(ids, true);
    });
  });
}

/** Keep the git state fresh while a view that shows it is mounted: at
 *  once, on window focus, as a session's turn ends, when a checkout's
 *  stamp moves (a commit, checkout, reset or stage from anywhere), every
 *  few seconds where a session is working, and every half minute.
 *  Returns the stop. */
export function watchGit(): () => void {
  watchers += 1;
  if (watchers === 1) {
    window.addEventListener('focus', onFocus);
    timer = setInterval(() => {
      if (document.hasFocus()) refreshGit(shownIds());
    }, POLL_MS);
    stampTimer = setInterval(stampTick, STAMP_MS);
    stopTurns = watchTurns();
  }
  // A turn may have ended while this view was unmounted; bypass the read gap.
  refreshGit(shownIds(), true);
  return () => {
    watchers -= 1;
    if (watchers === 0) {
      window.removeEventListener('focus', onFocus);
      clearInterval(timer);
      clearInterval(stampTimer);
      stamps = {};
      stopTurns?.();
      stopTurns = undefined;
    }
  };
}

// ---- checkouts being made ----

let nextKey = 0;
/** Show a placeholder for `branch` in `project` while `make` runs. */
export async function whileCutting<T>(project: string, branch: string, make: () => Promise<T>): Promise<T> {
  const key = ++nextKey;
  rows.pending = [...rows.pending, { key, project, branch }];
  try {
    return await make();
  } finally {
    rows.pending = rows.pending.filter((p) => p.key !== key);
  }
}
