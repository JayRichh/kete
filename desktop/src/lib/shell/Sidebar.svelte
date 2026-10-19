<script lang="ts">
  // The sidebar's outer container: its width, the drag handle on its right
  // edge, the fold button, and the folded icon rail with its hover peek.
  // What is inside (the repos, accounts, settings) is Rail.svelte's.
  import Rail from './Rail.svelte';
  import SidebarRail from './SidebarRail.svelte';
  import { PEEK_DELAY_MS, RAIL_WIDTH, SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN, dragWidth, keyWidth } from './sidebar';
  import { setSidebarCollapsed, setSidebarWidth, sidebar } from './rail-state.svelte';
  import { withKey } from '../shortcuts';

  /** A narrow window keeps its fixed icon rail: no handle, no fold. */
  let { narrow = false }: { narrow?: boolean } = $props();

  let box: HTMLDivElement;
  let dragging = $state(false);

  const folded = $derived(!narrow && sidebar.collapsed);
  const layoutWidth = $derived(narrow ? 52 : folded ? RAIL_WIDTH : sidebar.width);

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const left = box.getBoundingClientRect().left;
    dragging = true;
    const move = (ev: PointerEvent) => setSidebarWidth(dragWidth(ev.clientX, left));
    const up = () => {
      dragging = false;
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
      handle.removeEventListener('pointercancel', up);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
    handle.addEventListener('pointercancel', up);
  }
  function resizeKey(e: KeyboardEvent) {
    const w = keyWidth(e.key, sidebar.width, e.shiftKey);
    if (w === undefined) return;
    e.preventDefault();
    setSidebarWidth(w);
  }

  // The folded rail peeks the full sidebar over the content once the
  // pointer has rested on it; leaving the rail or the peek closes it.
  let peekTimer: ReturnType<typeof setTimeout> | undefined;
  function enter() {
    if (!folded || sidebar.peeking) return;
    clearTimeout(peekTimer);
    peekTimer = setTimeout(() => (sidebar.peeking = true), PEEK_DELAY_MS);
  }
  function leave() {
    clearTimeout(peekTimer);
    sidebar.peeking = false;
  }
  function peekKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && sidebar.peeking) {
      e.stopPropagation();
      sidebar.peeking = false;
    }
  }
  $effect(() => {
    if (!folded) leave();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={box}
  class="group/side relative h-full flex-none {dragging ? 'select-none' : ''}"
  style="width: {layoutWidth}px"
  data-sidebar
  data-sidebar-state={narrow ? 'narrow' : folded ? 'folded' : 'open'}
  onpointerenter={enter}
  onpointerleave={leave}
  onkeydown={peekKey}
>
  {#if narrow}
    <Rail compact />
  {:else if folded}
    <SidebarRail />
    {#if sidebar.peeking}
      <!-- Over the content, not in the layout: the page does not move. -->
      <div class="absolute inset-y-0 left-0 z-40 flex shadow-[4px_0_16px_rgba(0,0,0,0.18)]" style="width: {sidebar.width}px" data-sidebar-peek>
        <div class="flex min-h-0 flex-1 flex-col bg-bg [&>aside]:min-h-0 [&>aside]:flex-1"><Rail /></div>
      </div>
    {/if}
  {:else}
    <div class="flex h-full min-h-0 flex-col [&>aside]:min-h-0 [&>aside]:flex-1"><Rail /></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute inset-y-0 -right-[3px] z-20 w-[7px] cursor-col-resize hover:bg-rule-strong focus-visible:bg-rule-strong {dragging ? 'bg-rule-strong' : ''}"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the sidebar (arrow keys; double-click for the default width)"
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={sidebar.width}
      tabindex="0"
      data-sidebar-resizer
      onpointerdown={startDrag}
      onkeydown={resizeKey}
      ondblclick={() => setSidebarWidth(SIDEBAR_DEFAULT)}
    ></div>
    <button
      class="absolute top-12 -right-3 z-30 grid h-8 w-6 place-items-center border border-rule-strong bg-bg p-0 text-muted opacity-0 group-focus-within/side:opacity-100 group-hover/side:opacity-100 hover:text-ink focus-visible:opacity-100"
      title={withKey('Collapse the sidebar to its icon rail', 'toggle_sidebar')}
      aria-label="Collapse the sidebar"
      data-sidebar-collapse
      onclick={() => setSidebarCollapsed(true)}
    >
      <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3L5 8l5 5" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
    </button>
  {/if}
</div>
