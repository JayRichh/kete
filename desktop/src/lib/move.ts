// What a session leaves behind when it moves to another checkout: the
// uncommitted files and unpushed commits stay where they are. The words the
// move confirm shows, shared by the Move to... menu and a drag onto a band.
// The terminal app builds the same line (nebula-tui `move_report`).

import type { CheckoutState } from './worktrees/api';

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
