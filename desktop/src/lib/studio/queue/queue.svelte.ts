// The studio queue header's logic: autonomy levels, the runner's state in
// words, and a small store that polls the daemon while the header is up.

import { api } from '../../api';
import { toast } from '../../store.svelte';
import type { Autonomy, StudioQueueRow } from '../types';

export const LEVELS: { level: Autonomy; name: string }[] = [
  { level: 'A0', name: 'Hold' },
  { level: 'A1', name: 'Plan' },
  { level: 'A2', name: 'Deliver' },
  { level: 'A3', name: 'Release' },
];

/** What each level lets the repo's sessions do, for the tooltips. */
export const ALLOWS: Record<Autonomy, string> = {
  A0: 'nothing runs and no idle notes go out',
  A1: 'sessions plan and write notes, no pushes',
  A2: 'sessions may push and open pull requests',
  A3: 'sessions may also tag and release',
};

const rank = (a: Autonomy) => LEVELS.findIndex((l) => l.level === a);

/** Raising to A2 or A3 asks first; lowering never does. */
export function needsConfirm(from: Autonomy, to: Autonomy): boolean {
  return rank(to) > rank(from) && rank(to) >= 2;
}

export function confirmText(repoName: string, to: Autonomy): string {
  const name = LEVELS[rank(to)].name;
  const what = to === 'A3' ? 'tag and release' : 'push and open pull requests';
  return `Raise ${repoName} to ${to} (${name})? Its sessions may ${what}.`;
}

const hhmm = (ms: number) => {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
};

/** The runner's state in one line. `fmt` is for tests: epoch ms to a clock time. */
export function stateLine(row: StudioQueueRow | null, _nowMs: number, fmt: (ms: number) => string = hhmm): string {
  if (!row) return 'Studio is off for this repo. Run kete studio enable to turn it on';
  const n = row.unannounced;
  const head = row.held ? `Holding: ${row.held}` : `Ready: ${n} new ${n === 1 ? 'note' : 'notes'} for the next idle note`;
  return row.last_note_at != null ? `${head} \u00b7 last idle note at ${fmt(row.last_note_at)}` : head;
}

const rowFor = (rows: StudioQueueRow[], repo: string) => rows.find((r) => r.project === repo) ?? null;

/** Set the level and read the row back. Null when it failed (a toast says why). */
export async function setAutonomy(repo: string, row: StudioQueueRow, to: Autonomy): Promise<StudioQueueRow | null> {
  try {
    // An empty exclude list keeps the exclusions already on disk (the backend merges).
    await api.studioSet(repo, { studio_inbox: row.inbox, autonomy: to, queue_exclude: [] });
    return rowFor(await api.studioQueue(repo, false), repo);
  } catch (e) {
    toast(`Could not set autonomy: ${e}`);
    return null;
  }
}

const POLL_MS = 10_000;

/** Call during component init: polls `repo()` every 10 s until the component unmounts. */
export function queueState(repo: () => string | null) {
  let row = $state<StudioQueueRow | null>(null);
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let sending = $state(false);

  /** Drop an answer that arrives after the repo changed. */
  const take = (r: string, next: StudioQueueRow | null) => {
    if (r !== repo()) return;
    row = next;
    error = null;
    loaded = true;
  };
  const fail = (what: string, e: unknown) => {
    error = `${what}: ${e}`;
    toast(error);
  };

  async function refresh() {
    const r = repo();
    if (!r) return;
    try {
      take(r, rowFor(await api.studioQueue(r, false), r));
    } catch (e) {
      fail('Could not read the queue', e);
    }
  }

  $effect(() => {
    const r = repo();
    row = null;
    error = null;
    loaded = false;
    if (!r) return;
    void refresh();
    const id = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(id);
  });

  return {
    get row() {
      return row;
    },
    get error() {
      return error;
    },
    get loaded() {
      return loaded;
    },
    get sending() {
      return sending;
    },
    refresh,
    async sendNow() {
      const r = repo();
      if (!r || sending) return;
      const before = row?.last_note_at ?? null;
      sending = true;
      try {
        const next = rowFor(await api.studioQueue(r, true), r);
        take(r, next);
        if (next && next.last_note_at !== before) toast('Idle note sent');
        else if (next?.held) toast(`Not sent: ${next.held}`);
      } catch (e) {
        fail('Could not send the idle note', e);
      } finally {
        sending = false;
      }
    },
    async setLevel(to: Autonomy) {
      const r = repo();
      if (!r || !row) return;
      const next = await setAutonomy(r, row, to);
      if (next) take(r, next);
    },
  };
}
