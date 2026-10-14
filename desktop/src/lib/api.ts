// The window's half of the bridge: the model the Rust side mirrors from the
// daemon, and typed wrappers over its commands. The wire format to the
// daemon lives in Rust (nebula-core); nothing here speaks MessagePack.

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type AgentStatus =
  | 'fresh'
  | 'running'
  | 'finished'
  | 'needs_feedback'
  | 'terminated'
  | 'disconnected';

export interface Project {
  id: string;
  name: string;
  repo_path: string;
  sort_order: number;
}
export interface Worktree {
  id: string;
  project_id: string;
  path: string;
  branch: string;
  is_main: boolean;
  sort_order: number;
}
export interface PromptEntry {
  text: string;
  submitted_at: number;
}
export interface Agent {
  id: string;
  worktree_id: string;
  name: string;
  status: AgentStatus;
  archived: boolean;
  unseen: boolean;
  status_changed_at: number;
  kind: string;
  custom_harness: string | null;
  model: string | null;
  effort?: string | null;
  profile: string | null;
  alive: boolean;
  recent_prompts: PromptEntry[];
  role: string;
  sort_order: number;
  /** The session that started this one with `kete spawn`; null for a root. */
  parent: string | null;
}
/** A Claude in-session subagent, hung off the session it runs inside. */
export interface Subagent {
  /** Unique within its parent session only. */
  id: string;
  parent_agent: string;
  /** `general-purpose`, `Explore`, a custom agent; empty when unknown. */
  kind: string;
  /** The task the parent gave it; empty when unknown. */
  description: string;
  started_at: number;
  ended_at: number | null;
  status: 'running' | 'done' | 'failed' | 'ended_unknown';
}
export interface TerminalTab {
  id: string;
  worktree_id: string;
  name: string;
  sort_order: number;
  alive: boolean;
  run_command: string | null;
}
export interface WorktreePorts {
  worktree: string;
  block: { first: number; size: number } | null;
  live: number[];
}
export interface PrInfo {
  number: number;
  url: string;
  title: string;
  draft: boolean;
}
/** One measured plan limit (`nebula_core::usage::UsageWindow`). */
export interface UsageWindow {
  kind: 'session' | 'weekly' | 'model_weekly' | 'monthly';
  /** The model or allowance where the kind alone doesn't say. */
  label: string | null;
  percent: number;
  /** Epoch seconds. */
  resets_at: number | null;
}
export type UsageSource = 'statusline' | 'transcript' | 'rollout' | 'copilot_api' | 'copilot_events' | 'screen';
/** An account's latest plan usage, keyed by profile name or `@<harness>`. */
export interface AccountUsage {
  account: string;
  harness: string;
  windows: UsageWindow[];
  source: UsageSource;
  /** Epoch ms. */
  at: number;
  plan: string | null;
}
/** A session's context window. */
export interface ContextUsage {
  percent: number | null;
  tokens: number | null;
  window: number | null;
  compactions: number;
  last_compact_at: number | null;
  source: UsageSource;
  at: number;
}
/** One account in the rail (`accounts_list`). */
export interface AccountRow {
  key: string;
  name: string;
  harness: string;
  /** False for a harness's own login, which has no profile. */
  profile: boolean;
  default: boolean;
  disabled: boolean;
  useDefault: boolean;
  login: 'logged_in' | 'needs_login' | 'unknown';
  loginHint: string | null;
  usage: AccountUsage | null;
}
export interface Model {
  connected: boolean;
  error: string | null;
  daemon_pid: number;
  protocol_version: number;
  runtime_dir: string;
  projects: Project[];
  worktrees: Worktree[];
  agents: Agent[];
  terminals: TerminalTab[];
  ports: WorktreePorts[];
  prs: Record<string, PrInfo>;
  subagents: Subagent[];
  /** Plan usage by account key (a 55+ daemon pushes it). */
  accounts?: Record<string, AccountUsage>;
  /** Context by agent id. */
  contexts?: Record<string, ContextUsage>;
}

/** `SessionRef` exactly as serde spells it: externally tagged. */
export type SessionRef = { Agent: string } | { Terminal: string };

export type PtyEvent =
  | { kind: 'scrollback'; data: string }
  | { kind: 'output'; data: string }
  | { kind: 'exited'; code: number | null }
  | { kind: 'refused'; message: string }
  | { kind: 'kitty_flags'; flags: number };

