// The studio board's join, pure: the file half the Rust side reads
// (`studio_board`) with the live sessions, roles and spend the store holds.
// It mirrors `crates/nebula-core/src/studio/board.rs` `build`; a rule
// changed there changes here. No Svelte and no Tauri, so vitest reads it.

import type { Agent, ContextUsage, Worktree } from '../../api';
import type { SessionRole, SpendRow } from '../types';

// ---- the reply, as the Rust side sends it ----

export interface Department {
  slug: string;
  label: string;
}

export interface BoardNote {
  id: string;
  path: string;
  title: string;
  status: 'New' | 'Open' | string | { Other: string };
  to: string | null;
  about: string | null;
  priority: 'High' | 'Normal' | 'Low';
  at: string | null;
  by: string | null;
  /** Repo-relative. */
  rel: string;
}

export interface Verdict {
  id: string;
  kind: string;
  verdict: string;
  by: string | null;
  at: string | null;
  note: string | null;
  tags: string[];
  evidence: string[];
  reconciled: string | null;
  path: string;
  /** Epoch seconds. */
  modified: number | null;
}

export interface Refusal {
  /** Epoch ms. */
  at: number;
  project: string;
  agent: string | null;
  role: string | null;
  reason: string;
}

export interface FailedRenewal {
  at: number | null;
  old: string | null;
  reason: string;
}

export interface BoardReply {
  /** The repo root, '/'-separated. */
  root: string;
  /** The repo has `.kete/studio.toml`. */
  manifest: boolean;
  error: string | null;
  departments: Department[];
  notes: BoardNote[];
  verdicts: Record<string, Verdict>;
  refusals: Refusal[];
  renewal: FailedRenewal | null;
  /** The repo's main checkout worktree id. */
  worktree: string | null;
}

// ---- the join's output ----

export const UNASSIGNED = 'unassigned';
export const BUDGET_AMBER = 60;
export const BUDGET_RED = 90;

export type BudgetLevel = 'ok' | 'amber' | 'red';

export interface Cell {
  agent: Agent;
  /** The role's label, else the session's name. */
  label: string;
  model: string | null;
  profile: string | null;
  /** Context tokens now; null without a reading. */
  contextTokens: number | null;
  /** Context against the role's budget; null without a budget or a reading. */
  percent: number | null;
  /** The role's tier; undefined for a session with no role. */
  tier: number | undefined;
}

export interface Spend {
  tokens: number;
  cost_usd: number;
}

export interface Column {
  slug: string;
  label: string;
  queue: BoardNote[];
  lead: Cell | null;
  runners: Cell[];
  verdict: Verdict | null;
  blocked: number;
  spend: Spend;
}

export type NeedsYou =
  | { kind: 'session'; cell: Cell }
  | { kind: 'note'; note: BoardNote }
  | { kind: 'refusal'; refusal: Refusal }
  | { kind: 'renewal'; renewal: FailedRenewal };

export interface Board {
  needsYou: NeedsYou[];
  columns: Column[];
}

export interface Inputs {
  reply: BoardReply;
  agents: Agent[];
  worktrees: Worktree[];
  contexts: Record<string, ContextUsage>;
  roles: Record<string, SessionRole>;
  spend: Record<string, SpendRow>;
  /** The project id. */
  repo: string;
  /** Local date, `YYYY-MM-DD`. */
  today: string;
}

// ---- rules ----

