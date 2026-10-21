// The account menu where the window opens it: a right-click on a rail row
// (`account:<key>`) and on a row of the Accounts view, and that view's
// `?`. This half reads the state off the window and runs the verbs; the
// rows are menus/account.ts's. Registered as the `account` row builder
// when menus/targets.ts imports this file.

import type { AccountRow } from '../api';
import { app, focusQuickPrompt, setScope } from '../store.svelte';
import { nav } from '../shell/history.svelte';
import { accountVerbs, registryRows, setAccountMenu, type VerbTable } from '../actions';
import { accountScope } from '../scope';
import { accountItems, usageWords, type AccountState, type AccountVerbs } from '../menus/account';
import { setRowBuilder, type Opened } from '../menus/router';
import {
  copySignIn,
  duplicateAccount,
  openAccounts,
  removeAccount,
  renameAccount,
  renameBlocked,
  runLogin,
  setAccountEnabled,
  setAccountReserved,
  setDefaultAccount,
  setProjectAccount,
} from './actions';
import { openAddAccount } from './add.svelte';
import { editRequest } from './edit.svelte';
import { withReserveRow } from './reserve';
import { liveUsage, mergeRows, ownLoginSessionHarnesses, sessionsOn } from './usage';
import { projectAccount } from './view';

/** The rows the rail and the Accounts view draw: the listed ones, and a
 *  harness's own login a reading or a session brought in since. */
function accountRows(): AccountRow[] {
  return mergeRows(app.accounts, app.model.accounts ?? {}, ownLoginSessionHarnesses(app.model.agents));
}

function stateOf(r: AccountRow): AccountState {
  return {
    name: r.name,
    harness: r.harness,
    profile: r.profile,
    default: r.default,
    disabled: r.disabled,
    problem: r.problem || null,
    login: r.login,
    expired: !!r.expired,
    loginHint: r.loginHint,
    named: r.name !== '(no name)',
    renameBlocked: r.profile ? renameBlocked(r) : null,
    repos: app.model.projects.map((p) => ({ id: p.id, name: p.name, now: projectAccount(app.launch, p.repo_path, r.harness) })),
    ...usageWords(liveUsage(r, app.model.accounts ?? {}), app.now),
  };
}

function verbsOf(r: AccountRow, edit: () => void): AccountVerbs {
  return {
    start: () => focusQuickPrompt({ harness: r.harness, profile: r.profile ? r.name : undefined }),
    login: () => void runLogin(r),
    adopt: () => openAddAccount({ harness: r.harness, adopt: true }),
    copyLogin: () => copySignIn(r),
    setDefault: () => void setDefaultAccount(r),
    useFor: (repo, clear) => void setProjectAccount(repo, r.harness, clear ? null : r.name),
    setEnabled: () => void setAccountEnabled(r),
    edit,
    rename: () => void renameAccount(r),
    duplicate: () => void duplicateAccount(r),
    remove: () => void removeAccount(r),
  };
}

/** The registry's account verbs the window can run on `r` now: lane/wake's
 *  Wake up now and Wake-up schedule (a Claude account's), which the
 *  matrix places after Switch on, then Show its sessions and Account
 *  details. */
function tableOf(r: AccountRow): VerbTable {
  const t: VerbTable = { ...accountVerbs(r.key) };
  if (sessionsOn(r, app.model.agents).length) t.scope_account = { run: () => setScope(accountScope(app.model.scope ?? {}, r.key)) };
  if (nav.current.view !== 'accounts') t.accounts = { run: openAccounts };
  return t;
}

/** The menu of the account with key `key`; null when the window has no
 *  such row. `extra.edit`: what Edit does where the editor is at hand (the
 *  Accounts view opens it in the row); by default it asks the Accounts
 *  view for it. */
export function accountOpened(key: string, extra: { edit?: () => void } = {}): Opened | null {
  const r = accountRows().find((x) => x.key === key);
  if (!r) return null;
  const edit =
    extra.edit ??
    (() => {
      editRequest.key = r.key;
      openAccounts();
    });
  // The reserve row is this half's: menus/account.ts's rows do not know the flag yet.
  const items = withReserveRow(accountItems(stateOf(r), verbsOf(r, edit), registryRows('account', tableOf(r))), r, () => void setAccountReserved(r));
  return { items, label: r.name };
}

setRowBuilder('account', (key) => accountOpened(key));
// `menuFor({ kind: 'account' })` (`?` and the palette on a focused account
// row) is this menu too.
setAccountMenu((key) => accountOpened(key)?.items ?? []);
