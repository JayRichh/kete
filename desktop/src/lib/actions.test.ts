import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Agent, Model } from './api';

const calls: string[] = [];
let failRestart = false;
vi.mock('./api', () => {
  const rec = (name: string) => async (id: string) => {
    calls.push(`${name} ${id}`);
  };
  return {
    api: {
      archiveAgent: rec('archive'),
      unarchiveAgent: rec('unarchive'),
      deleteAgent: rec('delete'),
      boardScope: async (s: { repo?: string; worktree?: string }) => {
        calls.push(`scope ${s.repo}/${s.worktree}`);
      },
      followUp: async (id: string, text: string) => {
        calls.push(`follow-up ${id} ${text}`);
      },
      spawnFollowUp: async (id: string, text: string) => {
        calls.push(`spawn ${id} ${text}`);
        return 'a2';
      },
      askAside: async (id: string, question: string) => {
        calls.push(`aside ${id} ${question}`);
        return 'a3';
      },
      markSeen: async () => {},
      restartAgent: async (id: string) => {
        calls.push(`restart ${id}`);
        if (failRestart) throw 'gone';
      },
    },
  };
});

const { app } = await import('./store.svelte');
const { ui, ALT_CHOICE } = await import('./ui/ui.svelte');
const actions = await import('./actions');
const { leafItems } = await import('./menus/model');

const agent = (id: string, extra: Partial<Agent> = {}): Agent =>
  ({
    id,
    worktree_id: 'w1',
    name: id,
    status: 'finished',
    archived: false,
    unseen: false,
    status_changed_at: 0,
    kind: 'claude',
    ...extra,
  }) as Agent;

function setAgents(agents: Agent[]) {
  app.model = { ...app.model, agents, worktrees: [], projects: [], terminals: [] } as Model;
}

/** Answer the dialog that is up (or wait for one). */
async function answer(ok: boolean) {
  for (let i = 0; i < 50 && !ui.dialog; i++) await Promise.resolve();
  const d = ui.dialog;
  if (d?.kind !== 'confirm') throw new Error('no confirm up');
  d.resolve({ ok, checked: false });
  ui.dialog = null;
}

beforeEach(() => {
  calls.length = 0;
  failRestart = false;
  ui.dialog = null;
  app.toast = null;
});

describe('session verbs', () => {
  it('says a restart is under way only when it was', async () => {
    setAgents([agent('a')]);
    failRestart = true;
    await actions.restartAgent('a');
    expect(app.toast).toContain('Could not restart');
    failRestart = false;
    await actions.restartAgent('a');
    expect(app.toast).toBe('Restarting a.');
  });

  it('unarchive brings back the thread archive took', async () => {
    setAgents([agent('root', { archived: true }), agent('kid', { parent: 'root', archived: true }), agent('other', { archived: true })]);
    await actions.unarchiveAgent('root');
    expect(calls).toEqual(['unarchive root', 'unarchive kid']);
  });

  it('delete takes what is nested under the session when confirmed, not when asked', async () => {
    setAgents([agent('root')]);
    const done = actions.deleteAgent('root');
    // A session spawns under it while the dialog is up.
    setAgents([agent('root'), agent('late', { parent: 'root' })]);
    await answer(true);
    await done;
    expect(calls).toEqual(['delete late', 'delete root']);
  });

  it('archive from a subagent row always asks, and names the parent', async () => {
    app.launch = { ...app.launch, ask_before_archive: false };
    setAgents([agent('root')]);
    const done = actions.archiveAgent('root', true);
    for (let i = 0; i < 50 && !ui.dialog; i++) await Promise.resolve();
    expect(ui.dialog?.kind === 'confirm' && ui.dialog.message).toContain('parent session');
    await answer(false);
    await done;
    expect(calls).toEqual([]);
  });

  it('a subagent row menu says it acts on the parent session', () => {
    setAgents([agent('root')]);
    const menu = actions.menuFor({ kind: 'agent', id: 'root', viaSub: true });
    expect(menu[0]).toEqual({ heading: 'Parent session: root' });
    expect(actions.parseTarget('subagent:root')).toEqual({ kind: 'agent', id: 'root', viaSub: true });
  });
});

