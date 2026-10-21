<script lang="ts">
  import { app, goHome, openDiff, projectOf, sessionWorktree } from './store.svelte';
  import { toneOf, terminalTone, harnessLabel, placeOf, age, lastPrompt } from './status';
  import Status from './Status.svelte';
  import StopBadge from './StopBadge.svelte';
  import Terminal from './Terminal.svelte';
  import OrchBadge from './orchestrator/OrchBadge.svelte';
  import RoleChip from './roles/RoleChip.svelte';
  import ContextBar from './accounts/ContextBar.svelte';
  import SessionRail from './SessionRail.svelte';
  import SessionSubagents from './SessionSubagents.svelte';
  import { clampPane } from './subagents';
  import { followUp, newTerminal, openTargetMenu, restartAgent, unarchiveAgent } from './actions';
  import { openMenu } from './ui/ui.svelte';
  import { ownMenu } from './menus/router';
  import { toggleMaximize } from './commands';
  import { withKey } from './shortcuts';
  import IconButton from './ui/IconButton.svelte';
  import EditorButtons from './editors/EditorButtons.svelte';
  import { accountMenu } from './accounts/actions';
  import { accountLabel, canSwitch } from './accounts/view';

  /** The account chip: where this session runs, and the move menu. */
  function accounts(at: HTMLElement) {
    if (agent) openMenu(accountMenu(agent.id), at, 'Account');
  }

  function actions(at: HTMLElement | { x: number; y: number }) {
    if (agent) openTargetMenu({ kind: 'agent', id: agent.id }, at);
    else if (terminal) openTargetMenu({ kind: 'terminal', id: terminal.id }, at);
  }

  const s = $derived(app.session);
  const agent = $derived(s && 'Agent' in s ? app.model.agents.find((a) => a.id === s.Agent) : undefined);
  const terminal = $derived(
    s && 'Terminal' in s ? app.model.terminals.find((t) => t.id === s.Terminal) : undefined,
  );
  const wt = $derived(sessionWorktree(s));
  const where = $derived(wt ? placeOf(app.model, wt) : null);
  const tone = $derived(agent ? toneOf(agent) : terminal ? terminalTone(terminal) : 'idle');
  const prompt = $derived(agent ? lastPrompt(agent) : null);
  const target = $derived(agent ? `agent:${agent.id}` : terminal ? `terminal:${terminal.id}` : undefined);
  // The details rail beside the terminal; hidden by the reader, and
  // always in a pane too narrow to spare it (the header has the essentials).
  let railOpen = $state(true);

  // The subagent list under the chat: its height and the chat collapse are
  // remembered for the next session opened (per viewer, so browser storage).
  const PANE_KEY = 'kete.subagentPane';
  const PANE_DEFAULT = 200;
  function readPane(): { height: number; collapsed: boolean } {
    try {
      const v = JSON.parse(localStorage.getItem(PANE_KEY) ?? '{}');
      return { height: Number(v.height) || PANE_DEFAULT, collapsed: v.collapsed === true };
    } catch {
      return { height: PANE_DEFAULT, collapsed: false };
    }
  }
  const saved = readPane();
  let paneHeight = $state(saved.height);
  let chatCollapsed = $state(saved.collapsed);
  let column = $state<HTMLElement>();
  function keepPane() {
    try {
      localStorage.setItem(PANE_KEY, JSON.stringify({ height: paneHeight, collapsed: chatCollapsed }));
    } catch {
      /* storage can be blocked; the position just is not remembered */
    }
  }
  function setPaneHeight(px: number, commit: boolean) {
    paneHeight = clampPane(px, column?.clientHeight ?? 600);
    if (commit) keepPane();
  }
  function toggleChat() {
    chatCollapsed = !chatCollapsed;
    keepPane();
  }
</script>

