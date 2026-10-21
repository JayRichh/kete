// The state map, pure: the graph types and the text both apps and the agents
// read. It mirrors `crates/nebula-core/src/state_map.rs` (`tree`, `line`,
// `slice`, `ago`, `neighbourhood`, and `read`'s parse); a rule changed there
// changes here, and the slice text must match byte for byte. No Svelte and
// no Tauri, so vitest reads it.

/** The map's path, relative to the repo root. */
export const MAP_PATH = '.kete/state-map.json';
/** A slice is cut to fit this many UTF-8 bytes. */
export const SLICE_MAX_BYTES = 2048;
/** A map file over this many bytes is not read. */
export const MAX_MAP_BYTES = 8 * 1024 * 1024;
/** A label longer than this many characters is cut in a slice. */
const MAX_LABEL_CHARS = 100;

export const NODE_KINDS = ['department', 'system', 'subsystem', 'decision', 'lane', 'pr', 'file', 'agent'] as const;
export type NodeKind = (typeof NODE_KINDS)[number];
export const EDGE_KINDS = ['owns', 'depends_on', 'touched_by', 'blocked_by'] as const;
export type EdgeKind = (typeof EDGE_KINDS)[number];

export interface StateNode {
  id: string;
  kind: NodeKind;
  label: string;
  status: string;
  owner: string | null;
  /** The last change, epoch ms. */
  changed_at: number | null;
  /** Open questions on it. */
  questions: number;
  /** Paths (repo-relative) or URLs. */
  links: string[];
}

/** `from` owns, depends on, is touched by, or is blocked by `to`. */
export interface StateEdge {
  from: string;
  to: string;
  kind: EdgeKind;
}

export interface StateMap {
  repo: string;
  /** When it was built, epoch ms. */
  built_at: number;
  nodes: StateNode[];
  edges: StateEdge[];
  /** Why the repo layer is missing, when it is. */
  note: string | null;
}

// ---- reading the file ----

// Strict like serde: a wrong type anywhere refuses the whole file; a missing
// field takes the Rust default.
function bad(what: string): never {
  throw new Error(what);
}
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
function str(o: Record<string, unknown>, k: string, def?: string): string {
  const v = o[k];
  if (v === undefined && def !== undefined) return def;
  return typeof v === 'string' ? v : bad(`${k} must be a string`);
}
function int(o: Record<string, unknown>, k: string, min: number, max: number, def?: number): number {
  const v = o[k];
  if (v === undefined && def !== undefined) return def;
  return typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max ? v : bad(`${k} must be a whole number`);
}
function opt<T>(o: Record<string, unknown>, k: string, read: () => T): T | null {
  return o[k] === undefined || o[k] === null ? null : read();
}
function oneOf<T extends string>(o: Record<string, unknown>, k: string, all: readonly T[]): T {
  const v = str(o, k);
  return (all as readonly string[]).includes(v) ? (v as T) : bad(`unknown ${k} "${v}"`);
}
function list<T>(o: Record<string, unknown>, k: string, one: (item: unknown) => T): T[] {
  const v = o[k];
  if (v === undefined) return [];
  return Array.isArray(v) ? v.map(one) : bad(`${k} must be a list`);
}

function readNode(v: unknown): StateNode {
  if (!isObj(v)) return bad('a node must be an object');
  return {
    id: str(v, 'id'),
    kind: oneOf(v, 'kind', NODE_KINDS),
    label: str(v, 'label', ''),
    status: str(v, 'status', ''),
    owner: opt(v, 'owner', () => str(v, 'owner')),
    changed_at: opt(v, 'changed_at', () => int(v, 'changed_at', -Infinity, Infinity)),
    questions: int(v, 'questions', 0, 4_294_967_295, 0),
    links: list(v, 'links', (l) => (typeof l === 'string' ? l : bad('a link must be a string'))),
  };
}
function readEdge(v: unknown): StateEdge {
  if (!isObj(v)) return bad('an edge must be an object');
  return { from: str(v, 'from'), to: str(v, 'to'), kind: oneOf(v, 'kind', EDGE_KINDS) };
}

