import { describe, expect, it } from 'vitest';
import type { Agent, Model, Subagent } from './api';
import { changedWords, childCounts, railFacts } from './sessionRail';
import type { WorktreeGit } from './worktrees/api';

const agent = (over: Partial<Agent>): Agent =>
  ({
    id: 'a',
    worktree_id: 'w1',
    name: 'a',
    status: 'running',
    archived: false,
    unseen: false,
    status_changed_at: 0,
    kind: 'claude',
    custom_harness: null,
    model: null,
    effort: null,
    profile: null,
    alive: true,
    recent_prompts: [],
    role: '',
    sort_order: 0,
    parent: null,
    ...over,
  }) as Agent;

const sub = (id: string, status: Subagent['status']): Subagent => ({
  id,
  parent_agent: 'a',
  kind: 'Explore',
  description: '',
  started_at: 0,
  ended_at: null,
  status,
});

function model(agents: Agent[], subagents: Subagent[] = []): Model {
  return {
    projects: [{ id: 'p1', name: 'kete' }],
    worktrees: [{ id: 'w1', project_id: 'p1', path: 'C:/code/kete', branch: 'main', is_main: true, sort_order: 0 }],
    agents,
    subagents,
    contexts: {},
  } as unknown as Model;
}

const git = (changed: number, untracked: number) => ({ w1: { changed, untracked } as WorktreeGit });

describe('railFacts', () => {
  it('reads a local session: its checkout, its changed files, its model and effort', () => {
    const a = agent({ model: 'opus', effort: 'high' });
    const f = railFacts(model([a]), a, git(2, 1));
    expect(f.place).toEqual({ kind: 'local', project: 'kete', branch: 'main', path: 'C:/code/kete' });
    expect(f.changed).toBe(3);
    expect([f.model, f.effort]).toEqual(['opus', 'high']);
  });

  it('says changed files are unknown until the checkout is read, and for a cloud session', () => {
    const a = agent({});
    expect(railFacts(model([a]), a, {}).changed).toBeNull();
    const c = agent({ cloud_session_id: 'session_1' });
    const f = railFacts(model([c]), c, git(4, 0));
    expect(f.place).toEqual({ kind: 'cloud', url: 'https://claude.ai/code/session_1' });
    expect(f.changed).toBeNull();
  });
});

describe('childCounts', () => {
  it('counts spawned sessions and subagents by how they stand, archived ones aside', () => {
    const agents = [
      agent({}),
      agent({ id: 'b', parent: 'a', status: 'running' }),
      agent({ id: 'c', parent: 'a', status: 'needs_feedback' }),
      agent({ id: 'd', parent: 'a', status: 'finished' }),
      agent({ id: 'e', parent: 'a', status: 'finished', archived: true }),
      agent({ id: 'f', parent: 'zz', status: 'running' }),
    ];
    const subs = [sub('s1', 'running'), sub('s2', 'done'), sub('s3', 'ended_unknown')];
    expect(childCounts(model(agents, subs), 'a')).toEqual({ working: 2, needs: 1, done: 3 });
  });
});

it('changedWords', () => {
  expect([changedWords(0), changedWords(1), changedWords(5)]).toEqual(['No changes', '1 changed file', '5 changed files']);
});
