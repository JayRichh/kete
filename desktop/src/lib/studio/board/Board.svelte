<script lang="ts">
  // STUDIO BOARD: a repo's departments as columns. Each column holds the
  // notes queued to it, its lead and runner sessions, its last verdict,
  // what is blocked on Jay and today's spend. Above them, What needs you.
  // The file half comes from `studio_board` (every 10 s while visible, on
  // focus and on a repo change); sessions, roles and spend come from the
  // store, so they move as it does. The join is board.ts.
  import { untrack } from 'svelte';
  import { api } from '../../api';
  import { app, openSession } from '../../store.svelte';
  import { nav } from '../../shell/history.svelte';
  import { showWorktreeFile } from '../../files/files.svelte';
  import { agentMenu } from '../../actions';
  import { ownMenu } from '../../menus/router';
  import { openMenu, type MenuEntry, type MenuItem } from '../../ui/ui.svelte';
  import IconButton from '../../ui/IconButton.svelte';
  import { age } from '../../status';
  import { localDay } from '../../ledger/spend';
  import { gauntletItem } from '../types';
  import Dot from './Dot.svelte';
  import {
    UNASSIGNED,
    atMs,
    budgetLevel,
    contextWord,
    joinBoard,
    tokensWord,
    type BoardNote,
    type BoardReply,
    type Cell,
    type Column,
    type Verdict,
  } from './board';

  let { repo }: { repo: string | null } = $props();

  let reply = $state<BoardReply | null>(null);
  let failure = $state<string | null>(null);
  let now = $state(Date.now());
  let seq = 0;

  async function load(r: string) {
    const n = ++seq;
    try {
      const d = (await api.studioBoard(r)) as BoardReply;
      if (n !== seq) return;
      reply = d;
      failure = null;
      now = Date.now();
    } catch (e) {
      if (n !== seq) return;
      failure = String(e);
    }
  }

  // A repo (or its change) starts over; then it refreshes on a timer while
  // the page is visible, and when the window regains focus.
  $effect(() => {
    const r = repo;
    untrack(() => {
      reply = null;
      failure = null;
      seq++;
    });
    if (!r) return;
    void load(r);
    const tick = () => {
      if (document.visibilityState === 'visible') void load(r);
    };
    const timer = setInterval(tick, 10_000);
    window.addEventListener('focus', tick);
    return () => {
      clearInterval(timer);
      window.removeEventListener('focus', tick);
    };
  });

  const board = $derived(
    repo && reply?.manifest !== false && reply
      ? joinBoard({
          reply,
          agents: app.model.agents,
          worktrees: app.model.worktrees,
          contexts: app.model.contexts ?? {},
          roles: app.roles,
          spend: app.spend,
          repo,
          today: localDay(new Date(now)),
        })
      : null,
  );

  const NO_CHECKOUT = 'This repo has no checkout Kete knows';
  const canOpenNote = $derived(!!reply?.worktree);

  function showNote(n: BoardNote) {
    if (reply?.worktree) showWorktreeFile(reply.root, reply.worktree, n.rel, n.path);
  }
  function showVerdict(v: Verdict) {
    nav.go({ view: 'gauntlet', repo: repo ?? undefined, item: gauntletItem(v.kind, v.id) });
  }
  const open = (c: Cell) => openSession({ Agent: c.agent.id });

  /** A column's menu: only the verbs that have something to act on. */
  function columnMenu(col: Column): MenuItem[] {
    const items: MenuItem[] = [];
    const lead = col.lead;
    if (lead) items.push({ label: 'Open lead', title: lead.label, run: () => open(lead) });
    const v = col.verdict;
    if (v) items.push({ label: 'Review last verdict', title: v.id, run: () => showVerdict(v) });
    const top = col.queue[0];
    if (top && canOpenNote) items.push({ label: 'Open top note', title: top.title, run: () => showNote(top) });
    return items;
  }
  const columnOwn = (col: Column) => () => ({ items: columnMenu(col) as MenuEntry[], label: `${col.label} column` });
  const sessionOwn = (c: Cell) => () => ({ items: agentMenu(c.agent), label: `session ${c.label}` });

  function dotsAt(e: MouseEvent, items: MenuEntry[], label: string) {
    openMenu(items, e.currentTarget as HTMLElement, label);
  }

  const money = (n: number) => `est. US$${n.toFixed(2)}`;
  const ago = (ms: number | null) => (ms ? age(ms, now) : '');
  const levelMark = { ok: '', amber: '\u25b2 ', red: '\u2715 ' };
  const levelClass = { ok: 'text-muted', amber: 'text-git-modified', red: 'text-fail' };
  const levelWord = { ok: 'within budget', amber: 'over 60% of budget', red: 'over 90% of budget' };

  function ctxTitle(c: Cell): string {
    if (c.contextTokens == null) return 'No context reading yet';
    if (c.percent == null) return `Context: ${c.contextTokens} tokens, no budget set`;
    return `Context: ${c.contextTokens} tokens, ${c.percent}% of budget, ${levelWord[budgetLevel(c.percent)]}`;
  }
  function cellTitle(c: Cell): string {
    return `${c.label}: ${c.agent.status.replace('_', ' ')}. ${ctxTitle(c)}. Click to open the session.`;
  }
