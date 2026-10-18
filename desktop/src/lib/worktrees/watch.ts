// What the git watcher reads again. Every couple of seconds the window
// takes a stamp per shown checkout (src-tauri git.rs `git_stamp`: file
// metadata of HEAD, the index, the reflog and the packed refs, no git
// process) and reads only the checkouts whose stamp moved. An edit in the
// working tree moves no stamp, so a checkout with a running session is
// read again on a slower beat of its own. Pure, so the rules are tested
// without a window.

import type { Agent } from '../api';
import type { WorktreeGit } from './api';

/** How often stamps are taken while a view watches. */
export const STAMP_MS = 2_000;
/** The least time between two reads of a checkout a session is working in. */
export const RUNNING_MS = 6_000;

/** Checkouts whose stamp differs from the last one taken. A checkout seen
 *  for the first time counts as moved; one left out of `next` (gone, or
 *  not a checkout) does not. */
export function movedStamps(last: Record<string, string>, next: Record<string, string>): string[] {
  return Object.keys(next).filter((id) => last[id] !== next[id]);
}

/** Shown checkouts with a running session in them, last read at least
 *  RUNNING_MS ago (`readAt`, epoch ms). */
export function dueRunning(agents: Agent[], shown: string[], readAt: Map<string, number>, now: number): string[] {
  const working = new Set(agents.filter((a) => a.status === 'running' && !a.archived).map((a) => a.worktree_id));
  return shown.filter((id) => working.has(id) && now - (readAt.get(id) ?? 0) >= RUNNING_MS);
}

export function sameGit(a: WorktreeGit | undefined, b: WorktreeGit): boolean {
  if (!a) return false;
  return (
    a.changed === b.changed &&
    a.untracked === b.untracked &&
    a.upstream === b.upstream &&
    a.ahead === b.ahead &&
    a.behind === b.behind &&
    a.committed === b.committed &&
    a.ahead_of_base === b.ahead_of_base &&
    a.on_default_branch === b.on_default_branch
  );
}

/** The entries of `got` that differ from what `have` holds: the only
 *  ones written back, so an unchanged row keeps its object and nothing
 *  that reads it runs again. */
export function changedGit(have: Record<string, WorktreeGit>, got: Record<string, WorktreeGit>): Record<string, WorktreeGit> {
  const out: Record<string, WorktreeGit> = {};
  for (const [id, g] of Object.entries(got)) if (!sameGit(have[id], g)) out[id] = g;
  return out;
}
