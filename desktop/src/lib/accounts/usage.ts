// Accounts and context, as the rail and the session rows draw them: the
// pure half (tones, labels, which two bars), tested in usage.test.ts.
// The numbers come from the daemon (`nebula_core::usage`): the model's
// `accounts` / `contexts`, and `accounts_list` for the rows.

import type { AccountRow, AccountUsage, ContextUsage, UsageWindow } from '../api';

export type Tone = 'ok' | 'warn' | 'hot';

/** Ember from 80%, red from 95%: the same lines the terminal app draws. */
export const WARN_AT = 80;
export const HOT_AT = 95;

export function tone(percent: number | null | undefined): Tone {
  if (percent == null) return 'ok';
  if (percent >= HOT_AT) return 'hot';
  if (percent >= WARN_AT) return 'warn';
  return 'ok';
}

/** The fill class a tone paints with. */
export const FILL: Record<Tone, string> = { ok: 'bg-ink', warn: 'bg-ember', hot: 'bg-fail' };

/** `21%`, or an em dash when unknown. */
export function pct(percent: number | null | undefined): string {
  if (percent == null || Number.isNaN(percent)) return '\u2014';
  return `${Math.round(Math.max(0, Math.min(999, percent)))}%`;
}

/** A window's percent as of `nowMs`: past its reset it is empty. */
export function percentAt(w: UsageWindow, nowMs: number): number {
  return w.resets_at != null && w.resets_at > 0 && w.resets_at * 1000 <= nowMs ? 0 : w.percent;
}

/** Width of a bar's fill, 0-100; any use at all shows a sliver. */
export function fillWidth(percent: number | null | undefined): number {
  if (percent == null || percent <= 0) return 0;
  return Math.max(2, Math.min(100, percent));
}

/** The two bars a row draws: session (5h) then weekly; an account with
 *  neither (Copilot) shows its monthly allowances instead. */
export function bars(u: AccountUsage | null | undefined): [UsageWindow | null, UsageWindow | null] {
  if (!u) return [null, null];
  const find = (kind: UsageWindow['kind']) => u.windows.find((w) => w.kind === kind) ?? null;
  const session = find('session');
  const weekly = find('weekly');
  if (!session && !weekly) {
    const monthly = u.windows.filter((w) => w.kind === 'monthly');
    return [monthly[0] ?? null, monthly[1] ?? null];
  }
  return [session, weekly];
}

/** A bar's caption: `5h`, `wk`, or the allowance's label (`premium`). */
export function windowLabel(w: UsageWindow | null, slot: 0 | 1): string {
  if (!w) return slot === 0 ? '5h' : 'wk';
  // A caption is 40px of 10px mono: long allowance names are cut to four.
  if (w.label) return w.label.length > 5 ? w.label.slice(0, 4) : w.label;
  return { session: '5h', weekly: 'wk', model_weekly: 'model wk', monthly: 'mo' }[w.kind];
}

const SOURCE: Record<string, string> = {
  statusline: 'Claude status line',
  transcript: 'Claude transcript',
  rollout: 'Codex rollout',
  copilot_api: 'GitHub Copilot quota',
  copilot_events: 'Copilot session log',
  screen: 'usage notice on screen',
};
export const sourceLabel = (s: string) => SOURCE[s] ?? s;

/** `in 3h 12m`, `in 2d 4h`, `now`. */
export function resetsIn(resetsAtSecs: number, nowMs: number): string {
  const left = Math.floor(resetsAtSecs - nowMs / 1000);
  if (left <= 0) return 'now';
  const d = Math.floor(left / 86400);
  const h = Math.floor((left % 86400) / 3600);
  const m = Math.floor((left % 3600) / 60);
  if (d > 0) return `in ${d}d ${h}h`;
  if (h > 0) return `in ${h}h ${m}m`;
  return `in ${Math.max(1, m)}m`;
}

/** `3m ago`, for "last update". */
export function ago(ms: number, nowMs: number): string {
  const s = Math.max(0, Math.floor((nowMs - ms) / 1000));
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** The account's tooltip: each window, where the numbers came from, when. */
export function usageTitle(row: Pick<AccountRow, 'name' | 'harness' | 'profile'>, u: AccountUsage | null, nowMs: number): string {
  const head = row.profile ? `${row.name} (${row.harness})` : `${row.harness}: the CLI's own login`;
  if (!u || !u.windows.length) return `${head}\nNo usage read yet: it shows after the account's next turn.`;
  const lines = u.windows.map((w) => {
    const name = w.label ? `${w.label} ${w.kind === 'monthly' ? 'monthly' : 'weekly'}` : { session: '5-hour', weekly: 'weekly', model_weekly: 'model weekly', monthly: 'monthly' }[w.kind];
    const reset = w.resets_at ? `, resets ${resetsIn(w.resets_at, nowMs)}` : '';
    return `${name}: ${pct(percentAt(w, nowMs))}${reset}`;
  });
  const plan = u.plan ? ` \u00b7 plan ${u.plan}` : '';
  return [head, ...lines, `From the ${sourceLabel(u.source)}${plan}, ${ago(u.at, nowMs)}`].join('\n');
}

/** The account a session counts against: its profile, or `@<harness>`. */
export function accountKey(profile: string | null | undefined, harness: string): string {
  const p = profile?.trim();
  return p ? p : `@${harness.trim()}`;
}

/** The live reading for a row: the daemon's push over the list's copy. */
export function liveUsage(row: AccountRow, live: Record<string, AccountUsage>): AccountUsage | null {
  return live[row.key] ?? row.usage ?? null;
}

/** Harnesses whose own login an account row stands for. */
const TRACKED = new Set(['claude', 'codex', 'copilot']);

/** The rail's rows: the listed ones, plus a harness's own login that a
 *  live reading or a session brought in since the list was read (and that
 *  no profile of that harness covers). */
export function mergeRows(
  listed: AccountRow[],
  live: Record<string, AccountUsage>,
  sessionHarnesses: string[],
): AccountRow[] {
  const out = [...listed];
  const covered = new Set(listed.map((r) => r.harness));
  const own = new Set<string>();
  for (const key of Object.keys(live)) if (key.startsWith('@')) own.add(key.slice(1));
  for (const h of sessionHarnesses) if (TRACKED.has(h)) own.add(h);
  for (const h of [...own].sort()) {
    if (covered.has(h)) continue;
    out.push({
      key: `@${h}`,
      name: h,
      harness: h,
      profile: false,
      default: false,
      disabled: false,
      useDefault: true,
      login: 'unknown',
      loginHint: null,
      usage: null,
    });
  }
  return out;
}

/** A context's tooltip. */
export function contextTitle(c: ContextUsage, nowMs: number): string {
  const tokens = c.tokens != null && c.window ? ` (${Math.round(c.tokens / 1000)}k of ${Math.round(c.window / 1000)}k tokens)` : '';
  const lines = [`Context ${pct(c.percent)}${tokens}`];
  if (c.compactions > 0) {
    const last = c.last_compact_at != null ? `, last ${ago(c.last_compact_at, nowMs)}` : '';
    lines.push(`Compacted ${c.compactions} ${c.compactions === 1 ? 'time' : 'times'}${last}`);
  }
  lines.push(`From the ${sourceLabel(c.source)}, ${ago(c.at, nowMs)}`);
  return lines.join('\n');
}
