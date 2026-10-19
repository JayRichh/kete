// Drags and Alt+Up / Alt+Down: what is being dragged, and the `Reorder`
// (or move) a drop or a key sends. The rules are order.ts's; this file
// reads the model and talks to the daemon.

import { api, type Agent, type DropTarget, type DropVerb, type ReorderScope, type Worktree } from './api';
import { worktreeApi } from './worktrees/api';
import { moveQuestion, oneAtATime } from './move';
import { app, toast } from './store.svelte';
import { ask } from './ui/ui.svelte';
import { agentIds, boardIds, placeId, projectIds, rootFirst, stepId, treeSiblings, worktreeIds, type Place } from './order';
import { laneOf, lanes, type Lane } from './status';
import { worktreeGroups } from './worktrees/groups';
import { DRAG_PROJECT, DRAG_SESSION, DRAG_WORKTREE } from './overview/drag';

/** What a drag carries. A session from the board knows its lane, and
 *  core's verdict for each target as the drag began (`Card.drops`), so a
 *  card whose lane moves mid-drag keeps the verdicts it was picked up with. */
export type Dragged =
  | { kind: 'agent'; id: string; worktree: string; lane?: Lane; drops?: Partial<Record<DropTarget, DropVerb>> }
  | { kind: 'worktree'; id: string; project: string }
  | { kind: 'project'; id: string };

/** The drag in flight: `getData` is empty while dragging over a row, so
 *  the rows read what is dragged here. */
export const drag = $state<{ item: Dragged | null }>({ item: null });

const TYPE: Record<Dragged['kind'], string> = { agent: DRAG_SESSION, worktree: DRAG_WORKTREE, project: DRAG_PROJECT };

export function startDrag(e: DragEvent, item: Dragged) {
  if (!e.dataTransfer) return;
  e.dataTransfer.setData(TYPE[item.kind], item.id);
  e.dataTransfer.effectAllowed = 'move';
  drag.item = item;
}

export function endDrag() {
  drag.item = null;
}

// A dragged row can lose its node mid-drag (a card whose lane changes as
// its turn ends), and its dragend with it. The window's drop (after the
// rows' own) and the first mouse move after a drag (none fire during
// one) end it anyway, so no Archived zone or faded card stays behind.
if (typeof window !== 'undefined') {
  window.addEventListener('drop', endDrag);
  window.addEventListener('mousemove', () => drag.item && endDrag());
}

/** The drop zone's pointer offset from a row's top, and its height. */
export function offsetIn(e: DragEvent): [number, number] {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  return [e.clientY - r.top, r.height];
}

/** Whether the pointer left `e.currentTarget` itself, not just crossed
 *  into one of its children (which fires dragleave too). */
export function leftFor(e: DragEvent): boolean {
  const to = e.relatedTarget as Node | null;
  return !to || !(e.currentTarget as Node).contains(to);
}

/** Send a scope's new order. A refusal (an older daemon says what to run)
 *  is a toast. The focused row keeps the focus while its node moves. */
export async function sendReorder(scope: ReorderScope, ids: string[] | null) {
  if (!ids) return;
  const held = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  try {
    await api.reorder(scope, ids);
  } catch (e) {
    toast(`Could not reorder: ${e}`);
    return;
  }
  if (held) keepFocus(held);
}

/** A keyed list moves the row's own node, which drops the focus; give it
 *  back while the daemon's upserts land, unless the user went elsewhere. */
function keepFocus(el: HTMLElement) {
  let frames = 60;
  const tickFocus = () => {
    const now = document.activeElement;
    if (!el.isConnected || (now && now !== document.body && now !== el)) return;
    if (now !== el) {
      el.focus({ preventScroll: true });
      el.scrollIntoView({ block: 'nearest' });
    }
    if (--frames > 0) requestAnimationFrame(tickFocus);
  };
  requestAnimationFrame(tickFocus);
}

// ---- drops ----

/** Repo `id` dropped before or after repo `target`. */
export function dropProject(id: string, target: string, place: Place) {
  return sendReorder({ kind: 'projects' }, placeId(projectIds(app.model), id, target, place));
}

/** Worktree `id` dropped before or after `target`, in one repo. */
export function dropWorktree(project: string, id: string, target: string, place: Place) {
  const ids = placeId(worktreeIds(app.model, project), id, target, place);
  return sendReorder({ kind: 'worktrees', project }, ids && rootFirst(app.model, ids));
}

/** Session `id` dropped before or after `target`, in one checkout. */
export function dropAgent(worktree: string, id: string, target: string, place: Place) {
  return sendReorder({ kind: 'agents', worktree }, placeId(agentIds(app.model, worktree), id, target, place));
}

/** A board card dropped before or after another of its checkout's cards. */
export function dropCard(worktree: string, id: string, target: string, place: Place) {
  return sendReorder({ kind: 'agents', worktree }, placeId(boardIds(app.model, worktree), id, target, place));
}

