import { api } from '../api';
import { toast } from '../store.svelte';

/** Write today's usage ledger as a CSV; the app shows the file in Explorer. */
export async function exportUsage(): Promise<void> {
  try {
    toast(`Usage CSV written: ${await api.usageExport()}`);
  } catch (e) {
    toast(String(e));
  }
}
