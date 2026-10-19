// Every registry target's menu, built from a model, against the menu
// matrix (\u00a75.1-5.6, 5.15): rows, order, rules, submenus, state banners and
// the rows state hides, greys or turns.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Agent, Model } from '../api';

vi.mock('../api', () => ({ api: { boardScope: vi.fn(() => Promise.resolve()), markSeen: vi.fn(() => Promise.resolve()), archiveAgent: vi.fn(() => Promise.resolve()) } }));

const { api } = await import('../api');
const { app } = await import('../store.svelte');
const { ui } = await import('../ui/ui.svelte');
const actions = await import('../actions');
const { backgroundMenu } = await import('../commands');
const { rows: gitRows } = await import('../worktrees/rows.svelte');
const { nav } = await import('../shell/history.svelte');
const { critters } = await import('../critters/state.svelte');
import type { MenuEntry, MenuItem } from '../ui/ui.svelte';

/** A menu as lines: `---` a rule, `# words` a heading, `Words >` a
 *  submenu with its rows indented under it, `(x)` greyed, `[v]` / `[ ]`
 *  ticked or not. */
function dump(es: readonly MenuEntry[], indent = ''): string[] {
  return es.flatMap((e): string[] => {
    if (e === 'sep') return [`${indent}---`];
    if ('heading' in e) return [`${indent}# ${e.heading}`];
    const tick = e.checked === undefined ? '' : e.checked ? '[v] ' : '[ ] ';
    const words = `${indent}${tick}${e.disabled ? `(${e.label})` : e.label}`;
    return e.children ? [`${words} >`, ...dump(e.children, indent + '  ')] : [words];
  });
}

const find = (es: readonly MenuEntry[], label: string): MenuItem | undefined => {
  for (const e of es) {
    if (typeof e !== 'object' || !('run' in e)) continue;
    if (e.label === label) return e;
    const c = e.children && find(e.children, label);
    if (c) return c;
  }
  return undefined;
};

const agent = (id: string, extra: Partial<Agent> = {}): Agent =>
  ({
    id,
    worktree_id: 'w1',
    name: id,
    status: 'finished',
    archived: false,
    unseen: false,
    alive: true,
    status_changed_at: 0,
    kind: 'claude',
    custom_harness: null,
    profile: null,
    role: 'worker',
    sort_order: 0,
    parent: null,
    ...extra,
  }) as Agent;

const PR = { number: 42, url: 'https://github.com/o/r/pull/42', title: 'Fix it', draft: false, state: 'open' as const };

/** One repo, kete: its root on main, a linked feat-x with an open pull
 *  request, a dev server on 5173 and its run terminal up; three sessions
 *  there: a running, b finished and unread, c finished and read. */
function model(extra: Partial<Model> = {}): Model {
  return {
    ...app.model,
    connected: true,
    protocol_version: 62,
    projects: [{ id: 'p', name: 'kete', repo_path: 'C:\\code\\kete', sort_order: 0 }],
    worktrees: [
      { id: 'w0', project_id: 'p', path: 'C:\\code\\kete', branch: 'main', is_main: true, sort_order: 0 },
      { id: 'w1', project_id: 'p', path: 'C:\\code\\kete-feat-x', branch: 'feat-x', is_main: false, sort_order: 1 },
    ],
    agents: [agent('a', { status: 'running' }), agent('b', { unseen: true }), agent('c')],
    terminals: [{ id: 't1', worktree_id: 'w1', name: 'run', sort_order: 0, alive: true, run_command: 'npm run dev' }],
    ports: [{ worktree: 'w1', block: null, live: [5173] }],
    prs: { w1: PR },
    subagents: [],
    cards: {},
    lanes: { needs: [], running: ['a'], unseen: ['b'], done: ['c'], pr: [] },
    hidden: { needs: 0, running: 0, unseen: 0, done: 0, pr: 0 },
    scope: {},
    ...extra,
  } as Model;
}

beforeEach(() => {
  app.model = model();
  app.runCommands = { w1: 'npm run dev' };
  app.accounts = [];
  app.adoCtx = {};
  app.showArchived = false;
  app.home = 'overview';
  gitRows.git = {};
  ui.dialog = null;
  nav.go({ view: 'overview' });
});

