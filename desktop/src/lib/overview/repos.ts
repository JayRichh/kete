// One row per repo for the overview, the rail and the attention strip,
// built from the daemon model.

import type { Model, PrInfo, Project, TerminalTab, Worktree } from '../api';
import { buildForest, sortRepos, walkAll, type Node, type Status } from './tree';
import { hasUserOrder } from '../order';

export type RepoRow = {
  project: Project;
  name: string;
  forest: Node[];
  terminals: TerminalTab[];
  worktrees: Worktree[];
  main: Worktree | undefined;
  /** Ports something listens on, across the repo's worktrees. */
  ports: number[];
  /** Pull requests by worktree branch, in any state (`state` says which). */
  prs: { branch: string; worktree: string; pr: PrInfo }[];
  /** The checkout each port listens in. */
  portOf: Record<number, string>;
};

/** Rows for every repo. `archived` lists archived sessions too (the
 *  overview's Show archived): never for the rail, strip or board, which
 *  count what wants attention. */
export function repoRows(m: Model, opts: { archived?: boolean; foldFinished?: boolean } = {}): RepoRow[] {
  const rows = m.projects.map((project): RepoRow => {
    const worktrees = m.worktrees
      .filter((w) => w.project_id === project.id)
      .sort((a, b) => Number(b.is_main) - Number(a.is_main) || a.sort_order - b.sort_order);
    const wids = new Set(worktrees.map((w) => w.id));
    const agents = m.agents.filter((a) => wids.has(a.worktree_id) && (opts.archived || !a.archived));
    const ids = new Set(agents.map((a) => a.id));
    // Sessions in other repos, so a session started from one (by the
    // orchestrator, say) says where it came from.
    const outside = new Map(m.agents.filter((a) => !wids.has(a.worktree_id)).map((a) => [a.id, a]));
    const subs = m.subagents.filter((s) => ids.has(s.parent_agent));
    const ports = [...new Set(m.ports.filter((p) => wids.has(p.worktree)).flatMap((p) => p.live))].sort((a, b) => a - b);
    const prs = worktrees.flatMap((w) => (m.prs[w.id] ? [{ branch: w.branch, worktree: w.id, pr: m.prs[w.id] }] : []));
    const portOf: Record<number, string> = {};
    for (const p of m.ports) if (wids.has(p.worktree)) for (const n of p.live) portOf[n] ??= p.worktree;
    return {
      project,
      name: project.name,
      forest: buildForest(agents, subs, outside, opts.foldFinished),
      terminals: m.terminals.filter((t) => wids.has(t.worktree_id)).sort((a, b) => a.sort_order - b.sort_order),
      worktrees,
      main: worktrees.find((w) => w.is_main) ?? worktrees[0],
      ports,
      prs,
      portOf,
    };
  });
  return orderRepos(rows);
}

/** Repos in the user's order (the rail's) once they have one, which is
 *  as soon as there are two: repos hold slots 0..n from the day they were
 *  added (order.ts). A lone repo, or a model from before slots, falls back
 *  to attention then name. */
export function orderRepos(rows: RepoRow[]): RepoRow[] {
  if (!hasUserOrder(rows.map((r) => r.project))) return sortRepos(rows);
  return [...rows].sort((a, b) => a.project.sort_order - b.project.sort_order);
}

/** Repos as the rail lists them, in the user's own order: the rail is a
 *  map, not a ranking. The digit keys count down this list. */
export function railOrder<T extends { project: { sort_order: number } }>(rows: T[]): T[] {
  return [...rows].sort((a, b) => a.project.sort_order - b.project.sort_order);
}

/** Nodes by status, across the given rows. */
export function tally(rows: RepoRow[]): Record<Status, Node[]> {
  const out = {} as Record<Status, Node[]>;
  for (const r of rows) for (const n of walkAll(r.forest)) (out[n.status] ??= []).push(n);
  return out;
}
