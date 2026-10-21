//! The daemon's world: persisted entity tree + live PTY sessions, and the
//! operations the IPC surface exposes over them.

use crate::claude_bg;
use crate::git;
use crate::hooks::{self, HookEnv};
use crate::pty::{PtyEvent, PtySession, SpawnSpec, DEFAULT_COLS, DEFAULT_ROWS};
use crate::status::{AgentStatusMachine, Effect, HookEvent};
use crate::store::Store;
use crate::worktree_hooks::{self, HookContext, WorktreeHook};
use anyhow::{bail, Context, Result};
use nebula_core::env;
use nebula_core::project_file::{self, ProjectCommand};
use nebula_core::{
    Agent, AgentId, AgentKind, AgentStatus, EnterOutcome, Entity, EntityId, LinkId, PrewarmInfo,
    Project, ProjectId, ServerEvent, SessionRef, TerminalId, TerminalTab, Worktree, WorktreeId,
    MAX_CLOUD_PROMPT_BYTES,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// A warm agent CLI older than this is reaped — it holds memory and its
/// conversation context grows stale.
const PREWARM_MAX_AGE: Duration = Duration::from_secs(15 * 60);
/// A live same-spec warm CLI older than this is recycled (killed and
/// re-booted fresh) when its slot is re-requested, instead of being kept.
/// Clients keep-warm the selected worktree on a cadence shorter than
/// `PREWARM_MAX_AGE - PREWARM_RECYCLE_AGE`, so a slot they still care about
/// is always refreshed before the reaper can empty it.
const PREWARM_RECYCLE_AGE: Duration = Duration::from_secs(10 * 60);
/// Gap between the boots of a worktree prewarm sweep. A worktree with five
/// agents must not fork five agent CLIs at once: they would all contend for
/// the CPU with the one session the user is actually waiting to see, which
/// is the whole reason the sweep exists. Nothing is watching these, so
/// warming them slowly costs the user nothing.
const PREWARM_STAGGER: Duration = Duration::from_millis(1500);
/// Hook events buffered on a warm session before its row exists (oldest
/// dropped beyond this).
const PREWARM_HOOK_BUFFER_CAP: usize = 64;
/// A resumed agent CLI that exits with an error this soon after its spawn
/// is taken to have not found the session it was told to resume. Generous
/// on purpose: the login shell alone takes most of a second, and a worktree
/// prewarm sweep boots CLIs back to back — the 2 s this used to be let slow
/// failures through, leaving the pane on the CLI's error.
const RESUME_FAIL_WINDOW: Duration = Duration::from_secs(10);

/// A resumed spawn, watched for the fast failure of a missing session.
struct ResumeWatch {
    /// The PTY it booted, so a later respawn's exit is never mistaken for it.
    session: std::sync::Weak<PtySession>,
    spawned_at: Instant,
    cols: u16,
    rows: u16,
}
/// `$SHELL -l -i -c <cmd>`: a login *and* interactive shell, so zsh sources
/// ~/.zprofile and ~/.zshrc both and the child sees the PATH the user's
/// terminal has. CLI probes use it directly, and PTY launches wrap it through
/// [`LOGIN_SHELL_STDIN_SHIM`] below so rc-file startup sees stdin as non-TTY
/// until nebula's command is about to run.
#[cfg(unix)]
const LOGIN_SHELL_ARGS: [&str; 3] = ["-l", "-i", "-c"];
/// Tiny POSIX wrapper for PTY launches: keep stdout/stderr on the PTY, but
/// give the login shell `/dev/null` as stdin while it sources rc files.
/// Shell startup tools that auto-`exec` only when stdin is a TTY (Iris-style
/// autocomplete launchers) then leave the `-c` payload alone. The payload
/// reopens `/dev/tty` before starting the agent, so the CLI itself remains
/// fully interactive.
#[cfg(unix)]
const LOGIN_SHELL_STDIN_SHIM: &str = "exec </dev/null; exec \"$@\"";
/// Cap on one CLI probe. A heavy rc file costs ~1s; a hung one must not
/// stall a create forever, so on timeout the CLI is assumed present and
/// the spawn itself gets to report.
#[cfg(unix)]
const CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// A RUN TERMINAL whose command exited on its own: the PTY, kept for the
/// replay, and the exit code the OS reported.
type FinishedRun = (Arc<PtySession>, Option<i32>);
/// What a RUN TERMINAL's row is called in the Sessions panel.
const RUN_TERMINAL_NAME: &str = "run";
/// `r` on a worktree with nothing to run: the footer line naming both
/// places a RUN COMMAND can come from.
const NO_RUN_COMMAND: &str =
    "no run command for this worktree: set one in Settings (s) → Project, \
                              or add .kete.json with {\"run\": \"npm run dev\"}";
/// Why a RUN TERMINAL with nothing left to replay will not attach — its
/// run ended before a DAEMON restart, typically.
const RUN_NOT_RUNNING: &str = "this run has stopped: press r on its worktree to start it again";

pub(crate) struct CreateAgentSpec {
    pub worktree: WorktreeId,
    pub name: String,
    pub kind: AgentKind,
    /// Registry id of the custom harness, when `kind` is
    /// [`AgentKind::Custom`].
    pub custom_harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub auto_title: bool,
    pub cloud_prompt: Option<String>,
    /// The CLI's positional first prompt (an AGENT PRESET launch). Request-only.
    pub starting_prompt: Option<String>,
    pub pr_url: Option<String>,
    /// The GitHub issue an ISSUE SESSION was launched for (see
    /// `pr_scope::issue_rule`). Persisted like `pr_url`.
    pub issue_url: Option<String>,
    pub role: nebula_core::AgentRole,
    /// The PROFILE asked for; None = the harness's default.
    pub profile: Option<String>,
    /// The session that asked for this one (`kete spawn`, an orchestrator
    /// spawn); persisted as the row's `parent`. None for a person's launch.
    pub parent: Option<AgentId>,
}

/// A pre-spawned agent CLI waiting to be adopted by the next CreateAgent for
/// the same (worktree, kind). The PTY lives in the normal sessions map under
/// a pre-generated agent id, so its NEBULA_AGENT_ID env is already the id
/// the adopted row will use. Hook events that arrive before the row exists
/// (SessionStart carries the resume session id) are buffered here and
/// replayed at adoption.
struct PrewarmEntry {
    agent_id: AgentId,
    spawned_at: Instant,
    /// Model/effort the warm CLI booted with; a CreateAgent asking for a
    /// different spec can't adopt it (the CLI is already running the wrong
    /// model), so the entry is discarded instead.
    model: Option<String>,
    effort: Option<String>,
    /// The PROFILE the warm CLI booted under — its harness's default when
    /// the spare was made. A create resolving to another account cannot
    /// adopt it: the CLI is already logged in as the wrong one.
    profile: Option<String>,
    buffered_hooks: Vec<(HookEvent, Option<String>)>,
}

impl PrewarmEntry {
    /// Would a create with this spec adopt the spare? The model, effort
    /// and PROFILE must all match: a spread that now picks another account
    /// makes the spare useless, so the keep-check replaces it once.
    fn fits(&self, model: Option<&str>, effort: Option<&str>, profile: Option<&str>) -> bool {
        self.model.as_deref() == model
            && self.effort.as_deref() == effort
            && self.profile.as_deref() == profile
    }
}

/// A relocation waiting on its agent's turn end (`Daemon::pending_moves`).
#[derive(Debug, Clone)]
struct PendingMove {
    target: Worktree,
    /// Respawn on the relocation notice, so the turn carries on in the
    /// target: `nebula worktree`, which the agent ran mid-task. A move the
    /// user made resumes silent, since the turn it waited out never asked
    /// to move.
    notice: bool,
}

pub struct Daemon {
    // Lock order for the registry's parking_lot locks:
    // 1. Async operation gates (`worktree_ops`, then `spawn_gate`) may wrap
    //    store reads/writes and a spawn/install, but never the reverse.
    // 2. If a warm-pool decision needs liveness, take `prewarmed` before
    //    looking in `sessions`; other code snapshots one map and drops it
    //    before touching the next.
    // 3. Session install/removal touches `session_interest` before
    //    `sessions`; exit handling removes from `sessions` before clearing
    //    `session_interest`. No code may hold either while awaiting.
    // 4. Status/relocation state (`pending_moves`, `status_machines`,
    //    `last_cwd`, `resumes`, `finished_runs`, `transcripts`) is taken
    //    one lock at a time. When a status update needs relocation state,
    //    read `pending_moves`, drop it, then take `status_machines`.
    //
    // Store and PTY locks are parking_lot too: they do not poison, and they
    // are not held across daemon-map locks except under the operation gates
    // above. Keep SQLite work out of async workers with `store_blocking`.
    sessions: Mutex<HashMap<SessionRef, Arc<PtySession>>>,
    status_machines: Mutex<HashMap<AgentId, AgentStatusMachine>>,
    /// WORKTREE PORTS as the watcher last read them
    /// (`refresh_worktree_ports`), taken alone.
    live_ports: Mutex<Vec<nebula_core::ports::WorktreePorts>>,
    pub hook_env: HookEnv,
    /// Shared with the hook HTTP server, which reads agent rows to decide
    /// auto-title injection.
    pub store: Arc<Store>,
    /// Entity/status deltas fanned out to every subscribed client.
    pub events: broadcast::Sender<ServerEvent>,
    /// Every session the registry installs — a spawn, a respawn, a prewarm
    /// adoption — by ref, the moment it is in `sessions`. A client's forward
    /// task (`attach.rs`) listens so a kill-and-respawn behind an attached
    /// pane rebinds it to the new PTY instead of leaving it on a dead one.
    pub session_installs: broadcast::Sender<SessionRef>,
    pub shutdown: tokio_util::sync::CancellationToken,
    /// Serializes worktree create/delete with the background auto-sync so
    /// a checkout is never adopted twice while its row is mid-insert.
    worktree_ops: tokio::sync::Mutex<()>,
    /// Warm agent CLIs awaiting adoption, at most one per (worktree, kind).
    prewarmed: Mutex<HashMap<(WorktreeId, AgentKind), PrewarmEntry>>,
    /// Cached `command -v` results per CLI so a missing binary doesn't get
    /// re-probed (login shell spawn) on every prewarm request.
    cli_probes: Mutex<HashMap<String, (bool, Instant)>>,
    /// How many client connections are attached per session — a session
    /// with attachments (and its whole worktree) is "in view" and exempt
    /// from idle reaping.
    attach_counts: Mutex<HashMap<SessionRef, usize>>,
    /// When each live session was last "looked at": spawned, prewarmed,
    /// attached, or covered by the in-view sweep refresh. The idle reaper
    /// kills sessions whose stamp ages past `session_idle_timeout`.
    session_interest: Mutex<HashMap<SessionRef, Instant>>,
    /// Last hook-reported cwd per agent, recorded only for payloads that
    /// passed the foreign-session gate. An agent that walks into a checkout
    /// nebula hasn't adopted yet leaves its cwd here, so the worktree sync
    /// can finish the re-home once the row exists.
    last_cwd: Mutex<HashMap<AgentId, PathBuf>>,
    /// Where each Claude agent's transcript (and beside it, the session
    /// title `/rename` persists) lives, from its hook payloads. Read by
    /// the CLAUDE TITLE SYNC (`session_title.rs`) when the PTY's window
    /// title changes, which no hook reports.
    pub(crate) transcripts: Mutex<HashMap<AgentId, crate::session_title::TranscriptRef>>,
    /// Agents that ran `nebula worktree` and are waiting for their turn to
    /// end: the row already sits under the target worktree while the PTY
    /// still runs in the old checkout. Drained by `complete_pending_move`
    /// on the turn-end hook (kill + respawn resumed in the target), cleared
    /// by any other spawn of the agent, and consulted by the cwd reparent so
    /// the old checkout's cwd can't drag the row back in the meantime.
    /// A user's mid-turn `move_agent` waits here too.
    pending_moves: Mutex<HashMap<AgentId, PendingMove>>,
    /// Serializes the check-and-spawn inside [`Daemon::ensure_session`].
    /// Attach (the request loop) and the worktree prewarm sweep (its own
    /// task) can both reach for the same dead session; without this they
    /// would both miss the registry and fork two CLIs, orphaning one.
    spawn_gate: Mutex<()>,
    /// The worktree prewarm sweep currently running, so a newer one can
    /// cancel it. Walking the project tabs fires a sweep per step, and
    /// only the project the cursor rests on is worth warming.
    prewarm_sweep: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Resumed agent spawns, by agent, until their PTY exits: a resume that
    /// dies inside [`RESUME_FAIL_WINDOW`] could not find its session, and
    /// `watch_for_exit` reads that off this record.
    resumes: Mutex<HashMap<AgentId, ResumeWatch>>,
    /// RUN TERMINALS whose command exited on its own, with its exit code.
    /// The PTY is kept — outside `sessions`, so the row reads as not alive
    /// — so an attach replays how the run ended instead of running it
    /// again. Dropped when the run starts again, is stopped, or its row goes.
    finished_runs: Mutex<HashMap<TerminalId, FinishedRun>>,
    /// Claude subagents per parent session (`subagents.rs`), taken alone.
    subagents: Mutex<subagents::SubagentBook>,
    /// Where an orchestrator started without a project runs, when not the
    /// data dir's `orchestrator` folder: tests point it at a temp dir.
    orchestrator_home: Mutex<Option<PathBuf>>,
    /// Serializes orchestrator provisioning (start, set, clear), so a check,
    /// demote and create never interleave with another.
    pub(crate) provision_gate: tokio::sync::Mutex<()>,
    /// ACCOUNT USAGE and SESSION CONTEXT readings (`usage.rs`), taken alone.
    pub(crate) usage: Mutex<usage::UsageBook>,
    /// The SESSION SNAPSHOTS worker (`snapshots.rs`), taken alone.
    snapshots: snapshots::Snapshotter,
    /// STOP NOTES and the usage-limit lines sessions' screens showed
    /// (`stops.rs`), taken alone.
    stops: Mutex<stops::StopBook>,
    /// WHAKAOHO's hidden sessions in flight (`wake.rs`), by agent id;
    /// taken alone. Never in `sessions`.
    wakes: Mutex<HashMap<AgentId, wake::WakeSession>>,
}

mod agent_repos;
mod exit;
mod lifecycle;
mod order;
mod ports;
mod prewarm;
mod projects;
mod sessions;
mod snapshots;
mod spawn;
mod status;
mod stops;
mod subagents;
mod terminals;
mod usage;
mod wake;

pub use subagents::{subagent_title, SubagentCarry, SubagentHookInfo};

impl Daemon {
    pub fn new(store: Arc<Store>, hook_env: HookEnv) -> Arc<Self> {
        let (events, _) = broadcast::channel(1024);
        let (session_installs, _) = broadcast::channel(64);
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            status_machines: Mutex::new(HashMap::new()),
            live_ports: Mutex::new(Vec::new()),
            hook_env,
            store,
            events,
            session_installs,
            shutdown: tokio_util::sync::CancellationToken::new(),
            worktree_ops: tokio::sync::Mutex::new(()),
            prewarmed: Mutex::new(HashMap::new()),
            cli_probes: Mutex::new(HashMap::new()),
            attach_counts: Mutex::new(HashMap::new()),
            session_interest: Mutex::new(HashMap::new()),
            last_cwd: Mutex::new(HashMap::new()),
            transcripts: Mutex::new(HashMap::new()),
            pending_moves: Mutex::new(HashMap::new()),
            spawn_gate: Mutex::new(()),
            prewarm_sweep: Mutex::new(None),
            resumes: Mutex::new(HashMap::new()),
            finished_runs: Mutex::new(HashMap::new()),
            subagents: Mutex::new(subagents::SubagentBook::default()),
            orchestrator_home: Mutex::new(None),
            provision_gate: tokio::sync::Mutex::new(()),
            usage: Mutex::new(usage::UsageBook::default()),
            snapshots: snapshots::Snapshotter::default(),
            stops: Mutex::new(stops::StopBook::default()),
            wakes: Mutex::new(HashMap::new()),
        })
    }

    /// Point the orchestrator workspace somewhere other than the data dir.
    pub fn set_orchestrator_home(&self, dir: PathBuf) {
        *self.orchestrator_home.lock() = Some(dir);
    }

    pub(crate) fn orchestrator_home_override(&self) -> Option<PathBuf> {
        self.orchestrator_home.lock().clone()
    }

    pub(crate) async fn store_blocking<T, F>(self: &Arc<Self>, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Store) -> Result<T> + Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .context("store task panicked")?
    }
}

/// The session id a spawn of `agent` would hand `claude --resume` — the
/// only kind of session Claude can have sent to its background. None under
/// the `NEBULA_AGENT_CMD` override (tests), which never resumes.
fn claude_resumable_session(agent: &Agent) -> Option<&str> {
    if agent.kind != AgentKind::Claude || std::env::var(env::AGENT_CMD).is_ok() {
        return None;
    }
    agent.session_id.as_deref()
}

/// Whether Claude Code still holds the transcript `claude --resume <id>`
/// reads — `<projects>/<cwd slug>/<id>.jsonl` under any of `roots`, in any
/// slug: a session `nebula worktree` relocated keeps its transcript where
/// it began, and resumes from there. `None` when no root could be read at
/// all, which is no verdict.
fn claude_transcript_exists(roots: &[PathBuf], session_id: &str) -> Option<bool> {
    let file = format!("{session_id}.jsonl");
    // An id that isn't one plain file name names no transcript.
    if Path::new(&file).file_name() != Some(std::ffi::OsStr::new(&file)) {
        return Some(false);
    }
    let mut read_any = false;
    for root in roots {
        let Ok(slugs) = std::fs::read_dir(root) else {
            continue;
        };
        read_any = true;
        if slugs
            .flatten()
            .any(|slug| slug.path().join(&file).is_file())
        {
            return Some(true);
        }
    }
    read_any.then_some(false)
}

/// Program + args for an agent PTY. An override (tests) is used verbatim —
/// no resume args. Otherwise the kind picks the CLI and its resume shape:
/// `claude --resume <sid>` and `cursor-agent --resume <sid>` (flag) vs
/// `codex resume <sid> --cd <cwd>` (subcommand, so resume args must lead;
/// the `--cd` because codex reopens a resumed session in the directory its
/// transcript recorded — or asks which to use — unless told one, and a
/// relocated session must land in its new worktree, not the old checkout);
/// pi takes `pi --session-id <sid>`, which resumes the id where it exists
/// and creates it where it doesn't (a relocated session's new cwd). `cwd`
/// is the checkout every local spawn boots in, and None for a Cloud launch,
/// which has no local one. Codex and cursor always get their
/// skip-permissions flag (`--yolo` / `--force`), appended after the resume
/// args — same convention as Mission Control; pi has no permission gate to
/// skip.
/// Model/effort choices follow: `claude --model m --effort e`,
/// `codex -m m -c model_reasoning_effort=e`, `pi --model m --thinking e`,
/// and for cursor one flat id
/// joined from the two — `cursor-agent --model m-e` (`--model m` when
/// effort is None; the CLI's catalogue bakes the effort into the id and
/// rejects the `m[effort=e]` form its `--help` advertises).
/// Claude and pi then get nebula's worktree guidance appended to the system
/// prompt, any persisted PR scope is composed into that same system-prompt
/// argument, and an `initial_prompt` — the relocation notice a `nebula
/// worktree` respawn opens with, or the starting prompt an AGENT PRESET
/// launch composes — goes last, as the CLI's trailing positional prompt
/// (`claude [prompt]`, `codex [PROMPT]`, `cursor-agent [prompt...]`,
/// `pi [messages...]`).
///
/// The plain shape, as every restart/resume spawns it: booted in
/// `TEST_CWD`, no initial prompt, guidance on. Tests assert against this;
/// the daemon calls the full form. The descriptor resolves from a pinned
/// registry so tests never touch the user's config.
#[cfg(test)]
fn agent_spawn_command(
    kind: AgentKind,
    session_id: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
    cmd_override: Option<&str>,
) -> (String, Vec<String>, bool) {
    let all = test_registry();
    let harness = test_harness(&all, kind);
    agent_spawn_command_with(
        &harness,
        session_id,
        Some(Path::new(TEST_CWD)),
        model,
        effort,
        cmd_override,
        None,
        None,
        true,
    )
}

/// A pinned registry for spawn tests: the compiled-in rows, no overrides,
/// no legacy entries — what a fresh install launches.
#[cfg(test)]
fn test_registry() -> Vec<nebula_core::harness::HarnessDescriptor> {
    harness_registry_in(&std::collections::BTreeMap::new(), &[])
}

/// The pinned descriptor `kind` launches as in spawn tests.
#[cfg(test)]
fn test_harness(
    all: &[nebula_core::harness::HarnessDescriptor],
    kind: AgentKind,
) -> nebula_core::harness::HarnessDescriptor {
    resolve_harness_in(kind, None, all).expect("built-ins resolve from a pinned registry")
}

/// The pinned descriptor for a legacy custom entry in spawn tests.
#[cfg(test)]
fn test_custom_harness(
    all: &[nebula_core::harness::HarnessDescriptor],
    id: &str,
) -> nebula_core::harness::HarnessDescriptor {
    resolve_harness_in(AgentKind::Custom, Some(id), all).expect("the pinned entry resolves")
}

/// What nebula appends to Claude's system prompt: how to take a "do this
/// in a worktree" request through nebula (`Daemon::enter_worktree`) instead
/// of Claude's own EnterWorktree tool, whose checkout lands under
/// `<repo>/.claude/worktrees/` on a `worktree-*` branch — a layout the
/// worktree list only adopts after the fact, and not where a nebula user
/// keeps their worktrees. Claude and pi (both take `--append-system-prompt`;
/// pi has no EnterWorktree, but does run `git worktree add` on its own
/// unless told otherwise): codex and cursor have no system-prompt flag.
pub const CLAUDE_WORKTREE_GUIDANCE: &str = "[kete] This session runs inside kete, which \
manages this project's git worktrees. When the user asks this session to work in a worktree or move \
to one (\"do this in a worktree\", \"do this in a new worktree\", \"move to a worktree\", \"branch \
this off in its own checkout\"), do not use the EnterWorktree tool and do not run `git worktree add` \
yourself. Run this shell command instead, exactly once:\n\n  kete worktree <name>\n\nwhere <name> \
is the branch name the user gave, or a short kebab-case name for the task (`kete worktree` with no \
name invents one; `--base <ref>` picks the start point). kete creates or reuses the worktree, \
associates this current session with it, and relocates the session into it once your current turn \
ends. So when the command succeeds, end your turn at once: tell the user in one line that the \
session is moving into the worktree, and make no further tool calls or edits: you will be resumed \
inside the worktree with a prompt to carry on there. Do not use `kete spawn --worktree` for these \
relocation requests; that belongs to the spawn guidance and is only for starting or handing off to a \
separate new/parallel session. If the command fails, report the error and carry on in the current \
checkout.";

/// The prompt a relocated session is resumed with: it names the checkout
/// the process now runs in and asks for the work to pick back up there, so
/// the user never has to type "continue". Only for the CLIs verified to
/// open a resumed session on a trailing prompt — `claude --resume <sid>
/// "<prompt>"`, `codex resume <sid> [PROMPT]` (the positional its `--help`
/// documents; submitted as the next turn, verified live on codex 0.153.4)
/// and `pi --session-id <sid> "<prompt>"`. Whether `cursor-agent --resume
/// <id> [prompt...]` submits one is not, so a relocated cursor session
/// resumes silent and waits for the user.
fn relocation_prompt(relocate: bool, worktree: &Worktree) -> Option<String> {
    relocate.then(|| {
        format!(
            "[kete] This session now runs inside the worktree `{}` at {}: your working \
             directory is that checkout. Continue the user's most recent request there.",
            worktree.branch,
            worktree.path.display()
        )
    })
}

/// The checkout the test wrapper boots every spawn in.
#[cfg(test)]
const TEST_CWD: &str = "/nebula-test/p-feat";

