import { describe, expect, it, vi } from 'vitest';

// layout.ts reaches the verb registry, which reaches the api module.
vi.mock('../api', () => ({ api: {} }));

const { accountBanner, accountItems, usageWords } = await import('./account');
import type { AccountState } from './account';
import type { Row } from './layout';
import type { AccountUsage, UsageWindow } from '../api';
import type { MenuEntry, MenuHeading, MenuItem } from '../ui/ui.svelte';
import { setLabels } from '../accounts/labels.svelte';
import type { UsageLabel } from '../api';

const isItem = (e: MenuEntry): e is MenuItem => typeof e === 'object' && 'label' in e;
const rows = (es: MenuEntry[]): MenuItem[] => es.filter(isItem);
const labels = (es: MenuEntry[]) => es.map((e) => (e === 'sep' ? 'sep' : 'heading' in e ? e.heading : e.label));
const headings = (es: MenuEntry[]): MenuHeading[] => es.filter((e): e is MenuHeading => typeof e === 'object' && 'heading' in e);

function item(es: MenuEntry[], label: string): MenuItem {
  const r = rows(es).find((x) => x.label === label);
  if (!r) throw new Error(`no row labelled ${label}`);
  return r;
}

/** Every row that runs something, a submenu's rows too. */
const everyRow = (es: MenuEntry[]): MenuItem[] => rows(es).flatMap((r) => [r, ...rows(r.children ?? [])]);
/** The labels of the greyed-out rows, in menu order. */
const disabledRows = (es: MenuEntry[]) => rows(es).filter((r) => r.disabled).map((r) => r.label);
/** Every greyed-out row says why (the matrix, 8.4). */
const allExplained = (es: MenuEntry[]) => everyRow(es).filter((r) => r.disabled).every((r) => !!r.title);

const acct = (o: Partial<AccountState> = {}): AccountState => ({
  name: 'work',
  harness: 'claude',
  profile: true,
  default: true,
  disabled: false,
  problem: null,
  login: 'logged_in',
  expired: false,
  loginHint: 'kete profile login work',
  named: true,
  renameBlocked: null,
  repos: [
    { id: 'p1', name: 'kete', now: null },
    { id: 'p2', name: 'tiki', now: null },
  ],
  limitedResets: null,
  sessionPercent: null,
  ...o,
});

const verbs = () => ({
  start: vi.fn(),
  login: vi.fn(),
  adopt: vi.fn(),
  copyLogin: vi.fn(),
  setDefault: vi.fn(),
  useFor: vi.fn(),
  setEnabled: vi.fn(),
  edit: vi.fn(),
  rename: vi.fn(),
  duplicate: vi.fn(),
  remove: vi.fn(),
});

/** A registry row, as `registryRows` would hand it in. */
const reg = (id: string, section: Row['section'], label: string): Row => ({ id, section, item: { label, run: vi.fn() } });
const GO = [reg('scope_account', 'go', 'Show its sessions'), reg('accounts', 'go', 'Account details\u2026')];

const menu = (o: Partial<AccountState> = {}, registry: Row[] = GO) => accountItems(acct(o), verbs(), registry);

describe('a fresh profile (the default, switched on)', () => {
  it('lists its rows by section, with a rule where the section changes', () => {
    expect(labels(menu())).toEqual([
      'Start a session on it\u2026',
      'sep',
      'Show its sessions',
      'Account details\u2026',
      'sep',
      'Copy sign-in command',
      'sep',
      'Make default',
      'Use for',
      'Switch on',
      'Edit\u2026',
      'Rename\u2026',
      'Duplicate\u2026',
      'sep',
      'Remove\u2026',
    ]);
  });

  it('opens with no banner', () => {
    expect(headings(menu())).toEqual([]);
  });

  it('ticks Make default and Switch on, and shows the keys Rename and Remove have', () => {
    const items = menu();
    expect(item(items, 'Make default').checked).toBe(true);
    expect(item(items, 'Switch on').checked).toBe(true);
    expect(item(items, 'Rename\u2026').keys).toEqual(['r']);
    expect(item(items, 'Remove\u2026').keys).toEqual(['delete']);
    expect(item(items, 'Remove\u2026').danger).toBe(true);
  });

  it('greys out nothing', () => {
    expect(disabledRows(menu())).toEqual([]);
  });

  it('never folds: the registry rows and the change rows are plain rows', () => {
    const items = menu();
    expect(rows(items).filter((r) => r.children?.length).map((r) => r.label)).toEqual(['Use for']);
  });
});

