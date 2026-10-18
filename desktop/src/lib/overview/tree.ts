// The overview's session trees: sessions nest under the session that
// spawned them (`parent`), Claude's subagents under the session they run
// inside. Pure, so the ordering rules are tested without a window.

import type { Agent, Subagent } from '../api';
import { toneOf, subagentTone, type Tone } from '../status';
import { subagentRun } from '../subagents';

export type Status = Tone;

/** What a subagent row says beside its name: `closed` for one still open
 *  when its session ended or restarted, its type unless the name already
 *  is it, then the model and effort it runs at when known, the model
 *  without its `claude-` prefix. The terminal app's rows say the same
 *  (`nebula-tui/src/subagents.rs`, `sub_meta`). */
export function subagentMeta(s: Subagent): string {
  const closedWord = s.status === 'ended_unknown' ? 'closed' : '';
  const kind = s.description ? s.kind : '';
  return [closedWord, kind, subagentRun(s)].filter(Boolean).join(' \u00b7 ');
}

export type Node = {
  id: string;
  /** `finished`: the FINISHED GROUP, a session's ended subagents under one
   *  `N finished` row that starts folded. */
  kind: 'session' | 'subagent' | 'finished';
  status: Status;
  label: string;
  children: Node[];
  depth: number;
  /** Unique across the forest (subagent ids repeat across sessions). */
  key: string;
  agent?: Agent;
  sub?: Subagent;
  /** A root whose parent session lives in another repo (the orchestrator,
   *  most often): drawn with a "from <parent>" cue that opens it, rather
   *  than as a root that came from nowhere. */
  from?: Agent;
  /** Starts folded whatever its depth: the FINISHED GROUP. */
  folded?: boolean;
};

/** Children deeper than this start collapsed. */
export const OPEN_DEPTH = 2;

const RANK: Partial<Record<Status, number>> = { needs: 0, failed: 1, unread: 2, running: 3, done: 4 };
/** Rank of a status the strip does not count (idle, stopped, a shell). */
const QUIET = 5;
/** Rank of a repo with no sessions at all. */
const EMPTY = 6;

function ownRank(n: Node): number {
  return RANK[n.status] ?? QUIET;
}

/** needs-you 0, failed 1, unread 2, running 3, done 4: the most urgent in the subtree. */
export function attentionRank(n: Node): number {
  let r = ownRank(n);
  for (const c of n.children) r = Math.min(r, attentionRank(c));
  return r;
}

function forestRank(forest: Node[]): number {
  return forest.reduce((r, n) => Math.min(r, attentionRank(n)), EMPTY);
}

/**
 * `agents` nested by `parent`, subagents under the session they run in.
 * `outside`: sessions that are not in `agents` (another repo's), so a
 * root whose parent is one of them carries it as `from`. `foldFinished`
 * (the `hide_done_subagents` setting) puts each session's ended subagents
 * under one FINISHED GROUP node, `3 finished`, that starts folded; running
 * ones stay one by one.
 */
export function buildForest(agents: Agent[], subs: Subagent[], outside?: Map<string, Agent>, foldFinished = false): Node[] {
  const byId = new Map(agents.map((a) => [a.id, a]));
  const nodes = new Map<string, Node>();
  for (const a of agents) {
    nodes.set(a.id, {
      id: a.id,
      kind: 'session',
      // An archived session (shown on request) wants nothing.
      status: a.archived ? 'stopped' : toneOf(a),
      label: a.name,
      children: [],
      depth: 0,
      key: `a:${a.id}`,
      agent: a,
    });
  }

  /** Whether `a`'s parent chain reaches a real root without looping. */
  function attachable(a: Agent): boolean {
    const seen = new Set([a.id]);
    let p = a.parent;
    while (p) {
      if (seen.has(p)) return false;
      seen.add(p);
      const up = byId.get(p);
      if (!up) return true;
      p = up.parent;
    }
    return true;
  }

  const roots: Node[] = [];
  // The user's order, else oldest status change first (order.ts).
  const ordered = [...agents].sort((x, y) => x.sort_order - y.sort_order || x.status_changed_at - y.status_changed_at);
  for (const a of ordered) {
    const n = nodes.get(a.id)!;
    const parent = a.parent && a.parent !== a.id ? nodes.get(a.parent) : undefined;
    if (parent && attachable(a)) parent.children.push(n);
    else {
      const away = a.parent && !parent ? outside?.get(a.parent) : undefined;
      if (away && away.id !== a.id) n.from = away;
      roots.push(n);
    }
  }

  const orderedSubs = [...subs].sort((x, y) => x.started_at - y.started_at);
  const groups = new Map<string, Node>();
  for (const s of orderedSubs) {
    const n: Node = {
      id: s.id,
      kind: 'subagent',
      status: subagentTone(s),
      label: s.description || s.kind || 'subagent',
      children: [],
      depth: 0,
      key: `s:${s.parent_agent}:${s.id}`,
      sub: s,
    };
    const parent = nodes.get(s.parent_agent);
    if (!parent) {
      roots.push(n);
      continue;
    }
    if (foldFinished && s.status !== 'running') {
      let group = groups.get(s.parent_agent);
      if (!group) {
        group = { id: `f:${s.parent_agent}`, kind: 'finished', status: 'done', label: '', children: [], depth: 0, key: `f:${s.parent_agent}`, folded: true };
        groups.set(s.parent_agent, group);
      }
      group.children.push(n);
    } else parent.children.push(n);
  }
  for (const [parentId, group] of groups) {
    group.label = `${group.children.length} finished`;
    nodes.get(parentId)!.children.push(group);
  }

  const setDepth = (n: Node, d: number) => {
    n.depth = d;
    for (const c of n.children) setDepth(c, d + 1);
  };
  for (const r of roots) setDepth(r, 0);
  return roots;
}

