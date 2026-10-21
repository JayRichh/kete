<script lang="ts">
  // STATE MAP: what exists in a repo and who is on it, as a tree of owns
  // edges (left) and the selected node with a small graph of what joins it
  // (right). The daemon writes `.kete/state-map.json` in the repo's main
  // checkout; this reads it on mount, every 10 s and on Reload. The rules
  // (tree, line, slice) are state.ts, mirrored from the Rust.
  import { tick, untrack } from 'svelte';
  import { editorsApi } from '../editors/api';
  import { MAP_PATH, ago, line, neighbours, parseMap, slice, tree, type StateMap, type StateNode } from './state';

  let { worktree }: { worktree: string | null } = $props();

  let map = $state<StateMap | null>(null);
  let problem = $state<string | null>(null);
  let missing = $state(false);
  let note = $state<string | null>(null);
  let loaded = $state(false);
  let now = $state(Date.now());
  let seq = 0;

  async function load(w: string) {
    const n = ++seq;
    try {
      const f = await editorsApi.readFile(w, MAP_PATH);
      if (n !== seq) return;
      note = f.note;
      missing = f.text === null;
      if (f.text === null) {
        map = null;
        problem = null;
      } else {
        const parsed = parseMap(f.text);
        map = typeof parsed === 'string' ? null : parsed;
        problem = typeof parsed === 'string' ? parsed : null;
      }
      now = Date.now();
    } catch (e) {
      if (n !== seq) return;
      problem = `The state map could not be read: ${e}`;
    }
    loaded = true;
  }

  // A checkout (or its change) starts over; then it refreshes on a timer
  // while the page is visible.
  $effect(() => {
    const w = worktree;
    untrack(() => {
      map = null;
      problem = null;
      missing = false;
      note = null;
      loaded = false;
      pick = null;
      toggled = {};
      seq++;
    });
    if (!w) return;
    void load(w);
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void load(w);
    }, 10_000);
    return () => clearInterval(timer);
  });

  // ---- the tree ----

  let toggled = $state<Record<string, boolean>>({});
  let pick = $state<string | null>(null);
  let rowEls: HTMLElement[] = [];

  const full = $derived(map ? tree(map, () => true) : []);
  // Roots start open; everything under them starts closed.
  const rootIds = $derived(new Set(full.filter((r) => r.depth === 0).map((r) => map!.nodes[r.node].id)));
  const isOpen = (id: string) => toggled[id] ?? rootIds.has(id);
  const rows = $derived(map ? tree(map, isOpen) : []);
  const selId = $derived(
    map && pick && map.nodes.some((x) => x.id === pick) ? pick : rows.length ? map!.nodes[rows[0].node].id : null,
  );
  const sel = $derived(map && selId ? (map.nodes.find((x) => x.id === selId) ?? null) : null);

  /** Ember means needs you: a session waiting, or open questions. */
  const needsYou = (x: StateNode) => x.status === 'needs_feedback' || x.questions > 0;
  const words = (s: string) => s.replace(/_/g, ' ');
  const rowWord = (x: StateNode) =>
    [x.kind, x.status ? words(x.status) : '', x.questions > 0 ? `${x.questions} open ${x.questions === 1 ? 'question' : 'questions'}` : '']
      .filter(Boolean)
      .join(' \u00b7 ');

  async function go(to: number) {
    const r = rows[to];
    if (!r || !map) return;
    pick = map.nodes[r.node].id;
    await tick();
    rowEls[to]?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (!map || e.ctrlKey || e.metaKey || e.altKey) return;
    const i = rows.findIndex((r) => map!.nodes[r.node].id === selId);
    const row = rows[i];
    const id = row ? map.nodes[row.node].id : '';
    if (e.key === 'ArrowDown') void go(Math.min(i + 1, rows.length - 1));
    else if (e.key === 'ArrowUp') void go(Math.max(i - 1, 0));
    else if (e.key === 'ArrowRight' || e.key === 'Enter') {
      if (row?.hasChildren && !isOpen(id)) toggled[id] = true;
    } else if (e.key === 'ArrowLeft') {
      if (row?.hasChildren && isOpen(id)) toggled[id] = false;
    } else return;
    e.preventDefault();
  }

  /** Select a node from the joined list, opening what hides it. */
  function show(id: string) {
    if (!map) return;
    const i = full.findIndex((r) => map!.nodes[r.node].id === id);
    for (let j = i - 1, d = full[i]?.depth ?? 0; j >= 0 && d > 0; j--) {
      if (full[j].depth < d) {
        toggled[map.nodes[full[j].node].id] = true;
        d = full[j].depth;
      }
    }
    pick = id;
  }

  // ---- the graph ----

  const W = 480;
  const H = 340;
  const RX = 175;
  const RY = 125;
  const HW = 54;
  const HH = 14;
  const MAX_AROUND = 8;
  const short = (s: string) => (s.length > 16 ? `${s.slice(0, 15)}\u2026` : s);
  /** How far from a box's centre a ray with this direction leaves the box. */
  const clip = (ux: number, uy: number) => Math.min(HW / Math.max(Math.abs(ux), 1e-9), HH / Math.max(Math.abs(uy), 1e-9));

  const joined = $derived.by(() => {
    if (!map || !sel) return null;
    const near = neighbours(map, sel.id);
    if (!near) return null;
    const others = near.nodes.filter((x) => x.id !== sel.id);
    const edgesOf = (x: StateNode) => near.edges.filter((e) => e.from !== e.to && (e.from === x.id || e.to === x.id));
    const around = others.slice(0, MAX_AROUND).map((x, i, all) => {
      const a = -Math.PI / 2 + (2 * Math.PI * i) / all.length;
      const x0 = W / 2 + RX * Math.cos(a);
      const y0 = H / 2 + RY * Math.sin(a);
      const es = edgesOf(x);
      const out = es.filter((e) => e.from === sel.id).length;
      // The line runs from -> to so the arrowhead marks the `to` end.
      const into = out === 0;
      const [fx, fy, tx, ty] = into ? [x0, y0, W / 2, H / 2] : [W / 2, H / 2, x0, y0];
      const d = Math.hypot(tx - fx, ty - fy);
      const ux = (tx - fx) / d;
      const uy = (ty - fy) / d;
      const k = clip(ux, uy);
      return {
        node: x,
        x: x0,
        y: y0,
        label: [...new Set(es.map((e) => words(e.kind)))].join(', '),
        arrow: out === 0 || out === es.length,
        x1: fx + ux * k,
        y1: fy + uy * k,
        x2: tx - ux * (k + 2),
        y2: ty - uy * (k + 2),
        edges: es,
      };
    });
    return { around, all: others, hidden: Math.max(others.length - MAX_AROUND, 0) };
  });

  /** For the joined list: how a neighbour joins the selected node, in words. */
  function joinWord(l: { node: StateNode; edges: { from: string; to: string; kind: string }[] }): string {
    return l.edges.map((e) => `${e.from === sel!.id ? '' : 'from '}${words(e.kind)}`.trim()).join(', ');
  }
  const allJoined = $derived(
    joined
      ? joined.all.map((x) => ({
          node: x,
          edges: map!.edges.filter((e) => e.from !== e.to && ((e.from === x.id && e.to === sel!.id) || (e.to === x.id && e.from === sel!.id))),
        }))
      : [],
  );

  // ---- copy ----

  let copyNote = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(copyTimer));
  async function copy() {
    if (!map || !selId) return;
    const text = slice(map, selId, Date.now());
    if (text === null) return;
    clearTimeout(copyTimer);
    try {
      await navigator.clipboard.writeText(text);
      copyNote = 'Copied';
    } catch {
      copyNote = 'Could not copy';
    }
    copyTimer = setTimeout(() => (copyNote = null), 2000);
  }
