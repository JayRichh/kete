// Owner: the renew lane. 0.60 foundation placeholder: it fills this in.
import { toast } from '../store.svelte';
import type { RenewEvent } from '../studio/types';

/** Open the renew form for the orchestrator. */
export function openRenew(): void {
  toast('Renew: not in this build yet');
}

/** The Tauri event `orchestrator-renewal`, handed over by App.svelte. */
export function onRenewal(ev: RenewEvent): void {
  void ev;
}
