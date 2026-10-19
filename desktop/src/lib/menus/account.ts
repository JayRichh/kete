// The account menu (the menu matrix, \u00a75.9), for every place an account
// shows: the rail's list, the Accounts view's rows and its `?`. Pure, as
// rows.ts: what shows and what is greyed comes from a state the caller
// reads off the window (accounts/account-menu.ts), what a row does from
// the verbs it hands in, so the tests build both.
//
// The sections run act, go, copy, change, away. An account menu never
// folds: Use for is a submenu of its own, and the rest is under a dozen
// rows. The registry's rows (Show its sessions, Account details) arrive
// built, from the verb table, and take the go place.

import type { MenuEntry, MenuHeading, MenuItem } from '../ui/ui.svelte';
import type { MenuSection } from '../actions.gen';
import { layout, type Row } from './layout';
import { submenu } from './model';
import { labelFor } from '../accounts/labels.svelte';
import { shownAt } from '../accounts/usage';
import type { AccountUsage, UsageWindow } from '../api';

/** An account as its menu needs it. `profile`: a named profile (false: a
 *  harness's own login). `repos`: every repo, with the account that is its
 *  default for this harness now (null: none). `renameBlocked`: why a
 *  rename can't go ahead now, or null. `limitedResets` and
 *  `sessionPercent` are `usageWords`. */
export interface AccountState {
  name: string;
  harness: string;
  profile: boolean;
  default: boolean;
  disabled: boolean;
  problem: string | null;
  login: 'logged_in' | 'needs_login' | 'unknown';
  expired: boolean;
  loginHint: string | null;
  /** The profile has a name (a broken entry may not): Rename needs one. */
  named: boolean;
  renameBlocked: string | null;
  repos: { id: string; name: string; now: string | null }[];
  /** A window at 100% that resets: when, in local time, as the shared
 *  formatter words it (`9:50 pm - in 3 h 12 m`). */
  limitedResets: string | null;
  /** The session (5-hour) window's use now, whole percent. */
  sessionPercent: number | null;
}

export interface AccountVerbs {
  /** Start a session on it (the quick prompt, on this account). */
  start(): void;
  /** Run its login command in a terminal. */
  login(): void;
  /** An own login: add it as a named account. */
  adopt(): void;
  copyLogin(): void;
  setDefault(): void;
  /** Make the account a repo's default for its harness; `clear`: it
   *  already is, so clear that. */
  useFor(repo: string, clear: boolean): void;
  /** Switch it on, or off when it is on. */
  setEnabled(): void;
  edit(): void;
  rename(): void;
  duplicate(): void;
  remove(): void;
}

const row = (id: string, section: MenuSection | null, item: MenuItem): Row => ({ id, section, item });

/** A greyed row, its reason as its tooltip. */
const off = (item: MenuItem, why: string | false | null | undefined): MenuItem => (why ? { ...item, disabled: true, title: why } : item);

const nothing = () => {};

/** The 5-hour use from which the banner says so. */
const NEAR_AT = 90;

/** What the usage says that the banner needs: when the account's full
 *  windows reset (the latest of them: it runs again once all have), and
 *  how far the 5-hour window is. A window full with no reset time is not
 *  "limited": nothing says when. */
export function usageWords(u: AccountUsage | null | undefined, nowMs: number): Pick<AccountState, 'limitedResets' | 'sessionPercent'> {
  let latest: UsageWindow | null = null;
  let session: number | null = null;
  for (const w of u?.windows ?? []) {
    const p = shownAt(w, nowMs);
    if (p === null) continue;
    if (w.kind === 'session') session = Math.round(p);
    if (p >= 100 && w.resets_at != null && w.resets_at * 1000 > nowMs) if (!latest || w.resets_at > (latest.resets_at ?? 0)) latest = w;
  }
  const l = labelFor(latest);
  const when = l?.clock ? `${l.clock} \u00b7 ${l.countdown}` : (l?.countdown ?? 'soon');
  return { limitedResets: latest ? when : null, sessionPercent: session };
}

/** The line the menu opens with, when the account is in a state worth
 *  one: the first that holds. A login that needs doing is ember (it needs
 *  you). */
