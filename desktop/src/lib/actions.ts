// What can be done to a session, a terminal, a worktree or a repo: one
// list per target, used by the context menus (overview rows, board cards,
// the session header, repo headers), the row keys and the command
// palette. Destructive verbs confirm in the window first, as the terminal
// app's do: delete always, archive when Confirm on archive is on or the
// session has sessions nested under it (they go with it).

import { api, type AccountRow, type Agent, type Model, type StopNote, type TerminalTab, type Worktree } from './api';
import { stopLabel, stopTone } from './stop';
import { app, focusQuickPrompt, markSeen, openDiff, openSession, projectOf, revealRepo, setScope, toast } from './store.svelte';
import { ALT_CHOICE, ask, openMenu, type MenuEntry, type MenuHeading, type MenuIcon, type MenuItem } from './ui/ui.svelte';
import type { ShortcutId } from './shortcuts';
import { makeOrchestrator, openReview, unpinOrchestrator } from './orch.svelte';
import { isOrchestrator, orchestratorOf } from './orchestrator';
import type { ActionId, AppliesTo, MenuSection } from './actions.gen';
import { destructive, groupOf as menuGroupOf, menuIds, menuLabel, sectionOf, verbKeys } from './verbs';
import { worktreeApi } from './worktrees/api';
import { cardsLine, dirty, stateLines } from './worktrees/offer';
import { deleteEmptied, offerConfirm, prepareOffer, stillOffered } from './worktrees/delete';
import { canSwitchBranch, openBranchSwitch } from './worktrees/switcher.svelte';
import { checkoutSessions, deleteAllSessions } from './checkout';
import { moveToAccount, runLogin, setProjectAccount, SWITCH_UNAVAILABLE } from './accounts/actions';
import { canSwitch, projectAccount } from './accounts/view';
import { accountKey, sessionHarness } from './accounts/usage';
import { openWake, wake, wakeNowFor } from './wake/state.svelte';
import { rows as gitRows, setCollapsed, whileCutting } from './worktrees/rows.svelte';
import { confirmMove, moveStep, stepInFor, type StepIn } from './reorder.svelte';
import { openComposer, openMerge } from './ship/sheet.svelte';
import { isOpen, reviewVerbs } from './ship/ship';
import { dropPart, repoScope, worktreeScope } from './scope';
import { groupOf, repoKey } from './worktrees/groups';
import { nav } from './shell/history.svelte';
import { toneOf } from './status';
import { copyText } from './menus/clipboard';
import { submenu } from './menus/model';
import { insertRows, layout, withoutIds, type Row } from './menus/layout';
import { checkoutVerbs, factsFor, openCommandRow, touch } from './editors/verbs';

export type Target =
  /** `viaSub`: picked from one of its subagents' rows, so it acts on
   *  this, the parent session, and says so. */
  | { kind: 'agent'; id: string; viaSub?: boolean }
  | { kind: 'terminal'; id: string }
  | { kind: 'worktree'; id: string }
  | { kind: 'project'; id: string }
  /** An ACCOUNT, by its key: a profile's name, or `@claude`. */
  | { kind: 'account'; id: string };

/** A target as a string, for `data-session` attributes. */
export function targetKey(t: Target): string {
  return `${t.kind === 'agent' && t.viaSub ? 'subagent' : t.kind}:${t.id}`;
}
export function parseTarget(k: string | null | undefined): Target | null {
  if (!k) return null;
  const i = k.indexOf(':');
  const kind = k.slice(0, i);
  const id = k.slice(i + 1);
  if (kind === 'agent' || kind === 'terminal' || kind === 'worktree' || kind === 'project' || kind === 'account') return { kind, id };
  if (kind === 'subagent') return { kind: 'agent', id, viaSub: true };
  return null;
}

/** The nearest row around `el` whose target the window's keys know (a
 *  session, terminal, worktree or repo), past a chip's own (`pr:`,
 *  `port:`, `account:`), which only its menu reads. */
export function closestTarget(el: Element | null | undefined): Target | null {
  for (let at = el?.closest<HTMLElement>('[data-target]'); at; at = at.parentElement?.closest<HTMLElement>('[data-target]') ?? null) {
    const t = parseTarget(at.dataset.target);
    if (t) return t;
  }
  return null;
}

const m = (): Model => app.model;
const agentOf = (id: string) => m().agents.find((a) => a.id === id);
const terminalOf = (id: string) => m().terminals.find((t) => t.id === id);
const worktreeOf = (id: string | undefined) => m().worktrees.find((w) => w.id === id);

/** Every session started (`kete spawn`) under `id`, at any depth. */
export function descendants(model: Model, id: string): Agent[] {
  const out: Agent[] = [];
  const walk = (p: string) => {
    for (const a of model.agents) {
      if (a.parent === p && a.id !== id && !out.includes(a)) {
        out.push(a);
        walk(a.id);
      }
    }
  };
  walk(id);
  return out;
}

async function attempt(what: string, f: () => Promise<unknown>) {
  try {
    await f();
  } catch (e) {
    toast(`${what}: ${e}`);
  }
}

// ---- session verbs ----

export async function renameAgent(id: string) {
  const a = agentOf(id);
  if (!a) return;
  const v = await ask.prompt({ title: 'Rename session', fields: [{ key: 'name', label: 'Name', value: a.name }], confirm: 'Rename' });
  if (v && v.name.trim() && v.name.trim() !== a.name) await attempt('Could not rename', () => api.renameAgent(id, v.name));
}

/** Archive a session and the thread nested under it. `viaSub`: asked
 *  from a subagent's row, which acts on its parent, so it always confirms
 *  and says it is the parent going. */
export async function archiveAgent(id: string, viaSub = false): Promise<boolean> {
  const a = agentOf(id);
  if (!a || a.archived) return false;
  const kids = descendants(m(), id).filter((k) => !k.archived);
  if (viaSub || kids.length || app.launch.ask_before_archive !== false) {
    const r = await ask.confirm({
      title: kids.length ? `Archive thread \u00b7 ${kids.length + 1} sessions` : 'Archive session',
      message:
        (viaSub ? `This archives the parent session, not the subagent. ` : '') +
        (kids.length
          ? `Archive "${a.name}" and everything nested under it? U brings them back.`
          : `Archive "${a.name}"? Its agent stops; U brings it back.`),
      detail: kids.length ? [a.name, ...kids.map((k) => k.name)] : undefined,
      confirm: 'Archive',
    });
    if (!r.ok) return false;
  }
  // The model may have moved while the dialog was up: act on it as it is now.
  const now = agentOf(id);
  if (!now || now.archived) return false;
  const thread = descendants(m(), id).filter((k) => !k.archived);
  let done = false;
  await attempt('Could not archive', async () => {
    for (const x of [now, ...thread]) await api.archiveAgent(x.id);
    done = true;
  });
  return done;
}

/** Until the model shows `ok`, at most `ms`; whether it did. */
async function until(ok: () => boolean, ms = 3000): Promise<boolean> {
  for (let waited = 0; !ok() && waited < ms; waited += 50) await new Promise((r) => setTimeout(r, 50));
  return ok();
}

/** A Done card let go on Archived (core's `ArchiveAndOfferWorktreeDelete`):
 *  archive it, then, when that leaves its linked worktree with nothing live,
 *  offer the worktree's safe delete (worktrees/delete.ts), as a session
 *  delete does. Uncommitted work there needs its box ticked. */
