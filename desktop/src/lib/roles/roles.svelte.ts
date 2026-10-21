// The roles each repo defines, for the pickers: `api.rolesList` once per
// repo, kept for a short while so reopening the quick prompt is free yet a
// committed roles file shows up soon after.

import { api } from '../api';
import type { RoleRow } from '../studio/types';

export interface RepoRoles {
  rows: RoleRow[];
  loading: boolean;
  /** Why the roles file could not be read (it is invalid); null when it could. */
  error: string | null;
  /** Epoch ms of the last answer. */
  at: number;
}

/** How long an answer is kept, ms. */
const KEEP = 30_000;

/** By project id. */
export const roles = $state<Record<string, RepoRoles>>({});

/** Fetch `repo`'s roles unless a fresh answer is held (`force` asks again). */
export async function loadRoles(repo: string, force = false): Promise<void> {
  const have = roles[repo];
  if (!repo || have?.loading) return;
  if (!force && have && !have.error && Date.now() - have.at < KEEP) return;
  roles[repo] = { rows: have?.rows ?? [], loading: true, error: null, at: have?.at ?? 0 };
  try {
    roles[repo] = { rows: await api.rolesList(repo), loading: false, error: null, at: Date.now() };
  } catch (e) {
    roles[repo] = { rows: [], loading: false, error: String(e), at: Date.now() };
  }
}

/** A picker option: `Runner \u00b7 haiku \u00b7 allowlist`. */
export function roleOption(r: RoleRow): string {
  return `${r.label} \u00b7 ${r.model ?? 'default model'} \u00b7 ${r.permission}`;
}

/** A role's tooltip: `Runner: allowlist, tier 3`. */
export function roleTitle(r: { label: string; permission: string; tier: number }): string {
  return `${r.label}: ${r.permission}, tier ${r.tier}`;
}
