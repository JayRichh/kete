// The facts the session view's side rail shows for one session: where it
// runs, how full its context is, what its checkout has changed, the model
// and effort it was launched with, and how its children stand. Pure, so
// the rules are tested without a window.

import type { Agent, ContextUsage, Model } from './api';
import { placeOf, subagentTone, toneOf } from './status';
import type { WorktreeGit } from './worktrees/api';

export type RailPlace =
  | { kind: 'local'; project: string; branch: string; path: string }
  | { kind: 'cloud'; url: string };

/** A session's children by how they stand: `kete spawn` sessions it
 *  started, and its Claude subagents. */
export type ChildCounts = { working: number; needs: number; done: number };

export type RailFacts = {
  place: RailPlace;
  context: ContextUsage | null;
  /** Changed and untracked files in its checkout; null until read. */
  changed: number | null;
  model: string | null;
  effort: string | null;
  children: ChildCounts;
};

/** Where a session runs: its Claude Cloud session when it has one (the
 *  page the CLI printed), else its local checkout. */
export function railPlace(m: Model, a: Agent): RailPlace {
  if (a.cloud_session_id) return { kind: 'cloud', url: `https://claude.ai/code/${a.cloud_session_id}` };
  const p = placeOf(m, a.worktree_id);
  return { kind: 'local', project: p.project, branch: p.branch, path: p.path };
}

export function childCounts(m: Model, parent: string): ChildCounts {
  const out: ChildCounts = { working: 0, needs: 0, done: 0 };
  for (const a of m.agents) {
    if (a.parent !== parent || a.id === parent || a.archived) continue;
    const t = toneOf(a);
    if (t === 'running') out.working += 1;
    else if (t === 'needs') out.needs += 1;
    else if (t === 'done' || t === 'unread' || t === 'stopped') out.done += 1;
  }
  for (const s of m.subagents) {
    if (s.parent_agent !== parent) continue;
    if (subagentTone(s) === 'running') out.working += 1;
    else out.done += 1;
  }
  return out;
}

export function railFacts(m: Model, a: Agent, git: Record<string, WorktreeGit>): RailFacts {
  const g = git[a.worktree_id];
  return {
    place: railPlace(m, a),
    context: m.contexts?.[a.id] ?? null,
    changed: a.cloud_session_id || !g ? null : g.changed + g.untracked,
    model: a.model?.trim() || null,
    effort: a.effort?.trim() || null,
    children: childCounts(m, a.id),
  };
}

/** `3 changed files`, `1 changed file`, `No changes`. */
export function changedWords(n: number): string {
  if (n === 0) return 'No changes';
  return `${n} changed ${n === 1 ? 'file' : 'files'}`;
}
