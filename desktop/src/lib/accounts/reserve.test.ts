import { describe, expect, it } from 'vitest';
import type { MenuItem } from '../ui/ui.svelte';
import { reserveMenuLabel, reserveSwitch, withReserveRow, type ReservableRow } from './reserve';

const row = (over: Partial<ReservableRow> & { key: string }): ReservableRow => ({
  name: over.key.startsWith('@') ? over.key.slice(1) : over.key,
  harness: 'claude',
  profile: !over.key.startsWith('@'),
  default: false,
  disabled: false,
  useDefault: false,
  login: 'logged_in',
  loginHint: null,
  usage: null,
  ...over,
});

describe('the Reserve for me switch', () => {
  it('shows on a profile that works, not on an own login or a broken entry', () => {
    expect(reserveSwitch(row({ key: 'work' })).shown).toBe(true);
    expect(reserveSwitch(row({ key: '@claude' })).shown).toBe(false);
    expect(reserveSwitch(row({ key: '!0', problem: 'The name may only use letters' })).shown).toBe(false);
  });
  it('is on when the row is reserved, off when it is not or says nothing', () => {
    expect(reserveSwitch(row({ key: 'work', reserved: true })).on).toBe(true);
    expect(reserveSwitch(row({ key: 'work', reserved: false })).on).toBe(false);
    expect(reserveSwitch(row({ key: 'work' })).on).toBe(false);
  });
  it('says its label and tooltip in the spec words', () => {
    const s = reserveSwitch(row({ key: 'work' }));
    expect(s.label).toBe('Reserve for me');
    expect(s.tooltip).toBe('Kept for your own sessions; the orchestrator and scripts never pick it');
  });
  it('the menu row flips its words with the state', () => {
    expect(reserveMenuLabel(row({ key: 'work' }))).toBe('Reserve for me');
    expect(reserveMenuLabel(row({ key: 'work', reserved: true }))).toBe('Stop reserving');
  });
});

describe('the reserve row in the account menu', () => {
  const run = () => {};
  const menu: MenuItem[] = [{ label: 'Switch on', run }, { label: 'Edit\u2026', run }, { label: 'Remove\u2026', run }];
  const labels = (items: ReturnType<typeof withReserveRow>) => items.map((i) => (typeof i === 'object' && 'label' in i ? i.label : String(i)));

  it('stands ahead of Edit, saying what it does now', () => {
    expect(labels(withReserveRow(menu, row({ key: 'work' }), run))).toEqual(['Switch on', 'Reserve for me', 'Edit\u2026', 'Remove\u2026']);
    expect(labels(withReserveRow(menu, row({ key: 'work', reserved: true }), run))[1]).toBe('Stop reserving');
  });
  it('is absent on an own login and on a broken entry', () => {
    expect(withReserveRow(menu, row({ key: '@claude' }), run)).toEqual(menu);
    expect(withReserveRow(menu, row({ key: '!0', problem: 'bad' }), run)).toEqual(menu);
  });
});
