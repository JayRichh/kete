// The account verbs the window offers outside the Accounts view too: move
// a session to another account (its menu, its header chip), a repo's
// default account (its menu), running a login in a terminal, and the
// account's own verbs (default, switch on or off, duplicate, rename,
// remove, copy the sign-in command) that the rail's menu and the Accounts
// view's rows and buttons share. The words and choices come from view.ts;
// this half asks, writes and says what happened.

import { api, type AccountRow, type Agent } from '../api';
import { app, openSession, refreshAccounts, refreshLaunchables, sessionWorktree, toast } from '../store.svelte';
import { nav } from '../shell/history.svelte';
import { ask, type MenuEntry } from '../ui/ui.svelte';
import { keysOf, withKey } from '../shortcuts';
import { copyText } from '../menus/clipboard';
import { sessionHarness } from './usage';
import { removeWarning } from './pick';
import { setReserved, type ReservableRow } from './reserve';
import {
  canSwitch,
  nameProblem,
  projectsDefaultingTo,
  renameBlocker,
  sessionsFollow,
  suggestName,
  switchConfirm,
  switchTargets,
  type SwitchTarget,
} from './view';
import { watchSignIn } from './add.svelte';

/** Why the move is not offered, for a disabled control's tooltip. */
export const SWITCH_UNAVAILABLE = 'The running kete daemon is older than this window: restart it (kete kill, then reopen) to move sessions between accounts.';

/** The Accounts view. */
export function openAccounts() {
  nav.go({ view: 'accounts' });
}

const agentOf = (id: string) => app.model.agents.find((a) => a.id === id);

