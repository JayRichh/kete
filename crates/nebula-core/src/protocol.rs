use crate::entities::{
    Agent, AgentKind, AgentRole, AgentStatus, Entity, EntityId, Link, Project, TerminalTab,
    Worktree,
};
use crate::ids::{AgentId, LinkId, ProjectId, TerminalId, WorktreeId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Highest IPC protocol this build speaks.
///
/// Bump on every protocol change. Additive changes keep
/// [`MIN_COMPATIBLE_PROTOCOL`] where it is; breaking changes bump both.
pub const PROTOCOL_VERSION: u32 = 64;

/// Oldest IPC protocol this build can safely talk to.
///
/// Compatibility is a range overlap: two peers can talk when each peer's
/// `[MIN_COMPATIBLE_PROTOCOL, PROTOCOL_VERSION]` range includes at least one
/// version the other peer also supports.
pub const MIN_COMPATIBLE_PROTOCOL: u32 = 53;

pub fn protocol_ranges_overlap(
    local_min: u32,
    local_current: u32,
    peer_min: u32,
    peer_current: u32,
) -> bool {
    local_min <= peer_current && peer_min <= local_current
}

pub fn protocol_compatible_with(peer_version: u32) -> bool {
    peer_version >= MIN_COMPATIBLE_PROTOCOL
}

/// The first protocol whose peers decode [`ServerEvent::AccountUsage`]
/// and [`ServerEvent::SessionContexts`]. Both are pushed unasked, so the
/// daemon sends them only to a peer that said it speaks this or later:
/// an older one would fail to decode an unknown variant mid-stream.
///
/// The enums inside them travel by variant name, and a peer refuses a
/// name it has never seen: a new `usage::WindowKind` or
/// `usage::UsageSource` variant (or a new field on their structs) needs a
/// protocol bump and its own gate here, sending the new shape only to
/// peers that said the new version.
pub const USAGE_EVENTS_PROTOCOL: u32 = 55;

/// The first protocol whose daemon takes [`ClientRequest::Reorder`]. An
/// older daemon cannot decode the request and drops the connection on it,
/// so a client checks the version the daemon said at Hello and refuses the
/// move itself, asking for a daemon update, rather than send one.
pub const REORDER_PROTOCOL: u32 = 56;

impl ServerEvent {
    /// Can a peer that said `peer_version` at Hello decode this event?
    pub fn decodable_by(&self, peer_version: u32) -> bool {
        match self {
            ServerEvent::AccountUsage { .. } | ServerEvent::SessionContexts { .. } => {
                peer_version >= USAGE_EVENTS_PROTOCOL
            }
            ServerEvent::WorktreeBases { .. } => peer_version >= RECORDED_BASE_PROTOCOL,
            ServerEvent::SessionChanges { .. } => peer_version >= SESSION_CHANGES_PROTOCOL,
            ServerEvent::WakeDone { .. } | ServerEvent::AccountActivity { .. } => {
                peer_version >= WAKE_PROTOCOL
            }
            ServerEvent::SessionStops { .. } => peer_version >= LIMITS_PROTOCOL,
            ServerEvent::SubagentsRemoved { .. } => peer_version >= SUBAGENT_CLEANUP_PROTOCOL,
            _ => true,
        }
    }

    /// This event as a peer that said `peer_version` at Hello can decode
    /// it: the fields appended since its version left off. Every frame to
    /// a client passes through here (`nebula-daemon` `serve_client`).
    pub fn for_peer(mut self, peer_version: u32) -> ServerEvent {
        if peer_version < SUBAGENT_MODEL_PROTOCOL {
            let blank = |subs: &mut Vec<SubagentSummary>| {
                for s in subs {
                    s.run = None;
                }
            };
            match &mut self {
                ServerEvent::SubagentChanged { sub } => sub.run = None,
                ServerEvent::Snapshot { subagents, .. } => blank(subagents),
                ServerEvent::OrchestratorSessions { sessions, .. } => {
                    for row in sessions {
                        blank(&mut row.subagents);
                    }
                }
                _ => {}
            }
        }
        self
    }
}

/// The first protocol whose daemon takes [`ClientRequest::SwitchAgentAccount`].
/// A client sends it only to a daemon that said this or later at Hello: an
/// older one fails to decode the unknown variant and drops the connection.
pub const SWITCH_ACCOUNT_PROTOCOL: u32 = 57;

/// The first protocol whose daemon reads [`ClientRequest::SpawnSiblingAgent`]'s
/// trailing `profile`. The field is left off the frame when it is None, so a
/// spawn without one is the 57 frame byte for byte; one that names a profile
/// goes only to a daemon that said this or later at Hello (an older one
/// would refuse the longer frame and drop the connection).
pub const SPAWN_PROFILE_PROTOCOL: u32 = 58;

/// The first protocol whose peers decode [`ServerEvent::WorktreeBases`],
/// the base each worktree was cut from. Pushed unasked, after the Snapshot
/// and whenever a worktree is cut, so the daemon sends it only to a peer
/// that said this or later at Hello (`ServerEvent::decodable_by`). A
/// client of an older daemon never hears it, and derives every base as
/// before.
pub const RECORDED_BASE_PROTOCOL: u32 = 59;

/// The first protocol whose daemon takes [`ClientRequest::SessionChanges`]
/// (and whose clients decode its `ServerEvent::SessionChanges` reply). A
/// client checks the version the daemon said at Hello before it sends one:
/// an older daemon drops the connection on the unknown variant.
pub const SESSION_CHANGES_PROTOCOL: u32 = 59;

/// The first protocol whose daemon takes
/// [`ClientRequest::RenameProfileSessions`]. A client renames a profile
/// that sessions use only against a daemon that said this or later at
/// Hello (`profiles::rename_following`); an older one cannot decode the
/// request and would drop the connection, so the rename keeps refusing
/// while a session names the profile (`profiles::rename`).
pub const RENAME_PROFILE_PROTOCOL: u32 = 60;

/// The first protocol whose daemon takes [`ClientRequest::WakeAccount`]
/// and [`ClientRequest::AccountActivity`] (WHAKAOHO, `crate::wake`), and
/// whose clients decode their replies. A client checks the version the
/// daemon said at Hello before it sends either: an older daemon drops the
/// connection on the unknown variant.
pub const WAKE_PROTOCOL: u32 = 61;

/// The first protocol whose peers decode [`ServerEvent::SessionStops`]:
/// why a Claude session's turn stopped short (a usage limit it is waiting
/// out, a login it needs, an API error), beside its status. Pushed
/// unasked, after the Snapshot and as notes change, so the daemon sends it
/// only to a peer that said this or later at Hello. An older client just
/// never shows the note.
pub const LIMITS_PROTOCOL: u32 = 62;

/// The first protocol whose daemon takes [`ClientRequest::ClearEndedSubagents`]
/// and whose clients decode [`ServerEvent::SubagentsRemoved`]. The event is
/// pushed unasked (a clear, the retention sweep, a session restart), so the
/// daemon sends it only to a peer that said this or later at Hello; an
/// older client keeps the removed rows until its next Snapshot. A client
/// offers the clear only against a daemon that said this or later.
pub const SUBAGENT_CLEANUP_PROTOCOL: u32 = 63;

/// The first protocol whose peers read [`SubagentSummary::run`], the model
/// and effort a subagent runs at. The field is left off a row without one,
/// and the daemon blanks it for an older peer (`ServerEvent::for_peer`),
/// so every row an older peer gets is the shape it knows. 63 is 0.55.0's
/// subagent cleanup (`SUBAGENT_CLEANUP_PROTOCOL`).
pub const SUBAGENT_MODEL_PROTOCOL: u32 = 64;

/// Max IPC frame size (length prefix sanity bound).
pub const MAX_FRAME_LEN: u32 = 4 * 1024 * 1024;

/// The most bytes one `Input` carries: a paste longer than a frame goes as
/// several, which the daemon writes to the child back to back, in order.
pub const INPUT_CHUNK: usize = 1024 * 1024;

/// `data` for `session` as `Input` requests of at most [`INPUT_CHUNK`]
/// bytes each. A single frame over [`MAX_FRAME_LEN`] is refused by the
/// codec, and the connection goes with it.
pub fn input_requests(session: &SessionRef, data: Vec<u8>) -> Vec<ClientRequest> {
    if data.len() <= INPUT_CHUNK {
        return vec![ClientRequest::Input {
            session: session.clone(),
            data,
        }];
    }
    data.chunks(INPUT_CHUNK)
        .map(|chunk| ClientRequest::Input {
            session: session.clone(),
            data: chunk.to_vec(),
        })
        .collect()
}

/// Cloud tasks ultimately cross an OS argv boundary (twice: the login
/// shell's `-c` string and Claude's own argv). Leave ample room for shell
/// quoting expansion and the rest of the environment on every platform.
pub const MAX_CLOUD_PROMPT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionRef {
    Agent(AgentId),
    Terminal(TerminalId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientRequest {
    Hello {
        protocol_version: u32,
    },
    /// Reply is one Snapshot, then deltas stream on this connection forever.
    Subscribe,

    // -- PTY plane --
    Attach {
        session: SessionRef,
        /// Resume point for gap-free re-attach; None = replay whole ring.
        from_seq: Option<u64>,
        cols: u16,
        rows: u16,
    },
    Detach {
        session: SessionRef,
    },
    Input {
        session: SessionRef,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    Resize {
        session: SessionRef,
        cols: u16,
        rows: u16,
    },

    // -- entity CRUD (RPC-style; answered by Ack/Error with matching req_id) --
    /// Register a repo as a project. Refused when `path` resolves to a
    /// repo this machine already knows — one repo is one project.
    AddProject {
        req_id: u64,
        path: PathBuf,
        name: Option<String>,
        /// Make `path` a repository if it isn't one: create the directory
        /// when it doesn't exist on disk, and `git init` it when it isn't
        /// inside a repository. Set only after the user confirmed in the
        /// client.
        create_missing: bool,
    },
    RemoveProject {
        req_id: u64,
        id: ProjectId,
    },
    /// Retitle a project's row. Purely cosmetic: `repo_path` — the folder on
    /// disk — is never touched. An empty name resets the row to the folder's
    /// own name, which is the only way back from a rename.
    RenameProject {
        req_id: u64,
        id: ProjectId,
        name: String,
    },
    /// Point a project at the folder its repo now lives in, after the user
    /// renamed or moved it on disk. `path` must be the repo's main checkout
    /// and not another project's. Every worktree row inside the old folder
    /// moves with it, git's own links are repaired, and a row still named
    /// after the old folder takes the new folder's name. Nothing on disk is
    /// moved: the folder is already where `path` says.
    SetProjectPath {
        req_id: u64,
        id: ProjectId,
        path: PathBuf,
    },
    CreateWorktree {
        req_id: u64,
        project: ProjectId,
        branch: String,
        base: Option<String>,
    },
    DeleteWorktree {
        req_id: u64,
        id: WorktreeId,
        force: bool,
    },
    /// Opt-in cleanup for a linked worktree whose PR was detected as merged.
    /// The daemon re-checks the safety invariants before touching disk:
    /// never the root/default branch, branch and HEAD must still match the
    /// trusted PR row, the checkout must be clean, and sessions must be idle.
    CleanupMergedWorktree {
        req_id: u64,
        id: WorktreeId,
        branch: String,
        pr_number: u64,
        pr_url: String,
        head_sha: String,
    },
    CreateAgent {
        req_id: u64,
        worktree: WorktreeId,
        name: String,
        kind: AgentKind,
        /// Registry id of the custom harness, when `kind` is
        /// [`AgentKind::Custom`]. Persisted with the row like `model`.
        #[serde(default)]
        custom_harness: Option<String>,
        /// Model the CLI launches with; None = the CLI's own default.
        model: Option<String>,
        /// Reasoning effort the CLI launches with; None = the CLI's own default.
        effort: Option<String>,
        /// True when the user accepted the generated default name, marking
        /// the session eligible for one agent-driven auto-title (the CLI
        /// runs `nebula rename` on its first prompt).
        auto_title: bool,
        /// One-shot task for a fresh `claude --cloud <task>` launch. This is
        /// deliberately request-only: prompts are not persisted with Agent.
        #[serde(default)]
        cloud_prompt: Option<String>,
        /// The first turn handed to the CLI as its positional prompt
        /// (`claude "<text>"`, `codex "<text>"`, `cursor-agent "<text>"`) —
        /// what an AGENT PRESET launch composes from its prefix, the task
        /// and its postfix. Request-only like `cloud_prompt`: never
        /// persisted, so a RESUME can never replay it. Skips PREWARM POOL
        /// adoption, since a spare booted bare cannot be handed one.
        #[serde(default)]
        starting_prompt: Option<String>,
        /// The GitHub issue this session was created for — an ISSUE
        /// SESSION, launched from the ISSUES MODAL. Persisted with the row
        /// like a PR SESSION's URL, so every cold spawn and RESUME rebuilds
        /// the same issue context: Claude and Pi take it as an appended
        /// system prompt, a Codex / Cursor cold spawn as the opening of its
        /// first prompt. Skips PREWARM POOL adoption like `starting_prompt`,
        /// since a spare booted bare never got it.
        #[serde(default)]
        issue_url: Option<String>,
        /// Special role for the row. Only an orchestrator row may use the
        /// privileged orchestrator command surface.
        #[serde(default)]
        role: AgentRole,
        /// The PROFILE to run under, by name; None = the harness's default
        /// profile (or none). Resolved and persisted with the row.
        #[serde(default)]
        profile: Option<String>,
    },
    /// Create a local AGENT of any kind from an OPEN PRS row — a PR
    /// SESSION. It never runs in the ROOT WORKTREE: the daemon finds the
    /// PROJECT's worktree checked out on the PR's head branch, or creates
    /// one (fetching the branch from `origin`, or the PR ref for a fork),
    /// and every PR SESSION for that PR shares it. The PR URL is persisted
    /// as launch context so every cold spawn and RESUME rebuilds the same
    /// PR-scoped rule — naming that worktree — as Claude's appended system
    /// prompt, or the first prompt a Codex / Cursor cold spawn opens with.
    /// Separate from CreateAgent so ordinary callers cannot accidentally
    /// opt into a partial PR launch. The reply's `created` is the AGENT;
    /// a created worktree arrives as its own `EntityUpserted` first.
    CreatePrAgent {
        req_id: u64,
        /// The PROJECT whose repo the pull request is open on.
        project: ProjectId,
        name: String,
        kind: AgentKind,
        /// Registry id of the custom harness, when `kind` is
        /// [`AgentKind::Custom`]. Persisted with the row like `model`.
        #[serde(default)]
        custom_harness: Option<String>,
        /// Model the CLI launches with; None = the CLI's own default.
        model: Option<String>,
        /// Reasoning effort the CLI launches with; None = default.
        effort: Option<String>,
        auto_title: bool,
        pr_url: String,
        /// The branch the PR SESSION's worktree is checked out on: the
        /// pull request's head branch (`gh`'s `headRefName`) for a
        /// same-repo pull request, and `<owner>/<headRefName>` for a
        /// fork's — a fork's `main` is not ours, and must match neither
        /// the ROOT WORKTREE nor a branch on `origin`. The client names
        /// it; the daemon fetches `origin`'s branch of that name and,
        /// finding none, seeds it from `refs/pull/N/head`.
        head: String,
        /// The CLI's positional first prompt — an AGENT PRESET picked on
        /// the OPEN PRS row composes one, under `CreateAgent`'s rules for
        /// `starting_prompt`. It rides beside the PR rule, which stays
        /// Claude's appended system prompt (or the opening of a Codex /
        /// Cursor cold spawn's first prompt). None: the CLI's own input
        /// is the first prompt. Request-only, never persisted.
        #[serde(default)]
        starting_prompt: Option<String>,
        /// The PROFILE to run under, as on `CreateAgent`.
        #[serde(default)]
        profile: Option<String>,
    },
    /// Fire-and-forget: pre-spawn an agent CLI for this (worktree, kind) so
    /// the next CreateAgent adopts an already-booted session. Sent the
    /// moment the user picks the kind, before they type the name. No reply;
    /// a missing CLI or failed spawn silently degrades to a cold spawn.
    PrewarmAgent {
        worktree: WorktreeId,
        kind: AgentKind,
        /// Must match the CreateAgent that follows or the warm session is
        /// discarded (a CLI booted with the wrong model can't be adopted).
        model: Option<String>,
        effort: Option<String>,
    },
    /// Fire-and-forget: pre-spawn every dead (non-archived) session under a
    /// worktree so attaching later replays an already-booted screen instead
    /// of watching a login shell + CLI boot. Sent once the worktree
    /// selection has rested (debounced client-side); already-alive sessions
    /// are untouched. No reply; a failed spawn degrades to today's lazy
    /// spawn-on-attach.
    PrewarmWorktreeSessions {
        worktree: WorktreeId,
        /// Pane size the sessions boot at, so the later Attach resizes to
        /// the same grid and full-screen apps need no reflow.
        cols: u16,
        rows: u16,
    },
    RenameAgent {
        req_id: u64,
        id: AgentId,
        name: String,
    },
    /// Agent-initiated one-shot title (`nebula rename` inside the session's
    /// CLI). Applies only while the session still awaits its auto-title;
    /// answered with Error (informational, not a fault) once a title —
    /// user- or agent-set — already sticks, so a user rename is never
    /// clobbered by a late or repeated agent attempt.
    AutoRenameAgent {
        req_id: u64,
        id: AgentId,
        name: String,
    },
    /// `nebula worktree <name>`, run by the agent from inside its own
    /// session: create the worktree `branch` under the agent's project (or
    /// take the existing one with that branch), re-home the agent row under
    /// it at once, and relocate the live session into it when its current
    /// turn ends — killed and respawned resumed there, with a prompt that
    /// tells the CLI where it now is. Replies with `WorktreeEntered`.
    EnterWorktree {
        req_id: u64,
        id: AgentId,
        branch: String,
        base: Option<String>,
    },
    /// `nebula spawn "<task>"`, run by the agent from inside its own
    /// session: start a new AGENT beside it — same WORKTREE unless
    /// `worktree` names a branch, and the same AGENT KIND / MODEL / EFFORT
    /// unless `kind` names another harness — with `starting_prompt` as the
    /// new CLI's first prompt, so it begins the task at once. The caller's
    /// own process is untouched. Answered with `Ack { created:
    /// Some(EntityId::Agent(..)) }`; the row reaches every TUI as an
    /// ordinary `EntityUpserted`.
    SpawnSiblingAgent {
        req_id: u64,
        id: AgentId,
        kind: Option<AgentKind>,
        starting_prompt: String,
        /// `--worktree <branch>`: start the session in the caller's
        /// PROJECT's worktree on this branch instead, created (as
        /// `EnterWorktree` creates one) when there is none. None: the
        /// caller's own worktree.
        #[serde(default)]
        worktree: Option<String>,
        /// `--base <ref>`: start point for a new `worktree` branch, resolved
        /// and refused as `EnterWorktree`'s `base` is (a branch that
        /// already exists is refused). Ignored without `worktree`.
        #[serde(default)]
        base: Option<String>,
        /// `--profile <name>`: the ACCOUNT the new session runs on, resolved
        /// as every launch's is (it must exist and be for the new session's
        /// harness). None: the caller's account on the caller's harness,
        /// else that harness's default. Protocol 58; left off the frame
        /// when None, so a 57 daemon still reads every spawn without one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    /// `nebula open <file>…`, run by the agent from inside its own session:
    /// show these files to the user in every attached TUI's FILE TABS —
    /// one tab per file, the focused one previewed, Enter editing it.
    /// Paths are absolute (the CLI resolves them against its own cwd, which
    /// is the agent's). Answered with `Ack`; the files reach every
    /// subscriber as `FilesOpened`.
    OpenFiles {
        req_id: u64,
        id: AgentId,
        paths: Vec<PathBuf>,
    },
    /// Privileged orchestrator command: list the whole daemon tree in one
    /// bounded model-readable response. The daemon refuses this unless
    /// `caller` is the persisted orchestrator row.
    OrchestratorList {
        req_id: u64,
        caller: AgentId,
    },
    /// Privileged orchestrator command: read a bounded tail of a session's
    /// output. This is recent output only, not transcript export.
    OrchestratorRead {
        req_id: u64,
        caller: AgentId,
        session: SessionRef,
        max_bytes: u32,
    },
    /// Privileged orchestrator command: send a follow-up prompt to another
    /// local agent through the same PTY path as the TUI follow-up composer.
    OrchestratorSend {
        req_id: u64,
        caller: AgentId,
        target: AgentId,
        message: String,
    },
    /// Privileged orchestrator command: start an agent in any project and
    /// worktree. `worktree` reuses an existing checkout; otherwise `branch`
    /// is found or cut in `project`.
    OrchestratorSpawn {
        req_id: u64,
        caller: AgentId,
        project: ProjectId,
        worktree: Option<WorktreeId>,
        branch: Option<String>,
        base: Option<String>,
        name: String,
        kind: AgentKind,
        #[serde(default)]
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        starting_prompt: String,
    },
    /// Privileged orchestrator command: ask every TUI client to open the
    /// tabbed review modal.
    OrchestratorOpenReview {
        req_id: u64,
        caller: AgentId,
        sessions: Vec<SessionRef>,
        tabs: Vec<ReviewTabKind>,
    },
    /// Agent-readable list of every session across every project.
    ListSessions {
        req_id: u64,
    },
    /// Agent-readable recent context for one session, addressed by id or
    /// fuzzy name. Prefers harness transcripts when the daemon knows one.
    ReadSession {
        req_id: u64,
        target: String,
        max_lines: u32,
    },
    /// Send a labelled question to another local agent, optionally waiting
    /// for it to become idle first, and answer with clean text from that
    /// turn's output.
    AskSession {
        req_id: u64,
        caller: Option<AgentId>,
        target: String,
        question: String,
        timeout_ms: u64,
        wait: bool,
    },
    /// Kills the PTY, sets archived=1.
    ArchiveAgent {
        req_id: u64,
        id: AgentId,
    },
    UnarchiveAgent {
        req_id: u64,
        id: AgentId,
    },
    DeleteAgent {
        req_id: u64,
        id: AgentId,
    },
    /// Respawn; resumes the stored session id (`claude --resume` /
    /// `codex resume` / `cursor-agent --resume`) when one is stored.
    RestartAgent {
        req_id: u64,
        id: AgentId,
    },
    /// The user moved a session's card onto another checkout, of its own
    /// project or another one: the card menu's **Move to…**, or a drag
    /// onto another band or project tab.
    /// The row moves at once and a live session follows, resumed there:
    /// straight away when idle, at its turn's end when mid-turn. Answered
    /// with `Ack`.
    MoveAgent {
        req_id: u64,
        id: AgentId,
        worktree: WorktreeId,
    },
    /// Queue a message on the Claude Cloud session a row launched
    /// (`claude -p <message> --cloud <id>`). Fire-and-forget by nature: the
    /// CLI acknowledges the send and returns, and the reply only ever
    /// appears in the session's page in the browser. Rejected for rows
    /// without a `cloud_session_id`, and bounded by
    /// [`MAX_CLOUD_PROMPT_BYTES`] like the launch task.
    SendCloudMessage {
        req_id: u64,
        id: AgentId,
        message: String,
    },
    CreateTerminal {
        req_id: u64,
        worktree: WorktreeId,
        name: Option<String>,
    },
    /// Rewrite a link's URL. It is normalized daemon-side (a bare
    /// `github.com/...` gains an https:// scheme) and refused if it can't
    /// be made into an http(s) URL.
    UpdateLink {
        req_id: u64,
        id: LinkId,
        url: String,
    },
    DeleteLink {
        req_id: u64,
        id: LinkId,
    },
    RenameTerminal {
        req_id: u64,
        id: TerminalId,
        name: String,
    },
    CloseTerminal {
        req_id: u64,
        id: TerminalId,
    },
    /// `r` on a worktree: start the project's RUN COMMAND — the `run` of
    /// the `.nebula.json` in that checkout, else the main checkout's, read
    /// fresh — in the worktree's RUN TERMINAL, a login shell running that
    /// line and nothing else. An exited run's row is reused; a run still
    /// going is left alone and named in the reply, so a second client's
    /// press never starts a second one. Answered with
    /// `Ack { created: Some(EntityId::Terminal(..)) }`; no file or no
    /// command is an Error saying what to add.
    StartRun {
        req_id: u64,
        worktree: WorktreeId,
    },
    /// `r` again: kill the worktree's RUN TERMINAL and drop its row.
    /// Nothing running is not an error.
    StopRun {
        req_id: u64,
        worktree: WorktreeId,
    },

    /// Fire-and-forget opaque TUI blob (last selection etc.).
    SaveUiState {
        json: String,
    },

    /// Fire-and-forget: the user just opened this pull request, so
    /// everything up to `marker` has now been read.
    MarkPrSeen {
        url: String,
        marker: String,
    },

    /// Fire-and-forget: this agent's session is on screen, so a turn it
    /// finished unwatched (`Agent::unseen`) has now been looked at. The
    /// daemon answers with the agent's upsert when the flag actually flips.
    MarkAgentSeen {
        id: AgentId,
    },

    /// One point-in-time memory reading — the daemon plus every live
    /// session's process subtree. Answered by `ServerEvent::Metrics` with
    /// the same req_id (not an Ack).
    GetMetrics {
        req_id: u64,
    },

    /// The end of a live session's output ring — what a TERMINAL's card on
    /// the grid shows as the last lines its shell printed. Answered by
    /// `ServerEvent::OutputTail` with the same req_id (not an Ack).
    /// `after_seq` is the ring end the client last heard: a ring that has
    /// not grown past it answers with no bytes, so a grid asking after
    /// every terminal on it once a second costs the idle ones nothing.
    TailOutput {
        req_id: u64,
        session: SessionRef,
        /// At most this many bytes, from the end of the ring.
        max_bytes: u32,
        after_seq: Option<u64>,
    },

    Shutdown,

    // -- protocol 54: orchestrator provisioning and the richer verbs. --
    // Appended after `Shutdown` so every older variant keeps its place.
    /// Start the machine's one ORCHESTRATOR session. With `project` it
    /// runs in that project's root worktree; without, in the dedicated
    /// orchestrator workspace under the data dir (`<data>/orchestrator`,
    /// registered as a project named `orchestrator`). A live orchestrator
    /// already pinned refuses the start unless `replace`, which demotes it.
    /// Refused from inside an agent session. Answered with
    /// `Ack { created: Some(EntityId::Agent(..)) }`.
    StartOrchestrator {
        req_id: u64,
        kind: AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        profile: Option<String>,
        project: Option<ProjectId>,
        name: Option<String>,
        replace: bool,
    },
    /// Pin an existing agent as the ORCHESTRATOR, demoting any other row
    /// that held the role. Refused from inside an agent session.
    SetOrchestrator {
        req_id: u64,
        agent: AgentId,
    },
    /// Demote whichever row holds the ORCHESTRATOR role; none is not an
    /// error. Refused from inside an agent session.
    ClearOrchestrator {
        req_id: u64,
    },
    /// Privileged: every live session in attention order, with role,
    /// parent and subagents. Answered by `ServerEvent::OrchestratorSessions`.
    OrchestratorSessions {
        req_id: u64,
        caller: AgentId,
    },
    /// Privileged: deliver a follow-up to another agent, named by id or
    /// unambiguous fuzzy name, labelled `[from orchestrator]`. With
    /// `wait` a busy target is waited on until idle (bounded by
    /// `timeout_ms`, 0 = the default); without, a busy target refuses.
    /// The caller can never target itself. Answered with `Ack`, whose
    /// `created` names the agent it went to.
    OrchestratorTell {
        req_id: u64,
        caller: AgentId,
        target: String,
        message: String,
        timeout_ms: u64,
        wait: bool,
    },
    /// Privileged: transcript-aware recent context for a session named by
    /// id or fuzzy name, falling back to the cleaned PTY tail. Answered by
    /// `ServerEvent::SessionText`.
    OrchestratorReadSession {
        req_id: u64,
        caller: AgentId,
        target: String,
        max_lines: u32,
    },
    /// Privileged: `OrchestratorSpawn` with a PROFILE.
    OrchestratorSpawnWith {
        req_id: u64,
        caller: AgentId,
        project: ProjectId,
        worktree: Option<WorktreeId>,
        branch: Option<String>,
        base: Option<String>,
        name: String,
        kind: AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        profile: Option<String>,
        starting_prompt: String,
    },

    // -- protocol 56, appended so every older variant keeps its place; sent
    // only to a 56+ daemon (`REORDER_PROTOCOL`). --
    /// The user dragged a row (or pressed move up / down) into a new place:
    /// `ids` is the whole new order of `scope`, first to last. The daemon
    /// writes `sort_order` 0..n in one transaction, skips ids that are not
    /// in the scope (and repeats), appends the scope's rows `ids` left out
    /// in their old order, and broadcasts an upsert for every row whose
    /// slot changed. A project's main checkout is always first among its
    /// worktrees: an order that puts it anywhere else is refused, one that
    /// leaves it out keeps it first. Answered with `Ack`.
    Reorder {
        req_id: u64,
        scope: ReorderScope,
        ids: Vec<String>,
    },

    // -- protocol 57, appended after 56's. --
    /// Restart the agent on another ACCOUNT: the row's profile becomes
    /// `profile` (None: the harness's own login, no profile) and the CLI
    /// boots again in the same worktree, FRESH. A harness keeps its
    /// conversations in its config home and each profile has its own
    /// (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, ...), so the stored session id
    /// would not be found there: it is dropped. Refused for an archived or
    /// Cloud row, a checkout gone from disk, or a profile that is undefined
    /// or for another harness, before anything is killed. Answered with
    /// `Ack`. Sent only to a 57+ daemon (`SWITCH_ACCOUNT_PROTOCOL`).
    SwitchAgentAccount {
        req_id: u64,
        id: AgentId,
        profile: Option<String>,
    },

    // -- protocol 59, appended after 57's. --
    /// A session's snapshots (`nebula_core::session_snapshot`): the trees
    /// its checkout was in when it started and at its latest turn end, for
    /// the review's "this session" filter. Answered by
    /// `ServerEvent::SessionChanges`. Sent only to a 59+ daemon
    /// (`SESSION_CHANGES_PROTOCOL`).
    SessionChanges {
        req_id: u64,
        agent: AgentId,
    },

    // -- protocol 60, appended after 59's. --
    /// A PROFILE was renamed from `old` to `new` (`profiles::
    /// rename_following`, which moved its home and rewrote the settings):
    /// every session row naming `old`, archived ones included, names
    /// `new` from now on, so a stopped one resumes under it. A live
    /// session keeps the home it started in until it restarts. Answered
    /// with `Ack`. Sent only to a 60+ daemon (`RENAME_PROFILE_PROTOCOL`).
    RenameProfileSessions {
        req_id: u64,
        old: String,
        new: String,
    },

    // -- protocol 61, appended after 60's. --
    /// WHAKAOHO: send one tiny prompt to the cheapest model under
    /// `account` (a profile name, or `@claude`) in a hidden session — no
    /// row, never listed, killed when its turn ends or after two minutes
    /// — so its five-hour window starts. Whether it should is the
    /// client's call (`wake::decide`). Answered by `ServerEvent::WakeDone`
    /// once the turn is over. Sent only to a 61+ daemon (`WAKE_PROTOCOL`).
    WakeAccount {
        req_id: u64,
        account: String,
    },
    /// Every account's turns over the last two weeks, as local
    /// quarter-hour counts (`wake::activity`). Answered by
    /// `ServerEvent::AccountActivity`. Sent only to a 61+ daemon.
    AccountActivity {
        req_id: u64,
    },

    // -- protocol 63, appended after 61's. --
    /// Drop every ended subagent (done, failed, or closed with its
    /// session) of `parent`, or of every session for None. Running ones
    /// stay. Answered with `Ack`; the rows leave as
    /// `ServerEvent::SubagentsRemoved`. Sent only to a 63+ daemon
    /// (`SUBAGENT_CLEANUP_PROTOCOL`).
    ClearEndedSubagents {
        req_id: u64,
        parent: Option<AgentId>,
    },
}

/// What a [`ClientRequest::Reorder`] reorders, and the order rule every
/// client lays rows out by: `sort_order` ascending, then the client's own
/// tiebreak (the Snapshot lists rows `sort_order, created_at`, so a stable
/// sort keeps creation order), with a project's main checkout ahead of its
/// other worktrees. A client may group rows on top of that (a board's
/// status lanes, say) but keeps this order within a group.
///
/// A worktree or session scope has a user order when some row's
/// `sort_order` is above 0; until then every row sits at 0 and the
/// tiebreak alone decides (recency, status age). A new worktree or
/// session, and a session moved onto another checkout, goes last in a
/// scope with a user order and at 0 in one without. Projects have always
/// had slots 0..n in the order they were added, and a new one goes last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReorderScope {
    /// Every project: the TUI's project tabs, the desktop's rail.
    Projects,
    /// One project's worktrees.
    Worktrees(ProjectId),
    /// One worktree's sessions (agent rows, archived ones included).
    Agents(WorktreeId),
}

/// How much of a pull request's conversation the user had already seen the
/// last time they opened it. `marker` is the newest thing anyone else had
/// posted at that moment, as GitHub's RFC 3339 stamp — those sort
/// lexicographically, so "arrived since" is a string compare and nebula
/// never has to consult a clock. Empty means the PR was opened while its
/// conversation was still empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrSeen {
    pub url: String,
    pub marker: String,
}

/// Memory usage of one live session: the PTY child plus every descendant
/// (an agent CLI typically fans out into node workers, shells, MCP servers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub session: SessionRef,
    /// OS pid of the PTY child (the subtree's root).
    pub pid: u32,
    /// Resident set size summed over the whole subtree, bytes.
    pub rss_bytes: u64,
    /// Live processes in the subtree, the root included.
    pub procs: u32,
    /// Set when the session is a prewarm-pool spare: an agent CLI the
    /// daemon booted ahead of time for this worktree, waiting for the next
    /// new-agent request there to adopt it. It has no agent row yet, so
    /// this is the only handle a client has for naming and placing it.
    #[serde(default)]
    pub prewarm: Option<PrewarmInfo>,
}

/// Where a prewarm-pool spare is homed and what it booted as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrewarmInfo {
    pub worktree: WorktreeId,
    pub kind: AgentKind,
    pub model: Option<String>,
}

/// The end of a session's ring, for `ClientRequest::TailOutput`: the bytes
/// and the PTY size they were laid out against, so the client's throwaway
/// screen wraps them where the pane would.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTail {
    pub cols: u16,
    pub rows: u16,
    /// Seq the ring's next byte gets — sent back as the next ask's
    /// `after_seq`.
    pub end_seq: u64,
    /// The last `max_bytes` of the ring; empty when it has not grown past
    /// `after_seq`.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// One row in the agent-readable `nebula sessions` listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session: SessionRef,
    pub id: String,
    pub name: String,
    pub project_id: ProjectId,
    pub project: String,
    pub worktree_id: WorktreeId,
    pub worktree: PathBuf,
    pub branch: String,
    /// The agent harness (`claude`, `codex`, ...), or `terminal`.
    pub harness: String,
    /// Model-facing status: `idle`, `running`, `needs_feedback`,
    /// `terminated`, or `disconnected`.
    pub status: String,
    pub alive: bool,
    /// One-line recent context: newest user prompt when known, else title.
    pub summary: String,
    /// The PROFILE an agent session runs under; None for none, and for
    /// every terminal.
    #[serde(default)]
    pub profile: Option<String>,
    /// The first port of its worktree's PORT BLOCK (`PORT` in its
    /// environment), once one is allocated.
    #[serde(default)]
    pub port: Option<u16>,
    /// Ports its worktree's sessions are listening on right now.
    #[serde(default)]
    pub live_ports: Vec<u16>,
}