describe('Make default', () => {
  it('is unticked on another account, and runs the verb', () => {
    const v = verbs();
    const items = accountItems(acct({ default: false }), v, GO);
    const r = item(items, 'Make default');
    expect(r.checked).toBe(false);
    expect(r.disabled).toBeFalsy();
    r.run();
    expect(v.setDefault).toHaveBeenCalledTimes(1);
  });

  it('does nothing when the account already is the default', () => {
    const v = verbs();
    const r = item(accountItems(acct({ default: true }), v, GO), 'Make default');
    expect(r.disabled).toBeFalsy();
    r.run();
    expect(v.setDefault).not.toHaveBeenCalled();
  });
});

describe('a switched-off profile', () => {
  it('opens with the switched off line, unticks Switch on and offers no session', () => {
    const v = verbs();
    const items = accountItems(acct({ default: false, disabled: true }), v, GO);
    expect(headings(items)).toEqual([{ heading: 'switched off' }]);
    expect(item(items, 'Switch on').checked).toBe(false);
    expect(labels(items)).not.toContain('Start a session on it\u2026');
    item(items, 'Switch on').run();
    expect(v.setEnabled).toHaveBeenCalledTimes(1);
  });

  it('keeps the rows that change it', () => {
    const l = labels(menu({ disabled: true }));
    expect(l).toEqual(expect.arrayContaining(['Make default', 'Use for', 'Edit\u2026', 'Rename\u2026', 'Duplicate\u2026', 'Remove\u2026']));
  });
});

describe('a logged-out profile', () => {
  it('opens with an ember logged out line and puts Log in again after Start, with an ember dot', () => {
    const v = verbs();
    const items = accountItems(acct({ login: 'needs_login' }), v, GO);
    expect(headings(items)).toEqual([{ heading: 'logged out', ember: true }]);
    expect(labels(items).slice(0, 3)).toEqual(['logged out', 'Start a session on it\u2026', 'Log in again\u2026']);
    const r = item(items, 'Log in again\u2026');
    expect(r.icon).toEqual({ dot: 'ember' });
    expect(r.title).toBe('kete profile login work');
    r.run();
    expect(v.login).toHaveBeenCalledTimes(1);
  });

  it('says login expired when the credentials ran out', () => {
    const items = menu({ login: 'needs_login', expired: true });
    expect(headings(items)).toEqual([{ heading: 'login expired', ember: true }]);
    expect(labels(items)).toContain('Log in again\u2026');
  });

  it('offers no login with no command to run, and none while signed in', () => {
    expect(labels(menu({ login: 'needs_login', loginHint: null }))).not.toContain('Log in again\u2026');
    expect(labels(menu())).not.toContain('Log in again\u2026');
  });

  it('copies the sign-in command from its own row', () => {
    const v = verbs();
    const r = item(accountItems(acct({ login: 'needs_login' }), v, GO), 'Copy sign-in command');
    r.run();
    expect(v.copyLogin).toHaveBeenCalledTimes(1);
    expect(labels(menu({ loginHint: null }))).not.toContain('Copy sign-in command');
  });
});

