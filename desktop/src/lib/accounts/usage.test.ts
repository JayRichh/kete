import { describe, expect, it } from 'vitest';
import type { AccountRow, AccountUsage, Agent, UsageWindow } from '../api';
import { setLabels } from './labels.svelte';
import type { UsageLabel } from '../api';
import {
  accountKey,
  bars,
  contextTitle,
  fillWidth,
  liveUsage,
  loginTitle,
  mergeRows,
  ownLoginSessionHarnesses,
  pct,
  sessionsOn,
  starTitle,
  percentAt,
  headline,
  shownAt,
  staleAt,
  tone,
  usageTitle,
  windowLabel,
} from './usage';

const w = (kind: UsageWindow['kind'], percent: number, extra: Partial<UsageWindow> = {}): UsageWindow => ({
  kind,
  label: null,
  percent,
  resets_at: null,
  read_at: 1,
  ...extra,
});
const usage = (windows: UsageWindow[], extra: Partial<AccountUsage> = {}): AccountUsage => ({
  account: '@claude',
  harness: 'claude',
  windows,
  source: 'statusline',
  at: 0,
  plan: null,
  ...extra,
});

/** A label as the Rust formatter would hand it, for the parts a test reads. */
const label = (compact: string, resets: string, countdown: string): UsageLabel => ({
  percent: 34,
  step: 0,
  clock: resets.replace('resets ', ''),
  countdown,
  start: null,
  iso: null,
  estimated: false,
  compact,
  long: compact,
  resets,
  hover: '',
  banner: null,
});

describe('bar thresholds', () => {
  it('steps at 70, 85 and 95', () => {
    expect(tone(0)).toBe('ok');
    expect(tone(69.9)).toBe('ok');
    expect(tone(70)).toBe('watch');
    expect(tone(84.9)).toBe('watch');
    expect(tone(85)).toBe('warn');
    expect(tone(94.9)).toBe('warn');
    expect(tone(95)).toBe('hot');
    expect(tone(120)).toBe('hot');
    expect(tone(null)).toBe('ok');
  });
  it('fills a sliver for any use and nothing for none or unknown', () => {
    expect(fillWidth(null)).toBe(0);
    expect(fillWidth(0)).toBe(0);
    expect(fillWidth(0.3)).toBe(2);
    expect(fillWidth(50)).toBe(50);
    expect(fillWidth(140)).toBe(100);
  });
});

describe('unknown values', () => {
  it('read as a dash', () => {
    expect(pct(null)).toBe('\u2014');
    expect(pct(undefined)).toBe('\u2014');
    expect(pct(NaN)).toBe('\u2014');
    expect(pct(21.4)).toBe('21%');
  });
  it('a window past its reset is empty', () => {
    expect(percentAt(w('session', 90, { resets_at: 100 }), 99_000)).toBe(90);
    expect(percentAt(w('session', 90, { resets_at: 100 }), 100_000)).toBe(0);
  });
  it('no reading says so in the tooltip', () => {
    expect(usageTitle({ name: 'claude', harness: 'claude', profile: false }, null, 0)).toContain('No usage read yet');
  });
});

describe('stale and expired readings', () => {
  const H = 3600_000;
  it('a window with no reset ages out after its nominal length', () => {
    const s = w('session', 50, { read_at: 1000 });
    expect(percentAt(s, 1000_000 + 5 * H - 1)).toBe(50);
    expect(percentAt(s, 1000_000 + 5 * H)).toBe(0);
    expect(shownAt(s, 1000_000 + 5 * H)).toBeNull();
    expect(shownAt(w('weekly', 9, { read_at: 0 }), 0)).toBeNull();
    expect(shownAt(w('session', 9, { resets_at: 10, read_at: 1 }), 20_000)).toBe(0);
  });
  it('draws a session reading dim after three hours, a weekly one after its week', () => {
    expect(staleAt(w('session', 1, { read_at: 1000 }), 1000_000 + 3 * H)).toBe(false);
    expect(staleAt(w('session', 1, { read_at: 1000 }), 1000_000 + 3 * H + 1000)).toBe(true);
    expect(staleAt(w('weekly', 1, { read_at: 1000 }), 1000_000 + 24 * H)).toBe(false);
    expect(staleAt(w('weekly', 1, { read_at: 0 }), 0)).toBe(true);
  });
  it('says a stale reading is stale in the tooltip', () => {
    const t = usageTitle({ name: 'claude', harness: 'claude', profile: false }, usage([w('session', 40, { read_at: 1000 })]), 1000_000 + 4 * H);
    expect(t).toContain('read 4h ago (stale)');
  });
});

describe('which two bars', () => {
  it('session then weekly', () => {
    const [a, b] = bars(usage([w('weekly', 82), w('session', 34)]));
    expect(a?.kind).toBe('session');
    expect(b?.kind).toBe('weekly');
    expect(windowLabel(a, 0)).toBe('5h');
    expect(windowLabel(b, 1)).toBe('wk');
  });
  it('a lone window leaves the other slot empty', () => {
    const [a, b] = bars(usage([w('weekly', 82)]));
    expect(a).toBeNull();
    expect(b?.percent).toBe(82);
    expect(windowLabel(a, 0)).toBe('5h');
  });
  it('monthly allowances when there is no 5h or weekly (Copilot)', () => {
    const [a, b] = bars(usage([w('monthly', 10, { label: 'premium' }), w('monthly', 1, { label: 'chat' })], { source: 'copilot_api' }));
    expect(windowLabel(a, 0)).toBe('prem');
    expect(windowLabel(b, 1)).toBe('chat');
    expect(bars(null)).toEqual([null, null]);
  });
});

