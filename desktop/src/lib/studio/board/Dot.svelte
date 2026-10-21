<script lang="ts">
  // A session's status as the house square plus a glyph: never colour alone.
  // The word comes with it on the lead row, and rests in the title on a chip.
  import Status from '../../Status.svelte';
  import { toneOf } from '../../status';
  import type { Agent } from '../../api';
  import { TONE_GLYPH } from './board';

  let { agent, word = false }: { agent: Agent; word?: boolean } = $props();
  const tone = $derived(toneOf(agent));
</script>

<span class="inline-flex flex-none items-center gap-1">
  <Status {tone} {word} />
  <span class="font-mono text-[11px] leading-none text-muted" aria-hidden="true">{TONE_GLYPH[tone]}</span>
</span>