export interface DiffFile {
  path: string;
  from: string | null;
  status: 'M' | 'A' | 'D' | 'R' | 'U' | 'C';
  added: number | null;
  removed: number | null;
}
export interface DiffPair {
  original: string;
  modified: string;
  note: string | null;
}
/** A harness's model and effort pickers, from the Agents tab's lists. */
export interface LaunchChoices {
  models: string[];
  /** The configured model ('default' = the CLI's pick). */
  model: string;
  /** Efforts by model; empty where the harness has none. */
  efforts: Record<string, string[]>;
  /** The configured effort ('n/a' where the harness has none). */
  effort: string;
}
export interface HarnessInfo {
  id: string;
  label: string;
  choices?: LaunchChoices;
}
export interface Launchables {
  harnesses: HarnessInfo[];
  profiles: { name: string; harness: string; default: boolean }[];
  /** The Agents tab's quick prompt harness; empty from an old backend. */
  default_harness?: string;
  /** Quick prompt starts in a new worktree (Sessions tab). */
  new_worktree?: boolean;
  /** Open the session a launch starts (Sessions tab). */
  follow?: boolean;
  /** Archive asks first (Confirm on archive). */
  ask_before_archive?: boolean;
}
/** An agent preset (agent_presets.json, shared with the terminal app). */
export interface Preset {
  name: string;
  harness: string;
  model: string | null;
  effort: string | null;
  prefix: string;
  postfix: string;
  skipTask: boolean;
  spec?: string;
  usable?: boolean;
}
export interface ProfileRow {
  name: string;
  harness: string;
  default: boolean;
  useDefault: boolean;
  envKeys: string[];
  home: string | null;
  problem: string | null;
  login: string | null;
  usage: string | null;
}
export interface ProfileForm {
  name: string;
  harness: string;
  default: boolean;
  useDefault: boolean;
  /** KEY=VALUE lines. */
  env: string;
}
export interface ProfileSaved {
  rows: ProfileRow[];
  login: string | null;
  home: string | null;
}
export interface QuickLaunch {
  project: string;
  worktree: string;
  newWorktree: boolean;
  /** A harness id, or 'terminal'. */
  kind: string;
  profile?: string | null;
  prompt: string;
  model?: string | null;
  effort?: string | null;
  preset?: string | null;
}
export interface Launched {
  worktree: string;
  agent: string | null;
  terminal: string | null;
  /** The account spread's pick, when the launch named no profile. */
  picked?: string | null;
}

/** Where a settings write lands, as the Rust side's `schema::Target`. */
export type SettingTarget =
  | { scope: 'global'; key: string }
  | { scope: 'agent'; harness: string; field: string }
  | { scope: 'project'; path: string; field: string }
  | { scope: 'desktop'; key: string };
export type SettingControl =
  | { type: 'toggle' }
  | { type: 'choice'; options: string[]; labels: string[] }
  | { type: 'text'; placeholder: string };
export interface SettingRow {
  target: SettingTarget;
  label: string;
  hint: string;
  control: SettingControl;
  value: boolean | string;
  default: boolean | string;
  restart: string | null;
  is_new: boolean;
}
export interface SettingGroup {
  title: string;
  subtitle: string | null;
  terminal: boolean;
  rows: SettingRow[];
}
export interface HotkeyRow {
  id: string;
  label: string;
  hint: string;
  group: string;
  defaults: string[];
  bound: string[];
  overridden: boolean;
}
export interface SettingsTab {
  id: string;
  title: string;
  note: string | null;
  groups: SettingGroup[];
  hotkeys: HotkeyRow[];
}
export interface SettingsSchema {
  tabs: SettingsTab[];
}
export interface DesktopPrefs {
  theme: string;
  diff_mode: string;
  home: string;
}
export interface Startup {
  view: string | null;
  session: string | null;
  theme: string | null;
  diff_mode: string | null;
}

