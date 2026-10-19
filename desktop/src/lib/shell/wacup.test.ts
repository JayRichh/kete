import { describe, expect, it } from 'vitest';
import { POLL_MS, clock, clockLine, idleLine, marquee, type WacupState } from './wacup';

const state = (over: Partial<WacupState>): WacupState => ({
  supported: true,
  running: true,
  player: 'WACUP',
  playback: 'playing',
  title: 'A - B',
  position_ms: 0,
  length_ms: 0,
  can_launch: false,
  ...over,
});

describe('the WACUP panel clock', () => {
  it('reads minutes and seconds, hours from an hour', () => {
    expect(clock(0)).toBe('0:00');
    expect(clock(5_999)).toBe('0:05');
    expect(clock(185_000)).toBe('3:05');
    expect(clock(3_723_000)).toBe('1:02:03');
    expect(clock(-5)).toBe('0:00');
    expect(clock(Number.NaN)).toBe('0:00');
  });

  it('shows elapsed over length, or the elapsed alone for a stream', () => {
    expect(clockLine(83_000, 215_500)).toBe('1:23 / 3:35');
    expect(clockLine(83_000, 0)).toBe('1:23');
  });
});

describe('the title marquee', () => {
  it('does not move a title that fits', () => {
    expect(marquee(100, 120)).toBeNull();
    expect(marquee(120, 120)).toBeNull();
  });

  it('slides a long title by how much it overflows, slowly enough to read', () => {
    expect(marquee(300, 200)).toEqual({ shift: 100, seconds: 5 });
    expect(marquee(205, 200)?.seconds).toBe(4);
    expect(marquee(1000, 200)?.shift).toBe(800);
  });
});

describe('the panel states', () => {
  it('says WACUP is not running, and when it is stopped', () => {
    expect(idleLine(state({ running: false }))).toBe('WACUP not running');
    expect(idleLine(state({ playback: 'stopped' }))).toBe('WACUP is stopped');
    expect(idleLine(state({}))).toBe('');
  });

  it('polls once a second', () => {
    expect(POLL_MS).toBe(1000);
  });
});
