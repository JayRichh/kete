<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { app } from '../store.svelte';
  import Empty from '../Empty.svelte';
  import SubagentBar from '../SubagentBar.svelte';
  import RepoRow from './RepoRow.svelte';
  import { repoRows } from './repos';
  import { scopeModel } from '../scope';
  import { refreshGit, shownIds, watchGit } from '../worktrees/rows.svelte';
  import { gitSignal } from '../worktrees/groups';

  let { active = true }: { active?: boolean } = $props();
  let scroller: HTMLDivElement;

  // The repos, worktrees and sessions the scope admits (the header's chip).
  // Finished subagents fold under each session while `hide_done_subagents`
  // is on (the default), as on the board and in the terminal app.
  const rows = $derived(
    repoRows(scopeModel(app.model), { archived: app.showArchived, foldFinished: app.launch.hide_done_subagents !== false }),
  );

  // The worktree groups' git state: read while the overview is up, and
  // again when a checkout comes or goes or a session there changes state.
  onMount(watchGit);
  const gitSig = $derived(gitSignal(app.model));
  $effect(() => {
    if (gitSig) untrack(() => refreshGit(shownIds()));
  });

  // Only a repo picked in the rail scrolls the overview (`revealRepo`, which
  // nothing else sets). Session clicks, strip jumps, Esc and back/forward
  // change `nav.current.repo` too, and must leave the scroll alone. A pick
  // made while the overview was not mounted is still pending (`done` lags
  // `seq`), so it scrolls once the overview mounts.
  $effect(() => {
    const { repo, seq, done } = app.reveal;
    if (seq === done || !repo || !scroller) return;
    app.reveal.done = seq;
    tick().then(() => {
      const section = scroller.querySelector(`[data-repo="${CSS.escape(repo)}"]`);
      const selected = section?.querySelector('[data-row][aria-selected="true"]');
      if (selected) selected.scrollIntoView({ block: 'nearest' });
      else section?.scrollIntoView({ block: 'start' });
    });
  });

  /** j/k and the arrows walk the session rows; Enter opens (on the row). */
  function onKey(e: KeyboardEvent) {
    if (!active || e.ctrlKey || e.altKey || e.metaKey) return;
    const t = e.target as HTMLElement;
    if (t.closest('input, textarea, select, .xterm, .monaco-editor')) return;
    const down = e.key === 'ArrowDown' || e.key === 'j';
    const up = e.key === 'ArrowUp' || e.key === 'k';
    if (!down && !up) return;
    const all = [...scroller.querySelectorAll<HTMLElement>('[data-row]')];
    if (!all.length) return;
    e.preventDefault();
    const i = all.indexOf(document.activeElement as HTMLElement);
    const next = i < 0 ? all[0] : all[Math.max(0, Math.min(all.length - 1, i + (down ? 1 : -1)))];
    next.focus();
    next.scrollIntoView({ block: 'nearest' });
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="flex h-full min-h-0 flex-col">
<SubagentBar />
<div class="@container min-h-0 flex-1 overflow-y-auto pb-8" bind:this={scroller}>
  {#if !rows.length}
    <Empty />
  {:else}
    {#each rows as row (row.project.id)}
      <RepoRow {row} />
    {/each}
  {/if}
</div>
</div>
