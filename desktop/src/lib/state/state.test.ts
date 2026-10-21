import { describe, expect, it } from 'vitest';
import {
  MAX_MAP_BYTES,
  SLICE_MAX_BYTES,
  ago,
  line,
  neighbours,
  parseMap,
  slice,
  tree,
  type EdgeKind,
  type NodeKind,
  type StateEdge,
  type StateMap,
  type StateNode,
} from './state';

// Ported from the tests in crates/nebula-core/src/state_map.rs. Its `map`
// helper goes through `merge`, which sorts by kind; these fixtures are
// already in that order, so the plain build below is the same map.
const n = (id: string, kind: NodeKind, over: Partial<StateNode> = {}): StateNode => ({
  id,
  kind,
  label: '',
  status: '',
  owner: null,
  changed_at: null,
  questions: 0,
  links: [],
  ...over,
});
const edge = (from: string, to: string, kind: EdgeKind): StateEdge => ({ from, to, kind });
const map = (nodes: StateNode[], edges: StateEdge[]): StateMap => ({ repo: 'C:/Code/game', built_at: 0, nodes, edges, note: null });
const bytes = (s: string) => new TextEncoder().encode(s).length;

/** Rust's `tree_text`: the whole hierarchy as indented lines. */
const treeText = (m: StateMap) =>
  tree(m, () => true)
    .map((r) => '  '.repeat(r.depth) + line(m.nodes[r.node]) + '\n')
    .join('');

describe('tree', () => {
  const m = map(
    [n('A', 'system'), n('B', 'subsystem'), n('C', 'subsystem'), n('X', 'file'), n('Y', 'file')],
    [
      edge('A', 'B', 'owns'),
      edge('A', 'C', 'owns'),
      edge('B', 'C', 'owns'),
      edge('A', 'X', 'depends_on'),
      edge('X', 'Y', 'owns'),
      edge('Y', 'X', 'owns'),
    ],
  );

  it('follows owns edges and survives cycles', () => {
    const shown = tree(m, () => true).map((r) => [m.nodes[r.node].id, r.depth]);
    expect(shown).toEqual([
      ['A', 0],
      ['B', 1],
      ['C', 2],
      ['X', 0],
      ['Y', 1],
    ]);
  });

  it('hides what a closed node owns', () => {
    const closed = tree(m, (id) => id !== 'A');
    expect(closed).toHaveLength(3);
    expect(closed[0].hasChildren).toBe(true);
    expect(closed.map((r) => m.nodes[r.node].id)).toEqual(['A', 'X', 'Y']);
  });

  it('a closed node deep in the tree hides only its own rows', () => {
    const rows = tree(m, (id) => id !== 'B');
    expect(rows.map((r) => m.nodes[r.node].id)).toEqual(['A', 'B', 'X', 'Y']);
  });

  it('prints as indented text', () => {
    expect(treeText(m)).toBe('system A\n  subsystem B\n    subsystem C\nfile X\n  file Y\n');
  });
});

describe('line', () => {
  it('is kind id, then label, status and questions when there are any', () => {
    expect(line(n('A', 'system'))).toBe('system A');
    expect(line(n('A', 'system', { label: 'A' }))).toBe('system A');
    expect(line(n('D-012', 'decision', { label: 'Combat pacing', status: 'open', questions: 2 }))).toBe(
      'decision D-012: Combat pacing (open), 2 open questions',
    );
    expect(line(n('D', 'decision', { questions: 1 }))).toBe('decision D, 1 open question');
  });

  it('cuts a label past 100 characters to 97 and three dots', () => {
    const text = line(n('A', 'file', { label: 'x'.repeat(101) }));
    expect(text).toBe(`file A: ${'x'.repeat(97)}...`);
    expect(line(n('A', 'file', { label: 'x'.repeat(100) }))).toBe(`file A: ${'x'.repeat(100)}`);
  });

  it('counts characters, not UTF-16 units', () => {
    const label = '\u{1F600}'.repeat(100);
    expect(line(n('A', 'file', { label }))).toBe(`file A: ${label}`);
  });
});