</script>

{#snippet context(c: Cell)}
  {@const lvl = c.percent == null ? 'ok' : budgetLevel(c.percent)}
  <span class="flex-none font-mono text-[12px] {levelClass[lvl]}" title={ctxTitle(c)}>
    <span aria-hidden="true">{levelMark[lvl]}</span>{contextWord(c)}<span class="sr-only">, {c.percent == null ? 'no budget level' : levelWord[lvl]}</span>
  </span>
{/snippet}

{#if repo}
  <div class="flex min-w-0 flex-col gap-3" data-studio-board>
    {#if failure && !reply}
      <p class="m-0 text-[13px] text-fail" role="alert">The board could not be read: {failure}</p>
    {:else if !reply}
      <p class="m-0 text-[13px] text-muted">Reading the board&#x2026;</p>
    {:else if reply.manifest === false}
      <p class="m-0 max-w-[60ch] text-[13px] text-muted" data-board-empty>
        This repo has no <code class="font-mono text-ink">.kete/studio.toml</code>, so it has no departments.
      </p>
    {:else if board}
      {#if reply.error}
        <p class="m-0 border border-fail px-3 py-2 text-[13px] text-fail" role="alert">
          <span aria-hidden="true">&#x2715; </span>Manifest error: {reply.error}
        </p>
      {/if}

      <section class="border border-rule" data-board-needs-you aria-label="What needs you">
        <h2 class="m-0 flex items-center gap-2 border-b border-rule px-3 py-1.5 text-[12px] font-medium text-muted">
          What needs you
          {#if board.needsYou.length}<span class="font-mono text-ember-ink">{board.needsYou.length}</span>{/if}
        </h2>
        {#if board.needsYou.length === 0}
          <p class="m-0 px-3 py-2 text-[13px] text-muted">Nothing needs you</p>
        {:else}
          <ul class="m-0 flex list-none flex-col p-0">
            {#each board.needsYou as n, i (i)}
              <li class="flex min-w-0 items-baseline gap-2 border-b border-rule px-3 py-1.5 last:border-b-0">
                <span class="flex-none font-mono text-[12px] text-ember-ink" aria-hidden="true">!</span>
                {#if n.kind === 'session'}
                  <button
                    class="min-w-0 flex-1 truncate border-0 bg-transparent p-0 text-left text-[13px] text-ink hover:underline"
                    title="Open {n.cell.label}, which is waiting for you"
                    onclick={() => open(n.cell)}>{n.cell.label} is waiting for you</button
                  >
                {:else if n.kind === 'note'}
                  <button
                    class="min-w-0 flex-1 truncate border-0 bg-transparent p-0 text-left text-[13px] text-ink enabled:hover:underline disabled:text-muted"
                    data-board-note={n.note.id}
                    disabled={!canOpenNote}
                    title={canOpenNote ? `Open ${n.note.rel}` : NO_CHECKOUT}
                    onclick={() => showNote(n.note)}>Note to you: {n.note.title}</button
                  >
                  <span class="flex-none font-mono text-[11px] text-muted">{n.note.priority}{#if ago(atMs(n.note.at))} &#xb7; {ago(atMs(n.note.at))}{/if}</span>
                {:else if n.kind === 'refusal'}
                  <span class="min-w-0 flex-1 truncate text-[13px] text-ink" title={n.refusal.reason}>
                    Refused{n.refusal.role ? ` (${n.refusal.role})` : ''}: {n.refusal.reason}
                  </span>
                  <span class="flex-none font-mono text-[11px] text-muted">{ago(n.refusal.at)}</span>
                {:else}
                  <span class="min-w-0 flex-1 truncate text-[13px] text-ink" title={n.renewal.reason}>Orchestrator renewal failed: {n.renewal.reason}</span>
                  <span class="flex-none font-mono text-[11px] text-muted">{ago(n.renewal.at)}</span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      {#if board.columns.length === 0}
        <p class="m-0 max-w-[60ch] text-[13px] text-muted">No departments: add a departments table to the manifest's departments file.</p>
      {:else}
        <!-- relative: the cells' sr-only words are absolute, and would
             widen the page past a scroller that is not their containing block. -->
        <div class="relative flex min-w-0 items-start gap-3 overflow-x-auto pb-2">
          {#each board.columns as col (col.slug)}
            {@const un = col.slug === UNASSIGNED}
            {@const menu = columnMenu(col)}
            <section class="flex w-[240px] flex-none flex-col border border-rule bg-bg" data-board-column={col.slug} aria-label={col.label}>
              <header class="flex items-center gap-2 border-b border-rule py-1.5 pr-1.5 pl-3" use:ownMenu={columnOwn(col)}>
                <h2 class="m-0 min-w-0 flex-1 truncate text-[13px] font-semibold text-ink" title={col.label}>{col.label}</h2>
                {#if !un}
                  <span class="flex-none text-[12px] text-muted" title="Open notes queued to {col.label}">
                    <span class="font-mono text-ink" data-board-queue-count>{col.queue.length}</span> queued
                  </span>
                {/if}
                <IconButton
                  icon="dots"
                  label="Column actions"
                  title={menu.length ? `Actions for ${col.label}` : `Nothing to do in ${col.label} yet`}
                  menu
                  disabled={!menu.length}
                  onclick={(e) => dotsAt(e, menu as MenuEntry[], `${col.label} column`)}
                />
              </header>

              {#if !un}
                <div class="flex flex-col border-b border-rule">
                  {#each col.queue.slice(0, 3) as note (note.id)}
                    <button
                      class="flex min-w-0 flex-col gap-0.5 border-0 border-b border-rule bg-transparent px-3 py-1.5 text-left last:border-b-0 enabled:hover:bg-surface disabled:text-muted"
                      data-board-note={note.id}
                      disabled={!canOpenNote}
                      title={canOpenNote ? `Open ${note.rel}` : NO_CHECKOUT}
                      onclick={() => showNote(note)}
                    >
                      <span class="truncate text-[13px] text-ink">{note.title}</span>
                      <span class="font-mono text-[11px] {note.priority === 'High' ? 'text-ink' : 'text-muted'}"
                        >{note.priority}{#if ago(atMs(note.at))} &#xb7; {ago(atMs(note.at))}{/if}</span
                      >
                    </button>
                  {:else}
                    <p class="m-0 px-3 py-1.5 text-[12px] text-muted">No notes queued</p>
                  {/each}
                  {#if col.queue.length > 3}
                    <p class="m-0 px-3 pb-1.5 text-[12px] text-muted">{col.queue.length - 3} more queued</p>
                  {/if}
                </div>
              {/if}

              <div class="flex flex-col gap-1.5 border-b border-rule px-3 py-2">
                {#if col.lead}
                  {@const lead = col.lead}
                  <div class="flex min-w-0 items-center gap-1" use:ownMenu={sessionOwn(lead)}>
                    <button
                      class="flex min-w-0 flex-1 flex-col gap-0.5 border-0 bg-transparent p-0 text-left hover:bg-surface"
                      title={cellTitle(lead)}
                      onclick={() => open(lead)}
                    >
                      <span class="flex min-w-0 items-center gap-2">
                        <Dot agent={lead.agent} word />
                      </span>
                      <span class="flex min-w-0 items-baseline gap-2">
                        <span class="min-w-0 flex-1 truncate text-[13px] font-medium text-ink">Lead: {lead.label}</span>
                        {@render context(lead)}
                      </span>
                      <span class="flex min-w-0 gap-2 text-[11px] text-muted">
                        {#if lead.model}<span class="truncate" title="Model">{lead.model}</span>{/if}
                        {#if lead.profile}<span class="truncate" title="Account">{lead.profile}</span>{/if}
                      </span>
                    </button>
                    <IconButton icon="dots" label="Session actions" title="Actions for {lead.label}" menu onclick={(e) => dotsAt(e, agentMenu(lead.agent), `session ${lead.label}`)} />
                  </div>
                {:else if !un}
                  <p class="m-0 text-[12px] text-muted">No lead running</p>
                {/if}
                {#if col.runners.length}
                  <ul class="m-0 flex list-none flex-wrap gap-1 p-0">
                    {#each col.runners as r (r.agent.id)}
                      <li use:ownMenu={sessionOwn(r)}>
                        <button
                          class="inline-flex h-6 max-w-full items-center gap-1.5 border border-rule bg-transparent px-1.5 text-[12px] text-ink hover:border-rule-strong"
                          title="{cellTitle(r)} Right-click for more."
                          onclick={() => open(r)}
                        >
                          <Dot agent={r.agent} />
                          <span class="max-w-[100px] truncate">{r.label}</span>
                          {@render context(r)}
                        </button>
                      </li>
                    {/each}
                  </ul>
                {/if}
                {#if !col.lead && !col.runners.length}
                  <p class="m-0 text-[12px] text-muted">No sessions</p>
                {/if}
              </div>

              <div class="flex flex-col gap-1.5 px-3 py-2 text-[12px]">
                {#if !un}
                  {#if col.verdict}
                    {@const v = col.verdict}
                    <button
                      class="flex min-w-0 items-baseline gap-2 border-0 bg-transparent p-0 text-left hover:underline"
                      title="Open verdict {v.id} in the gauntlet"
                      onclick={() => showVerdict(v)}
                    >
                      <span class="flex-none text-muted">Verdict</span>
                      <span class="flex-none font-medium text-ink">{v.verdict}</span>
                      <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-muted">{v.id}</span>
                      <span class="flex-none font-mono text-[11px] text-muted">{ago(atMs(v.at, v.modified))}</span>
                    </button>
                  {:else}
                    <span class="text-muted">No verdict yet</span>
                  {/if}
                {/if}
                <span class={col.blocked ? 'font-medium text-ember-ink' : 'text-muted'} title="Notes to Jay about this department, and its sessions waiting on a reply">
                  {#if col.blocked}<span aria-hidden="true">! </span>{/if}Blocked on Jay: <span class="font-mono" data-board-blocked>{col.blocked}</span>
                </span>
                <span class="text-muted" title="Input plus output tokens today, and a list-price estimate">
                  Today: <span class="font-mono text-ink">{tokensWord(col.spend.tokens)}</span> tokens, <span class="font-mono text-ink">{money(col.spend.cost_usd)}</span>
                </span>
              </div>
            </section>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
{/if}
