// Where the window is, where it has been, and what each view was showing.
// Back and forward walk a browser-style stack of places; every view's UI
// state (filters, selections) lives here per repo, so going back lands on
// the view as it was left. View state is also saved to ado.json, after a
// pause, through whatever saver the app installs.

/** Every view the window can be on. `place.item` on `gauntlet` is `<kind>/<id>`. */
export const VIEWS = [
  'overview',
  'board',
  'session',
  'diff',
  'files',
  'tasks',
  'branches',
  'prs',
  'settings',
  'orchestrator',
  'worktrees',
  'accounts',
  'whakaoho',
  'studio',
  'gauntlet',
] as const;
export type View = (typeof VIEWS)[number];
export type Place = { view: View; repo?: string; item?: string };
export type UiState = Record<string, unknown>;
export type Saver = (view: string, repo: string, state: UiState) => void;

export interface NavOptions {
  save?: Saver;
  /** How long a view's state must stay still before it is saved. */
  debounceMs?: number;
  home?: Place;
}

/** Most places kept behind the current one. */
const MAX_STACK = 200;

function same(a: Place, b: Place): boolean {
  return a.view === b.view && (a.repo ?? '') === (b.repo ?? '') && (a.item ?? '') === (b.item ?? '');
}

/** A new reactive view-state object. */
function fresh(): UiState {
  const s: UiState = $state({});
  return s;
}

function plain(state: UiState): UiState {
  return JSON.parse(JSON.stringify(state)) as UiState;
}

export function createNav(opts: NavOptions = {}) {
  const home: Place = opts.home ?? { view: 'overview' };
  const debounceMs = opts.debounceMs ?? 400;
  let saver: Saver | undefined = opts.save;

  let stack = $state<Place[]>([home]);
  let index = $state(0);
  // Keyed `view\0repo`. Each entry is its own $state object, made on first
  // read: views re-render on change, and a first read inside a $derived
  // never mutates state that derived depends on.
  const states = new Map<string, UiState>();
  const handles = new Map<string, UiState>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  const keyOf = (view: string, repo: string) => `${view}\u0000${repo}`;

  function schedule(view: string, repo: string) {
    if (!saver) return;
    const k = keyOf(view, repo);
    clearTimeout(timers.get(k));
    timers.set(
      k,
      setTimeout(() => {
        timers.delete(k);
        saver?.(view, repo, plain(states.get(k) ?? {}));
      }, debounceMs),
    );
  }

  function raw(view: string, repo: string): UiState {
    const k = keyOf(view, repo);
    let s = states.get(k);
    if (!s) {
      s = fresh();
      states.set(k, s);
    }
    return s;
  }

  return {
    get current(): Place {
      return stack[index];
    },
    get canBack(): boolean {
      return index > 0;
    },
    get canForward(): boolean {
      return index < stack.length - 1;
    },
    back() {
      if (index > 0) index -= 1;
    },
    forward() {
      if (index < stack.length - 1) index += 1;
    },
    /** Go somewhere new: whatever was forward of here is dropped. */
    go(p: Place) {
      if (same(p, stack[index])) return;
      const next = [...stack.slice(0, index + 1), { ...p }];
      stack = next.length > MAX_STACK ? next.slice(next.length - MAX_STACK) : next;
      index = stack.length - 1;
    },
    /** Change the current place without adding history. */
    replace(p: Place) {
      stack[index] = { ...p };
    },
    /** The view's state for a repo. Setting a top-level key saves it. */
    ui(view: string, repo: string): UiState {
      const k = keyOf(view, repo);
      let h = handles.get(k);
      if (!h) {
        const target = raw(view, repo);
        h = new Proxy(target, {
          set(t, prop, value) {
            Reflect.set(t, prop, value);
            if (typeof prop === 'string') schedule(view, repo);
            return true;
          },
          deleteProperty(t, prop) {
            Reflect.deleteProperty(t, prop);
            if (typeof prop === 'string') schedule(view, repo);
            return true;
          },
        });
        handles.set(k, h);
      }
      return h;
    },
    /** State saved earlier for `repo`, keyed by view. Not saved back. */
    seed(repo: string, saved: Record<string, unknown> | null | undefined) {
      if (!saved || typeof saved !== 'object') return;
      for (const [view, value] of Object.entries(saved)) {
        if (!value || typeof value !== 'object' || Array.isArray(value)) continue;
        const target = raw(view, repo);
        for (const [k, v] of Object.entries(value)) if (!(k in target)) target[k] = v;
      }
    },
    setSaver(s: Saver | undefined) {
      saver = s;
    },
  };
}

export type Nav = ReturnType<typeof createNav>;

/** The window's one history. */
export const nav = createNav();
