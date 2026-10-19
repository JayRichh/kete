// The sidebar's live state: its width, whether it is folded to the icon
// rail, and whether the folded rail is peeking. Width and fold are saved
// under `desktop` in config.json (per profile, as the rest of it is), a
// moment after the last change so a drag writes once.

import { api } from '../api';
import { toast } from '../store.svelte';
import { SIDEBAR_DEFAULT, clampSidebar, savedSidebar } from './sidebar';

export const sidebar = $state({
  width: SIDEBAR_DEFAULT,
  collapsed: false,
  /** The folded rail shows the full sidebar over the content. */
  peeking: false,
});

let saveTimer: ReturnType<typeof setTimeout> | undefined;
function save() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    // The default is stored as 0: a width never moved is never written.
    const width = sidebar.width === SIDEBAR_DEFAULT ? 0 : sidebar.width;
    api.desktopSetSidebar(width, sidebar.collapsed).catch((e) => toast(`Could not save the sidebar: ${e}`));
  }, 400);
}

/** The saved width and fold, at startup. */
export function loadSidebar(prefs: { sidebar_width?: number; sidebar_collapsed?: boolean } | null | undefined) {
  sidebar.width = savedSidebar(prefs?.sidebar_width);
  sidebar.collapsed = prefs?.sidebar_collapsed === true;
}

export function setSidebarWidth(width: number) {
  sidebar.width = clampSidebar(width);
  save();
}

export function setSidebarCollapsed(on: boolean) {
  sidebar.collapsed = on;
  sidebar.peeking = false;
  save();
}

export function toggleSidebar() {
  setSidebarCollapsed(!sidebar.collapsed);
}
