// The checkouts sessions last ran in, newest first: the quick prompt's
// Recent row, one click each. The terminal app's quick prompt lists the
// same ones (nebula-tui `recent_checkouts`).

import type { Model } from './api';

export interface RecentCheckout {
  worktree: string;
  project: string;
  /** `repo / branch`. */
  label: string;
  /** The newest status change of a session there, epoch ms. */
  at: number;
}

/** How many the Recent row offers. */
export const RECENT_KEPT = 5;

/** Per checkout, the newest `status_changed_at` of its sessions (archived
 *  ones count: they were started there), newest first. Linked checkouts with
 *  no session yet come last, at 0. Checkouts and repos
 *  gone from the model drop out. */
export function recentCheckouts(m: Model, limit = RECENT_KEPT): RecentCheckout[] {
  const newest = new Map<string, number>();
  for (const a of m.agents) newest.set(a.worktree_id, Math.max(newest.get(a.worktree_id) ?? 0, a.status_changed_at));
  // A linked checkout nobody has run a session in yet (an agent cut it, or
  // it was adopted from git) is still somewhere to start one: it follows
  // the ones that have history.
  for (const w of m.worktrees) if (!w.is_main && !newest.has(w.id)) newest.set(w.id, 0);
  const out: RecentCheckout[] = [];
  for (const [id, at] of newest) {
    const w = m.worktrees.find((x) => x.id === id);
    const p = w && m.projects.find((x) => x.id === w.project_id);
    if (!w || !p) continue;
    out.push({ worktree: w.id, project: p.id, label: `${p.name} / ${w.branch}`, at });
  }
  return out.sort((a, b) => b.at - a.at || a.label.localeCompare(b.label)).slice(0, limit);
}
