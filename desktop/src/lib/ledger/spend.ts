// The ledger's one-line wording for the session rail: today's spend.
import type { SpendRow } from '../studio/types';

/** `412k`, `1.5M`, or the whole number under a thousand. */
function short(n: number): string {
  if (n >= 999_500) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Today's local date, `YYYY-MM-DD`, as the ledger names its days. */
export function localDay(d: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Milliseconds from `now` to the next local midnight. */
export function msToMidnight(now: Date = new Date()): number {
  const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return next.getTime() - now.getTime();
}

/** Call `onDay` with the new local date just after each local midnight
 *  (a timer, so a machine that slept catches up when it wakes); the
 *  returned function stops it. */
export function everyMidnight(onDay: (day: string) => void): () => void {
  let timer: ReturnType<typeof setTimeout>;
  const arm = () => {
    timer = setTimeout(() => {
      onDay(localDay());
      arm();
    }, msToMidnight() + 1000);
  };
  arm();
  return () => clearTimeout(timer);
}

/** `today: 412k tokens, est. US$3.10, 58 tool uses, 9 turns`. Tokens are
 *  input plus output; cache tokens are left out (pending Jay). Nothing for
 *  no row, or for a row from another day (yesterday's, past midnight). */
export function spendLine(row: SpendRow | undefined, today: string = localDay()): string | null {
  if (!row || row.day !== today) return null;
  const tokens = row.input_tokens + row.output_tokens;
  const words = tokens === 1 ? 'token' : 'tokens';
  return [
    `today: ${short(tokens)} ${words}`,
    `est. US$${row.cost_usd.toFixed(2)}`,
    count(row.tool_uses, 'tool use', 'tool uses'),
    count(row.turns, 'turn', 'turns'),
  ].join(', ');
}