export const api = {
  state: () => invoke<Model>('get_state'),
  onState: (f: (m: Model) => void) => listen<Model>('kete://state', (e) => f(e.payload)),
  retry: () => invoke<void>('retry_connect'),
  attach: (session: SessionRef, cols: number, rows: number, onEvent: Channel<PtyEvent>) =>
    invoke<void>('attach', { session, cols, rows, onEvent }),
  detach: (session: SessionRef) => invoke<void>('detach', { session }),
  input: (session: SessionRef, data: string, binary = false) =>
    invoke<void>('input', { session, data, binary }),
  resize: (session: SessionRef, cols: number, rows: number) =>
    invoke<void>('resize', { session, cols, rows }),
  markSeen: (agent: string) => invoke<void>('mark_seen', { agent }),
  createAgent: (spec: {
    worktree: string;
    kind: string;
    profile?: string | null;
    prompt?: string;
    model?: string | null;
    effort?: string | null;
  }) => invoke<string>('create_agent', { spec }),
  quickLaunch: (spec: QuickLaunch) => invoke<Launched>('quick_launch', { spec }),
  // Session verbs
  renameAgent: (agent: string, name: string) => invoke<void>('rename_agent', { agent, name }),
  archiveAgent: (agent: string) => invoke<void>('archive_agent', { agent }),
  unarchiveAgent: (agent: string) => invoke<void>('unarchive_agent', { agent }),
  deleteAgent: (agent: string) => invoke<void>('delete_agent', { agent }),
  restartAgent: (agent: string) => invoke<void>('restart_agent', { agent }),
  moveAgent: (agent: string, worktree: string) => invoke<void>('move_agent', { agent, worktree }),
  renameTerminal: (terminal: string, name: string) => invoke<void>('rename_terminal', { terminal, name }),
  closeTerminal: (terminal: string) => invoke<void>('close_terminal', { terminal }),
  followUp: (agent: string, text: string) => invoke<void>('follow_up', { agent, text }),
  // Projects and worktrees
  suggestBranch: (project: string) => invoke<string>('suggest_branch', { project }),
  createWorktree: (project: string, branch: string, base: string | null) =>
    invoke<string>('create_worktree', { project, branch, base }),
  deleteWorktree: (worktree: string, force: boolean) => invoke<void>('delete_worktree', { worktree, force }),
  renameProject: (project: string, name: string) => invoke<void>('rename_project', { project, name }),
  removeProject: (project: string) => invoke<void>('remove_project', { project }),
  setProjectPath: (project: string, path: string) => invoke<void>('set_project_path', { project, path }),
  openWorktree: (worktree: string) => invoke<string>('open_worktree', { worktree }),
  openRemote: (worktree: string) => invoke<string>('open_remote', { worktree }),
  // Presets and profiles
  presets: () => invoke<Preset[]>('presets_list'),
  presetSave: (original: string | null, preset: Preset) => invoke<Preset[]>('preset_save', { original, preset }),
  presetDelete: (name: string) => invoke<Preset[]>('preset_delete', { name }),
  profiles: () => invoke<ProfileRow[]>('profiles_list'),
  profileHarnesses: () => invoke<string[]>('profile_harnesses'),
  profileAdd: (form: ProfileForm) => invoke<ProfileSaved>('profile_add', { form }),
  profileUpdate: (form: ProfileForm, keepEnv: boolean) => invoke<ProfileSaved>('profile_update', { form, keepEnv }),
  profileRemove: (name: string) => invoke<ProfileSaved>('profile_remove', { name }),
  profileSetDefault: (name: string) => invoke<ProfileRow[]>('profile_set_default', { name }),
  accounts: () => invoke<AccountRow[]>('accounts_list'),
  profileSetEnabled: (name: string, enabled: boolean) => invoke<null>('profile_set_enabled', { name, enabled }),
  createTerminal: (worktree: string) => invoke<string>('create_terminal', { worktree }),
  addProject: (path: string) => invoke<string>('add_project', { path }),
  startRun: (worktree: string) => invoke<string>('start_run', { worktree }),
  stopRun: (worktree: string) => invoke<void>('stop_run', { worktree }),
  /** Worktree id to the command its Run button starts; absent: none. */
  runCommands: () => invoke<Record<string, string>>('run_commands'),
  settingsSchema: () => invoke<SettingsSchema>('settings_schema'),
  settingsSet: (target: SettingTarget, value: boolean | string) => invoke<void>('settings_set', { target, value }),
  settingsReset: () => invoke<void>('settings_reset'),
  desktopPrefs: () => invoke<DesktopPrefs>('desktop_prefs'),
  diffFiles: (worktree: string) => invoke<DiffFile[]>('diff_files', { worktree }),
  diffPair: (worktree: string, file: DiffFile) => invoke<DiffPair>('diff_pair', { worktree, file }),
  openUrl: (url: string) => invoke<void>('open_url', { url }),
  launchables: () => invoke<Launchables>('launchables'),
  startup: () => invoke<Startup>('startup'),
};

export { Channel };

export function decodeB64(data: string): Uint8Array {
  const bin = atob(data);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
