// The words for a usage window's reset, from the one formatter in Rust
// (`nebula_core::usage_window::format`, in the OS time zone). The view asks
// for the labels of the windows it holds when they change and each tick;
// components read them synchronously by `labelFor`. Nothing here words a
// time itself.

import { api, type UsageLabel, type UsageWindow } from '../api';

const cache = $state<{ map: Record<string, UsageLabel> }>({ map: {} });

/** What identifies a window's reading, so a new reading asks again. */
export function windowKey(w: UsageWindow): string {
  return `${w.kind}|${w.label ?? ''}|${w.percent}|${w.resets_at ?? ''}|${w.read_at}|${w.estimated ? 1 : 0}`;
}

/** The label last worked out for this window, or null before the first. */
export function labelFor(w: UsageWindow | null | undefined): UsageLabel | null {
  return w ? (cache.map[windowKey(w)] ?? null) : null;
}

/** Put labels in by hand (tests; the real source is `refreshLabels`). */
export function setLabels(pairs: [UsageWindow, UsageLabel][]) {
  cache.map = Object.fromEntries(pairs.map(([w, l]) => [windowKey(w), l]));
}

let inflight = false;
/** Ask Rust for the labels of `windows` as of `nowMs`. One call at a time;
 *  a failure keeps the last labels. */
export async function refreshLabels(windows: UsageWindow[], nowMs: number) {
  if (inflight || !windows.length) return;
  inflight = true;
  try {
    const keyed = new Map(windows.map((w) => [windowKey(w), w]));
    const list = [...keyed.values()];
    const labels = await api.usageLabels(list, Math.floor(nowMs / 1000));
    cache.map = Object.fromEntries(list.map((w, i) => [windowKey(w), labels[i]]));
  } catch {
    /* keep the last words */
  } finally {
    inflight = false;
  }
}
