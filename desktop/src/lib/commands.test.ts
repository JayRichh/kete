import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./api', () => ({ api: { boardScope: vi.fn(() => Promise.resolve()) } }));

const { api } = await import('./api');

const { app } = await import('./store.svelte');
const { nav } = await import('./shell/history.svelte');
const { runShortcut, toggleMaximize, verbsMenu } = await import('./commands');
const { ui } = await import('./ui/ui.svelte');
const { paletteCommands } = await import('./commands');
const { selection } = await import('./selection.svelte');

describe('the full pane', () => {
  beforeEach(() => {
    app.paneMax = false;
    nav.go({ view: 'overview' });
  });

  it('does nothing without a split', () => {
    expect(toggleMaximize()).toBe(false);
    expect(runShortcut('maximize')).toBe(false);
    expect(app.paneMax).toBe(false);
  });

  it('toggles with a split up, by the key or the button', () => {
    nav.go({ view: 'diff', item: 'w1' });
    expect(runShortcut('maximize_z')).toBe(true);
    expect(app.paneMax).toBe(true);
    expect(runShortcut('maximize_z')).toBe(true);
    expect(app.paneMax).toBe(false);
    expect(runShortcut('maximize')).toBe(true);
    expect(app.paneMax).toBe(true);
    expect(toggleMaximize()).toBe(true);
    expect(app.paneMax).toBe(false);
  });

  it('comes back down on the first Esc and closes the split on the next', () => {
    nav.go({ view: 'overview' });
    nav.go({ view: 'session', item: 'a:x' });
    app.paneMax = true;
    expect(runShortcut('close_split')).toBe(true);
    expect(app.paneMax).toBe(false);
    expect(nav.current.view).toBe('session');
    expect(runShortcut('close_split')).toBe(true);
    expect(nav.current.view).toBe('overview');
  });
});

describe('the shared keys', () => {
  beforeEach(() => {
    app.paneMax = false;
    nav.go({ view: 'overview' });
  });

  it('f and / open the palette, unless the view filters itself', async () => {
    const { ui } = await import('./ui/ui.svelte');
    ui.palette = false;
    expect(runShortcut('find')).toBe(true);
    expect(ui.palette).toBe(true);
    ui.palette = false;
    nav.go({ view: 'diff', item: 'w1' });
    expect(runShortcut('find')).toBe(false);
    nav.go({ view: 'branches', repo: 'p1' });
    expect(runShortcut('find')).toBe(false);
    expect(runShortcut('refresh')).toBe(false);
    expect(ui.palette).toBe(false);
  });

  it('a digit opens that repo on the rail, in the rail order', () => {
    const was = app.model.projects;
    app.model.projects = [
      { id: 'p1', name: 'api', repo_path: '/api', sort_order: 1 },
      { id: 'p2', name: 'web', repo_path: '/web', sort_order: 0 },
    ] as never;
    expect(runShortcut('repo_1')).toBe(true);
    expect(nav.current.repo).toBe('p2');
    // It narrows the window's scope to that repo too (the header's chip).
    expect(api.boardScope).toHaveBeenLastCalledWith({ repo: 'p2' });
    expect(runShortcut('repo_2')).toBe(true);
    expect(nav.current.repo).toBe('p1');
    expect(runShortcut('repo_3')).toBe(false);
    app.model.projects = was;
  });
});

describe('the verbs menu (?)', () => {
  const label = (e: unknown) => (typeof e === 'object' && e && 'label' in e ? (e as { label: string }).label : null);

  it('lists the verbs of the row with their keys, All keys last', () => {
    app.model = {
      ...app.model,
      agents: [{ id: 'a', worktree_id: 'w', name: 'fix-login', archived: false, unseen: false, status: 'finished' }],
      terminals: [],
      worktrees: [{ id: 'w', project_id: 'p', path: '/p', branch: 'main', is_main: true, sort_order: 0 }],
      projects: [{ id: 'p', name: 'demo', repo_path: '/p', sort_order: 0 }],
    } as unknown as typeof app.model;
    const { items, title } = verbsMenu({ kind: 'agent', id: 'a' });
    expect(title).toBe('session fix-login');
    expect(label(items[0])).toBe('Open');
    const last = items[items.length - 1] as { label: string; pinned?: boolean; run: () => void };
    expect(last.label).toBe('All keys\u2026');
    expect(last.pinned).toBe(true);
    last.run();
    expect(ui.help).toBe(true);
    ui.help = false;
  });

  it('offers the own verbs of the window with nothing selected', () => {
    const { items, title } = verbsMenu(null);
    expect(title).toBe('Verbs');
    expect(items.map(label)).toContain('New session\u2026');
  });

  it('offers the 0.60 window verbs in the palette while they apply', () => {
    nav.go({ view: 'overview' });
    const labels = () => paletteCommands().map((c) => c.label);
    app.studioRepos = [];
    expect(labels()).toContain('Export usage (CSV)');
    expect(labels()).not.toContain('Studio board');
    expect(labels()).not.toContain('Triage note\u2026');
    app.studioRepos = [{ repo: 'p', name: 'demo', manifest: true, enabled: true, autonomy: 'A1' }] as unknown as typeof app.studioRepos;
    app.model = {
      ...app.model,
      agents: [...app.model.agents, { id: 'o', worktree_id: 'w', name: 'orchestrator', archived: false, unseen: false, status: 'idle', role: 'orchestrator' }],
    } as unknown as typeof app.model;
    expect(labels()).toEqual(expect.arrayContaining(['Studio board', 'Gauntlet', 'Triage note\u2026', 'Export usage (CSV)']));
    app.studioRepos = [];
  });

  it('puts the verbs of the selection first in the palette', () => {
    selection.target = { kind: 'agent', id: 'a' };
    const rows = paletteCommands().filter((c) => c.pinned);
    expect(rows[0]).toMatchObject({ label: 'Open', group: 'session fix-login' });
    selection.target = null;
  });
});
