<script lang="ts">
  import { app, focusQuickPrompt, markSeen, openReview, openSession, openDiff, setScope, toast } from './store.svelte';
  import { offerTip } from './learn.svelte';
  import { LANE_EMPTY, emptyCopy } from './learn';
  import { verbKeys } from './verbs';
  import { LANES, lanes, toneOf, subagentTone, harnessLabel, lastPrompt, age, placeOf, type Lane } from './status';
  import { cardSubagents, isEnded, movedAt, subagentRun, subagentWord } from './subagents';
  import { clearFinishedSubagents, foldingFinished } from './subagent-actions';
  import SubagentBar from './SubagentBar.svelte';
  import Status from './Status.svelte';
  import StopBadge from './StopBadge.svelte';
  import { retract } from './motion';
  import Empty from './Empty.svelte';
  import type { Agent, DropTarget } from './api';
  import { agentMenu, archiveAgent, archiveAndOfferWorktree, followUp, unarchiveAgent } from './actions';
  import { layerOpen, openMenu } from './ui/ui.svelte';
  import { keyLabel, withKey } from './shortcuts';
  import Icon from './ui/Icon.svelte';
  import { drag, dropCard, endDrag, leftFor, offsetIn, startDrag } from './reorder.svelte';
  import { dropZone, type Place } from './order';
  import { onMount } from 'svelte';
  import { rows, watchGit } from './worktrees/rows.svelte';
  import { commitsWord, dropHint, dropVerbOf, laneLook, prChip } from './ship/ship';
  import { openComposer, openMerge } from './ship/sheet.svelte';
  import { hiddenTotal } from './scope';
  // A Files row dropped on a card types its path into that session.
  import { pathDrop } from './files/drop.svelte';

  // The Review lane reads each checkout's changes off the git reads the
  // overview already makes (src-tauri keeps the last one); keep them
  // coming while the board is up in its place.
  onMount(watchGit);

  let { active = true }: { active?: boolean } = $props();

  // Drags on the board. What a card let go on a lane does is core's
  // (`Card.drops`, snapshotted as the drag starts): Review to Done marks it
  // seen, Review to Ship opens the PR composer, Ship to Done the merge
  // confirm, the Archived zone (shown while dragging) archives it, from
  // Done offering its emptied worktree too; a drop beside a card of its own
  // checkout reorders them. A lane that refuses dims while the card is over
  // it and says why there, and takes no drop. The keys: Alt+Up / Alt+Down
  // reorder, A archives, the menu's Mark seen, Open PR and Merge.
  /** The lane under the pointer, and the card it would land beside. */
  let target = $state<{ lane: DropTarget; card?: { id: string; place: Place } } | null>(null);
  const dragged = $derived(drag.item?.kind === 'agent' && drag.item.lane ? drag.item : null);
  $effect(() => {
    if (!dragged) target = null;
  });

  function overLane(e: DragEvent, lane: DropTarget) {
    if (!dragged) return;
    if (target?.lane !== lane || target.card) target = { lane };
    // A refusing lane never takes the drop: the pointer says no, the lane
    // says why.
    if (laneLook(dragged.drops, dragged.lane!, lane) === 'refuse') return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
  }

  function overCard(e: DragEvent, lane: Lane, a: Agent) {
    if (!dragged || dragged.lane !== lane || dragged.id === a.id) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    // Only beside a card of its own checkout: the order is per checkout.
    if (a.worktree_id !== dragged.worktree) return void (target = { lane });
    const [y, h] = offsetIn(e);
    target = { lane, card: { id: a.id, place: dropZone(y, h, { insert: true, onto: false }) as Place } };
  }

  async function dropOn(e: DragEvent, lane: DropTarget, over?: Agent) {
    const d = dragged;
    const at = target;
    target = null;
    if (!d?.lane) return;
    e.preventDefault();
    e.stopPropagation();
    endDrag();
    const v = dropVerbOf(d.drops, lane);
    switch (v.act) {
      case 'refuse':
        return;
      case 'mark_seen':
        return markSeen({ Agent: d.id });
      case 'open_pr':
        return openComposer(d.worktree, d.id);
      case 'merge_pr':
        return openMerge(d.worktree, d.id);
      case 'archive':
        return void archiveAgent(d.id);
      case 'archive_and_offer_worktree_delete':
        return archiveAndOfferWorktree(d.id);
      case 'reorder':
        if (at?.card) return dropCard(d.worktree, d.id, at.card.id, at.card.place);
        if (over && over.id !== d.id && over.worktree_id !== d.worktree)
          toast(`Cards reorder beside cards of their own worktree (${placeOf(app.model, d.worktree).branch}); move the session (M) to change worktree.`);
    }
  }

  function endBoardDrag() {
    target = null;
    endDrag();
  }

  /** What a lane does while a card is dragged: `act` takes an ember rule
   *  under the pointer, `refuse` dims with its reason. */
  function lookOf(lane: DropTarget): 'act' | 'refuse' | 'none' {
    if (!dragged) return 'none';
    return laneLook(dragged.drops, dragged.lane!, lane);
  }
  function laneMark(lane: Lane): string {
    const look = lookOf(lane);
    if (look === 'refuse') return 'opacity-55';
    if (look === 'act' && target?.lane === lane && !target.card) return 'outline-2 -outline-offset-2 outline-ember';
    return '';
  }

  const board = $derived(lanes(app.model));
  const total = $derived(LANES.reduce((n, l) => n + board[l.id].length, 0));
  const outside = $derived(hiddenTotal(app.model));

  // Subagents under their session's card. Finished ones fold into a count
  // while `hide_done_subagents` is on (the default); a card's count opens
  // its own, SubagentBar's checkbox flips the setting for both apps.
  const hideDone = $derived(foldingFinished());
  let openFinished = $state<Record<string, boolean>>({});
  const clearFinished = clearFinishedSubagents;

  // First-use tips for the board's own concepts, while it is the one up:
  // a card reaching Review (the lanes), then two cards to drag.
  $effect(() => {
    if (!active) return;
    if (board.unseen.length) offerTip('lanes');
    if (total >= 2) offerTip('drag');
  });
  /** An empty lane's line and the verb that fills it, with its key. */
  function emptyOf(lane: Lane) {
    const e = emptyCopy(LANE_EMPTY[lane]);
    const key = e.verb ? verbKeys(e.verb)[0] : undefined;
    return { ...e, key: key ? keyLabel(key) : null };
  }

  // The selected card, by id: opening one marks it seen, which can move it
  // to another lane; the selection follows it there.
  let selId = $state<string | null>(null);
  const cursor = $derived.by(() => {
    if (!selId) return null;
    for (let lane = 0; lane < LANES.length; lane++) {
      const row = board[LANES[lane].id].findIndex((a) => a.id === selId);
      if (row >= 0) return { lane, row };
    }
    return null;
  });

  function selectedAgent(): Agent | null {
    return cursor ? (board[LANES[cursor.lane].id][cursor.row] ?? null) : null;
  }

  /** Arrow-key step: across lanes (skipping empty ones) or down a lane. */
  function step(dx: number, dy: number) {
    const ids = LANES.map((l) => board[l.id]);
    if (!cursor) {
      const lane = ids.findIndex((l) => l.length);
      if (lane >= 0) selId = ids[lane][0].id;
      return focusSel();
    }
    let { lane, row } = cursor;
    if (dx) {
      let next = lane;
      do next += dx;
      while (next >= 0 && next < ids.length && !ids[next].length);
      if (next < 0 || next >= ids.length) return;
      lane = next;
      row = Math.min(row, ids[lane].length - 1);
    } else {
      row = Math.max(0, Math.min(ids[lane].length - 1, row + dy));
    }
    selId = ids[lane][row]?.id ?? selId;
    focusSel();
  }

  /** The card takes the focus, so its row keys (Space, A, Delete and the rest) reach it. */
  function focusSel() {
    queueMicrotask(() => {
      const el = document.querySelector<HTMLElement>('[data-card-sel]');
      el?.focus();
      el?.scrollIntoView({ block: 'nearest' });
    });
  }

  /** Enter or a click: a Review or Ship card opens its review (the board
   *  stays left, the worktree's diff against its base on the right, its
   *  verbs above it); the rest open the session. */
  function openCard(a: Agent, lane: Lane) {
    selId = a.id;
    if (lane === 'unseen' || lane === 'pr') {
      openReview(a.id);
      // Read, the card may move lanes and its node with it: keep the focus
      // on it, so the board's keys still work beside the diff.
      setTimeout(() => {
        if (document.activeElement === document.body) focusSel();
      }, 250);
    } else openSession({ Agent: a.id });
  }

  function onKey(e: KeyboardEvent) {
    if (layerOpen()) return;
    const t = e.target as HTMLElement;
    // Beside an open session or diff the board still answers once the
    // focus is in it.
    if (!active && !t.closest('[data-board]')) return;
    if (t.closest('input, textarea, select, .xterm, .monaco-editor')) return;
    const map: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0], h: [-1, 0], ArrowRight: [1, 0], l: [1, 0],
      ArrowUp: [0, -1], k: [0, -1], ArrowDown: [0, 1], j: [0, 1],
    };
    if (map[e.key] && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      step(...map[e.key]);
    } else if (e.key === 'Enter') {
      // A card's own action button takes its Enter itself.
      if (t.closest('[data-card-tools]')) return;
      const a = selectedAgent();
      if (a && cursor) openCard(a, LANES[cursor.lane].id);
    }
  }

  /** A card's Diff (D is the window's, which does the same for the
   *  focused card): the card becomes the session, its worktree diffed. */
  function diffCard(a: Agent) {
    app.session = { Agent: a.id };
    markSeen(app.session);
    openDiff(a.worktree_id);
  }

  const tool =
    'grid size-6 place-items-center border-0 border-l border-rule bg-transparent p-0 text-muted first:border-l-0 hover:bg-surface hover:text-ink';
  const chip = 'flex-none border px-1 font-mono text-[11px] leading-4';
  const CHIP_TONE = { open: 'border-rule-strong text-ink', draft: 'border-rule text-muted', merged: 'border-merged text-merged', closed: 'border-rule text-faint line-through' };