/// Daemon-side half of the metrics modal's data; the client stacks its own
/// RSS on top. Session subtrees are daemon descendants, so `daemon_rss_bytes`
/// counts the daemon process alone — the total stays double-count-free.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub daemon_pid: u32,
    pub daemon_rss_bytes: u64,
    /// Physical memory installed on the machine, bytes; 0 = unknown.
    pub system_total_bytes: u64,
    pub sessions: Vec<SessionMetrics>,
}

/// What `EnterWorktree` did to the agent's live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnterOutcome {
    /// The agent already lived in that worktree; nothing changed.
    AlreadyThere,
    /// The row moved; the live session respawns inside the worktree, resumed,
    /// once its current turn ends.
    Relocating,
    /// The row moved and nothing was running: the next launch lands there.
    NextLaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTabKind {
    Terminal,
    Diff,
    History,
    PullRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerEvent {
    HelloOk {
        protocol_version: u32,
        daemon_pid: u32,
    },
    Incompatible {
        daemon_protocol_version: u32,
    },
    Snapshot {
        projects: Vec<Project>,
        worktrees: Vec<Worktree>,
        agents: Vec<Agent>,
        terminals: Vec<TerminalTab>,
        links: Vec<Link>,
        /// How far the user has read into each pull request they've opened.
        pr_seen: Vec<PrSeen>,
        ui_state: Option<String>,
        /// Claude's in-session subagents the daemon knows of, open and
        /// recently ended. Last, with a default, so this build still decodes
        /// the protocol-52 shape; a 52 peer cannot decode this one, and is
        /// refused at the handshake (MIN_COMPATIBLE_PROTOCOL 53).
        #[serde(default)]
        subagents: Vec<SubagentSummary>,
    },

    Ack {
        req_id: u64,
        created: Option<EntityId>,
    },
    /// Reply to `EnterWorktree`: the worktree the agent now belongs to, and
    /// what that meant for its process.
    WorktreeEntered {
        req_id: u64,
        worktree: Worktree,
        outcome: EnterOutcome,
    },
    Error {
        req_id: Option<u64>,
        message: String,
    },
    /// Reply to `OrchestratorList`.
    OrchestratorList {
        req_id: u64,
        projects: Vec<Project>,
        worktrees: Vec<Worktree>,
        agents: Vec<Agent>,
        terminals: Vec<TerminalTab>,
        links: Vec<Link>,
    },
    /// Reply to `ListSessions`.
    SessionList {
        req_id: u64,
        sessions: Vec<SessionSummary>,
    },
    /// Reply to `ReadSession`.
    SessionText {
        req_id: u64,
        session: SessionRef,
        text: String,
    },
    /// Reply to `AskSession`.
    SessionAnswer {
        req_id: u64,
        session: AgentId,
        answer: String,
    },
    /// Broadcast by `OrchestratorOpenReview`: clients open a local review
    /// modal for these sessions and tabs.
    ReviewOpened {
        opener: AgentId,
        sessions: Vec<SessionRef>,
        tabs: Vec<ReviewTabKind>,
    },
    /// An `Attach` the DAEMON could not honour: the session's process was
    /// not running and could not be started (its checkout is gone from
    /// disk, its CLI is missing, it is archived, ...). `Attach` carries no
    /// request id, so a plain `Error` could not say which pane it answers;
    /// this one names the session, so the pane that is waiting on it can
    /// say why instead of booting forever.
    AttachRefused {
        session: SessionRef,
        message: String,
    },

    // -- deltas (pushed to all subscribers) --
    EntityUpserted {
        entity: Entity,
    },
    EntityRemoved {
        id: EntityId,
    },
    StatusChanged {
        agent: AgentId,
        status: AgentStatus,
        /// Epoch ms the change was stamped with (matches the persisted
        /// `status_changed_at`, so clients regroup consistently).
        changed_at: i64,
        /// The agent's `unseen` flag after this change: set when a live
        /// turn just finished, cleared when it left `finished`.
        #[serde(default)]
        unseen: bool,
    },

    /// `nebula open` from an agent session: the files the user asked to
    /// see, for every subscriber to raise its FILE TABS on. `root` is the
    /// agent's worktree checkout — the editor's cwd, and what the tab
    /// labels are relative to.
    FilesOpened {
        agent: AgentId,
        root: PathBuf,
        paths: Vec<PathBuf>,
    },

    // -- PTY plane (only to clients attached to that session) --
    /// Ring replay on attach; client resets its parser before applying.
    Scrollback {
        session: SessionRef,
        base_seq: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// Live coalesced output. `seq` = byte offset of the first byte.
    Output {
        session: SessionRef,
        seq: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    SessionExited {
        session: SessionRef,
        exit_code: Option<i32>,
    },
    /// The child's kitty-keyboard-protocol flags changed (or, right after
    /// Scrollback on attach, the current value). 0 = legacy encoding.
    KittyFlags {
        session: SessionRef,
        flags: u8,
    },

    /// Reply to `ClientRequest::GetMetrics`.
    Metrics {
        req_id: u64,
        snapshot: MetricsSnapshot,
    },
    /// Reply to `ClientRequest::TailOutput`: `None` when the session has no
    /// live PTY — its ring went with its shell.
    OutputTail {
        req_id: u64,
        session: SessionRef,
        tail: Option<OutputTail>,
    },
    /// WORKTREE PORTS: every worktree's PORT BLOCK and the ports its
    /// sessions are listening on — the whole list, sent after the Snapshot
    /// and again whenever any of it changes (`nebula_core::ports`).
    WorktreePorts {
        ports: Vec<crate::ports::WorktreePorts>,
    },
    /// A subagent started, ended, or was closed with its parent session.
    /// The whole row each time; keyed by `(parent_agent, id)`.
    SubagentChanged {
        sub: SubagentSummary,
    },

    // -- protocol 54, appended so every older variant keeps its place. --
    /// Reply to `OrchestratorSessions`.
    OrchestratorSessions {
        req_id: u64,
        sessions: Vec<OrchestratorSession>,
    },

    // -- protocol 55, appended; pushed only to 55+ peers
    // (`ServerEvent::decodable_by`). --
    /// Every ACCOUNT's latest plan usage (`nebula_core::usage`): the whole
    /// list, after the Snapshot and again whenever a reading changes.
    AccountUsage {
        accounts: Vec<crate::usage::AccountUsage>,
    },
    /// Sessions' context windows, keyed by agent: every known one after
    /// the Snapshot, then one at a time as they change. Folded in by id.
    SessionContexts {
        contexts: Vec<crate::usage::SessionContext>,
    },

    // -- protocol 59, appended; sent only to 59+ peers
    // (`ServerEvent::decodable_by`). --
    /// The base each worktree Kete cut was cut from
    /// (`review_base::RecordedBase`): the whole list, after the Snapshot
    /// and again whenever a worktree is cut. A worktree missing
    /// from it has no record, and its base is derived.
    WorktreeBases {
        bases: Vec<crate::review_base::RecordedBase>,
    },
    /// Reply to `ClientRequest::SessionChanges`.
    SessionChanges {
        req_id: u64,
        changes: crate::session_snapshot::SessionChanges,
    },

    // -- protocol 61, appended; replies, so only ever to 61+ peers. --
    /// Reply to `ClientRequest::WakeAccount`.
    WakeDone {
        req_id: u64,
        report: crate::wake::WakeReport,
    },
    /// Reply to `ClientRequest::AccountActivity`.
    AccountActivity {
        req_id: u64,
        accounts: Vec<crate::wake::AccountActivity>,
    },
    // -- protocol 62, appended; pushed only to 62+ peers
    // (`ServerEvent::decodable_by`). --
    /// Why sessions' last turns stopped short (`crate::stop`): every known
    /// note after the Snapshot, then one at a time as they change. A
    /// `None` note clears that session's. Folded in by agent id.
    SessionStops {
        stops: Vec<crate::stop::SessionStop>,
    },
    // -- protocol 63, appended; pushed only to 63+ peers
    // (`ServerEvent::decodable_by`). --
    /// Subagent rows the daemon dropped from `parent`'s book: a
    /// `ClearEndedSubagents`, the retention sweep (`subagent_retention`),
    /// or the session restarting. Keyed by `(parent, id)`.
    SubagentsRemoved {
        parent: AgentId,
        ids: Vec<String>,
    },
}

/// One row of `ServerEvent::OrchestratorSessions`: the `kete sessions`
/// row plus what the orchestrator needs to triage it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorSession {
    pub summary: SessionSummary,
    /// Position in attention order, 0 = most in need of the user:
    /// needs_feedback, then running, then finished-unread, then the rest,
    /// most recently changed first within each.
    pub attention: u32,
    /// `needs_feedback`, `running`, `unread`, `idle`, `terminated`,
    /// `disconnected`: `SessionSummary::status` with a finished turn
    /// nobody has looked at yet split out as `unread`.
    pub state: String,
    pub role: AgentRole,
    pub parent: Option<AgentId>,
    pub status_changed_at: i64,
    /// Claude subagents open or recently ended inside this session.
    pub subagents: Vec<SubagentSummary>,
}

/// Ended subagents kept per parent session; beyond it the oldest ended
/// go, in the daemon's book and in every client mirroring it.
pub const SUBAGENTS_ENDED_KEPT: usize = 20;

/// One of Claude's in-session subagents (the Task / Agent tool), as its
/// SubagentStart and SubagentStop hooks report it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentSummary {
    /// The hook payload's `agent_id` (Cursor's `subagent_id`). Unique
    /// within its parent session only.
    pub id: String,
    /// The session the subagent runs inside.
    pub parent_agent: AgentId,
    /// The payload's `agent_type`: `general-purpose`, `Explore`, a custom
    /// agent's name. Empty when the CLI sent none.
    pub kind: String,
    /// The task the parent gave it: the `description` of the Task / Agent
    /// tool call that started it. Empty when none was seen.
    pub description: String,
    /// Epoch ms.
    pub started_at: i64,
    /// Epoch ms; None while it runs.
    pub ended_at: Option<i64>,
    pub status: SubagentStatus,
    // -- protocol 64, appended (`SUBAGENT_MODEL_PROTOCOL`). --
    /// The model and effort it runs at, when either is known. Left off the
    /// frame when None, so the row is 63's byte for byte; the daemon blanks
    /// it for an older peer (`ServerEvent::for_peer`), which would refuse
    /// the longer frame. Must stay the last field: a later addition goes
    /// inside [`SubagentRun`] behind its own gate, or after this one the
    /// same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<SubagentRun>,
}

/// What a subagent runs on, beside its type (protocol 64).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentRun {
    /// The model the Task / Agent call asked for (`haiku`, `sonnet`, a
    /// full id). Empty when it named none: the subagent runs on its
    /// definition's model, else the session's.
    #[serde(default)]
    pub model: String,
    /// The reasoning effort it runs at: the call's `effort`, else the
    /// `effort.level` its own hooks report. Empty when unknown.
    #[serde(default)]
    pub effort: String,
}

