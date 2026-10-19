import { describe, expect, it } from 'vitest';
import type { Agent } from '../api';
import type { WorktreeGit } from './api';
import { RUNNING_MS, changedGit, dueRunning, movedStamps, sameGit, stampTouchedAt } from './watch';

const g = (over: Partial<WorktreeGit> = {}): WorktreeGit => ({
  changed: 0,
  untracked: 0,
  upstream: 'origin/main',
  ahead: 0,
  behind: 0,
  committed: 100,
  ahead_of_base: 0,
  on_default_branch: true,
  ...over,
});

const agent = (id: string, worktree_id: string, status: Agent['status'], archived = false) =>
  ({ id, worktree_id, status, archived }) as Agent;

describe('movedStamps', () => {
  it('names new and changed stamps and ignores the ones that held still or left', () => {
    const last = { w1: 'a', w2: 'b', gone: 'c' };
    expect(movedStamps(last, { w1: 'a', w2: 'B', w3: '1:0' })).toEqual(['w2', 'w3']);
    expect(movedStamps(last, {})).toEqual([]);
  });
});

describe('a checkout stamped for the first time', () => {
  // 5 000 ms and 9 000 ms after the epoch, in the stamp's nanoseconds.
  const stamp = (ms: number) => `41:${ms * 1e6}|137:${(ms - 500) * 1e6}|-`;

  it('reads its newest file time off the stamp, missing files ignored', () => {
    expect(stampTouchedAt(stamp(9_000))).toBe(9_000);
    expect(stampTouchedAt('-|-')).toBe(0);
  });

  it('is read again when a file changed after its last read began', () => {
    // Staged while the read that began at 8 000 ran: that read may have missed it.
    expect(movedStamps({}, { w1: stamp(9_000) }, new Map([['w1', 8_000]]))).toEqual(['w1']);
    // Untouched since the read: nothing to do.
    expect(movedStamps({}, { w1: stamp(5_000) }, new Map([['w1', 8_000]]))).toEqual([]);
    // Never read: read.
    expect(movedStamps({}, { w1: stamp(5_000) })).toEqual(['w1']);
  });
});

describe('dueRunning', () => {
  it('reads a shown checkout with a running session once its gap has passed', () => {
    const agents = [agent('a', 'w1', 'running'), agent('b', 'w2', 'finished'), agent('c', 'w3', 'running', true)];
    const readAt = new Map([['w1', 1_000]]);
    expect(dueRunning(agents, ['w1', 'w2', 'w3'], readAt, 1_000 + RUNNING_MS - 1)).toEqual([]);
    expect(dueRunning(agents, ['w1', 'w2', 'w3'], readAt, 1_000 + RUNNING_MS)).toEqual(['w1']);
    // Not shown: the overview folded its repo.
    expect(dueRunning(agents, ['w2'], new Map(), 50_000)).toEqual([]);
  });
});

describe('changedGit', () => {
  it('keeps only the entries whose fields changed', () => {
    const have = { w1: g(), w2: g({ changed: 2 }) };
    const got = { w1: g(), w2: g({ changed: 3 }), w3: g() };
    expect(Object.keys(changedGit(have, got))).toEqual(['w2', 'w3']);
    expect(sameGit(have.w1, g())).toBe(true);
    expect(sameGit(have.w1, g({ upstream: null }))).toBe(false);
    expect(sameGit(undefined, g())).toBe(false);
  });
});
