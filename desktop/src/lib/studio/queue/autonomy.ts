// Owner: the queue lane. 0.60 foundation placeholder: it fills this in.
import { toast } from '../../store.svelte';

/** Open the autonomy control, on `repo` (null: the repo in view). */
export function openAutonomy(repo: string | null): void {
  void repo;
  toast('Autonomy: not in this build yet');
}