export async function archiveAndOfferWorktree(id: string) {
  const a = agentOf(id);
  if (!a || a.archived) return;
  const { worktree_id: wt, name } = a;
  if (!(await archiveAgent(id))) return;
  // The offer counts live cards off the model: wait for the archive to land.
  if (!(await until(() => !!agentOf(id)?.archived))) return;
  const p = await prepareOffer(wt, 0, []);
  if (!p) return;
  const o = offerConfirm(p);
  if (!o.check) {
    // Delete emptied worktree is on and the checkout is clean: no question.
    toast(o.line);
    await attempt('Could not delete the worktree', async () => deleteEmptied(stillOffered(p, false, 0, [])));
    return;
  }
  const risky = !o.checked;
  const r = await ask.confirm({
    title: 'Delete its worktree too?',
    message: `Archived "${name}". ${o.line}`,
    detail: o.detail,
    confirm: 'Delete worktree',
    danger: true,
    ...(risky ? { check: o.check, checked: false } : {}),
  });
  if (!r.ok || (risky && !r.checked)) return;
  await attempt('Could not delete the worktree', async () => deleteEmptied(stillOffered(p, true, 0, [])));
}

/** Bring back an archived session and the thread archived with it. */
export async function unarchiveAgent(id: string) {
  const a = agentOf(id);
  if (!a?.archived) return;
  const thread = descendants(m(), id).filter((k) => k.archived);
  await attempt('Could not unarchive', async () => {
    for (const x of [a, ...thread]) await api.unarchiveAgent(x.id);
  });
}

export async function deleteAgent(id: string) {
  const a = agentOf(id);
  if (!a) return;
  const kids = descendants(m(), id);
  // Emptying a linked worktree offers it too (worktrees/delete.ts).
  const taken = (xs: Agent[]) => xs.filter((x) => x.worktree_id === a.worktree_id && !x.archived).length;
  const doomed = [a, ...kids].map((x) => x.id);
  const p = await prepareOffer(a.worktree_id, taken([a, ...kids]), doomed);
  const o = p && offerConfirm(p);
  const r = await ask.confirm({
    title: kids.length ? `Delete thread \u00b7 ${kids.length + 1} sessions` : 'Delete session',
    message:
      (kids.length
        ? `Delete "${a.name}" and everything nested under it? Their history goes too.`
        : `Delete "${a.name}"? Its history goes too; this can't be undone.`) + (o ? ` ${o.line}` : ''),
    detail: kids.length || o ? [...(kids.length ? [a.name, ...kids.map((k) => k.name)] : []), ...(o?.detail ?? [])] : undefined,
    confirm: 'Delete',
    danger: true,
    check: o?.check,
    checked: o?.checked,
  });
  if (!r.ok) return;
  // Sessions spawned while the dialog was up go too: recount now.
  const now = agentOf(id);
  if (!now) return;
  const gone = descendants(m(), id);
  const wt = stillOffered(p, r.checked, taken([now, ...gone]), [now, ...gone].map((x) => x.id));
  if (app.session && 'Agent' in app.session && [now, ...gone].some((x) => x.id === (app.session as { Agent: string }).Agent))
    app.session = null;
  let done = false;
  await attempt('Could not delete', async () => {
    for (const x of [...gone].reverse().concat(now)) await api.deleteAgent(x.id);
    done = true;
  });
  if (done) await attempt('Could not delete the worktree', async () => deleteEmptied(wt));
}

export async function restartAgent(id: string) {
  const a = agentOf(id);
  if (!a || a.archived) return;
  await attempt('Could not restart', async () => {
    await api.restartAgent(id);
    toast(`Restarting ${a.name}.`);
  });
}

export async function moveAgent(id: string) {
  const a = agentOf(id);
  if (!a) return;
  const name = (w: Worktree) => m().projects.find((p) => p.id === w.project_id)?.name ?? '';
  const items = m()
    .worktrees.filter((w) => w.id !== a.worktree_id)
    .map((w) => ({ id: w.id, label: `${name(w)} / ${w.branch}`, hint: w.is_main ? 'main checkout' : undefined }));
  if (!items.length) return toast('There is no other worktree to move it to. Make one first.');
  const to = await ask.pick({ title: `Move "${a.name}" to`, items });
  const target = to ? m().worktrees.find((w) => w.id === to) : undefined;
  if (!to || !target || !(await confirmMove(a, target))) return;
  await attempt('Could not move', () => api.moveAgent(id, to));
}

export function duplicateAgent(id: string) {
  const a = agentOf(id);
  if (!a) return;
  focusQuickPrompt({
    project: projectOf(a.worktree_id),
    worktree: a.worktree_id,
    harness: a.custom_harness ?? a.kind,
    profile: a.profile ?? undefined,
    model: a.model,
    effort: a.effort ?? null,
    newWorktree: false,
  });
}

export async function followUp(id: string) {
  const a = agentOf(id);
  if (!a) return;
  const v = await ask.prompt({
    title: `Follow-up to ${a.name}`,
    note: a.alive
      ? 'Start as new session runs it beside this one, in the same checkout.'
      : 'This session is not running: restart it first, or start the follow-up as a new session beside it.',
    fields: [{ key: 'text', label: 'Message', multiline: true, placeholder: 'What next?' }],
    confirm: 'Send',
    alt: { label: 'Start as new session' },
  });
  if (!v?.text.trim()) return;
  if (v[ALT_CHOICE]) await startFollowUpSession(id, v.text);
  else await attempt('Could not send', () => api.followUp(id, v.text));
}

/** The follow-up as a session of its own, nested under `id`, opened. */
export async function startFollowUpSession(id: string, text: string) {
  await attempt('Could not start the session', async () => openSession({ Agent: await api.spawnFollowUp(id, text) }));
}

/** ASK ASIDE: a side question about the session, answered by a session of
 *  its own beside it that leaves the checkout alone (the desktop app's
 *  /btw in its own tab). */
export async function askAside(id: string) {
  const a = agentOf(id);
  if (!a) return;
  const v = await ask.prompt({
    title: `Ask aside about ${a.name}`,
    note: 'A new session in the same checkout answers it without changing files. This one keeps working.',
    fields: [{ key: 'question', label: 'Question', multiline: true, placeholder: 'What do you want to know?' }],
    confirm: 'Ask aside',
  });
  if (!v?.question.trim()) return;
  await attempt('Could not ask aside', async () => openSession({ Agent: await api.askAside(id, v.question) }));
}

export function seeAgent(id: string) {
  markSeen({ Agent: id });
}

// ---- terminals ----

export async function renameTerminal(id: string) {
  const t = terminalOf(id);
  if (!t) return;
  const v = await ask.prompt({ title: 'Rename terminal', fields: [{ key: 'name', label: 'Name', value: t.name }], confirm: 'Rename' });
  if (v && v.name.trim() && v.name.trim() !== t.name) await attempt('Could not rename', () => api.renameTerminal(id, v.name));
}

export async function closeTerminal(id: string) {
  const t = terminalOf(id);
  if (!t) return;
  // Its worktree's last card: the confirm offers the worktree too.
  const p = await prepareOffer(t.worktree_id, 1, [id]);
  const o = p && offerConfirm(p);
  let checked = false;
  if (t.alive || o) {
    const r = await ask.confirm({
      title: 'Close terminal',
      message:
        (t.alive ? `Close "${t.run_command ? 'run' : t.name}"? Its shell and anything running in it are killed.` : `Close "${t.name}"?`) +
        (o ? ` ${o.line}` : ''),
      detail: o?.detail,
      confirm: 'Close',
      danger: true,
      check: o?.check,
      checked: o?.checked,
    });
    if (!r.ok) return;
    checked = r.checked;
  }
  const wt = stillOffered(p, checked, 1, [id]);
  let done = false;
  await attempt('Could not close', async () => {
    await api.closeTerminal(id);
    done = true;
  });
  if (done) await attempt('Could not delete the worktree', async () => deleteEmptied(wt));
}