/** The map file's text as a map, or the one-line error `read` would give. */
export function parseMap(text: string): StateMap | string {
  const not = (why: string) => `${MAP_PATH} is not a state map: ${why}`;
  if (new TextEncoder().encode(text).length > MAX_MAP_BYTES) return `${MAP_PATH} is over ${MAX_MAP_BYTES >> 20} MiB`;
  try {
    const v: unknown = JSON.parse(text);
    if (!isObj(v)) return not('expected an object');
    return {
      repo: str(v, 'repo'),
      built_at: int(v, 'built_at', -Infinity, Infinity),
      nodes: list(v, 'nodes', readNode),
      edges: list(v, 'edges', readEdge),
      note: opt(v, 'note', () => str(v, 'note')),
    };
  } catch (e) {
    return not(e instanceof Error ? e.message : String(e));
  }
}

// ---- the hierarchy ----

/** One row of the hierarchy. */
export interface TreeRow {
  /** Index into `StateMap.nodes`. */
  node: number;
  depth: number;
  hasChildren: boolean;
}

/**
 * The hierarchy by owns edges: roots are the nodes nothing owns, each
 * followed by what it owns, in node order. A node owned twice shows under its
 * first owner; one only reachable through a cycle shows as a root. Only nodes
 * `open` says yes to show their children.
 */
export function tree(map: StateMap, open: (id: string) => boolean): TreeRow[] {
  const count = map.nodes.length;
  const index = new Map<string, number>();
  map.nodes.forEach((n, i) => index.set(n.id, i));
  const children: number[][] = Array.from({ length: count }, () => []);
  const owned: boolean[] = new Array(count).fill(false);
  for (const e of map.edges) {
    if (e.kind !== 'owns') continue;
    const from = index.get(e.from);
    const to = index.get(e.to);
    if (from !== undefined && to !== undefined && !children[from].includes(to)) {
      children[from].push(to);
      owned[to] = true;
    }
  }
  for (const kids of children) kids.sort((a, b) => a - b);
  const placed: boolean[] = new Array(count).fill(false);
  let rows: TreeRow[] = [];
  const order = [...Array(count).keys()];
  // Roots first, then everything left (the cycles), in node order.
  for (const root of [...order.filter((i) => !owned[i]), ...order]) {
    if (placed[root]) continue;
    // Depth first, the stack holding [node, depth].
    const stack: [number, number][] = [[root, 0]];
    for (let top = stack.pop(); top; top = stack.pop()) {
      const [i, depth] = top;
      if (placed[i]) continue;
      placed[i] = true;
      const kids = children[i].filter((c) => !placed[c]);
      rows.push({ node: i, depth, hasChildren: kids.length > 0 });
      for (let k = kids.length - 1; k >= 0; k--) stack.push([kids[k], depth + 1]);
    }
  }
  // Rows are depth first, so a closed node's descendants are the rows after
  // it that sit deeper. A dropped row leaves `closedAt` as it was.
  let closedAt: number | null = null;
  rows = rows.filter((row) => {
    if (closedAt !== null && row.depth > closedAt) return false;
    closedAt = row.hasChildren && !open(map.nodes[row.node].id) ? row.depth : null;
    return true;
  });
  return rows;
}

// ---- text ----

const chars = (text: string) => Array.from(text);
const bytes = (text: string) => new TextEncoder().encode(text).length;

/** `text` cut to at most `max` characters, ending in `...` when cut. */
function cut(text: string, max: number): string {
  const all = chars(text);
  return all.length <= max ? text : all.slice(0, Math.max(max - 3, 0)).join('') + '...';
}

/** `text` cut to at most `max` UTF-8 bytes on a character boundary, ending in `...` when cut. */
function cutBytes(text: string, max: number): string {
  if (bytes(text) <= max) return text;
  let out = '';
  let used = 0;
  for (const c of chars(text)) {
    const b = bytes(c);
    if (used + b > max - 3) break;
    out += c;
    used += b;
  }
  return out + '...';
}

function plural(n: number, word: string): string {
  return n === 1 ? `1 ${word}` : `${n} ${word}s`;
}

