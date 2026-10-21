// Studio mode's shapes, as the Rust side sends them (nebula-core
// `studio::wire` and `studio::settings`, serde snake_case). Mirror a
// change there here. Pure: no imports, so a test reads it without Tauri.

/** Hold, Plan, Deliver, Release. */
export type Autonomy = 'A0' | 'A1' | 'A2' | 'A3';

/** What this machine allows one repo (`config.local.json`, key `studio`). */
export interface StudioSettings {
  studio_inbox: string;
  autonomy: Autonomy;
  queue_exclude: string[];
}

export interface QueueNote {
  id: string;
  title: string;
  to: string;
  /** `high`, `normal` or `low`. */
  priority: string;
  /** Repo-relative. */
  path: string;
}

export interface StudioQueueRow {
  /** The project id. */
  project: string;
  name: string;
  autonomy: Autonomy;
  inbox: string;
  open: number;
  to_jay: number;
  unannounced: number;
  next: QueueNote[];
  /** Why the runner is holding; null when it would send now. */
  held: string | null;
  /** Epoch ms. */
  last_note_at: number | null;
}

/** serde's default enum shape: unit variants are strings. */
export type RenewState = 'Scheduled' | 'Done' | { Failed: { reason: string } };

export interface SessionRole {
  agent: string;
  card: string;
  label: string;
  /** 0 to 3. */
  tier: number;
  department: string | null;
  /** `prompt`, `allowlist` or `unattended`. */
  permission: string;
  /** Tokens. */
  context_budget: number | null;
}

export interface SpendRow {
  agent: string;
  /** Local date, `YYYY-MM-DD`. */
  day: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** A list-price estimate, not a bill. */
  cost_usd: number;
  tool_uses: number;
  turns: number;
}

export interface LedgerRow {
  spend: SpendRow;
  session: string;
  project: string;
  branch: string | null;
  role_card: string | null;
  profile: string | null;
  model: string | null;
  status: string;
  pr_url: string | null;
}

/** One project as `studio_repos` reports it. */
export interface StudioRepo {
  /** The project id. */
  repo: string;
  name: string;
  /** The repo has `.kete/studio.toml`. */
  manifest: boolean;
  /** This machine has a `studio` entry for it. */
  enabled: boolean;
  autonomy: Autonomy | null;
}

export interface RoleRow {
  id: string;
  label: string;
  model: string | null;
  permission: string;
  tier: number;
  department: string | null;
}

/** The Tauri event `orchestrator-renewal`. */
export interface RenewEvent {
  old: string;
  new: string | null;
  state: RenewState;
}

/** The repos that have studio mode: a manifest or a machine entry. */
export function studioOnly(rows: StudioRepo[]): StudioRepo[] {
  return rows.filter((r) => r.manifest || r.enabled);
}

/** Whether the rail shows Studio and Gauntlet: once any repo has studio mode. */
export function showStudioRail(repos: StudioRepo[]): boolean {
  return repos.length > 0;
}

/** `<kind>/<id>`, the gauntlet view's place item. */
export function gauntletItem(kind: string, id: string): string {
  return `${kind}/${id}`;
}

/** The inverse; null for an item that is not `<kind>/<id>`. */
export function parseGauntletItem(item: string | undefined): { kind: string; id: string } | null {
  if (!item) return null;
  const cut = item.indexOf('/');
  if (cut <= 0 || cut === item.length - 1) return null;
  return { kind: item.slice(0, cut), id: item.slice(cut + 1) };
}
