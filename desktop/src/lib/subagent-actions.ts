// The subagent verbs both the board and the overview offer: flip the
// shared `hide_done_subagents` setting, and ask the daemon to drop
// finished subagent records.

import { api } from './api';
import { app, refreshLaunchables, toast } from './store.svelte';

/** The `hide_done_subagents` setting as the window last read it (on by default). */
export function foldingFinished(): boolean {
  return app.launch.hide_done_subagents !== false;
}

/** Flip `hide_done_subagents` for both apps. */
export function toggleFoldFinished() {
  api
    .settingsSet({ scope: 'global', key: 'hide_done_subagents' }, foldingFinished() ? 'shown' : 'hidden')
    .then(refreshLaunchables)
    .catch((e) => toast(`Couldn't save the setting: ${e}`));
}

/** Drop the finished subagents of session `parent`, or of every session for null. */
export function clearFinishedSubagents(parent: string | null) {
  api.clearFinishedSubagents(parent).catch((e) => toast(String(e)));
}
