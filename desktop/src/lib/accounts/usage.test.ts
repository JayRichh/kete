import { describe, expect, it } from 'vitest';
import type { AccountRow, AccountUsage, UsageWindow } from '../api';
import {
  accountKey,
  bars,
  contextTitle,
  fillWidth,
  liveUsage,
  mergeRows,
  pct,
  percentAt,
  resetsIn,
  tone,
  usageTitle,
  windowLabel,
} from './usage';

const w = (kind: UsageWindow['kind'], percent: number, extra: Partial<UsageWindow> = {}): UsageWindow => ({
  kind,
  label: null,
  percent,
  resets_at: null,
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

describe('bar thresholds', () => {
  it('goes ember at 80 and red at 95', () => {
    expect(tone(0)).toBe('ok');
    expect(tone(79.9)).toBe('ok');
    expect(tone(80)).toBe('warn');
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
    const t = usageTitle(
      { name: 'claude', harness: 'claude', profile: false },
      usage([w('session', 34, { resets_at: now / 1000 + 3 * 3600 + 12 * 60 }), w('weekly', 82)], { at: now - 180_000 }),
      now,
    );
    expect(t).toContain("claude: the CLI's own login");
    expect(t).toContain('5-hour: 34%, resets in 3h 12m');
    expect(t).toContain('weekly: 82%');
    expect(t).toContain('From the Claude status line, 3m ago');
  });
  it('a harness own login joins the rail once a reading or session brings it, unless a profile covers it', () => {
    const work = { key: 'work', name: 'work', harness: 'claude', profile: true } as AccountRow;
    const rows = mergeRows([work], { '@claude': usage([]), '@codex': usage([], { account: '@codex' }) }, ['copilot', 'aider']);
    expect(rows.map((r) => r.key)).toEqual(['work', '@codex', '@copilot']);
    expect(rows[1].name).toBe('codex');
    expect(rows[1].profile).toBe(false);
    expect(mergeRows([], {}, []).length).toBe(0);
  });
  it('resets read as time left', () => {
    expect(resetsIn(100 + 2 * 86400 + 4 * 3600, 100_000)).toBe('in 2d 4h');
    expect(resetsIn(5, 10_000)).toBe('now');
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