describe('slice', () => {
  it('names the node and its neighbours', () => {
    const m = map(
      [
        n('D-012', 'decision', { label: 'Combat pacing', status: 'open', questions: 2, changed_at: 0 }),
        n('SYS-1', 'system'),
        n('dept:design', 'department'),
      ],
      [edge('dept:design', 'D-012', 'owns'), edge('D-012', 'SYS-1', 'depends_on')],
    );
    expect(slice(m, 'D-012', 180_000)).toBe(
      'State map of C:/Code/game, built 3 min ago.\n' +
        'decision D-012: Combat pacing (open), 2 open questions\n' +
        'Changed 3 min ago.\n' +
        'Owned by:\n' +
        '- department dept:design\n' +
        'Depends on:\n' +
        '- system SYS-1\n',
    );
    expect(slice(m, 'nope', 0)).toBeNull();
  });

  it('adds owner and links, and groups both directions', () => {
    const m = map(
      [
        n('A', 'lane', { owner: 'Jay', links: ['docs/a.md', 'https://x.invalid/1'] }),
        n('B', 'pr'),
        n('C', 'lane'),
        n('D', 'file'),
        n('E', 'agent'),
      ],
      [
        edge('A', 'B', 'blocked_by'),
        edge('C', 'A', 'blocked_by'),
        edge('A', 'D', 'touched_by'),
        edge('E', 'A', 'touched_by'),
        edge('C', 'A', 'depends_on'),
        edge('A', 'gone', 'owns'),
      ],
    );
    expect(slice(m, 'A', 0)).toBe(
      'State map of C:/Code/game, built just now.\n' +
        'lane A\n' +
        'Owner: Jay.\n' +
        'Links: docs/a.md, https://x.invalid/1.\n' +
        'Needed by:\n- lane C\n' +
        'Blocked by:\n- pr B\n' +
        'Blocks:\n- lane C\n' +
        'Touched by:\n- file D\n' +
        'Touches:\n- agent E\n',
    );
  });

  it('never passes its cap', () => {
    const nodes = [n('hub', 'system')];
    const edges: StateEdge[] = [];
    for (let i = 0; i < 400; i++) {
      const id = `file-${String(i).padStart(3, '0')}`;
      nodes.push(n(id, 'file', { label: 'a long label '.repeat(20) }));
      edges.push(edge('hub', id, 'owns'));
    }
    const text = slice(map(nodes, edges), 'hub', 0)!;
    expect(bytes(text)).toBeLessThanOrEqual(SLICE_MAX_BYTES);
    expect(text.endsWith('`kete state --slice hub --json` has them all.\n')).toBe(true);
    expect(text.startsWith('State map of')).toBe(true);
    expect(text).toMatch(/\nAnd \d+ lines more\. /);
  });

  it('measures the cap in UTF-8 bytes', () => {
    const nodes = [n('hub', 'system')];
    const edges: StateEdge[] = [];
    for (let i = 0; i < 100; i++) {
      nodes.push(n(`f${i}`, 'file', { label: '\u00e9\u4e2d'.repeat(40) }));
      edges.push(edge('hub', `f${i}`, 'owns'));
    }
    const text = slice(map(nodes, edges), 'hub', 0)!;
    expect(bytes(text)).toBeLessThanOrEqual(SLICE_MAX_BYTES);
    expect(text.length).toBeLessThan(bytes(text));
    expect(text.endsWith('has them all.\n')).toBe(true);
  });

  it('stays under its cap with a huge id and a multibyte label', () => {
    const id = 'x'.repeat(3000);
    const nodes = [n(id, 'system', { label: String.fromCodePoint(0x1f30a).repeat(1500) })];
    const edges: StateEdge[] = [];
    for (let i = 0; i < 50; i++) {
      nodes.push(n(`f${i}`, 'file', { label: String.fromCodePoint(0xe9).repeat(99) }));
      edges.push(edge(id, `f${i}`, 'owns'));
    }
    const text = slice(map(nodes, edges), id, 0)!;
    expect(bytes(text)).toBeLessThanOrEqual(SLICE_MAX_BYTES);
    expect(text.endsWith('has them all.\n')).toBe(true);
  });
});