describe('the session menu (\u00a75.1)', () => {
  it('reads as the matrix lays it out: sections ruled, groups folded, the checkout last', () => {
    expect(dump(actions.menuFor({ kind: 'agent', id: 'a' }))).toEqual([
      '# running',
      'Open',
      'Follow-up prompt\u2026',
      'Ask aside\u2026',
      '---',
      'Diff',
      'Pull request >',
      '  Review\u2026',
      '  Open in browser',
      '  Merge\u2026',
      '---',
      'Open in >',
      '  VS Code',
      '  Explorer',
      '  Browser \u00b7 localhost:5173',
      '---',
      'Copy >',
      '  Path',
      '  Branch name',
      '  Pull request link',
      '---',
      'Restart agent',
      'Duplicate',
      'Rename\u2026',
      'Make orchestrator',
      '---',
      'Move >',
      '  To another worktree\u2026',
      '  To another account\u2026',
      '  Up',
      '  Down',
      '---',
      'Archive',
      'Delete\u2026',
      '---',
      'Worktree feat-x >',
      '  New session\u2026',
      '  New terminal',
      '  Stop run',
      '  Focus here',
      '  Open remote in browser',
      '  Show run terminal',
      '  [v] Open in editor',
      '  Mark all seen',
      '  Move worktree up',
      '  Move worktree down',
      '  Archive done sessions\u2026',
      '  Delete all sessions\u2026',
      '  Delete worktree\u2026',
    ]);
  });

  it("names a stop note up top in the terminal app's words", () => {
    const note = (reason: string, extra: object = {}) => ({ reason, waiting: false, resumes: null, resets_at: null, detail: null, at: Date.now(), ...extra });
    const banner = (n: ReturnType<typeof note>, a: Partial<Agent> = {}) => {
      app.model = model({ agents: [agent('a', a)], stops: { a: n } });
      return dump(actions.menuFor({ kind: 'agent', id: 'a' }))[0];
    };
    expect(banner(note('rate_limit', { waiting: true, resumes: '3:45pm' }))).toBe('# limited \u00b7 resumes 3:45pm');
    expect(banner(note('authentication_failed'), { profile: 'work' })).toBe('# needs you \u00b7 log in again (work)');
    expect(banner(note('authentication_failed'))).toBe('# needs you \u00b7 log in again (claude)');
    expect(banner(note('billing_error'))).toBe('# needs you \u00b7 billing');
    expect(banner(note('overloaded'))).toBe('# error \u00b7 overloaded');
    expect(banner(note('overloaded'), { role: 'orchestrator' })).toBe('# orchestrator \u00b7 error \u00b7 overloaded');
    // The model helper keeps what it is not given: no note for the tests after.
    app.model = model({ stops: {} });
  });

  it('offers Mark seen on an unread one, says so up top, and leaves Make orchestrator off while another is pinned', () => {
    app.model = model({ agents: [agent('a'), agent('b', { unseen: true }), agent('o', { role: 'orchestrator', worktree_id: 'w0' })] });
    const lines = dump(actions.menuFor({ kind: 'agent', id: 'b' }));
    expect(lines[0]).toBe('# done \u00b7 unread');
    expect(lines).toContain('Mark seen');
    expect(lines).not.toContain('Make orchestrator');
    expect(dump(actions.menuFor({ kind: 'agent', id: 'o' }))).toContain('Unpin orchestrator');
  });

  it('greys Merge on a draft with its reason, and shows a merged one as that, with no Create', () => {
    app.model = model({ prs: { w1: { ...PR, draft: true } } });
    const merge = find(actions.menuFor({ kind: 'agent', id: 'a' }), 'Merge\u2026');
    expect(merge).toMatchObject({ disabled: true, title: 'Draft pull request: publish it first' });
    app.model = model({ prs: { w1: { ...PR, state: 'merged' } } });
    const lines = dump(actions.menuFor({ kind: 'agent', id: 'a' }));
    expect(lines).toContain('  Open merged pull request');
    expect(lines.some((l) => l.includes('Create'))).toBe(false);
    expect(lines.some((l) => l.includes('Merge'))).toBe(false);
  });

  it('keeps Review alone in its whole words with no pull request rows, and Create on the base branch greyed', () => {
    app.model = model({ prs: {} });
    gitRows.git = { w1: { changed: 0, untracked: 0, upstream: null, ahead: null, behind: null, committed: 0, ahead_of_base: 0, on_default_branch: true } };
    const menu = actions.menuFor({ kind: 'agent', id: 'a' });
    const create = find(menu, 'Create\u2026');
    expect(create?.disabled).toBe(true);
    expect(create?.title).toMatch(/base branch/);
  });

  it('greys To another account on a daemon too old to move sessions, saying why', () => {
    app.model = model({ protocol_version: 40 });
    const row = find(actions.menuFor({ kind: 'agent', id: 'a' }), 'To another account\u2026');
    expect(row?.disabled).toBe(true);
    expect(row?.title).toMatch(/older than this window/);
  });

  it('adds Log in to the account first, and a parent session to open', () => {
    app.accounts = [
      { key: 'work', name: 'work', harness: 'claude', profile: true, default: false, disabled: false, useDefault: false, login: 'needs_login', loginHint: 'kete profile login work', usage: null },
    ] as never;
    app.model = model({ agents: [agent('boss'), agent('a', { profile: 'work', parent: 'boss', status: 'running' })] });
    const lines = dump(actions.menuFor({ kind: 'agent', id: 'a' }));
    expect(lines[0]).toBe('# needs you \u00b7 log in again (work)');
    expect(lines[1]).toBe('Log in to work\u2026');
    expect(lines.indexOf('Open parent session boss')).toBe(lines.indexOf('Diff') + 1);
  });

  it('gives an archived one Duplicate, Unarchive and Delete under its banner, and nothing else', () => {
    app.model = model({ agents: [agent('a', { archived: true })] });
    expect(dump(actions.menuFor({ kind: 'agent', id: 'a' }))).toEqual(['# archived', 'Duplicate', '---', 'Unarchive', 'Delete\u2026']);
  });

  it("names a subagent row's parent and drops its reorder", () => {
    const lines = dump(actions.menuFor({ kind: 'agent', id: 'a', viaSub: true }));
    expect(lines[0]).toBe('# Parent session: a');
    expect(lines).toContain('Archive parent session\u2026');
    expect(lines).not.toContain('  Up');
  });
});