// Nine positional knobs are two over clippy's line; the callers are the
// two thin wrappers above and the tests, so a builder would only add
// ceremony. Every behavior comes off `harness` — the registry descriptor
// the launch resolved — never off the kind: a repointed program, a renamed
// flag or a whole new CLI flows through here with no new arms.
#[allow(clippy::too_many_arguments)]
fn agent_spawn_command_with(
    harness: &nebula_core::harness::HarnessDescriptor,
    session_id: Option<&str>,
    cwd: Option<&Path>,
    model: Option<&str>,
    effort: Option<&str>,
    cmd_override: Option<&str>,
    initial_prompt: Option<&str>,
    additional_system_prompt: Option<&str>,
    guidance: bool,
) -> (String, Vec<String>, bool) {
    if let Some(cmd) = cmd_override {
        let mut parts = cmd.split_whitespace().map(String::from).collect::<Vec<_>>();
        if parts.is_empty() {
            parts.push(harness.program.trim().to_string());
        }
        let program = parts.remove(0);
        return (program, parts, false);
    }
    let program = harness.program.trim().to_string();
    // Resume: a flag (`--resume <id>`), a positional subcommand
    // (`resume <id>`, with `--cd`), or nothing — a stored id with no
    // resume mapping is ignored and the CLI boots fresh.
    let (mut args, resumed) = match (&harness.resume.flag, &harness.resume.subcommand, session_id) {
        (Some(flag), _, Some(sid)) => (vec![flag.clone(), sid.to_string()], true),
        (None, Some(subcommand), Some(sid)) => {
            let mut args = vec![subcommand.clone(), sid.to_string()];
            if harness.resume.cd {
                if let Some(cwd) = cwd {
                    args.extend(["--cd".to_string(), cwd.to_string_lossy().into_owned()]);
                }
            }
            (args, true)
        }
        _ => (Vec::new(), false),
    };
    if let Some(flag) = harness.permissions_flag.as_deref() {
        args.push(flag.to_string());
    }
    // The model rides its flag — composed with the effort into one id for
    // Cursor's family-suffix shape (`claude-opus-5` + `high`: the TUI only
    // sends an effort the family ships; an effort without a family has
    // nothing to hang off and is dropped).
    if let (Some(m), Some(flag)) = (model, harness.model.flag.as_deref()) {
        let id = match (harness.compose_model_effort, effort) {
            (true, Some(e)) => format!("{m}-{e}"),
            _ => m.to_string(),
        };
        args.extend([flag.to_string(), id]);
    }
    // The effort rides its flag, Codex's `-c key=value` pair, or nothing
    // (composed above, or unmapped and dropped like before).
    if let Some(e) = effort {
        if !harness.compose_model_effort {
            if let Some(flag) = harness.effort.flag.as_deref() {
                args.extend([flag.to_string(), e.to_string()]);
            } else if let (Some(flag), Some(key)) = (
                harness.effort.config_flag.as_deref(),
                harness.effort.config_key.as_deref(),
            ) {
                args.extend([flag.to_string(), format!("{key}={e}")]);
            }
        }
    }
    // Guidance (worktree rules, then the PR scope) rides the
    // system-prompt flag where one is mapped, folds into the first prompt
    // where prepend is, and is dropped where neither is.
    let mut initial_prompt = initial_prompt.map(str::to_string);
    if let Some(flag) = harness.system.append_flag.as_deref() {
        push_system_prompt(&mut args, flag, guidance, additional_system_prompt);
    } else if harness.system.prepend_to_first_prompt {
        let mut first = Vec::new();
        if guidance {
            first.push(CLAUDE_WORKTREE_GUIDANCE.to_string());
            first.push(crate::sibling::CLAUDE_SPAWN_GUIDANCE.to_string());
            first.push(crate::sibling::CLAUDE_SESSION_CONTEXT_GUIDANCE.to_string());
            first.push(crate::open_files::CLAUDE_OPEN_GUIDANCE.to_string());
        }
        if let Some(prompt) = additional_system_prompt {
            first.push(prompt.to_string());
        }
        if !first.is_empty() {
            let head = first.join("\n\n");
            initial_prompt = Some(match initial_prompt {
                Some(task) => format!("{head}\n\n{task}"),
                None => head,
            });
        }
    }
    // The starting prompt rides trailing, like every CLI's positional —
    // or its own flag where the positional is something else (OpenCode's
    // `--prompt`: its positional is the project path).
    if let Some(p) = initial_prompt {
        if let Some(flag) = harness.prompt_flag.as_deref() {
            args.push(flag.to_string());
        }
        args.push(p);
    }
    (program, args, resumed)
}

/// A spawn's prompts once the row's launch rule (the PR / issue scope) and
/// its role are folded in. The ORCHESTRATOR's guidance rides with the role
/// on every cold spawn: the system-prompt flag where the harness maps one,
/// the opening of the first prompt where it prepends (a resume's
/// transcript already holds it). Never into a Cloud dispatch.
pub(super) fn agent_launch_prompts(
    agent: &Agent,
    harness: &nebula_core::harness::HarnessDescriptor,
    rule: Option<String>,
    cloud: bool,
    initial_prompt: Option<&str>,
) -> crate::pr_scope::LaunchPrompts {
    let rule = if cloud {
        rule
    } else {
        crate::orchestrator::launch_rule(agent.role, rule)
    };
    crate::pr_scope::launch_prompts(
        harness.system.append_flag.is_some(),
        agent.session_id.is_some(),
        rule.as_deref(),
        initial_prompt,
    )
}

/// One system-prompt flag carrying nebula's guidance (worktree, spawn,
/// then open) and whatever else the launch adds (the PR scope).
fn push_system_prompt(
    args: &mut Vec<String>,
    flag: &str,
    guidance: bool,
    additional: Option<&str>,
) {
    let mut system_prompt = Vec::new();
    if guidance {
        system_prompt.push(CLAUDE_WORKTREE_GUIDANCE);
        system_prompt.push(crate::sibling::CLAUDE_SPAWN_GUIDANCE);
        system_prompt.push(crate::sibling::CLAUDE_SESSION_CONTEXT_GUIDANCE);
        system_prompt.push(crate::open_files::CLAUDE_OPEN_GUIDANCE);
    }
    if let Some(prompt) = additional {
        system_prompt.push(prompt);
    }
    if !system_prompt.is_empty() {
        args.extend([flag.to_string(), system_prompt.join("\n\n")]);
    }
}