impl SubagentRun {
    /// None when neither is known: the row then carries no `run`.
    pub fn known(model: String, effort: String) -> Option<SubagentRun> {
        (!model.is_empty() || !effort.is_empty()).then_some(SubagentRun { model, effort })
    }
}

impl SubagentSummary {
    /// The model it runs on; empty when unknown.
    pub fn model(&self) -> &str {
        self.run.as_ref().map_or("", |r| r.model.as_str())
    }

    /// The effort it runs at; empty when unknown.
    pub fn effort(&self) -> &str {
        self.run.as_ref().map_or("", |r| r.effort.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Running,
    /// Its SubagentStop arrived.
    Done,
    /// Reserved: Claude's hooks report no failed subagent today.
    Failed,
    /// Still open when its parent session ended or the daemon restarted:
    /// whether it finished is unknown.
    EndedUnknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paste bigger than a frame goes as frames that fit, and back to
    /// back they are the paste, markers and all.
    #[test]
    fn a_paste_bigger_than_a_frame_is_split_into_frames_that_fit() {
        let session = SessionRef::Agent(crate::AgentId("a1".into()));
        let paste = [
            &b"\x1b[200~"[..],
            &vec![b'x'; 5 * 1024 * 1024],
            b"\x1b[201~",
        ]
        .concat();
        let reqs = input_requests(&session, paste.clone());
        assert_eq!(reqs.len(), 6);
        let mut joined = Vec::new();
        for req in &reqs {
            let frame = rmp_serde::to_vec(req).unwrap();
            assert!(
                frame.len() <= MAX_FRAME_LEN as usize,
                "{} bytes",
                frame.len()
            );
            match req {
                ClientRequest::Input { session: s, data } => {
                    assert_eq!(s, &session);
                    joined.extend_from_slice(data);
                }
                other => panic!("not an Input: {other:?}"),
            }
        }
        assert_eq!(joined, paste);
        assert_eq!(input_requests(&session, b"a".to_vec()).len(), 1);
    }

    #[test]
    fn protocol_ranges_overlap_when_either_side_is_additive() {
        assert!(protocol_ranges_overlap(47, 47, 47, 48));
        assert!(protocol_ranges_overlap(47, 48, 47, 47));
        assert!(!protocol_ranges_overlap(48, 48, 47, 47));
        assert!(!protocol_ranges_overlap(47, 47, 48, 48));
    }

    #[test]
    fn protocol_compatibility_accepts_current_and_additive_peers() {
        assert!(protocol_compatible_with(PROTOCOL_VERSION));
        assert!(protocol_compatible_with(PROTOCOL_VERSION + 1));
        assert!(!protocol_compatible_with(MIN_COMPATIBLE_PROTOCOL - 1));
    }

    /// Protocol 53 grew `Agent` and `Snapshot` by a trailing field, which
    /// a 52 decoder refuses mid-stream (rmp `LengthMismatch`). 52 must
    /// therefore be refused at the handshake, on both sides.
    #[test]
    fn protocol_52_peers_are_refused_at_the_handshake() {
        assert!(!protocol_compatible_with(52));
        assert!(!protocol_ranges_overlap(
            MIN_COMPATIBLE_PROTOCOL,
            PROTOCOL_VERSION,
            52,
            52
        ));
    }

    /// The codec names enum variants on the wire, so the protocol-54
    /// variants appended at the end leave every 53 variant's frame
    /// byte-for-byte what a 53 peer sends and expects.
    #[test]
    fn variants_travel_by_name_so_appending_is_additive() {
        let bytes = encode(&ClientRequest::Shutdown);
        assert!(
            bytes.windows(8).any(|w| w == b"Shutdown"),
            "variant encoded by index: appending variants would not be additive"
        );
    }

    #[test]
    fn protocol_55_usage_events_round_trip_and_are_held_back_from_older_peers() {
        use crate::usage::{
            AccountUsage, ContextUsage, SessionContext, UsageSource, UsageWindow, WindowKind,
        };
        let usage = ServerEvent::AccountUsage {
            accounts: vec![AccountUsage {
                account: "@claude".into(),
                harness: "claude".into(),
                windows: vec![UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent: 21.5,
                    resets_at: Some(1_791_891_379),
                    read_at: 0,
                }],
                source: UsageSource::Statusline,
                at: 9,
                plan: None,
            }],
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&usage)).unwrap();
        let ServerEvent::AccountUsage { accounts } = back else {
            panic!("not AccountUsage");
        };
        assert_eq!(accounts[0].windows[0].percent, 21.5);
        assert_eq!(accounts[0].account, "@claude");

        let ctx = ServerEvent::SessionContexts {
            contexts: vec![SessionContext {
                agent: AgentId("a".into()),
                context: ContextUsage {
                    percent: Some(42.0),
                    tokens: Some(84_000),
                    window: Some(200_000),
                    compactions: 2,
                    last_compact_at: None,
                    source: UsageSource::Transcript,
                    at: 3,
                },
            }],
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&ctx)).unwrap();
        let ServerEvent::SessionContexts { contexts } = back else {
            panic!("not SessionContexts");
        };
        assert_eq!(contexts[0].context.compactions, 2);

