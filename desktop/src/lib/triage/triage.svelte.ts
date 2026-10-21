// The triage strip's shapes and words. Pure apart from the input the strip
// registers for `focusTriage`; imports types only, so a test reads it
// without Tauri.
import type { Model } from '../api';

export type Target = 'desk' | 'orchestrator';

/** What `api.triageSubmit` resolves to (src-tauri triage). */
export type TriageSent =
  | { kind: 'filed'; id: string; path: string; to: Target | null; route: string | null; busy: boolean; unsent: string | null }
  | { kind: 'duplicate'; path: string; reason: string };

export interface TriageNote {
  id: string;
  title: string;
  path: string;
  status: string;
  to: string | null;
  at: string | null;
}

/** What `api.triageTail` resolves to. */
export interface TriageTail {
  repo: string;
  name: string;
  /** Absolute inbox folder; null when the repo names none. */
  inbox: string | null;
  /** Why there is no inbox. */
  error: string | null;
  /** The repo defines a `triage` card, or a desk runs. */
  desk: boolean;
  orchestrator: boolean;
  /** The last route line this window got for the repo. */
  route: string | null;
  /** Newest 3. */
  notes: TriageNote[];
}

export const targetLabel = (t: Target): string => (t === 'desk' ? 'Triage desk' : 'Orchestrator');

export const defaultTarget = (tail: Pick<TriageTail, 'desk'> | null): Target => (tail?.desk ? 'desk' : 'orchestrator');

/** The target the chip may take: the desk only where the repo has one. */
export const allowedTarget = (tail: Pick<TriageTail, 'desk'> | null, want: Target): Target =>
  want === 'desk' && !tail?.desk ? 'orchestrator' : want;

/** The other target, or the same one when the chip cannot switch. */
export const nextTarget = (tail: Pick<TriageTail, 'desk'> | null, now: Target): Target =>
  allowedTarget(tail, now === 'desk' ? 'orchestrator' : 'desk');

/** The route line while a note is on its way. */
export const sendingLabel = (t: Target): string => `Sending to the ${t === 'desk' ? 'triage desk' : 'orchestrator'}\u2026`;

const fileName = (p: string): string => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

/** The route line once a note is filed. */
export function routeText(s: TriageSent): string {
  if (s.kind === 'duplicate') return `Already in the inbox: ${fileName(s.path)} (${s.reason})`;
  if (s.route) return s.route;
  if (s.busy) return `Filed ${s.id}. The desk is busy, so the note waits in the inbox.`;
  if (s.unsent) return `Filed ${s.id}. Not sent: ${s.unsent}`;
  return `Filed ${s.id}.`;
}

/** The repo the orchestrator works in: its session's checkout's project. */
export function orchestratorRepo(m: Model): string | null {
  const a = m.agents.find((x) => x.role === 'orchestrator' && !x.archived);
  return m.worktrees.find((w) => w.id === a?.worktree_id)?.project_id ?? null;
}

/** The repos a note can be filed into: the orchestrator's, then each studio repo. */
export function repoChoices(m: Model, studio: { repo: string; name: string }[]): { id: string; name: string }[] {
  const own = orchestratorRepo(m);
  const ids = [...new Set([...(own ? [own] : []), ...studio.map((r) => r.repo)])];
  return ids.flatMap((id) => {
    const name = m.projects.find((p) => p.id === id)?.name ?? studio.find((r) => r.repo === id)?.name;
    return name ? [{ id, name }] : [];
  });
}

let input: HTMLInputElement | null = null;

/** The strip hands over its field while it is on screen (null when it goes). */
export function registerInput(el: HTMLInputElement | null): void {
  input = el;
}

/** Put the cursor in the strip's field; false when the strip is not shown. */
export function focusInput(): boolean {
  input?.focus();
  return !!input;
}