// ---- worktrees and repos ----

export async function newWorktree(project: string | undefined) {
  if (!project) return toast('Pick a repo first.');
  const p = m().projects.find((x) => x.id === project);
  const suggestion = await api.suggestBranch(project).catch(() => '');
  const v = await ask.prompt({
    title: `New worktree in ${p?.name ?? 'repo'}`,
    note: 'Spaces become hyphens. Leave the branch empty to take the suggested name.',
    fields: [
      { key: 'branch', label: 'Branch', placeholder: suggestion, mono: true },
      { key: 'base', label: 'From (optional: a branch or commit; empty uses the default base)', placeholder: 'origin/main', mono: true },
    ],
    confirm: 'Create',
  });
  if (!v) return;
  toast('Making the worktree\u2026');
  await attempt('Could not make the worktree', async () => {
    const branch = v.branch.trim() || suggestion;
    const id = await whileCutting(project, branch, () => api.createWorktree(project, branch, v.base.trim() || null));
    toast(`Worktree ready: ${worktreeOf(id)?.branch ?? v.branch}`);
    focusQuickPrompt({ project, worktree: id, newWorktree: false });
  });
}

export async function deleteWorktree(id: string) {
  const w = worktreeOf(id);
  if (!w || w.is_main) return;
  const live = m().agents.filter((a) => a.worktree_id === id && !a.archived).length;
  const terms = m().terminals.filter((t) => t.worktree_id === id).length;
  // What a non-forced delete would refuse over, so Force is an informed pick.
  const state = await worktreeApi.checkoutState(id).catch(() => null);
  const cards = cardsLine(live, terms);
  const r = await ask.confirm({
    title: 'Delete worktree',
    message: `Delete the worktree on ${w.branch}? Its folder is removed; the branch stays.`,
    detail: [w.path, ...(cards ? [cards] : []), ...stateLines(state)],
    confirm: 'Delete',
    danger: true,
    check: dirty(state)
      ? 'Force: delete with its uncommitted changes (without this, git refuses)'
      : 'Force: delete even with uncommitted changes',
  });
  if (r.ok) await attempt('Could not delete the worktree', () => api.deleteWorktree(id, r.checked));
}

export async function openEditor(worktree: string | undefined) {
  if (!worktree) return;
  await attempt('Could not open', async () => {
    let r = await api.openWorktree(worktree);
    if (!r.opened) {
      // From the repo's .kete.json, not the user's setting: anyone who can
      // push wrote it. Show it once per repo and command before it runs.
      const ok = await ask.confirm({
        title: 'Run the repo\'s open command?',
        message: 'This command comes from the repo\'s .kete.json, not your settings. Run it? You won\'t be asked again for this repo unless it changes.',
        detail: [r.command],
        confirm: 'Run',
      });
      if (!ok.ok) return;
      const shown = r.command;
      r = await api.openWorktree(worktree, shown);
      if (!r.opened) {
        toast("The repo's open command changed before it ran. Open again to review it.");
        return;
      }
    }
    toast(`Opened with: ${r.command}`);
  });
}

/** Ask every checkout's pull request again now. */
export function refreshPrs() {
  worktreeApi
    .refreshPrs()
    .then(() => toast('Asking for pull requests\u2026'))
    .catch((e) => toast(`Could not refresh: ${e}`));
}

export async function openRemote(worktree: string | undefined) {
  if (!worktree) return;
  await attempt('Could not open the remote', () => api.openRemote(worktree));
}

export async function newTerminal(worktree: string | undefined) {
  if (!worktree) return;
  await attempt('Could not open a terminal', async () => {
    const id = await api.createTerminal(worktree);
    if (id) openSession({ Terminal: id });
  });
}

function mainOf(project: string): Worktree | undefined {
  return m().worktrees.find((w) => w.project_id === project && w.is_main) ?? m().worktrees.find((w) => w.project_id === project);
}

export async function renameProject(id: string) {
  const p = m().projects.find((x) => x.id === id);
  if (!p) return;
  const v = await ask.prompt({
    title: 'Rename repo',
    note: 'Only the row changes; the folder on disk stays. Empty puts the folder name back.',
    fields: [{ key: 'name', label: 'Name', value: p.name }],
    confirm: 'Rename',
  });
  if (v && v.name.trim() !== p.name) await attempt('Could not rename', () => api.renameProject(id, v.name));
}

export async function locateProject(id: string) {
  const p = m().projects.find((x) => x.id === id);
  if (!p) return;
  const v = await ask.prompt({
    title: `Locate ${p.name}`,
    note: 'Point the repo at the folder it lives in now, after you moved or renamed it on disk. Nothing is moved.',
    fields: [{ key: 'path', label: 'Folder', value: p.repo_path, mono: true }],
    confirm: 'Locate',
  });
  if (v && v.path.trim() !== p.repo_path) await attempt('Could not locate', () => api.setProjectPath(id, v.path));
}

export async function removeProject(id: string) {
  const p = m().projects.find((x) => x.id === id);
  if (!p) return;
  const r = await ask.confirm({
    title: 'Remove repo',
    message: `Remove ${p.name} from Kete? Its sessions go from the list; nothing on disk is touched.`,
    detail: [p.repo_path],
    confirm: 'Remove',
    danger: true,
  });
  if (r.ok) await attempt('Could not remove', () => api.removeProject(id));
}

// ---- menus ----

// Every menu is built from the registry's rows for the thing's kind
// (MENUS), in its order and words, each with its keys, from the verbs the
// window can run on it now (`verbTable`), then laid out by the menu matrix
// (menus/layout.ts): a rule where the section changes, a submenu group's
// rows folded into `Title \u25b8` where two or more mean something. The
// window's own rows (Review, the card's checkout, Default account) take
// their matrix places among them. State hides a row, greys it with its
// reason, or turns its words; it never reorders.

/** What the window does for a registry verb on one thing. */
export interface VerbImpl {
  run: () => unknown;
  /** Words where the verb turns (Run or Stop run, Unpin orchestrator). */
  label?: string;
  /** Its word inside its submenu, where that turns too ("Browser \u00b7
   *  localhost:5173", "Open merged pull request"). */
  word?: string;
  disabled?: boolean;
  /** Why it is disabled: the row's tooltip, and what its key says. */
  why?: string;
  /** What a live row acts on (a folder, a link): its tooltip. */
  note?: string;
  /** A checkbox row (View \u25b8 Archived sessions), or with `radio` one of a
   *  set (View \u25b8 Overview). On an editor's row: the default, what the
   *  checkout or file opens in. */
  checked?: boolean;
  radio?: boolean;
  /** The row's icon: an editor's own (a data URL). */
  icon?: string;
  /** More rows of the same verb right after it, in its submenu: each other
   *  install of an editor (Insiders, Cursor; another Visual Studio). */
  also?: VerbImpl[];
}
export type VerbTable = Partial<Record<ActionId, VerbImpl>>;