export type Row = {
  node: Node;
  /** Whether its children are showing. */
  expanded: boolean;
  /** Descendants hidden by a collapse, for "+ 7 agents". */
  hidden: number;
};

function count(n: Node): number {
  return n.children.reduce((t, c) => t + 1 + count(c), 0);
}

/**
 * The rows to draw, depth first. A node's children show while they sit at
 * depth OPEN_DEPTH or shallower, unless the user opened (`expanded`) or
 * closed (`collapsed`) that node by id.
 */
export function flatten(forest: Node[], expanded: Set<string>, collapsed: Set<string> = new Set()): Row[] {
  const out: Row[] = [];
  const walk = (n: Node) => {
    const has = n.children.length > 0;
    const open = has && !collapsed.has(n.id) && (expanded.has(n.id) || (!n.folded && n.depth + 1 <= OPEN_DEPTH));
    out.push({ node: n, expanded: open, hidden: has && !open ? count(n) : 0 });
    if (open) for (const c of n.children) walk(c);
  };
  for (const r of forest) walk(r);
  return out;
}

/** Every node, depth first. */
export function walkAll(forest: Node[]): Node[] {
  const out: Node[] = [];
  const walk = (n: Node) => {
    out.push(n);
    n.children.forEach(walk);
  };
  forest.forEach(walk);
  return out;
}

/** Repo rows by attention (a needs-you anywhere floats first), then name. */
export function sortRepos<T extends { name: string; forest: Node[] }>(rows: T[]): T[] {
  return rows
    .map((r) => ({ r, rank: forestRank(r.forest) }))
    .sort((a, b) => a.rank - b.rank || a.r.name.localeCompare(b.r.name))
    .map((x) => x.r);
}

/** A parent row's descendants by state, for its rollup and the session
 *  rail's children: `working` the running ones, `needs` those waiting on
 *  you or ended on an error, `done` the finished ones, read or not, and
 *  those that stopped. The ended subagents a FINISHED GROUP folds all count
 *  as done; the group row itself is not one of them. */
export type Rollup = { working: number; needs: number; done: number };

export function rollup(n: Node): Rollup {
  const r: Rollup = { working: 0, needs: 0, done: 0 };
  const walk = (d: Node) => {
    if (d.kind === 'finished') r.done += d.children.length;
    else {
      if (d.status === 'running') r.working += 1;
      else if (d.status === 'needs' || d.status === 'failed') r.needs += 1;
      else if (d.status === 'done' || d.status === 'unread' || d.status === 'stopped') r.done += 1;
      d.children.forEach(walk);
    }
  };
  n.children.forEach(walk);
  return r;
}

/** `2 working \u00b7 1 needs you \u00b7 3 done`, the empty counts left out; empty
 *  when no descendant is in any of the three. */
export function rollupLabel(r: Rollup): string {
  const parts: string[] = [];
  if (r.working) parts.push(`${r.working} working`);
  if (r.needs) parts.push(`${r.needs} needs you`);
  if (r.done) parts.push(`${r.done} done`);
  return parts.join(' \u00b7 ');
}
