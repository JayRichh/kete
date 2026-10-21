<script lang="ts">
  import { retract } from '../motion';
  import { app, openSession, sessionKey } from '../store.svelte';
  import { nav } from '../shell/history.svelte';
  import { age, harnessLabel, lastPrompt } from '../status';
  import type { TerminalTab } from '../api';
  import Status from '../Status.svelte';
  import RunButton from './RunButton.svelte';
  import ContextBar from '../accounts/ContextBar.svelte';
  import StopBadge from '../StopBadge.svelte';
  import RoleChip from '../roles/RoleChip.svelte';
  import { flatten, rollup, rollupLabel, subagentMeta, type Node } from './tree';
  // A row's right-click menu comes from its data-target (menus/router.ts).
  import OrchBadge from '../orchestrator/OrchBadge.svelte';
  import { openOrchestrator } from '../orch.svelte';
  import { agentDropOk, drag, dropAgent, endDrag, leftFor, offsetIn, startDrag } from '../reorder.svelte';
  import { dropZone, type Place } from '../order';
  import { withKey } from '../shortcuts';
  // A Files row dropped on a session or terminal types its path there.
  import { pathDrop } from '../files/drop.svelte';

  let { repo, forest, terminals }: { repo: string; forest: Node[]; terminals: TerminalTab[] } = $props();

  // Which nodes the user opened or closed, kept with the view's state so
  // back and forward find the tree as it was.
  const ui = $derived(nav.ui('overview', repo));
  const opened = $derived(new Set((ui.open as string[] | undefined) ?? []));
  const closed = $derived(new Set((ui.closed as string[] | undefined) ?? []));
  const rows = $derived(flatten(forest, opened, closed));

  function toggle(n: Node, open: boolean) {
    const o = new Set(opened);
    const c = new Set(closed);
    if (open) (o.add(n.id), c.delete(n.id));
    else (c.add(n.id), o.delete(n.id));
    ui.open = [...o];
    ui.closed = [...c];
  }

  const current = $derived(
    nav.current.view === 'session' || nav.current.view === 'diff' ? sessionKey(app.session) : '',
  );

  function openNode(n: Node) {
    const id = n.agent?.id ?? n.sub?.parent_agent;
    if (id) openSession({ Agent: id });
  }

  function onRowKey(e: KeyboardEvent, n: Node, expanded: boolean, hasKids: boolean) {
    // Enter opens; Space is the follow-up, the window's row key.
    if (e.key === 'Enter') {
      e.preventDefault();
      openNode(n);
    } else if (e.key === 'ArrowRight' && hasKids && !expanded) {
      e.preventDefault();
      toggle(n, true);
    } else if (e.key === 'ArrowLeft' && hasKids && expanded) {
      e.preventDefault();
      toggle(n, false);
    }
  }

  function meta(n: Node): string {
    if (n.agent?.archived) return ['archived', harnessLabel(n.agent)].join(' \u00b7 ');
    if (n.agent) return [harnessLabel(n.agent), n.agent.profile].filter(Boolean).join(' \u00b7 ');
    // Indent and the lighter name already say "subagent"; the type (unless
    // the name already is it), model and effort are the news. `closed`:
    // still open when its session ended or restarted.
    if (n.sub) return subagentMeta(n.sub);
    if (n.kind === 'finished') return 'subagents, folded';
    return '';
  }

  function when(n: Node): number {
    if (n.agent) return n.agent.status_changed_at;
    if (n.sub) return n.sub.ended_at ?? n.sub.started_at;
    if (n.kind === 'finished') return Math.max(0, ...n.children.map(when));
    return 0;
  }

  // A session row dragged beside another under the same parent reorders
  // them (Alt+Up / Alt+Down from the keys); onto a worktree's header it
  // moves (WorktreeBand). The 1px ember rule says where it lands.
  let mark = $state<{ id: string; place: Place } | null>(null);

  function over(e: DragEvent, n: Node) {
    const id = n.agent?.id;
    if (!id || !agentDropOk(drag.item, id)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const [y, h] = offsetIn(e);
    mark = { id, place: dropZone(y, h, { insert: true, onto: false }) as Place };
  }

  function dropOn(e: DragEvent, n: Node) {
    const d = drag.item;
    const at = mark;
    mark = null;
    if (!n.agent || !at || at.id !== n.agent.id || !agentDropOk(d, n.agent.id)) return;
    e.preventDefault();
    e.stopPropagation();
    endDrag();
    void dropAgent(n.agent.worktree_id, d.id, n.agent.id, at.place);
  }

  const reorderHint = $derived(withKey('drag to reorder', ['move_up', 'move_down']));

  const ROW =
    'group relative grid min-h-9 cursor-default items-center border-b border-rule pr-4 outline-none ' +
    'grid-cols-[112px_minmax(200px,1.5fr)_minmax(0,170px)_minmax(0,2fr)_44px] ' +
    '@max-[760px]:grid-cols-[104px_minmax(0,1fr)_44px] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-rule-strong';
</script>

<div role="tree" aria-label="Sessions">
  {#each rows as { node: n, expanded, hidden } (n.key)}
    {@const hasKids = n.children.length > 0}
    {@const needs = n.status === 'needs'}
    {@const sel = !!n.agent && current === `a:${n.agent.id}`}
    {@const prompt = n.agent ? lastPrompt(n.agent) : null}
    <div
      role="treeitem"
      aria-level={n.depth + 1}
      aria-expanded={hasKids ? expanded : undefined}
      aria-selected={sel}
      tabindex="0"
      data-row
      data-target={n.agent ? `agent:${n.agent.id}` : n.sub ? `subagent:${n.sub.parent_agent}` : undefined}
      class="{ROW} {sel ? 'bg-surface' : 'hover:bg-surface'} {n.agent?.archived ? 'opacity-60' : ''}"
      title={n.agent ? `${n.label}: ${reorderHint}, or onto a worktree to move it` : undefined}
      draggable={!!n.agent}
      ondragstart={(e) => n.agent && startDrag(e, { kind: 'agent', id: n.agent.id, worktree: n.agent.worktree_id })}
      ondragend={() => ((mark = null), endDrag())}
      ondragenter={(e) => over(e, n)}
      ondragover={(e) => over(e, n)}
      ondragleave={(e) => leftFor(e) && mark?.id === n.agent?.id && (mark = null)}
      ondrop={(e) => dropOn(e, n)}
      use:pathDrop={{ target: () => (n.agent ? { Agent: n.agent.id } : null) }}
      onclick={() => (n.kind === 'finished' ? toggle(n, !expanded) : openNode(n))}
      onkeydown={(e) => onRowKey(e, n, expanded, hasKids)}
    >
      {#if mark && drag.item && mark.id === n.agent?.id}
        <span class="pointer-events-none absolute inset-x-0 z-10 h-px bg-ember {mark.place === 'before' ? '-top-px' : '-bottom-px'}" aria-hidden="true"></span>
      {/if}
      {#if needs}
        <span class="absolute inset-y-0 left-0 w-[3px] origin-left bg-ember" out:retract aria-hidden="true"></span>
      {/if}
      <span class="pl-6"><Status tone={n.status} /></span>

      <span class="relative flex min-w-0 items-center self-stretch" style="padding-left: {n.depth * 20}px">
        {#each { length: n.depth } as _, lvl (lvl)}
          <span class="absolute inset-y-0 w-px bg-rule" style="left: {lvl * 20 + 6}px" aria-hidden="true"></span>
        {/each}
        {#if hasKids}
          <button
            class="mr-1 grid size-4 flex-none place-items-center border-0 bg-transparent p-0 text-muted hover:text-ink"
            tabindex="-1"
            aria-label={expanded ? 'Collapse' : 'Expand'}
            onclick={(e) => {
              e.stopPropagation();
              toggle(n, !expanded);
            }}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
              ><path d={expanded ? 'M1.5 3.5L5 7l3.5-3.5' : 'M3.5 1.5L7 5 3.5 8.5'} fill="none" stroke="currentColor" stroke-width="1.4" /></svg
            >
          </button>
        {:else}
          <span class="mr-1 size-4 flex-none" aria-hidden="true"></span>
        {/if}
        <span class="relative min-w-0 truncate py-1.5 {n.kind === 'subagent' ? 'text-[13px] text-ink' : 'text-[14px] font-medium'} {needs ? 'text-ember-ink' : 'text-ink'}">
          {n.label}
          {#if n.status === 'running'}
            <span class="absolute inset-x-0 bottom-1 h-px overflow-hidden" aria-hidden="true"
              ><span class="block h-px run-seg w-1/3 animate-[kete-run_1.8s_linear_infinite] bg-running"></span></span
            >
          {/if}
        </span>
        {#if n.agent?.role === 'orchestrator'}<span class="ml-2"><OrchBadge /></span>{/if}
        {#if n.agent}<RoleChip agent={n.agent.id} />{/if}
        {#if n.from}
          {@const from = n.from}
          <button
            class="ml-2 inline-flex max-w-[45%] flex-none items-center gap-1 truncate border border-rule bg-transparent px-1 font-mono text-[11px] leading-4 text-muted hover:border-rule-strong hover:text-ink"
            title="Started by {from.name}, in another worktree or repo. Open it."
            onclick={(e) => {
              e.stopPropagation();
              if (from.role === 'orchestrator') openOrchestrator();
              else openSession({ Agent: from.id });
            }}
            >from {#if from.role === 'orchestrator'}<span aria-hidden="true">&#9670;</span> Orchestrator{:else}{from.name}{/if}</button
          >
        {/if}
        {#if hasKids && n.kind === 'session'}
          <!-- What its nested sessions and subagents are doing, so a folded
               parent still says one of them needs you. -->
          {@const tally = rollupLabel(rollup(n))}
          {#if tally}
            <span class="ml-2 min-w-0 flex-none truncate font-mono text-[11px] text-muted @max-[760px]:hidden" data-rollup title="Under {n.label}: {tally}">{tally}</span>
          {/if}
        {/if}
        {#if hidden && n.kind !== 'finished'}
          <button
            class="ml-2 flex-none border-0 bg-transparent p-0 font-mono text-[12px] text-muted hover:text-ink hover:underline hover:underline-offset-4"
            onclick={(e) => {
              e.stopPropagation();
              toggle(n, true);
            }}>+ {hidden} {hidden === 1 ? 'agent' : 'agents'}</button
          >
        {/if}
        {#if n.agent && !n.agent.archived}
          {@const stop = app.model.stops?.[n.agent.id]}
          <!-- The context rides the name, so the meta column keeps its
               harness and profile whole. -->
          <span class="ml-2 inline-flex flex-none"><ContextBar context={app.model.contexts?.[n.agent.id]} /></span>
          <!-- Why the last turn stopped short: resumes at, logged out, ... -->
          {#if stop}<span class="ml-2 inline-flex flex-none"><StopBadge note={stop} /></span>{/if}
        {/if}
      </span>

      <span class="flex min-w-0 items-center gap-2 pl-3 text-[13px] text-muted @max-[760px]:hidden"
        ><span class="min-w-0 truncate" title={meta(n)}>{meta(n)}</span></span
      >
      <span class="truncate pl-3 text-[13px] text-muted @max-[760px]:hidden">
        {prompt ?? ''}
      </span>
      <span class="text-right font-mono text-[12px] text-muted">{age(when(n), app.now)}</span>
    </div>
  {/each}

  {#each terminals as t (t.id)}
    {@const sel = current === `t:${t.id}`}
    <div
      role="treeitem"
      aria-level={1}
      aria-selected={sel}
      tabindex="0"
      data-row
      data-target="terminal:{t.id}"
      use:pathDrop={{ target: () => ({ Terminal: t.id }) }}
      class="{ROW} {sel ? 'bg-surface' : 'hover:bg-surface'}"
      onclick={() => openSession({ Terminal: t.id })}
      onkeydown={(e) => (e.key === 'Enter' ? (e.preventDefault(), openSession({ Terminal: t.id })) : e.key === ' ' && e.preventDefault())}
    >
      <span class="pl-6"><Status tone={t.alive ? 'shell' : 'stopped'} /></span>
      <span class="flex min-w-0 items-center">
        <span class="mr-1 size-4 flex-none" aria-hidden="true"></span>
        <span class="truncate py-1.5 text-[14px] text-ink">{t.run_command ? 'run' : t.name}</span>
      </span>
      <span class="truncate pl-3 text-[13px] text-muted @max-[760px]:hidden">terminal</span>
      <span class="truncate pl-3 font-mono text-[12px] text-muted @max-[760px]:hidden">{t.run_command ?? ''}</span>
      <span class="text-right">
        {#if t.run_command}
          {@const w = app.model.worktrees.find((x) => x.id === t.worktree_id)}
          {#if w}<RunButton worktree={w} label="" />{/if}
        {/if}
      </span>
    </div>
  {/each}
</div>