const entry = (id: ActionId, on: AppliesTo, v: VerbImpl): MenuItem => ({
  label: v.label ?? menuLabel(id, on),
  run: () => void v.run(),
  keys: verbKeys(id),
  danger: destructive(id),
  ...(v.disabled ? { disabled: true } : {}),
  ...(v.disabled ? (v.why ? { title: v.why } : {}) : v.note ? { title: v.note } : {}),
  ...(v.checked !== undefined ? { checked: v.checked } : {}),
  ...(v.radio ? { radio: true } : {}),
  ...(v.icon ? { icon: { src: v.icon } } : {}),
});

/** The registry's rows of `on`'s menu that `table` has, in its order,
 *  with their sections and submenu groups; a verb's `also` rows (an
 *  editor's other installs) right after it, in the same group. `titles`:
 *  a group's title where the window knows better (the repo's Remote \u25b8 is
 *  GitHub \u25b8). */
export function registryRows(on: AppliesTo, table: VerbTable, titles: Record<string, string> = {}): Row[] {
  return menuIds(on).flatMap((id): Row[] => {
    const v = table[id];
    if (!v) return [];
    const g = menuGroupOf(id, on);
    return [v, ...(v.also ?? [])].map((x) => ({
      id,
      section: sectionOf(id, on) ?? null,
      ...(g ? { group: { id: g.group.id, title: titles[g.group.id] ?? g.group.title, label: x.word ?? g.label } } : {}),
      item: entry(id, on, x),
    }));
  });
}

/** The registry's rows of `on`'s menu that `table` has, flat and in its
 *  order, a verb's `also` rows after it: what a table offers, laid out by
 *  nothing (the tests, a list). */
export function flatRows(on: AppliesTo, table: VerbTable): MenuItem[] {
  return registryRows(on, table).map((r) => r.item);
}

/** A row the window builds itself, at its matrix place. */
export const localRow = (id: string, section: MenuSection | null, item: MenuItem, group?: Row['group']): Row => ({
  id,
  section,
  item,
  ...(group ? { group } : {}),
});

/** A checkout's pull request verbs by its state (the matrix's table):
 *  none open, Create (greyed on the base branch); open, its page and
 *  Merge (greyed on a draft); merged or closed, its page only, saying so.
 *  `session`: the card it is asked from, whose drop verdict on Ship says
 *  whether the branch is the base. */
function prVerbs(worktree: string, session?: string): VerbTable {
  const pr = m().prs[worktree];
  const t: VerbTable = {};
  const page = () => pr && api.openUrl(pr.url).catch((e) => toast(String(e)));
  if (pr && !isOpen(pr)) {
    const words = `Open ${pr.state} pull request`;
    t.open_pull_request = { run: page, label: words, word: words, note: pr.url };
    return t;
  }
  const onBase = !!gitRows.git[worktree]?.on_default_branch;
  const rv = reviewVerbs(pr, session ? m().cards[session]?.drops?.pr : undefined, !!worktreeOf(worktree)?.is_main, onBase);
  if (pr) t.open_pull_request = { run: page, note: pr.url };
  if (rv.pr.kind === 'open')
    t.create_pull_request = { run: () => openComposer(worktree, session ?? null), disabled: !!rv.pr.why, why: rv.pr.why ?? undefined };
  if (rv.merge)
    t.merge_pull_request = {
      run: () => openMerge(worktree, session ?? null),
      disabled: !!rv.merge.why,
      why: pr?.draft ? DRAFT_MERGE : (rv.merge.why ?? undefined),
    };
  return t;
}

const DRAFT_MERGE = 'Draft pull request: publish it first';

/** Focus here, or once the scope is on it, back out: a checkout widens to
 *  its repo, a repo to everything. The account narrowing stays. */
function focusVerb(on: { worktree: string } | { repo: string }): VerbImpl {
  const s = m().scope ?? {};
  if ('worktree' in on && s.worktree === on.worktree) return { run: () => setScope(dropPart(s, 'worktree')), label: 'Widen to the repo' };
  if ('repo' in on && s.repo === on.repo && !s.worktree && !s.session) return { run: () => setScope(dropPart(s, 'repo')), label: 'Widen to everything' };
  return { run: () => setScope('repo' in on ? repoScope(s, on.repo) : worktreeScope(m(), s, on.worktree)) };
}

/** A session's verbs as the window runs them: none an archived one does
 *  not have, no reorder from a subagent's row (it acts on its parent). */
function agentVerbs(a: Agent, viaSub = false, where: StepIn = stepInFor()): VerbTable {
  if (a.archived)
    return {
      unarchive: { run: () => unarchiveAgent(a.id) },
      duplicate_session: { run: () => duplicateAgent(a.id) },
      delete: { run: () => deleteAgent(a.id) },
    };
  const pinned = orchestratorOf(m())?.id === a.id;
  const t: VerbTable = {
    activate: { run: () => openSession({ Agent: a.id }) },
    follow_up: { run: () => followUp(a.id) },
    ask_aside: { run: () => askAside(a.id) },
    git_diff: { run: () => openDiff(a.worktree_id) },
    restart: { run: () => restartAgent(a.id) },
    duplicate_session: { run: () => duplicateAgent(a.id) },
    move_session: { run: () => moveAgent(a.id) },
    // A fresh restart on another login: the conversation stays behind.
    move_to_account: canSwitch(m())
      ? { run: () => moveToAccount(a.id) }
      : { run: () => toast(SWITCH_UNAVAILABLE), disabled: true, why: SWITCH_UNAVAILABLE },
    rename: { run: () => renameAgent(a.id) },
    // Make orchestrator only while none is pinned (the menu matrix):
    // replacing one is rare, and Unpin on it comes first.
    pin_orchestrator: pinned
      ? { run: unpinOrchestrator, label: 'Unpin orchestrator' }
      : orchestratorOf(m())
        ? undefined
        : { run: () => makeOrchestrator(a) },
    archive: { run: () => archiveAgent(a.id, viaSub), label: viaSub ? 'Archive parent session\u2026' : undefined },
    delete: { run: () => deleteAgent(a.id) },
  };
  // Its worktree's pull request.
  Object.assign(t, prVerbs(a.worktree_id, a.id));
  if (!viaSub) {
    t.reorder_up = { run: () => moveStep({ kind: 'agent', id: a.id }, -1, where) };
    t.reorder_down = { run: () => moveStep({ kind: 'agent', id: a.id }, 1, where) };
  }
  Object.assign(t, sessionMenuVerbs(a));
  return t;
}

function terminalVerbs(x: TerminalTab): VerbTable {
  return {
    activate: { run: () => openSession({ Terminal: x.id }) },
    rename: { run: () => renameTerminal(x.id) },
    delete: { run: () => closeTerminal(x.id) },
  };
}

/** `w`'s run terminal while it runs. */
export function runTerminalOf(w: string): TerminalTab | undefined {
  return m().terminals.find((t) => t.worktree_id === w && !!t.run_command && t.alive);
}

/** Run or stop `w`'s run command: with one set, or while one still runs
 *  (the command taken off since). */
function runVerbOf(w: Worktree | undefined): VerbImpl | undefined {
  if (!w) return undefined;
  const running = !!runTerminalOf(w.id);
  if (!app.runCommands[w.id] && !running) return undefined;
  return {
    run: () => attempt(running ? 'Could not stop' : 'Could not run', () => (running ? api.stopRun(w.id) : api.startRun(w.id))),
    label: running ? 'Stop run' : undefined,
    note: app.runCommands[w.id],
  };
}

