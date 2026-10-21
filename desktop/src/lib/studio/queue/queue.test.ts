import { describe, expect, it } from 'vitest';
import type { StudioQueueRow } from '../types';
import { LEVELS, needsConfirm, offLine, stateLine } from './queue.svelte';

const row = (over: Partial<StudioQueueRow>): StudioQueueRow => ({
  project: 'p1',
  name: 'my-game',
  autonomy: 'A1',
  inbox: 'Design/Studio/inbox',
  open: 4,
  to_jay: 1,
  unannounced: 3,
  next: [],
  held: null,
  last_note_at: null,
  ...over,
});

describe('LEVELS', () => {
  it('names the four levels', () => {
    expect(LEVELS.map((l) => `${l.level} ${l.name}`)).toEqual(['A0 Hold', 'A1 Plan', 'A2 Deliver', 'A3 Release']);
  });
});

describe('needsConfirm', () => {
  it('asks only when raising to A2 or A3', () => {
    expect(needsConfirm('A1', 'A2')).toBe(true);
    expect(needsConfirm('A2', 'A3')).toBe(true);
    expect(needsConfirm('A0', 'A3')).toBe(true);
    expect(needsConfirm('A0', 'A1')).toBe(false);
    expect(needsConfirm('A3', 'A1')).toBe(false);
    expect(needsConfirm('A1', 'A1')).toBe(false);
  });
});

describe('stateLine', () => {
  const fmt = () => '14:05';

  it('says the studio is off with no row', () => {
    expect(stateLine(null, 0)).toBe('Studio is off for this repo. Run kete studio enable <repo> to turn it on');
    expect(offLine('wheako')).toBe('Studio is off for this repo. Run kete studio enable wheako to turn it on');
  });

  it('says why it holds', () => {
    expect(stateLine(row({ held: 'you typed 40 s ago' }), 0)).toBe('Holding: you typed 40 s ago');
  });

  it('counts the new notes, singular and plural', () => {
    expect(stateLine(row({}), 0)).toBe('Ready: 3 new notes for the next idle note');
    expect(stateLine(row({ unannounced: 1 }), 0)).toBe('Ready: 1 new note for the next idle note');
  });

  it('appends the last idle note time', () => {
    expect(stateLine(row({ last_note_at: 1 }), 0, fmt)).toBe('Ready: 3 new notes for the next idle note \u00b7 last idle note at 14:05');
    expect(stateLine(row({ held: 'nothing new', last_note_at: 1 }), 0, fmt)).toBe('Holding: nothing new \u00b7 last idle note at 14:05');
  });
});
