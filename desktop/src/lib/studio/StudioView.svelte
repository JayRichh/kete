<script lang="ts">
  // STUDIO: the repos that keep a studio (a manifest, or a machine entry),
  // one at a time. A repo picker, then that repo's queue header and board.
  import { app } from '../store.svelte';
  import { nav } from '../shell/history.svelte';
  import QueueHeader from './queue/QueueHeader.svelte';
  import Board from './board/Board.svelte';

  const repos = $derived(app.studioRepos);
  /** The repo the place names, else the first with studio mode. */
  const picked = $derived(repos.find((r) => r.repo === nav.current.repo)?.repo ?? repos[0]?.repo ?? null);

  function pick(repo: string) {
    nav.replace({ view: 'studio', repo });
  }
</script>

<section class="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pt-6 pb-6" data-studio-view>
  {#if repos.length === 0}
    <h1 class="m-0 text-[28px] font-semibold tracking-tight text-ink">Studio</h1>
    <p class="m-0 max-w-[60ch] text-[14px] text-muted">
      No repo has studio mode yet. Run <code class="font-mono text-[13px] text-ink">kete studio enable &lt;repo&gt;</code> to turn it on for one.
    </p>
  {:else}
    <header class="flex min-w-0 flex-wrap items-center gap-3">
      <h1 class="m-0 text-[28px] font-semibold tracking-tight text-ink">Studio</h1>
      <div class="flex min-w-0 flex-wrap border border-rule" role="radiogroup" aria-label="Repo">
        {#each repos as r, i (r.repo)}
          {@const on = r.repo === picked}
          <button
            role="radio"
            aria-checked={on}
            class="h-7 max-w-[220px] truncate border-0 px-3 text-[13px] {i ? 'border-l border-rule' : ''} {on ? 'bg-ink text-bg' : 'bg-transparent text-muted hover:text-ink'}"
            title="Show the studio of {r.name}"
            onclick={() => pick(r.repo)}>{r.name}</button
          >
        {/each}
      </div>
    </header>
    <QueueHeader repo={picked} />
    <Board repo={picked} />
  {/if}
</section>
