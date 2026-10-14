<script lang="ts">
  // One thin square bar: a caption, the track with its fill (ember from
  // 80%, red from 95%), and the percent right-aligned in mono. Unknown is
  // an empty track and a dash.
  import { FILL, fillWidth, pct, tone } from './usage';

  let { label, percent, width = 'flex' }: { label?: string; percent: number | null; width?: 'flex' | number } = $props();
  const t = $derived(tone(percent));
</script>

<span class="flex min-w-0 items-center gap-1.5 {width === 'flex' ? 'w-full' : ''}">
  {#if label}<span class="w-[40px] flex-none truncate font-mono text-[10px] leading-none text-faint">{label}</span>{/if}
  <span
    class="relative h-[2px] bg-rule {width === 'flex' ? 'min-w-0 flex-1' : 'flex-none'}"
    style={width === 'flex' ? '' : `width: ${width}px`}
    aria-hidden="true"
  >
    <span class="absolute inset-y-0 left-0 {FILL[t]}" style="width: {fillWidth(percent)}%"></span>
  </span>
  <span
    class="w-[30px] flex-none text-right font-mono text-[11px] leading-none tabular-nums {t === 'hot'
      ? 'text-fail'
      : t === 'warn'
        ? 'text-ember-ink'
        : percent == null
          ? 'text-faint'
          : 'text-muted'}">{pct(percent)}</span
  >
</span>
