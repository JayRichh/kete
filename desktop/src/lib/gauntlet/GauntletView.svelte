<script lang="ts">
  // GAUNTLET: the sign-off verdicts a repo's studio kept, read only. A tab
  // per kind, a word and an age filter, the verdicts newest first, and the
  // selected one in full: its note, tags, files and evidence. Nothing here
  // writes a verdict; Open in Studio hands the browser the kind's own page.
  import { tick, untrack } from 'svelte';
  import { api } from '../api';
  import { app, openGauntlet, toast } from '../store.svelte';
  import { nav } from '../shell/history.svelte';
  import { ownMenu } from '../menus/router';
  import { editorsApi } from '../editors/api';
  import { underRoot } from '../editors/verbs';
  import { showWorktreeFile } from '../files/files.svelte';
  import { TOO_BIG } from '../files/menus';
  import { VIEWER_MAX_BYTES } from '../files/tree';
  import { parseGauntletItem } from '../studio/types';
  import {
    AGES,
    atLabel,
    filterVerdicts,
    firstLine,
    NO_CHECKOUT,
    openEvidenceWhy,
    toneClass,
    toneGlyph,
    toneLabel,
    verdictWords,
    wordLabel,
    type Age,
    type EvidenceEntry,
    type GauntletList,
    type VerdictRow,
  } from './gauntlet';
  import { verdictMenu } from './menus';

  // `item` is `<kind>/<id>` of the verdict to open, when one is named.
  let props: { item?: string } = $props();

  const repo = $derived(nav.current.repo ?? app.studioRepos[0]?.repo);
  const repoName = $derived(app.studioRepos.find((r) => r.repo === repo)?.name ?? repo ?? '');

  let data = $state<GauntletList | null>(null);
  let dataRepo = $state<string | undefined>(undefined);
  let loading = $state(false);
  let failure = $state<string | null>(null);
  let now = $state(Date.now());

  let kind = $state('');
  let word = $state<string | null>(null);
  let age = $state<Age>('all');
  let selId = $state<string | null>(null);
  let evidence = $state<Record<string, EvidenceEntry[] | undefined>>({});
  let detailEl = $state<HTMLElement | null>(null);
  // The (repo, item) last applied to the list: plain, so it is no dependency.
  let applied = '';

  let loadSeq = 0;
  async function load(r: string | undefined) {
    const n = ++loadSeq;
    if (!r) {
      loading = false;
      return;
    }
    loading = true;
    failure = null;
    try {
      const d = (await api.gauntletList(r)) as GauntletList;
      if (n !== loadSeq) return;
      data = d;
      dataRepo = r;
      now = Date.now();
    } catch (e) {
      if (n !== loadSeq) return;
      data = null;
      failure = String(e);
    } finally {
      if (n === loadSeq) loading = false;
    }
  }

  // A repo (or its change) starts the view over.
  $effect(() => {
    const r = repo;
    untrack(() => {
      data = null;
      dataRepo = undefined;
      kind = '';
      word = null;
      selId = null;
      evidence = {};
      applied = '';
      void load(r);
    });
  });

  // The place's verdict, once the list is here: its tab and row. Applied once
  // per (repo, item), so a Refresh or a click is not undone.
  $effect(() => {
    const d = data;
    const key = `${repo}|${props.item ?? ''}`;
    if (!d || dataRepo !== repo || key === applied) return;
    applied = key;
    const want = parseGauntletItem(props.item);
    const k = want && d.kinds.find((x) => x.kind === want.kind);
    if (!want || !k) return;
    kind = k.kind;
    word = null;
    age = 'all';
    selId = k.verdicts.some((v) => v.id === want.id) ? want.id : null;
  });

  const kinds = $derived(data?.kinds ?? []);
  const cur = $derived(kinds.find((k) => k.kind === kind) ?? kinds[0] ?? null);
  const words = $derived(cur ? verdictWords(cur.verdicts) : []);
  const rows = $derived(cur ? filterVerdicts(cur.verdicts, { word, age }, now) : []);
  const sel = $derived(rows.find((v) => v.id === selId) ?? null);
  const worktree = $derived(data?.worktree ?? null);

  // The selected verdict's evidence listings, read as it is selected.
  $effect(() => {
    const v = sel;
    const r = repo;
    untrack(() => (evidence = {}));
    if (!v || !r) return;
    for (const ev of v.evidence) {
      api
        .gauntletEvidence(r, ev)
        .then((l) => {
          if (sel === v) evidence = { ...evidence, [ev]: l as EvidenceEntry[] };
        })
        .catch(() => {
          if (sel === v) evidence = { ...evidence, [ev]: [] };
        });
    }
  });

  function selectKind(k: string) {
    if (k === cur?.kind) return;
    kind = k;
    word = null;
    selId = null;
  }

  function openStudio(v: VerdictRow) {
    if (repo) api.gauntletOpenStudio(repo, v.kind, v.id).catch((e) => toast(String(e)));
  }

  async function showEvidence(v: VerdictRow) {
    selId = v.id;
    await tick();
    detailEl?.querySelector('[data-gauntlet-evidence-section]')?.scrollIntoView({ block: 'nearest' });
  }

  /** The row's own menu, by right-click (the router's own-menu hook). */
  const menuOf = (v: VerdictRow) => () => {
    selId = v.id;
    return { items: verdictMenu(v, { studio: !!cur?.studio, openStudio: () => openStudio(v), showEvidence: () => void showEvidence(v) }), label: `verdict ${v.id}` };
  };

  function rowKey(e: KeyboardEvent, i: number) {
    if (e.target !== e.currentTarget || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      selId = rows[i].id;
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const j = Math.min(rows.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1)));
      selId = rows[j].id;
      ((e.currentTarget as HTMLElement).parentElement?.children[j] as HTMLElement | undefined)?.focus();
    }
  }

  const openKete = (rel: string) => data && worktree && showWorktreeFile(data.root, worktree, rel, underRoot(data.root, rel));
  const reveal = (rel: string) => void editorsApi.reveal(worktree, rel).catch((e) => toast(String(e)));

  const btn = 'h-6 border border-rule bg-transparent px-2 text-[12px] text-ink hover:border-rule-strong disabled:text-faint disabled:hover:border-rule';
  const head = 'font-mono text-[11px] text-faint';
  const chip = (on: boolean) =>
    `h-6 border px-2 text-[12px] ${on ? 'border-rule-strong bg-ink text-bg' : 'border-rule bg-transparent text-muted hover:border-rule-strong hover:text-ink'}`;
