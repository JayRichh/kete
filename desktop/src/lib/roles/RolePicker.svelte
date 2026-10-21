<script lang="ts">
  // The role a launch starts with, from the repo's committed
  // `.kete/roles.toml`. Nothing while the repo has no roles (or they are
  // still loading). `bar` is the quick prompt's option row; `field` is a
  // form field (Start orchestrator).
  import { untrack } from 'svelte';
  import { loadRoles, roleOption, roleTitle, roles } from './roles.svelte';

  let { repo, value = $bindable(null), bar = false }: { repo: string; value: string | null; bar?: boolean } = $props();

  const held = $derived(roles[repo]);
  const rows = $derived(held?.rows ?? []);
  const current = $derived(rows.find((r) => r.id === value));

  // Fresh on each opening and each repo.
  $effect(() => {
    const r = repo;
    untrack(() => loadRoles(r));
  });
  // A role belongs to its repo: drop one the repo does not have.
  $effect(() => {
    if (value && !rows.some((r) => r.id === value)) value = null;
  });

  const tip = $derived(current ? roleTitle(current) : 'Start the session as one of the roles this repo commits');
  const barSelect =
    'h-full w-full min-w-0 appearance-none border-0 bg-transparent pr-7 pl-3 text-[13px] text-ink outline-none hover:bg-surface focus-visible:-outline-offset-2';
  const fieldSelect = 'h-8 w-full border border-rule bg-bg px-2 text-[13px] text-ink outline-none hover:border-rule-strong focus:border-rule-strong';
</script>

{#snippet options()}
  <option value="">No role</option>
  {#each rows as r (r.id)}<option value={r.id}>{roleOption(r)}</option>{/each}
{/snippet}

{#if rows.length}
  {#if bar}
    <span class="relative flex max-w-72 min-w-0 items-center border-r border-rule">
      <span class="pl-3 font-mono text-[11px] text-muted">role</span>
      <select class={barSelect} value={value ?? ''} onchange={(e) => (value = e.currentTarget.value || null)} aria-label="Role" title={tip}>
        {@render options()}
      </select>
      <svg class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
        ><path d="M1.5 3.5L5 7l3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.4" /></svg
      >
    </span>
  {:else}
    <label class="col-span-2 flex flex-col gap-1">
      <span class="text-[12px] font-medium text-muted">Role</span>
      <select class={fieldSelect} value={value ?? ''} onchange={(e) => (value = e.currentTarget.value || null)} aria-label="Role" title={tip}>
        {@render options()}
      </select>
    </label>
  {/if}
{:else if held?.error}
  <span class="px-3 font-mono text-[11px] text-muted {bar ? 'flex items-center border-r border-rule' : 'col-span-2'}" title={held.error}>roles file invalid</span>
{/if}
