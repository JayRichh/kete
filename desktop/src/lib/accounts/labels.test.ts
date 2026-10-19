import { describe, expect, it, vi } from 'vitest';
import type { UsageLabel, UsageWindow } from '../api';

// The label the Rust formatter (`usage_window::format`, NZDT) returns for a
// 5-hour window at 87% resetting 9:50 pm, 1 h 12 m from now: the wording is
// pinned in usage_window.rs; here it travels through the real cache path.
const reply: UsageLabel = {
  percent: 87,
  step: 2,
  clock: '9:50 pm',
  countdown: 'in 1 h 12 m',
  start: '4:50 pm',
  iso: '2026-10-19T21:50:00+13:00',
  estimated: false,
  compact: '87% \u00b7 resets 9:50 pm',
  long: '87% \u00b7 resets 9:50 pm \u00b7 in 1 h 12 m',
  resets: 'resets 9:50 pm',
  hover: 'window began 4:50 pm\nresets 9:50 pm\nin 1 h 12 m\n2026-10-19T21:50:00+13:00',
  banner: null,
};
const usageLabels = vi.fn(() => Promise.resolve([reply]));
vi.mock('../api', () => ({ api: { usageLabels: (...a: unknown[]) => (usageLabels as (...a: unknown[]) => unknown)(...a) } }));

const win: UsageWindow = { kind: 'session', label: null, percent: 87, resets_at: 1_792_000_000, read_at: 1_791_995_000 };

describe('usage labels from the formatter', () => {
  it('refreshLabels asks once and the tooltip carries the local reset words', async () => {
    const { refreshLabels, labelFor } = await import('./labels.svelte');
    const { usageTitle } = await import('./usage');
    expect(labelFor(win)).toBeNull();
    const now = 1_791_995_100_000;
    await refreshLabels([win, { ...win }], now);
    expect(usageLabels).toHaveBeenCalledTimes(1);
    expect(labelFor(win)?.compact).toBe('87% \u00b7 resets 9:50 pm');
    const t = usageTitle({ name: 'claude', harness: 'claude', profile: false }, { account: '@claude', harness: 'claude', windows: [win], source: 'statusline', at: now - 1000, plan: null }, now);
    expect(t).toContain('5-hour: 87%, resets 9:50 pm in 1 h 12 m');
  });
});