describe('accounts', () => {
  it('a session with no profile counts against its harness own login', () => {
    expect(accountKey(null, 'claude')).toBe('@claude');
    expect(accountKey(' ', 'codex')).toBe('@codex');
    expect(accountKey('work', 'claude')).toBe('work');
  });
  it('the live push wins over the listed copy', () => {
    const row = { key: '@claude', usage: usage([w('session', 1)]) } as AccountRow;
    expect(liveUsage(row, {})?.windows[0].percent).toBe(1);
    expect(liveUsage(row, { '@claude': usage([w('session', 34)]) })?.windows[0].percent).toBe(34);
  });
  it('the tooltip names each window, its reset, the source and the age', () => {
    const now = 1_000_000_000;
    const five = w('session', 34, { resets_at: now / 1000 + 3 * 3600 + 12 * 60 });
    setLabels([[five, label('34% resets 9:50 pm', 'resets 9:50 pm', 'in 3 h 12 m')]]);
    const t = usageTitle(
      { name: 'claude', harness: 'claude', profile: false },
      usage([five, w('weekly', 82, { read_at: now / 1000 - 180 })], { at: now - 180_000 }),
      now,
    );
    expect(t).toContain("claude: the CLI's own login");
    expect(t).toContain('5-hour: 34%, resets 9:50 pm in 3 h 12 m');
    expect(t).toContain('weekly: 82%');
    expect(t).toContain('From the Claude status line, 3m ago');
  });
  it('a harness own login joins the rail while a reading or a session on it says it is in use', () => {
    const work = { key: 'work', name: 'work', harness: 'claude', profile: true } as AccountRow;
    const rows = mergeRows([work], { '@claude': usage([]), '@codex': usage([], { account: '@codex' }) }, ['copilot', 'aider']);
    expect(rows.map((r) => r.key)).toEqual(['work', '@claude', '@codex', '@copilot']);
    expect(rows[2].name).toBe('codex');
    expect(rows[2].profile).toBe(false);
    const listed = { key: '@claude', name: 'claude', harness: 'claude', profile: false } as AccountRow;
    expect(mergeRows([listed], { '@claude': usage([]) }, ['claude']).length).toBe(1);
    expect(mergeRows([], {}, []).length).toBe(0);
  });
  it('sessions count against their profile, or their harness own login', () => {
    const a = (id: string, profile: string | null, extra: Partial<Agent> = {}) =>
      ({ id, name: id, kind: 'claude', custom_harness: null, profile, archived: false, ...extra }) as Agent;
    const agents = [
      a('s1', 'work'),
      a('s2', null),
      a('s3', 'work', { archived: true }),
      a('s4', null, { kind: 'custom', custom_harness: 'aider' }),
      a('s5', ' ', { kind: 'codex' }),
    ];
    expect(sessionsOn({ key: 'work' }, agents).map((x) => x.id)).toEqual(['s1']);
    expect(sessionsOn({ key: '@claude' }, agents).map((x) => x.id)).toEqual(['s2']);
    expect(sessionsOn({ key: '@aider' }, agents).map((x) => x.id)).toEqual(['s4']);
    expect(ownLoginSessionHarnesses(agents)).toEqual(['claude', 'aider', 'codex']);
  });
  it('the login words say credentials were found, not that they work', () => {
    expect(loginTitle({ login: 'logged_in', expired: false, loginHint: null })).toContain('credentials found');
    expect(loginTitle({ login: 'needs_login', expired: true, loginHint: 'kete profile login work' })).toBe(
      'Login expired: the credentials say it ran out. Sign in with: kete profile login work',
    );
    expect(loginTitle({ login: 'needs_login', loginHint: null })).toBe('Not signed in: no credentials found');
  });
  it('a switched-off default keeps its star, marked disabled', () => {
    const r = { name: 'work', harness: 'claude', default: false, flaggedDefault: true, disabled: true };
    expect(starTitle(r)).toContain('work is the claude default (disabled)');
    expect(starTitle({ ...r, disabled: false, flaggedDefault: false })).toBe('Make work the claude default');
    expect(starTitle({ ...r, default: true, disabled: false })).toBe('work is the claude default');
  });
  it('the headline is the fullest 5-hour window of any account', () => {
    const now = 1_000_000_000;
    const acct = (account: string, percent: number): AccountUsage => usage([w('session', percent, { read_at: now / 1000 })], { account });
    expect(headline({}, now)).toBeNull();
    expect(headline({ a: acct('a', 40), b: acct('b', 87) }, now)?.account).toBe('b');
  });
  it('a context tooltip counts compactions', () => {
    const t = contextTitle(
      { percent: 96, tokens: 192_000, window: 200_000, compactions: 2, last_compact_at: 0, source: 'statusline', at: 0 },
      600_000,
    );
    expect(t).toContain('Context 96% (192k of 200k tokens)');
    expect(t).toContain('Compacted 2 times, last 10m ago');
  });
});