/// Validate an AGENT PRESET's composed starting prompt before it becomes the
/// CLI's positional argument. Same bounds as a cloud task — it crosses the
/// same login-shell `-c` string and argv — with its own wording.
fn validate_starting_prompt(raw: &str) -> Result<String> {
    let text = raw.trim().to_string();
    if text.is_empty() {
        bail!("starting prompt is empty");
    }
    if text.contains('\0') {
        bail!("starting prompt cannot contain NUL bytes");
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "starting prompt is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

/// Why a Cloud row's restart and attach are refused: the agent has no
/// local session, and the pane's panel already says where it does run.
/// Refuse to start a session in a worktree whose checkout is gone from disk
/// (removed with `git worktree remove` or `rm -rf` outside nebula). The PTY
/// layer would quietly run the process in `$HOME` instead (portable-pty drops
/// a cwd that isn't a directory), and an agent's hook install would recreate
/// the path as an empty folder that is not a git checkout; either way the
/// session would look like it runs in its worktree and doesn't. The row is
/// kept while sessions hang off it and the WORKTREE SYNC matches rows by
/// path, so recreating the checkout where it was brings them back.
fn require_checkout(worktree: &Worktree) -> Result<()> {
    if worktree.path.is_dir() {
        return Ok(());
    }
    bail!(
        "the checkout for '{branch}' is gone from disk ({path}). Recreate it where it was and \
         its sessions pick it up: `git worktree prune && git worktree add {path} {branch}` \
         (`add -b {branch} {path}` if the branch is gone too). Or delete the worktree",
        branch = worktree.branch,
        path = worktree.path.display(),
    );
}

const CLOUD_ROW_NO_LOCAL_SESSION: &str =
    "this session runs in Claude Cloud: open it in the browser";

/// Trim and bounds-check text handed to the Claude CLI as one argv item —
/// a Cloud task on create, a message queued on an existing session. Both
/// ride the login shell's `-c` string as well as Claude's argv: quoting
/// stops injection, but a NUL would truncate the command and an unbounded
/// string would blow the argv limit, so both are rejected here rather than
/// at the shell.
fn validate_cloud_text(raw: &str, what: &str) -> Result<String> {
    let text = raw.trim().to_string();
    if text.is_empty() {
        bail!("Claude Cloud needs a {what}");
    }
    if text.contains('\0') {
        bail!("Claude Cloud {what} cannot contain NUL bytes");
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "Claude Cloud {what} is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

/// The Cloud dispatch is a one-shot variation of the normal fresh-Claude
/// command. Keeping it a wrapper leaves every resume/restart caller on
/// the persisted local-session contract, and makes the no-override argument
/// shape directly unit-testable. No worktree guidance either: the Cloud
/// sandbox has no nebula CLI to follow it with. The value binds with `=`
/// (`--cloud=<task>`): the flag takes an *optional* value, so a separate
/// argv item that starts with `--` would be parsed as another Claude flag.
fn claude_cloud_spawn_command(
    harness: &nebula_core::harness::HarnessDescriptor,
    task: &str,
    model: Option<&str>,
    effort: Option<&str>,
    cmd_override: Option<&str>,
) -> (String, Vec<String>, bool) {
    let (program, mut args, resumed) = agent_spawn_command_with(
        harness,
        None,
        None,
        model,
        effort,
        cmd_override,
        None,
        None,
        false,
    );
    if cmd_override.is_none() {
        args.insert(0, format!("--cloud={task}"));
    }
    (program, args, resumed)
}

/// Normalize an agent-supplied title: control characters become spaces,
/// whitespace collapses, and over-long titles are cut — models occasionally
/// hand over a whole sentence no matter what the instruction says.
pub(crate) fn sanitize_title(raw: &str) -> String {
    const MAX_CHARS: usize = 60;
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut title = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() > MAX_CHARS {
        title = title.chars().take(MAX_CHARS).collect();
        title.truncate(title.trim_end().len());
    }
    title
}

/// Canonicalize for path containment tests, falling back to the raw path
/// when it doesn't resolve (deleted checkout, not-yet-created dir). macOS
/// symlinks (`/tmp` → `/private/tmp`) otherwise break `starts_with`.
fn canonical_or_raw(path: &Path) -> std::path::PathBuf {
    nebula_core::paths::canonical(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Does this agent still have a backgrounded tool call running — a job it
/// cut loose from its terminal, which the turn that started it has already
/// left behind? An unknown child pid counts as busy, as for terminals.
fn agent_has_detached_job(session: &PtySession) -> bool {
    match session.child_pid {
        Some(pid) => crate::pty::detached_job_under(pid),
        None => true,
    }
}

/// Does this terminal's shell have any child processes (a command or job
/// still running)? An unknown child pid or a failed probe counts as busy —
/// never kill what can't be inspected. Windows asks the process table.
#[cfg(windows)]
fn shell_has_children(session: &PtySession) -> bool {
    let Some(pid) = session.child_pid else {
        return true;
    };
    let table = nebula_core::platform::process_table();
    table.is_empty() || table.iter().any(|row| row.ppid == pid)
}

/// Does this terminal's shell have any child processes (a command or job
/// still running)? An unknown child pid or a failed probe counts as busy —
/// never kill what can't be inspected.
#[cfg(unix)]
fn shell_has_children(session: &PtySession) -> bool {
    let Some(pid) = session.child_pid else {
        return true;
    };
    !matches!(
        std::process::Command::new("pgrep")
            .arg("-P")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
        Ok(status) if !status.success()
    )
}

/// Canonical form of a user-typed link. Pasting a URL out of a browser is
/// the common case, but people also type `github.com/o/r/pull/7`, so a
/// scheme-less value gets https://. Anything else — another scheme, or no
/// host at all — is refused rather than stored: the TUI hands these to
/// `open(1)`, and only http(s) may ever reach it.
pub(crate) fn normalize_url(url: &str) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        bail!("link URL is empty");
    }
    if url.contains(char::is_whitespace) {
        bail!("link URL contains whitespace");
    }
    let normalized = match url.split_once("://") {
        Some(("http" | "https", _)) => url.to_string(),
        Some((scheme, _)) => bail!("only http(s) links are supported (got {scheme}://)"),
        // Scheme-less: a bare host is a URL people type; a bare word is not.
        None => {
            let host = url.split(['/', '?', '#']).next().unwrap_or_default();
            if !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
                bail!("not a URL: {url}");
            }
            format!("https://{url}")
        }
    };
    // Reject "https://" and friends: a scheme with nothing behind it.
    if normalized
        .split_once("://")
        .is_none_or(|(_, rest)| rest.is_empty())
    {
        bail!("not a URL: {url}");
    }
    Ok(normalized)
}

/// `pre_exec` hook putting the child in a session of its own (`setsid`):
/// an interactive shell there cannot reach the daemon's controlling
/// terminal, and the child leads a group a timeout can sweep.
#[cfg(unix)]
pub(crate) fn own_session() -> std::io::Result<()> {
    nix::unistd::setsid()
        .map(drop)
        .map_err(|errno| std::io::Error::from_raw_os_error(errno as i32))
}

/// Why a create was refused when the agent CLI isn't installed. One line —
/// the TUI shows it in the footer flash, which truncates. Unlike git (which
/// the daemon runs with its own inherited PATH), agent CLIs are spawned
/// through the user's login shell, so a fresh install is picked up on the
/// next try with no daemon restart.
fn cli_missing_message(program: &str) -> String {
    format!("{program} was not found on your PATH: install it, then try again.")
}

/// The effective harness registry from the current config: the
/// compiled-in known harnesses with the `harnesses` map applied, then the
/// legacy `custom_harnesses` list, then map-only new ids. Every launch,
/// resume and hook install resolves through here, so a config edit (not a
/// rebuild) is what adds a CLI.
fn harness_registry() -> Vec<nebula_core::harness::HarnessDescriptor> {
    let config = crate::config::Config::load();
    harness_registry_in(&config.harnesses, &config.custom_harnesses)
}

/// [`harness_registry`] against an explicit config, so tests can pin the
/// registry without touching the user's files.
fn harness_registry_in(
    overrides: &std::collections::BTreeMap<String, nebula_core::harness::HarnessOverride>,
    customs: &[nebula_core::harness::CustomHarness],
) -> Vec<nebula_core::harness::HarnessDescriptor> {
    nebula_core::harness::registry(overrides, customs)
}

/// The descriptor a launch or row runs as: built-ins by kind, customs by
/// registry id. A missing id, or a broken entry, refuses the caller with
/// its reason before anything spawns. An `enabled` switch gates the
/// picker, never an existing row — a harness switched off after its
/// sessions were created keeps running them.
fn resolve_harness(
    kind: AgentKind,
    id: Option<&str>,
) -> Result<nebula_core::harness::HarnessDescriptor> {
    resolve_harness_in(kind, id, &harness_registry())
}

/// [`resolve_harness`] against an explicit registry, so tests can pin
/// entries without touching the user's config.
fn resolve_harness_in(
    kind: AgentKind,
    id: Option<&str>,
    all: &[nebula_core::harness::HarnessDescriptor],
) -> Result<nebula_core::harness::HarnessDescriptor> {
    if kind == AgentKind::Custom && id.map(str::trim).filter(|id| !id.is_empty()).is_none() {
        bail!("custom harness launch is missing its registry id");
    }
    nebula_core::harness::resolve(all, kind, id)
        .cloned()
        .map_err(anyhow::Error::msg)
}

/// Wrap `program args…` in a login + interactive shell (`$SHELL -l -i -c
/// 'exec </dev/tty; unset …; export …; prog args'`) so the child gets the
/// user's real environment — ~/.zprofile and ~/.zshrc on zsh — rather than
/// the daemon's.
///
/// The command word goes in bare, so the shell resolves it the way a typed
/// command line would: an alias or function from the rc files wins over the
/// binary on PATH. That is where a work setup reroutes `claude` through a
/// wrapper (another backend, another login), and the earlier `exec env …
/// 'claude'` form skipped it — `env` looks the name up on PATH, and neither
/// zsh nor bash expands the word after an `exec` either — so every session
/// landed on the raw CLI. Without an `exec` the shell stays in charge of the
/// launch: zsh execs a plain last command itself, so the agent is still the
/// PTY's direct child there; bash, and any command that resolves to a
/// function, run it as a job under the shell instead, in a process group of
/// its own — which is why `PtySession::kill` sweeps the whole tree rather
/// than one group.
///
/// The wrapper process feeds `/dev/null` to the shell while those files run,
/// then the prelude reopens `/dev/tty` and restates what the pane is *after*
/// startup:
/// `TERM` and `COLORTERM` name nebula's own grid — 24-bit colour whatever
/// the host terminal — and `NO_COLOR` / `FORCE_COLOR` are dropped. A
/// login-only profile that exports `NO_COLOR` reaches a session here and
/// nowhere else (foot and Ghostty on Linux start non-login shells), and
/// Claude Code takes it as "no colour": its whole UI in the default
/// foreground while the TUI around it stays coloured (#37). The spawn sets
/// the same three against the daemon's inherited environment; this covers
/// the profile's.
#[cfg(unix)]
fn login_shell_wrap(shell: &str, program: &str, args: &[String]) -> (String, Vec<String>) {
    login_shell_wrap_env(shell, program, args, &Default::default())
}

/// [`login_shell_wrap`] for a launch under a PROFILE: its environment edit
/// rides the line itself, after the rc files have run — set on the spawn
/// alone, an `export ANTHROPIC_API_KEY=…` in `~/.zshrc` would win it back.
#[cfg(unix)]
fn login_shell_wrap_env(
    shell: &str,
    program: &str,
    args: &[String],
    env: &nebula_core::profiles::LaunchEnv,
) -> (String, Vec<String>) {
    let mut line = profile_env_line(env);
    line.push_str(&command_word(program));
    for arg in args {
        line.push(' ');
        line.push_str(&nebula_core::shell::single_quote(arg));
    }
    login_shell_line(shell, &line)
}

/// [`login_shell_wrap`] for a line that is already shell syntax — a RUN
/// TERMINAL's `.nebula.json` `run`, pipes and `&&` and all — behind the
/// same prelude.
#[cfg(unix)]
fn login_shell_line(shell: &str, line: &str) -> (String, Vec<String>) {
    let mut cmdline = String::from("exec </dev/tty; unset");
    for name in env::PANE_COLOR_OVERRIDES {
        cmdline.push(' ');
        cmdline.push_str(name);
    }
    cmdline.push_str("; export TERM=");
    cmdline.push_str(env::PANE_TERM);
    cmdline.push_str(" COLORTERM=");
    cmdline.push_str(env::PANE_COLORTERM);
    cmdline.push_str("; ");
    cmdline.push_str(line);
    let mut args = vec!["-c".to_string(), LOGIN_SHELL_STDIN_SHIM.to_string()];
    args.push("nebula-login-shell".to_string());
    args.push(shell.to_string());
    args.extend(LOGIN_SHELL_ARGS.iter().map(|s| s.to_string()));
    args.push(cmdline);
    ("/bin/sh".to_string(), args)
}

/// Windows: there is no login shell to wrap the launch in. The daemon's
/// environment is already the user's (no profile adds to it the way
/// ~/.zprofile does), the PTY spawn resolves `program` through PATH +
/// PATHEXT and runs a `.cmd` or `.ps1` under its host
/// (`nebula_core::shell::launch`), and `TERM`/`COLORTERM` are set on the
/// spawn itself. So the launch is `program args…` as given.
#[cfg(windows)]
fn login_shell_wrap(_shell: &str, program: &str, args: &[String]) -> (String, Vec<String>) {
    (program.to_string(), args.to_vec())
}

/// Windows [`login_shell_wrap_env`]: no rc file runs between the spawn's
/// environment and the CLI, so the edit rides the spawn alone.
#[cfg(windows)]
fn login_shell_wrap_env(
    shell: &str,
    program: &str,
    args: &[String],
    _env: &nebula_core::profiles::LaunchEnv,
) -> (String, Vec<String>) {
    login_shell_wrap(shell, program, args)
}

/// A PROFILE's environment edit as shell syntax ahead of the command word:
/// `unset A B; export K='v'; `, or nothing when there is no edit.
#[cfg(unix)]
fn profile_env_line(env: &nebula_core::profiles::LaunchEnv) -> String {
    let mut line = String::new();
    let removals = env.removals();
    if !removals.is_empty() {
        line.push_str("unset");
        for name in &removals {
            line.push(' ');
            line.push_str(name);
        }
        line.push_str("; ");
    }
    for (key, value) in &env.set {
        let plain = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !plain {
            continue;
        }
        line.push_str(&format!(
            "export {key}={}; ",
            nebula_core::shell::single_quote(value)
        ));
    }
    line
}

/// The PROFILE a create on `harness` runs under, by name: `asked` when
/// the launch named one — it must exist and be an account for that
/// harness — else the default account the project at `repo` sets for the
/// harness (Settings, Accounts; `kete profile set-project`), else the
/// ACCOUNT SPREAD's pick (`nebula_core::accounts::resolve_launch`): under
/// `manual`, the harness's default, else none.
fn resolve_profile_name(
    harness: &nebula_core::harness::HarnessDescriptor,
    asked: Option<&str>,
    repo: Option<&Path>,
) -> Result<Option<String>> {
    let config = crate::config::Config::load();
    let project_default = repo.and_then(|repo| config.project_default_profile(repo, &harness.id));
    let pick = nebula_core::accounts::resolve_launch_now(
        &config.profiles,
        &harness.id,
        asked,
        project_default,
    )
    .map_err(anyhow::Error::msg)?;
    tracing::debug!(choice = %pick.describe(&harness.id), "account spread");
    Ok(pick.profile.map(|p| p.name.trim().to_string()))
}

/// Refuse a launch that names `profile` when [`resolve_profile_name`]
/// would refuse it at create (undefined, switched off, or for another
/// harness): for a caller with work to do first, like `nebula spawn
/// --worktree` cutting a checkout, so nothing is cut for a launch that
/// cannot happen.
pub(crate) fn check_launch_profile(
    kind: AgentKind,
    custom_harness: Option<&str>,
    profile: Option<&str>,
) -> Result<()> {
    let Some(profile) = profile else {
        return Ok(());
    };
    let harness = resolve_harness(kind, custom_harness)?;
    resolve_profile_name(&harness, Some(profile), None).map(|_| ())
}

/// The PROFILE `agent` runs under, read fresh from the config: None for a
/// session with none, `Err` when its profile was removed or now names
/// another harness — the spawn is refused with the reason rather than
/// booting on whatever login the daemon's environment has.
fn session_profile(
    agent: &Agent,
    harness: &nebula_core::harness::HarnessDescriptor,
) -> Result<Option<nebula_core::profiles::Profile>> {
    let Some(name) = agent.profile.as_deref() else {
        return Ok(None);
    };
    let config = crate::config::Config::load();
    nebula_core::profiles::resolve(&config.profiles, &harness.id, Some(name))
        .map(|profile| profile.cloned())
        .map_err(anyhow::Error::msg)
}

/// Windows [`login_shell_wrap`] for a line in the user shell's own syntax
/// (a RUN TERMINAL's `run`): `-Command` for PowerShell, `/c` for cmd.
#[cfg(windows)]
fn login_shell_line(shell: &str, line: &str) -> (String, Vec<String>) {
    (
        shell.to_string(),
        nebula_core::shell::shell_line_args(shell, line),
    )
}

/// `program` as the command word of a shell line: bare when it is a plain
/// name (letters, digits, `-`, `_`, `.`, `/`), since a quoted word is exempt
/// from alias expansion in every shell; single-quoted otherwise.
#[cfg(unix)]
fn command_word(program: &str) -> String {
    let plain = !program.is_empty()
        && program
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
    if plain {
        program.to_string()
    } else {
        nebula_core::shell::single_quote(program)
    }
}

/// The login-shell line [`Daemon::probe_cli`] runs to ask whether
/// `program` resolves. The word is single-quoted
/// ([`nebula_core::shell::single_quote`]):
/// a `harnesses` entry in config.json can name any string, and pasted in
/// bare a quote would close the word and run the rest as a command — at
/// daemon boot, since [`Daemon::warm_cli_probes`] asks for every entry.
/// Built-in names come out exactly as they always did (`'claude'`).
#[cfg(unix)]
fn cli_probe_line(program: &str) -> String {
    format!(
        "command -v {} >/dev/null 2>&1",
        nebula_core::shell::single_quote(program)
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Claude argv: `args`, then nebula's appended guidance (worktree and
    /// spawn, one `--append-system-prompt`).
    fn guided(flag: &str, args: &[&str]) -> Vec<String> {
        args.iter()
            .map(|s| s.to_string())
            .chain([
                flag.to_string(),
                [
                    CLAUDE_WORKTREE_GUIDANCE,
                    crate::sibling::CLAUDE_SPAWN_GUIDANCE,
                    crate::sibling::CLAUDE_SESSION_CONTEXT_GUIDANCE,
                    crate::open_files::CLAUDE_OPEN_GUIDANCE,
                ]
                .join("\n\n"),
            ])
            .collect()
    }

    #[test]
    fn claude_transcript_lookup_searches_every_project_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join("projects");
        for slug in ["-w-issue-68", "-w-root-branch-switcher"] {
            std::fs::create_dir_all(projects.join(slug)).unwrap();
        }
        std::fs::write(projects.join("-w-issue-68").join("sid-1.jsonl"), "{}\n").unwrap();
        let missing = tmp.path().join("no-such-config").join("projects");
        let roots = [missing.clone(), projects];
        // Found under the slug the session began in, whichever checkout
        // resumes it (a relocated session).
        assert_eq!(claude_transcript_exists(&roots, "sid-1"), Some(true));
        // A CLI nobody prompted: an id, and no file behind it.
        assert_eq!(claude_transcript_exists(&roots, "sid-2"), Some(false));
        // An id that tries to be a path names nothing.
        assert_eq!(
            claude_transcript_exists(&roots, "../-w-issue-68/sid-1"),
            Some(false)
        );
        // Nothing readable: no verdict, so the resume is still tried.
        assert_eq!(claude_transcript_exists(&[missing], "sid-1"), None);
    }

    /// Kete's four guidance blocks, joined as the system-prompt flag
    /// carries them, written out here rather than through [`guided`] so the
    /// pin below does not lean on the code it pins.
    fn guidance_0_59() -> String {
        [
            CLAUDE_WORKTREE_GUIDANCE,
            crate::sibling::CLAUDE_SPAWN_GUIDANCE,
            crate::sibling::CLAUDE_SESSION_CONTEXT_GUIDANCE,
            crate::open_files::CLAUDE_OPEN_GUIDANCE,
        ]
        .join("\n\n")
    }

    /// The 0.59 argv for the four launches 0.60 must leave alone, byte for
    /// byte: a plain Claude session, a resume with model, effort and a
    /// trailing prompt, a PR launch, and an orchestrator start. Pinned
    /// before the launch overlay existed.
    #[test]
    fn the_0_59_claude_argv_is_pinned() {
        let all = test_registry();
        let claude = test_harness(&all, AgentKind::Claude);
        let cwd = Some(Path::new(TEST_CWD));
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();

        let plain = agent_spawn_command_with(&claude, None, cwd, None, None, None, None, None, true);
        assert_eq!(
            plain,
            (
                "claude".to_string(),
                vec!["--append-system-prompt".to_string(), guidance_0_59()],
                false
            )
        );

        let resume = agent_spawn_command_with(
            &claude,
            Some("sid-1"),
            cwd,
            Some("opus"),
            Some("high"),
            None,
            Some("carry on"),
            None,
            true,
        );
        let mut want = s(&["--resume", "sid-1", "--model", "opus", "--effort", "high"]);
        want.extend(["--append-system-prompt".to_string(), guidance_0_59()]);
        want.push("carry on".into());
        assert_eq!(resume, ("claude".to_string(), want, true));

        let pr = agent_spawn_command_with(
            &claude,
            None,
            cwd,
            None,
            None,
            None,
            None,
            Some("PR RULE"),
            true,
        );
        assert_eq!(
            pr.1,
            vec![
                "--append-system-prompt".to_string(),
                format!("{}\n\nPR RULE", guidance_0_59())
            ]
        );

        let orchestrator = Agent {
            parent: None,
            profile: None,
            id: AgentId("o".into()),
            worktree_id: WorktreeId("w".into()),
            name: "orchestrator".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Orchestrator,
            recent_prompts: Vec::new(),
        };
        let prompts = agent_launch_prompts(&orchestrator, &claude, None, false, None);
        let start = agent_spawn_command_with(
            &claude,
            None,
            cwd,
            None,
            None,
            None,
            prompts.initial.as_deref(),
            prompts.system.as_deref(),
            true,
        );
        assert_eq!(
            start.1,
            vec![
                "--append-system-prompt".to_string(),
                format!(
                    "{}\n\n{}",
                    guidance_0_59(),
                    crate::orchestrator::CLAUDE_ORCHESTRATOR_GUIDANCE
                )
            ]
        );
    }

    #[test]
    fn spawn_command_per_kind_resume_shapes() {
        // Fresh sessions: bare CLI (Claude plus its system-prompt guidance).
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, None, None, None, None),
            (
                "claude".into(),
                guided("--append-system-prompt", &[]),
                false
            )
        );
        // Codex/cursor always run in skip-permissions mode.
        assert_eq!(
            agent_spawn_command(AgentKind::Codex, None, None, None, None),
            ("codex".into(), vec!["--yolo".to_string()], false)
        );
        // Cursor's agent CLI is `cursor-agent`, not `cursor` (the editor).
        assert_eq!(
            agent_spawn_command(AgentKind::Cursor, None, None, None, None),
            ("cursor-agent".into(), vec!["--force".to_string()], false)
        );
        // Pi has no permission gate to skip and takes the same guidance as
        // Claude (it has the system-prompt flag).
        assert_eq!(
            agent_spawn_command(AgentKind::Pi, None, None, None, None),
            ("pi".into(), guided("--append-system-prompt", &[]), false)
        );
        // Pi resumes by exact id — one that is missing is created, so a
        // relocated session's new cwd never dies on a stale id.
        assert_eq!(
            agent_spawn_command(AgentKind::Pi, Some("sid-4"), None, None, None),
            (
                "pi".into(),
                guided("--append-system-prompt", &["--session-id", "sid-4"]),
                true
            )
        );
        // Muse boots bare and fresh: no resume flag is mapped yet, so a
        // stored session id is ignored rather than sent.
        assert_eq!(
            agent_spawn_command(AgentKind::Muse, None, None, None, None),
            ("muse".into(), vec![], false)
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Muse, Some("sid-9"), None, None, None),
            ("muse".into(), vec![], false)
        );
        // OpenCode boots bare like Codex (no system-prompt flag) and with
        // no permission flag (its prompts drive NEEDS FEEDBACK through the
        // managed plugin); a stored id resumes by `--session`.
        assert_eq!(
            agent_spawn_command(AgentKind::OpenCode, None, None, None, None),
            ("opencode".into(), vec![], false)
        );
        assert_eq!(
            agent_spawn_command(AgentKind::OpenCode, Some("ses_9"), None, None, None),
            (
                "opencode".into(),
                vec!["--session".to_string(), "ses_9".to_string()],
                true
            )
        );
        // Claude resumes with a flag; codex with a subcommand (order matters).
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, Some("sid-1"), None, None, None),
            (
                "claude".into(),
                guided("--append-system-prompt", &["--resume", "sid-1"]),
                true
            )
        );
        // Skip-permissions flags trail the resume args. A codex resume is
        // told its checkout (`--cd`): without it codex reopens the session
        // in the directory its transcript recorded, which for a relocated
        // session is the old one.
        assert_eq!(
            agent_spawn_command(AgentKind::Codex, Some("sid-2"), None, None, None),
            (
                "codex".into(),
                vec![
                    "resume".to_string(),
                    "sid-2".to_string(),
                    "--cd".to_string(),
                    TEST_CWD.to_string(),
                    "--yolo".to_string()
                ],
                true
            )
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Cursor, Some("sid-3"), None, None, None),
            (
                "cursor-agent".into(),
                vec![
                    "--resume".to_string(),
                    "sid-3".to_string(),
                    "--force".to_string()
                ],
                true
            )
        );
        // Override wins for both kinds and never gets resume args.
        assert_eq!(
            agent_spawn_command(
                AgentKind::Claude,
                Some("sid"),
                None,
                None,
                Some("/bin/sh -i")
            ),
            ("/bin/sh".into(), vec!["-i".to_string()], false)
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Codex, Some("sid"), None, None, Some("/bin/sh")),
            ("/bin/sh".into(), vec![], false)
        );
    }

    #[test]
    fn spawn_command_model_and_effort_flags() {
        // Claude gets --model/--effort; either alone works.
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, None, Some("opus"), Some("high"), None),
            (
                "claude".into(),
                guided(
                    "--append-system-prompt",
                    &["--model", "opus", "--effort", "high"]
                ),
                false
            )
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, None, None, Some("max"), None),
            (
                "claude".into(),
                guided("--append-system-prompt", &["--effort", "max"]),
                false
            )
        );
        // Codex takes --model plus a config override for effort, after --yolo.
        assert_eq!(
            agent_spawn_command(AgentKind::Codex, None, Some("gpt-5.5"), Some("high"), None),
            (
                "codex".into(),
                vec![
                    "--yolo".to_string(),
                    "--model".to_string(),
                    "gpt-5.5".to_string(),
                    "-c".to_string(),
                    "model_reasoning_effort=high".to_string()
                ],
                false
            )
        );
        // Pi: `--model <pattern>` plus `--thinking <level>`, ahead of the
        // guidance like Claude's.
        assert_eq!(
            agent_spawn_command(AgentKind::Pi, None, Some("sonnet"), Some("high"), None),
            (
                "pi".into(),
                guided(
                    "--append-system-prompt",
                    &["--model", "sonnet", "--thinking", "high"]
                ),
                false
            )
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Pi, None, None, Some("off"), None),
            (
                "pi".into(),
                guided("--append-system-prompt", &["--thinking", "off"]),
                false
            )
        );
        // Muse takes `--model` verbatim and no effort flag yet: effort is
        // dropped, never sent.
        assert_eq!(
            agent_spawn_command(AgentKind::Muse, None, Some("spark"), Some("high"), None),
            (
                "muse".into(),
                vec!["--model".to_string(), "spark".to_string()],
                false
            )
        );
        // OpenCode takes a `provider/model` id verbatim and has no effort
        // flag: effort is dropped, never sent.
        assert_eq!(
            agent_spawn_command(
                AgentKind::OpenCode,
                None,
                Some("anthropic/claude-sonnet-5"),
                Some("high"),
                None
            ),
            (
                "opencode".into(),
                vec![
                    "--model".to_string(),
                    "anthropic/claude-sonnet-5".to_string()
                ],
                false
            )
        );
        // Resume keeps the model/effort flags (a fallback fresh spawn needs
        // them, and the CLIs accept them alongside resume).
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, Some("sid"), Some("sonnet"), None, None),
            (
                "claude".into(),
                guided(
                    "--append-system-prompt",
                    &["--resume", "sid", "--model", "sonnet"]
                ),
                true
            )
        );
        // Cursor joins family and effort into the CLI's one flat id; a
        // family alone is passed bare, an effort alone has nothing to
        // join and is dropped.
        assert_eq!(
            agent_spawn_command(
                AgentKind::Cursor,
                None,
                Some("claude-opus-5-thinking"),
                Some("high"),
                None
            ),
            (
                "cursor-agent".into(),
                vec![
                    "--force".to_string(),
                    "--model".to_string(),
                    "claude-opus-5-thinking-high".to_string()
                ],
                false
            )
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Cursor, Some("sid"), Some("auto"), None, None),
            (
                "cursor-agent".into(),
                vec![
                    "--resume".to_string(),
                    "sid".to_string(),
                    "--force".to_string(),
                    "--model".to_string(),
                    "auto".to_string()
                ],
                true
            )
        );
        assert_eq!(
            agent_spawn_command(AgentKind::Cursor, None, None, Some("high"), None),
            ("cursor-agent".into(), vec!["--force".to_string()], false)
        );
        // Override still wins over everything.
        assert_eq!(
            agent_spawn_command(AgentKind::Claude, None, Some("opus"), None, Some("/bin/sh")),
            ("/bin/sh".into(), vec![], false)
        );
    }

    #[test]
    fn custom_spawn_uses_entry_program_and_model_flag() {
        // Custom entries launch with their own program and model flag; a
        // stored session id is ignored (fresh boot) and effort is dropped.
        let agy = nebula_core::harness::CustomHarness {
            id: "agy".into(),
            label: "Agy".into(),
            program: "agy".into(),
            enabled: true,
            model: "default".into(),
            model_flag: "--model".into(),
            hooks: None,
        };
        let all = harness_registry_in(
            &std::collections::BTreeMap::new(),
            std::slice::from_ref(&agy),
        );
        let harness = test_custom_harness(&all, "agy");
        let (program, args, resumed) = agent_spawn_command_with(
            &harness,
            Some("sid-1"),
            Some(Path::new(TEST_CWD)),
            Some("big-1"),
            Some("high"),
            None,
            Some("do it"),
            None,
            true,
        );
        assert_eq!(program, "agy");
        assert_eq!(args, vec!["--model", "big-1", "do it"]);
        assert!(!resumed);
        // A custom model flag spelling is honored verbatim.
        let gemini = nebula_core::harness::CustomHarness {
            model_flag: "-m".into(),
            ..agy.clone()
        };
        let all = harness_registry_in(&std::collections::BTreeMap::new(), &[gemini]);
        let harness = test_custom_harness(&all, "agy");
        let (_, args, _) = agent_spawn_command_with(
            &harness,
            None,
            Some(Path::new(TEST_CWD)),
            Some("flash"),
            None,
            None,
            None,
            None,
            true,
        );
        assert_eq!(args, vec!["-m", "flash"]);
    }

    /// The orchestrator's guidance is injected by role on both launch
    /// paths: Claude's system-prompt flag, and the opening of a Codex cold
    /// spawn's first prompt. A resumed Codex session's transcript already
    /// holds it, a worker never gets it, and a Cloud dispatch never does.
    #[test]
    fn the_orchestrator_guidance_is_injected_by_role_on_both_launch_paths() {
        use crate::orchestrator::CLAUDE_ORCHESTRATOR_GUIDANCE as GUIDANCE;
        let agent = |role, session_id: Option<&str>| Agent {
            parent: None,
            profile: None,
            id: AgentId("o".into()),
            worktree_id: WorktreeId("w".into()),
            name: "orchestrator".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: session_id.map(str::to_string),
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        };
        let orchestrator = agent(nebula_core::AgentRole::Orchestrator, None);
        let worker = agent(nebula_core::AgentRole::Worker, None);
        let all = test_registry();
        let launch = |harness: &nebula_core::harness::HarnessDescriptor,
                      a: &Agent,
                      rule: Option<&str>,
                      cloud: bool| {
            let prompts = agent_launch_prompts(a, harness, rule.map(str::to_string), cloud, None);
            agent_spawn_command_with(
                harness,
                a.session_id.as_deref(),
                Some(Path::new(TEST_CWD)),
                None,
                None,
                None,
                prompts.initial.as_deref(),
                prompts.system.as_deref(),
                true,
            )
            .1
        };

        let claude = test_harness(&all, AgentKind::Claude);
        let flag = claude.system.append_flag.clone().expect("claude appends");
        let args = launch(&claude, &orchestrator, Some("PR RULE"), false);
        let system = &args[args.iter().position(|a| *a == flag).unwrap() + 1];
        assert!(system.contains(GUIDANCE), "{system}");
        assert!(
            system.contains("PR RULE"),
            "the launch rule still rides along"
        );
        assert!(!launch(&claude, &worker, None, false)
            .iter()
            .any(|a| a.contains(GUIDANCE)));
        assert!(!launch(&claude, &orchestrator, None, true)
            .iter()
            .any(|a| a.contains(GUIDANCE)));

        let codex = test_harness(&all, AgentKind::Codex);
        assert!(
            codex.system.append_flag.is_none(),
            "codex has no system flag"
        );
        let args = launch(&codex, &orchestrator, None, false);
        assert!(
            args.last().is_some_and(|first| first.starts_with(GUIDANCE)),
            "{args:?}"
        );
        assert!(!launch(&codex, &worker, None, false)
            .iter()
            .any(|a| a.contains(GUIDANCE)));
        let resumed = agent(nebula_core::AgentRole::Orchestrator, Some("sid"));
        assert!(!launch(&codex, &resumed, None, false)
            .iter()
            .any(|a| a.contains(GUIDANCE)));
    }

    #[test]
    fn custom_spawn_honors_map_deltas_for_resume_and_hooks() {
        // A legacy entry gains a resume flag and an effort flag purely
        // through the `harnesses` map — no code change, no new shape.
        use nebula_core::harness::{Clearable, HarnessOverride};
        let agy = nebula_core::harness::CustomHarness {
            id: "agy".into(),
            label: "Agy".into(),
            program: "agy".into(),
            enabled: true,
            model: "default".into(),
            model_flag: "--model".into(),
            hooks: None,
        };
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "agy".into(),
            HarnessOverride {
                resume_flag: Clearable::Set("--resume".into()),
                effort_flag: Clearable::Set("--effort".into()),
                effort_offered: Some(true),
                hooks: Clearable::Set("claude".into()),
                ..HarnessOverride::default()
            },
        );
        let all = harness_registry_in(&overrides, &[agy]);
        let harness = test_custom_harness(&all, "agy");
        assert_eq!(
            harness.hook_dialect(),
            Some(AgentKind::Claude),
            "the dialect installs Claude hooks for a third-party CLI"
        );
        let (program, args, resumed) = agent_spawn_command_with(
            &harness,
            Some("sid-1"),
            Some(Path::new(TEST_CWD)),
            Some("big-1"),
            Some("high"),
            None,
            Some("do it"),
            None,
            true,
        );
        assert_eq!(program, "agy");
        assert_eq!(
            args,
            vec!["--resume", "sid-1", "--model", "big-1", "--effort", "high", "do it"]
        );
        assert!(resumed);
    }

    /// A third-party CLI with every row mapped — shaped like xAI's
    /// `grok` (`--model`, `--reasoning-effort`, `--resume <id>`,
    /// `--rules` appending to the system prompt, trailing prompt) —
    /// spawns, resumes and carries guidance with config alone.
    #[test]
    fn third_party_harness_with_all_rows_mapped_spawns_and_resumes() {
        use nebula_core::harness::{Clearable, HarnessOverride};
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "grok".into(),
            HarnessOverride {
                program: Clearable::Set("grok".into()),
                model_flag: Clearable::Set("--model".into()),
                effort_flag: Clearable::Set("--reasoning-effort".into()),
                effort_offered: Some(true),
                resume_flag: Clearable::Set("--resume".into()),
                system_append_flag: Clearable::Set("--rules".into()),
                ..HarnessOverride::default()
            },
        );
        let all = harness_registry_in(&overrides, &[]);
        let grok = test_custom_harness(&all, "grok");
        assert_eq!(grok.problem(), None);

        // Fresh boot: model, effort, guidance and prompt in order, no
        // permissions flag the entry never named.
        let (program, args, resumed) = agent_spawn_command_with(
            &grok,
            None,
            Some(Path::new(TEST_CWD)),
            Some("grok-code"),
            Some("high"),
            None,
            Some("fix auth"),
            None,
            true,
        );
        assert_eq!(program, "grok");
        assert!(!resumed);
        let mut expected = vec![
            "--model".to_string(),
            "grok-code".to_string(),
            "--reasoning-effort".to_string(),
            "high".to_string(),
        ];
        expected.append(&mut guided("--rules", &[]));
        expected.push("fix auth".into());
        assert_eq!(args, expected);

        // Resume: the stored id rides the mapped flag.
        let (_, args, resumed) = agent_spawn_command_with(
            &grok,
            Some("sid-9"),
            Some(Path::new(TEST_CWD)),
            None,
            None,
            None,
            None,
            None,
            false,
        );
        assert!(resumed);
        assert_eq!(args, vec!["--resume", "sid-9"]);
    }

    #[test]
    fn harness_resolve_names_missing_unknown_and_broken_entries() {
        use nebula_core::harness::CustomHarness;
        let agy = CustomHarness {
            id: "agy".into(),
            label: String::new(),
            program: "agy".into(),
            enabled: true,
            model: "default".into(),
            model_flag: "--model".into(),
            hooks: None,
        };
        let all = harness_registry_in(
            &std::collections::BTreeMap::new(),
            std::slice::from_ref(&agy),
        );
        // Built-ins resolve by kind, whatever the id says.
        assert_eq!(
            resolve_harness_in(AgentKind::Claude, None, &all)
                .unwrap()
                .program,
            "claude"
        );
        // Custom without an id, or with an unknown one, refuses.
        let err = resolve_harness_in(AgentKind::Custom, None, &all).unwrap_err();
        assert!(err.to_string().contains("registry id"), "{err}");
        let err = resolve_harness_in(AgentKind::Custom, Some("gone"), &all).unwrap_err();
        assert!(err.to_string().contains("no longer defined"), "{err}");
        // A broken entry refuses with its reason, never launches.
        let broken = CustomHarness {
            program: String::new(),
            ..agy.clone()
        };
        let all = harness_registry_in(&std::collections::BTreeMap::new(), &[broken]);
        let err = resolve_harness_in(AgentKind::Custom, Some("agy"), &all).unwrap_err();
        assert!(err.to_string().contains("no program"), "{err}");
        // The usable entry resolves, disabled or not: a harness switched
        // off after its sessions were created keeps running them.
        let off = CustomHarness {
            enabled: false,
            ..agy.clone()
        };
        let all = harness_registry_in(&std::collections::BTreeMap::new(), &[off]);
        assert_eq!(
            resolve_harness_in(AgentKind::Custom, Some("agy"), &all)
                .unwrap()
                .program,
            "agy"
        );
    }

    #[test]
    fn grok_spawn_uses_verified_flags_and_preserves_prompt_boundaries() {
        let grok = nebula_core::harness::builtin("grok").unwrap();
        assert_eq!(
            agent_spawn_command_with(&grok, None, None, None, None, None, None, None, false),
            ("grok".into(), vec![], false)
        );
        let (program, args, resumed) = agent_spawn_command_with(
            &grok,
            Some("session-id"),
            Some(Path::new(TEST_CWD)),
            Some("model-id"),
            Some("high"),
            None,
            Some("fix the bug; keep this as one argument"),
            Some("Review only PR #42"),
            false,
        );
        assert_eq!(program, "grok");
        assert!(resumed);
        assert_eq!(
            args,
            vec![
                "--resume",
                "session-id",
                "--model",
                "model-id",
                "--reasoning-effort",
                "high",
                "--rules",
                "Review only PR #42",
                "fix the bug; keep this as one argument",
            ]
        );
        assert_eq!(grok.hook_dialect(), None);
        assert_eq!(grok.permissions_flag, None);
    }

    #[test]
    fn copilot_spawn_rides_its_prompt_on_dash_i_and_resumes_by_flag() {
        let copilot = nebula_core::harness::builtin("copilot").unwrap();
        assert_eq!(
            agent_spawn_command_with(&copilot, None, None, None, None, None, None, None, false),
            ("copilot".into(), vec![], false)
        );
        // Its positional is a subcommand: a trailing prompt would be read
        // as one, so the first turn rides `-i`.
        let (program, args, resumed) = agent_spawn_command_with(
            &copilot,
            Some("7ca4e4d4-9b63-4d30-ab86-7bd455ab60c0"),
            Some(Path::new(TEST_CWD)),
            Some("auto"),
            Some("high"),
            None,
            Some("fix the bug; keep this as one argument"),
            None,
            false,
        );
        assert_eq!(program, "copilot");
        assert!(resumed);
        assert_eq!(
            args,
            vec![
                "--resume",
                "7ca4e4d4-9b63-4d30-ab86-7bd455ab60c0",
                "--model",
                "auto",
                "--reasoning-effort",
                "high",
                "-i",
                "fix the bug; keep this as one argument",
            ]
        );
        assert_eq!(copilot.hook_dialect(), Some(AgentKind::Copilot));
    }

    #[test]
    fn spawn_command_initial_prompt_is_the_trailing_positional_argument() {
        let all = test_registry();
        let claude = test_harness(&all, AgentKind::Claude);
        let codex = test_harness(&all, AgentKind::Codex);
        let cursor = test_harness(&all, AgentKind::Cursor);
        let pi = test_harness(&all, AgentKind::Pi);
        // The relocation notice trails everything, guidance included.
        let (_, args, resumed) = agent_spawn_command_with(
            &claude,
            Some("sid"),
            Some(Path::new(TEST_CWD)),
            Some("opus"),
            None,
            None,
            Some("carry on"),
            None,
            true,
        );
        assert!(resumed);
        let mut expected = guided(
            "--append-system-prompt",
            &["--resume", "sid", "--model", "opus"],
        );
        expected.push("carry on".into());
        assert_eq!(args, expected);
        // Codex and cursor take it as their trailing positional too.
        assert_eq!(
            agent_spawn_command_with(
                &codex,
                Some("sid"),
                Some(Path::new(TEST_CWD)),
                None,
                None,
                None,
                Some("carry on"),
                None,
                true,
            )
            .1,
            vec!["resume", "sid", "--cd", TEST_CWD, "--yolo", "carry on"]
        );
        assert_eq!(
            agent_spawn_command_with(
                &cursor,
                Some("sid"),
                Some(Path::new(TEST_CWD)),
                None,
                None,
                None,
                Some("carry on"),
                None,
                true,
            )
            .1,
            vec!["--resume", "sid", "--force", "carry on"]
        );
        // A fresh spawn with a starting prompt (an AGENT PRESET launch):
        // model, effort and system prompt all precede it.
        let mut expected = guided(
            "--append-system-prompt",
            &["--model", "opus", "--effort", "high"],
        );
        expected.push("fix auth".into());
        assert_eq!(
            agent_spawn_command_with(
                &claude,
                None,
                Some(Path::new(TEST_CWD)),
                Some("opus"),
                Some("high"),
                None,
                Some("fix auth"),
                None,
                true,
            )
            .1,
            expected
        );
        assert_eq!(
            agent_spawn_command_with(
                &codex,
                None,
                Some(Path::new(TEST_CWD)),
                Some("gpt-5.5"),
                Some("high"),
                None,
                Some("fix auth"),
                None,
                true,
            )
            .1,
            vec![
                "--yolo",
                "--model",
                "gpt-5.5",
                "-c",
                "model_reasoning_effort=high",
                "fix auth"
            ]
        );
        assert_eq!(
            agent_spawn_command_with(
                &cursor,
                None,
                Some(Path::new(TEST_CWD)),
                None,
                None,
                None,
                Some("fix auth"),
                None,
                true,
            )
            .1,
            vec!["--force", "fix auth"]
        );
        // Pi: the prompt trails the resume id and the guidance, as pi's
        // `[messages...]` positional.
        let mut expected = guided("--append-system-prompt", &["--session-id", "sid"]);
        expected.push("carry on".into());
        assert_eq!(
            agent_spawn_command_with(
                &pi,
                Some("sid"),
                Some(Path::new(TEST_CWD)),
                None,
                None,
                None,
                Some("carry on"),
                None,
                true,
            )
            .1,
            expected
        );
        // An override is verbatim: no guidance, no prompt.
        assert_eq!(
            agent_spawn_command_with(
                &claude,
                None,
                Some(Path::new(TEST_CWD)),
                None,
                None,
                Some("/bin/sh -i"),
                Some("carry on"),
                None,
                true,
            ),
            ("/bin/sh".into(), vec!["-i".to_string()], false)
        );
    }

    /// OpenCode's positional is the project path, so a starting prompt —
    /// an AGENT PRESET's task, a PR SESSION's rule — rides `--prompt`
    /// after the model flag, where every other CLI takes a trailing
    /// positional; the field changes nothing for them.
    #[test]
    fn opencode_first_prompt_rides_its_flag() {
        let all = test_registry();
        let opencode = test_harness(&all, AgentKind::OpenCode);
        let (program, args, resumed) = agent_spawn_command_with(
            &opencode,
            None,
            Some(Path::new(TEST_CWD)),
            Some("opencode/big-pickle"),
            None,
            None,
            Some("Fix auth"),
            None,
            true,
        );
        assert_eq!(program, "opencode");
        assert_eq!(
            args,
            ["--model", "opencode/big-pickle", "--prompt", "Fix auth"]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert!(!resumed);
        let claude = test_harness(&all, AgentKind::Claude);
        let (_, args, _) = agent_spawn_command_with(
            &claude,
            None,
            Some(Path::new(TEST_CWD)),
            None,
            None,
            None,
            Some("Fix auth"),
            None,
            false,
        );
        assert_eq!(args, vec!["Fix auth".to_string()]);
    }

    /// The relocation notice reaches the CLIs whose resume submits a
    /// trailing prompt — Claude, codex and pi — and names the checkout;
    /// cursor's is unverified and OpenCode's `--session <id> --prompt` is
    /// not submitted (verified on opencode 1.18.32), so their relocated
    /// sessions reopen silent. (Codex was gated out until #39: `codex
    /// resume <id> --yolo` sat at Ready in the worktree until the user
    /// typed "continue".)
    #[test]
    fn relocation_prompt_reaches_every_kind_but_cursor_and_opencode() {
        let feat = Worktree {
            id: WorktreeId("feat".into()),
            project_id: ProjectId("p".into()),
            path: "/nebula-test/p-feat".into(),
            branch: "feat".into(),
            is_main: false,
            sort_order: 0,
        };
        let all = test_registry();
        for kind in [AgentKind::Claude, AgentKind::Codex, AgentKind::Pi] {
            let harness = test_harness(&all, kind);
            assert!(harness.relocation_prompt, "{kind:?} maps the notice");
            let prompt = relocation_prompt(harness.relocation_prompt, &feat)
                .unwrap_or_else(|| panic!("{kind:?}"));
            assert!(prompt.contains("`feat`"), "{kind:?}: {prompt}");
            assert!(prompt.contains("/nebula-test/p-feat"), "{kind:?}: {prompt}");
            assert!(prompt.contains("Continue the user's most recent request"));
        }
        let cursor = test_harness(&all, AgentKind::Cursor);
        assert_eq!(relocation_prompt(cursor.relocation_prompt, &feat), None);
        let opencode = test_harness(&all, AgentKind::OpenCode);
        assert_eq!(relocation_prompt(opencode.relocation_prompt, &feat), None);

        // And the codex respawn it feeds: resumed, re-rooted in the
        // worktree, and opening on the notice.
        let codex = test_harness(&all, AgentKind::Codex);
        let notice = relocation_prompt(codex.relocation_prompt, &feat).unwrap();
        let (program, args, resumed) = agent_spawn_command_with(
            &codex,
            Some("sid"),
            Some(&feat.path),
            None,
            None,
            None,
            Some(&notice),
            None,
            true,
        );
        assert_eq!(program, "codex");
        assert!(resumed);
        assert_eq!(
            args,
            vec![
                "resume",
                "sid",
                "--cd",
                "/nebula-test/p-feat",
                "--yolo",
                notice.as_str()
            ]
        );
    }

    #[test]
    fn spawn_command_keeps_pr_scope_and_url_in_claudes_system_prompt() {
        let pr_url = "https://github.com/AgentSystemLabs/nebula/pull/42";
        let pr_prompt = crate::pr_scope::rule(&crate::pr_scope::PrScope {
            url: pr_url,
            worktree: Path::new("/w/nebula-worktrees/fix"),
            branch: "fix",
            root: Some(Path::new("/w/nebula")),
        });
        let all = test_registry();
        let claude = test_harness(&all, AgentKind::Claude);
        let (_, args, resumed) = agent_spawn_command_with(
            &claude,
            Some("sid"),
            Some(Path::new(TEST_CWD)),
            None,
            None,
            None,
            None,
            Some(&pr_prompt),
            true,
        );
        assert!(resumed);
        let prompts = args
            .windows(2)
            .filter(|pair| pair[0] == "--append-system-prompt")
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>();
        assert_eq!(prompts.len(), 1, "Claude gets one composed system prompt");
        assert!(prompts[0].contains(CLAUDE_WORKTREE_GUIDANCE));
        assert!(prompts[0].contains(crate::sibling::CLAUDE_SPAWN_GUIDANCE));
        assert!(prompts[0].contains(crate::open_files::CLAUDE_OPEN_GUIDANCE));
        assert!(prompts[0].contains("All work in this session must be scoped"));
        assert!(prompts[0].contains(pr_url));
        assert!(prompts[0].contains("/w/nebula-worktrees/fix"));
    }

    #[test]
    fn spawn_command_claude_cloud_passes_the_task_as_one_argument() {
        let all = test_registry();
        let claude = test_harness(&all, AgentKind::Claude);
        assert_eq!(
            claude_cloud_spawn_command(
                &claude,
                "Fix auth\nRun tests; don't stop",
                Some("opus"),
                Some("high"),
                None,
            ),
            (
                "claude".into(),
                vec![
                    "--cloud=Fix auth\nRun tests; don't stop".to_string(),
                    "--model".to_string(),
                    "opus".to_string(),
                    "--effort".to_string(),
                    "high".to_string(),
                ],
                false,
            )
        );
        assert_eq!(
            claude_cloud_spawn_command(&claude, "--dangerously-skip-permissions", None, None, None)
                .1,
            vec!["--cloud=--dangerously-skip-permissions"]
        );
        // Overrides (tests) stay verbatim — no cloud flag at all.
        assert_eq!(
            claude_cloud_spawn_command(&claude, "task", None, None, Some("/bin/true")).1,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn send_cloud_message_requires_a_cloud_session() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", "/tmp", true);
        seed_agent(&daemon, "local", "w", None);
        let err = daemon
            .send_cloud_message(&AgentId("local".into()), "hi")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not launched in Claude Cloud"),
            "{err}"
        );
    }

    /// A Cloud row never gets a local CLI booted in its name: the agent
    /// runs in the cloud sandbox, and the row's pane links there. Both
    /// paths that would otherwise fork a bare `claude` — a restart, and an
    /// attach finding no PTY — refuse instead.
    #[tokio::test]
    async fn cloud_row_is_never_respawned_locally() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", "/tmp", true);
        seed_agent(&daemon, "cloud", "w", None);
        let id = AgentId("cloud".into());
        daemon
            .store
            .set_agent_cloud_session_id(&id, Some("session_016SiQW5Lem2LbnUf1A3undt"))
            .unwrap();

        let err = daemon.restart_agent(&id).await.unwrap_err();
        assert!(err.to_string().contains("runs in Claude Cloud"), "{err}");
        let err = daemon
            .ensure_session(&SessionRef::Agent(id.clone()), 80, 24)
            .err()
            .expect("a cloud row's attach is refused");
        assert!(err.to_string().contains("runs in Claude Cloud"), "{err}");
        assert!(!daemon.is_alive(&SessionRef::Agent(id)));
    }

    /// A worktree whose checkout was removed outside nebula (`git worktree
    /// remove` by an agent, `rm -rf`) keeps its row while sessions hang off
    /// it. Booting one of those sessions must refuse with the reason, not
    /// run in `$HOME` and not recreate the path: a Claude spawn installs its
    /// hooks into `<worktree>/.claude/` first, which used to create the
    /// whole directory again, empty and outside git.
    #[tokio::test]
    async fn sessions_in_a_deleted_checkout_refuse_and_create_nothing() {
        // `/bin/cat` stands in for the CLI, so a regression boots nothing real.
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let daemon = test_daemon();
        let gone = tempfile::tempdir().unwrap();
        let path = gone.path().join("feature");
        std::fs::create_dir(&path).unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", path.to_str().unwrap(), false);
        seed_agent(&daemon, "a", "w", Some("sid-1"));
        daemon
            .store
            .insert_terminal(&TerminalTab {
                id: TerminalId("t".into()),
                worktree_id: WorktreeId("w".into()),
                name: "shell".into(),
                sort_order: 0,
                alive: false,
                run_command: None,
            })
            .unwrap();
        std::fs::remove_dir(&path).unwrap();

        for sref in [
            SessionRef::Agent(AgentId("a".into())),
            SessionRef::Terminal(TerminalId("t".into())),
        ] {
            let err = daemon
                .ensure_session(&sref, 80, 24)
                .err()
                .expect("a session in a deleted checkout is refused");
            assert!(err.to_string().contains("gone from disk"), "{err}");
            assert!(!daemon.is_alive(&sref));
        }
        let err = daemon
            .restart_agent(&AgentId("a".into()))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("gone from disk"), "{err:#}");
        assert!(!path.exists(), "the deleted checkout was recreated");

        // What the refusal tells the user to do: the checkout back where it
        // was, and the same rows start again.
        std::fs::create_dir(&path).unwrap();
        let term = SessionRef::Terminal(TerminalId("t".into()));
        daemon.ensure_session(&term, 80, 24).unwrap();
        assert!(daemon.is_alive(&term));
        daemon.kill_session(&term);
    }

    /// A session still running in a checkout that has since been deleted
    /// keeps running through a restart that refuses: the check comes before
    /// the kill, so the restart never turns a live session into a dead one.
    #[cfg(unix)]
    #[tokio::test]
    async fn restart_in_a_deleted_checkout_keeps_the_live_session() {
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let daemon = test_daemon();
        let gone = tempfile::tempdir().unwrap();
        let path = gone.path().join("feature");
        std::fs::create_dir(&path).unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", path.to_str().unwrap(), false);
        seed_agent(&daemon, "a", "w", None);
        let sref = SessionRef::Agent(AgentId("a".into()));
        daemon.ensure_session(&sref, 80, 24).unwrap();
        // The boot installed its hooks in the checkout: remove the lot, as
        // `git worktree remove` does.
        std::fs::remove_dir_all(&path).unwrap();

        let err = daemon
            .restart_agent(&AgentId("a".into()))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("gone from disk"), "{err:#}");
        assert!(
            daemon.is_alive(&sref),
            "the restart killed the live session"
        );
        daemon.kill_session(&sref);
    }

    /// A data dir whose config.json defines a Claude profile `work` and a
    /// Codex profile `gpt`.
    fn profiles_data_dir() -> tempfile::TempDir {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("config.json"),
            r#"{"profiles": [
                {"name": "work", "harness": "claude"},
                {"name": "off", "harness": "claude", "disabled": true},
                {"name": "gpt", "harness": "codex"}
            ]}"#,
        )
        .unwrap();
        data
    }

    /// A switch that cannot happen says why before anything is killed or
    /// written: the row keeps its account and its session id.
    #[tokio::test]
    async fn switching_account_refuses_before_touching_the_row() {
        let data = profiles_data_dir();
        let _env = EnvGuard::set_all(&[
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", dir.path().to_str().unwrap(), true);
        seed_agent(&daemon, "a", "w", Some("sid-1"));
        let id = AgentId("a".into());
        for (to, why) in [
            (Some("gone"), "no longer defined"),
            (Some("gpt"), "is a codex account"),
            (None, "already runs on its harness's own login"),
            (Some("  "), "already runs on its harness's own login"),
        ] {
            let err = daemon.switch_agent_account(&id, to).await.unwrap_err();
            assert!(err.to_string().contains(why), "{to:?}: {err}");
        }
        let row = daemon.store.get_agent(&id).unwrap().unwrap();
        assert_eq!(row.profile, None);
        assert_eq!(row.session_id.as_deref(), Some("sid-1"));

        // Archived and Cloud rows never switch.
        daemon.store.set_agent_archived(&id, true).unwrap();
        let err = daemon
            .switch_agent_account(&id, Some("work"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("archived"), "{err}");
        daemon.store.set_agent_archived(&id, false).unwrap();
        daemon
            .store
            .set_agent_cloud_session_id(&id, Some("session_x"))
            .unwrap();
        let err = daemon
            .switch_agent_account(&id, Some("work"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Claude Cloud"), "{err}");
        assert_eq!(daemon.store.get_agent(&id).unwrap().unwrap().profile, None);
    }

    /// A renamed profile: every row naming it follows, archived ones too,
    /// and a row on another profile (or none) is left alone.
    #[tokio::test]
    async fn a_renamed_profile_is_followed_by_every_session_naming_it() {
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", dir.path().to_str().unwrap(), true);
        for (id, profile) in [
            ("a", Some(" work")),
            ("b", Some("work")),
            ("c", Some("home")),
            ("d", None),
        ] {
            seed_agent(&daemon, id, "w", None);
            daemon
                .store
                .set_agent_profile(&AgentId(id.into()), profile)
                .unwrap();
        }
        daemon
            .store
            .set_agent_archived(&AgentId("b".into()), true)
            .unwrap();
        let mut rx = daemon.events.subscribe();
        daemon.rename_profile_sessions("work", " max2 ").unwrap();
        let profile = |id: &str| {
            daemon
                .store
                .get_agent(&AgentId(id.into()))
                .unwrap()
                .unwrap()
                .profile
        };
        assert_eq!(profile("a").as_deref(), Some("max2"));
        assert_eq!(
            profile("b").as_deref(),
            Some("max2"),
            "archived ones follow"
        );
        assert_eq!(profile("c").as_deref(), Some("home"));
        assert_eq!(profile("d"), None);
        let mut upserts = 0;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, ServerEvent::EntityUpserted { .. }) {
                upserts += 1;
            }
        }
        assert_eq!(upserts, 2, "each row that changed is broadcast");
        assert!(daemon.rename_profile_sessions("", "x").is_err());
    }

    /// The switch: the row takes the new account, drops its session id
    /// (the conversation lives in the old account's config home) and the
    /// agent boots again in the same worktree. A switched-off profile can
    /// be named, and back to the harness's own login works too.
    #[tokio::test]
    async fn switching_account_restarts_fresh_on_the_new_profile() {
        #[cfg(unix)]
        let stand_in = "/bin/cat";
        #[cfg(windows)]
        let stand_in = "cmd.exe";
        let data = profiles_data_dir();
        let _env = EnvGuard::set_all(&[
            (env::AGENT_CMD, stand_in),
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", dir.path().to_str().unwrap(), true);
        seed_agent(&daemon, "a", "w", Some("sid-1"));
        let id = AgentId("a".into());
        let sref = SessionRef::Agent(id.clone());

        daemon
            .switch_agent_account(&id, Some("work"))
            .await
            .unwrap();
        let row = daemon.store.get_agent(&id).unwrap().unwrap();
        assert_eq!(row.profile.as_deref(), Some("work"));
        assert_eq!(row.session_id, None, "a fresh start on the new account");
        assert_eq!(row.worktree_id.0, "w", "same worktree");
        assert!(daemon.is_alive(&sref));
        assert!(
            data.path()
                .join("profiles")
                .join("work")
                .join("claude")
                .is_dir(),
            "the new account's home was prepared for the boot"
        );

        daemon.switch_agent_account(&id, Some("off")).await.unwrap();
        assert_eq!(
            daemon
                .store
                .get_agent(&id)
                .unwrap()
                .unwrap()
                .profile
                .as_deref(),
            Some("off"),
            "a named profile runs even when switched off"
        );
        daemon.switch_agent_account(&id, None).await.unwrap();
        assert_eq!(daemon.store.get_agent(&id).unwrap().unwrap().profile, None);
        assert!(daemon.is_alive(&sref));
        daemon.kill_session(&sref);
    }

    fn start_subagent(daemon: &Daemon, parent: &AgentId, id: &str) {
        daemon.track_subagent(
            parent,
            &crate::status::HookEvent::SubagentStart {
                subagent_id: Some(id.into()),
            },
            SubagentHookInfo::default(),
        );
    }

    fn subagent_status(daemon: &Daemon, id: &str) -> Option<nebula_core::SubagentStatus> {
        daemon
            .subagent_rows()
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| r.status)
    }

    /// Restart, account switch and a move into its worktree each drop the
    /// old process's subagent rows before the new process starts.
    #[tokio::test]
    async fn every_restart_path_reaps_the_old_process_subagents() {
        #[cfg(unix)]
        let stand_in = "/bin/cat";
        #[cfg(windows)]
        let stand_in = "cmd.exe";
        let data = profiles_data_dir();
        let _env = EnvGuard::set_all(&[
            (env::AGENT_CMD, stand_in),
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", dir.path().to_str().unwrap(), true);
        seed_agent(&daemon, "a", "w", None);
        let id = AgentId("a".into());
        let sref = SessionRef::Agent(id.clone());
        daemon.ensure_session(&sref, 80, 24).unwrap();

        start_subagent(&daemon, &id, "before-restart");
        daemon.restart_agent(&id).await.unwrap();
        assert!(daemon.subagent_rows().is_empty(), "restart reaps");

        start_subagent(&daemon, &id, "before-switch");
        daemon
            .switch_agent_account(&id, Some("work"))
            .await
            .unwrap();
        assert!(daemon.subagent_rows().is_empty(), "account switch reaps");

        start_subagent(&daemon, &id, "before-move");
        let home = daemon
            .store
            .get_worktree(&WorktreeId("w".into()))
            .unwrap()
            .unwrap();
        // Its return says whether the stand-in opened on the move; the
        // kill and the reap happen either way.
        daemon.relocate_into(&id, &home, false);
        assert!(daemon.subagent_rows().is_empty(), "relocation reaps");
        daemon.kill_session(&sref);
    }

    /// A deliberate kill (archive, the idle reap) closes the session's open
    /// subagents at once. The killed PTY's exit lands later, after a
    /// respawn has started subagents of its own, and leaves those running.
    #[tokio::test]
    async fn a_kill_closes_its_subagents_and_its_late_exit_spares_the_respawn() {
        #[cfg(unix)]
        let stand_in = "/bin/cat";
        #[cfg(windows)]
        let stand_in = "cmd.exe";
        let _cmd = EnvGuard::set(env::AGENT_CMD, stand_in);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", dir.path().to_str().unwrap(), true);
        seed_agent(&daemon, "a", "w", None);
        let id = AgentId("a".into());
        let sref = SessionRef::Agent(id.clone());
        daemon.ensure_session(&sref, 80, 24).unwrap();
        start_subagent(&daemon, &id, "old");

        daemon.kill_session(&sref);
        assert_eq!(
            subagent_status(&daemon, "old"),
            Some(nebula_core::SubagentStatus::EndedUnknown),
            "closed with the kill"
        );

        // Respawned before the old PTY's exit has landed.
        daemon.ensure_session(&sref, 80, 24).unwrap();
        start_subagent(&daemon, &id, "new");
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        assert_eq!(
            subagent_status(&daemon, "new"),
            Some(nebula_core::SubagentStatus::Running),
            "the old exit closed the new process's subagent"
        );
        daemon.kill_session(&sref);
        assert_eq!(
            subagent_status(&daemon, "new"),
            Some(nebula_core::SubagentStatus::EndedUnknown)
        );
    }

    /// A create naming no profile takes its project's default account for
    /// the harness; one naming a profile keeps it; another project gets
    /// the usual pick (manual, no default: the CLI's own login).
    #[test]
    fn a_create_naming_no_profile_takes_the_project_default() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("config.json"),
            r#"{"profiles": [
                {"name": "work", "harness": "claude"},
                {"name": "home", "harness": "claude"}
            ], "projects": {"/tmp/p": {"default_profiles": {"claude": "work"}}}}"#,
        )
        .unwrap();
        let _env = EnvGuard::set_all(&[
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let claude = resolve_harness(AgentKind::Claude, None).unwrap();
        let p = Some(Path::new("/tmp/p"));
        assert_eq!(
            resolve_profile_name(&claude, None, p).unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(
            resolve_profile_name(&claude, Some("home"), p)
                .unwrap()
                .as_deref(),
            Some("home")
        );
        assert_eq!(
            resolve_profile_name(&claude, None, Some(Path::new("/tmp/q"))).unwrap(),
            None
        );
        assert!(resolve_profile_name(&claude, Some("gone"), p).is_err());
    }

    /// `nebula worktree <branch>` onto a row whose checkout is gone refuses
    /// before anything moves: the agent's row stays where it was and no
    /// relocation is queued, so the turn end never kills it for a respawn
    /// that would refuse.
    #[tokio::test]
    async fn entering_a_deleted_checkout_refuses_and_moves_nothing() {
        let daemon = test_daemon();
        let root = tempfile::tempdir().unwrap();
        let here = root.path().join("here");
        let there = root.path().join("there");
        std::fs::create_dir(&here).unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "here", here.to_str().unwrap(), false);
        seed_worktree(&daemon, "p", "there", there.to_str().unwrap(), false);
        seed_agent(&daemon, "a", "here", None);
        let id = AgentId("a".into());

        let err = daemon.enter_worktree(&id, "there", None).await.unwrap_err();
        assert!(err.to_string().contains("gone from disk"), "{err}");
        let agent = daemon.store.get_agent(&id).unwrap().unwrap();
        assert_eq!(agent.worktree_id, WorktreeId("here".into()));
        assert!(!daemon.relocation_pending(&id));
    }

    #[test]
    fn cloud_text_is_trimmed_and_bounded() {
        assert_eq!(
            validate_cloud_text("  fix auth  ", "task").unwrap(),
            "fix auth"
        );
        // Newlines are part of a multi-row task/message; only the ends go.
        assert_eq!(
            validate_cloud_text("\nline one\nline two\n", "message").unwrap(),
            "line one\nline two"
        );
        for bad in ["", "   ", "\n"] {
            assert!(validate_cloud_text(bad, "task").is_err(), "{bad:?}");
        }
        // A NUL would truncate the login shell's -c string.
        assert!(validate_cloud_text("fix\0auth", "task").is_err());
        assert!(validate_cloud_text(&"x".repeat(MAX_CLOUD_PROMPT_BYTES), "task").is_ok());
        assert!(validate_cloud_text(&"x".repeat(MAX_CLOUD_PROMPT_BYTES + 1), "task").is_err());
        // The label rides into the message the user sees.
        let err = validate_cloud_text("", "message").unwrap_err().to_string();
        assert!(err.contains("message"), "{err}");
    }

    /// What every agent launch runs once the login shell's files have run,
    /// ahead of the command itself.
    #[cfg(unix)]
    const PANE_ENV: &str =
        "unset NO_COLOR FORCE_COLOR; export TERM=xterm-256color COLORTERM=truecolor;";

    #[cfg(unix)]
    #[test]
    fn cli_probe_line_looks_the_program_up_verbatim() {
        // Built-ins read exactly as before the registry.
        assert_eq!(
            cli_probe_line("claude"),
            "command -v 'claude' >/dev/null 2>&1"
        );
        assert_eq!(
            cli_probe_line("cursor-agent"),
            "command -v 'cursor-agent' >/dev/null 2>&1"
        );
        // A config-named program is any string. A quote in it stays
        // inside the word instead of closing it and opening a command.
        let hostile = "x'; echo INJECTED; echo '";
        let line = cli_probe_line(hostile);
        assert_eq!(
            line,
            "command -v 'x'\\''; echo INJECTED; echo '\\''' >/dev/null 2>&1"
        );
        // And a real shell agrees: the lookup fails quietly, nothing runs.
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &line])
            .output()
            .expect("/bin/sh");
        assert!(!out.status.success(), "no such program");
        assert!(
            out.stdout.is_empty(),
            "{:?}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_wrap_quotes_args_and_leaves_the_command_word_bare() {
        let (program, args) = login_shell_wrap(
            "/bin/zsh",
            "claude",
            &["--resume".to_string(), "sid-1".to_string()],
        );
        assert_eq!(program, "/bin/sh");
        assert_eq!(
            args,
            vec![
                "-c",
                LOGIN_SHELL_STDIN_SHIM,
                "nebula-login-shell",
                "/bin/zsh",
                "-l",
                "-i",
                "-c",
                &format!("exec </dev/tty; {PANE_ENV} claude '--resume' 'sid-1'")
            ]
        );
        // Single quotes in an arg survive the wrapping.
        let (_, args) = login_shell_wrap("/bin/zsh", "echo", &["it's".to_string()]);
        assert_eq!(
            args[7],
            format!(r"exec </dev/tty; {PANE_ENV} echo 'it'\''s'")
        );
        // A command word that isn't a plain name is quoted like an argument.
        let (_, args) = login_shell_wrap("/bin/zsh", "my tool", &[]);
        assert_eq!(args[7], format!("exec </dev/tty; {PANE_ENV} 'my tool'"));
    }

    /// The command word resolves through the shell, so an alias or function
    /// from the rc files takes precedence over the binary on PATH — the
    /// reason a launch goes through the login shell at all. A function
    /// stands in for the alias: every POSIX sh honours one in a `-c` string,
    /// and the old `exec env` form bypassed both alike.
    /// A PROFILE's environment edit rides the line after the rc files, so
    /// a key exported there cannot win the login back.
    #[cfg(unix)]
    #[test]
    fn a_profile_edit_rides_the_launch_line() {
        let env = nebula_core::profiles::LaunchEnv {
            unset: vec!["ANTHROPIC_API_KEY".into(), "CLAUDE_CONFIG_DIR".into()],
            set: vec![("CLAUDE_CONFIG_DIR".into(), "/p/it's".into())],
        };
        assert_eq!(
            profile_env_line(&env),
            r"unset ANTHROPIC_API_KEY; export CLAUDE_CONFIG_DIR='/p/it'\''s'; "
        );
        let (_, args) = login_shell_wrap_env("/bin/zsh", "claude", &[], &env);
        assert!(
            args[7].ends_with(&format!("{} claude", profile_env_line(&env).trim_end())),
            "{}",
            args[7]
        );
        assert_eq!(profile_env_line(&Default::default()), "");
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_wrap_lets_the_shell_resolve_the_command() {
        let (_, args) = login_shell_wrap(
            "/bin/sh",
            "claude",
            &["--resume".to_string(), "sid-1".to_string()],
        );
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "claude() {{ printf 'routed %s' \"$*\"; }}; {}",
                args[7]
                    .strip_prefix("exec </dev/tty; ")
                    .expect("launch line restores the PTY before the command")
            ))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "routed --resume sid-1",
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The shape of the report itself: zsh, `-l -i -c`, and an alias in
    /// `.zshrc` that reroutes `claude`. Skipped where there is no zsh.
    #[cfg(unix)]
    #[test]
    fn login_shell_wrap_honours_a_zshrc_alias() {
        use std::os::unix::process::CommandExt;
        if std::process::Command::new("zsh")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .status()
            .is_err()
        {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(".zshrc"), "alias claude='echo routed'\n").unwrap();
        let (_program, args) = login_shell_wrap(
            "zsh",
            "claude",
            &["--resume".to_string(), "sid-1".to_string()],
        );
        let mut cmd = std::process::Command::new(args[3].as_str());
        let mut zsh_args = args[4..7].to_vec();
        zsh_args.push(
            args[7]
                .strip_prefix("exec </dev/tty; ")
                .expect("launch line restores the PTY before the command")
                .to_string(),
        );
        cmd.args(&zsh_args)
            .env("ZDOTDIR", home.path())
            .stdin(std::process::Stdio::null());
        // Own session, as the daemon's CLI probe does: an interactive zsh
        // must not make itself the foreground of the terminal running the
        // tests.
        unsafe {
            cmd.pre_exec(own_session);
        }
        let out = cmd.output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            "routed --resume sid-1",
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The command line's `env` really does undo a profile's colour
    /// overrides and restate the pane: run it through a plain `sh -c` with
    /// `NO_COLOR` and a foreign `TERM` already exported, as a `.profile`
    /// would leave them, and read back what the program sees.
    #[cfg(unix)]
    #[test]
    fn login_shell_wrap_restates_the_pane_after_the_profile() {
        let (_, args) = login_shell_wrap(
            "/bin/sh",
            "sh",
            &[
                "-c".to_string(),
                r#"printf '%s|%s|%s|%s' "$TERM" "$COLORTERM" "${NO_COLOR-unset}" "${FORCE_COLOR-unset}""#
                    .to_string(),
            ],
        );
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(
                args[7]
                    .strip_prefix("exec </dev/tty; ")
                    .expect("launch line restores the PTY before the command"),
            )
            .env("NO_COLOR", "1")
            .env("FORCE_COLOR", "0")
            .env("TERM", "foot")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "xterm-256color|truecolor|unset|unset",
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn test_daemon() -> Arc<Daemon> {
        let store = Arc::new(Store::open_in_memory().unwrap());
        Daemon::new(
            store,
            HookEnv {
                port: 0,
                token: String::new(),
            },
        )
    }

    #[test]
    fn claude_projects_dirs_follow_the_transcripts_hooks_reported() {
        // An agent's shell may point CLAUDE_CONFIG_DIR somewhere the daemon's
        // own env never heard of; its hooks name the real transcript.
        let daemon = test_daemon();
        let reported = crate::session_title::TranscriptRef::from_payload(
            Some("/cfg/alt/projects/-w-feat/sid-9.jsonl"),
            Some("sid-9"),
        )
        .unwrap();
        daemon
            .transcripts
            .lock()
            .insert(AgentId("a1".into()), reported);
        let dirs = daemon.claude_projects_dirs();
        assert!(
            dirs.contains(&PathBuf::from("/cfg/alt/projects")),
            "{dirs:?}"
        );
    }

    /// A project whose main checkout is a fresh directory, registered with
    /// `daemon`, for the RUN TERMINAL tests to write `.nebula.json` into.
    fn run_worktree(daemon: &Daemon) -> (tempfile::TempDir, Worktree) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "demo".into(),
            repo_path: dir.path().to_path_buf(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id,
            path: dir.path().to_path_buf(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        daemon.store.insert_worktree(&worktree).unwrap();
        (dir, worktree)
    }

    #[tokio::test]
    async fn run_needs_a_project_file_starts_once_and_stops() {
        let daemon = test_daemon();
        let (dir, worktree) = run_worktree(&daemon);

        let missing = daemon.start_run(&worktree.id).unwrap_err();
        assert!(
            missing.to_string().contains("Settings (s) → Project")
                && missing.to_string().contains(".kete.json"),
            "names both places: {missing}"
        );
        assert!(daemon
            .store
            .run_terminals_in(&worktree.id)
            .unwrap()
            .is_empty());

        std::fs::write(dir.path().join(".kete.json"), r#"{"run": "sleep 30"}"#).unwrap();
        let EntityId::Terminal(id) = daemon.start_run(&worktree.id).unwrap() else {
            panic!("a run lives in a terminal");
        };
        let sref = SessionRef::Terminal(id.clone());
        assert!(daemon.is_alive(&sref));
        let row = daemon.store.get_terminal(&id).unwrap().unwrap();
        assert_eq!(row.run_command.as_deref(), Some("sleep 30"));

        // A second press while it runs starts nothing new.
        assert_eq!(
            daemon.start_run(&worktree.id).unwrap(),
            EntityId::Terminal(id.clone())
        );
        assert_eq!(
            daemon.store.run_terminals_in(&worktree.id).unwrap().len(),
            1
        );

        daemon.stop_run(&worktree.id).unwrap();
        assert!(!daemon.is_alive(&sref));
        assert!(daemon
            .store
            .run_terminals_in(&worktree.id)
            .unwrap()
            .is_empty());
        // Nothing left to stop is not an error.
        daemon.stop_run(&worktree.id).unwrap();
    }

    /// The project's `run_command` setting (Settings → Project) is what
    /// `r` runs when it is set, over whatever `.nebula.json` says; blank,
    /// the file decides as before; a project's setting is its own; and a
    /// file that won't parse is still that file's error, not "no command".
    #[tokio::test]
    async fn the_project_setting_wins_over_the_project_file() {
        let daemon = test_daemon();
        let (dir, worktree) = run_worktree(&daemon);
        let config =
            |json: String| -> crate::config::Config { serde_json::from_str(&json).unwrap() };
        let entry = |path: &Path, run: &str| {
            config(format!(
                r#"{{"projects": {{{}: {{"run_command": "{run}"}}}}}}"#,
                serde_json::to_string(&path.display().to_string()).unwrap()
            ))
        };
        let run_with = |cfg: &crate::config::Config| -> String {
            let EntityId::Terminal(id) = daemon.start_run_with(&worktree.id, cfg).unwrap() else {
                panic!("a run lives in a terminal");
            };
            let command = daemon.store.get_terminal(&id).unwrap().unwrap().run_command;
            daemon.stop_run(&worktree.id).unwrap();
            command.unwrap()
        };

        // No file: the setting is the whole answer.
        assert_eq!(run_with(&entry(dir.path(), "sleep 31")), "sleep 31");
        // Both: the setting wins.
        std::fs::write(dir.path().join(".kete.json"), r#"{"run": "sleep 30"}"#).unwrap();
        assert_eq!(run_with(&entry(dir.path(), "sleep 31")), "sleep 31");
        // Blank, or another project's: the file.
        assert_eq!(run_with(&entry(dir.path(), "   ")), "sleep 30");
        assert_eq!(
            run_with(&entry(Path::new("/tmp/other"), "sleep 31")),
            "sleep 30"
        );
        // Neither: the message names both.
        std::fs::remove_file(dir.path().join(".kete.json")).unwrap();
        let err = daemon
            .start_run_with(&worktree.id, &entry(dir.path(), ""))
            .unwrap_err();
        assert!(err.to_string().contains("Settings (s) → Project"), "{err}");
        // A broken file is its own complaint, whatever the setting isn't.
        std::fs::write(dir.path().join(".kete.json"), "{").unwrap();
        let err = daemon
            .start_run_with(&worktree.id, &crate::config::Config::default())
            .unwrap_err();
        assert!(err.to_string().contains(".kete.json:"), "{err}");
        assert_eq!(
            run_with(&entry(dir.path(), "sleep 31")),
            "sleep 31",
            "and the setting never opens the file"
        );
    }

    /// A run that ends on its own keeps its output for the attach that
    /// comes to read it, and never runs again on that attach — only `r`
    /// starts a RUN COMMAND.
    #[tokio::test]
    async fn an_exited_run_replays_instead_of_running_again() {
        let daemon = test_daemon();
        let (dir, worktree) = run_worktree(&daemon);
        std::fs::write(
            dir.path().join(".kete.json"),
            r#"{"run": "echo run-finished"}"#,
        )
        .unwrap();
        let EntityId::Terminal(id) = daemon.start_run(&worktree.id).unwrap() else {
            panic!("a run lives in a terminal");
        };
        let sref = SessionRef::Terminal(id.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        while daemon.finished_run_exit(&sref).is_none() {
            assert!(Instant::now() < deadline, "the run never exited");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!daemon.is_alive(&sref));

        let session = daemon.ensure_session(&sref, 80, 24).unwrap();
        assert!(!daemon.is_alive(&sref), "an attach spawned nothing");
        let (_, bytes) = session.snapshot(None);
        assert!(
            String::from_utf8_lossy(&bytes).contains("run-finished"),
            "the replay is the run's output"
        );

        // `r` again reuses the row.
        assert_eq!(
            daemon.start_run(&worktree.id).unwrap(),
            EntityId::Terminal(id)
        );
        assert_eq!(
            daemon.store.run_terminals_in(&worktree.id).unwrap().len(),
            1
        );
        daemon.stop_run(&worktree.id).unwrap();
    }

    #[tokio::test]
    async fn cloud_create_validates_tasks_and_rejects_non_claude_kinds() {
        let daemon = test_daemon();
        let worktree = WorktreeId("unused".into());

        let empty = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                profile: None,
                worktree: worktree.clone(),
                name: "cloud".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: Some(" \n ".into()),
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap_err();
        assert!(empty.to_string().contains("needs a task"));

        let nul = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                profile: None,
                worktree: worktree.clone(),
                name: "cloud".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: Some("fix\0auth".into()),
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap_err();
        assert!(nul.to_string().contains("NUL"));

        let too_long = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                profile: None,
                worktree: worktree.clone(),
                name: "cloud".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: Some("x".repeat(MAX_CLOUD_PROMPT_BYTES + 1)),
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap_err();
        assert!(too_long.to_string().contains("too long"));

        let wrong_kind = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                profile: None,
                worktree,
                name: "cloud".into(),
                kind: AgentKind::Codex,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: Some("Fix auth".into()),
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap_err();
        assert!(wrong_kind.to_string().contains("only supported for Claude"));
    }

    #[tokio::test]
    async fn pr_launch_context_is_accepted_for_every_kind_but_never_with_cloud() {
        let daemon = test_daemon();
        let spec = |kind: AgentKind, cloud: Option<&str>| CreateAgentSpec {
            parent: None,
            profile: None,
            worktree: WorktreeId("unused".into()),
            name: "pr".into(),
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: cloud.map(String::from),
            starting_prompt: None,
            pr_url: Some("https://github.com/o/r/pull/7".into()),
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        };
        for kind in AgentKind::ALL {
            if kind == AgentKind::Custom {
                // Without a registry id the custom refusal comes first.
                let err = daemon.create_agent(spec(kind, None)).await.unwrap_err();
                assert!(err.to_string().contains("registry id"), "{err}");
                continue;
            }
            // Validation passes for every harness; the missing worktree is
            // what stops this spec, one check later.
            let err = daemon.create_agent(spec(kind, None)).await.unwrap_err();
            assert!(
                err.to_string().contains("worktree not found"),
                "{kind:?}: {err}"
            );
        }
        let cloud = daemon
            .create_agent(spec(AgentKind::Claude, Some("Fix auth")))
            .await
            .unwrap_err();
        assert!(cloud.to_string().contains("not supported for Claude Cloud"));
        let not_a_pr = CreateAgentSpec {
            parent: None,
            pr_url: Some("https://github.com/o/r/issues/7".into()),
            ..spec(AgentKind::Codex, None)
        };
        let err = daemon.create_agent(not_a_pr).await.unwrap_err();
        assert!(err.to_string().contains("not a pull request URL"), "{err}");
    }

    /// A PR SESSION's checkout is made once for the PR's head branch —
    /// fetched from `origin`, under the WORKTREE DIR, never the ROOT
    /// WORKTREE — and every later launch for that PR finds the same row.
    /// Only a PR whose branch the root itself has checked out runs there.
    #[tokio::test]
    async fn pr_worktree_is_created_once_and_shared_by_later_launches() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let origin = root.join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        git_in(&origin, &["init", "--bare", "-b", "main"]);
        git_in(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git_in(&repo, &["push", "-u", "origin", "main"]);
        git_in(&repo, &["branch", "feat-x", "main"]);
        git_in(&repo, &["push", "origin", "feat-x"]);
        git_in(&repo, &["branch", "-D", "feat-x"]);

        let daemon = test_daemon();
        let EntityId::Project(project) = daemon.add_project(&repo, None, false).await.unwrap()
        else {
            panic!("expected a project id");
        };
        let mut events = daemon.events.subscribe();

        let first = daemon.pr_worktree(&project, 7, "feat-x").await.unwrap();
        assert!(!first.is_main, "never the ROOT WORKTREE");
        assert_eq!(first.branch, "feat-x");
        assert_eq!(first.path, git::worktree_dir(&repo, "feat-x"));
        assert!(first.path.join(".git").exists(), "a real checkout");
        assert!(
            matches!(
                events.try_recv(),
                Ok(ServerEvent::EntityUpserted {
                    entity: Entity::Worktree(w)
                }) if w.id == first.id
            ),
            "clients hear about the new row before the agent lands in it"
        );

        let again = daemon.pr_worktree(&project, 7, "feat-x").await.unwrap();
        assert_eq!(
            again.id, first.id,
            "one checkout per PR, shared by every launch"
        );
        assert!(events.try_recv().is_err(), "nothing new to broadcast");

        let on_main = daemon.pr_worktree(&project, 8, "main").await.unwrap();
        assert!(
            on_main.is_main,
            "a PR whose branch the root has checked out is already there"
        );

        // A fork's `main` is not that branch: the client names its checkout
        // for the fork's owner, so the root is no match — the contributor's
        // commit gets a checkout of its own, shared like any other.
        git_in(&repo, &["commit", "--allow-empty", "-m", "fork work"]);
        git_in(&repo, &["push", "origin", "HEAD:refs/pull/129/head"]);
        git_in(&repo, &["reset", "--hard", "origin/main"]);
        let fork = daemon
            .pr_worktree(&project, 129, "someone/main")
            .await
            .unwrap();
        assert!(!fork.is_main, "a fork's main is never the ROOT WORKTREE");
        assert_eq!(fork.branch, "someone/main");
        assert_eq!(fork.path, git::worktree_dir(&repo, "someone/main"));
        let again = daemon
            .pr_worktree(&project, 129, "someone/main")
            .await
            .unwrap();
        assert_eq!(again.id, fork.id, "and later launches share it");

        // A bad head never reaches git — it is refused ahead of the lookup.
        let err = daemon
            .create_pr_agent(crate::pr_scope::CreatePrAgentSpec {
                project: project.clone(),
                name: "pr".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                pr_url: "https://github.com/o/r/pull/7".into(),
                head: "--force".into(),
                starting_prompt: None,
                profile: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a branch name"), "{err}");

        // A preset's composed prompt rides the same create, and is held to
        // `CreateAgent`'s rules once the checkout is found: it reaches the
        // agent create, where the NUL is refused.
        let err = daemon
            .create_pr_agent(crate::pr_scope::CreatePrAgentSpec {
                project: project.clone(),
                name: "pr".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: true,
                pr_url: "https://github.com/o/r/pull/7".into(),
                head: "feat-x".into(),
                starting_prompt: Some("fix\0auth".into()),
                profile: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("NUL"), "{err}");
    }

    /// The ISSUE SESSION's URL is checked at the boundary like the PR
    /// SESSION's: every harness may carry one, a PR URL is not an issue,
    /// and Claude Cloud takes no launch context at all.
    #[tokio::test]
    async fn issue_launch_context_is_accepted_for_every_kind_but_never_with_cloud() {
        let daemon = test_daemon();
        let spec = |kind: AgentKind, cloud: Option<&str>| CreateAgentSpec {
            parent: None,
            profile: None,
            worktree: WorktreeId("unused".into()),
            name: "issue".into(),
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: true,
            cloud_prompt: cloud.map(String::from),
            starting_prompt: Some("Fix it".into()),
            pr_url: None,
            issue_url: Some("https://github.com/o/r/issues/15".into()),
            role: nebula_core::AgentRole::Worker,
        };
        for kind in AgentKind::ALL {
            if kind == AgentKind::Custom {
                let err = daemon.create_agent(spec(kind, None)).await.unwrap_err();
                assert!(err.to_string().contains("registry id"), "{err}");
                continue;
            }
            let err = daemon.create_agent(spec(kind, None)).await.unwrap_err();
            assert!(
                err.to_string().contains("worktree not found"),
                "{kind:?}: {err}"
            );
        }
        let cloud = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                starting_prompt: None,
                ..spec(AgentKind::Claude, Some("Fix auth"))
            })
            .await
            .unwrap_err();
        assert!(cloud.to_string().contains("not supported for Claude Cloud"));
        let not_an_issue = CreateAgentSpec {
            parent: None,
            issue_url: Some("https://github.com/o/r/pull/7".into()),
            role: nebula_core::AgentRole::Worker,
            ..spec(AgentKind::Codex, None)
        };
        let err = daemon.create_agent(not_an_issue).await.unwrap_err();
        assert!(err.to_string().contains("not an issue URL"), "{err}");
    }

    #[tokio::test]
    async fn starting_prompt_is_validated_and_never_adopts_a_warm_cli() {
        let daemon = test_daemon();
        let spec = |kind: AgentKind, cloud: Option<&str>, starting: Option<&str>| CreateAgentSpec {
            parent: None,
            profile: None,
            worktree: WorktreeId("unused".into()),
            name: "preset".into(),
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: true,
            cloud_prompt: cloud.map(String::from),
            starting_prompt: starting.map(String::from),
            pr_url: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
        };
        // Validation runs before the worktree lookup, so an unknown
        // worktree is fine here and every failure is the prompt's own.
        for (kind, starting, needle) in [
            (AgentKind::Claude, " \n ", "is empty"),
            (AgentKind::Codex, "fix\0auth", "NUL"),
            (
                AgentKind::Cursor,
                &*"x".repeat(MAX_CLOUD_PROMPT_BYTES + 1),
                "too long",
            ),
        ] {
            let err = daemon
                .create_agent(spec(kind, None, Some(starting)))
                .await
                .unwrap_err();
            assert!(err.to_string().contains(needle), "{kind:?}: {err}");
        }
        let with_cloud = daemon
            .create_agent(spec(AgentKind::Claude, Some("Fix auth"), Some("Fix auth")))
            .await
            .unwrap_err();
        assert!(with_cloud
            .to_string()
            .contains("not supported for Claude Cloud"));

        // A starting prompt rides the CLI's argv, so a warm spare (booted
        // bare) is never adopted: the pool entry survives the create.
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w1", "/tmp", true);
        let key = (WorktreeId("w1".into()), AgentKind::Claude);
        daemon.prewarmed.lock().insert(
            key.clone(),
            PrewarmEntry {
                profile: None,
                agent_id: AgentId("warm-1".into()),
                spawned_at: Instant::now(),
                model: None,
                effort: None,
                buffered_hooks: Vec::new(),
            },
        );
        let mut preset = spec(AgentKind::Claude, None, Some("Fix auth"));
        preset.worktree = WorktreeId("w1".into());
        // Without a CLI on this box the cold path fails at the probe; the
        // result is beside the point — adoption would have emptied the pool.
        let _ = daemon.create_agent(preset).await;
        assert!(
            daemon.prewarmed.lock().contains_key(&key),
            "a starting-prompt create must not adopt the warm spare"
        );
    }

    /// Which launches boot straight into a turn — the question the
    /// optimistic `running` hangs on. Claude and Pi take a launch rule on
    /// their system-prompt flag, so only a task makes them work at once;
    /// Codex, Cursor and Muse have no such flag, so the rule itself opens
    /// their first prompt.
    #[test]
    fn a_launch_submits_a_first_prompt_when_it_carries_a_task_or_an_unflagged_rule() {
        for kind in AgentKind::ALL {
            if kind == AgentKind::Custom {
                continue; // no descriptor without a registry entry
            }
            let harness = resolve_harness(kind, None).unwrap();
            let flagged = harness.system.append_flag.is_some();
            assert!(
                Daemon::launch_submits_first_prompt(&harness, Some("Fix auth"), false),
                "{kind:?}: a task is always the first prompt"
            );
            assert!(
                Daemon::launch_submits_first_prompt(&harness, Some("Fix auth"), true),
                "{kind:?}: a task beside a rule too"
            );
            assert_eq!(
                Daemon::launch_submits_first_prompt(&harness, None, true),
                !flagged,
                "{kind:?}: a bare rule opens the first prompt only without the flag"
            );
            assert!(
                !Daemon::launch_submits_first_prompt(&harness, None, false),
                "{kind:?}: a bare launch parks at the CLI's own input"
            );
        }
    }

    /// The row the clients are told about is already `running` when the
    /// launch carries a task: the CLI submits it as it boots, and the
    /// session must not sit gray — and sort under everything mid-turn —
    /// for the seconds until its first hook lands.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_create_with_a_task_broadcasts_a_running_row() {
        let daemon = test_daemon();
        let (dir, worktree) = run_worktree(&daemon);
        // `/bin/cat` stands in for the CLI: spawned verbatim, it blocks on
        // the PTY instead of running anything.
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let spec = |name: &str, task: Option<&str>| CreateAgentSpec {
            parent: None,
            worktree: worktree.id.clone(),
            name: name.into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: task.map(String::from),
            pr_url: None,
            issue_url: None,
            profile: None,
            role: nebula_core::AgentRole::Worker,
        };

        let created = |mut events: broadcast::Receiver<ServerEvent>| {
            std::iter::from_fn(|| events.try_recv().ok())
                .find_map(|e| match e {
                    ServerEvent::EntityUpserted {
                        entity: Entity::Agent(a),
                    } => Some(a),
                    _ => None,
                })
                .expect("the create broadcasts its row")
        };

        let events = daemon.events.subscribe();
        let EntityId::Agent(with_task) = daemon
            .create_agent(spec("task", Some("Fix auth")))
            .await
            .unwrap()
        else {
            panic!("a create makes an agent");
        };
        assert_eq!(created(events).status, AgentStatus::Running);
        assert_eq!(
            daemon.store.get_agent(&with_task).unwrap().unwrap().status,
            AgentStatus::Running,
            "and that is what a restart reads back"
        );
        // Seeded with its reprieve, so the CLI's startup progress-clear
        // cannot green it out before the turn begins.
        daemon.apply_hook_event(&with_task, HookEvent::Progress { busy: false }, None);
        assert_eq!(
            daemon.store.get_agent(&with_task).unwrap().unwrap().status,
            AgentStatus::Running
        );

        // A launch with nothing to do still parks at the CLI's input box.
        let events = daemon.events.subscribe();
        daemon.create_agent(spec("bare", None)).await.unwrap();
        assert_eq!(created(events).status, AgentStatus::Fresh);
        drop(dir);
    }

    /// `kete spawn` from inside a session: the new row records the caller
    /// as its parent, persisted and on the row every client is sent.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_spawn_records_the_calling_session_as_parent() {
        let daemon = test_daemon();
        let (dir, worktree) = run_worktree(&daemon);
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let caller = AgentId("caller".into());
        daemon
            .store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: caller.clone(),
                worktree_id: worktree.id.clone(),
                name: "caller".into(),
                status: AgentStatus::Running,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: false,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            })
            .unwrap();

        let events = daemon.events.subscribe();
        let EntityId::Agent(child) = daemon
            .spawn_sibling_agent(&caller, None, None, "Write the tests", None)
            .await
            .unwrap()
        else {
            panic!("a spawn makes an agent");
        };
        let mut events = events;
        let sent = std::iter::from_fn(|| events.try_recv().ok())
            .find_map(|e| match e {
                ServerEvent::EntityUpserted {
                    entity: Entity::Agent(a),
                } if a.id == child => Some(a),
                _ => None,
            })
            .expect("the spawn broadcasts its row");
        assert_eq!(sent.parent, Some(caller.clone()));
        assert_eq!(
            daemon.store.get_agent(&child).unwrap().unwrap().parent,
            Some(caller)
        );
        drop(dir);
    }

    /// Save/restore around a process-wide env var a test has to set. Holds
    /// a lock for its life: tests run on parallel threads, and one guard's
    /// drop restoring "unset" mid-way through another's run made that run
    /// spawn the real `claude` (absent on CI, so the create failed).
    pub(crate) struct EnvGuard {
        was: Vec<(&'static str, Option<String>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        pub(crate) fn set(key: &'static str, value: &str) -> Self {
            Self::set_all(&[(key, value)])
        }

        /// Several variables under the one lock: two guards in one test
        /// would wait on each other.
        pub(crate) fn set_all(vars: &[(&'static str, &str)]) -> Self {
            static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            // A test that panicked holding the lock left the env restored
            // (drop still ran), so its poison means nothing here.
            let lock = LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was = vars
                .iter()
                .map(|(key, value)| {
                    let was = std::env::var(key).ok();
                    std::env::set_var(key, value);
                    (*key, was)
                })
                .collect();
            Self { was, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, was) in self.was.drain(..).rev() {
                match was {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    fn failed_agent_spawn_rolls_back_the_persisted_row() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "w", "/tmp", true);
        seed_agent(&daemon, "cloud", "w", None);
        let id = AgentId("cloud".into());

        let error = daemon
            .rollback_agent_on_spawn_error(&id, Err::<(), _>(anyhow::anyhow!("spawn failed")))
            .unwrap_err();

        assert!(error.to_string().contains("spawn failed"));
        assert!(daemon.store.get_agent(&id).unwrap().is_none());
    }

    fn seed_projects(daemon: &Daemon, names: &[&str]) {
        for (i, name) in names.iter().enumerate() {
            daemon
                .store
                .insert_project(&Project {
                    id: ProjectId((*name).into()),
                    name: (*name).into(),
                    repo_path: format!("/tmp/{name}").into(),
                    sort_order: i as i64,
                })
                .unwrap();
        }
    }

    fn seed_worktree(daemon: &Daemon, project: &str, id: &str, path: &str, is_main: bool) {
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId(id.into()),
                project_id: ProjectId(project.into()),
                path: path.into(),
                branch: id.into(),
                is_main,
                sort_order: 0,
            })
            .unwrap();
    }

    fn seed_agent(daemon: &Daemon, id: &str, worktree: &str, session_id: Option<&str>) {
        daemon
            .store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: AgentId(id.into()),
                worktree_id: WorktreeId(worktree.into()),
                name: id.into(),
                status: AgentStatus::Running,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: session_id.map(str::to_string),
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: false,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            })
            .unwrap();
    }

    fn agent_worktree(daemon: &Daemon, id: &str) -> String {
        daemon
            .store
            .get_agent(&AgentId(id.into()))
            .unwrap()
            .unwrap()
            .worktree_id
            .to_string()
    }

    #[tokio::test]
    async fn enter_worktree_takes_an_existing_branch_and_moves_the_row_now() {
        let daemon = test_daemon();
        // Real directories: entering refuses a checkout that is not on disk.
        let dir = tempfile::tempdir().unwrap();
        let (root, feat) = (dir.path().join("p"), dir.path().join("p-feat"));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&feat).unwrap();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", root.to_str().unwrap(), true);
        seed_worktree(&daemon, "p", "feat", feat.to_str().unwrap(), false);
        seed_agent(&daemon, "a1", "root", Some("s1"));
        let a1 = AgentId("a1".into());
        let mut rx = daemon.events.subscribe();

        let (target, outcome) = daemon.enter_worktree(&a1, "feat", None).await.unwrap();
        assert_eq!(target.id.to_string(), "feat");
        // No PTY runs here, so nothing waits on a turn end.
        assert_eq!(outcome, EnterOutcome::NextLaunch);
        assert!(!daemon.relocation_pending(&a1));
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");
        match rx.try_recv().unwrap() {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } => assert_eq!(a.worktree_id.to_string(), "feat"),
            other => panic!("expected agent upsert, got {other:?}"),
        }

        // Already there: a settled answer, no broadcast.
        let (again, outcome) = daemon.enter_worktree(&a1, "feat", None).await.unwrap();
        assert_eq!(again.id, target.id);
        assert_eq!(outcome, EnterOutcome::AlreadyThere);
        assert!(rx.try_recv().is_err(), "no broadcast for a no-op enter");

        // Blank names are refused before anything is touched.
        assert!(daemon.enter_worktree(&a1, "  ", None).await.is_err());
        // A start point for a branch that already has a checkout is ignored
        // for `nebula worktree`, matching the long-standing CLI fallback.
        let (again, outcome) = daemon
            .enter_worktree(&a1, "feat", Some("main"))
            .await
            .unwrap();
        assert_eq!(again.id, target.id);
        assert_eq!(outcome, EnterOutcome::AlreadyThere);
    }

    /// Between `nebula worktree` and the turn's Stop the row already sits
    /// under the target while the process still reports the old checkout:
    /// that cwd must not drag it back, and only a turn-end hook drains the
    /// pending relocation.
    #[test]
    fn pending_relocation_ignores_the_old_cwd_until_the_turn_ends() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_agent(&daemon, "a1", "feat", Some("s1"));
        let a1 = AgentId("a1".into());
        let feat = daemon
            .store
            .get_worktree(&WorktreeId("feat".into()))
            .unwrap()
            .unwrap();
        daemon.pending_moves.lock().insert(
            a1.clone(),
            PendingMove {
                target: feat,
                notice: true,
            },
        );

        daemon.reparent_agent_by_cwd(&a1, "/nebula-test/p", Some("s1"), false);
        assert_eq!(
            agent_worktree(&daemon, "a1"),
            "feat",
            "the old checkout's cwd is ignored mid-relocation"
        );

        daemon.complete_pending_move(
            &a1,
            &HookEvent::PostToolUse {
                tool_name: Some("Bash".into()),
                subagent_id: None,
            },
        );
        assert!(
            daemon.relocation_pending(&a1),
            "a tool hook is not a turn end"
        );
        daemon.complete_pending_move(&a1, &HookEvent::Stop { background: None });
        assert!(!daemon.relocation_pending(&a1));

        // Drained, the reparent is live again.
        daemon.reparent_agent_by_cwd(&a1, "/nebula-test/p", Some("s1"), false);
        assert_eq!(agent_worktree(&daemon, "a1"), "root");
    }

    /// `nebula worktree` from a live session: the turn end that triggers
    /// the relocation must not finish the row for the seconds until the
    /// respawned CLI's first hook — the card would drop to the bottom of
    /// the grid and climb back — so the Stop is held at `running`, the
    /// respawn is seeded as a launch (its startup progress-clear swallowed),
    /// and only the relocated turn's own end finishes it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_relocation_keeps_the_row_running_through_the_respawn() {
        let daemon = test_daemon();
        let (dir, main) = run_worktree(&daemon);
        let feat_dir = tempfile::tempdir().unwrap();
        let feat = Worktree {
            id: WorktreeId::generate(),
            project_id: main.project_id.clone(),
            path: feat_dir.path().to_path_buf(),
            branch: "feat".into(),
            is_main: false,
            sort_order: 1,
        };
        daemon.store.insert_worktree(&feat).unwrap();
        // `/bin/cat` stands in for the CLI, on the first boot and the
        // respawn alike (an override takes no argv, notice included).
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let EntityId::Agent(id) = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                worktree: main.id.clone(),
                name: "a".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                profile: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap()
        else {
            panic!("a create makes an agent");
        };
        let sref = SessionRef::Agent(id.clone());
        let status = |id: &AgentId| daemon.store.get_agent(id).unwrap().unwrap().status;
        daemon.apply_hook_event(&id, HookEvent::UserPromptSubmit, Some("s1".into()));
        assert_eq!(status(&id), AgentStatus::Running);

        // The turn runs `nebula worktree feat`: the row moves now, the PTY
        // waits for the turn's end.
        let (_, outcome) = daemon.enter_worktree(&id, "feat", None).await.unwrap();
        assert_eq!(outcome, EnterOutcome::Relocating);
        let first = daemon.session(&sref).expect("still the first PTY");
        let mut rx = daemon.events.subscribe();

        // The turn's Stop lands as the hook loop delivers it: through the
        // status machine first, then the relocation.
        daemon.apply_hook_event(&id, HookEvent::Stop { background: None }, Some("s1".into()));
        assert_eq!(
            status(&id),
            AgentStatus::Running,
            "the Stop is held for the relocation"
        );
        daemon.complete_pending_move(&id, &HookEvent::Stop { background: None });
        assert!(!daemon.relocation_pending(&id));
        let second = daemon.session(&sref).expect("respawned in the worktree");
        assert!(!Arc::ptr_eq(&first, &second), "a new PTY");
        assert_eq!(status(&id), AgentStatus::Running);
        // The respawned CLI's startup progress-clear is its boot, not a
        // turn ending.
        daemon.apply_hook_event(&id, HookEvent::Progress { busy: false }, None);
        assert_eq!(status(&id), AgentStatus::Running);
        while let Ok(ev) = rx.try_recv() {
            assert!(
                !matches!(
                    ev,
                    ServerEvent::StatusChanged {
                        status: AgentStatus::Finished,
                        ..
                    }
                ),
                "nothing in between said finished: {ev:?}"
            );
        }
        // The relocated turn's own end does.
        daemon.apply_hook_event(&id, HookEvent::Stop { background: None }, Some("s1".into()));
        assert_eq!(status(&id), AgentStatus::Finished);
        drop(dir);
    }

    /// The same relocation when the respawn carries no notice (cursor
    /// resumes silent and waits for the user) or nothing is left to
    /// respawn: the held turn end finishes the row after all, since no
    /// prompt is carrying the work on.
    #[test]
    fn a_relocation_with_no_respawn_lets_the_held_turn_end_finish() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_agent(&daemon, "a1", "feat", Some("s1"));
        let a1 = AgentId("a1".into());
        let feat = daemon
            .store
            .get_worktree(&WorktreeId("feat".into()))
            .unwrap()
            .unwrap();
        daemon.pending_moves.lock().insert(
            a1.clone(),
            PendingMove {
                target: feat,
                notice: true,
            },
        );
        let status = |id: &AgentId| daemon.store.get_agent(id).unwrap().unwrap().status;

        daemon.apply_hook_event(&a1, HookEvent::Stop { background: None }, Some("s1".into()));
        assert_eq!(status(&a1), AgentStatus::Running, "held while pending");
        // No PTY to relocate: the hold is released and the Stop finishes it.
        daemon.complete_pending_move(&a1, &HookEvent::Stop { background: None });
        assert!(!daemon.relocation_pending(&a1));
        assert_eq!(status(&a1), AgentStatus::Finished);
    }

    /// A dead session's move is the row alone, into its own project's
    /// checkouts or another project's; the one it is already in is a no-op.
    #[test]
    fn move_agent_rehomes_a_dead_row_across_checkouts_and_projects() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p", "q"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_worktree(&daemon, "q", "other", "/nebula-test/q", true);
        seed_agent(&daemon, "a1", "root", Some("s1"));
        let a1 = AgentId("a1".into());

        daemon.move_agent(&a1, &WorktreeId("feat".into())).unwrap();
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");
        assert!(!daemon.relocation_pending(&a1), "nothing runs to follow");

        daemon.move_agent(&a1, &WorktreeId("feat".into())).unwrap();
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");

        daemon.move_agent(&a1, &WorktreeId("other".into())).unwrap();
        assert_eq!(agent_worktree(&daemon, "a1"), "other");
    }

    /// A reorder reaches every subscriber as one upsert per row whose slot
    /// changed, carrying the new slot: nothing for rows that stayed, and
    /// nothing at all for a refused order.
    #[test]
    fn reorder_broadcasts_an_upsert_for_each_row_that_moved() {
        use nebula_core::ReorderScope;
        let upserts = |rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>| {
            let mut out = Vec::new();
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    ServerEvent::EntityUpserted { entity } => out.push(match entity {
                        Entity::Project(p) => (p.id.0, p.sort_order),
                        Entity::Worktree(w) => (w.id.0, w.sort_order),
                        Entity::Agent(a) => (a.id.0, a.sort_order),
                        other => panic!("unexpected upsert {other:?}"),
                    }),
                    other => panic!("expected upserts only, got {other:?}"),
                }
            }
            out
        };
        let daemon = test_daemon();
        seed_projects(&daemon, &["p", "q"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_agent(&daemon, "a1", "root", None);
        seed_agent(&daemon, "a2", "root", None);
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut rx = daemon.events.subscribe();

        daemon
            .reorder(&ReorderScope::Projects, &ids(&["q", "p"]))
            .unwrap();
        let mut got = upserts(&mut rx);
        got.sort();
        assert_eq!(got, vec![("p".into(), 1), ("q".into(), 0)]);

        daemon
            .reorder(
                &ReorderScope::Agents(WorktreeId("root".into())),
                &ids(&["a2", "a1"]),
            )
            .unwrap();
        // Both seeded at 0: a2 keeps its slot, only a1 moves.
        assert_eq!(upserts(&mut rx), vec![("a1".into(), 1)]);

        // root (main) and feat already sit main-first at 0, 0: feat moves
        // to 1 and is the only upsert.
        let worktrees = ReorderScope::Worktrees(ProjectId("p".into()));
        daemon.reorder(&worktrees, &ids(&["feat"])).unwrap();
        assert_eq!(upserts(&mut rx), vec![("feat".into(), 1)]);

        assert!(daemon.reorder(&worktrees, &ids(&["feat", "root"])).is_err());
        assert!(upserts(&mut rx).is_empty(), "a refused order says nothing");
    }

    /// A live session: idle, it is respawned in the target at once; mid
    /// turn, it waits for the turn end and then resumes silent, so the
    /// held Stop finishes the row instead of a notice carrying it on.
    #[cfg(unix)]
    #[tokio::test]
    async fn move_agent_relocates_an_idle_session_now_and_a_busy_one_at_turn_end() {
        let daemon = test_daemon();
        let (dir, main) = run_worktree(&daemon);
        let feat_dir = tempfile::tempdir().unwrap();
        let feat = Worktree {
            id: WorktreeId::generate(),
            project_id: main.project_id.clone(),
            path: feat_dir.path().to_path_buf(),
            branch: "feat".into(),
            is_main: false,
            sort_order: 1,
        };
        daemon.store.insert_worktree(&feat).unwrap();
        let _cmd = EnvGuard::set(env::AGENT_CMD, "/bin/cat");
        let EntityId::Agent(id) = daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                worktree: main.id.clone(),
                name: "a".into(),
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: None,
                pr_url: None,
                issue_url: None,
                profile: None,
                role: nebula_core::AgentRole::Worker,
            })
            .await
            .unwrap()
        else {
            panic!("a create makes an agent");
        };
        let sref = SessionRef::Agent(id.clone());
        let status = |id: &AgentId| daemon.store.get_agent(id).unwrap().unwrap().status;

        // Mid-turn: the row moves, the PTY waits for the turn's end.
        daemon.apply_hook_event(&id, HookEvent::UserPromptSubmit, Some("s1".into()));
        daemon.move_agent(&id, &feat.id).unwrap();
        assert_eq!(
            agent_worktree(&daemon, &id.to_string()),
            feat.id.to_string()
        );
        assert!(daemon.relocation_pending(&id));
        let first = daemon.session(&sref).expect("still the first PTY");
        daemon.apply_hook_event(&id, HookEvent::Stop { background: None }, Some("s1".into()));
        daemon.complete_pending_move(&id, &HookEvent::Stop { background: None });
        assert!(!daemon.relocation_pending(&id));
        let second = daemon.session(&sref).expect("respawned in the target");
        assert!(!Arc::ptr_eq(&first, &second), "a new PTY");
        assert_eq!(
            status(&id),
            AgentStatus::Finished,
            "no notice carries the turn on"
        );

        // Idle: back to the root checkout straight away.
        daemon.move_agent(&id, &main.id).unwrap();
        assert!(!daemon.relocation_pending(&id));
        assert_eq!(
            agent_worktree(&daemon, &id.to_string()),
            main.id.to_string()
        );
        let third = daemon.session(&sref).expect("respawned in the root");
        assert!(!Arc::ptr_eq(&second, &third), "a new PTY");
        drop(dir);
    }

    #[tokio::test]
    async fn enter_worktree_creates_the_checkout_in_nebulas_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let daemon = test_daemon();
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "p".into(),
                repo_path: repo.clone(),
                sort_order: 0,
            })
            .unwrap();
        seed_worktree(&daemon, "p", "root", &repo.to_string_lossy(), true);
        seed_agent(&daemon, "a1", "root", Some("s1"));
        let a1 = AgentId("a1".into());
        let mut rx = daemon.events.subscribe();

        let (target, _) = daemon.enter_worktree(&a1, "feat", None).await.unwrap();
        assert_eq!(target.branch, "feat");
        assert_eq!(target.path, root.join("repo-worktrees").join("feat"));
        assert!(target.path.join(".git").exists(), "a real checkout");
        assert_eq!(agent_worktree(&daemon, "a1"), target.id.to_string());
        // The worktree's upsert lands first, then the agent's.
        assert!(matches!(
            rx.try_recv().unwrap(),
            ServerEvent::EntityUpserted { entity: Entity::Worktree(w) } if w.id == target.id
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            ServerEvent::EntityUpserted { entity: Entity::Agent(a) } if a.worktree_id == target.id
        ));
    }

    fn seed_pending_agent(daemon: &Daemon, id: &str, worktree: &str) {
        daemon
            .store
            .insert_agent_with_auto_title(
                &Agent {
                    parent: None,
                    profile: None,
                    id: AgentId(id.into()),
                    worktree_id: WorktreeId(worktree.into()),
                    name: format!("{id}-default"),
                    status: AgentStatus::Fresh,
                    archived: false,
                    archived_at: 0,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: 0,
                    alive: false,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                },
                true,
            )
            .unwrap();
    }

    #[test]
    fn auto_rename_applies_once_and_defers_to_user_titles() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_pending_agent(&daemon, "a1", "root");
        let mut rx = daemon.events.subscribe();

        // First agent attempt lands, sanitized, and is broadcast.
        daemon
            .auto_rename_agent(&AgentId("a1".into()), "  Fix   Login\tRedirect  ")
            .unwrap();
        match rx.try_recv().unwrap() {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } => assert_eq!(a.name, "Fix Login Redirect"),
            other => panic!("expected agent upsert, got {other:?}"),
        }

        // A second attempt is declined with a settled, informative error.
        let err = daemon
            .auto_rename_agent(&AgentId("a1".into()), "Another Title")
            .unwrap_err();
        assert!(err.to_string().contains("already has a title"), "{err}");
        assert_eq!(
            daemon
                .store
                .get_agent(&AgentId("a1".into()))
                .unwrap()
                .unwrap()
                .name,
            "Fix Login Redirect"
        );

        // A user rename beats a pending auto-title: the CLI's later attempt
        // must not clobber it.
        seed_pending_agent(&daemon, "a2", "root");
        daemon
            .rename_agent(&AgentId("a2".into()), "my session")
            .unwrap();
        let err = daemon
            .auto_rename_agent(&AgentId("a2".into()), "Model Title")
            .unwrap_err();
        assert!(err.to_string().contains("already has a title"), "{err}");

        // Garbage titles are rejected outright.
        assert!(daemon
            .auto_rename_agent(&AgentId("a1".into()), " \u{7}\n ")
            .is_err());
        // Unknown agents report cleanly.
        let err = daemon
            .auto_rename_agent(&AgentId("ghost".into()), "Some Title")
            .unwrap_err();
        assert!(err.to_string().contains("agent not found"), "{err}");
    }

    #[test]
    fn sanitize_title_collapses_and_caps() {
        assert_eq!(
            sanitize_title(" Fix   Login\u{7}Redirect \n"),
            "Fix Login Redirect"
        );
        assert_eq!(sanitize_title("\u{1b}[31m"), "[31m");
        assert_eq!(sanitize_title("   "), "");
        let long = "word ".repeat(30);
        assert!(sanitize_title(&long).chars().count() <= 60);
        assert!(!sanitize_title(&long).ends_with(' '));
    }

    #[test]
    fn reparent_by_cwd_picks_deepest_matching_worktree() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        // Nested layout: the linked checkout lives under the repo root, so
        // both paths are prefixes of a cwd inside it — deepest must win.
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p/.wt/feat", false);
        seed_agent(&daemon, "a1", "root", None);

        // cwd inside the root checkout (but outside the nested worktree)
        // keeps the agent where it is.
        daemon.reparent_agent_by_cwd(&AgentId("a1".into()), "/nebula-test/p/src", None, false);
        assert_eq!(agent_worktree(&daemon, "a1"), "root");

        // cwd inside the nested worktree re-homes it there.
        daemon.reparent_agent_by_cwd(
            &AgentId("a1".into()),
            "/nebula-test/p/.wt/feat/src",
            None,
            false,
        );
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");

        // cwd outside every worktree is ignored.
        daemon.reparent_agent_by_cwd(&AgentId("a1".into()), "/elsewhere", None, false);
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");
    }

    /// Regression: a session that creates a worktree and steps into it
    /// reports the new cwd *before* the sync has adopted a row for it (the
    /// `Stop` hook fires long before the next 2s sync tick). The cwd must be
    /// remembered and replayed on adoption, or the row sits under the old
    /// checkout until the user's next prompt.
    #[tokio::test]
    async fn worktree_sync_replays_a_cwd_reported_before_adoption() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);

        let daemon = test_daemon();
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: repo.clone(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        seed_worktree(&daemon, "p", "root", &repo.to_string_lossy(), true);
        seed_agent(&daemon, "a1", "root", Some("s1"));

        // The agent creates a sibling worktree and walks into it. The hook
        // lands first: no row exists yet, so nothing moves.
        let feat = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &feat.to_string_lossy(), "-b", "feat"],
        );
        daemon.reparent_agent_by_cwd(
            &AgentId("a1".into()),
            &feat.to_string_lossy(),
            Some("s1"),
            false,
        );
        assert_eq!(agent_worktree(&daemon, "a1"), "root");

        // The sync adopts the checkout and replays the remembered cwd.
        daemon.sync_project_worktrees(&project).await.unwrap();
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let adopted = worktrees
            .iter()
            .find(|w| w.branch == "feat")
            .expect("feat worktree adopted");
        assert_eq!(agent_worktree(&daemon, "a1"), adopted.id.to_string());
    }

    /// The replay is scoped to the synced project and skips archived rows.
    #[test]
    fn cwd_replay_skips_other_projects_and_archived_agents() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p", "q"]);
        seed_worktree(&daemon, "p", "p-root", "/nebula-test/p", true);
        seed_worktree(&daemon, "q", "q-root", "/nebula-test/q", true);
        seed_worktree(&daemon, "q", "q-feat", "/nebula-test/q-feat", false);
        seed_agent(&daemon, "a1", "q-root", None);
        seed_agent(&daemon, "a2", "q-root", None);

        // Both agents report a cwd inside q-feat before it exists...
        daemon
            .store
            .delete_worktree(&WorktreeId("q-feat".into()))
            .unwrap();
        daemon.reparent_agent_by_cwd(&AgentId("a1".into()), "/nebula-test/q-feat", None, false);
        daemon.reparent_agent_by_cwd(&AgentId("a2".into()), "/nebula-test/q-feat", None, false);
        seed_worktree(&daemon, "q", "q-feat", "/nebula-test/q-feat", false);

        // ...but a replay for project p touches neither.
        let p = daemon
            .store
            .get_project(&ProjectId("p".into()))
            .unwrap()
            .unwrap();
        daemon.reparent_agents_by_last_cwd(&p);
        assert_eq!(agent_worktree(&daemon, "a1"), "q-root");

        // Archived agents stay put; live ones re-home.
        daemon
            .store
            .set_agent_archived(&AgentId("a2".into()), true)
            .unwrap();
        let q = daemon
            .store
            .get_project(&ProjectId("q".into()))
            .unwrap()
            .unwrap();
        daemon.reparent_agents_by_last_cwd(&q);
        assert_eq!(agent_worktree(&daemon, "a1"), "q-feat");
        assert_eq!(agent_worktree(&daemon, "a2"), "q-root");
    }

    /// Renaming a project relabels its row and nothing else: the checkout on
    /// disk keeps its name, `repo_path` keeps pointing at it, and an empty
    /// name puts the row back on the folder's own name — the only way back
    /// once a project has been renamed.
    #[tokio::test]
    async fn rename_project_relabels_the_row_and_leaves_the_folder_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("acme-api");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);

        let daemon = test_daemon();
        let id = match daemon.add_project(&repo, None, false).await.unwrap() {
            EntityId::Project(id) => id,
            other => panic!("expected a project id, got {other:?}"),
        };
        let named = |daemon: &Arc<Daemon>| daemon.store.get_project(&id).unwrap().unwrap();
        assert_eq!(named(&daemon).name, "acme-api", "named after the folder");

        daemon.rename_project(&id, "  Acme API  ").unwrap();
        let project = named(&daemon);
        assert_eq!(project.name, "Acme API", "trimmed and stored");
        assert_eq!(project.repo_path, repo, "the folder is untouched");
        assert!(repo.exists(), "and still on disk under its own name");
        assert_eq!(
            project.folder_subtitle().as_deref(),
            Some("acme-api"),
            "a renamed row still shows where it lives"
        );

        daemon.rename_project(&id, "   ").unwrap();
        let project = named(&daemon);
        assert_eq!(project.name, "acme-api", "empty resets to the folder name");
        assert_eq!(project.folder_subtitle(), None, "nothing left to show");
    }

    /// A repo with a sibling checkout (`acme-worktrees/feat`, where nebula
    /// puts new worktrees) and one nested inside it (`acme/.wt/nested`),
    /// added as a project. Returns the temp dir (held for the test's life),
    /// the root dir in it, the repo and the project id.
    async fn project_with_checkouts(
        daemon: &Arc<Daemon>,
    ) -> (tempfile::TempDir, PathBuf, PathBuf, ProjectId) {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("acme");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let feat = root.join("acme-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &feat.to_string_lossy(), "-b", "feat"],
        );
        git_in(&repo, &["worktree", "add", ".wt/nested", "-b", "nested"]);
        let id = match daemon.add_project(&repo, None, false).await.unwrap() {
            EntityId::Project(id) => id,
            other => panic!("expected a project id, got {other:?}"),
        };
        (tmp, root, repo, id)
    }

    /// Renaming a project's folder on disk used to strand it: the row kept
    /// the old path, and adding the new one made a second project. Pointing
    /// the project at the new folder moves the rows inside it, leaves the
    /// sibling checkout where it is, repairs git's links both ways, and
    /// gives the row the new folder's name.
    #[tokio::test]
    async fn set_project_path_follows_a_renamed_folder() {
        let daemon = test_daemon();
        let (_tmp, root, repo, id) = project_with_checkouts(&daemon).await;
        let moved = root.join("acme-web");
        std::fs::rename(&repo, &moved).unwrap();

        daemon.set_project_path(&id, &moved).await.unwrap();

        let (projects, worktrees, _, _) = daemon.store.load_tree().unwrap();
        assert_eq!(projects.len(), 1, "still one project: {projects:#?}");
        assert_eq!(projects[0].repo_path, moved);
        assert_eq!(projects[0].name, "acme-web", "named after the new folder");
        let path_of = |branch: &str| {
            let row = worktrees.iter().find(|w| w.branch == branch);
            row.unwrap_or_else(|| panic!("no {branch} row: {worktrees:#?}"))
                .path
                .clone()
        };
        assert_eq!(path_of("main"), moved, "the ⌂ root row moved");
        assert_eq!(
            path_of("nested"),
            moved.join(".wt/nested"),
            "so did the nested one"
        );
        let feat = root.join("acme-worktrees").join("feat");
        assert_eq!(path_of("feat"), feat, "the sibling checkout stayed put");
        assert_eq!(worktrees.len(), 3, "no row added or lost: {worktrees:#?}");
        // git's links: each checkout finds the repo again, and the repo
        // lists each checkout where it now is.
        git_in(&feat, &["status"]);
        git_in(&moved.join(".wt/nested"), &["status"]);
        let mut listed: Vec<PathBuf> = git::list_worktrees(&moved)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        listed.sort();
        let mut want = [moved.clone(), feat, moved.join(".wt/nested")];
        want.sort();
        assert_eq!(listed, want);
    }

    /// A row the user retitled keeps its title when its folder moves; only
    /// a row still named after the old folder follows the new one.
    #[tokio::test]
    async fn set_project_path_keeps_a_chosen_name() {
        let daemon = test_daemon();
        let (_tmp, root, repo, id) = project_with_checkouts(&daemon).await;
        daemon.rename_project(&id, "Acme").unwrap();
        let moved = root.join("acme-web");
        std::fs::rename(&repo, &moved).unwrap();

        daemon.set_project_path(&id, &moved).await.unwrap();

        let project = daemon.store.get_project(&id).unwrap().unwrap();
        assert_eq!(project.name, "Acme");
        assert_eq!(project.folder_subtitle().as_deref(), Some("acme-web"));
    }

    /// The new path has to be the repo's main checkout, and nobody else's
    /// project; a refused move changes nothing.
    #[tokio::test]
    async fn set_project_path_refuses_what_is_not_the_moved_repo() {
        let daemon = test_daemon();
        let (_tmp, root, repo, id) = project_with_checkouts(&daemon).await;
        let other = root.join("other");
        std::fs::create_dir(&other).unwrap();
        git_in(&other, &["init", "-b", "main"]);
        git_in(&other, &["commit", "--allow-empty", "-m", "init"]);
        daemon.add_project(&other, None, false).await.unwrap();
        let plain = root.join("plain");
        std::fs::create_dir(&plain).unwrap();
        let before = daemon.store.load_tree().unwrap();

        for (path, why) in [
            (plain, "not a git repository"),
            (root.join("acme-worktrees").join("feat"), "is a worktree of"),
            (other, "is already the project \"other\""),
        ] {
            let err = daemon.set_project_path(&id, &path).await.unwrap_err();
            assert!(format!("{err:#}").contains(why), "{path:?}: {err:#}");
        }

        let after = daemon.store.load_tree().unwrap();
        assert_eq!(
            format!("{:?}", after.0),
            format!("{:?}", before.0),
            "projects untouched"
        );
        assert_eq!(
            format!("{:?}", after.1),
            format!("{:?}", before.1),
            "worktrees untouched"
        );
        assert_eq!(
            daemon.store.get_project(&id).unwrap().unwrap().repo_path,
            repo
        );
    }

    /// `git rev-parse --show-toplevel` answers with the checkout it ran in, so
    /// `nebula add .` from inside a linked worktree used to make the worktree
    /// the project: named after the branch directory, `repo_path` pointing at
    /// it, and a ⌂ root row for a directory the project did not own. The repo
    /// is the project no matter which of its checkouts you add it from.
    #[tokio::test]
    async fn add_project_from_inside_a_worktree_roots_at_the_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let feat = root.join("repo-worktrees").join("gentle-narwhal-files");
        git_in(
            &repo,
            &[
                "worktree",
                "add",
                &feat.to_string_lossy(),
                "-b",
                "gentle-narwhal-files",
            ],
        );

        let daemon = test_daemon();
        daemon.add_project(&feat, None, false).await.unwrap();

        let (projects, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let project = projects.first().expect("project added");
        assert_eq!(project.repo_path, repo, "project is rooted at the repo");
        assert_eq!(project.name, "repo", "named after the repo, not the branch");

        let main: Vec<&Worktree> = worktrees.iter().filter(|w| w.is_main).collect();
        assert_eq!(main.len(), 1, "exactly one root row: {worktrees:#?}");
        assert_eq!(
            main[0].path, repo,
            "the ⌂ root row is the project's own dir"
        );
        assert_eq!(main[0].branch, "main");
        let linked = worktrees
            .iter()
            .find(|w| !w.is_main)
            .expect("the worktree we added from is a plain row");
        assert_eq!(linked.path, feat);
        assert_eq!(linked.branch, "gentle-narwhal-files");
    }

    /// Adding the repo from a worktree of one already registered is the
    /// same repo, so it collides instead of arriving as a second project.
    #[tokio::test]
    async fn adding_a_worktree_of_a_known_repo_is_a_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let feat = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &feat.to_string_lossy(), "-b", "feat"],
        );

        let daemon = test_daemon();
        daemon.add_project(&repo, None, false).await.unwrap();
        let err = daemon.add_project(&feat, None, false).await.unwrap_err();
        assert!(
            err.to_string().contains("already added"),
            "expected a duplicate error, got: {err}"
        );
    }

    /// Root-ness is derived from git's checkout list on every pass, not frozen
    /// at insert time: a project whose rows were seeded before the root was
    /// known (or seeded wrong) has its ⌂ root row repaired in place, and the
    /// stale one loses the reprieve that kept it undeletable.
    #[tokio::test]
    async fn reconcile_moves_root_ness_onto_the_checkout_git_lists_first() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        let feat = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &feat.to_string_lossy(), "-b", "feat"],
        );

        let daemon = test_daemon();
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: repo.clone(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        // The wrong way round: the linked checkout wears the root badge and
        // the repo's own checkout is a plain row.
        seed_worktree(&daemon, "p", "wt", &feat.to_string_lossy(), true);
        seed_worktree(&daemon, "p", "rt", &repo.to_string_lossy(), false);

        daemon.sync_project_worktrees(&project).await.unwrap();

        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let by = |id: &str| worktrees.iter().find(|w| w.id.as_str() == id).unwrap();
        assert!(by("rt").is_main, "the repo's checkout is the root row");
        assert!(!by("wt").is_main, "the linked checkout gave the badge back");
        assert_eq!(by("rt").branch, "main");
        assert_eq!(by("wt").branch, "feat");
    }

    /// A row still carrying a stale `is_main` no longer survives its checkout
    /// going away — the real root is always in git's list, so anything missing
    /// from it is a linked checkout, whatever flag it happens to hold.
    #[tokio::test]
    async fn reconcile_drops_a_vanished_row_that_still_claims_to_be_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);

        let daemon = test_daemon();
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: repo.clone(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        seed_worktree(&daemon, "p", "rt", &repo.to_string_lossy(), true);
        seed_worktree(
            &daemon,
            "p",
            "ghost",
            &root.join("repo-worktrees").join("gone").to_string_lossy(),
            true,
        );

        daemon.sync_project_worktrees(&project).await.unwrap();

        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        assert!(
            worktrees.iter().all(|w| w.id.as_str() != "ghost"),
            "the ghost row is gone: {worktrees:#?}"
        );
        let rt = worktrees.iter().find(|w| w.id.as_str() == "rt").unwrap();
        assert!(rt.is_main, "the surviving root row keeps the badge");
    }

    /// A checkout an agent cuts beside the repo (`repo-economy` next to
    /// `repo`, outside any registered root) is adopted by the next sync, and
    /// leaves the sidebar's rows once it is removed on disk.
    #[tokio::test]
    async fn a_sibling_worktree_is_adopted_and_dropped_once_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let sibling = root.join("repo-economy");
        git_in(
            &repo,
            &[
                "worktree",
                "add",
                &sibling.to_string_lossy(),
                "-b",
                "economy",
            ],
        );
        let daemon = test_daemon();
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: repo.clone(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        seed_worktree(&daemon, "p", "rt", &repo.to_string_lossy(), true);

        daemon.sync_project_worktrees(&project).await.unwrap();
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let found = worktrees.iter().find(|w| w.path == sibling).unwrap();
        assert_eq!(found.branch, "economy");

        git_in(
            &repo,
            &["worktree", "remove", "--force", &sibling.to_string_lossy()],
        );
        daemon.sync_project_worktrees(&project).await.unwrap();
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        assert!(
            worktrees.iter().all(|w| w.path != sibling),
            "{worktrees:#?}"
        );
    }

    /// A repo nobody registered, found because it holds an agent's checkout
    /// and sits beside a registered one, becomes a project with its checkouts.
    #[tokio::test]
    async fn an_unregistered_repo_with_agent_checkouts_is_registered() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let known = init_repo(&root);
        let other = root.join("other");
        std::fs::create_dir(&other).unwrap();
        git_in(&other, &["init", "-b", "main"]);
        git_in(&other, &["commit", "--allow-empty", "-m", "init"]);
        let cut = root.join("other-audio");
        git_in(
            &other,
            &["worktree", "add", &cut.to_string_lossy(), "-b", "audio"],
        );
        let daemon = test_daemon();
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: known.clone(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        seed_worktree(&daemon, "p", "rt", &known.to_string_lossy(), true);

        let mut tried = std::collections::HashSet::new();
        assert_eq!(daemon.adopt_agent_repos(&mut tried).await, 1);

        let (projects, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let added = projects.iter().find(|p| p.repo_path == other).unwrap();
        let mine: Vec<_> = worktrees
            .iter()
            .filter(|w| w.project_id == added.id)
            .collect();
        assert_eq!(mine.len(), 2, "{mine:#?}");
        assert!(mine.iter().any(|w| w.path == cut && w.branch == "audio"));
        // A second pass finds nothing new.
        assert_eq!(daemon.adopt_agent_repos(&mut tried).await, 0);
    }

    /// A fresh repo with one commit, at a canonical path (the macOS
    /// tempdir is a symlink, and git reports worktrees canonically).
    fn init_repo(root: &Path) -> PathBuf {
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_in(&repo, &["init", "-b", "main"]);
        git_in(&repo, &["commit", "--allow-empty", "-m", "init"]);
        repo
    }

    #[cfg(unix)]
    fn hook_script(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("hook.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn project_at(daemon: &Daemon, repo: &Path) -> Project {
        let project = Project {
            id: ProjectId("p".into()),
            name: "p".into(),
            repo_path: repo.to_path_buf(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        seed_worktree(daemon, "p", "rt", &repo.to_string_lossy(), true);
        project
    }

    /// Every `Error` a daemon broadcast with no `req_id` — the warnings a
    /// WORKTREE HOOK raises after its request already succeeded.
    fn drain_warnings(events: &mut broadcast::Receiver<ServerEvent>) -> Vec<String> {
        let mut warnings = Vec::new();
        while let Ok(ev) = events.try_recv() {
            if let ServerEvent::Error {
                req_id: None,
                message,
            } = ev
            {
                warnings.push(message);
            }
        }
        warnings
    }

    /// The create hook runs once the checkout exists and its row is out,
    /// with the main repo and the new checkout as its arguments and the
    /// branch in its environment.
    #[cfg(unix)]
    #[tokio::test]
    async fn create_worktree_runs_the_create_hook() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let log = root.join("hook.log");
        let hook = hook_script(
            &root,
            &format!(
                "printf '%s %s %s %s\\n' \"$NEBULA_HOOK\" \"$1\" \"$2\" \"$NEBULA_WORKTREE_BRANCH\" > '{}'",
                log.display()
            ),
        );
        git_in(
            &repo,
            &[
                "config",
                "nebula.worktreeCreateHook",
                &hook.to_string_lossy(),
            ],
        );
        let daemon = test_daemon();
        let project = project_at(&daemon, &repo);
        let mut events = daemon.events.subscribe();

        let created = daemon
            .create_worktree(&project.id, "feat", None)
            .await
            .unwrap();
        let EntityId::Worktree(id) = created else {
            panic!("a worktree id: {created:?}");
        };
        let worktree = daemon.store.get_worktree(&id).unwrap().unwrap();
        assert!(worktree.path.exists(), "the checkout is real");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!(
                "worktree-create {} {} feat\n",
                repo.display(),
                worktree.path.display()
            )
        );
        assert!(
            drain_warnings(&mut events).is_empty(),
            "a clean run warns nobody"
        );
    }

    /// Two callers finding-or-cutting the same branch share one checkout:
    /// the lookup and `git worktree add` both happen under `worktree_ops`.
    #[tokio::test]
    async fn worktree_on_branch_race_shares_the_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let daemon = test_daemon();
        let project = project_at(&daemon, &repo);

        let left = {
            let daemon = daemon.clone();
            let project_id = project.id.clone();
            async move { daemon.worktree_on_branch(&project_id, "feat", None).await }
        };
        let right = {
            let daemon = daemon.clone();
            let project_id = project.id.clone();
            async move { daemon.worktree_on_branch(&project_id, "feat", None).await }
        };
        let (left, right) = tokio::join!(left, right);
        let left = left.unwrap();
        let right = right.unwrap();

        assert_eq!(left.id, right.id);
        assert_eq!(left.path, right.path);
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        let feat_rows: Vec<_> = worktrees.iter().filter(|w| w.branch == "feat").collect();
        assert_eq!(feat_rows.len(), 1, "one registered row: {worktrees:#?}");
        let listed: Vec<_> = git::list_worktrees(&repo)
            .await
            .unwrap()
            .into_iter()
            .filter(|entry| entry.branch == "feat")
            .collect();
        assert_eq!(listed.len(), 1, "one git checkout: {listed:#?}");
    }

    /// A delete that is not forced keeps a checkout with uncommitted work
    /// — refused in words a client can match, the row and the files left
    /// as they were — and the forced one takes it.
    #[tokio::test]
    async fn an_unforced_delete_keeps_a_dirty_checkout_and_force_takes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let wt = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &wt.to_string_lossy(), "-b", "feat"],
        );
        std::fs::write(wt.join("notes.txt"), "unsaved").unwrap();
        let daemon = test_daemon();
        project_at(&daemon, &repo);
        seed_worktree(&daemon, "p", "feat", &wt.to_string_lossy(), false);

        let err = daemon
            .delete_worktree(&WorktreeId("feat".into()), false)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(super::projects::UNCOMMITTED_REFUSAL), "{err}");
        assert!(wt.join("notes.txt").exists(), "the work is still there");
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        assert!(worktrees.iter().any(|w| w.id.as_str() == "feat"));

        daemon
            .delete_worktree(&WorktreeId("feat".into()), true)
            .await
            .unwrap();
        assert!(!wt.exists(), "force takes it");
    }

    /// The delete hook runs after the checkout is gone and the row is
    /// dropped, sees the deleted path, and its failure is a broadcast
    /// warning: the request still succeeds and the row stays gone.
    #[cfg(unix)]
    #[tokio::test]
    async fn delete_worktree_runs_the_delete_hook_and_survives_its_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let wt = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &wt.to_string_lossy(), "-b", "feat"],
        );
        let log = root.join("hook.log");
        let hook = hook_script(
            &root,
            &format!(
                "printf '%s %s %s\\n' \"$NEBULA_HOOK\" \"$1\" \"$2\" > '{}'\n\
                 [ -e \"$2\" ] && echo 'still there' >&2\n\
                 echo 'slot 7 was not ours' >&2\n\
                 exit 2",
                log.display()
            ),
        );
        git_in(
            &repo,
            &[
                "config",
                "nebula.worktreeDeleteHook",
                &hook.to_string_lossy(),
            ],
        );
        let daemon = test_daemon();
        project_at(&daemon, &repo);
        seed_worktree(&daemon, "p", "feat", &wt.to_string_lossy(), false);
        let mut events = daemon.events.subscribe();

        daemon
            .delete_worktree(&WorktreeId("feat".into()), false)
            .await
            .unwrap();

        assert!(!wt.exists(), "the checkout is gone");
        let (_, worktrees, _, _) = daemon.store.load_tree().unwrap();
        assert!(
            worktrees.iter().all(|w| w.id.as_str() != "feat"),
            "the row stays deleted despite the hook: {worktrees:#?}"
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("worktree-delete {} {}\n", repo.display(), wt.display())
        );
        let warnings = drain_warnings(&mut events);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("worktree-delete hook")
                && warnings[0].contains("exited 2: slot 7 was not ours"),
            "{}",
            warnings[0]
        );
    }

    /// A create of the path a delete hook is still releasing waits for
    /// that hook: the delete hook sees the checkout gone (no "still on
    /// disk" skip), then the create hook runs, in that order.
    #[cfg(unix)]
    #[tokio::test]
    async fn recreating_a_path_waits_for_its_delete_hook() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let wt = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &wt.to_string_lossy(), "-b", "feat"],
        );
        let log = root.join("hook.log");
        let hook = hook_script(
            &root,
            &format!(
                "[ \"$NEBULA_HOOK\" = worktree-delete ] && sleep 0.5\n\
                 echo \"$NEBULA_HOOK $2\" >> '{}'",
                log.display()
            ),
        );
        for key in ["nebula.worktreeCreateHook", "nebula.worktreeDeleteHook"] {
            git_in(&repo, &["config", key, &hook.to_string_lossy()]);
        }
        let daemon = test_daemon();
        let project = project_at(&daemon, &repo);
        seed_worktree(&daemon, "p", "feat", &wt.to_string_lossy(), false);
        let mut events = daemon.events.subscribe();

        let deleting = {
            let daemon = daemon.clone();
            tokio::spawn(async move {
                daemon
                    .delete_worktree(&WorktreeId("feat".into()), false)
                    .await
            })
        };
        // Let the delete get into its hook, then ask for the same path back.
        tokio::time::sleep(Duration::from_millis(150)).await;
        daemon
            .create_worktree(&project.id, "feat", None)
            .await
            .unwrap();
        deleting.await.unwrap().unwrap();

        let got = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            got,
            format!(
                "worktree-delete {wt}\nworktree-create {wt}\n",
                wt = wt.display()
            ),
            "delete hook first, create hook second"
        );
        assert!(wt.exists(), "the recreated checkout is there");
        assert!(
            drain_warnings(&mut events).is_empty(),
            "neither hook was skipped or failed"
        );
    }

    /// No hook configured: a delete is exactly what it was.
    #[tokio::test]
    async fn delete_worktree_without_a_hook_warns_nobody() {
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let wt = root.join("repo-worktrees").join("feat");
        git_in(
            &repo,
            &["worktree", "add", &wt.to_string_lossy(), "-b", "feat"],
        );
        let daemon = test_daemon();
        project_at(&daemon, &repo);
        seed_worktree(&daemon, "p", "feat", &wt.to_string_lossy(), false);
        let mut events = daemon.events.subscribe();

        daemon
            .delete_worktree(&WorktreeId("feat".into()), false)
            .await
            .unwrap();

        assert!(!wt.exists());
        assert!(drain_warnings(&mut events).is_empty());
    }

    fn git_in(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn reparent_by_cwd_ignores_foreign_sessions_unless_capturing() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_agent(&daemon, "a1", "root", Some("s1"));

        // A different session id on a non-capturing event (a nested claude
        // launched inside the agent's PTY) must not move the row.
        daemon.reparent_agent_by_cwd(
            &AgentId("a1".into()),
            "/nebula-test/p-feat",
            Some("s2"),
            false,
        );
        assert_eq!(agent_worktree(&daemon, "a1"), "root");

        // A capturing event (re)establishes ownership, so it may move it.
        daemon.reparent_agent_by_cwd(
            &AgentId("a1".into()),
            "/nebula-test/p-feat",
            Some("s2"),
            true,
        );
        assert_eq!(agent_worktree(&daemon, "a1"), "feat");
    }

    #[test]
    fn normalize_url_adds_https_and_refuses_non_links() {
        // Pasted URLs pass through untouched.
        assert_eq!(
            normalize_url("https://github.com/o/r/pull/7").unwrap(),
            "https://github.com/o/r/pull/7"
        );
        assert_eq!(normalize_url("  http://x.dev  ").unwrap(), "http://x.dev");
        // Typed hosts gain the scheme.
        assert_eq!(
            normalize_url("github.com/o/r/pull/7").unwrap(),
            "https://github.com/o/r/pull/7"
        );
        // Anything that isn't an http(s) URL is refused, so `open(1)` can
        // never be handed a scheme the user didn't intend.
        for bad in [
            "",
            "   ",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "just a note",
            "notaurl",
        ] {
            assert!(normalize_url(bad).is_err(), "expected refusal: {bad:?}");
        }
    }

    #[test]
    fn cli_missing_message_names_the_binary_not_the_kind() {
        // Cursor ships its agent as `cursor-agent`; naming the kind would
        // send the user off to install the wrong thing.
        assert!(cli_missing_message(AgentKind::Cursor.cli_program())
            .starts_with("cursor-agent was not found"));
        assert!(cli_missing_message(AgentKind::Claude.cli_program())
            .starts_with("claude was not found"));
        assert!(
            cli_missing_message(AgentKind::Codex.cli_program()).starts_with("codex was not found")
        );
        assert!(cli_missing_message(AgentKind::Pi.cli_program()).starts_with("pi was not found"));
        assert!(cli_missing_message("agy").starts_with("agy was not found"));
        // No "restart nebula": agent CLIs are spawned through the user's
        // login shell, so a fresh install is picked up on the next try.
        for kind in AgentKind::ALL {
            if kind == AgentKind::Custom {
                continue; // no static program; resolved through the registry
            }
            let msg = cli_missing_message(kind.cli_program());
            assert!(msg.contains("try again"), "{msg}");
            assert!(!msg.contains("restart"), "{msg}");
        }
    }

    #[test]
    fn prewarm_pool_buffers_hooks_and_drops_dead_entries() {
        let daemon = test_daemon();
        let key = (WorktreeId("w1".into()), AgentKind::Claude);
        daemon.prewarmed.lock().insert(
            key.clone(),
            PrewarmEntry {
                profile: None,
                agent_id: AgentId("warm-1".into()),
                spawned_at: Instant::now(),
                model: None,
                effort: None,
                buffered_hooks: Vec::new(),
            },
        );

        // Hooks for the warm (row-less) id are buffered on the entry, not
        // dropped; hooks for unrelated unknown ids still vanish quietly.
        daemon.apply_hook_event(
            &AgentId("warm-1".into()),
            HookEvent::SessionStart { source: None },
            Some("sid-9".into()),
        );
        daemon.apply_hook_event(
            &AgentId("stranger".into()),
            HookEvent::Stop { background: None },
            None,
        );
        {
            let pool = daemon.prewarmed.lock();
            let entry = pool.get(&key).unwrap();
            assert_eq!(entry.buffered_hooks.len(), 1);
            assert_eq!(
                entry.buffered_hooks[0],
                (
                    HookEvent::SessionStart { source: None },
                    Some("sid-9".to_string())
                )
            );
        }

        // The buffer is bounded: overflow drops the oldest.
        for i in 0..(PREWARM_HOOK_BUFFER_CAP + 5) {
            daemon.apply_hook_event(
                &AgentId("warm-1".into()),
                HookEvent::Notification {
                    notification_type: Some(format!("n{i}")),
                },
                None,
            );
        }
        assert_eq!(
            daemon
                .prewarmed
                .lock()
                .get(&key)
                .unwrap()
                .buffered_hooks
                .len(),
            PREWARM_HOOK_BUFFER_CAP
        );

        // No live PTY backs the entry, so take() refuses it (create falls
        // back to a cold spawn) and reap clears it out.
        assert!(daemon
            .take_prewarmed(
                &WorktreeId("w1".into()),
                AgentKind::Claude,
                None,
                None,
                None
            )
            .is_none());
        assert!(daemon.prewarmed.lock().is_empty());

        daemon.prewarmed.lock().insert(
            key.clone(),
            PrewarmEntry {
                profile: None,
                agent_id: AgentId("warm-2".into()),
                spawned_at: Instant::now(),
                model: None,
                effort: None,
                buffered_hooks: Vec::new(),
            },
        );
        daemon.reap_prewarmed();
        assert!(daemon.prewarmed.lock().is_empty());
    }

    #[test]
    fn a_spare_fits_only_a_create_on_its_own_account() {
        let entry = PrewarmEntry {
            profile: Some("work".into()),
            agent_id: AgentId("warm".into()),
            spawned_at: Instant::now(),
            model: Some("opus".into()),
            effort: None,
            buffered_hooks: Vec::new(),
        };
        assert!(entry.fits(Some("opus"), None, Some("work")));
        assert!(
            !entry.fits(Some("opus"), None, Some("home")),
            "the spread moved on"
        );
        assert!(!entry.fits(Some("opus"), None, None));
        assert!(!entry.fits(None, None, Some("work")));
    }

    /// Switching `prewarm_agents` off drains the pool on the next sweep. A
    /// spare is a real CLI process the user can see — Claude's own
    /// `/list-agents` lists it beside their sessions, named after the
    /// directory (issue #15) — so the toggle has to take it away now, not
    /// when it ages out.
    #[tokio::test]
    async fn reap_prewarmed_drains_the_pool_once_prewarming_is_off() {
        let daemon = test_daemon();
        let key = (WorktreeId("w1".into()), AgentKind::Claude);
        let id = AgentId("warm-1".into());
        let sref = SessionRef::Agent(id.clone());
        // Any child that idles for the length of the test. `sleep` is only
        // on a Windows PATH inside Git Bash; `ping` ships with Windows.
        #[cfg(unix)]
        let (program, args) = ("sleep", vec!["30".to_string()]);
        #[cfg(windows)]
        let (program, args) = (
            "ping",
            vec!["-n".to_string(), "30".to_string(), "127.0.0.1".to_string()],
        );
        let session = PtySession::spawn(
            sref.clone(),
            SpawnSpec {
                program: program.into(),
                args,
                cwd: std::env::temp_dir(),
                env: vec![],
                scrub_env: &[],
                unset_env: Vec::new(),
                cols: 80,
                rows: 24,
            },
        )
        .unwrap();
        daemon.install_session(session);
        daemon.prewarmed.lock().insert(
            key.clone(),
            PrewarmEntry {
                profile: None,
                agent_id: id.clone(),
                spawned_at: Instant::now(),
                model: None,
                effort: None,
                buffered_hooks: Vec::new(),
            },
        );
        let with_pool = |on: bool| crate::config::Config {
            prewarm_agents: on,
            ..crate::config::Config::default()
        };

        // Live, young and wanted: the ordinary sweep keeps it.
        daemon.reap_prewarmed_with(&with_pool(true));
        assert!(daemon.prewarmed.lock().contains_key(&key));
        assert!(daemon.is_alive(&sref), "a kept spare keeps its PTY");

        // Off: the same sweep takes it, PTY and all.
        daemon.reap_prewarmed_with(&with_pool(false));
        assert!(daemon.prewarmed.lock().is_empty());
        assert!(
            !daemon.is_alive(&sref),
            "a drained spare's PTY goes with it"
        );
    }

    #[test]
    fn kill_prewarmed_in_scopes_to_worktrees() {
        let daemon = test_daemon();
        for (wt, id) in [("w1", "a"), ("w2", "b")] {
            daemon.prewarmed.lock().insert(
                (WorktreeId(wt.into()), AgentKind::Codex),
                PrewarmEntry {
                    profile: None,
                    agent_id: AgentId(id.into()),
                    spawned_at: Instant::now(),
                    model: None,
                    effort: None,
                    buffered_hooks: Vec::new(),
                },
            );
        }
        daemon.kill_prewarmed_in(&[WorktreeId("w1".into())]);
        let pool = daemon.prewarmed.lock();
        assert_eq!(pool.len(), 1);
        assert!(pool.contains_key(&(WorktreeId("w2".into()), AgentKind::Codex)));
    }

    #[test]
    fn reparent_by_cwd_skips_archived_agents() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_worktree(&daemon, "p", "feat", "/nebula-test/p-feat", false);
        seed_agent(&daemon, "a1", "root", None);
        daemon
            .store
            .set_agent_archived(&AgentId("a1".into()), true)
            .unwrap();

        daemon.reparent_agent_by_cwd(&AgentId("a1".into()), "/nebula-test/p-feat", None, false);
        assert_eq!(agent_worktree(&daemon, "a1"), "root");
    }

    /// The status broadcast carries the flag it persisted: a live turn
    /// landing on finished says `unseen`, the next prompt says not.
    #[test]
    fn status_broadcast_carries_the_unseen_flag() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_agent(&daemon, "a1", "root", None); // running
        let id = AgentId("a1".into());
        let mut rx = daemon.events.subscribe();

        daemon.apply_status_effects(&id, vec![Effect::SetStatus(AgentStatus::Finished)]);
        match rx.try_recv().unwrap() {
            ServerEvent::StatusChanged { status, unseen, .. } => {
                assert_eq!(status, AgentStatus::Finished);
                assert!(unseen, "yellow → green with nobody told otherwise");
            }
            other => panic!("expected a status change, got {other:?}"),
        }
        daemon.apply_status_effects(&id, vec![Effect::SetStatus(AgentStatus::Running)]);
        match rx.try_recv().unwrap() {
            ServerEvent::StatusChanged { unseen, .. } => {
                assert!(!unseen, "a new turn: nothing finished to read")
            }
            other => panic!("expected a status change, got {other:?}"),
        }
    }

    /// `mark_agent_seen` clears the flag and hands every subscriber the row
    /// — once. Marking a row already read sends nothing.
    #[test]
    fn mark_agent_seen_broadcasts_only_a_flip() {
        let daemon = test_daemon();
        seed_projects(&daemon, &["p"]);
        seed_worktree(&daemon, "p", "root", "/nebula-test/p", true);
        seed_agent(&daemon, "a1", "root", None);
        let id = AgentId("a1".into());
        daemon
            .store
            .set_agent_status(&id, AgentStatus::Finished)
            .unwrap();
        let mut rx = daemon.events.subscribe();

        daemon.mark_agent_seen(&id).unwrap();
        match rx.try_recv().unwrap() {
            ServerEvent::EntityUpserted {
                entity: Entity::Agent(a),
            } => assert!(!a.unseen),
            other => panic!("expected agent upsert, got {other:?}"),
        }
        daemon.mark_agent_seen(&id).unwrap();
        assert!(rx.try_recv().is_err(), "nothing to say twice");
    }

    /// SESSION SNAPSHOTS: a start when the session starts (kept through a
    /// second start), a last at each turn end, the pair diffed to the
    /// session's own changes, and the refs gone with the row, then with
    /// the project.
    #[test]
    fn session_snapshots_attribute_a_turn_and_their_refs_go_with_the_rows() {
        use nebula_core::session_snapshot::{files_between, kept_agents};
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let project = project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        seed_agent(&daemon, "a2", "rt", None);
        let (a1, a2) = (AgentId("a1".into()), AgentId("a2".into()));
        std::fs::write(
            repo.join("before.txt"),
            "someone else's
",
        )
        .unwrap();
        daemon.snapshot_session_start(&a1);
        daemon.snapshot_session_start(&a2);
        daemon.settle_snapshots();
        let start = daemon.session_changes(&a1).unwrap().start.unwrap();
        assert!(start.tree.is_some());

        std::fs::write(
            repo.join("mine.txt"),
            "a1's work
",
        )
        .unwrap();
        daemon.apply_status_effects(&a1, vec![Effect::SetStatus(AgentStatus::Finished)]);
        // A respawn's start leaves the first one.
        daemon.snapshot_session_start(&a1);
        daemon.settle_snapshots();
        let changes = daemon.session_changes(&a1).unwrap();
        assert_eq!(changes.start.as_ref(), Some(&start));
        assert_eq!(changes.sessions, 2);
        let (from, to) = changes.trees().expect("both trees");
        let files = files_between(&repo, from, to).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["mine.txt"], "only what changed in its turn");
        assert_eq!(daemon.session_changes(&a2).unwrap().last, None);
        assert_eq!(
            kept_agents(&repo, &ns),
            vec!["a1".to_string(), "a2".to_string()]
        );

        daemon.delete_agent(&a1).unwrap();
        daemon.settle_snapshots();
        assert_eq!(kept_agents(&repo, &ns), vec!["a2".to_string()]);
        daemon.remove_project(&project.id).unwrap();
        daemon.settle_snapshots();
        assert!(kept_agents(&repo, &ns).is_empty());
    }

    /// A worktree cut from a named ref records it and where it forked, and
    /// tells 59+ subscribers; one cut from HEAD records nothing.
    #[tokio::test]
    async fn a_cut_records_its_base_and_head_records_none() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let daemon = test_daemon();
        let project = project_at(&daemon, &repo);
        let mut events = daemon.events.subscribe();
        let EntityId::Worktree(id) = daemon
            .create_worktree(&project.id, "feat", Some("main"))
            .await
            .unwrap()
        else {
            panic!("a worktree id");
        };
        let main = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "main"])
            .output()
            .unwrap();
        let main = String::from_utf8_lossy(&main.stdout).trim().to_string();
        let bases = daemon.store.worktree_bases().unwrap();
        assert_eq!(
            bases,
            vec![nebula_core::review_base::RecordedBase {
                worktree: id,
                refname: "main".into(),
                sha: main,
            }]
        );
        let mut told = false;
        while let Ok(ev) = events.try_recv() {
            if let ServerEvent::WorktreeBases { bases: sent } = ev {
                assert_eq!(sent, bases);
                told = true;
            }
        }
        assert!(told, "subscribers hear the new base");
        daemon
            .create_worktree(&project.id, "spike", Some("HEAD"))
            .await
            .unwrap();
        assert_eq!(daemon.store.worktree_bases().unwrap().len(), 1);
    }

    /// A worktree's delete takes its sessions' snapshot refs with it.
    #[tokio::test]
    async fn deleting_a_worktree_drops_its_sessions_snapshot_refs() {
        use nebula_core::session_snapshot::kept_agents;
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let project = project_at(&daemon, &repo);
        let EntityId::Worktree(id) = daemon
            .create_worktree(&project.id, "feat", Some("main"))
            .await
            .unwrap()
        else {
            panic!("a worktree id");
        };
        seed_agent(&daemon, "a1", id.as_str(), None);
        seed_agent(&daemon, "keep", "rt", None);
        daemon.snapshot_session_start(&AgentId("a1".into()));
        daemon.snapshot_session_start(&AgentId("keep".into()));
        daemon.settle_snapshots();
        assert_eq!(
            kept_agents(&repo, &ns),
            vec!["a1".to_string(), "keep".to_string()]
        );
        daemon.delete_worktree(&id, true).await.unwrap();
        daemon.settle_snapshots();
        assert_eq!(kept_agents(&repo, &ns), vec!["keep".to_string()]);
    }

    /// `git <args>` in `repo`, its stdout.
    fn git_out(repo: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// One status edge, its snapshot settled before the next file write.
    fn edge(daemon: &Daemon, id: &str, status: AgentStatus) {
        daemon.apply_status_effects(&AgentId(id.into()), vec![Effect::SetStatus(status)]);
        daemon.settle_snapshots();
    }

    /// One turn of `id`'s: running, `work`, finished.
    fn turn(daemon: &Daemon, id: &str, work: impl FnOnce()) {
        edge(daemon, id, AgentStatus::Running);
        work();
        edge(daemon, id, AgentStatus::Finished);
    }

    /// The paths `id`'s review shows, and the diff text of `path` in it.
    fn session_diff(daemon: &Daemon, repo: &Path, id: &str, path: &str) -> (Vec<String>, String) {
        let changes = daemon.session_changes(&AgentId(id.into())).unwrap();
        let (from, to) = changes.trees().expect("both trees");
        let files = nebula_core::session_snapshot::files_between(repo, from, to).unwrap();
        let path = if path.is_empty() { "." } else { path };
        let diff = git_out(repo, &["diff", "-U0", "--no-color", from, to, "--", path]);
        (files.into_iter().map(|f| f.path).collect(), diff)
    }

    /// The changed lines (`+`/`-`, not the headers) of a diff.
    fn changed_lines(diff: &str) -> Vec<&str> {
        diff.lines()
            .filter(|l| {
                (l.starts_with('+') || l.starts_with('-'))
                    && !l.starts_with("+++")
                    && !l.starts_with("---")
            })
            .map(str::trim_end)
            .collect()
    }

    fn session_refs(repo: &Path, ns: &str, agent: &str) -> String {
        let prefix = format!("refs/kete/d/{ns}/sessions/{agent}/");
        git_out(repo, &["for-each-ref", "--format=%(refname)", &prefix])
    }

    /// TURN ATTRIBUTION: two sessions in one checkout, turns interleaved;
    /// each review shows exactly its own turns' files, where 0.51's
    /// start..last gave a1 a2's file too. The turns and their refs are
    /// recorded per session.
    #[test]
    fn interleaved_turns_attribute_each_session_exactly() {
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        seed_agent(&daemon, "a2", "rt", None);
        daemon.snapshot_session_start(&AgentId("a1".into()));
        daemon.snapshot_session_start(&AgentId("a2".into()));
        let write = |p: &str, body: &str| std::fs::write(repo.join(p), body).unwrap();

        turn(&daemon, "a1", || write("one.txt", "a1 first\n"));
        turn(&daemon, "a2", || write("two.txt", "a2's\n"));
        // Someone with no session at all, between a1's turns.
        write("loose.txt", "by hand\n");
        turn(&daemon, "a1", || write("three.txt", "a1 again\n"));

        assert_eq!(
            session_diff(&daemon, &repo, "a1", "").0,
            vec!["one.txt", "three.txt"]
        );
        assert_eq!(session_diff(&daemon, &repo, "a2", "").0, vec!["two.txt"]);
        let a1 = AgentId("a1".into());
        let turns = daemon.store.session_turns(&a1).unwrap();
        assert_eq!(turns.iter().map(|t| t.n).collect::<Vec<_>>(), vec![1, 2]);
        assert!(turns.iter().all(|t| t.end_at.is_some()));
        let refs = session_refs(&repo, &ns, "a1");
        for slot in [
            "base", "last", "start", "t1/e", "t1/s", "t2/e", "t2/s", "tip",
        ] {
            assert!(
                refs.contains(&format!("sessions/a1/{slot}\n")),
                "{slot} kept: {refs}"
            );
        }
        // The reply keeps its shape: the session's start and last points,
        // carrying the base and tip.
        let changes = daemon.session_changes(&a1).unwrap();
        assert_eq!(changes.sessions, 2);
        let snaps = daemon.store.session_snapshots(&a1).unwrap().unwrap();
        assert_eq!(changes.start.unwrap().tree, snaps.base);
        assert_eq!(changes.last.unwrap().tree, snaps.tip);
    }

    /// Two sessions on the same file: each review shows its own hunks
    /// only, the other's lines in both trees. A needs-feedback wait is a
    /// turn boundary, so an edit made during it is not the session's.
    #[test]
    fn sessions_editing_one_file_see_only_their_own_hunks() {
        let daemon = test_daemon();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        seed_agent(&daemon, "a2", "rt", None);
        let lines = |l: [&str; 8]| l.join("\n") + "\n";
        std::fs::write(
            repo.join("f.txt"),
            lines(["1", "2", "3", "4", "5", "6", "7", "8"]),
        )
        .unwrap();
        git_in(&repo, &["add", "f.txt"]);
        git_in(&repo, &["commit", "-m", "f"]);
        daemon.snapshot_session_start(&AgentId("a1".into()));
        daemon.snapshot_session_start(&AgentId("a2".into()));
        let write = |l: [&str; 8]| std::fs::write(repo.join("f.txt"), lines(l)).unwrap();

        turn(&daemon, "a1", || {
            write(["A1", "2", "3", "4", "5", "6", "7", "8"])
        });
        turn(&daemon, "a2", || {
            write(["A1", "2", "3", "4", "5", "6", "7", "B2"])
        });
        edge(&daemon, "a1", AgentStatus::Running);
        write(["A1", "2", "3", "A1b", "5", "6", "7", "B2"]);
        edge(&daemon, "a1", AgentStatus::NeedsFeedback);
        // a2 while a1 waits on the user.
        turn(&daemon, "a2", || {
            write(["A1", "2", "3", "A1b", "5", "B2b", "7", "B2"])
        });
        edge(&daemon, "a1", AgentStatus::Running);
        edge(&daemon, "a1", AgentStatus::Finished);

        let (files, diff) = session_diff(&daemon, &repo, "a1", "f.txt");
        assert_eq!(files, vec!["f.txt"]);
        assert_eq!(
            changed_lines(&diff),
            vec!["-1", "+A1", "-4", "+A1b"],
            "{diff}"
        );
        let (_, diff) = session_diff(&daemon, &repo, "a2", "f.txt");
        assert_eq!(
            changed_lines(&diff),
            vec!["-6", "+B2b", "-8", "+B2"],
            "{diff}"
        );
    }

    /// Turns past the cap go, rows and refs, and what they did stays in
    /// the review: the base and tip already hold it.
    #[test]
    fn old_turns_are_pruned_and_their_work_still_shows() {
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        seed_agent(&daemon, "a2", "rt", None);
        let write = |p: &str| std::fs::write(repo.join(p), p).unwrap();
        turn(&daemon, "a1", || write("first.txt"));
        let total = snapshots::KEEP_TURNS + 2;
        for n in 2..=total {
            turn(&daemon, "a2", || write(&format!("other{n}.txt")));
            turn(&daemon, "a1", || write(&format!("a1-{n}.txt")));
        }
        let a1 = AgentId("a1".into());
        let kept: Vec<u32> = daemon
            .store
            .session_turns(&a1)
            .unwrap()
            .iter()
            .map(|t| t.n)
            .collect();
        let want: Vec<u32> = ((total - snapshots::KEEP_TURNS + 1)..=total).collect();
        assert_eq!(kept, want);
        let refs = session_refs(&repo, &ns, "a1");
        assert!(!refs.contains("/t1/"), "turn 1's refs pruned: {refs}");
        assert!(refs.contains(&format!("/t{total}/e")));
        let (files, _) = session_diff(&daemon, &repo, "a1", "");
        let mut want = vec!["first.txt".to_string()];
        want.extend((2..=total).map(|n| format!("a1-{n}.txt")));
        want.sort();
        assert_eq!(files, want);
    }

    /// The startup sweep deletes this daemon's refs for sessions it does
    /// not have, and nothing of another daemon's in the same repo: not
    /// its namespace, not 0.51 refs of sessions this one never had.
    #[test]
    fn the_sweep_spares_another_daemons_refs() {
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        daemon.snapshot_session_start(&AgentId("a1".into()));
        daemon.settle_snapshots();
        let tree = git_out(&repo, &["rev-parse", "HEAD^{tree}"]);
        let tree = tree.trim();
        for name in [
            format!("refs/kete/d/{ns}/sessions/ghost/start"),
            format!("refs/kete/d/{ns}/sessions/ghost/t4/e"),
            "refs/kete/d/0th3r/sessions/ghost/start".to_string(),
            "refs/kete/d/0th3r/sessions/a9/last".to_string(),
            "refs/kete/sessions/stranger/start".to_string(),
        ] {
            git_in(&repo, &["update-ref", &name, tree]);
        }
        daemon.sweep_snapshot_refs();
        daemon.settle_snapshots();
        let refs = git_out(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/kete/"],
        );
        let mut want = vec![
            "refs/kete/d/0th3r/sessions/a9/last".to_string(),
            "refs/kete/d/0th3r/sessions/ghost/start".to_string(),
            format!("refs/kete/d/{ns}/sessions/a1/start"),
            "refs/kete/sessions/stranger/start".to_string(),
        ];
        want.sort();
        assert_eq!(refs.lines().collect::<Vec<_>>(), want);
        // A second daemon on its own data dir has its own id; one data
        // dir keeps its.
        assert_ne!(test_daemon().store.daemon_id().unwrap(), ns);
        assert_eq!(daemon.store.daemon_id().unwrap(), ns);
    }

    fn kete_refs(repo: &Path) -> Vec<String> {
        git_out(repo, &["for-each-ref", "--format=%(refname)", "refs/kete/"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Removing a project takes this daemon's refs there with it: its
    /// sessions' and an orphan's (a session whose delete crashed before
    /// its refs went), never another daemon's.
    #[test]
    fn removing_a_project_sweeps_its_repo() {
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        let project = project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        daemon.snapshot_session_start(&AgentId("a1".into()));
        daemon.settle_snapshots();
        let tree = git_out(&repo, &["rev-parse", "HEAD^{tree}"]);
        for name in [
            format!("refs/kete/d/{ns}/sessions/ghost/start"),
            "refs/kete/d/0th3r/sessions/ghost/start".to_string(),
        ] {
            git_in(&repo, &["update-ref", &name, tree.trim()]);
        }
        assert_eq!(kete_refs(&repo).len(), 3);
        daemon.remove_project(&project.id).unwrap();
        daemon.settle_snapshots();
        assert_eq!(
            kete_refs(&repo),
            vec!["refs/kete/d/0th3r/sessions/ghost/start"]
        );
    }

    /// Adding a project sweeps what this daemon left in its repo (a
    /// project removed and added back, refs from a crash between).
    #[tokio::test]
    async fn adding_a_project_sweeps_its_repo() {
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let root = nebula_core::paths::canonical(tmp.path()).unwrap();
        let repo = init_repo(&root);
        let tree = git_out(&repo, &["rev-parse", "HEAD^{tree}"]);
        for name in [
            format!("refs/kete/d/{ns}/sessions/ghost/start"),
            format!("refs/kete/d/{ns}/sessions/ghost/t1/e"),
            "refs/kete/sessions/stranger/start".to_string(),
        ] {
            git_in(&repo, &["update-ref", &name, tree.trim()]);
        }
        daemon.add_project(&repo, None, false).await.unwrap();
        daemon.settle_snapshots();
        assert_eq!(kete_refs(&repo), vec!["refs/kete/sessions/stranger/start"]);
    }

    /// A session from 0.51 (start and last, refs in the old namespace):
    /// the startup sweep moves its refs into this daemon's namespace, its
    /// review still shows what it did before the upgrade, and a 0.52 turn
    /// folds onto that, others' work between left out.
    #[test]
    fn a_051_session_keeps_its_changes_and_moves_its_refs() {
        use nebula_core::session_snapshot::take;
        let daemon = test_daemon();
        let ns = daemon.store.daemon_id().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        let a1 = AgentId("a1".into());
        // What 0.51 left: a start, a last, both refs, and migration 35's
        // base and tip from them.
        let start = take(&repo);
        std::fs::write(repo.join("before.txt"), "pre-upgrade work\n").unwrap();
        let last = take(&repo);
        let (s, l) = (start.tree.clone().unwrap(), last.tree.clone().unwrap());
        let store = &daemon.store;
        store
            .set_snapshot_start(&a1, &WorktreeId("rt".into()), &start)
            .unwrap();
        store.set_snapshot_last(&a1, &last).unwrap();
        store.set_snapshot_view(&a1, &s, Some(&l)).unwrap();
        git_in(&repo, &["update-ref", "refs/kete/sessions/a1/start", &s]);
        git_in(&repo, &["update-ref", "refs/kete/sessions/a1/last", &l]);

        daemon.sweep_snapshot_refs();
        daemon.settle_snapshots();
        let refs = git_out(
            &repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/kete/",
            ],
        );
        assert_eq!(
            refs.trim(),
            format!(
                "refs/kete/d/{ns}/sessions/a1/last {l}\nrefs/kete/d/{ns}/sessions/a1/start {s}"
            )
        );
        assert_eq!(session_diff(&daemon, &repo, "a1", "").0, vec!["before.txt"]);

        std::fs::write(repo.join("between.txt"), "someone else\n").unwrap();
        turn(&daemon, "a1", || {
            std::fs::write(repo.join("after.txt"), "post-upgrade work\n").unwrap()
        });
        assert_eq!(
            session_diff(&daemon, &repo, "a1", "").0,
            vec!["after.txt", "before.txt"]
        );
    }

    /// A turn left open (a restart mid-turn) ends where the next one
    /// starts, and the review shows it then, not a turn later.
    #[test]
    fn an_open_turn_ends_at_the_next_start() {
        let daemon = test_daemon();
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo(tmp.path());
        project_at(&daemon, &repo);
        seed_agent(&daemon, "a1", "rt", None);
        edge(&daemon, "a1", AgentStatus::Running);
        std::fs::write(repo.join("cut.txt"), "work before the restart\n").unwrap();
        edge(&daemon, "a1", AgentStatus::Disconnected);
        edge(&daemon, "a1", AgentStatus::Running);
        let a1 = AgentId("a1".into());
        let turns = daemon.store.session_turns(&a1).unwrap();
        assert_eq!(turns.len(), 2);
        assert!(turns[0].end_at.is_some() && turns[1].end_at.is_none());
        assert_eq!(session_diff(&daemon, &repo, "a1", "").0, vec!["cut.txt"]);
    }
}
