// Accounts and context, as the rail and the session rows draw them: the
// pure half (tones, labels, which two bars), tested in usage.test.ts.
// The numbers come from the daemon (`nebula_core::usage`): the model's
// `accounts` / `contexts`, and `accounts_list` for the rows.

import type { AccountRow, AccountUsage, Agent, ContextUsage, UsageWindow } from '../api';
import { labelFor } from './labels.svelte';

export type Tone = 'ok' | 'watch' | 'warn' | 'hot';

/** Steps at 70, 85 and 95%: the same lines the terminal app draws
 *  (`nebula_core::usage::tone`). */
export const WATCH_AT = 70;
export const WARN_AT = 85;
export const HOT_AT = 95;

export function tone(percent: number | null | undefined): Tone {
  if (percent == null) return 'ok';
  if (percent >= HOT_AT) return 'hot';
  if (percent >= WARN_AT) return 'warn';
  if (percent >= WATCH_AT) return 'watch';
  return 'ok';
}

/** The fill class a tone paints with. */
export const FILL: Record<Tone, string> = { ok: 'bg-ink', watch: 'bg-muted', warn: 'bg-ember', hot: 'bg-fail' };

/** The account whose 5-hour window is fullest, with that window: what the
 *  header meter, the window title and the Details rail lead with. */
export function headline(accounts: Record<string, AccountUsage> | undefined, nowMs: number): { account: string; window: UsageWindow } | null {
  let best: { account: string; window: UsageWindow } | null = null;
  let bestPct = -1;
  for (const [account, u] of Object.entries(accounts ?? {})) {
    const win = u.windows.find((x) => x.kind === 'session');
    const p = win ? shownAt(win, nowMs) : null;
    if (win && p != null && p > bestPct) {
      best = { account, window: win };
      bestPct = p;
    }
  }
  return best;
}

/** `21%`, or an em dash when unknown. */
export function pct(percent: number | null | undefined): string {
  if (percent == null || Number.isNaN(percent)) return '\u2014';
  return `${Math.round(Math.max(0, Math.min(999, percent)))}%`;
}

/** A window's nominal length in seconds (`WindowKind::nominal_secs`). */
const NOMINAL: Record<UsageWindow['kind'], number> = {
  session: 5 * 3600,
  weekly: 7 * 86400,
  model_weekly: 7 * 86400,
  monthly: 30 * 86400,
};
/** How old a reading may get before it draws dim (`stale_after_secs`). */
const STALE_AFTER: Record<UsageWindow['kind'], number> = { ...NOMINAL, session: 3 * 3600 };

/** Has the window run out: past its reset, or with no reset time, its
 *  nominal length since it was read (never read: at once). */
export function expiredAt(w: UsageWindow, nowMs: number): boolean {
  const now = nowMs / 1000;
  if (w.resets_at != null && w.resets_at > 0) return w.resets_at <= now;
  const read = w.read_at ?? 0;
  return read <= 0 || read + NOMINAL[w.kind] <= now;
}

/** A window's percent as of `nowMs`: an expired one is empty. */
export function percentAt(w: UsageWindow, nowMs: number): number {
  return expiredAt(w, nowMs) ? 0 : w.percent;
}

/** What a bar shows: the percent, 0 past a known reset, unknown once a
 *  window with no reset time has aged out (`UsageWindow::shown_at`). */
export function shownAt(w: UsageWindow, nowMs: number): number | null {
  if (w.resets_at != null && w.resets_at > 0) return percentAt(w, nowMs);
  return expiredAt(w, nowMs) ? null : w.percent;
}

/** Is the reading too old to trust at a glance (drawn dim and hatched)? */
export function staleAt(w: UsageWindow, nowMs: number): boolean {
  const read = w.read_at ?? 0;
  return read <= 0 || nowMs / 1000 - read > STALE_AFTER[w.kind];
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
    const l = labelFor(w);
    const reset = l && l.resets ? `, ${l.resets} ${l.countdown ?? ''}`.trimEnd() : '';
    const read = staleAt(w, nowMs) ? (w.read_at > 0 ? `, read ${ago(w.read_at * 1000, nowMs)} (stale)` : ', read time unknown') : '';
    return `${name}: ${pct(shownAt(w, nowMs))}${reset}${read}`;
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

/** Harnesses whose own login an account row stands for (the ones with a
 *  known sign-in, `profiles::login_args`). */
const TRACKED = new Set(['claude', 'codex', 'copilot']);

/** The harness a session runs: a custom harness's id, else its kind. */
export function sessionHarness(a: Pick<Agent, 'kind' | 'custom_harness'>): string {
  return a.kind === 'custom' && a.custom_harness ? a.custom_harness : a.kind;
}

/** Harnesses with a live (unarchived) session on their own login: no profile. */
export function ownLoginSessionHarnesses(agents: Agent[]): string[] {
  return [...new Set(agents.filter((a) => !a.archived && !a.profile?.trim()).map(sessionHarness))];
}

/** The unarchived sessions running under an account row, in model order
 *  (`nebula_core::accounts::sessions_by_account`). */
export function sessionsOn(row: Pick<AccountRow, 'key'>, agents: Agent[]): Agent[] {
  return agents.filter((a) => !a.archived && accountKey(a.profile, sessionHarness(a)) === row.key);
}

/** The rail's rows: the listed ones, plus a harness's own login that a
 *  live reading or a session on it brought in since the list was read.
 *  One rule with `accounts::own_login_harnesses`: an own login shows while
 *  it is in use, whatever profiles its harness has. */
export function mergeRows(
  listed: AccountRow[],
  live: Record<string, AccountUsage>,
  ownSessionHarnesses: string[],
): AccountRow[] {
  const out = [...listed];
  const have = new Set(listed.map((r) => r.key));
  const own = new Set<string>();
  for (const key of Object.keys(live)) if (key.startsWith('@')) own.add(key.slice(1));
  for (const h of ownSessionHarnesses) if (TRACKED.has(h)) own.add(h);
  for (const h of [...own].sort()) {
    if (have.has(`@${h}`)) continue;
    out.push({
      key: `@${h}`,
      name: h,
      harness: h,
      profile: false,
      default: false,
      flaggedDefault: false,
      disabled: false,
      useDefault: true,
      login: 'unknown',
      loginHint: null,
      usage: null,
    });
  }
  return out;
}

/** What the row's login state means, said plainly: kete only looks for
 *  the credentials, it never tries them. */
export function loginTitle(row: Pick<AccountRow, 'login' | 'expired' | 'loginHint'>): string {
  if (row.login === 'logged_in') return 'Signed in (credentials found; not checked against the service)';
  if (row.login === 'needs_login') {
    const why = row.expired ? 'Login expired: the credentials say it ran out' : 'Not signed in: no credentials found';
    return row.loginHint ? `${why}. Sign in with: ${row.loginHint}` : why;
  }
  return 'Sign-in state unknown: kete knows no credentials file for this harness';
}

/** The star's words: in force, flagged but off, or not the default. */
export function starTitle(row: Pick<AccountRow, 'name' | 'harness' | 'default' | 'flaggedDefault' | 'disabled'>): string {
  if (row.default) return `${row.name} is the ${row.harness} default`;
  if (row.flaggedDefault && row.disabled)
    return `${row.name} is the ${row.harness} default (disabled): while it is off, launches naming no profile pass it over. Switch it on to use it`;
  return `Make ${row.name} the ${row.harness} default`;
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
