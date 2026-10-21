import { toast } from '../store.svelte';
import { focusInput } from './triage.svelte';

/** Put the cursor in the triage strip. */
export function focusTriage(): void {
  if (!focusInput()) toast('Triage: start an orchestrator or a triage desk first');
}
