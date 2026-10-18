import { expect, test } from 'vitest';
import type { Agent, AgentStatus, Subagent } from '../api';
import { attentionRank, buildForest, flatten, rollup, rollupLabel, sortRepos, subagentMeta, type Node } from './tree';

function agent(id: string, parent: string | null = null, status: AgentStatus = 'finished', unseen = false): Agent {
  return {
    id,
    worktree_id: 'w1',
    name: id,
    status,
    archived: false,
    unseen,
    status_changed_at: 0,
    kind: 'claude',
    custom_harness: null,
    model: null,
    profile: null,
    alive: true,
    recent_prompts: [],
    role: 'worker',
    sort_order: 0,
    parent,
  };
}

function sub(parent: string, id: string, status: Subagent['status'] = 'running'): Subagent {
  return { id, parent_agent: parent, kind: 'Explore', description: `look at ${id}`, started_at: 0, ended_at: null, status };
}

function find(forest: Node[], id: string): Node | undefined {
  for (const n of forest) {
    if (n.id === id) return n;
    const hit = find(n.children, id);
    if (hit) return hit;
  }
  return undefined;
}

test('the forest nests three deep', () => {
  const forest = buildForest(
    [agent('lead'), agent('child', 'lead'), agent('grandchild', 'child')],
    [],
  );
  expect(forest.map((n) => n.id)).toEqual(['lead']);
  const child = forest[0].children[0];
  expect(child.id).toBe('child');
  expect(child.depth).toBe(1);
  expect(child.children[0].id).toBe('grandchild');
  expect(child.children[0].depth).toBe(2);
});

test('a subagent nests under its session', () => {
  const forest = buildForest([agent('lead'), agent('child', 'lead')], [sub('child', 's1')]);
  const s = find(forest, 'child')!.children.find((n) => n.kind === 'subagent');
  expect(s).toBeDefined();
  expect(s!.depth).toBe(2);
  expect(s!.status).toBe('running');
  expect(s!.label).toBe('look at s1');
});

test('an orphan whose parent is missing becomes a root', () => {
  const forest = buildForest([agent('lead'), agent('lost', 'gone')], [sub('nobody', 's9')]);
  expect(forest.map((n) => n.id).sort()).toContain('lost');
  expect(forest.find((n) => n.id === 'lost')!.depth).toBe(0);
  expect(forest.some((n) => n.kind === 'subagent' && n.depth === 0)).toBe(true);
});

test('a row ranks by the most urgent node in its subtree', () => {
  const forest = buildForest(
    [agent('lead', null, 'running'), agent('asks', 'lead', 'needs_feedback')],
    [],
  );
  expect(attentionRank(forest[0])).toBe(0);
  const done = buildForest([agent('a', null, 'finished')], [sub('a', 'x', 'failed')]);
  expect(attentionRank(done[0])).toBe(1);
  expect(attentionRank(buildForest([agent('u', null, 'finished', true)], [])[0])).toBe(2);
  expect(attentionRank(buildForest([agent('r', null, 'running')], [])[0])).toBe(3);
  expect(attentionRank(buildForest([agent('d')], [])[0])).toBe(4);
});

test('a repo with a needs-you child floats first', () => {
  const quiet = { name: 'alpha', forest: buildForest([agent('a1', null, 'running')], []) };
  const asks = {
    name: 'zeta',
    forest: buildForest([agent('z1', null, 'running'), agent('z2', 'z1', 'needs_feedback')], []),
  };
  const idle = { name: 'beta', forest: [] as Node[] };
  expect(sortRepos([quiet, idle, asks]).map((r) => r.name)).toEqual(['zeta', 'alpha', 'beta']);
});

test('ties sort by name', () => {
  const a = { name: 'web', forest: buildForest([agent('w')], []) };
  const b = { name: 'api', forest: buildForest([agent('x')], []) };
  expect(sortRepos([a, b]).map((r) => r.name)).toEqual(['api', 'web']);
});

test('nodes deeper than two are collapsed by default', () => {
  const forest = buildForest(
    [agent('lead'), agent('child', 'lead'), agent('grandchild', 'child'), agent('deep', 'grandchild')],
    [sub('grandchild', 's1')],
  );
  const rows = flatten(forest, new Set());
  expect(rows.map((r) => r.node.id)).toEqual(['lead', 'child', 'grandchild']);
  const gc = rows.find((r) => r.node.id === 'grandchild')!;
  expect(gc.hidden).toBe(2);
  expect(gc.expanded).toBe(false);

  const open = flatten(forest, new Set(['grandchild']));
  expect(open.map((r) => r.node.id)).toEqual(['lead', 'child', 'grandchild', 'deep', 's1']);
});

