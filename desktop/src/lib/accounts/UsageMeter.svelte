<script lang="ts">
  // The plan's 5-hour window in one line: `5h 87% - resets 9:50 pm`, the
  // reset in the local zone from the shared formatter, coloured at 70, 85
  // and 95 percent. Hover: when the window began, the countdown, and the
  // ISO time.
  import type { UsageWindow } from '../api';
  import { labelFor } from './labels.svelte';
  import { pct } from './usage';

  let {
    window: w,
    caption = '5h',
    short = false,
  }: { window: UsageWindow | null | undefined; caption?: string; short?: boolean } = $props();
  const label = $derived(labelFor(w));
  const TONE = ['text-muted', 'text-ink', 'text-ember-ink', 'text-fail'];
  const text = $derived(label ? (short ? label.compact.replace('resets ', '') : label.compact) : w ? pct(w.percent) : '');
</script>

{#if w}
  <span
    class="inline-flex min-w-0 items-baseline gap-1.5 font-mono text-[12px] tabular-nums {TONE[label?.step ?? 0]} {label && label.step >= 3 ? 'font-semibold' : ''}"
    data-usage-meter
    data-step={label?.step ?? 0}
    title={label?.hover || undefined}
  >
    {#if caption}<span class="text-faint">{caption}</span>{/if}
    <span class="truncate">{text}</span>
  </span>
{/if}