function worktreeVerbs(w: Worktree): VerbTable {
  const doomed = checkoutSessions(m(), w.id, app.showArchived);
  const count = doomed.agents.length + doomed.terminals.length;
  const project = w.project_id;
  const t: VerbTable = {
    quick_prompt: { run: () => focusQuickPrompt({ project, worktree: w.id, newWorktree: false }) },
    new_terminal: { run: () => newTerminal(w.id) },
    toggle_run: runVerbOf(w),
    git_diff: { run: () => openDiff(w.id) },
    // The Open command, only where there is one.
    open_worktree: openCommandRow(factsFor(w.id), () => openEditor(w.id)),
    open_repo: { run: () => openRemote(w.id) },
    ...prVerbs(w.id),
    focus_scope: focusVerb({ worktree: w.id }),
  };
  if (w.is_main && canSwitchBranch(project)) t.switch_branch = { run: () => openBranchSwitch(project) };
  if (!w.is_main) {
    t.reorder_up = { run: () => moveStep({ kind: 'worktree', id: w.id }, -1) };
    t.reorder_down = { run: () => moveStep({ kind: 'worktree', id: w.id }, 1) };
    t.delete_worktree = { run: () => deleteWorktree(w.id) };
  }
  if (count) t.delete_all = { run: () => deleteAllSessions(w.id) };
  Object.assign(t, worktreeMenuVerbs(w));
  return t;
}

function projectVerbs(project: string): VerbTable {
  const main = mainOf(project);
  const t: VerbTable = {
    quick_prompt: { run: () => focusQuickPrompt({ project }) },
    new_worktree: { run: () => newWorktree(project) },
    toggle_run: runVerbOf(main),
    open_worktree: main ? openCommandRow(factsFor(main.id), () => openEditor(main.id)) : { run: () => openEditor(undefined) },
    open_repo: { run: () => openRemote(main?.id) },
    refresh_pull_requests: { run: refreshPrs },
    focus_scope: focusVerb({ repo: project }),
    rename: { run: () => renameProject(project) },
    reorder_up: { run: () => moveStep({ kind: 'project', id: project }, -1) },
    reorder_down: { run: () => moveStep({ kind: 'project', id: project }, 1) },
    remove_project: { run: () => removeProject(project) },
  };
  if (canSwitchBranch(project)) t.switch_branch = { run: () => openBranchSwitch(project) };
  // The Worktrees view's sweep, with a linked checkout to look at. Its
  // module imports this one, so it loads when asked.
  const linked = m().worktrees.filter((w) => w.project_id === project && !w.is_main);
  if (linked.length)
    t.sweep_worktrees = {
      run: () =>
        import('./worktrees/verbs').then((v) =>
          v.sweepWorktrees(
            project,
            linked.map((w) => groupOf(m(), w)),
          ),
        ),
    };
  Object.assign(t, repoMenuVerbs(project));
  return t;
}

// ---- the menu matrix's verbs (desk-menus) ----

/** A finished turn nobody has read: what Mark seen clears. */
const unread = (a: Agent) => a.unseen && !a.archived;
/** A finished session already read, not the orchestrator: what Archive
 *  done sessions takes (the terminal app's rule; one still waiting on its
 *  subagents reads running, not finished). */
export const isDone = (a: Agent) => a.status === 'finished' && !a.unseen && !a.archived && !isOrchestrator(a);

/** Mark every unread session of `list` seen. */
export function markAllSeen(list: Agent[]) {
  const n = list.filter(unread);
  for (const a of n) api.markSeen(a.id).catch(() => {});
  toast(n.length ? `Marked ${n.length} ${n.length === 1 ? 'session' : 'sessions'} seen.` : 'Nothing unread here.');
}

/** Archive the done sessions of `list` (`where` says whose), after one
 *  confirm that lists them; U brings any back. Each is checked again as
 *  it goes: one that started a turn while the confirm was up stays. */
export async function archiveDone(list: () => Agent[], where: string) {
  const done = list().filter(isDone);
  if (!done.length) return toast(`No finished, read sessions ${where}.`);
  const r = await ask.confirm({
    title: `Archive ${done.length} done ${done.length === 1 ? 'session' : 'sessions'}`,
    message: `Archive the finished sessions you have read ${where}? Their agents stop; U brings any back.`,
    detail: done.map((a) => sessionPath(m(), a)),
    confirm: 'Archive',
  });
  if (!r.ok) return;
  const still = new Set(list().filter(isDone).map((a) => a.id));
  let n = 0;
  await attempt('Could not archive', async () => {
    for (const a of done) if (still.has(a.id)) (await api.archiveAgent(a.id), n++);
  });
  if (n) toast(`Archived ${n} done ${n === 1 ? 'session' : 'sessions'}.`);
}

/** Open `url` in the browser. */
function openLink(url: string) {
  api.openUrl(url).catch((e) => toast(String(e)));
}

/** The dev server checkout `w` listens on: its first live port. */
export function livePort(w: string | undefined): number | undefined {
  return w ? m().ports.find((p) => p.worktree === w)?.live[0] : undefined;
}

/** Open in \u25b8 Browser \u00b7 localhost:N, while `w`'s dev server listens. */
function browserVerb(w: string | undefined): VerbImpl | undefined {
  const port = livePort(w);
  if (!port) return undefined;
  const url = `http://localhost:${port}`;
  return { run: () => openLink(url), label: `Open localhost:${port} in browser`, word: `Browser \u00b7 localhost:${port}`, note: url };
}

/** Copy \u25b8 Pull request link, while the checkout has one. */
function prLinkVerb(w: string): VerbImpl | undefined {
  const pr = m().prs[w];
  return pr ? { run: () => copyText(pr.url, pr.url), note: pr.url } : undefined;
}

/** A checkout's editor and copy rows (lane/editors' `checkoutVerbs`: Open
 *  in VS Code and each other install, Visual Studio with a solution, Show
 *  in Explorer, Dock, Copy path, Copy branch name), the folder or branch
 *  each acts on as its tooltip. */
function editorVerbs(w: Worktree, on: AppliesTo): VerbTable {
  const t = checkoutVerbs(w, on);
  if (t.reveal_in_explorer) t.reveal_in_explorer = { ...t.reveal_in_explorer, note: w.path };
  if (t.copy_path) t.copy_path = { ...t.copy_path, note: w.path };
  if (t.copy_branch) t.copy_branch = { ...t.copy_branch, note: w.branch };
  return t;
}

/** The session's rows the matrix added: its dev server, its pull
 *  request's link, Mark seen. */
function sessionMenuVerbs(a: Agent): VerbTable {
  const t: VerbTable = {};
  const browser = browserVerb(a.worktree_id);
  if (browser) t.open_in_browser = browser;
  const link = prLinkVerb(a.worktree_id);
  if (link) t.copy_pr_url = link;
  if (unread(a)) t.mark_seen = { run: () => seeAgent(a.id) };
  return t;
}

/** The checkout's: its dev server, its link, and the bulk verbs over its
 *  sessions; Delete worktree says so when its pull request merged. */
function worktreeMenuVerbs(w: Worktree): VerbTable {
  const t: VerbTable = {};
  const browser = browserVerb(w.id);
  if (browser) t.open_in_browser = browser;
  const link = prLinkVerb(w.id);
  if (link) t.copy_pr_url = link;
  const mine = () => m().agents.filter((a) => a.worktree_id === w.id);
  if (mine().some(unread)) t.mark_seen = { run: () => markAllSeen(mine()) };
  if (mine().some(isDone)) t.archive_done = { run: () => archiveDone(mine, `in ${w.branch}`) };
  if (!w.is_main && m().prs[w.id]?.state === 'merged')
    t.delete_worktree = { run: () => deleteWorktree(w.id), label: 'Delete worktree (pull request merged)\u2026' };
  return t;
}

