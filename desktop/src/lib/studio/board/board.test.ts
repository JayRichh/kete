import { describe, expect, it } from 'vitest';
import type { Agent, ContextUsage, Worktree } from '../../api';
import type { SessionRole, SpendRow } from '../types';
import { BUDGET_AMBER, BUDGET_RED, UNASSIGNED, budgetLevel, joinBoard, slug, type BoardNote, type BoardReply, type Inputs } from './board';

const TODAY = '2026-10-22';

const agent = (id: string, over: Partial<Agent> = {}): Agent =>
  ({ id, worktree_id: 'w1', name: `name-${id}`, status: 'running', archived: false, alive: true, model: null, profile: null, ...over }) as Agent;

const role = (agentId: string, over: Partial<SessionRole> = {}): SessionRole => ({
  agent: agentId,
  card: 'worker',
  label: `role-${agentId}`,
  tier: 3,
  department: null,
  permission: 'prompt',
  context_budget: null,
  ...over,
});

const note = (id: string, over: Partial<BoardNote> = {}): BoardNote => ({
  id,
  path: `/r/${id}.md`,
  title: id,
  status: 'Open',
  to: null,
  about: null,
  priority: 'Normal',
  at: null,
  by: null,
  rel: `inbox/${id}.md`,
  ...over,
});

const spendRow = (agentId: string, over: Partial<SpendRow> = {}): SpendRow => ({
  agent: agentId,
  day: TODAY,
  input_tokens: 100,
  output_tokens: 50,
  cache_read_tokens: 999,
  cache_write_tokens: 999,
  cost_usd: 0.5,
  tool_uses: 0,
  turns: 0,
  ...over,
});

const reply = (over: Partial<BoardReply> = {}): BoardReply => ({
  root: 'C:/r',
  manifest: true,
  error: null,
  departments: [
    { slug: 'art-and-3d', label: 'Art and 3D' },
    { slug: 'audio', label: 'Audio' },
  ],
  notes: [],
  verdicts: {},
  refusals: [],
  renewal: null,
  worktree: 'w1',
  ...over,
});

const worktrees = [
  { id: 'w1', project_id: 'p1' },
  { id: 'w2', project_id: 'p2' },
] as Worktree[];

const inputs = (over: Partial<Inputs> = {}): Inputs => ({
  reply: reply(),
  agents: [],
  worktrees,
  contexts: {},
  roles: {},
  spend: {},
  repo: 'p1',
  today: TODAY,
  ...over,
});

const col = (b: ReturnType<typeof joinBoard>, s: string) => b.columns.find((c) => c.slug === s)!;

describe('slug', () => {
  it('lowercases and joins runs with one dash', () => {
    expect(slug('Art and 3D')).toBe('art-and-3d');
    expect(slug('  Web, community, release ')).toBe('web-community-release');
    expect(slug('--A__B--')).toBe('a-b');
    expect(slug('')).toBe('');
  });
});

describe('budget level', () => {
  it('turns amber at 60 and red at 90', () => {
    expect([BUDGET_AMBER, BUDGET_RED]).toEqual([60, 90]);
    expect(budgetLevel(59)).toBe('ok');
    expect(budgetLevel(60)).toBe('amber');
    expect(budgetLevel(89)).toBe('amber');
    expect(budgetLevel(90)).toBe('red');
  });
});