describe('a harness own login', () => {
  const own = { profile: false, default: false, name: 'claude', loginHint: 'claude auth login --claudeai' };

  it('offers Add as account and none of the rows that change a profile', () => {
    const v = verbs();
    const items = accountItems(acct(own), v, GO);
    expect(labels(items)).toEqual(['Add as account', 'sep', 'Show its sessions', 'Account details\u2026', 'sep', 'Copy sign-in command']);
    item(items, 'Add as account').run();
    expect(v.adopt).toHaveBeenCalledTimes(1);
  });

  it('shows Add as account alone when it has nothing else', () => {
    expect(labels(menu({ ...own, loginHint: null }, []))).toEqual(['Add as account']);
  });

  it('puts Log in again before Add as account when it is logged out', () => {
    const items = menu({ ...own, login: 'needs_login' });
    expect(labels(items).slice(0, 3)).toEqual(['logged out', 'Log in again\u2026', 'Add as account']);
  });
});

describe('a broken entry', () => {
  const broken = { problem: 'the env is not a table', default: false, loginHint: null, login: 'unknown' as const };

  it('opens with the broken line and keeps only Edit, Rename and Remove of the profile rows', () => {
    const items = menu(broken);
    expect(headings(items)).toEqual([{ heading: 'broken' }]);
    expect(labels(items)).toEqual(['broken', 'Show its sessions', 'Account details\u2026', 'sep', 'Edit\u2026', 'Rename\u2026', 'sep', 'Remove\u2026']);
  });

  it('greys out Edit with the way to fix it', () => {
    const items = menu(broken);
    expect(disabledRows(items)).toEqual(['Edit\u2026']);
    expect(item(items, 'Edit\u2026').title).toBe('Fix this entry in config.json, or rename it');
    expect(allExplained(items)).toBe(true);
  });

  it('puts the broken line before a login line', () => {
    expect(headings(menu({ ...broken, login: 'needs_login' }))).toEqual([{ heading: 'broken' }]);
  });
});

describe('Edit', () => {
  it('runs its verb on a working profile', () => {
    const v = verbs();
    const r = item(accountItems(acct(), v, GO), 'Edit\u2026');
    expect(r.disabled).toBeFalsy();
    r.run();
    expect(v.edit).toHaveBeenCalledTimes(1);
  });

  it('is left out for an own login', () => {
    expect(labels(menu({ profile: false, default: false }))).not.toContain('Edit\u2026');
  });
});

describe('Rename', () => {
  it('greys out while sessions block it, saying what blocks', () => {
    const why = 'work has sessions: 1 running (a). Move them to another account (or archive them), then rename.';
    const items = menu({ renameBlocked: why });
    expect(disabledRows(items)).toEqual(['Rename\u2026']);
    expect(item(items, 'Rename\u2026').title).toBe(why);
  });

  it('is left out for a profile with no name', () => {
    expect(labels(menu({ named: false }))).not.toContain('Rename\u2026');
  });

  it('runs its verb when nothing blocks it', () => {
    const v = verbs();
    item(accountItems(acct(), v, GO), 'Rename\u2026').run();
    expect(v.rename).toHaveBeenCalledTimes(1);
  });
});

describe('Use for', () => {
  const repos = [
    { id: 'p1', name: 'kete', now: 'work' },
    { id: 'p2', name: 'tiki', now: 'play' },
    { id: 'p3', name: 'ampsize', now: null },
  ];

  it('lists a row per repo, ticked where this account is that repo default', () => {
    const items = menu({ repos });
    const use = item(items, 'Use for');
    expect(use.disabled).toBeFalsy();
    expect(use.children?.map((c) => (isItem(c) ? [c.label, c.checked] : null))).toEqual([
      ['kete', true],
      ['tiki', false],
      ['ampsize', false],
    ]);
  });

  it('says which account holds a repo that is another one default, and how to clear the ticked one', () => {
    const kids = rows(item(menu({ repos }), 'Use for').children ?? []);
    expect(kids.map((c) => c.title)).toEqual(['Clear', 'Now play', undefined]);
  });

  it('makes the account a repo default on an unticked row and clears it on the ticked one', () => {
    const v = verbs();
    const kids = rows(item(accountItems(acct({ repos }), v, GO), 'Use for').children ?? []);
    kids[2].run();
    expect(v.useFor).toHaveBeenLastCalledWith('p3', false);
    kids[1].run();
    expect(v.useFor).toHaveBeenLastCalledWith('p2', false);
    kids[0].run();
    expect(v.useFor).toHaveBeenLastCalledWith('p1', true);
  });

  it('is a plain greyed row with a reason when there is no repo', () => {
    const items = menu({ repos: [] });
    const use = item(items, 'Use for');
    expect(use.disabled).toBe(true);
    expect(use.title).toBe('Add a repo first');
    expect(use.children?.length ?? 0).toBe(0);
    expect(disabledRows(items)).toEqual(['Use for']);
  });

  it('runs nothing itself: it is a submenu row', () => {
    const v = verbs();
    item(accountItems(acct({ repos }), v, GO), 'Use for').run();
    expect(v.useFor).not.toHaveBeenCalled();
  });
});