/** The repo's: its remote's own view (Azure DevOps), its main checkout's
 *  dev server, the bulk verbs over its sessions. */
function repoMenuVerbs(project: string): VerbTable {
  const t: VerbTable = {};
  if (app.adoCtx[project]) t.azure_devops = { run: () => nav.go({ view: 'tasks', repo: project }) };
  const browser = browserVerb(mainOf(project)?.id);
  if (browser) t.open_in_browser = browser;
  const wts = new Set(m().worktrees.filter((w) => w.project_id === project).map((w) => w.id));
  const mine = () => m().agents.filter((a) => wts.has(a.worktree_id));
  const name = m().projects.find((p) => p.id === project)?.name ?? 'the repo';
  if (mine().some(unread)) t.mark_seen = { run: () => markAllSeen(mine()) };
  if (mine().some(isDone)) t.archive_done = { run: () => archiveDone(mine, `in ${name}`) };
  return t;
}

/** What the window can do to `t` now, verb by verb. */
export function verbTable(t: Target): VerbTable {
  switch (t.kind) {
    case 'agent': {
      const a = agentOf(t.id);
      return a ? sessionTable(a, t.viaSub) : {};
    }
    case 'terminal': {
      const x = terminalOf(t.id);
      return x ? terminalTable(x) : {};
    }
    case 'worktree': {
      const w = worktreeOf(t.id);
      return w ? worktreeTable(w) : {};
    }
    case 'project':
      return projectTable(t.id);
    case 'account':
      return accountVerbs(t.id);
  }
}

// The tables: a checkout's editor and copy rows (`editorVerbs`) under the
// thing's own verbs. An archived card runs in no checkout.
function sessionTable(a: Agent, viaSub = false, where: StepIn = stepInFor()): VerbTable {
  const w = worktreeOf(a.worktree_id);
  return { ...(a.archived || !w ? {} : editorVerbs(w, 'session')), ...agentVerbs(a, viaSub, where) };
}
function terminalTable(x: TerminalTab): VerbTable {
  // Copy path on a terminal is its checkout's folder (the matrix widened
  // `copy_path` to terminals; this one is desk-menus').
  const path = worktreeOf(x.worktree_id)?.path;
  return { ...terminalVerbs(x), ...(path ? { copy_path: { run: () => copyText(path, path), note: path } } : {}) };
}
function worktreeTable(w: Worktree): VerbTable {
  return { ...editorVerbs(w, 'worktree'), ...worktreeVerbs(w) };
}
function projectTable(project: string): VerbTable {
  const main = mainOf(project);
  const p = m().projects.find((x) => x.id === project);
  // Its main checkout's; a repo whose checkout isn't listed yet copies its folder.
  const under: VerbTable = main ? editorVerbs(main, 'repo') : p ? { copy_path: { run: () => copyText(p.repo_path, p.repo_path), note: p.repo_path } } : {};
  return { ...under, ...projectVerbs(project) };
}

/** Run verb `id` on `t`, as its menu row does. False when it means
 *  nothing there. */
export function runVerb(t: Target, id: ActionId): boolean {
  const v = verbTable(t)[id];
  if (v?.disabled && v.why) toast(v.why);
  if (!v || v.disabled) return false;
  void v.run();
  return true;
}

/** How a menu is asked for. `jump`: the surface doesn't show the
 *  checkout's sessions (the Worktrees view, a breadcrumb), so its menu
 *  lists them to open. */
export interface MenuOpts {
  jump?: boolean;
}

// ---- the session ----

/** The banner a session's menu opens with: its state when that is worth
 *  a line (\u00a71.7). */
function sessionBanner(a: Agent): MenuHeading | null {
  const orch = isOrchestrator(a) ? 'orchestrator \u00b7 ' : '';
  if (a.archived) return { heading: 'archived' };
  // Why its last turn stopped short says more than its status.
  const stop = m().stops?.[a.id];
  if (stop) {
    const s = stopBanner(stop, a);
    return { ...s, heading: orch + s.heading };
  }
  const login = loginAccount(a);
  if (login) return { heading: `${orch}needs you \u00b7 log in again (${login.name})`, ember: true };
  if (a.status === 'needs_feedback') return { heading: `${orch}needs you`, ember: true };
  if (a.status === 'disconnected' || a.status === 'terminated') return { heading: `${orch}disconnected \u00b7 restart to resume` };
  if (a.status === 'running') return { heading: `${orch}running` };
  if (a.status === 'finished' && a.unseen) return { heading: `${orch}done \u00b7 unread` };
  return orch ? { heading: 'orchestrator' } : null;
}

/** A stop note (lane/limits) in a session's banner, in the terminal app's
 *  words (tui-menus' `stop_words`): `limited \u00b7 resumes 3:45pm` while Claude
 *  waits a limit out, `needs you \u00b7 log in again (work)` for a login to
 *  renew, `needs you \u00b7 billing`, `error \u00b7 overloaded`. */
function stopBanner(n: StopNote, a: Agent): MenuHeading {
  const words = stopLabel(n, app.now);
  switch (stopTone(n)) {
    case 'waiting':
      return { heading: `limited \u00b7 ${words}` };
    case 'needs':
      return n.reason === 'authentication_failed' || n.reason === 'cloud_credential_error'
        ? { heading: `needs you \u00b7 log in again (${accountKey(a.profile, sessionHarness(a) ?? '').replace(/^@/, '')})`, ember: true }
        : { heading: `needs you \u00b7 ${words}`, ember: true };
    case 'error':
      return { heading: `error \u00b7 ${words}` };
  }
}

/** The account a session runs on, when it needs signing in again. */
function loginAccount(a: Agent): AccountRow | undefined {
  if (!app.accounts.length) return undefined;
  const key = accountKey(a.profile, sessionHarness(a) ?? '');
  const r = app.accounts.find((x) => x.key === key);
  return r && r.loginHint && !r.problem && (r.login === 'needs_login' || r.expired) ? r : undefined;
}

/** A session's menu (\u00a75.1). `where`: the board orders a lane's cards, the
 *  overview a tree's rows; by default wherever the focus is as it opens. */
export function agentMenu(a: Agent, viaSub = false, where: StepIn = stepInFor()): MenuEntry[] {
  // From a subagent's row every verb is the parent's: say so up top.
  const head: MenuEntry[] = viaSub ? [{ heading: `Parent session: ${a.name}` }] : [];
  const banner = sessionBanner(a);
  if (banner) head.push(banner);
  let rows = registryRows('session', sessionTable(a, viaSub, where));
  if (a.archived) return layout(rows, { head });
  const login = loginAccount(a);
  if (login) rows = insertRows(rows, localRow('local:session.login', 'act', { label: `Log in to ${login.name}\u2026`, run: () => void runLogin(login), title: login.loginHint ?? undefined }), { before: ['activate'] });
  const parent = a.parent ? agentOf(a.parent) : undefined;
  if (parent && !parent.archived)
    rows = insertRows(rows, localRow('local:session.parent', 'go', { label: `Open parent session ${parent.name}`, run: () => openSession({ Agent: parent.id }) }), {
      after: ['git_diff'],
    });
  // Review: the orchestrator's review sheet, first in Pull request \u25b8.
  const review = localRow('local:session.review', 'go', { label: 'Review\u2026', run: () => openReview([a.id]) }, { id: 'pull_request', title: 'Pull request', label: 'Review\u2026' });
  rows = insertRows(rows, review, { before: ['open_pull_request', 'create_pull_request', 'merge_pull_request'], after: ['local:session.parent', 'git_diff'] });
  const w = worktreeOf(a.worktree_id);
  const checkout = w && checkoutRow(w, rows);
  return layout(checkout ? [...rows, checkout] : rows, { head });
}