describe('the column model', () => {
  it('orders the queue by priority, then dated, then oldest, then id', () => {
    const notes = [
      note('n-low', { to: 'Audio', priority: 'Low', at: '2026-01-01' }),
      note('n-undated', { to: 'audio' }),
      note('n-new', { to: 'audio', at: '2026-03-01' }),
      note('n-old', { to: 'audio', at: '2026-02-01' }),
      note('n-b', { to: 'audio', at: '2026-02-01' }),
      note('n-high', { to: 'audio', priority: 'High' }),
      note('n-other', { to: 'art-and-3d' }),
    ];
    const b = joinBoard(inputs({ reply: reply({ notes }) }));
    expect(col(b, 'audio').queue.map((n) => n.id)).toEqual(['n-high', 'n-b', 'n-old', 'n-new', 'n-undated', 'n-low']);
    expect(col(b, 'art-and-3d').queue.map((n) => n.id)).toEqual(['n-other']);
  });

  it('picks the first tier 1 or 2 as lead and makes a second lead a runner', () => {
    const agents = [agent('a'), agent('b'), agent('c'), agent('d')];
    const roles = {
      a: role('a', { tier: 3, label: 'Animator', department: 'Art and 3D' }),
      b: role('b', { tier: 2, label: 'Zed lead', department: 'Art and 3D' }),
      c: role('c', { tier: 1, label: 'Art director', department: 'Art and 3D' }),
      d: role('d', { tier: 2, label: 'Alpha lead', department: 'Art and 3D' }),
    };
    const c = col(joinBoard(inputs({ agents, roles })), 'art-and-3d');
    expect(c.lead?.agent.id).toBe('c');
    expect(c.runners.map((r) => r.agent.id)).toEqual(['d', 'b', 'a']);
  });

  it('adds Unassigned last and only when a session needs it', () => {
    expect(joinBoard(inputs()).columns.map((c) => c.slug)).toEqual(['art-and-3d', 'audio']);
    expect(joinBoard(inputs({ agents: [agent('a')], roles: { a: role('a', { department: 'Audio' }) } })).columns.map((c) => c.slug)).toEqual([
      'art-and-3d',
      'audio',
    ]);
    // No role, and a role whose department matches no row.
    const agents = [agent('a'), agent('b')];
    const roles = { b: role('b', { department: 'Nonexistent', tier: 1 }) };
    const b = joinBoard(inputs({ agents, roles }));
    expect(b.columns.map((c) => c.slug)).toEqual(['art-and-3d', 'audio', UNASSIGNED]);
    const un = col(b, UNASSIGNED);
    expect(un.label).toBe('Unassigned');
    expect(un.lead).toBeNull();
    expect(un.runners.map((r) => r.agent.id)).toEqual(['b', 'a']);
    expect(un.queue).toEqual([]);
  });

  it('keeps only live sessions of this repo', () => {
    const agents = [
      agent('live', { worktree_id: 'w1' }),
      agent('gone', { archived: true }),
      agent('dead', { alive: false }),
      agent('other', { worktree_id: 'w2' }),
    ];
    const b = joinBoard(inputs({ agents }));
    expect(col(b, UNASSIGNED).runners.map((r) => r.agent.id)).toEqual(['live']);
  });

  it('puts the label and context percent on the cell', () => {
    const agents = [agent('a', { model: 'opus', profile: 'work' }), agent('b'), agent('c')];
    const roles = {
      a: role('a', { department: 'Audio', tier: 1, label: 'Audio lead', context_budget: 200_000 }),
      b: role('b', { department: 'Audio', context_budget: 0 }),
    };
    const contexts = { a: { tokens: 119_999 }, b: { tokens: 5 }, c: { tokens: 5 } } as unknown as Record<string, ContextUsage>;
    const a = col(joinBoard(inputs({ agents, roles, contexts })), 'audio');
    expect(a.lead).toMatchObject({ label: 'Audio lead', model: 'opus', profile: 'work', percent: 59, contextTokens: 119_999 });
    expect(a.runners[0].percent).toBeNull();
  });

  it('counts blocked as notes to Jay about the department plus sessions needing feedback', () => {
    const notes = [
      note('j1', { to: 'Jay', about: 'Audio' }),
      note('j2', { to: 'jay', about: 'audio' }),
      note('j3', { to: 'jay', about: 'art-and-3d' }),
      note('j4', { to: 'jay' }),
      note('x', { to: 'audio', about: 'audio' }),
    ];
    const agents = [agent('a', { status: 'needs_feedback' }), agent('b', { status: 'needs_feedback' }), agent('c')];
    const roles = { a: role('a', { department: 'Audio', tier: 1 }), b: role('b', { department: 'Audio' }), c: role('c', { department: 'Audio' }) };
    const b = joinBoard(inputs({ reply: reply({ notes }), agents, roles }));
    expect(col(b, 'audio').blocked).toBe(4);
    expect(col(b, 'art-and-3d').blocked).toBe(1);
  });

  it("sums today's tokens and cost only", () => {
    const agents = [agent('a'), agent('b'), agent('c')];
    const roles = {
      a: role('a', { department: 'Audio', tier: 1 }),
      b: role('b', { department: 'Audio' }),
      c: role('c', { department: 'Art and 3D' }),
    };
    const spend = {
      a: spendRow('a'),
      b: spendRow('b', { input_tokens: 10, output_tokens: 5, cost_usd: 0.25 }),
      c: spendRow('c', { day: '2026-10-21' }),
    };
    const b = joinBoard(inputs({ agents, roles, spend }));
    expect(col(b, 'audio').spend).toEqual({ tokens: 165, cost_usd: 0.75 });
    expect(col(b, 'art-and-3d').spend).toEqual({ tokens: 0, cost_usd: 0 });
  });
});

describe('what needs you', () => {
  it('lists sessions, notes to Jay, refusals, then the failed renewal', () => {
    const r = reply({
      notes: [note('to-jay', { to: 'Jay' }), note('to-audio', { to: 'audio' })],
      refusals: [
        { at: 2, project: 'p1', agent: null, role: null, reason: 'newer' },
        { at: 1, project: 'p1', agent: null, role: null, reason: 'older' },
      ],
      renewal: { at: 3, old: 'o1', reason: 'no budget' },
    });
    const agents = [agent('s1', { status: 'needs_feedback' }), agent('s2'), agent('s3', { status: 'needs_feedback', worktree_id: 'w2' })];
    const n = joinBoard(inputs({ reply: r, agents })).needsYou;
    expect(n.map((x) => x.kind)).toEqual(['session', 'note', 'refusal', 'refusal', 'renewal']);
    expect(n[0].kind === 'session' && n[0].cell.agent.id).toBe('s1');
    expect(n[2].kind === 'refusal' && n[2].refusal.reason).toBe('newer');
  });

  it('is empty when nothing is waiting', () => {
    expect(joinBoard(inputs()).needsYou).toEqual([]);
  });
});
