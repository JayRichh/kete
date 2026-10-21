// The Gauntlet view's pure logic: the backend's shapes, tones, time, filters
// and which rows a verdict's menu has. No Tauri or Svelte imports, so vitest
// loads it as is.

export type Tone = 'done' | 'failed' | 'needs_you' | 'plain';

export interface VerdictRow {
  id: string;
  kind: string;
  /** The word as written; may be "". */
  verdict: string;
  by: string | null;
  /** Often date-only ("2026-10-19"), or RFC3339. */
  at: string | null;
  note: string | null;
  tags: string[];
  /** Evidence ids. */
  evidence: string[];
  reconciled: string | null;
  path: string;
  /** Epoch seconds. */
  modified: number | null;
  tone: Tone;
  /** Repo-relative, '/'. */
  rel: string;
  reconciled_rel: string | null;
}

export interface KindRow {
  kind: string;
  department: string | null;
  /** Open in Studio applies to this kind. */
  studio: boolean;
  /** Newest first. */
  verdicts: VerdictRow[];
}

export interface GauntletList {
  root: string;
  /** The root checkout's worktree id; null greys Open in Kete and Show in Explorer. */
  worktree: string | null;
  error: string | null;
  kinds: KindRow[];
}

export interface EvidenceEntry {
  name: string;
  /** Repo-relative, '/'. */
  path: string;
  is_dir: boolean;
  bytes: number;
}

/** Why Open in Kete and Show in Explorer are greyed with no checkout. */
export const NO_CHECKOUT = 'This repo has no checkout Kete knows';

// ---- tones: a glyph beside the colour, so status never rests on colour alone ----

const GLYPHS: Record<Tone, string> = { done: '\u2713', failed: '\u2715', needs_you: '\u25cf', plain: '\u00b7' };
const LABELS: Record<Tone, string> = { done: 'done', failed: 'failed', needs_you: 'needs you', plain: 'other word' };
// Ember only where the verdict needs Jay; done and failed stay off it.
const CLASSES: Record<Tone, string> = { done: 'text-done', failed: 'text-fail', needs_you: 'text-ember-ink', plain: 'text-muted' };

export const toneGlyph = (t: Tone): string => GLYPHS[t];
export const toneLabel = (t: Tone): string => LABELS[t];
export const toneClass = (t: Tone): string => CLASSES[t];

// ---- time and filters ----

/** The verdict's moment, epoch ms: `at` (a bare date is local midnight), else
 *  the file's modified time, else null. */
export function verdictTime(v: Pick<VerdictRow, 'at' | 'modified'>): number | null {
  const at = v.at?.trim();
  if (at) {
    const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(at);
    const t = day ? new Date(+day[1], +day[2] - 1, +day[3]).getTime() : Date.parse(at);
    if (!Number.isNaN(t)) return t;
  }
  return v.modified != null ? v.modified * 1000 : null;
}

export type Age = '24h' | '7d' | 'all';
export const AGES: { id: Age; label: string }[] = [
  { id: '24h', label: '24 h' },
  { id: '7d', label: '7 days' },
  { id: 'all', label: 'All' },
];
const WINDOW: Record<Age, number> = { '24h': 24 * 3600e3, '7d': 7 * 24 * 3600e3, all: Infinity };

export interface Filter {
  /** null: every word. */
  word: string | null;
  age: Age;
}

/** The rows that pass: the word (case-insensitive) and the age. A row with no
 *  time passes only 'all'. */
export function filterVerdicts(rows: VerdictRow[], f: Filter, now: number): VerdictRow[] {
  const word = f.word?.toLowerCase() ?? null;
  return rows.filter((v) => {
    if (word !== null && v.verdict.toLowerCase() !== word) return false;
    if (f.age === 'all') return true;
    const t = verdictTime(v);
    return t !== null && now - t <= WINDOW[f.age];
  });
}

/** The distinct words of a kind, sorted; case-folded, first spelling kept. */
export function verdictWords(rows: VerdictRow[]): string[] {
  const seen = new Map<string, string>();
  for (const v of rows) if (!seen.has(v.verdict.toLowerCase())) seen.set(v.verdict.toLowerCase(), v.verdict);
  return [...seen.values()].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
}

/** A blank word, said. */
export const wordLabel = (w: string): string => (w === '' ? '(none)' : w);

/** The note's first non-empty line, trimmed. */
export function firstLine(note: string | null | undefined): string {
  return (note ?? '').split(/\r?\n/).find((l) => l.trim())?.trim() ?? '';
}

/** `at` as shown: a bare date as is, a full stamp cut to minutes. */
export function atLabel(v: Pick<VerdictRow, 'at' | 'modified'>): string {
  const at = v.at?.trim();
  return at ? at.replace('T', ' ').slice(0, 16) : '';
}

// ---- the row menu: which rows it has (menus.ts gives them their words) ----

export type MenuRowId = 'local:gauntlet.evidence' | 'local:gauntlet.studio' | 'local:gauntlet.copy_id' | 'local:gauntlet.copy_path';

/** The rows a verdict's menu has, in order. Missing Studio or evidence leaves
 *  the row out; nothing is greyed. */
export function menuRowIds(v: Pick<VerdictRow, 'evidence'>, studio: boolean): MenuRowId[] {
  return [
    ...(v.evidence.length > 0 ? (['local:gauntlet.evidence'] as const) : []),
    ...(studio ? (['local:gauntlet.studio'] as const) : []),
    'local:gauntlet.copy_id',
    'local:gauntlet.copy_path',
  ];
}

/** The evidence entry's Open in Kete: null when it can, else why not. */
export function openEvidenceWhy(e: EvidenceEntry, worktree: string | null, tooBig: number, why: string): string | null {
  if (e.is_dir) return 'A folder: show it in Explorer';
  if (!worktree) return NO_CHECKOUT;
  return e.bytes > tooBig ? why : null;
}
