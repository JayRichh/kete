<script lang="ts">
  import { app, goHome, sessionFromKey } from '../store.svelte';
  import { nav } from './history.svelte';
  import { placeOf } from '../status';
  import { parseItem } from '../ado/prs-api';
  import { parseDiffItem } from '../diff-api';
  import { parseGauntletItem } from '../studio/types';
  import { withKey } from '../shortcuts';
  import { ownMenu, type Opened } from '../menus/router';
  import { menuFor, targetLabel, type Target } from '../actions';
  import { viewMenu } from '../menus/targets';

  /** `menu`: its right-click (\u00a75.18): a repo's, a checkout's (with its
   *  sessions to open), a session's menu; a view's the views to go to. */
  type Crumb = { label: string; mono?: boolean; go?: () => void; menu?: () => Opened | null };

  const of = (t: Target, jump = false) => () => ({ items: menuFor(t, { jump }), label: targetLabel(t) });
  const views = () => ({ items: viewMenu(), label: 'View' });

  const VIEW_LABEL: Record<string, string> = {
    tasks: 'Tasks',
    branches: 'Branches',
    prs: 'Pull requests',
    settings: 'Settings',
    orchestrator: 'Orchestrator',
    accounts: 'Accounts',
    whakaoho: 'Whakaoho',
    worktrees: 'Worktrees',
    studio: 'Studio',
    gauntlet: 'Gauntlet',
  };

  const crumbs = $derived.by((): Crumb[] => {
    const p = nav.current;
    const m = app.model;
    const out: Crumb[] = [{ label: app.home === 'board' ? 'Board' : 'Overview', go: () => goHome(), menu: views }];
    const project = p.repo ? m.projects.find((x) => x.id === p.repo) : undefined;
    if (project) out.push({ label: project.name, go: () => goHome(project.id), menu: of({ kind: 'project', id: project.id }) });
    if (VIEW_LABEL[p.view]) out.push({ label: VIEW_LABEL[p.view], go: () => nav.go({ view: p.view, repo: p.repo }), menu: views });
    if (p.view === 'branches' && p.item) {
      // `repo / Branches / jari / gst-rounding`: the person, then the rest.
      const cut = p.item.indexOf('/');
      if (cut > 0) out.push({ label: p.item.slice(0, cut), mono: true }, { label: p.item.slice(cut + 1), mono: true });
      else out.push({ label: p.item, mono: true });
    }
    if (p.view === 'prs' && p.item) {
      // `repo / Pull requests / PR 3417`, or `/ New / <branch>` in the composer.
      const it = parseItem(p.item);
      if (it.kind === 'pr') out.push({ label: `PR ${it.id}`, mono: true });
      else if (it.kind === 'compose') out.push({ label: 'New' }, { label: it.branch, mono: true });
    }
    if (p.view === 'gauntlet' && p.item) {
      // `Gauntlet / <kind> / <id>`: the verdict that is open.
      const g = parseGauntletItem(p.item);
      if (g) out.push({ label: g.kind }, { label: g.id, mono: true });
    }
    if (p.view === 'session') {
      const s = sessionFromKey(p.item);
      const name =
        s && 'Agent' in s
          ? m.agents.find((a) => a.id === s.Agent)?.name
          : s
            ? m.terminals.find((t) => t.id === s.Terminal)?.name
            : undefined;
      if (name && s) out.push({ label: name, menu: of('Agent' in s ? { kind: 'agent', id: s.Agent } : { kind: 'terminal', id: s.Terminal }) });
    } else if (p.view === 'diff') {
      out.push({ label: 'Diff', menu: views });
      const d = parseDiffItem(p.item);
      // A range reads commits, so it names its two ends, not the worktree
      // it happens to be read in.
      const ref = (r: string) => r.replace(/^refs\/(heads|remotes)\//, '');
      if (d.range) out.push({ label: `${ref(d.range.base)}...${ref(d.range.tip)}`, mono: true });
      else if (d.worktree) out.push({ label: placeOf(m, d.worktree).branch, mono: true, menu: of({ kind: 'worktree', id: d.worktree }, true) });
    } else if (p.view === 'files') {
      out.push({ label: 'Files', menu: views });
      if (p.item && m.worktrees.some((w) => w.id === p.item)) out.push({ label: placeOf(m, p.item).branch, mono: true, menu: of({ kind: 'worktree', id: p.item }, true) });
    }
    return out;
  });

  const btn =
    'grid size-7 flex-none place-items-center border-0 bg-transparent text-ink enabled:hover:bg-surface disabled:text-faint';
</script>

<nav class="flex min-w-16 items-center gap-1" aria-label="Where you are">
  <button class={btn} onclick={() => nav.back()} disabled={!nav.canBack} title={withKey('Back', 'back')} aria-label="Back">
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"
      ><path d="M13 8H3.5M7.5 3.5L3 8l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="square" /></svg
    >
  </button>
  <button class={btn} onclick={() => nav.forward()} disabled={!nav.canForward} title={withKey('Forward', 'forward')} aria-label="Forward">
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"
      ><path d="M3 8h9.5M8.5 3.5L13 8l-4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="square" /></svg
    >
  </button>
  <ol class="m-0 ml-2 flex min-w-0 list-none items-center gap-2 p-0">
    {#each crumbs as c, i (i)}
      {@const last = i === crumbs.length - 1}
      <!-- Earlier crumbs give way first, so the place you are keeps its name;
           in a narrow header the middle ones go (Back still walks them). -->
      {@const middle = i > 0 && !last ? '@max-[640px]:hidden' : ''}
      {#if i}<li class="flex-none text-faint {middle}" aria-hidden="true">/</li>{/if}
      <li class="flex {last ? 'min-w-0' : 'min-w-4 shrink-[4]'} {middle}">
        {#if c.go && !last}
          <button
            class="min-w-0 truncate border-0 bg-transparent p-0 text-[14px] text-muted hover:text-ink hover:underline hover:underline-offset-4 {c.mono ? 'font-mono text-[13px]' : ''}"
            use:ownMenu={() => c.menu?.() ?? null}
            title={c.menu ? 'Go there; right-click for its menu' : undefined}
            onclick={c.go}>{c.label}</button
          >
        {:else}
          <span
            class="min-w-0 truncate text-[14px] font-semibold text-ink {c.mono ? 'font-mono text-[13px] font-medium' : ''}"
            aria-current={last ? 'page' : undefined}
            use:ownMenu={() => c.menu?.() ?? null}
            title={c.menu ? 'Right-click for its menu' : undefined}>{c.label}</span
          >
        {/if}
      </li>
    {/each}
  </ol>
</nav>
