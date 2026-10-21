// Orchestrator renewal, the window's half: the form's open state, the
// notice a renewal ends in, and the two calls. Held in a plain store, not
// runes: App.svelte and actions.ts import this file by its `.ts` name.
import { get, writable } from 'svelte/store';
import { api } from '../api';
import { editorsApi } from '../editors/api';
import { app, toast } from '../store.svelte';
import { canProvision, orchestratorOf, RELOAD_HINT } from '../orchestrator';
import type { RenewEvent } from '../studio/types';

export type RenewNote = { kind: 'done'; note: string | null } | { kind: 'failed'; reason: string };

export const renewForm = writable(false);
export const renewNote = writable<RenewNote | null>(null);

/** The note the form sent, kept until the renewal ends so `Done` can name it;
 *  a renewal from the CLI has none. */
let sent: string | null = null;

/** The file name of a path, either slash. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Open the renew form for the orchestrator (or say why not). */
export function openRenew(): void {
  if (!canProvision(app.model)) return toast(app.model.connected ? RELOAD_HINT : 'Not connected to the kete daemon.');
  if (!orchestratorOf(app.model)) return toast('There is no orchestrator to renew.');
  renewForm.set(true);
}

export function closeRenew(): void {
  renewForm.set(false);
}

/** Send the renewal. Resolves to the error to show in the form, or null
 *  when the daemon took it (the form closes and the toast says so). */
export async function submitRenew(note: string, profile: string | null, keepOld: boolean, now: boolean): Promise<string | null> {
  const path = note.trim();
  // Set before the reply: a swap with `now` can end before it lands.
  sent = path;
  try {
    const state = await api.orchestratorRenew(path, profile, keepOld, now);
    if (typeof state === 'object') {
      sent = null;
      return state.Failed.reason;
    }
    renewForm.set(false);
    toast('Renewal scheduled');
    return null;
  } catch (e) {
    sent = null;
    return String(e);
  }
}

/** The Tauri event `orchestrator-renewal`, handed over by App.svelte. */
export function onRenewal(ev: RenewEvent): void {
  if (ev.state === 'Scheduled') return;
  if (ev.state === 'Done') renewNote.set({ kind: 'done', note: sent });
  else renewNote.set({ kind: 'failed', reason: ev.state.Failed.reason });
  sent = null;
}

export function dismissRenewNote(): void {
  renewNote.set(null);
}

/** Open the note in its default app; the path stays on the notice if it
 *  cannot. */
export async function openRenewNote(): Promise<void> {
  const n = get(renewNote);
  if (n?.kind !== 'done' || !n.note) return;
  try {
    await editorsApi.openDefault(null, n.note);
  } catch (e) {
    toast(`Could not open the note: ${e}`);
  }
}
