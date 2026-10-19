import { describe, expect, it } from 'vitest';
import type { Subagent } from './api';
import { cardSubagents, endedTotal, subagentWord } from './subagents';

function sub(parent: string, id: string, status: Subagent['status'], at: number): Subagent {
  return {
    id,
    parent_agent: parent,
    kind: 'Explore',
    description: `look into ${id}`,
    started_at: at,
    ended_at: status === 'running' ? null : at + 1,
    status,
  };
}

const subs = [
  sub('a', 'old', 'done', 1),
  sub('a', 'live', 'running', 2),
  sub('a', 'lost', 'ended_unknown', 3),
  sub('b', 'other', 'done', 4),
];

describe('cardSubagents', () => {
  it('folds the ended ones into a count by default and keeps the running ones', () => {
    const c = cardSubagents(subs, 'a', true);
    expect(c.shown.map((s) => s.id)).toEqual(['live']);
    expect(c.folded).toBe(2);
    expect(c.running).toBe(1);
    expect(c.ended).toBe(2);
  });

  it('shows every one, running first then newest, when opened or not hiding', () => {
    for (const c of [cardSubagents(subs, 'a', true, true), cardSubagents(subs, 'a', false)]) {
      expect(c.shown.map((s) => s.id)).toEqual(['live', 'lost', 'old']);
      expect(c.folded).toBe(0);
    }
  });

  it('lists only the session it is asked about', () => {
    expect(cardSubagents(subs, 'b', false).shown.map((s) => s.id)).toEqual(['other']);
    expect(cardSubagents(subs, 'c', true)).toEqual({ shown: [], folded: 0, running: 0, ended: 0 });
  });
});

describe('subagentWord', () => {
  it('says closed for one left open when its session ended', () => {
    expect(subs.map(subagentWord)).toEqual(['done', 'running', 'closed', 'done']);
  });
});

describe('endedTotal', () => {
  it('counts what a clear drops', () => {
    expect(endedTotal(subs)).toBe(3);
  });
});
