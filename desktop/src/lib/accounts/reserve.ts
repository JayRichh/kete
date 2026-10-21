// Reserve for me: a profile kept for the person's own sessions, which the
// orchestrator and scripts never pick (the default for launches the person
// starts stays as it was). The switch's model on the Accounts view's rows and
// the menu's row; pure, as view.ts, so the tests build the rows.

import { invoke } from '@tauri-apps/api/core';
import type { AccountRow } from '../api';
import type { MenuEntry } from '../ui/ui.svelte';

/** A row as `accounts_list` sends it: `reserved` is not in api.ts's type yet. */
export type ReservableRow = AccountRow & { reserved?: boolean };
type Reservable = Pick<ReservableRow, 'profile' | 'problem' | 'reserved'>;

export const RESERVE_LABEL = 'Reserve for me';
export const RESERVE_TOOLTIP = 'Kept for your own sessions; the orchestrator and scripts never pick it';

/** The row's switch: shown on a profile that works (an own login has no
 *  profile to flag, a broken entry no switches), on when it is reserved. */
export function reserveSwitch(r: Reservable) {
  return { shown: r.profile && !r.problem, on: !!r.reserved, label: RESERVE_LABEL, tooltip: RESERVE_TOOLTIP };
}

/** The menu row's words: it says what it does now. */
export const reserveMenuLabel = (r: Pick<Reservable, 'reserved'>) => (r.reserved ? 'Stop reserving' : RESERVE_LABEL);

/** The account menu with the reserve row put in, ahead of Edit (after the
 *  registry's change rows); none on a row the switch is not shown on. */
export function withReserveRow(items: MenuEntry[], r: Reservable, run: () => void): MenuEntry[] {
  if (!reserveSwitch(r).shown) return items;
  const at = items.findIndex((i) => typeof i === 'object' && 'label' in i && i.label === 'Edit\u2026');
  const out = [...items];
  out.splice(at < 0 ? out.length : at, 0, { label: reserveMenuLabel(r), run, title: RESERVE_TOOLTIP });
  return out;
}

/** Flag or unflag the profile `name` in its config. */
export const setReserved = (name: string, reserved: boolean) => invoke<null>('profile_set_reserved', { name, reserved });
