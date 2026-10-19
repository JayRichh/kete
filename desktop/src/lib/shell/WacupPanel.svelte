<script lang="ts">
  // The sidebar's now-playing panel for WACUP: title, transport, the
  // clock; "WACUP not running" with a Launch button otherwise. It polls
  // once a second while it is on screen (mounted, window visible) and
  // draws nothing where WACUP control is not built (not Windows).
  import { onMount } from 'svelte';
  import { api } from '../api';
  import { toast } from '../store.svelte';
  import { POLL_MS, clockLine, idleLine, marquee, type WacupControl, type WacupState } from './wacup';

  let s = $state<WacupState | null>(null);
  let box = $state<HTMLElement>();
  let text = $state<HTMLElement>();
  let slide = $state<{ shift: number; seconds: number } | null>(null);

  async function poll() {
    try {
      s = await api.wacupState();
    } catch {
      s = null;
    }
  }

  onMount(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    const start = () => {
      if (timer || document.visibilityState === 'hidden') return;
      void poll();
      timer = setInterval(poll, POLL_MS);
    };
    const stop = () => {
      clearInterval(timer);
      timer = undefined;
    };
    const onVisible = () => (document.visibilityState === 'hidden' ? stop() : start());
    document.addEventListener('visibilitychange', onVisible);
    start();
    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisible);
    };
  });

  // A title wider than its line slides to show the end, and back.
  $effect(() => {
    void s?.title;
    if (!box || !text) {
      slide = null;
      return;
    }
    slide = marquee(text.scrollWidth, box.clientWidth);
  });

  async function press(what: WacupControl) {
    try {
      await api.wacupTransport(what);
      await poll();
    } catch (e) {
      toast(String(e));
    }
  }
  async function launch() {
    try {
      await api.wacupLaunch();
    } catch (e) {
      toast(String(e));
    }
  }

  const btn =
    'grid size-7 place-items-center border-0 bg-transparent p-0 text-muted hover:bg-surface hover:text-ink focus-visible:-outline-offset-2';
  const playing = $derived(s?.playback === 'playing');
</script>

{#if s?.supported}
  <section class="flex-none border-t border-r border-rule bg-bg px-3 py-2" aria-label="WACUP now playing" data-wacup>
    {#if !s.running}
      <div class="flex items-center gap-2">
        <span class="min-w-0 flex-1 truncate text-[13px] text-muted" data-wacup-state="idle">{idleLine(s)}</span>
        {#if s.can_launch}
          <button
            class="h-6 flex-none border border-rule bg-transparent px-2 text-[12px] text-muted hover:border-ink hover:text-ink"
            title="Start WACUP from its install folder"
            onclick={launch}>Launch</button
          >
        {/if}
      </div>
      {#if !s.can_launch}
        <p class="m-0 mt-0.5 text-[11px] text-faint">wacup.exe not found in Program Files or App Paths</p>
      {/if}
    {:else}
      <div bind:this={box} class="overflow-hidden whitespace-nowrap text-[13px] text-ink" title={s.title || s.player} data-wacup-title>
        <span
          bind:this={text}
          class="inline-block {slide ? 'wacup-slide' : ''}"
          style={slide ? `--shift: -${slide.shift}px; --secs: ${slide.seconds}s` : undefined}
          >{s.title || idleLine(s) || s.player}</span
        >
      </div>
      <div class="mt-1 flex items-center gap-0.5">
        <button class={btn} title="Previous track" aria-label="Previous track" onclick={() => press('previous')}>
          <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 3v10M13 3L5.5 8 13 13z" fill="currentColor" stroke="currentColor" stroke-width="1.2" /></svg>
        </button>
        <button class={btn} title={playing ? 'Pause' : 'Play'} aria-label={playing ? 'Pause' : 'Play'} data-wacup-play onclick={() => press('play_pause')}>
          {#if playing}
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 3v10M11.5 3v10" fill="none" stroke="currentColor" stroke-width="2.4" /></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 2.5v11l9-5.5z" fill="currentColor" /></svg>
          {/if}
        </button>
        <button class={btn} title="Next track" aria-label="Next track" onclick={() => press('next')}>
          <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M13 3v10M3 3l7.5 5L3 13z" fill="currentColor" stroke="currentColor" stroke-width="1.2" /></svg>
        </button>
        <span class="ml-auto font-mono text-[12px] text-muted" data-wacup-clock>{clockLine(s.position_ms, s.length_ms)}</span>
      </div>
    {/if}
  </section>
{/if}

<style>
  .wacup-slide {
    animation: wacup-marquee var(--secs) ease-in-out infinite alternate;
  }
  @keyframes wacup-marquee {
    0%,
    15% {
      transform: translateX(0);
    }
    85%,
    100% {
      transform: translateX(var(--shift));
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .wacup-slide {
      animation: none;
    }
  }
</style>