</script>

<div class="@container flex min-w-0 flex-col gap-3" data-state-map>
  {#if !worktree}
    <p class="m-0 max-w-[60ch] text-[13px] text-muted" data-state-empty>This repo has no checkout in Kete.</p>
  {:else}
    <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
      <h2 class="m-0 text-[13px] font-semibold text-ink">State map</h2>
      {#if map}
        <span class="min-w-0 truncate font-mono text-[12px] text-muted" title={map.repo}>{map.repo}</span>
        <span class="text-[12px] text-muted">Built {ago(now, map.built_at)}, {map.nodes.length} {map.nodes.length === 1 ? 'node' : 'nodes'}</span>
      {/if}
      <button
        class="ml-auto h-6 border border-rule bg-transparent px-2 text-[12px] text-ink hover:border-rule-strong"
        title="Reload the state map now"
        onclick={() => void load(worktree)}>Reload</button
      >
    </div>

    {#if problem}
      <p class="m-0 border border-fail px-3 py-2 text-[13px] text-fail" role="alert" data-state-error>
        <span aria-hidden="true">&#x2715; </span>{problem}
      </p>
    {/if}

    {#if map?.note}
      <p class="m-0 text-[12px] text-muted" data-state-note>Repo layer: {map.note}</p>
    {/if}

    {#if !loaded}
      <p class="m-0 text-[13px] text-muted">Reading the state map&#x2026;</p>
    {:else if missing}
      <div class="flex max-w-[60ch] flex-col gap-1" data-state-empty>
        <p class="m-0 text-[13px] text-muted">No state map yet. Kete builds one while studio mode is on for this repo.</p>
        {#if note}<p class="m-0 text-[12px] text-muted">{note}</p>{/if}
      </div>
    {:else if map && map.nodes.length === 0}
      <p class="m-0 text-[13px] text-muted" data-state-empty>The state map has no nodes yet.</p>
    {:else if map}
      <div class="flex min-w-0 flex-col gap-3 @min-[760px]:flex-row @min-[760px]:items-start">
        <div
          class="max-h-[320px] min-w-0 flex-none overflow-auto border border-rule @min-[760px]:max-h-[560px] @min-[760px]:w-[300px]"
          role="tree"
          aria-label="State map tree"
          tabindex="-1"
          onkeydown={onKey}
        >
          {#each rows as r, i (r.node)}
            {@const nd = map.nodes[r.node]}
            {@const on = nd.id === selId}
            {@const ember = needsYou(nd)}
            {@const open = isOpen(nd.id)}
            <div
              class="flex min-w-0 items-stretch border-b border-l-2 border-b-rule last:border-b-0 {on ? 'border-l-ink bg-raised' : 'border-l-transparent'}"
              role="treeitem"
              data-state-node={nd.id}
              aria-selected={on}
              aria-level={r.depth + 1}
              aria-expanded={r.hasChildren ? open : undefined}
              style="padding-left: {r.depth * 14}px"
            >
              {#if r.hasChildren}
                <button
                  class="w-6 flex-none border-0 bg-transparent p-0 text-[12px] text-muted hover:text-ink"
                  tabindex="-1"
                  aria-label="{open ? 'Close' : 'Open'} {nd.label || nd.id}"
                  title={open ? 'Close (Left arrow)' : 'Open (Right arrow or Enter)'}
                  onclick={() => {
                    toggled[nd.id] = !open;
                    pick = nd.id;
                  }}>{open ? '\u25be' : '\u25b8'}</button
                >
              {:else}
                <span class="w-6 flex-none" aria-hidden="true"></span>
              {/if}
              <button
                bind:this={rowEls[i]}
                class="flex min-w-0 flex-1 flex-col gap-0.5 border-0 bg-transparent py-1.5 pr-2 pl-0 text-left hover:bg-surface"
                tabindex={on ? 0 : -1}
                title="{line(nd)}. Up and Down move, Right or Enter opens, Left closes."
                onclick={() => (pick = nd.id)}
              >
                <span class="truncate text-[13px] text-ink">{nd.label || nd.id}</span>
                <span class="truncate font-mono text-[11px] {ember ? 'font-medium text-ember-ink' : 'text-muted'}"
                  >{#if ember}<span aria-hidden="true">! </span>{/if}{rowWord(nd)}</span
                >
              </button>
            </div>
          {/each}
        </div>

        {#if sel && joined}
          <section class="flex min-w-0 flex-1 flex-col gap-3" aria-label="Selected node" data-state-detail>
            <p class="m-0 font-mono text-[13px] break-words text-ink">
              {#if needsYou(sel)}<span class="text-ember-ink" aria-hidden="true">! </span>{/if}{line(sel)}
            </p>
            <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
              <dt class="text-muted">Owner</dt>
              <dd class="m-0 min-w-0 break-words text-ink">{sel.owner ?? 'none'}</dd>
              <dt class="text-muted">Changed</dt>
              <dd class="m-0 text-ink">{sel.changed_at === null ? 'unknown' : ago(now, sel.changed_at)}</dd>
              <dt class="text-muted">Links</dt>
              <dd class="m-0 min-w-0 text-ink">
                {#if sel.links.length === 0}none{:else}
                  <ul class="m-0 flex list-none flex-col gap-0.5 p-0">
                    {#each sel.links as link, i (i)}<li class="font-mono text-[11px] break-all">{link}</li>{/each}
                  </ul>
                {/if}
              </dd>
            </dl>

            <svg
              class="h-auto w-full max-w-[480px] border border-rule"
              viewBox="0 0 {W} {H}"
              role="img"
              aria-label="Graph of {sel.label || sel.id} and the {joined.all.length} nodes joined to it. They are listed below."
            >
              <defs>
                <marker id="state-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto">
                  <path d="M0 0 L8 4 L0 8 z" class="fill-muted" />
                </marker>
              </defs>
              {#each joined.around as l, i (i)}
                <line x1={l.x1} y1={l.y1} x2={l.x2} y2={l.y2} class="stroke-muted" stroke-width="1" marker-end={l.arrow ? 'url(#state-arrow)' : undefined} />
                <text
                  x={(l.x1 + l.x2) / 2}
                  y={(l.y1 + l.y2) / 2}
                  text-anchor="middle"
                  dominant-baseline="central"
                  class="fill-muted font-mono"
                  font-size="10"
                  style="paint-order: stroke; stroke: var(--color-bg); stroke-width: 4px">{l.label}</text
                >
              {/each}
              {#each joined.around as l, i (i)}
                <rect x={l.x - HW} y={l.y - HH} width={HW * 2} height={HH * 2} class="fill-bg {needsYou(l.node) ? 'stroke-ember-ink' : 'stroke-rule-strong'}" stroke-width="1" />
                <text x={l.x} y={l.y} text-anchor="middle" dominant-baseline="central" class="fill-ink" font-size="11">{needsYou(l.node) ? '! ' : ''}{short(l.node.label || l.node.id)}</text>
              {/each}
              <rect x={W / 2 - HW} y={H / 2 - HH} width={HW * 2} height={HH * 2} class="fill-raised {needsYou(sel) ? 'stroke-ember-ink' : 'stroke-rule-strong'}" stroke-width="2" />
              <text x={W / 2} y={H / 2} text-anchor="middle" dominant-baseline="central" class="fill-ink" font-size="11" font-weight="600">{needsYou(sel) ? '! ' : ''}{short(sel.label || sel.id)}</text>
            </svg>
            {#if joined.hidden > 0}
              <p class="m-0 text-[12px] text-muted">The graph shows {MAX_AROUND} of {joined.all.length} joined nodes. The rest are in the list.</p>
            {/if}

            <div class="flex flex-col border border-rule" data-state-joined>
              <h3 class="m-0 border-b border-rule px-3 py-1.5 text-[12px] font-medium text-muted">Joined to this node: {allJoined.length}</h3>
              {#if allJoined.length === 0}
                <p class="m-0 px-3 py-1.5 text-[12px] text-muted">Nothing is joined to it.</p>
              {:else}
                <ul class="m-0 flex max-h-[180px] list-none flex-col overflow-auto p-0">
                  {#each allJoined as l, i (i)}
                    <li class="border-b border-rule last:border-b-0">
                      <button
                        class="flex w-full min-w-0 items-baseline gap-2 border-0 bg-transparent px-3 py-1 text-left hover:bg-surface"
                        title="Select {l.node.label || l.node.id}"
                        onclick={() => show(l.node.id)}
                      >
                        <span class="flex-none font-mono text-[11px] text-muted">{joinWord(l)}</span>
                        <span class="min-w-0 flex-1 truncate text-[12px] text-ink">{l.node.label || l.node.id}</span>
                        <span class="flex-none font-mono text-[11px] {needsYou(l.node) ? 'font-medium text-ember-ink' : 'text-muted'}"
                          >{#if needsYou(l.node)}<span aria-hidden="true">! </span>{/if}{l.node.status ? words(l.node.status) : l.node.kind}</span
                        >
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>

            <div class="flex items-center gap-3">
              <button
                class="h-7 border border-rule bg-transparent px-3 text-[12px] text-ink hover:border-rule-strong"
                title="Copy a short text of this node and what joins it, for an agent to read"
                onclick={copy}>Copy slice</button
              >
              <span class="text-[12px] text-muted" role="status">{copyNote ?? ''}</span>
            </div>
          </section>
        {/if}
      </div>
    {/if}
  {/if}
</div>
