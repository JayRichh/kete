<script lang="ts">
  // A session's subagents as a pane of the session view, under the chat:
  // a header with the counts, a list that scrolls, finished ones folded
  // into one line that opens. The splitter above it moves the chat/list
  // boundary (remembered), and Collapse chat gives the list the pane.
  import { app } from './store.svelte';
  import Status from './Status.svelte';
  import { subagentTone, age } from './status';
  import { cardSubagents, isEnded, movedAt, subagentRun, subagentWord, tally } from './subagents';
  import { foldingFinished } from './subagent-actions';

  let { agent, height, collapsed, onheight, ontoggle }: { agent: string; height: number; collapsed: boolean; onheight: (px: number, commit: boolean) => void; ontoggle: () => void } = $props();

  let openFinished = $state(false);
  const all = $derived(app.model.subagents.filter((s) => s.parent_agent === agent));
  const counts = $derived(tally(all));
  const subs = $derived(cardSubagents(app.model.subagents, agent, foldingFinished(), openFinished));

  function startDrag(e: PointerEvent) {
    e.preventDefault();
    const startY = e.clientY;
    const start = height;
    const move = (m: PointerEvent) => onheight(start + (startY - m.clientY), false);
    const up = (m: PointerEvent) => {
      onheight(start + (startY - m.clientY), true);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 80 : 24;
    if (e.key === 'ArrowUp') onheight(height + step, true);
    else if (e.key === 'ArrowDown') onheight(height - step, true);
    else return;
    e.preventDefault();
  }
</script>

{#if all.length}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="h-[7px] flex-none cursor-row-resize border-0 border-t border-rule-strong hover:bg-rule focus-visible:bg-rule {collapsed ? 'hidden' : ''}"
    role="separator"
    aria-orientation="horizontal"
    aria-label="Resize the chat and the subagent list (arrow keys)"
    tabindex="0"
    data-subagent-splitter
    onpointerdown={startDrag}
    onkeydown={key}
  ></div>
  <section
    class="flex min-h-0 flex-col border-0 {collapsed ? 'flex-1 border-t border-rule-strong' : 'flex-none'}"
    style={collapsed ? undefined : `height: ${height}px`}
    aria-label="Subagents"
    data-subagent-pane
  >
    <header class="flex h-8 flex-none items-center gap-3 border-b border-rule px-4 text-[12px] text-muted">
      <span class="font-mono" data-subagent-tally>{counts.total} subagents &middot; {counts.running} running &middot; {counts.done} done</span>
      <button
        class="ml-auto inline-flex h-6 items-center border border-rule bg-transparent px-2 text-[12px] text-ink hover:border-rule-strong"
        aria-pressed={collapsed}
        data-collapse-chat
        title={collapsed ? 'Show the chat again' : 'Give the whole pane to the subagent list'}
        onclick={ontoggle}>{collapsed ? 'Show chat' : 'Collapse chat'}</button
      >
    </header>
    <ul class="m-0 min-h-0 flex-1 list-none overflow-y-auto px-4 py-1" aria-label="Subagents of this session" data-subagent-list>
      {#each subs.shown as s (s.id)}
        <li class="flex min-w-0 items-center gap-2 py-0.5 text-[12px]" title="{[s.kind || 'subagent', subagentRun(s)].filter(Boolean).join(' \u00b7 ')}: {s.description || 'no task seen'}">
          <Status tone={subagentTone(s)} word={false} />
          <span class="min-w-0 truncate {isEnded(s) ? 'text-faint' : 'text-ink'}">{s.description || s.kind || 'subagent'}</span>
          <span class="flex-none font-mono text-[11px] text-faint">{subagentRun(s)}</span>
          <span class="ml-auto flex-none font-mono text-[11px] text-muted">{subagentWord(s)} {age(movedAt(s), app.now)}</span>
        </li>
      {/each}
      {#if subs.ended && (subs.folded || openFinished)}
        <li class="py-0.5">
          <button class="border-0 bg-transparent p-0 font-mono text-[11px] text-faint hover:text-ink" aria-expanded={openFinished} onclick={() => (openFinished = !openFinished)}
            >{subs.ended} finished {openFinished ? '(hide)' : '(show)'}</button
          >
        </li>
      {/if}
    </ul>
  </section>
{/if}
