<script lang="ts">
  // A session's role: its card's label, on session rows, board cards and
  // the session header. Nothing for a session without one. Unattended (no
  // one answers its prompts) is said in a word, not a colour.
  import { app } from '../store.svelte';
  import { roleTitle } from './roles.svelte';

  let { agent }: { agent: string } = $props();
  const role = $derived(app.roles[agent]);
  const unattended = $derived(role?.permission === 'unattended');
</script>

{#if role}
  <span
    class="inline-flex h-5 max-w-40 flex-none items-center gap-1 border px-1.5 font-mono text-[11px] leading-none whitespace-nowrap {unattended
      ? 'border-rule-strong text-ink'
      : 'border-rule text-muted'}"
    title={roleTitle(role)}
  >
    <span class="truncate">{role.label}</span>
    {#if unattended}<span class="font-semibold">unattended</span>{/if}
    <span class="sr-only">{roleTitle(role)}</span>
  </span>
{/if}