export function accountBanner(s: AccountState): MenuHeading | null {
  if (s.problem) return { heading: 'broken' };
  if (s.login === 'needs_login' || s.expired) return { heading: s.expired ? 'login expired' : 'logged out', ember: true };
  if (s.disabled) return { heading: 'switched off' };
  if (s.limitedResets) return { heading: `limited \u00b7 resets ${s.limitedResets}` };
  if (s.sessionPercent !== null && s.sessionPercent >= NEAR_AT) return { heading: `${s.sessionPercent}% of 5h` };
  return null;
}

/** Use for's rows: one per repo, ticked where the account `name` is that
 *  repo's default for its harness (picking a ticked one clears it); where
 *  another account is, its name as the row's tooltip. Also the Accounts
 *  view's "Use for..." button's menu. */
export function useForItems(name: string, repos: AccountState['repos'], useFor: AccountVerbs['useFor']): MenuItem[] {
  return repos.map((p): MenuItem => {
    const mine = p.now === name;
    return {
      label: p.name,
      run: () => useFor(p.id, mine),
      checked: mine,
      ...(mine ? { title: 'Clear' } : p.now ? { title: `Now ${p.now}` } : {}),
    };
  });
}

/** The account's menu. `registry`: the verb table's rows for an account
 *  (Show its sessions, Account details...). Those in the go section stand
 *  after Start and Log in; any other (Wake up now: change) goes among the
 *  change rows, after Switch on. */
export function accountItems(s: AccountState, v: AccountVerbs, registry: Row[]): MenuEntry[] {
  // A named profile that works: the rows that change it.
  const usable = s.profile && !s.problem;
  const relogin = !!s.loginHint && !s.problem && (s.login === 'needs_login' || s.expired);
  const rows: Row[] = [];

  // act
  if (usable && !s.disabled) rows.push(row('account.start', 'act', { label: 'Start a session on it\u2026', run: v.start }));
  if (relogin) rows.push(row('account.login', 'act', { label: 'Log in again\u2026', run: v.login, icon: { dot: 'ember' }, title: s.loginHint ?? undefined }));
  if (!s.profile)
    rows.push(
      row('account.adopt', 'act', {
        label: 'Add as account',
        run: v.adopt,
        title: 'Add this login as a named account, so it can be starred, switched off or used for a repo',
      }),
    );

  // go
  rows.push(...registry.filter((r) => r.section === 'go'));

  // copy
  if (s.loginHint) rows.push(row('account.copy_login', 'copy', { label: 'Copy sign-in command', run: v.copyLogin, title: s.loginHint }));

  // change
  if (usable) {
    rows.push(row('account.default', 'change', { label: 'Make default', run: s.default ? nothing : v.setDefault, checked: s.default }));
    rows.push(
      row(
        'account.use_for',
        'change',
        s.repos.length
          ? submenu('Use for', useForItems(s.name, s.repos, v.useFor), {
              title: `Make ${s.name} the ${s.harness} account a repo starts sessions on`,
            })
          : off({ label: 'Use for', run: nothing }, 'Add a repo first'),
      ),
    );
    rows.push(row('account.enabled', 'change', { label: 'Switch on', run: v.setEnabled, checked: !s.disabled }));
  }
  rows.push(...registry.filter((r) => r.section !== 'go'));
  if (s.profile) rows.push(row('account.edit', 'change', off({ label: 'Edit\u2026', run: v.edit }, s.problem && 'Fix this entry in config.json, or rename it')));
  if (s.profile && s.named) rows.push(row('account.rename', 'change', off({ label: 'Rename\u2026', run: v.rename, keys: ['r'] }, s.renameBlocked)));
  if (usable) rows.push(row('account.duplicate', 'change', { label: 'Duplicate\u2026', run: v.duplicate }));

  // away
  if (s.profile) rows.push(row('account.remove', 'away', { label: 'Remove\u2026', run: v.remove, keys: ['delete'], danger: true }));

  const banner = accountBanner(s);
  return layout(rows, { fold: false, head: banner ? [banner] : [] });
}
