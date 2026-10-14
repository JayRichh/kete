<script lang="ts">
  import { app, goHome, openDiff, projectOf, sessionWorktree } from './store.svelte';
  import { toneOf, terminalTone, harnessLabel, placeOf, age, lastPrompt } from './status';
  import Status from './Status.svelte';
  import Terminal from './Terminal.svelte';
  import ContextBar from './accounts/ContextBar.svelte';
  import { agentMenu, terminalMenu } from './actions';
  import { openMenu } from './ui/ui.svelte';

  function actions(at: HTMLElement | { x: number; y: number }) {
    const items = agent ? agentMenu(agent) : terminal ? terminalMenu(terminal) : [];
    openMenu(items, at, agent?.name ?? terminal?.name ?? 'Session');
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

  const tool =
    'inline-flex h-7 items-center gap-1.5 border border-rule bg-transparent px-2 text-[13px] text-ink hover:border-rule-strong';
</script>

{#if s && (agent || terminal)}
  <div class="flex h-full min-h-0 flex-col">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="flex min-w-0 flex-none flex-col gap-1 border-b border-rule px-4 py-2"
      oncontextmenu={(e) => (e.preventDefault(), actions({ x: e.clientX, y: e.clientY }))}
    >
      <div class="flex min-w-0 items-center gap-3">
        <span class="min-w-0 truncate text-[15px] font-semibold {tone === 'needs' ? 'text-ember-ink' : 'text-ink'}"
          >{agent?.name ?? terminal?.name}</span
        >
        <Status {tone} />
        <span class="flex-1"></span>
        <button class={tool} onclick={() => openDiff(wt)} title="Diff this worktree (Ctrl+Shift+D)">Diff</button>
        <button
          class={tool}
          aria-haspopup="menu"
          title="Session actions: rename, restart, move, archive, delete (or right-click the header)"
          onclick={(e) => actions(e.currentTarget as HTMLElement)}>Actions</button
        >
        <button
          class="grid size-7 place-items-center border border-rule bg-transparent text-ink hover:border-rule-strong"
          onclick={() => goHome(projectOf(wt))}
          title="Close the split (Esc)"
          aria-label="Close the split"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="1.4" /></svg>
        </button>
      </div>
      <div class="flex min-w-0 items-baseline gap-3 text-[13px] text-muted">
        {#if agent}
          <span class="flex-none">{harnessLabel(agent)}{agent.profile ? ` \u00b7 ${agent.profile}` : ''}</span>
        {/if}
        {#if where}
          <span class="flex-none font-mono text-[12px]" title={where.path}>{where.project} / {where.branch}</span>
        {/if}
        {#if agent}<ContextBar context={app.model.contexts?.[agent.id]} wide />{/if}
        {#if prompt}<span class="min-w-0 truncate" title={prompt}>{prompt}</span>{/if}
        {#if agent}<span class="ml-auto flex-none font-mono text-[12px]">{age(agent.status_changed_at, app.now)}</span>{/if}
      </div>
    </div>
    <div class="min-h-0 flex-1">
      <Terminal session={s} dark={app.dark} />
    </div>
  </div>
{:else}
  <div class="grid h-full place-items-center text-[14px] text-muted">
    <p>That session is gone. Pick another in the overview.</p>
  </div>
{/if}