/** Whether a dragged worktree may land beside `target`: the same repo,
 *  never above the root. */
export function worktreeDropOk(d: Dragged | null, project: string, target: string): d is Extract<Dragged, { kind: 'worktree' }> {
  return d?.kind === 'worktree' && d.project === project && d.id !== target;
}

/** Whether a dragged session may land beside `target` in the overview:
 *  shown under the same parent in the same checkout. */
export function agentDropOk(d: Dragged | null, target: string): d is Extract<Dragged, { kind: 'agent' }> {
  return d?.kind === 'agent' && d.id !== target && treeSiblings(app.model, d.id, app.showArchived).includes(target);
}

/** Whether `agent` may move out of its checkout into `to`: a checkout
 *  with uncommitted files or unpushed commits says what stays behind and
 *  asks; a clean one moves at once. `asked` is the question a caller
 *  puts anyway (a drag), so a clean checkout still gets that one. */
export async function confirmMove(agent: Agent, to: Worktree, asked?: string): Promise<boolean> {
  const from = app.model.worktrees.find((x) => x.id === agent.worktree_id);
  // One read and one question per session at a time: a second Move press
  // while the first is reading or asking does nothing.
  const answer = await moveAsks(agent.id, async () => {
    const check = from ? await worktreeApi.moveCheck(from.id).catch(() => null) : null;
    const message = moveQuestion(check, from?.branch ?? '', agent.name, to.branch, asked);
    if (!message) return true;
    const r = await ask.confirm({ title: 'Move session', message, confirm: 'Move' });
    return r.ok;
  });
  return answer ?? false;
}
const moveAsks = oneAtATime<boolean>();

/** A session dropped on a checkout (a worktree's header, a repo in the
 *  rail: its root): asks, then moves it there. */
export async function moveOnto(agent: string, worktree: string) {
  const a = app.model.agents.find((x) => x.id === agent);
  const w = app.model.worktrees.find((x) => x.id === worktree);
  if (!a || !w || a.worktree_id === w.id) return;
  if (!(await confirmMove(a, w, `Move "${a.name}" to ${w.branch}? It carries on in that checkout.`))) return;
  try {
    await api.moveAgent(agent, w.id);
    toast(`Moved ${a.name} to ${w.branch}.`);
  } catch (err) {
    toast(`Could not move: ${err}`);
  }
}

/** A repo's root checkout (the first one when none is marked). */
export function rootOf(project: string): string | undefined {
  const ws = app.model.worktrees.filter((w) => w.project_id === project);
  return (ws.find((w) => w.is_main) ?? ws[0])?.id;
}

// ---- Alt+Up / Alt+Down and the menus' Move up / Move down ----

/** Where a step happens: the board's lane, else the overview's tree. */
export type StepIn = 'board' | 'tree';

/** One step up or down for a repo, worktree or session. False when the
 *  row has no order to step in (a terminal, a subagent's row). */
export function moveStep(t: { kind: string; id: string; viaSub?: boolean }, dir: -1 | 1, where: StepIn = 'tree'): boolean {
  const m = app.model;
  if (t.kind === 'project') {
    const ids = projectIds(m);
    void sendReorder({ kind: 'projects' }, stepId(ids, ids, t.id, dir));
    return true;
  }
  if (t.kind === 'worktree') {
    const w = m.worktrees.find((x) => x.id === t.id);
    if (!w) return false;
    if (w.is_main) {
      toast('The root checkout stays first.');
      return true;
    }
    // The checkouts the overview shows beside it, the root aside.
    const peers = worktreeGroups(m, w.project_id, { archived: app.showArchived, showAll: app.launch.show_all_worktrees !== false })
      .map((g) => g.worktree)
      .filter((x) => !x.is_main)
      .map((x) => x.id);
    const ids = stepId(worktreeIds(m, w.project_id), peers.includes(w.id) ? peers : [...peers, w.id], w.id, dir);
    void sendReorder({ kind: 'worktrees', project: w.project_id }, ids && rootFirst(m, ids));
    return true;
  }
  if (t.kind === 'agent' && !t.viaSub) {
    const a = m.agents.find((x) => x.id === t.id);
    if (!a) return false;
    const peers =
      where === 'board'
        ? lanes(m)[laneOf(m, a)].filter((x) => x.worktree_id === a.worktree_id).map((x) => x.id)
        : treeSiblings(m, a.id, app.showArchived);
    void sendReorder({ kind: 'agents', worktree: a.worktree_id }, stepId((where === 'board' ? boardIds : agentIds)(m, a.worktree_id), peers, a.id, dir));
    return true;
  }
  return false;
}

/** Where a row sits (the focused one by default), for a step. */
export function stepInFor(el: Element | null = typeof document === 'undefined' ? null : document.activeElement): StepIn {
  return el?.closest('[data-board]') ? 'board' : 'tree';
}