        assert!(usage.decodable_by(55));
        assert!(
            !usage.decodable_by(54),
            "a 54 peer would drop the connection"
        );
        assert!(!ctx.decodable_by(53));
        assert!(ServerEvent::WorktreePorts { ports: Vec::new() }.decodable_by(53));
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "55 and 56 are additive");
        assert!(protocol_compatible_with(54));
    }

    /// 57 adds one request a client sends only to a 57 daemon: older peers
    /// stay in range, and the request round-trips with and without a
    /// profile (None: the harness's own login).
    #[test]
    fn protocol_57_is_additive_and_switch_account_round_trips() {
        assert!(protocol_compatible_with(56));
        assert!(protocol_ranges_overlap(
            MIN_COMPATIBLE_PROTOCOL,
            PROTOCOL_VERSION,
            53,
            56
        ));
        const { assert!(SWITCH_ACCOUNT_PROTOCOL <= PROTOCOL_VERSION) };
        for profile in [Some("work".to_string()), None] {
            let req = ClientRequest::SwitchAgentAccount {
                req_id: 9,
                id: AgentId("a1".into()),
                profile: profile.clone(),
            };
            let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
            let ClientRequest::SwitchAgentAccount {
                req_id,
                id,
                profile: got,
            } = back
            else {
                panic!("not a SwitchAgentAccount");
            };
            assert_eq!((req_id, id.0.as_str(), got), (9, "a1", profile));
        }
    }

    /// 60 appends `RenameProfileSessions`, sent only to a 60 daemon: older
    /// peers stay in range and the request round-trips.
    #[test]
    fn protocol_60_rename_profile_sessions_round_trips() {
        assert!(protocol_compatible_with(59));
        const { assert!(RENAME_PROFILE_PROTOCOL <= PROTOCOL_VERSION) };
        let req = ClientRequest::RenameProfileSessions {
            req_id: 4,
            old: "work".into(),
            new: "max2".into(),
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
        let ClientRequest::RenameProfileSessions { req_id, old, new } = back else {
            panic!("not a RenameProfileSessions");
        };
        assert_eq!((req_id, old.as_str(), new.as_str()), (4, "work", "max2"));
    }

    /// 61 appends `WakeAccount` and `AccountActivity` with their replies:
    /// older peers stay in range, never hear the replies, and both
    /// round-trip.
    #[test]
    fn protocol_61_wake_requests_and_replies_round_trip() {
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "61 is additive");
        const { assert!(WAKE_PROTOCOL <= PROTOCOL_VERSION) };
        let req = ClientRequest::WakeAccount {
            req_id: 3,
            account: "@claude".into(),
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
        let ClientRequest::WakeAccount { req_id, account } = back else {
            panic!("not a WakeAccount");
        };
        assert_eq!((req_id, account.as_str()), (3, "@claude"));
        let back: ClientRequest =
            rmp_serde::from_slice(&encode(&ClientRequest::AccountActivity { req_id: 5 })).unwrap();
        assert!(matches!(back, ClientRequest::AccountActivity { req_id: 5 }));

        let report = crate::wake::WakeReport {
            resets_before: Some(10),
            resets_after: Some(20),
            error: None,
            reason: "fired".into(),
            submitted: true,
            finished: true,
            read: true,
        };
        let done = ServerEvent::WakeDone {
            req_id: 3,
            report: report.clone(),
        };
        assert!(!done.decodable_by(60) && done.decodable_by(61));
        let ServerEvent::WakeDone {
            req_id,
            report: got,
        } = rmp_serde::from_slice(&encode(&done)).unwrap()
        else {
            panic!("not a WakeDone");
        };
        assert_eq!((req_id, got), (3, report));
        let activity = ServerEvent::AccountActivity {
            req_id: 5,
            accounts: vec![crate::wake::AccountActivity {
                account: "work".into(),
                bins: vec![1; crate::wake::activity::BINS],
                last_turn_at: Some(99),
            }],
        };
        assert!(!activity.decodable_by(60));
        let ServerEvent::AccountActivity { accounts, .. } =
            rmp_serde::from_slice(&encode(&activity)).unwrap()
        else {
            panic!("not an AccountActivity");
        };
        assert_eq!(accounts[0].bins.len(), 96);
        assert_eq!(accounts[0].last_turn_at, Some(99));
    }

    /// 58 adds `profile` to the end of `SpawnSiblingAgent`. Without one the
    /// frame is 57's byte for byte (so a 57 daemon reads it), a 57 frame
    /// reads back as no profile, and a named one round-trips.
    #[test]
    fn protocol_58_spawn_profile_is_additive() {
        /// `SpawnSiblingAgent` as protocol 57 knew it.
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum V57 {
            SpawnSiblingAgent {
                req_id: u64,
                id: AgentId,
                kind: Option<AgentKind>,
                starting_prompt: String,
                #[serde(default)]
                worktree: Option<String>,
                #[serde(default)]
                base: Option<String>,
            },
        }
        const { assert!(SPAWN_PROFILE_PROTOCOL <= PROTOCOL_VERSION) };
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "58 is additive");
        assert!(protocol_compatible_with(57));
        let spawn = |profile: Option<&str>| ClientRequest::SpawnSiblingAgent {
            req_id: 4,
            id: AgentId("a1".into()),
            kind: Some(AgentKind::Codex),
            starting_prompt: "fix it".into(),
            worktree: Some("fix-login".into()),
            base: None,
            profile: profile.map(str::to_string),
        };
        let old = V57::SpawnSiblingAgent {
            req_id: 4,
            id: AgentId("a1".into()),
            kind: Some(AgentKind::Codex),
            starting_prompt: "fix it".into(),
            worktree: Some("fix-login".into()),
            base: None,
        };
        assert_eq!(encode(&spawn(None)), encode(&old));
        let from_57: V57 = rmp_serde::from_slice(&encode(&spawn(None))).unwrap();
        assert_eq!(from_57, old);
        let back: ClientRequest = rmp_serde::from_slice(&encode(&old)).unwrap();
        let ClientRequest::SpawnSiblingAgent { profile, .. } = back else {
            panic!("not a SpawnSiblingAgent");
        };
        assert_eq!(profile, None);
        let back: ClientRequest = rmp_serde::from_slice(&encode(&spawn(Some("work")))).unwrap();
        let ClientRequest::SpawnSiblingAgent {
            profile, worktree, ..
        } = back
        else {
            panic!("not a SpawnSiblingAgent");
        };
        assert_eq!(
            (profile.as_deref(), worktree.as_deref()),
            (Some("work"), Some("fix-login"))
        );
        assert!(
            rmp_serde::from_slice::<V57>(&encode(&spawn(Some("work")))).is_err(),
            "a 57 daemon refuses a named profile: the client gates it"
        );
    }

    #[test]
    fn protocol_54_is_additive_over_53() {
        assert!(protocol_ranges_overlap(
            MIN_COMPATIBLE_PROTOCOL,
            PROTOCOL_VERSION,
            53,
            53
        ));
        assert!(protocol_compatible_with(53));
    }

    /// 56 only adds `Reorder`, appended: a 0.47 (55) peer still shakes
    /// hands, and every scope survives the wire.
    #[test]
    fn protocol_56_adds_reorder_without_dropping_55_peers() {
        const { assert!(PROTOCOL_VERSION >= REORDER_PROTOCOL) };
        assert!(protocol_ranges_overlap(
            MIN_COMPATIBLE_PROTOCOL,
            PROTOCOL_VERSION,
            53,
            55
        ));
        for scope in [
            ReorderScope::Projects,
            ReorderScope::Worktrees(ProjectId("p".into())),
            ReorderScope::Agents(WorktreeId("w".into())),
        ] {
            let req = ClientRequest::Reorder {
                req_id: 9,
                scope: scope.clone(),
                ids: vec!["b".into(), "a".into()],
            };
            let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
            let ClientRequest::Reorder {
                req_id,
                scope: got,
                ids,
            } = back
            else {
                panic!("not Reorder");
            };
            assert_eq!((req_id, got, ids), (9, scope, vec!["b".into(), "a".into()]));
        }
    }

    #[test]
    fn protocol_54_requests_and_replies_round_trip() {
        let start = ClientRequest::StartOrchestrator {
            req_id: 3,
            kind: AgentKind::Codex,
            custom_harness: None,
            model: Some("gpt-5".into()),
            effort: None,
            profile: Some("work".into()),
            project: None,
            name: None,
            replace: true,
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&start)).unwrap();
        let ClientRequest::StartOrchestrator {
            kind,
            profile,
            replace,
            ..
        } = back
        else {
            panic!("not a StartOrchestrator");
        };
        assert_eq!(kind, AgentKind::Codex);
        assert_eq!(profile.as_deref(), Some("work"));
        assert!(replace);

        let tell = ClientRequest::OrchestratorTell {
            req_id: 4,
            caller: AgentId("o".into()),
            target: "fix-login".into(),
            message: "status?".into(),
            timeout_ms: 0,
            wait: true,
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&tell)).unwrap();
        assert!(
            matches!(back, ClientRequest::OrchestratorTell { target, .. } if target == "fix-login")
        );

        let row = OrchestratorSession {
            summary: SessionSummary {
                session: SessionRef::Agent(AgentId("child".into())),
                id: "child".into(),
                name: "agent-2".into(),
                project_id: ProjectId("p".into()),
                project: "demo".into(),
                worktree_id: WorktreeId("w".into()),
                worktree: PathBuf::from("/w"),
                branch: "main".into(),
                harness: "claude".into(),
                status: "idle".into(),
                alive: true,
                summary: "fix it".into(),
                profile: None,
                port: None,
                live_ports: Vec::new(),
            },
            attention: 0,
            state: "unread".into(),
            role: AgentRole::Worker,
            parent: Some(AgentId("o".into())),
            status_changed_at: 9,
            subagents: vec![sample_subagent()],
        };
        let reply = ServerEvent::OrchestratorSessions {
            req_id: 5,
            sessions: vec![row.clone()],
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&reply)).unwrap();
        let ServerEvent::OrchestratorSessions { sessions, .. } = back else {
            panic!("not OrchestratorSessions");
        };
        assert_eq!(sessions, vec![row]);
    }

    fn sample_agent(parent: Option<&str>) -> Agent {
        Agent {
            id: AgentId("child".into()),
            worktree_id: WorktreeId("w".into()),
            name: "agent-2".into(),
            status: AgentStatus::Running,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 7,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role: AgentRole::Worker,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
            profile: None,
            parent: parent.map(|p| AgentId(p.into())),
        }
    }

    fn sample_subagent() -> SubagentSummary {
        SubagentSummary {
            id: "agent-abc123".into(),
            parent_agent: AgentId("child".into()),
            kind: "Explore".into(),
            description: "Find the hook fixtures".into(),
            started_at: 10,
            ended_at: None,
            status: SubagentStatus::Running,
            run: None,
        }
    }

    fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
        rmp_serde::to_vec(msg).unwrap()
    }

    /// 59 appends `SessionChanges` (request and reply) and `WorktreeBases`:
    /// older peers stay in range, a 58 peer is never pushed the new event,
    /// and both round-trip. A 58 daemon's decoder (the enum as it ended at
    /// `SwitchAgentAccount`) refuses the request, which is why clients gate it.
    #[test]
    fn protocol_59_is_additive_and_its_messages_round_trip() {
        use crate::review_base::RecordedBase;
        use crate::session_snapshot::{SessionChanges, SnapshotPoint};
        const { assert!(RECORDED_BASE_PROTOCOL <= PROTOCOL_VERSION) };
        const { assert!(SESSION_CHANGES_PROTOCOL <= PROTOCOL_VERSION) };
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "59 is additive");
        assert!(protocol_compatible_with(58));
        assert!(protocol_ranges_overlap(
            MIN_COMPATIBLE_PROTOCOL,
            PROTOCOL_VERSION,
            53,
            58
        ));

        let bases = ServerEvent::WorktreeBases {
            bases: vec![RecordedBase {
                worktree: WorktreeId("w".into()),
                refname: "origin/main".into(),
                sha: "abc".into(),
            }],
        };
        assert!(bases.decodable_by(59));
        assert!(
            !bases.decodable_by(58),
            "a 58 peer would drop the connection"
        );
        let back: ServerEvent = rmp_serde::from_slice(&encode(&bases)).unwrap();
        let ServerEvent::WorktreeBases { bases: got } = back else {
            panic!("not WorktreeBases");
        };
        assert_eq!(got[0].refname, "origin/main");

        let req = ClientRequest::SessionChanges {
            req_id: 7,
            agent: AgentId("a1".into()),
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
        let ClientRequest::SessionChanges { req_id, agent } = back else {
            panic!("not SessionChanges");
        };
        assert_eq!((req_id, agent.0.as_str()), (7, "a1"));

        /// `ClientRequest` as a 58 daemon knows its tail: the variant
        /// before 59's, at the same position.
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        enum V58Tail {
            SwitchAgentAccount {
                req_id: u64,
                id: AgentId,
                profile: Option<String>,
            },
        }
        assert!(rmp_serde::from_slice::<V58Tail>(&encode(&req)).is_err());

        let point = |tree: Option<&str>| SnapshotPoint {
            head: Some("h".into()),
            tree: tree.map(str::to_string),
            at: 5,
        };
        let reply = ServerEvent::SessionChanges {
            req_id: 7,
            changes: SessionChanges {
                agent: AgentId("a1".into()),
                worktree: Some(WorktreeId("w".into())),
                start: Some(point(Some("t1"))),
                last: Some(point(None)),
                sessions: 2,
            },
        };
        assert!(!reply.decodable_by(58));
        let back: ServerEvent = rmp_serde::from_slice(&encode(&reply)).unwrap();
        let ServerEvent::SessionChanges { req_id, changes } = back else {
            panic!("not SessionChanges");
        };
        assert_eq!(req_id, 7);
        assert_eq!(changes.sessions, 2);
        assert_eq!(changes.last.as_ref().unwrap().tree, None, "unavailable");
        assert_eq!(changes.trees(), None);
    }

    /// 63 appends `ClearEndedSubagents` and `SubagentsRemoved`: older peers
    /// stay in range, are never pushed the removal, and both round-trip.
    #[test]
    fn protocol_63_subagent_cleanup_round_trips_and_is_held_back_from_older_peers() {
        const { assert!(SUBAGENT_CLEANUP_PROTOCOL <= PROTOCOL_VERSION) };
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "63 is additive");
        let req = ClientRequest::ClearEndedSubagents {
            req_id: 4,
            parent: Some(AgentId("p".into())),
        };
        let back: ClientRequest = rmp_serde::from_slice(&encode(&req)).unwrap();
        let ClientRequest::ClearEndedSubagents { req_id, parent } = back else {
            panic!("not ClearEndedSubagents");
        };
        assert_eq!((req_id, parent), (4, Some(AgentId("p".into()))));

        let removed = ServerEvent::SubagentsRemoved {
            parent: AgentId("p".into()),
            ids: vec!["a".into(), "b".into()],
        };
        assert!(removed.decodable_by(SUBAGENT_CLEANUP_PROTOCOL));
        assert!(!removed.decodable_by(SUBAGENT_CLEANUP_PROTOCOL - 1));
        let back: ServerEvent = rmp_serde::from_slice(&encode(&removed)).unwrap();
        let ServerEvent::SubagentsRemoved { parent, ids } = back else {
            panic!("not SubagentsRemoved");
        };
        assert_eq!(parent, AgentId("p".into()));
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    /// A subagent row as protocol 63 and earlier knew it.
    mod v63 {
        use super::*;
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        pub struct SubagentSummary {
            pub id: String,
            pub parent_agent: AgentId,
            pub kind: String,
            pub description: String,
            pub started_at: i64,
            pub ended_at: Option<i64>,
            pub status: SubagentStatus,
        }
        #[derive(Debug, Deserialize)]
        pub enum ServerEvent {
            SubagentChanged { sub: SubagentSummary },
        }
    }

    /// 64 appends `run` to a subagent row: it round-trips, a row without
    /// one is the old frame, and an older peer reads what `for_peer` sends
    /// it, which it would refuse with the field on.
    #[test]
    fn protocol_64_subagent_run_round_trips_and_is_blanked_for_older_peers() {
        const { assert!(SUBAGENT_MODEL_PROTOCOL <= PROTOCOL_VERSION) };
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "64 is additive");
        let mut sub = sample_subagent();
        sub.run = SubagentRun::known("haiku".into(), "low".into());
        let changed = ServerEvent::SubagentChanged { sub: sub.clone() };

        let back: ServerEvent = rmp_serde::from_slice(&encode(&changed)).unwrap();
        let ServerEvent::SubagentChanged { sub: got } = back else {
            panic!("not SubagentChanged");
        };
        assert_eq!((got.model(), got.effort()), ("haiku", "low"));

        assert!(
            rmp_serde::from_slice::<v63::ServerEvent>(&encode(&changed)).is_err(),
            "a 63 peer refuses the longer row"
        );
        let held = changed.clone().for_peer(SUBAGENT_MODEL_PROTOCOL - 1);
        let old: v63::ServerEvent = rmp_serde::from_slice(&encode(&held)).unwrap();
        let v63::ServerEvent::SubagentChanged { sub: old } = old;
        assert_eq!(
            (old.id.as_str(), old.kind.as_str()),
            ("agent-abc123", "Explore")
        );
        let ServerEvent::SubagentChanged { sub: kept } = changed.for_peer(SUBAGENT_MODEL_PROTOCOL)
        else {
            panic!("not SubagentChanged");
        };
        assert_eq!(kept.model(), "haiku", "a 64 peer keeps it");

        // A 63 daemon's row reads here as unknown.
        let bare = ServerEvent::SubagentChanged {
            sub: sample_subagent(),
        };
        let ServerEvent::SubagentChanged { sub: read } =
            rmp_serde::from_slice::<ServerEvent>(&encode(&bare)).unwrap()
        else {
            panic!("not SubagentChanged");
        };
        assert_eq!(read.model(), "");
        assert_eq!(read.run, None);
        assert_eq!(SubagentRun::known(String::new(), String::new()), None);
    }

    /// 62 appends `SessionStops`: older peers stay in range and are never
    /// pushed it, and a note (and a clear) round-trips.
    #[test]
    fn protocol_62_stop_notes_round_trip_and_are_held_back_from_older_peers() {
        use crate::stop::{SessionStop, StopNote};
        const { assert!(LIMITS_PROTOCOL <= PROTOCOL_VERSION) };
        assert_eq!(MIN_COMPATIBLE_PROTOCOL, 53, "62 is additive");
        assert!(protocol_compatible_with(61));
        let stops = ServerEvent::SessionStops {
            stops: vec![
                SessionStop {
                    agent: AgentId("a".into()),
                    note: Some(StopNote {
                        reason: "rate_limit".into(),
                        waiting: true,
                        resumes: Some("3:45pm".into()),
                        resets_at: Some(1_791_891_379),
                        detail: Some("429 Too Many Requests".into()),
                        at: 9,
                    }),
                },
                SessionStop {
                    agent: AgentId("b".into()),
                    note: None,
                },
            ],
        };
        assert!(stops.decodable_by(LIMITS_PROTOCOL));
        assert!(
            !stops.decodable_by(LIMITS_PROTOCOL - 1),
            "an older peer would drop the connection"
        );
        let back: ServerEvent = rmp_serde::from_slice(&encode(&stops)).unwrap();
        let (ServerEvent::SessionStops { stops: got }, ServerEvent::SessionStops { stops: sent }) =
            (back, stops)
        else {
            panic!("not SessionStops");
        };
        assert_eq!(got, sent);
    }

    #[test]
    fn a_snapshot_round_trips_parents_and_subagents() {
        let snap = ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: vec![sample_agent(Some("caller"))],
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
            subagents: vec![sample_subagent()],
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&snap)).unwrap();
        let ServerEvent::Snapshot {
            agents, subagents, ..
        } = back
        else {
            panic!("not a snapshot");
        };
        assert_eq!(agents[0].parent, Some(AgentId("caller".into())));
        assert_eq!(subagents, vec![sample_subagent()]);

        let delta = ServerEvent::SubagentChanged {
            sub: SubagentSummary {
                ended_at: Some(20),
                status: SubagentStatus::Done,
                ..sample_subagent()
            },
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&delta)).unwrap();
        let ServerEvent::SubagentChanged { sub } = back else {
            panic!("not a SubagentChanged");
        };
        assert_eq!(sub.status, SubagentStatus::Done);
        assert_eq!(sub.ended_at, Some(20));
    }

    /// Protocol 52's wire shapes, field for field: what a peer one version
    /// back puts on the socket. The codec is positional (rmp arrays), so
    /// only the order and the count matter.
    mod v52 {
        use super::*;

        #[derive(Serialize)]
        pub struct Agent<'a> {
            pub id: &'a AgentId,
            pub worktree_id: &'a WorktreeId,
            pub name: &'a str,
            pub status: &'a AgentStatus,
            pub archived: bool,
            pub archived_at: i64,
            pub unseen: bool,
            pub status_changed_at: i64,
            pub kind: &'a AgentKind,
            pub custom_harness: &'a Option<String>,
            pub model: &'a Option<String>,
            pub effort: &'a Option<String>,
            pub session_id: &'a Option<String>,
            pub cloud_session_id: &'a Option<String>,
            pub issue_url: &'a Option<String>,
            pub role: &'a AgentRole,
            pub sort_order: i64,
            pub alive: bool,
            pub recent_prompts: &'a [crate::entities::PromptEntry],
            pub profile: &'a Option<String>,
        }

        impl<'a> From<&'a super::Agent> for Agent<'a> {
            fn from(a: &'a super::Agent) -> Self {
                Self {
                    id: &a.id,
                    worktree_id: &a.worktree_id,
                    name: &a.name,
                    status: &a.status,
                    archived: a.archived,
                    archived_at: a.archived_at,
                    unseen: a.unseen,
                    status_changed_at: a.status_changed_at,
                    kind: &a.kind,
                    custom_harness: &a.custom_harness,
                    model: &a.model,
                    effort: &a.effort,
                    session_id: &a.session_id,
                    cloud_session_id: &a.cloud_session_id,
                    issue_url: &a.issue_url,
                    role: &a.role,
                    sort_order: a.sort_order,
                    alive: a.alive,
                    recent_prompts: &a.recent_prompts,
                    profile: &a.profile,
                }
            }
        }

        /// The first variants of `ServerEvent`, in order, as 52 had them.
        #[derive(Serialize)]
        #[allow(dead_code)]
        pub enum ServerEvent<'a> {
            HelloOk {
                protocol_version: u32,
                daemon_pid: u32,
            },
            Incompatible {
                daemon_protocol_version: u32,
            },
            Snapshot {
                projects: Vec<Project>,
                worktrees: Vec<Worktree>,
                agents: Vec<Agent<'a>>,
                terminals: Vec<TerminalTab>,
                links: Vec<Link>,
                pr_seen: Vec<PrSeen>,
                ui_state: Option<String>,
            },
        }
    }

    /// The 52 shape still decodes here: no `subagents` and no agent
    /// `parent` read as empty and None. The handshake refuses 52 peers,
    /// but the defaults keep a stray or recorded old frame readable.
    #[test]
    fn a_snapshot_from_a_protocol_52_peer_decodes_without_the_new_fields() {
        let agent = sample_agent(None);
        let old = v52::ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: vec![v52::Agent::from(&agent)],
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: Some("{}".into()),
        };
        let back: ServerEvent = rmp_serde::from_slice(&encode(&old)).unwrap();
        let ServerEvent::Snapshot {
            agents,
            subagents,
            ui_state,
            ..
        } = back
        else {
            panic!("not a snapshot");
        };
        assert!(subagents.is_empty());
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].parent, None);
        assert_eq!(agents[0].name, "agent-2");
        assert_eq!(ui_state.as_deref(), Some("{}"));
    }
}
