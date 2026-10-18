// What the board lists under a session card: every subagent the daemon
// knows of for it, running ones first. With the `hide_done_subagents`
// setting (shared with the terminal app, on by default) the ended ones
// fold into one count, `3 finished`, that a click opens; off, they show
// one by one with how they ended. The terminal app's cards follow the
// same rule (`nebula-tui/src/ui/subagents_view.rs`, `card_lines`).

import type { Subagent } from './api';

/** A subagent's word: `closed` for one still open when its session ended
 *  or restarted, which says more than `stopped`. */
export function subagentWord(s: Subagent): string {
  switch (s.status) {
    case 'running':
      return 'running';
    case 'done':
      return 'done';
    case 'failed':
      return 'failed';
    default:
      return 'closed';
  }
}

export function isEnded(s: Subagent): boolean {
  return s.status !== 'running';
}

/** The model (without its `claude-` prefix) and effort a subagent runs
 *  at, `haiku-4-5 \u00b7 low`; empty when neither is known. The terminal app's
 *  rows say the same (`nebula-tui/src/subagents.rs`, `sub_run`). */
export function subagentRun(s: Subagent): string {
  const model = (s.run?.model ?? '').replace(/^claude-/, '');
  return [model, s.run?.effort ?? ''].filter(Boolean).join(' \u00b7 ');
}

/** When a subagent last moved: its end, else its start. */
export function movedAt(s: Subagent): number {
  return s.ended_at ?? s.started_at;
}

export interface CardSubagents {
  /** The rows to draw, running first, then the most recent. */
  shown: Subagent[];
  /** Ended ones folded into the count; 0 while they show. */
  folded: number;
  running: number;
  ended: number;
}

/** Session `parent`'s subagents as its card lists them. `open` shows the
 *  folded ones (the card's count was clicked). */
export function cardSubagents(subs: Subagent[], parent: string, hideDone: boolean, open = false): CardSubagents {
  const mine = subs.filter((s) => s.parent_agent === parent);
  const ended = mine.filter(isEnded).length;
  const fold = hideDone && !open;
  const shown = mine
    .filter((s) => !fold || !isEnded(s))
    .sort((a, b) => Number(isEnded(a)) - Number(isEnded(b)) || movedAt(b) - movedAt(a));
  return { shown, folded: fold ? ended : 0, running: mine.length - ended, ended };
}

/** Every ended subagent the model holds: what the board's Clear drops. */
export function endedTotal(subs: Subagent[]): number {
  return subs.filter(isEnded).length;
}