/** Confirm, then restart `agent` fresh on `to`. */
export async function confirmSwitch(agent: Agent, to: SwitchTarget) {
  const c = switchConfirm(agent, to);
  const r = await ask.confirm(c);
  if (!r.ok) return;
  try {
    await api.accountSwitch(agent.id, to.profile);
    toast(`${agent.name} restarted on ${to.profile ?? `${sessionHarness(agent)}'s own login`}.`);
    refreshAccounts();
  } catch (e) {
    toast(`Could not move it: ${e}`);
  }
}

/** The move's choices as menu items (the header chip's menu). */
export function accountMenu(id: string): MenuEntry[] {
  const a = agentOf(id);
  if (!a) return [];
  const head: MenuEntry = { heading: `${sessionHarness(a)} \u00b7 runs on ${a.profile?.trim() || 'its own login'}` };
  if (!canSwitch(app.model)) return [head, { label: 'Move to account\u2026 (daemon too old)', run: () => toast(SWITCH_UNAVAILABLE), disabled: true }];
  const targets = switchTargets(a, app.accounts);
  const items: MenuEntry[] = targets.map((t) => ({ label: `Restart on ${t.label}\u2026`, run: () => confirmSwitch(a, t) }));
  if (!items.length) items.push({ label: `No other ${sessionHarness(a)} account: add one`, run: openAccounts });
  return [head, ...items, 'sep', { label: 'Accounts\u2026', run: openAccounts, keys: keysOf('accounts') }];
}

/** "Move to account...": pick where, then confirm. */
export async function moveToAccount(id: string) {
  const a = agentOf(id);
  if (!a) return;
  if (!canSwitch(app.model)) return toast(SWITCH_UNAVAILABLE);
  if (!app.accounts.length) await api.accounts().then((r) => (app.accounts = r)).catch(() => {});
  const targets = switchTargets(a, app.accounts);
  if (!targets.length) return toast(`There is no other ${sessionHarness(a)} account. Add one in ${withKey('Accounts', 'accounts')}.`);
  const own = '\u0000own';
  const pick = await ask.pick({
    title: `Move "${a.name}" to (restarts fresh)`,
    items: targets.map((t) => ({
      id: t.profile ?? own,
      label: t.label,
      hint: t.off ? 'off' : t.needsLogin ? 'needs login' : undefined,
    })),
  });
  if (pick == null) return;
  const to = targets.find((t) => (t.profile ?? own) === pick);
  const now = agentOf(id);
  if (to && now) await confirmSwitch(now, to);
}

/** Write a project's default account for a harness (null clears). */
export async function setProjectAccount(project: string, harness: string, profile: string | null) {
  const p = app.model.projects.find((x) => x.id === project);
  try {
    await api.projectSetAccount(project, harness, profile);
    toast(
      profile
        ? `${harness} sessions started in ${p?.name ?? 'the repo'} without a pick now run on ${profile}.`
        : `${p?.name ?? 'The repo'} has no default ${harness} account now: the account spread picks.`,
    );
    refreshLaunchables();
  } catch (e) {
    toast(`Could not save it: ${e}`);
  }
}

/** The worktree a login terminal opens in: the open session's, else the
 *  first repo's main checkout. */
export function loginWorktree(): string | null {
  const open = sessionWorktree(app.session);
  if (open) return open;
  const first = app.model.projects[0]?.id;
  const wts = app.model.worktrees.filter((w) => w.project_id === first);
  return (wts.find((w) => w.is_main) ?? wts[0])?.id ?? null;
}

/** Run an account's login command in a new terminal and show it; for a
 *  profile, the docked panel then watches the row until it signs in. */
export async function runLogin(r: Pick<AccountRow, 'key' | 'name' | 'profile'>): Promise<boolean> {
  const wt = loginWorktree();
  if (!wt) {
    toast('Add a repo first: the login runs in a terminal in one. Or copy the command.');
    return false;
  }
  try {
    const terminal = await api.accountLoginRun(wt, r.key);
    openSession({ Terminal: terminal });
    if (r.profile) watchSignIn(r.name);
    else toast(`Signing ${r.name} in: finish in the terminal, then the row updates.`);
    return true;
  } catch (e) {
    toast(String(e));
    return false;
  }
}

// ---- an account's own verbs ----

/** Make a profile its harness's default; nothing when it already is. */
export async function setDefaultAccount(r: AccountRow) {
  if (r.default) return;
  try {
    await api.profileSetDefault(r.name);
    refreshLaunchables();
  } catch (e) {
    toast(String(e));
  }
}

/** Switch a profile on when it is off, off when it is on. */
export async function setAccountEnabled(r: AccountRow) {
  try {
    await api.profileSetEnabled(r.name, r.disabled);
    toast(r.disabled ? `${r.name} is on.` : `${r.name} is off: launches that pick no account skip it.`);
    refreshLaunchables();
  } catch (e) {
    toast(String(e));
  }
}

/** Reserve a profile for the person's own sessions, or give it back. */
export async function setAccountReserved(r: ReservableRow) {
  try {
    await setReserved(r.name, !r.reserved);
    toast(r.reserved ? `${r.name} is no longer reserved.` : `${r.name} is reserved: the orchestrator and scripts never pick it.`);
    refreshLaunchables();
  } catch (e) {
    toast(String(e));
  }
}

/** A new profile like `r`, in a home of its own, then offer to sign it in. */
export async function duplicateAccount(r: AccountRow) {
  const taken = app.accounts.filter((x) => x.profile).map((x) => x.name);
  const v = await ask.prompt({
    title: `Duplicate ${r.name}`,
    note: `A new ${r.harness} account with the same variables and kind of login, in a home of its own: it signs in separately.`,
    fields: [{ key: 'name', label: 'Name', value: suggestName(r.name, taken), mono: true }],
    confirm: 'Duplicate',
  });
  if (!v) return;
  const why = nameProblem(v.name, taken);
  if (why) return toast(why);
  try {
    const out = await api.profileDuplicate(r.name, v.name.trim());
    refreshLaunchables();
    if (out.login) {
      const go = await ask.confirm({
        title: `Sign ${v.name.trim()} in`,
        message: `${v.name.trim()} is added. Sign it in now? The login runs in a new terminal.`,
        detail: [out.login],
        confirm: 'Sign in',
      });
      if (go.ok) {
        const rows = await api.accounts();
        app.accounts = rows;
        const row = rows.find((x) => x.profile && x.name === v.name.trim());
        if (row) await runLogin(row);
      }
    } else toast(`Added ${v.name.trim()}.`);
  } catch (e) {
    toast(String(e));
  }
}

/** Remove a profile, its home kept or deleted as the confirm says. */
export async function removeAccount(r: AccountRow) {
  const projects = projectsDefaultingTo(app.launch, app.model.projects, r.harness, r.name);
  const detail = [
    ...(r.home ? [`Its home is ${r.home}: it stays unless you tick the box below.`] : []),
    ...(projects.length ? [`Default account of ${projects.map((p) => p.name).join(', ')}: those fall back to the spread.`] : []),
  ];
  const ok = await ask.confirm({
    title: 'Remove account',
    message: removeWarning(r.name, app.model.agents),
    detail: detail.length ? detail : undefined,
    confirm: 'Remove',
    danger: true,
    // The purge `kete profile remove --purge` does: the login goes too.
    check: r.home ? `Also delete ${r.home} and the login in it` : undefined,
  });
  if (!ok.ok) return;
  try {
    const out = await api.profileRemove(r.name, !!r.home && ok.checked);
    toast(
      out.purged ? `Removed ${r.name} and deleted ${out.home}.` : out.home ? `Removed ${r.name}. Its home stays at ${out.home}.` : `Removed ${r.name}.`,
    );
    refreshLaunchables();
  } catch (e) {
    toast(String(e));
  }
}

export const RENAME_OFFLINE = 'Not connected to the kete daemon: a rename checks the sessions on the account first.';

/** Why `r` cannot be renamed now, or null: a daemon that does not move
 *  sessions with a rename refuses while any names the profile; offline,
 *  nothing can say. The rename's tooltip and its menu row's reason. */
export function renameBlocked(r: Pick<AccountRow, 'name'>): string | null {
  return app.model.connected ? renameBlocker(r.name, app.model.agents, sessionsFollow(app.model)) : RENAME_OFFLINE;
}

/** Rename: refused while a session names it; the home, the repos it is
 *  the default of and its reading move with the name. */
export async function renameAccount(r: AccountRow) {
  const blocked = renameBlocked(r);
  if (blocked) return toast(blocked);
  const taken = app.accounts.filter((x) => x.profile && x.key !== r.key).map((x) => x.name);
  const v = await ask.prompt({
    title: `Rename ${r.name}`,
    note: r.home
      ? `Its home moves from ${r.home} to the new name; repos that start on it follow.`
      : `Repos that start on it follow the new name.`,
    fields: [{ key: 'name', label: 'New name', value: r.name, mono: true }],
    confirm: 'Rename',
  });
  if (!v || v.name.trim() === r.name) return;
  const why = nameProblem(v.name, taken);
  if (why) return toast(why);
  try {
    const out = await api.profileRename(r.name, v.name.trim());
    const repos = out.projects.length ? `, the default of ${out.projects.length} ${out.projects.length === 1 ? 'repo' : 'repos'}` : '';
    const archived = out.archived.length ? ` ${out.archived.length} archived ${out.archived.length === 1 ? 'session still names' : 'sessions still name'} ${r.name}.` : '';
    const followed = out.followed
      ? ` ${out.followed} ${out.followed === 1 ? 'session follows' : 'sessions follow'} (a running one on its next restart).`
      : '';
    toast(`Renamed ${r.name} to ${out.name}${repos}.${archived}${followed}`);
    refreshLaunchables();
    refreshAccounts();
  } catch (e) {
    toast(String(e));
  }
}

/** Put an account's sign-in command on the clipboard. */
export function copySignIn(r: Pick<AccountRow, 'loginHint'>) {
  if (r.loginHint) void copyText(r.loginHint, `${r.loginHint}. Run it in a terminal to sign in.`);
}