describe('menus from the registry', () => {
  const words = (menu: ReturnType<typeof actions.menuFor>) =>
    menu.flatMap((e) => (typeof e === 'object' && 'label' in e ? [e.label] : []));
  /** Every row a menu runs, a submenu's in its whole words. */
  const leaves = (menu: ReturnType<typeof actions.menuFor>) => leafItems(menu).map((e) => e.full ?? e.label);

  it('a session menu says what the terminal app says, in its order, keys shown', () => {
    setAgents([agent('a')]);
    const menu = actions.menuFor({ kind: 'agent', id: 'a' });
    const w = words(menu);
    // Its diff is its own row, after the reply (the registry's Go
    // section), not left to the checkout's rows.
    expect(w.slice(0, 4)).toEqual(['Open', 'Follow-up prompt\u2026', 'Ask aside\u2026', 'Diff']);
    expect(w).toContain('Restart agent');
    expect(w).toContain('Rename\u2026');
    expect(w.indexOf('Archive')).toBeLessThan(w.indexOf('Delete\u2026'));
    const rename = menu.find((e) => typeof e === 'object' && 'label' in e && e.label === 'Rename\u2026');
    expect((rename as { keys?: string[] } | undefined)?.keys).toEqual(['r']);
  });

  it('offers Make orchestrator only while no other session is pinned', () => {
    setAgents([agent('a'), agent('o', { role: 'orchestrator' })]);
    expect(words(actions.menuFor({ kind: 'agent', id: 'a' }))).not.toContain('Make orchestrator');
    expect(words(actions.menuFor({ kind: 'agent', id: 'o' }))).toContain('Unpin orchestrator');
    setAgents([agent('a'), agent('o')]);
    expect(words(actions.menuFor({ kind: 'agent', id: 'a' }))).toContain('Make orchestrator');
  });

  it('a terminal closes, a repo is removed from Kete', () => {
    app.model = {
      ...app.model,
      agents: [],
      terminals: [{ id: 't', worktree_id: 'w1', name: 'shell', sort_order: 0, alive: true, run_command: null }],
      worktrees: [{ id: 'w1', project_id: 'p', path: '/p', branch: 'main', is_main: true, sort_order: 0 }],
      projects: [{ id: 'p', name: 'demo', repo_path: '/p', sort_order: 0 }],
    } as unknown as Model;
    // Its checkout's folder to copy (the matrix widened Copy path to
    // terminals), then the checkout's own menu behind a rule.
    const term = actions.menuFor({ kind: 'terminal', id: 't' });
    expect(words(term)).toEqual(['Open', 'Copy path', 'Rename\u2026', 'Close terminal\u2026', 'Checkout main']);
    expect(term[term.length - 2]).toBe('sep');
    const repo = words(actions.menuFor({ kind: 'project', id: 'p' }));
    expect(repo[0]).toBe('New session\u2026');
    expect(repo).toContain('Remove from Kete\u2026');
    expect(repo.indexOf('Locate folder\u2026')).toBe(repo.indexOf('Remove from Kete\u2026') - 1);
    // The sweep only with a linked worktree to look at, as the terminal app.
    expect(repo).not.toContain('Delete merged & clean worktrees\u2026');
    app.model = {
      ...app.model,
      worktrees: [...app.model.worktrees, { id: 'w2', project_id: 'p', path: '/p2', branch: 'feat', is_main: false, sort_order: 1 }],
    } as unknown as Model;
    const swept = words(actions.menuFor({ kind: 'project', id: 'p' }));
    expect(swept.indexOf('Delete merged & clean worktrees\u2026')).toBeLessThan(swept.indexOf('Remove from Kete\u2026'));
  });

  it('offers the pull request verbs and Focus here under the shared ids, in the terminal app words', () => {
    const base = {
      ...app.model,
      agents: [agent('a')],
      terminals: [],
      worktrees: [{ id: 'w1', project_id: 'p', path: '/p/w1', branch: 'feat', is_main: false, sort_order: 0 }],
      projects: [{ id: 'p', name: 'demo', repo_path: '/p', sort_order: 0 }],
      cards: {},
    } as unknown as Model;
    // No pull request yet: Review and the composer fold into Pull
    // request \u25b8; no page and no merge.
    app.model = { ...base, prs: {} } as Model;
    let menu = actions.menuFor({ kind: 'agent', id: 'a' });
    expect(words(menu)).toContain('Pull request');
    let w = leaves(menu);
    expect(w).toContain('Create pull request\u2026');
    expect(w).toContain('Review\u2026');
    expect(w).not.toContain('Open pull request');
    expect(w.some((l) => l.startsWith('Merge'))).toBe(false);
    // Focus here is the card's checkout's (\u00a75.2), once.
    expect(w.filter((l) => l === 'Worktree feat \u203a Focus here').length).toBe(1);
    // One open: its page and its merge, once each; no Create.
    const pr = { number: 7, url: 'https://x/7', title: 't', state: 'open', draft: false };
    app.model = { ...base, prs: { w1: pr } } as unknown as Model;
    menu = actions.menuFor({ kind: 'agent', id: 'a' });
    w = leaves(menu);
    expect(w).toContain('Open pull request');
    expect(w).not.toContain('Create pull request\u2026');
    expect(w.filter((l) => l.startsWith('Merge')).length).toBe(1);
    expect(w).not.toContain('Merge PR\u2026');
    // The link to copy, now there is one.
    expect(w).toContain('Copy pull request link');
    // Focus here narrows the chip to the checkout.
    expect(actions.runVerb({ kind: 'worktree', id: 'w1' }, 'focus_scope')).toBe(true);
    expect(calls).toEqual(['scope p/w1']);
  });

  it('greys Create pull request on a checkout on its base branch, and says why', async () => {
    const { rows } = await import('./worktrees/rows.svelte');
    app.model = {
      ...app.model,
      agents: [],
      terminals: [],
      worktrees: [{ id: 'w1', project_id: 'p', path: '/p', branch: 'main', is_main: true, sort_order: 0 }],
      projects: [{ id: 'p', name: 'demo', repo_path: '/p', sort_order: 0 }],
      cards: {},
      prs: {},
    } as unknown as Model;
    const create = () =>
      actions
        .menuFor({ kind: 'worktree', id: 'w1' })
        .find((e) => typeof e === 'object' && 'label' in e && e.label.startsWith('Create pull request')) as
        | { label: string; disabled?: boolean; title?: string }
        | undefined;
    expect(create()?.disabled).toBeFalsy();
    rows.git = { ...rows.git, w1: { changed: 0, untracked: 0, upstream: null, ahead: null, behind: null, committed: 0, ahead_of_base: 0, on_default_branch: true } };
    const row = create();
    expect(row?.disabled).toBe(true);
    expect(row?.title).toMatch(/base branch/);
    expect(actions.runVerb({ kind: 'worktree', id: 'w1' }, 'create_pull_request')).toBe(false);
    expect(app.toast).toMatch(/base branch/);
    rows.git = {};
  });

  it('runs a verb on its target as the menu row does, and refuses one that means nothing there', async () => {
    setAgents([agent('a', { archived: true })]);
    expect(actions.runVerb({ kind: 'agent', id: 'a' }, 'archive')).toBe(false);
    expect(actions.runVerb({ kind: 'agent', id: 'a' }, 'unarchive')).toBe(true);
    await Promise.resolve();
    expect(calls).toEqual(['unarchive a']);
  });
});