/** Lowercase; each run of chars outside a-z and 0-9 becomes one '-'; none at either end. */
export function slug(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

export function budgetLevel(pct: number): BudgetLevel {
  return pct >= BUDGET_RED ? 'red' : pct >= BUDGET_AMBER ? 'amber' : 'ok';
}

/** Whether a note is to `who`. */
const toIs = (n: BoardNote, who: string) => n.to != null && slug(n.to) === who;

const PRIORITY: Record<string, number> = { High: 0, Normal: 1, Low: 2 };
// Plain code-unit order, as Rust compares strings.
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** High first, then dated before undated, then oldest, then id. */
export function byQueue(a: BoardNote, b: BoardNote): number {
  return (
    (PRIORITY[a.priority] ?? 1) - (PRIORITY[b.priority] ?? 1) ||
    Number(a.at == null) - Number(b.at == null) ||
    cmp(a.at ?? '', b.at ?? '') ||
    cmp(a.id, b.id)
  );
}

/** The repo's live sessions: not archived, alive, in one of its worktrees. */
export function liveSessions(agents: Agent[], worktrees: Worktree[], repo: string): Agent[] {
  const mine = new Set(worktrees.filter((w) => w.project_id === repo).map((w) => w.id));
  return agents.filter((a) => !a.archived && a.alive && mine.has(a.worktree_id));
}

function cellOf(a: Agent, i: Inputs): Cell {
  const role = i.roles[a.id];
  const tokens = i.contexts[a.id]?.tokens ?? null;
  const budget = role?.context_budget ?? null;
  return {
    agent: a,
    label: role?.label ?? a.name,
    model: a.model,
    profile: a.profile,
    contextTokens: tokens,
    percent: tokens != null && budget != null && budget > 0 ? Math.floor((tokens * 100) / budget) : null,
    tier: role?.tier,
  };
}

const sum = (cells: Cell[], i: Inputs): Spend =>
  cells.reduce<Spend>(
    (s, c) => {
      const r = i.spend[c.agent.id];
      return r && r.day === i.today ? { tokens: s.tokens + r.input_tokens + r.output_tokens, cost_usd: s.cost_usd + r.cost_usd } : s;
    },
    { tokens: 0, cost_usd: 0 },
  );

export function joinBoard(i: Inputs): Board {
  const { reply } = i;
  const live = liveSessions(i.agents, i.worktrees, i.repo);
  const known = new Set(reply.departments.map((d) => d.slug));
  const byDept = new Map<string, Cell[]>();
  for (const a of live) {
    const dept = i.roles[a.id]?.department;
    const s = dept != null ? slug(dept) : '';
    const key = known.has(s) ? s : UNASSIGNED;
    byDept.set(key, [...(byDept.get(key) ?? []), cellOf(a, i)]);
  }

  const depts = [...reply.departments];
  if (byDept.has(UNASSIGNED)) depts.push({ slug: UNASSIGNED, label: 'Unassigned' });

  const columns = depts.map((d): Column => {
    const un = d.slug === UNASSIGNED;
    const cells = [...(byDept.get(d.slug) ?? [])].sort((a, b) => (a.tier ?? Infinity) - (b.tier ?? Infinity) || cmp(a.label, b.label));
    const at = un ? -1 : cells.findIndex((c) => c.tier === 1 || c.tier === 2);
    const lead = at >= 0 ? cells.splice(at, 1)[0] : null;
    const all = lead ? [lead, ...cells] : cells;
    const toJay = reply.notes.filter((n) => toIs(n, 'jay') && n.about != null && slug(n.about) === d.slug).length;
    const waiting = all.filter((c) => c.agent.status === 'needs_feedback').length;
    return {
      slug: d.slug,
      label: d.label,
      queue: un ? [] : reply.notes.filter((n) => toIs(n, d.slug)).sort(byQueue),
      lead,
      runners: cells,
      verdict: reply.verdicts[d.slug] ?? null,
      blocked: toJay + waiting,
      spend: sum(all, i),
    };
  });

  const needsYou: NeedsYou[] = [
    ...live.filter((a) => a.status === 'needs_feedback').map((a): NeedsYou => ({ kind: 'session', cell: cellOf(a, i) })),
    ...reply.notes.filter((n) => toIs(n, 'jay')).map((note): NeedsYou => ({ kind: 'note', note })),
    ...reply.refusals.map((refusal): NeedsYou => ({ kind: 'refusal', refusal })),
    ...(reply.renewal ? [{ kind: 'renewal', renewal: reply.renewal } as NeedsYou] : []),
  ];
  return { needsYou, columns };
}

// ---- wording ----

/** A note's or verdict's `at`, as epoch ms: a bare date is local midnight. */
export function atMs(at: string | null | undefined, modifiedSecs?: number | null): number | null {
  const s = at?.trim();
  if (s) {
    const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
    const t = day ? new Date(+day[1], +day[2] - 1, +day[3]).getTime() : Date.parse(s);
    if (!Number.isNaN(t)) return t;
  }
  return modifiedSecs != null ? modifiedSecs * 1000 : null;
}

/** Glyphs for a session's tone, beside the dot: status never rests on colour. */
export const TONE_GLYPH: Record<string, string> = {
  needs: '!',
  failed: '\u2715',
  unread: '\u25cf',
  running: '\u25b6',
  done: '\u2713',
  merged: '\u2713',
  pr: '\u25cb',
  idle: '\u25cb',
  stopped: '\u25a0',
  shell: '\u25cb',
};

/** `64% of budget` is for the title; this is the cell's number. */
export function contextWord(c: Cell): string {
  if (c.percent != null) return `${c.percent}%`;
  return c.contextTokens != null ? `${Math.round(c.contextTokens / 1000)}k` : 'no reading';
}

/** `12k`, `1.5M`, or the whole number under a thousand. */
export function tokensWord(n: number): string {
  if (n >= 999_500) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}
