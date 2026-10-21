// Owner: the queue lane.
import { tick } from 'svelte';
import { nav } from '../../shell/history.svelte';

/** Open the autonomy control, on `repo` (null: the repo in view). */
export function openAutonomy(repo: string | null): void {
  nav.go({ view: 'studio', repo: repo ?? nav.current.repo });
  void tick().then(() => document.querySelector<HTMLElement>('[data-autonomy-control]')?.focus());
}
