// The window's transient layers: the context menu, the in-window dialog
// (a prompt, a confirm or a pick list), the command palette and the
// keyboard help. One of each at a time; Esc closes the top one.

import { sheet as files } from '../files/files.svelte';
import { addSheet } from '../accounts/add.svelte';
import { shipSheet } from '../ship/state.svelte';
import { learn } from '../learn.svelte';
import { logSheet } from '../critters/sheet.svelte';

export type MenuItem = {
  label: string;
  run: () => void;
  /** Chords from the shortcut table, shown at the right, every one. */
  keys?: string[];
  danger?: boolean;
  disabled?: boolean;
  /** Why it is greyed out, as its tooltip; on a live row, what it acts on. */
  title?: string;
  /** Kept last whatever a searchable menu's query (the verbs menu's All
   *  keys). */
  pinned?: boolean;
  /** A submenu (`Open in \u25b8`): the row opens these and runs nothing.
   *  Submenus never nest: a child's own `children` are ignored. */
  children?: MenuEntry[];
  /** A checkbox row (or a radio row with `radio`): ticked or not. */
  checked?: boolean;
  /** One of a set (View \u25b8 Overview / Board): a radio, not a checkbox. */
  radio?: boolean;
  /** The icon slot ahead of the words. */
  icon?: MenuIcon;
  /** The row's whole words outside its submenu ("Open in VS Code" for the
   *  submenu's "VS Code"): what the palette lists. */
  full?: string;
};
/** A status dot (its tone a theme colour), an image (an editor's icon, a
 *  file's), or a glyph (an account's harness letter). */
export type MenuIcon =
  | { dot: 'ember' | 'ink' | 'muted' | 'faint' | 'done' | 'fail' | 'merged' | 'unread' }
  | { src: string }
  | { glyph: string };
/** A heading: small caps, faint; `ember` for a state that needs you (a
 *  menu's state banner, \u00a71.7 of the menu matrix). */
export type MenuHeading = { heading: string; ember?: boolean };
export type MenuEntry = MenuItem | MenuHeading | 'sep';

export type Field = {
  key: string;
  label: string;
  value?: string;
  placeholder?: string;
  mono?: boolean;
  multiline?: boolean;
};

export type DialogSpec =
  | {
      kind: 'prompt';
      title: string;
      note?: string;
      fields: Field[];
      confirm: string;
      /** A second send beside the confirm (Start as new session), on
       *  Ctrl+Shift+Enter: the answer then carries `ALT_CHOICE`. */
      alt?: { label: string };
      resolve: (v: Record<string, string> | null) => void;
    }
  | {
      kind: 'confirm';
      title: string;
      message: string;
      /** Extra lines (names going away, say). */
      detail?: string[];
      confirm: string;
      danger?: boolean;
      /** An opt-in box (force a delete). */
      check?: string;
      /** The box starts ticked. */
      checked?: boolean;
      resolve: (v: { ok: boolean; checked: boolean }) => void;
    }
  | {
      kind: 'pick';
      title: string;
      /** A line above the list. */
      note?: string;
      items: PickItem[];
      /** The row the cursor starts on, by id. */
      selected?: string;
      /** A button beside Cancel that answers its own id (From a file). */
      extra?: { id: string; label: string };
      resolve: (id: string | null) => void;
    };

export type PickItem = {
  id: string;
  label: string;
  hint?: string;
  /** Colour squares after the label: a theme's canvas, text and syntax. */
  swatch?: string[];
  /** A font family: the row under the cursor shows a sample in it. */
  font?: string;
};

/** The key a prompt's answer carries, set to '1', when its `alt` button
 *  (or Ctrl+Shift+Enter) sent it. */
export const ALT_CHOICE = '$alt';

export const ui = $state({
  /** `search`: a field over the rows narrows them as you type (the
   *  verbs menu, `?`). `keep`: the focus stays where it was (a field, the
   *  terminal, an editor) and the menu takes the keys from the window, as
   *  a native menu does (menus/router.ts). */
  menu: null as { x: number; y: number; items: MenuEntry[]; label: string; search?: boolean; keep?: boolean } | null,
  dialog: null as DialogSpec | null,
  palette: false,
  help: false,
  /** The review sheet (the orchestrator asked, or a session's Review). */
  review: null as ReviewSheet | null,
  /** The Start orchestrator form. */
  startOrchestrator: false,
  /** The branch switcher (worktrees/BranchSwitch.svelte). */
  branchSwitch: false,
});

export type ReviewTab = 'terminal' | 'diff' | 'history' | 'pr';
export type ReviewSheet = { sessions: string[]; at: number; tab: ReviewTab };

/** Open a context menu at the pointer (or under an element). `label`
 *  names it for a screen reader: the thing it acts on. */
export function openMenu(
  items: MenuEntry[],
  at: { x: number; y: number } | HTMLElement,
  label = 'Actions',
  opts: { search?: boolean; keep?: boolean } = {},
) {
  if (!items.length) return;
  let x: number, y: number;
  if (at instanceof HTMLElement) {
    const r = at.getBoundingClientRect();
    x = r.left;
    y = r.bottom;
  } else ({ x, y } = at);
  ui.menu = { x, y, items, label, ...(opts.search ? { search: true } : {}), ...(opts.keep ? { keep: true } : {}) };
}

export function closeMenu() {
  ui.menu = null;
}

function settle<T>(make: (resolve: (v: T) => void) => DialogSpec): Promise<T> {
  return new Promise<T>((resolve) => {
    ui.dialog = make((v: T) => {
      ui.dialog = null;
      resolve(v);
    });
  });
}

export const ask = {
  prompt: (o: { title: string; note?: string; fields: Field[]; confirm?: string; alt?: { label: string } }) =>
    settle<Record<string, string> | null>((resolve) => ({ kind: 'prompt', confirm: 'Save', ...o, resolve })),
  confirm: (o: { title: string; message: string; detail?: string[]; confirm: string; danger?: boolean; check?: string; checked?: boolean }) =>
    settle<{ ok: boolean; checked: boolean }>((resolve) => ({ kind: 'confirm', ...o, resolve })),
  pick: (o: { title: string; note?: string; items: PickItem[]; selected?: string; extra?: { id: string; label: string } }) =>
    settle<string | null>((resolve) => ({ kind: 'pick', ...o, resolve })),
};

/** Is any layer up (so page keys stand down)? */
export function layerOpen(): boolean {
  return !!(
    ui.menu ||
    ui.dialog ||
    ui.palette ||
    ui.help ||
    learn.whatsNew ||
    ui.review ||
    ui.startOrchestrator ||
    ui.branchSwitch ||
    files.open ||
    addSheet.phase === 'form' ||
    logSheet.open ||
    !!shipSheet.mode
  );
}