describe('the terminal menu (\u00a75.3)', () => {
  it('copies its checkout folder and carries the checkout last', () => {
    const lines = dump(actions.menuFor({ kind: 'terminal', id: 't1' }));
    expect(lines.slice(0, 6)).toEqual(['Open', '---', 'Copy path', '---', 'Rename\u2026', '---']);
    expect(lines).toContain('Close terminal\u2026');
    expect(lines).toContain('Worktree feat-x >');
    expect(find(actions.menuFor({ kind: 'terminal', id: 't1' }), 'Copy path')?.title).toBe('C:\\code\\kete-feat-x');
  });
});

describe('the worktree menu (\u00a75.5)', () => {
  it('folds its pull request, Open in and Copy, and opens with its banner', () => {
    expect(dump(actions.menuFor({ kind: 'worktree', id: 'w1' }))).toEqual([
      '# running :5173',
      'New session\u2026',
      'New terminal',
      'Stop run',
      '---',
      'Diff',
      'Pull request >',
      '  Open in browser',
      '  Merge\u2026',
      'Focus here',
      'Open remote in browser',
      'Show run terminal',
      '---',
      'Open in >',
      '  [v] Open command',
      '  VS Code',
      '  Explorer',
      '  Browser \u00b7 localhost:5173',
      '---',
      'Copy >',
      '  Path',
      '  Branch name',
      '  Pull request link',
      '---',
      'Mark all seen',
      '---',
      'Move >',
      '  Up',
      '  Down',
      '---',
      'Archive done sessions\u2026',
      'Delete all sessions\u2026',
      'Delete worktree\u2026',
    ]);
  });

  it('turns Focus here into Widen to the repo once scoped to it, and says a merged one can go', () => {
    app.model = model({ scope: { repo: 'p', worktree: 'w1' }, prs: { w1: { ...PR, state: 'merged' } } });
    const lines = dump(actions.menuFor({ kind: 'worktree', id: 'w1' }));
    expect(lines[0]).toBe('# merged \u00b7 running :5173');
    expect(lines).toContain('Widen to the repo');
    expect(lines).toContain('Delete worktree (pull request merged)\u2026');
  });

  it('lists its sessions to open where they are not on screen', () => {
    const lines = dump(actions.menuFor({ kind: 'worktree', id: 'w1' }, { jump: true }));
    const at = lines.indexOf('Open session >');
    expect(at).toBe(lines.indexOf('Show run terminal') + 1);
    expect(lines.slice(at + 1, at + 5)).toEqual(['  a', '  b', '  c', '  run']);
  });

  it('gives the root Switch branch and no Move, Delete worktree or Stop run without a run', () => {
    app.model = model({ terminals: [], ports: [] });
    app.runCommands = {};
    const lines = dump(actions.menuFor({ kind: 'worktree', id: 'w0' }));
    expect(lines).toContain('Switch branch\u2026');
    expect(lines.some((l) => /Move|Delete worktree|Stop run|^Run$/.test(l))).toBe(false);
  });

  it('marks its unread sessions seen and archives its done ones after one confirm', async () => {
    find(actions.menuFor({ kind: 'worktree', id: 'w1' }), 'Mark all seen')!.run();
    expect(api.markSeen).toHaveBeenCalledWith('b');
    find(actions.menuFor({ kind: 'worktree', id: 'w1' }), 'Archive done sessions\u2026')!.run();
    for (let i = 0; i < 20 && !ui.dialog; i++) await Promise.resolve();
    expect(ui.dialog?.kind === 'confirm' && ui.dialog.title).toBe('Archive 1 done session');
    if (ui.dialog?.kind === 'confirm') ui.dialog.resolve({ ok: true, checked: false });
    for (let i = 0; i < 20; i++) await Promise.resolve();
    expect(api.archiveAgent).toHaveBeenCalledWith('c');
    expect(api.archiveAgent).not.toHaveBeenCalledWith('a');
  });
});

