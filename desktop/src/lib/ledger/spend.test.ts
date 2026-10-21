import { describe, expect, it, vi } from 'vitest';
import type { SpendRow } from '../studio/types';
import { everyMidnight, localDay, msToMidnight, spendLine } from './spend';

const row = (over: Partial<SpendRow>): SpendRow => ({
  agent: 'a',
  day: localDay(),
  input_tokens: 0,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_write_tokens: 0,
  cost_usd: 0,
  tool_uses: 0,
  turns: 0,
  ...over,
});

describe('spendLine', () => {
  it('says nothing without a row', () => {
    expect(spendLine(undefined)).toBeNull();
  });

  it('ticks to the new day just after each local midnight, and stops', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date(2026, 9, 21, 23, 59, 30));
      expect(msToMidnight()).toBe(30_000);
      const days: string[] = [];
      const stop = everyMidnight((d) => days.push(d));
      vi.advanceTimersByTime(29_000);
      expect(days).toEqual([]);
      vi.advanceTimersByTime(2_000);
      expect(days).toEqual(['2026-10-22']);
      // Yesterday's row stops reading as today's on that tick.
      expect(spendLine(row({ day: '2026-10-21', turns: 2 }), days[0])).toBeNull();
      vi.advanceTimersByTime(24 * 3600 * 1000);
      expect(days).toEqual(['2026-10-22', '2026-10-23']);
      stop();
      vi.advanceTimersByTime(48 * 3600 * 1000);
      expect(days).toHaveLength(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("says nothing for another day's row", () => {
    expect(spendLine(row({ day: '2026-10-21', turns: 3 }), '2026-10-22')).toBeNull();
    expect(spendLine(row({ day: '2026-10-22', turns: 3 }), '2026-10-22')).toContain('3 turns');
    expect(localDay(new Date(2026, 9, 2, 23, 59))).toBe('2026-10-02');
  });

  it('words the day: k tokens, two decimals of USD, counts', () => {
    const r = row({ input_tokens: 400_000, output_tokens: 12_345, cost_usd: 3.1, tool_uses: 58, turns: 9 });
    expect(spendLine(r)).toBe('today: 412k tokens, est. US$3.10, 58 tool uses, 9 turns');
  });

  it('leaves cache tokens out of the count', () => {
    const r = row({ input_tokens: 100, output_tokens: 50, cache_read_tokens: 9_000_000, cache_write_tokens: 9_000_000 });
    expect(spendLine(r)).toContain('150 tokens');
  });

  it('prints under a thousand whole and millions with one decimal', () => {
    expect(spendLine(row({ output_tokens: 950 }))).toContain('950 tokens');
    expect(spendLine(row({ output_tokens: 1_500_000 }))).toContain('1.5M tokens');
    expect(spendLine(row({ output_tokens: 2_000_000 }))).toContain('2M tokens');
    expect(spendLine(row({ output_tokens: 999_600 }))).toContain('1M tokens');
  });

  it('rounds cost to two decimals', () => {
    expect(spendLine(row({ cost_usd: 3.456 }))).toContain('est. US$3.46');
    expect(spendLine(row({ cost_usd: 12 }))).toContain('est. US$12.00');
  });

  it('uses singular forms for one', () => {
    expect(spendLine(row({ input_tokens: 1, tool_uses: 1, turns: 1 }))).toBe('today: 1 token, est. US$0.00, 1 tool use, 1 turn');
  });
});