describe('sections', () => {
  const noDouble = (es: MenuEntry[]) => es.every((e, i) => e !== 'sep' || (i > 0 && i < es.length - 1 && es[i - 1] !== 'sep'));

  const states: Partial<AccountState>[] = [
    {},
    { default: false },
    { disabled: true },
    { login: 'needs_login' },
    { login: 'needs_login', expired: true, default: false },
    { profile: false, default: false },
    { profile: false, default: false, loginHint: null },
    { problem: 'broken', default: false, loginHint: null, login: 'unknown' },
    { repos: [] },
    { renameBlocked: 'it has sessions' },
    { limitedResets: 'in 2h 5m' },
    { sessionPercent: 93 },
  ];

  it.each(states.map((s, i) => [i, s] as const))('state %#: no rule leads, trails or doubles', (_i, s) => {
    expect(noDouble(menu(s))).toBe(true);
  });

  it('draws a rule between the five sections and none inside one', () => {
    const l = labels(menu({ login: 'needs_login' }));
    const at = (x: string) => l.indexOf(x);
    // act | go | copy | change | away
    expect(l.slice(at('Start a session on it\u2026'), at('Log in again\u2026') + 1)).not.toContain('sep');
    expect(l[at('Log in again\u2026') + 1]).toBe('sep');
    expect(l.slice(at('Show its sessions'), at('Account details\u2026') + 1)).not.toContain('sep');
    expect(l[at('Account details\u2026') + 1]).toBe('sep');
    expect(l[at('Copy sign-in command') + 1]).toBe('sep');
    expect(l.slice(at('Make default'), at('Duplicate\u2026') + 1)).not.toContain('sep');
    expect(l[at('Duplicate\u2026') + 1]).toBe('sep');
    expect(l.filter((x) => x === 'sep')).toHaveLength(4);
  });

  it('puts the wake rows (change) among the change rows, after Switch on, and keeps the go rows in the go place', () => {
    const wake = [...GO, reg('wake_account', 'change', 'Wake up now'), reg('wake_schedule', 'change', 'Wake-up schedule\u2026')];
    const l = labels(menu({}, wake));
    expect(l).toEqual([
      'Start a session on it\u2026',
      'sep',
      'Show its sessions',
      'Account details\u2026',
      'sep',
      'Copy sign-in command',
      'sep',
      'Make default',
      'Use for',
      'Switch on',
      'Wake up now',
      'Wake-up schedule\u2026',
      'Edit\u2026',
      'Rename\u2026',
      'Duplicate\u2026',
      'sep',
      'Remove\u2026',
    ]);
  });

  it('greys out nothing without saying why, in every state', () => {
    for (const s of states) expect(allExplained(menu(s))).toBe(true);
  });
});