describe('neighbours', () => {
  it('is the node, what an edge joins it to, and those edges', () => {
    const m = map(
      [n('D-012', 'decision'), n('SYS-1', 'system'), n('dept:design', 'department'), n('far', 'file')],
      [edge('dept:design', 'D-012', 'owns'), edge('D-012', 'SYS-1', 'depends_on'), edge('far', 'dept:design', 'owns')],
    );
    const near = neighbours(m, 'SYS-1')!;
    expect(near.nodes.map((x) => x.id)).toEqual(['D-012', 'SYS-1']);
    expect(near.edges).toHaveLength(1);
    expect(neighbours(m, 'nope')).toBeNull();
  });
});

describe('ago', () => {
  it('reads plainly', () => {
    expect(ago(59_000, 0)).toBe('just now');
    expect(ago(120_000, 0)).toBe('2 min ago');
    expect(ago(7_200_000, 0)).toBe('2 h ago');
    expect(ago(86_400_000, 0)).toBe('1 day ago');
    expect(ago(0, 5_000)).toBe('just now');
  });

  it('turns over at the boundaries', () => {
    expect(ago(60_000, 0)).toBe('1 min ago');
    expect(ago(3_599_000, 0)).toBe('59 min ago');
    expect(ago(3_600_000, 0)).toBe('1 h ago');
    expect(ago(86_399_000, 0)).toBe('23 h ago');
    expect(ago(172_800_000, 0)).toBe('2 days ago');
  });
});

describe('parseMap', () => {
  it('reads a full map', () => {
    const m = parseMap(
      JSON.stringify({
        repo: 'C:/Code/game',
        built_at: 5,
        nodes: [{ id: 'D-012', kind: 'decision', label: 'Combat pacing', status: 'open', owner: 'Jay', changed_at: 7, questions: 2, links: ['a.md'] }],
        edges: [{ from: 'D-012', to: 'SYS-1', kind: 'depends_on' }],
        note: 'no extractor',
      }),
    ) as StateMap;
    expect(m.nodes[0]).toEqual(n('D-012', 'decision', { label: 'Combat pacing', status: 'open', owner: 'Jay', changed_at: 7, questions: 2, links: ['a.md'] }));
    expect(m.edges).toEqual([edge('D-012', 'SYS-1', 'depends_on')]);
    expect(m.note).toBe('no extractor');
  });

  it('defaults what is missing, as the Rust does', () => {
    const m = parseMap('{"repo":"r","built_at":1,"nodes":[{"id":"a","kind":"file"}]}') as StateMap;
    expect(m).toEqual({ repo: 'r', built_at: 1, nodes: [n('a', 'file')], edges: [], note: null });
    expect(parseMap('{"repo":"r","built_at":1}')).toEqual({ repo: 'r', built_at: 1, nodes: [], edges: [], note: null });
    expect((parseMap('{"repo":"r","built_at":1,"note":null}') as StateMap).note).toBeNull();
  });

  it('gives one error line for anything else', () => {
    const errs = [
      parseMap('{'),
      parseMap('[]'),
      parseMap('null'),
      parseMap('{"built_at":1}'),
      parseMap('{"repo":"r"}'),
      parseMap('{"repo":"r","built_at":1,"nodes":[{"id":"a","kind":"spaceship"}]}'),
      parseMap('{"repo":"r","built_at":1,"nodes":[{"kind":"file"}]}'),
      parseMap('{"repo":"r","built_at":1,"nodes":[{"id":"a","kind":"file","questions":-1}]}'),
      parseMap('{"repo":"r","built_at":1,"edges":[{"from":"a","to":"b","kind":"rhymes_with"}]}'),
    ];
    for (const e of errs) {
      expect(typeof e).toBe('string');
      expect(e as string).toMatch(/^\.kete\/state-map\.json is not a state map: .+/);
      expect(e as string).not.toContain('\n');
    }
  });

  it('refuses a file over 8 MiB', () => {
    expect(parseMap(' '.repeat(MAX_MAP_BYTES + 1))).toBe('.kete/state-map.json is over 8 MiB');
  });
});