</script>

<svelte:window onkeydown={onKey} />

<!-- The alternate home: lanes are ruled columns, sessions are ruled rows. -->
{#if total === 0 && outside === 0}
  <Empty />
{:else}
  <div class="relative flex h-full min-h-0 flex-col" data-board>
  <SubagentBar />
  <div
    class="grid min-h-0 flex-1 grid-cols-[repeat(5,minmax(220px,1fr))] overflow-x-auto"
    role="grid"
    aria-label="Sessions by status"
  >
    {#each LANES as lane (lane.id)}
      {@const cards = board[lane.id]}
      {@const look = lookOf(lane.id)}
      {@const hidden = app.model.hidden?.[lane.id] ?? 0}
      {@const why = dragged && look !== 'none' ? dropHint(dropVerbOf(dragged.drops, lane.id)) : ''}
      <section
        class="relative flex min-h-0 min-w-0 flex-col border-r border-rule last:border-r-0 {laneMark(lane.id)}"
        aria-label={lane.title}
        title={look === 'refuse' ? why : undefined}
        ondragenter={(e) => overLane(e, lane.id)}
        ondragover={(e) => overLane(e, lane.id)}
        ondragleave={(e) => leftFor(e) && target?.lane === lane.id && (target = null)}
        ondrop={(e) => dropOn(e, lane.id)}
      >
        <!-- Its right-click: the lane's own verb (open the next, mark all
             seen, archive the done ones), menus/rows.ts laneItems. Critters
             perch on its strong rule. -->
        <header
          class="flex h-10 flex-none items-center gap-2 border-b border-rule-strong px-4"
          data-target="lane:{lane.id}"
          data-perch="bottom"
          title="{lane.title}: right-click for the lane's actions"
        >
          <h2 class="m-0 text-[15px] font-semibold {lane.id === 'needs' && cards.length ? 'text-ember-ink' : 'text-ink'}">{lane.title}</h2>
          <span class="ml-auto font-mono text-[12px] font-bold {cards.length ? 'text-ink' : 'text-muted'}">{cards.length}</span>
        </header>
        {#if why && target?.lane === lane.id}
          <!-- While a card is over the lane: what letting go does, or why it can't. -->
          <p
            class="m-0 flex-none border-b px-4 py-1.5 text-[12px] {look === 'refuse' ? 'border-rule text-muted' : 'border-ember text-ember-ink'}"
            role="status"
          >
            {why}
          </p>
        {/if}
        <div class="flex min-h-0 flex-col overflow-y-auto">
          {#each cards as a, ri (a.id)}
            {@const where = placeOf(app.model, a.worktree_id)}
            {@const prompt = lastPrompt(a)}
            {@const pr = app.model.prs[a.worktree_id]}
            {@const commits = commitsWord(rows.git[a.worktree_id]?.ahead_of_base)}
            {@const stop = app.model.stops?.[a.id]}
            {@const sel = cursor !== null && LANES[cursor.lane].id === lane.id && cursor.row === ri}
            {@const alert = lane.id === 'needs'}
            {@const subs = cardSubagents(app.model.subagents, a.id, hideDone, !!openFinished[a.id])}
            {@const hasSubs = subs.shown.length > 0 || subs.ended > 0}
            <!-- The card's verbs show on hover or focus; the card keys (Space, A,
                 D, Shift+F10) do the same from the card. -->
            {@const mark = dragged && target?.card?.id === a.id ? target.card.place : null}
            <div
              class="group relative {dragged?.id === a.id ? 'opacity-60' : ''}"
              data-target="agent:{a.id}"
              role="presentation"
              draggable="true"
              ondragstart={(e) => startDrag(e, { kind: 'agent', id: a.id, worktree: a.worktree_id, lane: lane.id, drops: { ...app.model.cards[a.id]?.drops } })}
              ondragend={endBoardDrag}
              ondragenter={(e) => overCard(e, lane.id, a)}
              ondragover={(e) => overCard(e, lane.id, a)}
              ondrop={(e) => dropOn(e, lane.id, a)}
              use:pathDrop={{ target: () => ({ Agent: a.id }) }}
            >
            {#if mark}
              <span class="pointer-events-none absolute inset-x-0 z-10 h-px bg-ember {mark === 'before' ? '-top-px' : '-bottom-px'}" aria-hidden="true"></span>
            {/if}
            <button
              title="{a.name}: {lane.id === 'unseen' || lane.id === 'pr' ? 'Enter opens its review (the diff against its base, with its verbs); ' : ''}{withKey('drag to reorder beside cards of its worktree', ['move_up', 'move_down'])}, or onto a lane, Archived or a repo in the rail"
              data-card-sel={sel || undefined}
              class="relative flex w-full flex-col gap-1 border-0 {hasSubs ? '' : 'border-b border-rule'} px-4 py-2.5 text-left focus-visible:-outline-offset-2
                {sel ? 'bg-surface' : 'bg-transparent hover:bg-surface'} {app.review === a.id ? 'shadow-[inset_3px_0_0_var(--color-ink)]' : ''}"
              onclick={() => openCard(a, lane.id)}
              onfocus={() => (selId = a.id)}
            >
              {#if alert}<span class="absolute inset-y-0 left-0 w-[3px] origin-left bg-ember" out:retract|global aria-hidden="true"></span>{/if}
              <span class="flex min-w-0 items-baseline gap-2">
                <span class="truncate text-[14px] font-medium {alert ? 'text-ember-ink' : 'text-ink'}">{a.name}</span>
                <span class="ml-auto font-mono text-[12px] text-muted">{age(a.status_changed_at, app.now)}</span>
              </span>
              <Status tone={lane.id === 'pr' ? 'pr' : lane.id === 'done' && pr?.state === 'merged' ? 'merged' : toneOf(a)} />
              <span class="truncate text-[13px] text-muted">{harnessLabel(a)}{a.profile ? ` \u00B7 ${a.profile}` : ''}</span>
              <span class="truncate font-mono text-[12px] text-muted">{where.project} / {where.branch}</span>
              {#if stop || commits || pr}
                <span class="flex min-w-0 flex-wrap items-center gap-1.5">
                  <StopBadge note={stop} />
                  {#if commits}<span class="{chip} border-rule text-muted" title="Commits on this branch since its base">{commits}</span>{/if}
                  {#if pr}
                    {@const c = prChip(pr)}
                    <span class="{chip} {CHIP_TONE[c.tone]}" data-target="pr:{a.worktree_id}" title="{pr.title}. Right-click for its menu.">{c.word}</span>
                  {/if}
                </span>
              {/if}
              {#if prompt}<span class="line-clamp-2 text-[13px] text-muted">{prompt}</span>{/if}
            </button>
            {#if hasSubs}
              <!-- Its subagents, under it: running first, finished folded into a count. -->
              <ul class="m-0 flex list-none flex-col gap-1 border-0 border-b border-rule pt-0 pr-4 pb-2 pl-6" aria-label="{a.name} subagents">
                {#each subs.shown as s (s.id)}
                  {@const run = subagentRun(s)}
                  <li class="flex min-w-0 items-center gap-2 text-[12px]" title="{[s.kind || 'subagent', run].filter(Boolean).join(' \u00b7 ')}: {s.description || 'no task seen'}">
                    <Status tone={subagentTone(s)} word={false} />
                    <span class="min-w-0 truncate {isEnded(s) ? 'text-faint' : 'text-muted'}">{s.description || s.kind || 'subagent'}</span>
                    {#if run}<span class="min-w-0 flex-none truncate text-[11px] text-faint">{run}</span>{/if}
                    <span class="ml-auto flex-none font-mono text-[11px] text-muted">{subagentWord(s)} {age(movedAt(s), app.now)}</span>
                  </li>
                {/each}
                {#if subs.ended}
                  <li class="flex items-center gap-2">
                    {#if hideDone}
                      <button
                        class="{chip} border-rule bg-transparent text-muted hover:text-ink"
                        aria-expanded={!subs.folded}
                        title={subs.folded ? 'Show the finished subagents' : 'Fold the finished subagents'}
                        onclick={() => (openFinished[a.id] = !openFinished[a.id])}>{subs.folded ? `${subs.ended} finished` : 'fold finished'}</button
                      >
                    {/if}
                    <button
                      class="{chip} border-rule bg-transparent text-muted hover:text-ink"
                      title="Remove this session's finished subagent records"
                      onclick={() => clearFinished(a.id)}>clear</button
                    >
                  </li>
                {/if}
              </ul>
            {/if}
            <div
              data-card-tools
              class="absolute top-2 right-3 hidden border border-rule bg-bg group-focus-within:flex group-hover:flex"
              role="toolbar"
              aria-label="{a.name} actions"
            >
              <button class={tool} title={withKey('Diff its worktree', ['diff', 'open_diff'])} aria-label="Diff {a.name}" onclick={() => diffCard(a)}><Icon name="diff" size={12} /></button>
              {#if a.archived}
                <button class={tool} title={withKey('Unarchive', 'unarchive')} aria-label="Unarchive {a.name}" onclick={() => unarchiveAgent(a.id)}><Icon name="unarchive" size={12} /></button>
              {:else}
                <button class={tool} title={withKey('Follow-up prompt', 'follow_up')} aria-label="Follow-up to {a.name}" onclick={() => followUp(a.id)}><Icon name="reply" size={12} /></button>
                <button class={tool} title={withKey('Archive', 'archive')} aria-label="Archive {a.name}" onclick={() => archiveAgent(a.id)}><Icon name="archive" size={12} /></button>
              {/if}
              <button
                class={tool}
                title={withKey('Actions: open PR, merge, restart, move, rename, delete', 'row_menu')}
                aria-label="{a.name} actions"
                aria-haspopup="menu"
                onclick={(e) => {
                  selId = a.id;
                  openMenu(agentMenu(a), e.currentTarget as HTMLElement, a.name);
                }}><Icon name="dots" size={12} /></button
              >
            </div>
            </div>
          {/each}
          {#if !cards.length}
            <!-- What lands in this lane, and the one thing to do about it. -->
            {@const e = emptyOf(lane.id)}
            <div class="flex flex-col items-start gap-2 px-4 py-3">
              <p class="m-0 text-[13px] text-muted">{e.text}</p>
              {#if e.verb === 'quick_prompt'}
                <button
                  class="inline-flex h-6 items-center gap-1.5 border border-rule bg-transparent px-2 text-[12px] text-ink hover:border-rule-strong"
                  title={withKey('New session: the quick prompt', ['quick_prompt', 'quick_prompt_alt'])}
                  onclick={() => focusQuickPrompt()}
                  >New session{#if e.key}<kbd class="font-mono text-[11px] text-faint">{e.key}</kbd>{/if}</button
                >
              {/if}
            </div>
          {/if}
          {#if hidden}
            <!-- The scope hides these: a click shows everything again. -->
            <button
              class="m-0 border-0 border-b border-rule bg-transparent px-4 py-2 text-left font-mono text-[12px] text-muted hover:bg-surface hover:text-ink"
              title="Clear the scope: show every repo, worktree and account"
              onclick={() => setScope({})}>{hidden} more outside scope</button
            >
          {/if}
        </div>
      </section>
    {/each}
  </div>
  {#if dragged}
    {@const look = lookOf('archived')}
    <!-- Only while a card is dragged: drop it here to archive it (A on the card). -->
    <div
      class="absolute inset-x-4 bottom-4 z-20 flex h-12 items-center justify-center gap-2 border bg-bg text-[13px] {target?.lane === 'archived' && look === 'act'
        ? 'border-ember text-ink'
        : 'border-rule-strong text-muted'}"
      role="region"
      aria-label="Archive"
      ondragenter={(e) => overLane(e, 'archived')}
      ondragover={(e) => overLane(e, 'archived')}
      ondragleave={(e) => leftFor(e) && target?.lane === 'archived' && (target = null)}
      ondrop={(e) => dropOn(e, 'archived')}
    >
      <Icon name="archive" size={12} />{dropHint(dropVerbOf(dragged.drops, 'archived')) || 'Drop here to archive'}
    </div>
  {/if}
  </div>
{/if}
