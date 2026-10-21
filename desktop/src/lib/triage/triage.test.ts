import { describe, expect, it } from 'vitest';
import type { Model } from '../api';
import {
  allowedTarget,
  defaultTarget,
  nextTarget,
  orchestratorRepo,
  repoChoices,
  routeText,
  sendingLabel,
  targetLabel,
  type TriageSent,
} from './triage.svelte';

const filed = (o: Partial<Extract<TriageSent, { kind: 'filed' }>> = {}): TriageSent => ({
  kind: 'filed',
  id: 'n-1',
  path: 'C:/r/inbox/n-1.md',
  to: 'desk',
  route: null,
  busy: false,
  unsent: null,
  ...o,
});

describe('target chip', () => {
  it('defaults to the desk only when the repo has one', () => {
    expect(defaultTarget({ desk: true })).toBe('desk');
    expect(defaultTarget({ desk: false })).toBe('orchestrator');
    expect(defaultTarget(null)).toBe('orchestrator');
  });

  it('refuses the desk and switching without one', () => {
    expect(allowedTarget({ desk: false }, 'desk')).toBe('orchestrator');
    expect(nextTarget({ desk: false }, 'orchestrator')).toBe('orchestrator');
    expect(nextTarget({ desk: true }, 'desk')).toBe('orchestrator');
    expect(nextTarget({ desk: true }, 'orchestrator')).toBe('desk');
  });

  it('names the targets', () => {
    expect(targetLabel('desk')).toBe('Triage desk');
    expect(targetLabel('orchestrator')).toBe('Orchestrator');
  });
});

describe('wording', () => {
  it('says where a pending note is going', () => {
    expect(sendingLabel('desk')).toBe('Sending to the triage desk\u2026');
    expect(sendingLabel('orchestrator')).toBe('Sending to the orchestrator\u2026');
  });

  it('shows the desk answer, the busy wait, the unsent reason and the duplicate', () => {
    expect(routeText(filed({ route: 'Routed to fix-login.' }))).toBe('Routed to fix-login.');
    expect(routeText(filed({ busy: true }))).toBe('Filed n-1. The desk is busy, so the note waits in the inbox.');
    expect(routeText(filed({ to: null, unsent: 'no desk and no orchestrator' }))).toBe('Filed n-1. Not sent: no desk and no orchestrator');
    expect(routeText({ kind: 'duplicate', path: 'C:\\r\\inbox\\n-0.md', reason: 'same text' })).toBe('Already in the inbox: n-0.md (same text)');
  });

  it('uses no em dash', () => {
    expect(sendingLabel('desk') + routeText(filed({ busy: true }))).not.toContain('\u2014');
  });
});

describe('repo', () => {
  const m = {
    projects: [
      { id: 'a', name: 'alpha' },
      { id: 'b', name: 'beta' },
    ],
    worktrees: [{ id: 'w1', project_id: 'b' }],
    agents: [
      { id: 'o0', role: 'orchestrator', archived: true, worktree_id: 'gone' },
      { id: 'o1', role: 'orchestrator', archived: false, worktree_id: 'w1' },
    ],
  } as unknown as Model;

  it('files into the orchestrator repo first, then the studio repos, once each', () => {
    expect(orchestratorRepo(m)).toBe('b');
    expect(repoChoices(m, [{ repo: 'a', name: 'alpha' }, { repo: 'b', name: 'beta' }]).map((r) => r.id)).toEqual(['b', 'a']);
  });

  it('has no orchestrator repo without a live orchestrator', () => {
    expect(orchestratorRepo({ ...m, agents: [] } as unknown as Model)).toBeNull();
  });
});