test('a collapsed node can be opened and an open one closed', () => {
  const forest = buildForest([agent('lead'), agent('child', 'lead')], []);
  expect(flatten(forest, new Set()).length).toBe(2);
  expect(flatten(forest, new Set(), new Set(['lead'])).map((r) => r.node.id)).toEqual(['lead']);
});

test('archived sessions show only when asked, as stopped, and never count for attention', async () => {
  const { repoRows, tally } = await import('./repos');
  const gone = { ...agent('gone', null, 'needs_feedback', true), archived: true };
  const live = agent('live', null, 'running');
  const m = {
    connected: true, error: null, daemon_pid: 1, protocol_version: 1, runtime_dir: '',
    projects: [{ id: 'p', name: 'p', repo_path: '/p', sort_order: 0 }],
    worktrees: [{ id: 'w', project_id: 'p', path: '/p', branch: 'main', is_main: true, sort_order: 0 }],
    agents: [gone, live].map((a) => ({ ...a, worktree_id: 'w' })),
    terminals: [], ports: [], prs: {}, subagents: [],
    cards: {}, lanes: { needs: [], running: [], unseen: [], done: [], pr: [] }, hidden: { needs: 0, running: 0, unseen: 0, done: 0, pr: 0 }, scope: {},
  };
  expect(repoRows(m)[0].forest.map((n) => n.id)).toEqual(['live']);
  const shown = repoRows(m, { archived: true });
  expect(shown[0].forest.map((n) => n.id).sort()).toEqual(['gone', 'live']);
  expect(shown[0].forest.find((n) => n.id === 'gone')?.status).toBe('stopped');
  expect(tally(shown).needs ?? []).toEqual([]);
});

test('finished subagents fold into one N finished node that starts folded', () => {
  const subs = [sub('a', 'live'), sub('a', 'ok', 'done'), sub('a', 'lost', 'ended_unknown')];
  const forest = buildForest([agent('a', null, 'running')], subs, undefined, true);
  const group = find(forest, 'f:a')!;
  expect(group.kind).toBe('finished');
  expect(group.label).toBe('2 finished');
  expect(group.children.map((n) => n.id)).toEqual(['ok', 'lost']);
  const rows = flatten(forest, new Set());
  expect(rows.map((r) => r.node.key)).toEqual(['a:a', 's:a:live', 'f:a']);
  expect(rows[2].hidden).toBe(2);
  expect(flatten(forest, new Set(['f:a'])).length).toBe(5);
  // Off, every subagent is a row of its own, as before.
  expect(flatten(buildForest([agent('a', null, 'running')], subs), new Set()).length).toBe(4);
});

test('a parent rolls up its descendants as working, needs you and done', () => {
  const forest = buildForest(
    [
      agent('p'),
      agent('busy', 'p', 'running'),
      agent('asks', 'p', 'needs_feedback'),
      agent('read', 'p', 'finished'),
      agent('unread', 'busy', 'finished', true),
      agent('idle', 'p', 'fresh'),
    ],
    [sub('busy', 's1', 'running'), sub('read', 's2', 'done')],
  );
  const p = find(forest, 'p')!;
  expect(rollup(p)).toEqual({ working: 2, needs: 1, done: 3 });
  expect(rollupLabel(rollup(p))).toBe('2 working \u00b7 1 needs you \u00b7 3 done');
  expect(rollupLabel(rollup(find(forest, 'read')!))).toBe('1 done');
  expect(rollupLabel(rollup(find(forest, 'idle')!))).toBe('');
});

test('the ended subagents a FINISHED GROUP folds count as done, the group row not at all', () => {
  const subs = [sub('a', 'live'), sub('a', 'ok', 'done'), sub('a', 'lost', 'ended_unknown')];
  const forest = buildForest([agent('a', null, 'running')], subs, undefined, true);
  expect(rollup(find(forest, 'a')!)).toEqual({ working: 1, needs: 0, done: 2 });
  expect(rollup(buildForest([agent('a', null, 'running')], subs)[0])).toEqual({ working: 1, needs: 0, done: 2 });
});

test('a subagent row names its model and effort when known', () => {
  const s = sub('a', 'x');
  expect(subagentMeta(s)).toBe('Explore');
  expect(subagentMeta({ ...s, run: { model: 'claude-haiku-4-5', effort: 'low' } })).toBe('Explore \u00b7 haiku-4-5 \u00b7 low');
  expect(subagentMeta({ ...s, kind: '', run: { model: '', effort: 'high' } })).toBe('high');
  // Named by its type: the type is not said twice.
  expect(subagentMeta({ ...s, description: '', run: { model: 'sonnet', effort: '' } })).toBe('sonnet');
  expect(subagentMeta({ ...s, status: 'ended_unknown' })).toBe('closed \u00b7 Explore');
  expect(subagentMeta({ ...s, run: null })).toBe('Explore');
});