describe('accountBanner', () => {
  it('names the first state that holds', () => {
    expect(accountBanner(acct({ problem: 'x', login: 'needs_login', disabled: true }))).toEqual({ heading: 'broken' });
    expect(accountBanner(acct({ login: 'needs_login', disabled: true, limitedResets: 'in 1h 0m' }))).toEqual({ heading: 'logged out', ember: true });
    expect(accountBanner(acct({ disabled: true, limitedResets: 'in 1h 0m' }))).toEqual({ heading: 'switched off' });
    expect(accountBanner(acct({ limitedResets: 'in 1h 0m', sessionPercent: 100 }))).toEqual({ heading: 'limited \u00b7 resets in 1h 0m' });
  });

  it('says how far the 5-hour window is from 90 percent', () => {
    expect(accountBanner(acct({ sessionPercent: 89 }))).toBeNull();
    expect(accountBanner(acct({ sessionPercent: 90 }))).toEqual({ heading: '90% of 5h' });
    expect(accountBanner(acct({ sessionPercent: 100 }))).toEqual({ heading: '100% of 5h' });
    expect(accountBanner(acct({ sessionPercent: null }))).toBeNull();
  });

  it('is plain except for the two that need you', () => {
    expect(accountBanner(acct({ problem: 'x' }))).not.toHaveProperty('ember');
    expect(accountBanner(acct({ disabled: true }))).not.toHaveProperty('ember');
    expect(accountBanner(acct({ login: 'needs_login' }))).toHaveProperty('ember', true);
    expect(accountBanner(acct({ expired: true }))).toHaveProperty('ember', true);
  });

  it('is the first entry of the menu, with a rule before nothing', () => {
    const items = menu({ limitedResets: 'in 2h 5m' });
    expect(labels(items)[0]).toBe('limited \u00b7 resets in 2h 5m');
    expect(items[1]).not.toBe('sep');
  });
});

const words = (clock: string, countdown: string): UsageLabel => ({
  percent: 100,
  step: 3,
  clock,
  countdown,
  start: null,
  iso: null,
  estimated: false,
  compact: '',
  long: '',
  resets: `resets ${clock}`,
  hover: '',
  banner: null,
});

describe('usageWords', () => {
  // Epoch seconds 1e9, as the windows count them.
  const NOW = 1_000_000_000_000;
  const win = (kind: UsageWindow['kind'], percent: number, resetsIn: number | null): UsageWindow => ({
    kind,
    label: null,
    percent,
    resets_at: resetsIn === null ? null : 1_000_000_000 + resetsIn,
    read_at: 1_000_000_000 - 60,
  });
  const usage = (...windows: UsageWindow[]): AccountUsage => ({ account: 'work', harness: 'claude', windows, source: 'statusline', at: NOW, plan: null });

  it('says nothing for an account with no reading', () => {
    expect(usageWords(null, NOW)).toEqual({ limitedResets: null, sessionPercent: null });
    expect(usageWords(usage(), NOW)).toEqual({ limitedResets: null, sessionPercent: null });
  });

  it('says when a full window resets, and how far the 5-hour one is', () => {
    const full = win('session', 100, 3 * 3600 + 12 * 60);
    const u = usage(full, win('weekly', 40, 3 * 86400));
    setLabels([[full, words('9:50 pm', 'in 3 h 12 m')]]);
    expect(usageWords(u, NOW)).toEqual({ limitedResets: '9:50 pm \u00b7 in 3 h 12 m', sessionPercent: 100 });
  });

  it('waits for the last of the full windows', () => {
    const last = win('weekly', 100, 2 * 86400 + 4 * 3600);
    const u = usage(win('session', 100, 3600), last);
    setLabels([[last, words('Wed 2:10 am', 'in 2 d 4 h')]]);
    expect(usageWords(u, NOW).limitedResets).toBe('Wed 2:10 am \u00b7 in 2 d 4 h');
  });

  it('is not limited below 100, nor once the reset has passed, nor with no reset to name', () => {
    expect(usageWords(usage(win('session', 95, 3600)), NOW)).toEqual({ limitedResets: null, sessionPercent: 95 });
    expect(usageWords(usage(win('session', 100, -60)), NOW)).toEqual({ limitedResets: null, sessionPercent: 0 });
    expect(usageWords(usage(win('session', 100, null)), NOW)).toEqual({ limitedResets: null, sessionPercent: 100 });
  });

  it('takes the 5-hour use from the session window, not the weekly one', () => {
    expect(usageWords(usage(win('weekly', 97, 86400)), NOW)).toEqual({ limitedResets: null, sessionPercent: null });
  });
});
