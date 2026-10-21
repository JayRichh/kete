<script lang="ts">
  // The four autonomy levels as one row of segments. Raising to A2 or A3
  // asks first, in a row under the segments (never a browser dialog).
  import type { Autonomy } from '../types';
  import { ALLOWS, LEVELS, confirmText, needsConfirm } from './queue.svelte';

  let props: { repoName: string; level: Autonomy; onchange: (to: Autonomy) => void } = $props();

  let pending = $state<Autonomy | null>(null);
  let group: HTMLElement | undefined = $state();
  let raise: HTMLButtonElement | undefined = $state();

  $effect(() => {
    if (pending) raise?.focus();
  });

  function pick(to: Autonomy) {
    if (to === props.level) return;
    if (needsConfirm(props.level, to)) pending = to;
    else props.onchange(to);
  }

  function confirm() {
    const to = pending;
    pending = null;
    if (to) props.onchange(to);
  }

  function cancel() {
    pending = null;
    group?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
  }

  /** Arrows move focus; Enter or Space picks, so a raise still asks. */
  function arrow(e: KeyboardEvent) {
    const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
    if (!step || !group) return;
    e.preventDefault();
    const items = [...group.querySelectorAll<HTMLElement>('[role="radio"]')];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + step + items.length) % items.length]?.focus();
  }
</script>

<div
  class="flex min-w-0 flex-col gap-2"
  data-autonomy-control
  tabindex="-1"
  role="group"
  aria-label="Autonomy of {props.repoName}"
>
  <div bind:this={group} class="flex flex-wrap border border-rule" role="radiogroup" aria-label="Autonomy" tabindex="-1" onkeydown={arrow}>
    {#each LEVELS as l, i (l.level)}
      {@const on = l.level === props.level}
      <button
        role="radio"
        aria-checked={on}
        tabindex={on ? 0 : -1}
        class="h-7 border-0 px-3 text-[13px] {i ? 'border-l border-rule' : ''} {on ? 'bg-ink text-bg' : 'bg-transparent text-muted hover:text-ink'}"
        title="{l.level} {l.name}: {ALLOWS[l.level]}. Arrow keys move, Enter picks."
        onclick={() => pick(l.level)}>{l.level} {l.name}</button
      >
    {/each}
  </div>
  {#if pending}
    <div
      class="flex flex-wrap items-center gap-2 border border-rule px-3 py-2 text-[13px] text-ink"
      role="alertdialog"
      aria-label="Confirm autonomy"
      tabindex="-1"
      onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), cancel())}
    >
      <span class="min-w-0">{confirmText(props.repoName, pending)}</span>
      <button
        bind:this={raise}
        class="h-7 border border-rule bg-ink px-3 text-[13px] text-bg"
        title="Raise it (Enter)"
        onclick={confirm}>Raise</button
      >
      <button
        class="h-7 border border-rule bg-transparent px-3 text-[13px] text-ink hover:bg-surface"
        title="Keep the current level (Escape)"
        onclick={cancel}>Cancel</button
      >
    </div>
  {/if}
</div>
