// The window's half of the bridge: the model the Rust side mirrors from the
// daemon, and typed wrappers over its commands. The wire format to the
// daemon lives in Rust (nebula-core); nothing here speaks MessagePack.

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { WakeNowResult, WakeOverview, WakeSchedule } from './wake/types';

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
  /** Absent from an older backend, which only reported open ones. */
  state?: 'open' | 'merged' | 'closed';
}

/** One measured plan limit (`nebula_core::usage::UsageWindow`). */
export interface UsageWindow {
  kind: 'session' | 'weekly' | 'model_weekly' | 'monthly';
  /** The model or allowance where the kind alone doesn't say. */
  label: string | null;
  percent: number;
  /** Epoch seconds. */
  resets_at: number | null;
  /** Epoch seconds it was read; 0 = unknown. */
  read_at: number;
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
/** Why a Claude session's last turn stopped short: waiting out a usage
 *  limit, or needing you (logged out, billing, ...), or an API error.
 *  Core's `StopNote`; `lib/stop.ts` turns it into words and a tone. */
export interface StopNote {
  /** Claude's `StopFailure` error (`rate_limit`, `billing_error`, ...) or
   *  the notification behind the note (`quota_auto_resume_stale`). Free
   *  text: a value this build has never heard of reads as an API error. */
  reason: string;
  /** Claude is waiting out a usage limit and continues on its own. */
  waiting: boolean;
  /** The reset as Claude printed it (`3:45pm`), already local time. */
  resumes: string | null;
  /** Epoch seconds the account's limit resets, when the screen said nothing. */
  resets_at: number | null;
  /** Claude's own words, for a tooltip. */
  detail: string | null;
  /** Epoch ms it was noted. */
  at: number;
}
/** One account in the rail (`accounts_list`). */
export interface AccountRow {
  key: string;
  name: string;
  harness: string;
  /** False for a harness's own login, which has no profile. */
  profile: boolean;
  /** The default in force: a launch naming no profile takes it. */
  default: boolean;
  /** Flagged default, on or off: a flagged one that is off keeps a dim star. */
  flaggedDefault?: boolean;
  disabled: boolean;
  useDefault: boolean;
  /** Credentials found or not; not proof that they still work. */
  login: 'logged_in' | 'needs_login' | 'unknown';
  /** The credentials say the login has run out. */
  expired?: boolean;
  /** The command that signs it in (`kete profile login work`, `claude auth login --claudeai`). */
  loginHint: string | null;
  /** Why a profile entry cannot be used (shown, with the way to Settings). */
  problem?: string | null;
  usage: AccountUsage | null;
  /** Its profile home; null for an own login or an adopted one. */
  home?: string | null;
  /** The variable names its profile sets (values stay in Rust). */
  envKeys?: string[];
}
/** What the quick prompt's "auto" profile runs under now (`account_pick`). */
export interface AutoPick {
  spread: string;
  /** null: the CLI's own login. */
  profile: string | null;
  line: string;
}
/** A board lane (`nebula_core::board::Lane`), by its kept id: running,
 *  needs you, review (`unseen`), ship (`pr`), done, left to right. */
export type Lane = 'needs' | 'running' | 'unseen' | 'done' | 'pr';
/** Where a card can be let go: a lane, or the Archived zone. */
export type DropTarget = Lane | 'archived';
/** What letting a card go there does (`nebula_core::board::DropVerb`). */
export type DropVerb =
  | { act: 'reorder' }
  | { act: 'mark_seen' }
  | { act: 'open_pr' }
  | { act: 'merge_pr' }
  | { act: 'archive' }
  | { act: 'archive_and_offer_worktree_delete' }
  | { act: 'refuse'; reason: string };
/** What the window is looking at (`nebula_core::scope::Scope`); a
 *  missing field matches anything. Set with `board_scope`. */
export interface Scope {
  repo?: string | null;
  worktree?: string | null;
  session?: string | null;
  account?: string | null;
}
/** A session's place on the board and in the attention order, worked out
 *  by the Tauri side with nebula-core's rule (src-tauri `board.rs`). */
export interface Card {
  lane: Lane;
  /** 0 needs you, 1 running, 2 unread (finished, unseen), 3 the rest. */
  tier: number;
  /** The scope admits it: what the attention strip counts. */
  in_scope: boolean;
  /** Core's verdict for letting the card go on each target. */
  drops: Record<DropTarget, DropVerb>;
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
  /** Why a session's last turn stopped short, by agent id (a 62+ daemon
   *  pushes it); a session with no entry has nothing to say. */
  stops?: Record<string, StopNote>;
  /** Each session's lane and tier by agent id, archived ones too. */
  cards: Record<string, Card>;
  /** The board's cards by lane, in the order it shows them: live
   *  sessions only, the orchestrator left out. */
  lanes: Record<Lane, string[]>;
  /** Cards each lane would hold but the scope leaves out. */
  hidden: Record<Lane, number>;
  /** The scope the lanes are narrowed to. */
  scope: Scope;
}

/** `SessionRef` exactly as serde spells it: externally tagged. */
export type SessionRef = { Agent: string } | { Terminal: string };

/** What `reorder` reorders: every project, one project's worktrees, or
 *  one worktree's sessions. Rows lay out by `sort_order`, then the view's
 *  tiebreak, a project's main checkout first; the daemon keeps that rule. */
export type ReorderScope =
  | { kind: 'projects' }
  | { kind: 'worktrees'; project: string }
  | { kind: 'agents'; worktree: string };

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
  /** Usable profiles, enabled first; `default` is the one in force. */
  profiles: { name: string; harness: string; default: boolean; disabled?: boolean }[];
  /** The account spread: `manual`, `failover`, `least_used`; absent from an old backend. */
  spread?: string;
  /** The failover threshold, percent. */
  spread_threshold?: string;
  /** Project default accounts by repo path: harness id to profile name. */
  project_accounts?: Record<string, Record<string, string>>;
  /** The Agents tab's quick prompt harness; empty from an old backend. */
  default_harness?: string;
  /** Quick prompt starts in a new worktree (Sessions tab). */
  new_worktree?: boolean;
  /** Open the session a launch starts (Sessions tab). */
  follow?: boolean;
  /** Archive asks first (Confirm on archive). */
  ask_before_archive?: boolean;
  /** Show all worktrees: an empty checkout keeps its overview group. */
  show_all_worktrees?: boolean;
  /** Finished subagents fold into a count under their session; absent from an old backend. */
  hide_done_subagents?: boolean;
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
  /** The remove deleted the home. */
  purged: boolean;
}
/** What a rename did. */
export interface ProfileRenamed {
  name: string;
  home: string | null;
  /** Repos whose default account it is. */
  projects: string[];
  /** Archived sessions still naming the old name. */
  archived: string[];
  /** Sessions the daemon moved onto the new name (protocol 60). */
  followed: number;
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
  | { type: 'text'; placeholder: string }
  /** A font family: an installed one, or empty for Kete's own (`placeholder`). */
  | { type: 'font'; placeholder: string; mono: boolean };
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
/** `actions_list`: the registry, and the window's key overrides by key
 *  id (an empty list: deliberately unbound). */
export interface ActionsList {
  /** actions.gen.ts's ACTIONS as the Rust side has them (its field
   *  names): the window reads the generated copy. */
  verbs: unknown[];
  overrides: Record<string, string[]>;
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
  /** Why the settings files can't be written now (one can't be parsed). */
  problem: string | null;
}
/** What importing a settings file would change (src-tauri settings_io.rs). */
export interface ImportPreview {
  path: string;
  /** `kete config import --dry-run`: the summary, then what changes. */
  lines: string[];
  changes: boolean;
}

/** The tips' state and whether What's new is due (src-tauri learn.rs). */
export interface LearnState {
  showTips: boolean;
  seen: string[];
  whatsNew: boolean;
  version: string;
}

export interface DesktopPrefs {
  theme: string;
  diff_mode: string;
  home: string;
  /** `home` or `orchestrator`: what the window opens on. */
  startup?: string;
  /** Folded overview groups: `repo:<id>`, `wt:<id>`. */
  collapsed?: string[];
  /** What opens a folder: `auto`, `vscode`, `visual-studio` or `command`. */
  editor_folder?: string;
  /** What opens a file: `auto`, `vscode`, `visual-studio` or `kete`. */
  editor_file?: string;
  /** Font families; absent or empty: Kete's own (Geist, Geist Mono, JetBrains Mono). */
  font_ui?: string;
  font_mono?: string;
  font_term?: string;
  /** Sizes, as the words their rows store: `14`, `13`, `12.5`, `1.12` by default. */
  ui_size?: string;
  term_size?: string;
  editor_size?: string;
  term_line?: string;
  /** Imported themes by id, as their derived palettes (`theme` = `import:<id>`). */
  themes?: Record<string, Palette>;
}
/** A theme mapped onto the window's colour tokens (nebula-config look.rs):
 *  each token is a CSS custom property minus its `--`. */
export interface Palette {
  label: string;
  light: boolean;
  tokens: Record<string, string>;
}
/** `look_vscode`: what VS Code's current theme maps to. */
export interface VsCodeLook {
  stamp: number | null;
  changed: boolean;
  palette: Palette | null;
  problem: string | null;
}
/** An installed VS Code theme, for the import list. */
export interface InstalledTheme {
  name: string;
  label: string;
  light: boolean;
  extension: string;
  builtin: boolean;
  /** Canvas, text and four syntax colours. */
  swatch: string[];
}
/** An installed font family (src-tauri fonts.rs). */
export interface FontFamily {
  name: string;
  mono: boolean;
}
export interface Startup {
  view: string | null;
  session: string | null;
  theme: string | null;
  diff_mode: string | null;
}

// ---- Ship: a worktree's pull request on GitHub (gh) or Azure DevOps ----

export type ForgeKind = 'github' | 'ado';
/** `signIn`: offer the forge's sign-in; `mergeInAdo`: open `url` to finish. */
export type ShipErrorKind =
  | 'signIn'
  | 'noTool'
  | 'noForge'
  | 'noBranch'
  | 'noPr'
  | 'mergeInAdo'
  | 'offline'
  | 'other';
/** What every ship command rejects with. */
export interface ShipError {
  kind: ShipErrorKind;
  forge: ForgeKind | null;
  message: string;
  /** The page to finish on, when there is one. */
  url: string | null;
}
export interface ShipPr {
  forge: ForgeKind;
  /** GitHub's number, or ADO's id. */
  number: number;
  url: string;
  title: string;
  state: 'open' | 'merged' | 'closed';
  draft: boolean;
  head: string;
  base: string;
  headSha: string | null;
}
export interface ShipSuggestion {
  title: string;
  body: string;
}
export interface ShipStatus {
  /** null: no remote on GitHub or Azure DevOps. */
  forge: ForgeKind | null;
  remote: string | null;
  /** null on a detached HEAD. */
  branch: string | null;
  /** The review base a new PR goes into. */
  base: string | null;
  /** Commits no remote has; null with no remote. */
  unpushed: number | null;
  pr: ShipPr | null;
  /** Why `pr` could not be read; the rest stands. */
  error: ShipError | null;
  /** For a branch with no open PR: what the composer starts from. */
  suggestion: ShipSuggestion | null;
}
export interface ShipNewPr {
  title: string;
  body?: string;
  draft?: boolean;
  /** null: the review base. */
  base?: string | null;
}
export interface ShipCreated {
  pr: ShipPr;
  /** The open PR the push updated; nothing new was opened. */
  existing: boolean;
  note: string | null;
}
export type MergeMethod = 'squash' | 'merge' | 'rebase';
export interface ShipMergeOpts {
  method?: MergeMethod;
  /** On the remote only; the worktree delete removes the local branch. */
  deleteBranch?: boolean;
}
export interface ShipMerged {
  pr: ShipPr;
  /** `queued`: the forge took it behind its checks. */
  state: 'merged' | 'queued';
  branchDeleted: boolean;
  note: string | null;
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
  /** The scope's new order, first to last; ids left out keep their old
   *  order after it. Refused (with what to run) by a daemon older than
   *  protocol 56. */
  reorder: (scope: ReorderScope, ids: string[]) => invoke<void>('reorder', { scope, ids }),
  /** Drop the ended subagents of one session, or of every session for null. */
  clearFinishedSubagents: (parent: string | null) => invoke<void>('clear_finished_subagents', { parent }),
  /** Rejects with a {@link ShipError}, as do the two below. */
  shipStatus: (worktree: string, session?: string | null) =>
    invoke<ShipStatus>('ship_status', { worktree, session: session ?? null }),
  shipCreatePr: (worktree: string, pr: ShipNewPr) => invoke<ShipCreated>('ship_create_pr', { worktree, pr }),
  shipMerge: (worktree: string, opts: ShipMergeOpts) => invoke<ShipMerged>('ship_merge', { worktree, opts }),
  /** `gh auth login` in a new terminal in `worktree` (GitHub's sign-in, run
   *  as an account's login is); the terminal's id. */
  shipGhSignIn: (worktree: string) => invoke<string>('ship_gh_sign_in', { worktree }),
  /** Narrow the board, the cards' `in_scope` and `model.scope` to `scope`
   *  (`{}`: everything). The window's own; it survives a reconnect. */
  boardScope: (scope: Scope) => invoke<void>('board_scope', { scope }),
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
  /** Run the open command; `opened: false` means it comes from the repo's
   *  .kete.json and wants a yes first (call again with `approve` set to the
   *  command that was shown; a command changed since is not run). */
  openWorktree: (worktree: string, approve: string | null = null) =>
    invoke<{ command: string; opened: boolean }>('open_worktree', { worktree, approve }),
  openRemote: (worktree: string) => invoke<string>('open_remote', { worktree }),
  // Presets and profiles
  presets: () => invoke<Preset[]>('presets_list'),
  presetSave: (original: string | null, preset: Preset) => invoke<Preset[]>('preset_save', { original, preset }),
  presetDelete: (name: string) => invoke<Preset[]>('preset_delete', { name }),
  profileHarnesses: () => invoke<string[]>('profile_harnesses'),
  profileAdd: (form: ProfileForm) => invoke<ProfileSaved>('profile_add', { form }),
  profileUpdate: (form: ProfileForm, keepEnv: boolean) => invoke<ProfileSaved>('profile_update', { form, keepEnv }),
  /** Remove an account; `purge` deletes its home (the login) too. */
  profileRemove: (name: string, purge = false) => invoke<ProfileSaved>('profile_remove', { name, purge }),
  /** Rename an account: its home, repo defaults and reading follow; refused while a session names it. */
  profileRename: (old: string, name: string) => invoke<ProfileRenamed>('profile_rename', { old, new: name }),
  profileSetDefault: (name: string) => invoke<ProfileRow[]>('profile_set_default', { name }),
  accounts: () => invoke<AccountRow[]>('accounts_list'),
  /** Whakaoho: every Claude account's wake-up schedule, next run, usual
   *  hours, suggestion, each anchor's preview and last decisions
   *  (`kete wake status --json`). */
  wakeOverview: () => invoke<WakeOverview>('wake_overview'),
  /** Save an account's wake-up schedule and rewrite the scheduled task;
   *  the line saying what the task now does. */
  wakeSet: (account: string, schedule: WakeSchedule) => invoke<string>('wake_set', { account, schedule }),
  /** Wake an account now, whatever its schedule (`kete wake <account> --force`). */
  wakeNow: (account: string) => invoke<WakeNowResult[]>('wake_now', { account }),
  profileSetEnabled: (name: string, enabled: boolean) => invoke<null>('profile_set_enabled', { name, enabled }),
  /** A new terminal in `worktree` running the account's login command; its id. */
  accountLoginRun: (worktree: string, key: string) => invoke<string>('account_login_run', { worktree, key }),
  /** What "auto" would pick on a harness now (in `project`, an id, when
   *  given: its default account first); null: it has no profiles. */
  accountPick: (kind: string, project: string | null = null) => invoke<AutoPick | null>('account_pick', { kind, project }),
  /** Restart a session fresh on another account (null: its harness's own login). */
  accountSwitch: (agent: string, profile: string | null) => invoke<void>('account_switch', { agent, profile }),
  /** Set (null clears) a project's default account for a harness. */
  projectSetAccount: (project: string, harness: string, profile: string | null) =>
    invoke<void>('project_set_account', { project, harness, profile }),
  /** A new profile copying `from`'s harness, variables and login kind. */
  profileDuplicate: (from: string, name: string) => invoke<ProfileSaved>('profile_duplicate', { from, name }),
  createTerminal: (worktree: string) => invoke<string>('create_terminal', { worktree }),
  addProject: (path: string) => invoke<string>('add_project', { path }),
  startRun: (worktree: string) => invoke<string>('start_run', { worktree }),
  stopRun: (worktree: string) => invoke<void>('stop_run', { worktree }),
  /** Worktree id to the command its Run button starts; absent: none. */
  runCommands: () => invoke<Record<string, string>>('run_commands'),
  settingsSchema: () => invoke<SettingsSchema>('settings_schema'),
  settingsSet: (target: SettingTarget, value: boolean | string) => invoke<void>('settings_set', { target, value }),
  settingsReset: () => invoke<void>('settings_reset'),
  /** Save dialog, then `kete config export`; null when cancelled. */
  settingsExport: () => invoke<string | null>('settings_export'),
  /** Open dialog, then `kete config import --dry-run`; null when cancelled. */
  settingsImportPreview: () => invoke<ImportPreview | null>('settings_import_preview'),
  settingsImport: (path: string) => invoke<string>('settings_import', { path }),
  desktopPrefs: () => invoke<DesktopPrefs>('desktop_prefs'),
  /** The installed font families, sorted; empty where they can't be listed. */
  systemFonts: () => invoke<FontFamily[]>('system_fonts'),
  /** VS Code's current theme as a palette, unless `known` is still its stamp. */
  lookVscode: (osLight: boolean, known: number | null) => invoke<VsCodeLook>('look_vscode', { osLight, known }),
  /** Which VS Code the look reads (`VS Code`, `VS Code Insiders`); null: none found. */
  lookVscodeInstall: () => invoke<string | null>('look_vscode_install'),
  lookInstalledThemes: () => invoke<InstalledTheme[]>('look_installed_themes'),
  /** Import an installed theme and wear it. */
  lookImportInstalled: (name: string) => invoke<DesktopPrefs>('look_import_installed', { name }),
  /** Open dialog, then import that theme file and wear it; null: cancelled. */
  lookImportFile: () => invoke<DesktopPrefs | null>('look_import_file'),
  lookRemoveTheme: (id: string) => invoke<DesktopPrefs>('look_remove_theme', { id }),
  /** Kete's own look back; the imported themes stay. */
  lookReset: () => invoke<DesktopPrefs>('look_reset'),
  lookUseVscodeFonts: () => invoke<{ prefs: DesktopPrefs; said: string }>('look_use_vscode_fonts'),
  /** Scale the whole window (the Interface size over 14). */
  lookZoom: (scale: number) => invoke<void>('look_zoom', { scale }),
  /** The verb registry and the window's own key bindings. */
  actionsList: () => invoke<ActionsList>('actions_list'),
  /** Bind one of the window's keys (an empty list unbinds it), or with
   *  null put it back to its default. Answers every binding now. */
  desktopKeySet: (id: string, chords: string[] | null) => invoke<Record<string, string[]>>('desktop_key_set', { id, chords }),
  desktopSetCollapsed: (keys: string[]) => invoke<void>('desktop_set_collapsed', { keys }),
  /** First-use tips and What's new: the shared seen-state (src-tauri learn.rs). */
  learnState: () => invoke<LearnState>('learn_state'),
  learnSeen: (id: string) => invoke<void>('learn_seen', { id }),
  learnTips: (on: boolean) => invoke<void>('learn_tips', { on }),
  /** Why config.json can't be written (it can't be parsed); null: fine. */
  configProblem: () => invoke<string | null>('config_problem'),
  diffFiles: (worktree: string) => invoke<DiffFile[]>('diff_files', { worktree }),
  diffPair: (worktree: string, file: DiffFile) => invoke<DiffPair>('diff_pair', { worktree, file }),
  openUrl: (url: string) => invoke<void>('open_url', { url }),
  launchables: () => invoke<Launchables>('launchables'),
  startup: () => invoke<Startup>('startup'),
  // The orchestrator
  startOrchestrator: (spec: StartOrchestrator) => invoke<string>('start_orchestrator', { spec }),
  setOrchestrator: (agent: string) => invoke<void>('set_orchestrator', { agent }),
  clearOrchestrator: () => invoke<void>('clear_orchestrator'),
  readSession: (agent: string) => invoke<string>('read_session', { agent }),
  onReview: (f: (r: ReviewOpened) => void) => listen<ReviewOpened>('kete://review', (e) => f(e.payload)),
};

export interface StartOrchestrator {
  kind: string;
  profile?: string | null;
  model?: string | null;
  effort?: string | null;
  /** A project id: its root worktree; none, a workspace of its own. */
  project?: string | null;
  /** The session's name; none, the daemon's. */
  name?: string | null;
  replace?: boolean;
}
/** `kete orchestrator review`, as the daemon broadcasts it. */
export interface ReviewOpened {
  opener: string;
  sessions: SessionRef[];
  /** `terminal`, `diff`, `history`, `pr`; empty: all. */
  tabs: string[];
}

export { Channel };

export function decodeB64(data: string): Uint8Array {
  const bin = atob(data);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