describe('the repo menu (\u00a75.6)', () => {
  it('lists its worktrees, its remote by host, its default account and its own rows in order', () => {
    app.launch = { ...app.launch, profiles: [{ name: 'work', harness: 'claude', disabled: false }] as never, project_accounts: {} as never };
    const lines = dump(actions.menuFor({ kind: 'project', id: 'p' }));
    expect(lines.slice(0, 4)).toEqual(['New session\u2026', 'New worktree\u2026', '---', 'Focus here']);
    expect(lines).toContain('GitHub >');
    expect(lines).toContain('Worktrees >');
    expect(lines).toContain('  feat-x');
    expect(lines).toContain('  All worktrees\u2026');
    expect(lines).toContain('Default account >');
    expect(lines).toContain('  [v] Its default');
    expect(lines).toContain('  [ ] work');
    const tail = lines.slice(lines.indexOf('Archive done sessions\u2026'));
    expect(tail).toEqual(['Archive done sessions\u2026', 'Locate folder\u2026', 'Delete merged & clean worktrees\u2026', 'Remove from Kete\u2026']);
  });

  it('turns Focus here into Widen to everything once scoped to it, and names Azure DevOps', () => {
    app.model = model({ scope: { repo: 'p' } });
    app.adoCtx = { p: { org: 'o', project: 'x', repo: 'r' } as never };
    const lines = dump(actions.menuFor({ kind: 'project', id: 'p' }));
    expect(lines).toContain('Widen to everything');
    expect(lines).toContain('Azure DevOps >');
    expect(lines).toContain('  Azure DevOps\u2026');
  });
});

describe('the background menu (\u00a75.15)', () => {
  it('ticks the home in use and the archived sessions shown, and works on the scope', () => {
    app.showArchived = true;
    const lines = dump(backgroundMenu());
    expect(lines.slice(0, 3)).toEqual(['New session\u2026', 'Add project\u2026', 'Command palette']);
    expect(lines).toContain('View >');
    expect(lines).toContain('  [v] Overview');
    expect(lines).toContain('  [ ] Board');
    expect(lines).toContain('  [v] Archived sessions');
    expect(lines).toContain('Settings');
    expect(lines).toContain('Mark all seen');
    expect(lines[lines.length - 1]).toBe('Archive done sessions\u2026');
  });

  it('lists Whakaoho in View after Accounts, and Critters once they are found, ticked while on', async () => {
    const settingsSet = vi.fn(() => Promise.resolve());
    Object.assign(api, { settingsSet });
    nav.go({ view: 'overview' });
    critters.found = false;
    critters.prefs = { ...critters.prefs, on: false };
    const first = dump(backgroundMenu());
    const view = first.slice(first.indexOf('View >') + 1);
    expect(view.indexOf('  [ ] Whakaoho')).toBe(view.indexOf('  [ ] Accounts') + 1);
    expect(view.indexOf('  [ ] Archived sessions')).toBeGreaterThan(view.indexOf('  [ ] Whakaoho'));
    // An easter egg: no row until the palette, the Konami code or the kete mark found them.
    expect(first).not.toContain('[ ] Critters');
    critters.found = true;
    const lines = dump(backgroundMenu());
    expect(lines.indexOf('[ ] Critters')).toBe(lines.indexOf('Mark all seen') + 1);
    find(backgroundMenu(), 'Whakaoho')!.run();
    expect(nav.current.view).toBe('whakaoho');
    expect(dump(backgroundMenu())).toContain('  [v] Whakaoho');
    find(backgroundMenu(), 'Critters')!.run();
    await vi.waitFor(() => expect(settingsSet).toHaveBeenCalledWith({ scope: 'global', key: 'critters' }, true));
    expect(dump(backgroundMenu())).toContain('[v] Critters');
  });

  it('leaves the bulk rows off with nothing to take', () => {
    app.model = model({ agents: [agent('a', { status: 'running' })] });
    const lines = dump(backgroundMenu());
    expect(lines).not.toContain('Mark all seen');
    expect(lines).not.toContain('Archive done sessions\u2026');
  });
});