/** The card's `Worktree \u2039branch\u203a \u25b8` (\u00a75.2): the checkout's own menu less
 *  every verb the card already shows, flat; its Move rows say whose. */
function checkoutRow(w: Worktree, shown: readonly Row[]): Row | null {
  const own = new Set(['reorder_up', 'reorder_down', 'mark_seen']);
  const said = shown.map((r) => r.id).filter((id) => !own.has(id));
  const rows = withoutIds(checkoutRows(w, {}), said).map((r) =>
    r.id === 'reorder_up' || r.id === 'reorder_down'
      ? { ...r, item: { ...r.item, label: r.id === 'reorder_up' ? 'Move worktree up' : 'Move worktree down' } }
      : r,
  );
  const kids = layout(rows, { fold: false, rules: false });
  if (!kids.length) return null;
  return localRow('local:session.checkout', null, submenu(`${w.is_main ? 'Checkout' : 'Worktree'} ${w.branch}`, kids));
}

// ---- terminals, checkouts, repos ----

export function terminalMenu(t: TerminalTab): MenuEntry[] {
  const rows = registryRows('terminal', terminalTable(t));
  const w = worktreeOf(t.worktree_id);
  const checkout = w && checkoutRow(w, rows);
  return layout(checkout ? [...rows, checkout] : rows);
}

// ---- accounts ----

/** Whakaoho runs Claude accounts only: `@claude`, or a profile on Claude. */
function isClaudeAccount(key: string): boolean {
  if (key === '@claude') return true;
  if (key.startsWith('@')) return false;
  const harness = app.accounts.find((r) => r.key === key)?.harness;
  return harness === undefined || harness === 'claude';
}

/** The registry's account verbs that are lane/wake's: Wake up now and the
 *  Wake-up schedule, on a Claude account only (any other has neither:
 *  the matrix hides what can never apply). The account menu
 *  (accounts/account-menu.ts) starts its table from these. */
export function accountVerbs(key: string): VerbTable {
  if (!isClaudeAccount(key)) return {};
  return { wake_account: wakeNowVerb(key), wake_schedule: { run: () => openWake(key) } };
}

/** Wake up now on a Claude account: disabled while it is switched off,
 *  until it is signed in, and while it is already waking; labelled with
 *  its live window's end (a wake then starts nothing new). */
function wakeNowVerb(key: string): VerbImpl {
  const row = app.accounts.find((r) => r.key === key);
  if (row?.disabled) return { run: () => {}, disabled: true, why: 'Switched off: switch it on to wake it.' };
  if (row?.login === 'needs_login') return { run: () => {}, disabled: true, why: `Log in first: ${row.loginHint ?? 'sign it in'}` };
  if (wake.waking[key]) return { run: () => {}, disabled: true, why: 'Waking now: up to two minutes.' };
  const live = m().accounts?.[key]?.windows.find((w) => w.kind === 'session' && (w.resets_at ?? 0) * 1000 > Date.now());
  return {
    run: () => wakeNowFor(key),
    ...(live?.resets_at ? { label: `${menuLabel('wake_account', 'account')} (window live until ${clock(live.resets_at)})` } : {}),
  };
}