{#if s && (agent || terminal)}
  <div class="flex h-full min-h-0 flex-col">
    <!-- The header is the session's row: with one of its controls focused,
         the row keys (Space, T, Shift+O, A, Del) act on this session. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="@container flex min-w-0 flex-none flex-col gap-1 border-b border-rule px-4 py-2"
      data-target={target}
    >
      <div class="flex min-w-0 items-center gap-3">
        <span class="min-w-0 truncate text-[15px] font-semibold {tone === 'needs' ? 'text-ember-ink' : 'text-ink'}"
          >{agent?.name ?? terminal?.name}</span
        >
        {#if agent?.role === 'orchestrator'}<OrchBadge />{/if}
        {#if agent}<RoleChip agent={agent.id} />{/if}
        <Status {tone} />
        <!-- A usage limit it waits out, or why it stopped: "running, resumes 3:45pm".
             Off in a narrow header, where it would squeeze the name away: the
             session row and the board card still carry it. -->
        <StopBadge note={agent ? app.model.stops?.[agent.id] : null} class="self-center @max-[560px]:hidden" />
        <span class="flex-1"></span>
        <div class="flex flex-none items-center gap-1" role="toolbar" aria-label="Session">
          {#if agent?.archived}
            <IconButton icon="unarchive" label="Unarchive" word="Unarchive" title={withKey('Unarchive', 'unarchive')} onclick={() => unarchiveAgent(agent.id)} />
          {:else if agent}
            <IconButton
              icon="reply"
              label="Follow-up prompt"
              word="Follow-up"
              title={withKey(agent.alive ? 'Follow-up prompt' : 'Follow-up prompt (not running: restart it first)', 'follow_up')}
              onclick={() => followUp(agent.id)}
            />
            <IconButton icon="restart" label="Restart the agent" title={withKey('Restart the agent', 'restart')} onclick={() => restartAgent(agent.id)} />
          {/if}
          <!-- Secondary verbs give way in a narrow pane; Actions keeps them. -->
          <span class="hidden items-center gap-1 @min-[560px]:flex">
            <IconButton icon="terminal" label="New terminal in this worktree" title={withKey('New terminal in this worktree', 'new_terminal')} onclick={() => newTerminal(wt ?? undefined)} />
            <EditorButtons
              worktree={app.model.worktrees.find((x) => x.id === wt)}
              btn="inline-flex h-7 min-w-7 flex-none items-center justify-center border border-rule bg-transparent px-1.5 text-ink hover:border-rule-strong"
            />
          </span>
          {#if agent}
            <button
              class="hidden h-7 flex-none items-center border border-rule px-1.5 text-[12px] text-ink hover:border-rule-strong @min-[720px]:inline-flex {railOpen ? 'bg-surface' : 'bg-transparent'}"
              aria-pressed={railOpen}
              title={railOpen ? 'Hide the session details' : 'Show the session details: where it runs, context, changed files'}
              onclick={() => (railOpen = !railOpen)}>Details</button
            >
          {/if}
          <IconButton icon="diff" label="Diff this worktree" word="Diff" title={withKey('Diff this worktree', ['diff', 'open_diff'])} onclick={() => openDiff(wt)} />
          <IconButton
            icon={app.paneMax ? 'restore' : 'maximize'}
            label={app.paneMax ? 'Bring the pane back down' : 'Full pane'}
            title={withKey(app.paneMax ? 'Bring the pane back down' : 'Full pane: give the session the window', ['maximize_z', 'maximize'])}
            pressed={app.paneMax}
            onclick={toggleMaximize}
          />
          <IconButton
            icon="dots"
            label="Session actions"
            menu
            title={withKey('Session actions: rename, move, duplicate, archive, delete (or right-click the header)', 'row_menu')}
            onclick={(e) => actions(e.currentTarget as HTMLElement)}
          />
          <IconButton icon="close" label="Close the split" title="Close the split (Esc)" onclick={() => goHome(projectOf(wt))} />
        </div>
      </div>
      <div class="flex min-w-0 items-baseline gap-3 text-[13px] text-muted">
        {#if agent}
          <span class="flex-none">{harnessLabel(agent)}</span>
          <button
            class="inline-flex h-5 flex-none items-center gap-1 self-center border border-rule bg-transparent px-1.5 font-mono text-[11px] text-ink hover:border-rule-strong"
            aria-haspopup="menu"
            use:ownMenu={() => (agent ? { items: accountMenu(agent.id), label: 'Account' } : null)}
            title={canSwitch(app.model)
              ? `Runs on ${agent.profile?.trim() || "its harness's own login"}. Move it to another account (restarts fresh in this worktree)`
              : `Runs on ${agent.profile?.trim() || "its harness's own login"}. Moving accounts needs a newer daemon`}
            onclick={(e) => accounts(e.currentTarget as HTMLElement)}
            >{accountLabel(agent)}<svg width="7" height="7" viewBox="0 0 8 8" aria-hidden="true"><path d="M1 2.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.2" /></svg
            ></button
          >
        {/if}
        {#if where && wt}
          <!-- Its place: the repo's and the checkout's own menus on a
               right-click (the checkout's lists its sessions to open). -->
          <span class="flex-none font-mono text-[12px]" title="{where.path}. Right-click the repo or the branch for its menu"
            ><span data-target="project:{projectOf(wt) ?? ''}">{where.project}</span> / <span data-target="worktree:{wt}" data-menu-jump
              >{where.branch}</span
            ></span
          >
        {/if}
        {#if agent}<ContextBar context={app.model.contexts?.[agent.id]} wide />{/if}
        {#if prompt}<span class="min-w-0 truncate" title={prompt}>{prompt}</span>{/if}
        {#if agent}<span class="ml-auto flex-none font-mono text-[12px]">{age(agent.status_changed_at, app.now)}</span>{/if}
      </div>
    </div>
    <div class="@container flex min-h-0 flex-1">
      <div class="flex min-h-0 min-w-0 flex-1 flex-col" bind:this={column} data-session-column>
        <div class="min-h-0 flex-1 {agent && chatCollapsed && app.model.subagents.some((x) => x.parent_agent === agent.id) ? 'hidden' : ''}" data-chat>
          <Terminal session={s} dark={app.dark} />
        </div>
        {#if agent}
          <SessionSubagents agent={agent.id} height={paneHeight} collapsed={chatCollapsed} onheight={setPaneHeight} ontoggle={toggleChat} />
        {/if}
      </div>
      {#if agent && railOpen}
        <div class="hidden @min-[720px]:flex"><SessionRail {agent} /></div>
      {/if}
    </div>
  </div>
{:else}
  <div class="grid h-full place-items-center text-[14px] text-muted">
    <div class="flex flex-col items-center gap-3">
      <p class="m-0">That session is gone. Pick another in the overview.</p>
      <button
        class="h-7 border border-rule bg-transparent px-3 text-[13px] text-ink hover:border-rule-strong"
        title="Back to the overview (Esc)"
        onclick={() => goHome(projectOf(wt))}>Back to the overview</button
      >
    </div>
  </div>
{/if}