describe('follow-ups that run as sessions of their own', () => {
  /** Fill the prompt that is up and send it, by its second button when `alt`. */
  async function send(values: Record<string, string>, alt = false) {
    for (let i = 0; i < 50 && !ui.dialog; i++) await Promise.resolve();
    const d = ui.dialog;
    if (d?.kind !== 'prompt') throw new Error('no prompt up');
    d.resolve(alt ? { ...values, [ALT_CHOICE]: '1' } : values);
    ui.dialog = null;
  }

  it('the follow-up box sends a turn, or starts it as a new session beside the card and opens that', async () => {
    setAgents([agent('a', { alive: true })]);
    let done = actions.followUp('a');
    for (let i = 0; i < 50 && !ui.dialog; i++) await Promise.resolve();
    expect(ui.dialog?.kind === 'prompt' && ui.dialog.alt?.label).toBe('Start as new session');
    await send({ text: 'run the tests' });
    await done;
    expect(calls).toEqual(['follow-up a run the tests']);

    calls.length = 0;
    done = actions.followUp('a');
    await send({ text: 'write the docs' }, true);
    await done;
    expect(calls).toEqual(['spawn a write the docs']);
    expect(app.session).toEqual({ Agent: 'a2' });
  });

  it('Ask aside sends the question for a session beside this one and opens it', async () => {
    setAgents([agent('a', { alive: true })]);
    const done = actions.askAside('a');
    await send({ question: 'why two refreshes?' });
    await done;
    expect(calls).toEqual(['aside a why two refreshes?']);
    expect(app.session).toEqual({ Agent: 'a3' });
  });

  it('a session menu carries Ask aside after the follow-up', () => {
    setAgents([agent('a', { alive: true })]);
    const labels = leafItems(actions.menuFor({ kind: 'agent', id: 'a' })).map((e) => e.label);
    expect(labels.indexOf('Ask aside\u2026')).toBe(labels.indexOf('Follow-up prompt\u2026') + 1);
  });
});