/** `HH:MM` on the local clock for an epoch second. */
function clock(epoch: number): string {
  const d = new Date(epoch * 1000);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** The registry's global `wake_schedule` ("Whakaoho..."), with no account
 *  named: the view. */
export const openWakeView = (): void => openWake();

/** An account's full menu (\u00a75.9), which accounts/account-menu.ts
 *  builds: bound late, as that module imports this one. */
let accountMenuOf: (key: string) => MenuEntry[] = () => [];

/** accounts/account-menu.ts hands its builder over as it loads. */
export function setAccountMenu(build: (key: string) => MenuEntry[]) {
  accountMenuOf = build;
}

/** The banner a checkout's menu opens with. */
function checkoutBanner(w: Worktree): MenuHeading | null {
  const words: string[] = [];
  if (m().prs[w.id]?.state === 'merged') words.push('merged');
  else if (!w.is_main && gitRows.git[w.id]?.on_default_branch) words.push('on the base branch');
  if (runTerminalOf(w.id)) {
    const port = livePort(w.id);
    words.push(port ? `running :${port}` : 'running');
  }
  return words.length ? { heading: words.join(' \u00b7 ') } : null;
}

/** A checkout's rows (\u00a75.5): the registry's, Show run terminal while its
 *  run goes, and with `jump` its live sessions to open. */
function checkoutRows(w: Worktree, opts: MenuOpts): Row[] {
  let rows = registryRows('worktree', worktreeTable(w));
  const run = runTerminalOf(w.id);
  if (run)
    rows = insertRows(rows, localRow('local:worktree.run_terminal', 'go', { label: 'Show run terminal', run: () => openSession({ Terminal: run.id }), title: run.run_command ?? undefined }), {
      after: ['open_repo', 'focus_scope', 'git_diff'],
    });
  if (opts.jump) {
    const kids = sessionJumps(w.id);
    if (kids.length)
      rows = insertRows(rows, localRow('local:worktree.sessions', 'go', submenu('Open session', kids)), {
        after: ['local:worktree.run_terminal', 'open_repo', 'focus_scope', 'git_diff'],
      });
  }
  return rows;
}

const TONE_DOT: Record<string, NonNullable<Extract<MenuIcon, { dot: string }>['dot']>> = {
  needs: 'ember',
  running: 'ink',
  failed: 'fail',
  unread: 'unread',
  done: 'done',
};

/** Checkout `w`'s live sessions and terminals, up to nine, to open. */
function sessionJumps(w: string): MenuItem[] {
  const agents = m()
    .agents.filter((a) => a.worktree_id === w && !a.archived)
    .sort((a, b) => a.sort_order - b.sort_order);
  const terms = m().terminals.filter((t) => t.worktree_id === w);
  const rows: MenuItem[] = [
    ...agents.map((a) => ({ label: a.name, run: () => openSession({ Agent: a.id }), icon: { dot: TONE_DOT[toneOf(a)] ?? 'faint' } })),
    ...terms.map((t) => ({ label: t.name, run: () => openSession({ Terminal: t.id }), icon: { glyph: '>' } })),
  ];
  return rows.slice(0, 9) as MenuItem[];
}

/** A checkout's menu (\u00a75.5). */
export function checkoutMenu(w: Worktree, opts: MenuOpts = {}): MenuEntry[] {
  const banner = checkoutBanner(w);
  return layout(checkoutRows(w, opts), { head: banner ? [banner] : [] });
}

/** The repo's remote by its host, for Remote \u25b8's title: Azure DevOps by
 *  its probe, GitHub once a pull request was found on it. */
function remoteTitle(project: string): string | undefined {
  if (app.adoCtx[project]) return 'Azure DevOps';
  const wts = new Set(m().worktrees.filter((w) => w.project_id === project).map((w) => w.id));
  return Object.keys(m().prs).some((w) => wts.has(w)) ? 'GitHub' : undefined;
}

/** Bring a checkout's band into view on the overview, focused. */
export function revealWorktree(w: Worktree) {
  setCollapsed(repoKey(w.project_id), false);
  revealRepo(w.project_id);
  setTimeout(() => {
    const el = document.querySelector<HTMLElement>(`[data-worktree="${CSS.escape(w.id)}"] [data-target]`);
    el?.scrollIntoView({ block: 'nearest' });
    el?.focus();
  }, 120);
}

/** A repo's Default account \u25b8: per harness with accounts, each one (the
 *  repo's checked) and "Its default", the spread picking. */
function defaultAccountRows(project: string): MenuEntry[] {
  const p = m().projects.find((x) => x.id === project);
  const profiles = app.launch.profiles;
  if (!p || !profiles.length) return [];
  const harnesses = [...new Set(profiles.map((x) => x.harness))];
  const out: MenuEntry[] = [];
  for (const h of harnesses) {
    const now = projectAccount(app.launch, p.repo_path, h);
    if (harnesses.length > 1) out.push({ heading: h });
    out.push({
      label: 'Its default',
      title: `No ${h} account of its own: the account spread picks`,
      checked: !now,
      radio: true,
      run: () => void (now && setProjectAccount(project, h, null)),
    });
    for (const x of profiles.filter((y) => y.harness === h))
      out.push({
        label: x.name,
        checked: x.name === now,
        radio: true,
        ...(x.disabled ? { title: `${x.name} is switched off: launches that pick no account skip it` } : {}),
        run: () => void (x.name !== now && setProjectAccount(project, h, x.name)),
      });
  }
  return out;
}

/** A repo's menu (\u00a75.6). */
export function projectMenu(project: string): MenuEntry[] {
  let rows = registryRows('repo', projectTable(project), { remote: remoteTitle(project) ?? 'Remote' });
  const linked = m()
    .worktrees.filter((w) => w.project_id === project && !w.is_main)
    .sort((a, b) => a.sort_order - b.sort_order);
  if (linked.length) {
    const kids: MenuEntry[] = [
      ...linked.map((w): MenuItem => ({ label: w.branch, run: () => revealWorktree(w), title: w.path })),
      'sep',
      { label: 'All worktrees\u2026', run: () => nav.go({ view: 'worktrees', repo: project }) },
    ];
    rows = insertRows(rows, localRow('local:repo.worktrees', 'go', submenu('Worktrees', kids)), {
      after: ['refresh_pull_requests', 'azure_devops', 'open_repo', 'focus_scope'],
    });
  }
  const accounts = defaultAccountRows(project);
  if (accounts.length) rows = insertRows(rows, localRow('local:repo.default_account', 'change', submenu('Default account', accounts)), { after: ['rename', 'switch_branch', 'mark_seen'] });
  rows = insertRows(rows, localRow('local:repo.locate', 'away', { label: 'Locate folder\u2026', run: () => void locateProject(project) }), {
    before: ['sweep_worktrees', 'remove_project'],
  });
  return layout(rows);
}

/** The menu for `t`: every way in (right-click, `\u22ef`, Shift+F10, Enter on
 *  a Worktrees row, `?`) builds this one. */
export function menuFor(t: Target, opts: MenuOpts = {}): MenuEntry[] {
  // Whether an editor window is open on its checkout, for Dock and the
  // icons' dot: asked as the menu opens, never polled.
  touch(t.kind === 'agent' ? agentOf(t.id)?.worktree_id : t.kind === 'terminal' ? terminalOf(t.id)?.worktree_id : t.kind === 'worktree' ? t.id : t.kind === 'project' ? mainOf(t.id)?.id : undefined);
  switch (t.kind) {
    case 'agent': {
      const a = agentOf(t.id);
      return a ? agentMenu(a, t.viaSub) : [];
    }
    case 'terminal': {
      const x = terminalOf(t.id);
      return x ? terminalMenu(x) : [];
    }
    case 'worktree': {
      const w = worktreeOf(t.id);
      return w ? checkoutMenu(w, opts) : [];
    }
    case 'project':
      return projectMenu(t.id);
    case 'account':
      return accountMenuOf(t.id);
  }
}

/** What a menu for `t` is named: the thing it acts on. */
export function targetLabel(t: Target): string {
  switch (t.kind) {
    case 'agent':
      return agentOf(t.id)?.name ?? 'Session';
    case 'terminal':
      return terminalOf(t.id)?.name ?? 'Terminal';
    case 'worktree':
      return worktreeOf(t.id)?.branch ?? 'Worktree';
    case 'project':
      return m().projects.find((p) => p.id === t.id)?.name ?? 'Repo';
    case 'account':
      return app.accounts.find((r) => r.key === t.id)?.name ?? t.id;
  }
}

/** Open `t`'s menu at `at`, named for it: the kebabs, the Menu key,
 *  Enter on a Worktrees row. */
export function openTargetMenu(t: Target, at: { x: number; y: number } | HTMLElement, opts: MenuOpts = {}) {
  openMenu(menuFor(t, opts), at, targetLabel(t));
}

/** A row key from the shortcut table, on its target. True when handled. */
export function runRowKey(id: ShortcutId, t: Target): boolean {
  const a = t.kind === 'agent' ? agentOf(t.id) : undefined;
  const term = t.kind === 'terminal' ? terminalOf(t.id) : undefined;
  const wt = a?.worktree_id ?? term?.worktree_id ?? (t.kind === 'worktree' ? t.id : t.kind === 'project' ? mainOf(t.id)?.id : undefined);
  const table: Partial<Record<ShortcutId, () => unknown>> = a
    ? {
        rename: () => renameAgent(a.id),
        archive: () => archiveAgent(a.id, t.kind === 'agent' && t.viaSub),
        unarchive: () => unarchiveAgent(a.id),
        delete: () => deleteAgent(a.id),
        restart: () => restartAgent(a.id),
        move: () => moveAgent(a.id),
        duplicate: () => duplicateAgent(a.id),
        follow_up: () => followUp(a.id),
      }
    : term
      ? { rename: () => renameTerminal(term.id), delete: () => closeTerminal(term.id) }
      : {};
  if (t.kind === 'project') table.rename = () => renameProject(t.id);
  Object.assign(table, {
    new_terminal: () => newTerminal(wt),
    open_editor: () => openEditor(wt),
    open_remote: () => openRemote(wt),
    // The row's repo: only its root checkout switches.
    switch_branch: () => {
      const p = m().worktrees.find((w) => w.id === wt)?.project_id;
      if (p && canSwitchBranch(p)) openBranchSwitch(p);
      else toast('Switch branch is for repos without an Azure DevOps remote: their branches live in the ADO views.');
    },
    // Shift+V, as in the terminal app: the checkout's pull request page.
    open_pr: () => {
      const pr = wt ? m().prs[wt] : undefined;
      if (pr) api.openUrl(pr.url).catch((e) => toast(String(e)));
      else toast('No pull request on this branch yet.');
    },
  });
  const f = table[id];
  if (!f) return false;
  f();
  return true;
}

/** The label a session goes by in lists: `repo / branch / name`. */
export function sessionPath(model: Model, a: Agent): string {
  const w = model.worktrees.find((x) => x.id === a.worktree_id);
  const p = model.projects.find((x) => x.id === w?.project_id);
  return [p?.name, w && !w.is_main ? w.branch : null, a.name].filter(Boolean).join(' / ');
}
