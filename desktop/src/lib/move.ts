// What a session leaves behind when it moves to another checkout: the
// uncommitted files and unpushed commits stay where they are. The words the
// move confirm shows, shared by the Move to... menu and a drag onto a band.
// Both apps count the same way (`git status -uall`, untracked files one by
// one; commits HEAD has that its upstream lacks, or with no upstream that no
// remote has) and build the same line (nebula-tui `move_check::report`).

import type { CheckoutState, MoveCheck } from './worktrees/api';

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** "3 changed files and 2 unpushed commits stay in feature-x.", or null
 *  for a clean checkout (or one whose state could not be read), which
 *  moves without asking. Untracked files count as changed. */
export function moveReport(state: CheckoutState | null, branch: string): string | null {
  if (!state) return null;
  const files = state.changed + state.untracked;
  const unpushed = state.unpushed ?? 0;
  const parts = [
    ...(files ? [plural(files, 'changed file', 'changed files')] : []),
    ...(unpushed ? [plural(unpushed, 'unpushed commit', 'unpushed commits')] : []),
  ];
  if (!parts.length) return null;
  return `${parts.join(' and ')} ${files + unpushed === 1 ? 'stays' : 'stay'} in ${branch}.`;
}

/** The move confirm's question, or null to move without asking: the
 *  report, or for a read that ran out of time a line saying so, then the
 *  question; `asked` (a drag's own question) for a clean checkout. */
export function moveQuestion(check: MoveCheck | null, from: string, name: string, to: string, asked?: string): string | null {
  const line = check?.timed_out
    ? `Could not read what stays behind in ${from} (git took too long).`
    : moveReport(check?.state ?? null, from);
  if (line) return `${line} Move "${name}" to ${to} anyway?`;
  return asked ?? null;
}

/** Keys with a read out: a second call for one returns null at once and
 *  starts nothing, so Move pressed twice reads the checkout once. */
export function oneAtATime<T>() {
  const pending = new Set<string>();
  return async (key: string, run: () => Promise<T>): Promise<T | null> => {
    if (pending.has(key)) return null;
    pending.add(key);
    try {
      return await run();
    } finally {
      pending.delete(key);
    }
  };
}