</script>

<!-- A file's two verbs. `bytes` null: size not known, so never "too big". -->
{#snippet files(rel: string, bytes: number | null, isDir: boolean)}
  {@const why = openEvidenceWhy({ name: rel, path: rel, is_dir: isDir, bytes: bytes ?? 0 }, worktree, VIEWER_MAX_BYTES, TOO_BIG)}
  <span class="flex flex-none flex-wrap gap-1.5">
    <button class={btn} disabled={!!why} onclick={() => openKete(rel)} title={why ?? `Open ${rel} in Kete`} aria-label="Open {rel} in Kete">Open in Kete</button>
    <button class={btn} disabled={!worktree} onclick={() => reveal(rel)} title={worktree ? `Show ${rel} in Explorer` : NO_CHECKOUT} aria-label="Show {rel} in Explorer"
      >Show in Explorer</button
    >
  </span>
{/snippet}

<section class="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto" aria-label="Gauntlet" data-gauntlet-view>
  <div class="mx-auto w-full max-w-[1180px] px-6 pt-5 pb-10 max-sm:px-3">
    <div class="flex flex-wrap items-baseline gap-x-3 gap-y-2 border-b border-rule-strong pb-2">
      <h1 class="m-0 text-[22px] font-semibold tracking-tight text-ink">Gauntlet</h1>
      {#if app.studioRepos.length > 1}
        <select
          class="h-7 border border-rule bg-bg px-1.5 font-mono text-[12px] text-ink outline-none hover:border-rule-strong"
          value={repo}
          onchange={(e) => openGauntlet((e.currentTarget as HTMLSelectElement).value)}
          aria-label="Repo"
          title="Which repo's verdicts to read"
        >
          {#each app.studioRepos as r (r.repo)}<option value={r.repo}>{r.name}</option>{/each}
        </select>
      {:else if repoName}
        <span class="font-mono text-[12px] text-muted">{repoName}</span>
      {/if}
      <span class="flex-1"></span>
      <button class={btn} disabled={!repo || loading} onclick={() => load(repo)} title="Read the verdicts again">{loading ? 'Reading\u2026' : 'Refresh'}</button>
    </div>

    {#if !repo}
      <p class="mt-6 text-[13px] text-muted" data-gauntlet-empty>No repo has studio mode yet: add .kete/studio.toml to one.</p>
    {:else if failure}
      <p class="mt-6 text-[13px] text-fail" data-gauntlet-empty>Could not read the verdicts: {failure}</p>
    {:else if !data}
      <p class="mt-6 text-[13px] text-muted" role="status">Reading the verdicts&hellip;</p>
    {:else if data.error}
      <p class="mt-6 text-[13px] text-muted" data-gauntlet-empty>{data.error}</p>
    {:else if !cur}
      <p class="mt-6 max-w-[60ch] text-[13px] text-muted" data-gauntlet-empty>No verdicts yet in {repoName}'s signoffs.</p>
    {:else}
      <!-- Kinds. -->
      <div class="mt-4 flex flex-wrap gap-x-1 border-b border-rule" role="tablist" aria-label="Kinds">
        {#each kinds as k (k.kind)}
          {@const on = k.kind === cur.kind}
          <button
            role="tab"
            aria-selected={on}
            data-gauntlet-kind={k.kind}
            class="-mb-px h-8 border-0 border-b-2 bg-transparent px-3 text-[13px] {on ? 'border-ink font-medium text-ink' : 'border-transparent text-muted hover:text-ink'}"
            onclick={() => selectKind(k.kind)}
            title={k.department ? `${k.kind}, ${k.department}` : k.kind}
            >{k.kind} <span class="font-mono text-[11px] text-faint">{k.verdicts.length}</span></button
          >
        {/each}
      </div>

      <!-- Filters. -->
      <div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
        <div class="flex flex-wrap items-center gap-1" role="group" aria-label="Verdict word">
          <span class={head}>word</span>
          <button class={chip(word === null)} aria-pressed={word === null} data-gauntlet-word="" onclick={() => (word = null)} title="Every verdict word">All</button>
          {#each words as w (w)}
            <button
              class={chip(word?.toLowerCase() === w.toLowerCase())}
              aria-pressed={word?.toLowerCase() === w.toLowerCase()}
              data-gauntlet-word={wordLabel(w)}
              onclick={() => (word = w)}
              title={w === '' ? 'Verdicts with no word' : `Only verdicts that say ${w}`}>{wordLabel(w)}</button
            >
          {/each}
        </div>
        <div class="flex items-center gap-1" role="group" aria-label="Age">
          <span class={head}>age</span>
          {#each AGES as a (a.id)}
            <button
              class={chip(age === a.id)}
              aria-pressed={age === a.id}
              data-gauntlet-age={a.id}
              onclick={() => ((age = a.id), (now = Date.now()))}
              title={a.id === 'all' ? 'Every verdict' : `Only verdicts from the last ${a.label}`}>{a.label}</button
            >
          {/each}
        </div>
      </div>

      <div class="mt-4 grid items-start gap-x-6 gap-y-5 lg:grid-cols-[minmax(0,1.2fr)_minmax(320px,1fr)]">
        <!-- Verdicts, newest first. -->
        <div class="min-w-0">
          {#if !rows.length}
            <p class="m-0 border-t border-rule py-6 text-[13px] text-muted" data-gauntlet-empty>
              {cur.verdicts.length ? 'No verdicts match these filters.' : `No ${cur.kind} verdicts yet.`}
            </p>
          {:else}
            <ul class="m-0 list-none border-t border-rule p-0" role="listbox" aria-label="{cur.kind} verdicts">
              {#each rows as v, i (v.id)}
                {@const on = v.id === sel?.id}
                <li
                  role="option"
                  aria-selected={on}
                  tabindex={on || (!sel && i === 0) ? 0 : -1}
                  data-gauntlet-row="{v.kind}/{v.id}"
                  class="cursor-default border-b border-rule px-2 py-2 outline-none hover:bg-surface focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-rule-strong {on ? 'bg-raised' : ''}"
                  onclick={() => (selId = v.id)}
                  onkeydown={(e) => rowKey(e, i)}
                  use:ownMenu={menuOf(v)}
                >
                  <div class="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
                    <span class="min-w-0 truncate font-mono text-[13px] text-ink" title={v.id}>{v.id}</span>
                    <span class="flex items-baseline gap-1.5 text-[12px] {toneClass(v.tone)}" title={toneLabel(v.tone)}>
                      <span aria-hidden="true">{toneGlyph(v.tone)}</span>
                      <span>{wordLabel(v.verdict)}</span>
                      <span class="sr-only">, {toneLabel(v.tone)}</span>
                    </span>
                    <span class="flex-1"></span>
                    <span class="font-mono text-[11px] text-muted">{[v.by, atLabel(v)].filter(Boolean).join(' \u00b7 ')}</span>
                  </div>
                  {#if firstLine(v.note)}<div class="mt-0.5 truncate text-[12px] text-muted" title={firstLine(v.note)}>{firstLine(v.note)}</div>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <!-- The selected verdict in full. -->
        <div class="min-w-0 border border-rule p-3" data-gauntlet-detail bind:this={detailEl}>
          {#if sel}
            <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <h2 class="m-0 min-w-0 truncate font-mono text-[14px] font-medium text-ink" title={sel.id}>{sel.id}</h2>
              <span class="flex items-baseline gap-1.5 text-[13px] {toneClass(sel.tone)}">
                <span aria-hidden="true">{toneGlyph(sel.tone)}</span>
                <span>{wordLabel(sel.verdict)}</span>
                <span class="text-[11px] text-muted">{toneLabel(sel.tone)}</span>
              </span>
              <span class="flex-1"></span>
              {#if cur.studio}
                <button class={btn} data-gauntlet-studio onclick={() => openStudio(sel)} title="Open this {cur.kind} in the studio, in your browser">Open in Studio</button>
              {/if}
            </div>
            <p class="m-0 mt-1 font-mono text-[11px] text-muted">
              {[sel.by, atLabel(sel), cur.department].filter(Boolean).join(' \u00b7 ') || 'no author or date'}
            </p>

            <div class="mt-3 {head}">note</div>
            {#if sel.note}
              <p class="m-0 mt-1 text-[13px] break-words whitespace-pre-wrap text-ink">{sel.note}</p>
            {:else}
              <p class="m-0 mt-1 text-[13px] text-muted">No note.</p>
            {/if}

            {#if sel.tags.length}
              <div class="mt-3 flex flex-wrap gap-1" aria-label="Tags">
                {#each sel.tags as t (t)}<span class="border border-rule px-1.5 font-mono text-[11px] text-muted">{t}</span>{/each}
              </div>
            {/if}

            <div class="mt-3 {head}">files</div>
            <ul class="m-0 mt-1 list-none p-0">
              <li class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t border-rule py-1.5">
                <span class="min-w-0 font-mono text-[12px] break-all text-ink"><span class="text-faint">verdict</span>{' '}{sel.rel}</span>
                {@render files(sel.rel, null, false)}
              </li>
              {#if sel.reconciled}
                <li class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t border-rule py-1.5">
                  <span class="min-w-0 font-mono text-[12px] break-all text-ink"><span class="text-faint">reconciled</span>{' '}{sel.reconciled_rel ?? sel.reconciled}</span>
                  {#if sel.reconciled_rel}{@render files(sel.reconciled_rel, null, false)}{/if}
                </li>
              {/if}
            </ul>

            {#if sel.evidence.length}
              <div class="mt-3 {head}" data-gauntlet-evidence-section>evidence</div>
              {#each sel.evidence as ev (ev)}
                {@const list = evidence[ev]}
                <div class="mt-1 border-t border-rule pt-1.5">
                  <div class="font-mono text-[12px] break-all text-ink">{ev}</div>
                  {#if list === undefined}
                    <p class="m-0 py-1 text-[12px] text-muted">Reading&hellip;</p>
                  {:else if !list.length}
                    <p class="m-0 py-1 text-[12px] text-muted">No files found for this evidence.</p>
                  {:else}
                    <ul class="m-0 list-none p-0">
                      {#each list as e (e.path)}
                        <li class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 py-1" data-gauntlet-evidence={e.path}>
                          <span class="min-w-0 font-mono text-[12px] break-all text-muted" title={e.path}>{e.name}{e.is_dir ? '/' : ''}</span>
                          {@render files(e.path, e.bytes, e.is_dir)}
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/each}
            {/if}
          {:else}
            <p class="m-0 text-[13px] text-muted">Select a verdict to read its note, files and evidence.</p>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</section>