/** A node as one line: `kind id: label (status)`. */
export function line(n: StateNode): string {
  let out = `${n.kind} ${n.id}`;
  if (n.label !== '' && n.label !== n.id) out += `: ${cut(n.label, MAX_LABEL_CHARS)}`;
  if (n.status !== '') out += ` (${n.status})`;
  if (n.questions > 0) out += `, ${plural(n.questions, 'open question')}`;
  return out;
}

/** How long ago `at` was: `just now`, `4 min ago`, `3 h ago`, `2 days ago`. */
export function ago(now: number, at: number): string {
  const secs = Math.floor(Math.max(now - at, 0) / 1000);
  if (secs <= 59) return 'just now';
  if (secs <= 3599) return `${Math.floor(secs / 60)} min ago`;
  if (secs <= 86_399) return `${Math.floor(secs / 3600)} h ago`;
  return `${plural(Math.floor(secs / 86_400), 'day')} ago`;
}

// (heading, edge kind, true when `id` is the edge's `to`)
const GROUPS: [string, EdgeKind, boolean][] = [
  ['Owned by', 'owns', true],
  ['Owns', 'owns', false],
  ['Depends on', 'depends_on', false],
  ['Needed by', 'depends_on', true],
  ['Blocked by', 'blocked_by', false],
  ['Blocks', 'blocked_by', true],
  ['Touched by', 'touched_by', false],
  ['Touches', 'touched_by', true],
];

/**
 * What an agent reads about `id`: the node, then each node an edge joins it
 * to, grouped by how, cut to SLICE_MAX_BYTES. Null when the map has no such
 * node.
 */
export function slice(map: StateMap, id: string, now: number): string | null {
  const n = map.nodes.find((x) => x.id === id);
  if (!n) return null;
  const byId = new Map<string, StateNode>();
  for (const x of map.nodes) byId.set(x.id, x);
  const lines = [`State map of ${map.repo}, built ${ago(now, map.built_at)}.`, line(n)];
  if (n.owner !== null) lines.push(`Owner: ${n.owner}.`);
  if (n.changed_at !== null) lines.push(`Changed ${ago(now, n.changed_at)}.`);
  if (n.links.length > 0) lines.push(`Links: ${n.links.join(', ')}.`);
  for (const [heading, kind, incoming] of GROUPS) {
    const others: StateNode[] = [];
    for (const e of map.edges) {
      if (e.kind !== kind) continue;
      const other = incoming ? (e.to === id ? byId.get(e.from) : undefined) : e.from === id ? byId.get(e.to) : undefined;
      if (other) others.push(other);
    }
    if (others.length === 0) continue;
    lines.push(`${heading}:`);
    for (const o of others) lines.push(`- ${line(o)}`);
  }
  return fit(lines, id);
}

/** `lines` joined, as many as fit SLICE_MAX_BYTES with room for the line that says how many were left out. */
function fit(lines: string[], whole: string): string {
  const id = cutBytes(whole, 200);
  const tailRoom = 120 + bytes(id);
  const total = lines.length;
  let out = '';
  let used = 0;
  for (let i = 0; i < total; i++) {
    const l = cutBytes(lines[i], SLICE_MAX_BYTES / 2);
    const room = i + 1 === total ? SLICE_MAX_BYTES : SLICE_MAX_BYTES - tailRoom;
    if (used + bytes(l) + 1 > room) {
      return out + `And ${plural(total - i, 'line')} more. \`kete state --slice ${id} --json\` has them all.\n`;
    }
    out += l + '\n';
    used += bytes(l) + 1;
  }
  return out;
}

// ---- the graph ----

/** The node and every node an edge joins it to, with those edges. Null when the map has no such node. */
export function neighbours(map: StateMap, id: string): StateMap | null {
  if (!map.nodes.some((n) => n.id === id)) return null;
  const edges = map.edges.filter((e) => e.from === id || e.to === id);
  const keep = new Set<string>([id, ...edges.flatMap((e) => [e.from, e.to])]);
  return { ...map, nodes: map.nodes.filter((n) => keep.has(n.id)), edges };
}
