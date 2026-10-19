import { describe, expect, it } from 'vitest';
import type { Agent, Model, Worktree } from './api';
import { recentCheckouts } from './recent';

const wt = (id: string, project_id = 'p'): Worktree => ({ id, project_id, path: `C:/r/${id}`, branch: id, is_main: false, sort_order: 0 });
const agent = (id: string, worktree_id: string, status_changed_at: number, archived = false): Agent =>
  ({ id, worktree_id, name: id, status: 'finished', archived, unseen: false, status_changed_at, sort_order: 0, parent: null, recent_prompts: [], role: 'worker' }) as unknown as Agent;
const model = (worktrees: Worktree[], agents: Agent[]): Model =>
  ({ projects: [{ id: 'p', name: 'repo', repo_path: 'C:/r', sort_order: 0 }], worktrees, agents }) as unknown as Model;

describe('recentCheckouts', () => {
  it('orders checkouts by their newest session, archived ones included', () => {
    const m = model([wt('a'), wt('b'), wt('c')], [agent('1', 'a', 10), agent('2', 'b', 30, true), agent('3', 'a', 20)]);
    expect(recentCheckouts(m).map((r) => [r.worktree, r.at, r.label])).toEqual([
      ['b', 30, 'repo / b'],
      ['a', 20, 'repo / a'],
      ['c', 0, 'repo / c'],
    ]);
  });

  it('lists a linked checkout no session ran in after those that have history, and never the repo root', () => {
    const root = { ...wt('main'), is_main: true };
    const m = model([root, wt('fresh'), wt('used')], [agent('1', 'used', 5)]);
    expect(recentCheckouts(m).map((r) => [r.worktree, r.at])).toEqual([
      ['used', 5],
      ['fresh', 0],
    ]);
  });

  it('drops checkouts and repos gone from the model, and keeps at most the limit', () => {
    const m = model([wt('a'), wt('x', 'gone')], [agent('1', 'a', 1), agent('2', 'x', 5), agent('3', 'missing', 9)]);
    expect(recentCheckouts(m).map((r) => r.worktree)).toEqual(['a']);
    const many = model(
      ['a', 'b', 'c', 'd', 'e', 'f'].map((id) => wt(id)),
      ['a', 'b', 'c', 'd', 'e', 'f'].map((id, i) => agent(id, id, i)),
    );
    expect(recentCheckouts(many).map((r) => r.worktree)).toEqual(['f', 'e', 'd', 'c', 'b']);
    expect(recentCheckouts(many, 2)).toHaveLength(2);
  });
});
