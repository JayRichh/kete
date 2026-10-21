//! The desktop window's client of the Kete daemon.
//!
//! The wire format is nebula-core's: `codec` frames `ClientRequest` /
//! `ServerEvent` as length-prefixed MessagePack, and `paths` names the
//! endpoint (a named pipe on Windows, a unix socket elsewhere) exactly as
//! the TUI derives it, so `NEBULA_RUNTIME_DIR` isolates a test daemon here
//! too. One connection carries everything: the Subscribe stream that keeps
//! [`Model`] current, the PTY plane of every attached pane, and RPCs
//! answered by `Ack` / `Error` with a matching `req_id`.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::ports::WorktreePorts;
use nebula_core::{
    paths, Agent, ClientRequest, Entity, EntityId, Project, ReviewTabKind, ServerEvent, SessionRef,
    SubagentStatus, SubagentSummary, TerminalTab, Worktree, PROTOCOL_VERSION, SUBAGENTS_ENDED_KEPT,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};

/// How long a freshly spawned daemon gets to start listening.
const BOOT_WAIT: Duration = Duration::from_secs(5);
const POLL_STEP: Duration = Duration::from_millis(50);
/// How long an RPC waits for its Ack before the UI is told it failed.
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything the frontend draws, mirrored from the daemon's Snapshot and
/// the deltas after it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Model {
    pub connected: bool,
    /// Why there is no connection, in words the window can show.
    pub error: Option<String>,
    pub daemon_pid: u32,
    pub protocol_version: u32,
    pub runtime_dir: String,
    pub projects: Vec<Project>,
    pub worktrees: Vec<Worktree>,
    pub agents: Vec<Agent>,
    pub terminals: Vec<TerminalTab>,
    /// Claude's in-session subagents, open and recently ended; each
    /// hangs off the agent named by its `parent_agent`. Agents carry
    /// their own `parent` for sessions started with `kete spawn`.
    pub subagents: Vec<SubagentSummary>,
    pub ports: Vec<WorktreePorts>,
    /// Open pull request per worktree id, looked up with `gh`.
    pub prs: HashMap<String, PrInfo>,
    /// Every account's latest plan usage (`ServerEvent::AccountUsage`),
    /// keyed by account: a profile's name, or `@<harness>` for a CLI's
    /// own login.
    pub accounts: HashMap<String, nebula_core::usage::AccountUsage>,
    /// Each session's context window, keyed by agent id
    /// (`ServerEvent::SessionContexts`).
    pub contexts: HashMap<String, nebula_core::usage::ContextUsage>,
    /// Why each session's last turn stopped short (waiting out a usage
    /// limit, logged out, an API error), keyed by agent id
    /// (`ServerEvent::SessionStops`, a 62+ daemon).
    pub stops: HashMap<String, nebula_core::stop::StopNote>,
    /// Each session's lane and attention tier by agent id, archived ones
    /// too: nebula-core's rule, filled in by [`Client::snapshot`]
    /// ([`crate::board::fill`]), never by the window.
    pub cards: HashMap<String, crate::board::Card>,
    /// The board's cards, lane by lane, each in the order it shows them:
    /// those [`Model::scope`] admits.
    pub lanes: HashMap<nebula_core::board::Lane, Vec<String>>,
    /// How many more cards each lane would hold but the scope leaves out
    /// ("2 more outside scope"); every lane present, 0 when none.
    pub hidden: HashMap<nebula_core::board::Lane, usize>,
    /// What the window is looking at (`board_scope`): narrows `lanes`,
    /// and each card's `in_scope`. The window's to set; it survives a
    /// reconnect.
    pub scope: nebula_core::scope::Scope,
    /// Whether each worktree (by id) has work no push carries, as its last
    /// `worktree_git` read found it: the board's `has_changes`. Filled by
    /// those reads, never by a read of its own.
    #[serde(skip)]
    pub worktree_changes: HashMap<String, nebula_core::board::CardFacts>,
    /// The base each worktree Kete cut was cut from, by worktree id
    /// (`ServerEvent::WorktreeBases`, a 59+ daemon): what the diff and the
    /// board read against before deriving one.
    #[serde(skip)]
    pub bases: HashMap<String, nebula_core::review_base::RecordedBase>,
    /// Each role session's role card, by agent id (`ServerEvent::
    /// SessionRoles`, a 66+ daemon): replaced whole by each push.
    pub roles: HashMap<String, nebula_core::studio::wire::SessionRole>,
    /// Today's spend per session, by agent id (`ServerEvent::SessionSpend`,
    /// a 66+ daemon): folded in row by row.
    pub spend: HashMap<String, nebula_core::studio::wire::SpendRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub draft: bool,
    /// `open`, `merged` or `closed`.
    pub state: String,
    /// The head commit `gh` reports; empty where the source has none
    /// (Azure DevOps). Only the merged-worktree cleanup reads it.
    #[serde(skip)]
    pub head_sha: String,
}

/// What a pane hears about its session, on the Channel it attached with.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PtyEvent {
    /// The ring replayed on attach (base64); the pane resets first.
    Scrollback {
        data: String,
    },
    /// Live output (base64).
    Output {
        data: String,
    },
    Exited {
        code: Option<i32>,
    },
    Refused {
        message: String,
    },
    KittyFlags {
        flags: u8,
    },
}

type Pending = HashMap<u64, oneshot::Sender<Result<Option<EntityId>, String>>>;
/// `AskSession` and `ReadSession` waiters: answered by `SessionAnswer` or
/// `SessionText`, not `Ack`.
type Answers = HashMap<u64, oneshot::Sender<Result<String, String>>>;

/// `SessionChanges` requests waiting on their reply.
type Changes =
    HashMap<u64, oneshot::Sender<Result<nebula_core::session_snapshot::SessionChanges, String>>>;

/// 0.60 requests answered by an event of their own rather than `Ack`
/// (`StudioQueueState`, `UsageLedgerRows`, a renewal's `Scheduled`):
/// the whole reply, for the lane's command to read.
type Replies = HashMap<u64, oneshot::Sender<Result<ServerEvent, String>>>;

/// A renewal, as the window hears it (the Tauri event
/// `orchestrator-renewal`): `Scheduled` answering a request, then
/// `Done` or `Failed` for everyone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Renewal {
    pub old: String,
    pub new: Option<String>,
    pub state: nebula_core::studio::wire::RenewState,
}

/// Where a [`Renewal`] goes: the app emits it to the window.
pub type RenewalSink = Box<dyn Fn(Renewal) + Send + Sync>;

/// The orchestrator asked for a review (`kete orchestrator review`): the
/// daemon's `ReviewOpened` broadcast, as the window hears it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewOpened {
    /// The agent that asked (the orchestrator).
    pub opener: String,
    pub sessions: Vec<SessionRef>,
    /// `terminal`, `diff`, `history`, `pr`, in the order asked; empty: all.
    pub tabs: Vec<String>,
}

/// Where a `ReviewOpened` goes: the app emits it to the window.
pub type ReviewSink = Box<dyn Fn(ReviewOpened) + Send + Sync>;

/// Which way a session just turned, for the done / needs-you alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    /// Running or needs-feedback into finished: the DONE SOUND's edge.
    Done,
    /// Anything else into needs-feedback: the FEEDBACK SOUND's edge.
    NeedsYou,
}

/// The edge a status change makes, as the TUI's `handle_status_event`
/// reads it: a re-stamp of the same status, and every other move, is none.
pub fn turn(before: nebula_core::AgentStatus, after: nebula_core::AgentStatus) -> Option<Turn> {
    use nebula_core::AgentStatus::{Finished, NeedsFeedback, Running};
    match (before, after) {
        (Running | NeedsFeedback, Finished) => Some(Turn::Done),
        (b, NeedsFeedback) if b != NeedsFeedback => Some(Turn::NeedsYou),
        _ => None,
    }
}

/// What the daemon says that the window acts on outside the model: a
/// session turning (alerts) and `kete open` from inside a session.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Turned { agent: String, turn: Turn },
    FilesOpened { root: PathBuf, paths: Vec<PathBuf> },
}

/// Where notices go: the app's alerts and file viewer.
pub type NoticeSink = Box<dyn Fn(Notice) + Send + Sync>;

/// How long a `ReadSession` may take (a transcript read).
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the daemon may wait for an asked session to answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(120);

/// An RPC whose Ack did not come in time. The request may still be done
/// by the daemon, so a caller can look for its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcTimeout;

impl std::fmt::Display for RpcTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the daemon did not answer in time")
    }
}

impl std::error::Error for RpcTimeout {}

/// Shared between the Tauri commands and the connection task.
pub struct Client {
    pub model: Mutex<Model>,
    tx: Mutex<Option<mpsc::UnboundedSender<ClientRequest>>>,
    pending: Mutex<Pending>,
    answers: Mutex<Answers>,
    changes: Mutex<Changes>,
    replies: Mutex<Replies>,
    panes: Mutex<HashMap<SessionRef, Channel<PtyEvent>>>,
    reviews: Mutex<Option<ReviewSink>>,
    renewals: Mutex<Option<RenewalSink>>,
    notices: Mutex<Option<NoticeSink>>,
    /// Each checkout's base ref, kept between `worktree_git` polls.
    bases: Mutex<nebula_core::review_base::BaseRefs>,
    next_req: AtomicU64,
    /// Bumped on every model change; the emitter task watches it.
    pub changed: tokio::sync::Notify,
    /// Pinged to make the connection loop try again at once.
    pub retry: tokio::sync::Notify,
}

impl Client {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            model: Mutex::new(Model {
                runtime_dir: paths::runtime_dir().display().to_string(),
                ..Model::default()
            }),
            tx: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            answers: Mutex::new(HashMap::new()),
            changes: Mutex::new(HashMap::new()),
            replies: Mutex::new(HashMap::new()),
            panes: Mutex::new(HashMap::new()),
            reviews: Mutex::new(None),
            renewals: Mutex::new(None),
            notices: Mutex::new(None),
            bases: Mutex::new(Default::default()),
            next_req: AtomicU64::new(1),
            changed: tokio::sync::Notify::new(),
            retry: tokio::sync::Notify::new(),
        })
    }

    /// The model as the window draws it, the board worked out.
    pub fn snapshot(&self) -> Model {
        let mut model = self.model.lock().expect("model lock").clone();
        crate::board::fill(&mut model);
        model
    }

    fn touch(&self) {
        self.changed.notify_one();
    }

    /// Fire-and-forget request.
    pub fn send(&self, req: ClientRequest) -> Result<()> {
        let tx = self.tx.lock().expect("tx lock");
        let tx = tx
            .as_ref()
            .ok_or_else(|| anyhow!("not connected to the kete daemon"))?;
        tx.send(req)
            .map_err(|_| anyhow!("the daemon connection closed"))
    }

    pub fn req_id(&self) -> u64 {
        self.next_req.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a request carrying `req_id` and wait for its Ack.
    pub async fn rpc(&self, req_id: u64, req: ClientRequest) -> Result<Option<EntityId>> {
        self.rpc_within(req_id, req, RPC_TIMEOUT).await
    }

    /// [`rpc`](Self::rpc) with its own wait, for requests that can take
    /// long (a worktree of a big repo). Running out of time is an
    /// [`RpcTimeout`] error: the daemon may still finish the work.
    pub async fn rpc_within(
        &self,
        req_id: u64,
        req: ClientRequest,
        timeout: Duration,
    ) -> Result<Option<EntityId>> {
        let (done_tx, done_rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending lock")
            .insert(req_id, done_tx);
        if let Err(e) = self.send(req) {
            self.pending.lock().expect("pending lock").remove(&req_id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, done_rx).await {
            Ok(Ok(Ok(created))) => Ok(created),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the daemon connection closed before it answered"),
            Err(_) => {
                self.pending.lock().expect("pending lock").remove(&req_id);
                Err(RpcTimeout.into())
            }
        }
    }

    /// Ask a session a question (`kete ask`): the daemon waits for it to
    /// go idle, asks, and answers with the clean text of that turn.
    pub async fn ask(&self, agent: &str, question: &str) -> Result<String> {
        let req_id = self.req_id();
        let (done_tx, done_rx) = oneshot::channel();
        self.answers
            .lock()
            .expect("answers lock")
            .insert(req_id, done_tx);
        let req = ClientRequest::AskSession {
            req_id,
            caller: None,
            target: agent.to_string(),
            question: question.to_string(),
            timeout_ms: ASK_TIMEOUT.as_millis() as u64,
            wait: true,
        };
        if let Err(e) = self.send(req) {
            self.answers.lock().expect("answers lock").remove(&req_id);
            return Err(e);
        }
        // A little longer than the daemon's own wait, so its error wins.
        match tokio::time::timeout(ASK_TIMEOUT + Duration::from_secs(10), done_rx).await {
            Ok(Ok(Ok(answer))) => Ok(answer),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the daemon connection closed before it answered"),
            Err(_) => {
                self.answers.lock().expect("answers lock").remove(&req_id);
                bail!("the session did not answer in time")
            }
        }
    }

    /// A session's snapshots (`SessionChanges`), for the review's "this
    /// session" filter. None from a daemon older than
    /// `SESSION_CHANGES_PROTOCOL`, which would drop the connection on the
    /// request: it is never sent one.
    pub async fn session_changes(
        &self,
        agent: &str,
    ) -> Result<Option<nebula_core::session_snapshot::SessionChanges>> {
        if self.daemon_protocol() < nebula_core::SESSION_CHANGES_PROTOCOL {
            return Ok(None);
        }
        let req_id = self.req_id();
        let (done_tx, done_rx) = oneshot::channel();
        self.changes
            .lock()
            .expect("changes lock")
            .insert(req_id, done_tx);
        let req = ClientRequest::SessionChanges {
            req_id,
            agent: nebula_core::AgentId(agent.to_string()),
        };
        if let Err(e) = self.send(req) {
            self.changes.lock().expect("changes lock").remove(&req_id);
            return Err(e);
        }
        match tokio::time::timeout(RPC_TIMEOUT, done_rx).await {
            Ok(Ok(Ok(changes))) => Ok(Some(changes)),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the daemon connection closed before it answered"),
            Err(_) => {
                self.changes.lock().expect("changes lock").remove(&req_id);
                bail!("the daemon did not answer in time")
            }
        }
    }

    /// The base Kete recorded for worktree `id` when it cut it.
    pub fn recorded_base(&self, id: &str) -> Option<nebula_core::review_base::RecordedBase> {
        self.model
            .lock()
            .expect("model lock")
            .bases
            .get(id)
            .cloned()
    }

    /// A session's recent context (`ReadSession`): its transcript when the
    /// daemon knows it, else the cleaned terminal tail.
    pub async fn read_session(&self, target: &str, max_lines: u32) -> Result<String> {
        let req_id = self.req_id();
        let (done_tx, done_rx) = oneshot::channel();
        self.answers
            .lock()
            .expect("answers lock")
            .insert(req_id, done_tx);
        let req = ClientRequest::ReadSession {
            req_id,
            target: target.to_string(),
            max_lines,
        };
        if let Err(e) = self.send(req) {
            self.answers.lock().expect("answers lock").remove(&req_id);
            return Err(e);
        }
        match tokio::time::timeout(READ_TIMEOUT, done_rx).await {
            Ok(Ok(Ok(text))) => Ok(text),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the daemon connection closed before it answered"),
            Err(_) => {
                self.answers.lock().expect("answers lock").remove(&req_id);
                bail!("the daemon did not answer in time")
            }
        }
    }

    /// Hand every `ReviewOpened` to `sink` from now on.
    pub fn on_review(&self, sink: ReviewSink) {
        *self.reviews.lock().expect("reviews lock") = Some(sink);
    }

    /// Hand every orchestrator [`Renewal`] to `sink` from now on.
    pub fn on_renewal(&self, sink: RenewalSink) {
        *self.renewals.lock().expect("renewals lock") = Some(sink);
    }

    /// Send a 0.60 request carrying `req_id` and wait for the event that
    /// answers it (`StudioQueueState`, `UsageLedgerRows`, the renewal's
    /// `OrchestratorRenewal`); an `Error` for it is the error. Refused
    /// before sending to a daemon below 66, which would drop the
    /// connection on the request.
    // The queue, ledger and renew lanes' commands call it.
    #[allow(dead_code)]
    pub async fn rpc_reply(&self, req_id: u64, req: ClientRequest) -> Result<ServerEvent> {
        if self.daemon_protocol() < nebula_core::STUDIO_PROTOCOL {
            bail!("needs the 0.60 daemon: run `kete kill` and reopen");
        }
        let (done_tx, done_rx) = oneshot::channel();
        self.replies
            .lock()
            .expect("replies lock")
            .insert(req_id, done_tx);
        if let Err(e) = self.send(req) {
            self.replies.lock().expect("replies lock").remove(&req_id);
            return Err(e);
        }
        match tokio::time::timeout(RPC_TIMEOUT, done_rx).await {
            Ok(Ok(Ok(ev))) => Ok(ev),
            Ok(Ok(Err(message))) => Err(anyhow!(message)),
            Ok(Err(_)) => bail!("the daemon connection closed before it answered"),
            Err(_) => {
                self.replies.lock().expect("replies lock").remove(&req_id);
                Err(RpcTimeout.into())
            }
        }
    }

    /// Hand a reply event to the request waiting on `req_id`, if any.
    fn reply(&self, req_id: u64, ev: ServerEvent) -> bool {
        match self.replies.lock().expect("replies lock").remove(&req_id) {
            Some(w) => {
                let _ = w.send(Ok(ev));
                true
            }
            None => false,
        }
    }

    /// Hand every [`Notice`] to `sink` from now on.
    pub fn on_notice(&self, sink: NoticeSink) {
        *self.notices.lock().expect("notices lock") = Some(sink);
    }

    fn notice(&self, n: Notice) {
        if let Some(sink) = self.notices.lock().expect("notices lock").as_ref() {
            sink(n);
        }
    }

    /// The protocol the connected daemon speaks; 0 while not connected.
    pub fn daemon_protocol(&self) -> u32 {
        let m = self.model.lock().expect("model lock");
        if m.connected {
            m.protocol_version
        } else {
            0
        }
    }

    pub fn attach(
        &self,
        session: SessionRef,
        cols: u16,
        rows: u16,
        pane: Channel<PtyEvent>,
    ) -> Result<()> {
        self.panes
            .lock()
            .expect("panes lock")
            .insert(session.clone(), pane);
        self.send(ClientRequest::Attach {
            session,
            from_seq: None,
            cols,
            rows,
        })
    }

    pub fn detach(&self, session: SessionRef) -> Result<()> {
        self.panes.lock().expect("panes lock").remove(&session);
        self.send(ClientRequest::Detach { session })
    }

    /// A client "connected" to a test: every request it sends lands on the
    /// receiver, and [`Client::answer`] plays the daemon's reply.
    #[cfg(test)]
    pub fn connected_for_test() -> (Arc<Self>, mpsc::UnboundedReceiver<ClientRequest>) {
        let client = Self::new();
        let (tx, rx) = mpsc::unbounded_channel();
        *client.tx.lock().unwrap() = Some(tx);
        client.model.lock().unwrap().connected = true;
        (client, rx)
    }

    /// Feed the client one event as if the daemon sent it.
    #[cfg(test)]
    pub fn answer(&self, event: ServerEvent) {
        self.handle(event);
    }

    /// Replace the mirrored model (a test's daemon state).
    #[cfg(test)]
    pub fn set_model(&self, f: impl FnOnce(&mut Model)) {
        f(&mut self.model.lock().unwrap());
    }

    pub fn set_prs(&self, prs: HashMap<String, PrInfo>) {
        let mut model = self.model.lock().expect("model lock");
        if model.prs != prs {
            model.prs = prs;
            drop(model);
            self.touch();
        }
    }

    /// Narrow the board to `scope` (the default: everything).
    pub fn set_scope(&self, scope: nebula_core::scope::Scope) {
        let mut model = self.model.lock().expect("model lock");
        if model.scope != scope {
            model.scope = scope;
            drop(model);
            self.touch();
        }
    }

    /// Take in what a `worktree_git` read found, worktree by worktree
    /// (its board facts, the pull request aside); the rest keep what they
    /// had.
    pub fn set_worktree_changes(
        &self,
        changes: impl IntoIterator<Item = (String, nebula_core::board::CardFacts)>,
    ) {
        let mut model = self.model.lock().expect("model lock");
        let mut changed = false;
        for (id, facts) in changes {
            changed |= model.worktree_changes.insert(id, facts) != Some(facts);
        }
        drop(model);
        if changed {
            self.touch();
        }
    }

    /// The base refs `worktree_git` keeps between polls, to read from.
    pub fn base_refs(&self) -> nebula_core::review_base::BaseRefs {
        self.bases.lock().expect("bases lock").clone()
    }

    /// Keep what a `worktree_git` read resolved.
    pub fn keep_base_refs(&self, bases: nebula_core::review_base::BaseRefs) {
        *self.bases.lock().expect("bases lock") = bases;
    }

    fn pane(&self, session: &SessionRef) -> Option<Channel<PtyEvent>> {
        self.panes.lock().expect("panes lock").get(session).cloned()
    }

    fn set_disconnected(&self, error: Option<String>) {
        *self.tx.lock().expect("tx lock") = None;
        for (_, waiter) in self.pending.lock().expect("pending lock").drain() {
            let _ = waiter.send(Err("the daemon connection closed".into()));
        }
        for (_, waiter) in self.answers.lock().expect("answers lock").drain() {
            let _ = waiter.send(Err("the daemon connection closed".into()));
        }
        self.panes.lock().expect("panes lock").clear();
        let mut model = self.model.lock().expect("model lock");
        model.connected = false;
        model.error = error;
        drop(model);
        self.touch();
    }

    fn handle(&self, event: ServerEvent) {
        let b64 = |data: &[u8]| base64::engine::general_purpose::STANDARD.encode(data);
        match event {
            ServerEvent::Snapshot {
                projects,
                worktrees,
                agents,
                terminals,
                subagents,
                ..
            } => {
                let mut m = self.model.lock().expect("model lock");
                m.projects = projects;
                m.worktrees = worktrees;
                m.agents = agents;
                m.terminals = terminals;
                m.subagents = subagents;
                // A 59+ daemon sends its recorded bases after this; an
                // older one never does, and none of these may outlive it.
                m.bases.clear();
                // A 62+ daemon sends its stop notes after this too.
                m.stops.clear();
                drop(m);
                self.touch();
            }
            ServerEvent::EntityUpserted { entity } => {
                let mut m = self.model.lock().expect("model lock");
                match entity {
                    Entity::Project(p) => upsert(&mut m.projects, p, |x| x.id.0.clone()),
                    Entity::Worktree(w) => upsert(&mut m.worktrees, w, |x| x.id.0.clone()),
                    Entity::Agent(a) => upsert(&mut m.agents, a, |x| x.id.0.clone()),
                    Entity::Terminal(t) => upsert(&mut m.terminals, t, |x| x.id.0.clone()),
                    Entity::Link(_) => {}
                }
                drop(m);
                self.touch();
            }
            ServerEvent::EntityRemoved { id } => {
                let mut m = self.model.lock().expect("model lock");
                match id {
                    EntityId::Project(id) => m.projects.retain(|x| x.id != id),
                    EntityId::Worktree(id) => m.worktrees.retain(|x| x.id != id),
                    EntityId::Agent(id) => {
                        m.contexts.remove(&id.0);
                        m.stops.remove(&id.0);
                        m.subagents.retain(|s| s.parent_agent != id);
                        m.agents.retain(|x| x.id != id);
                    }
                    EntityId::Terminal(id) => m.terminals.retain(|x| x.id != id),
                    EntityId::Link(_) => {}
                }
                drop(m);
                self.touch();
            }
            ServerEvent::StatusChanged {
                agent,
                status,
                changed_at,
                unseen,
            } => {
                let mut m = self.model.lock().expect("model lock");
                let mut turned = None;
                if let Some(a) = m.agents.iter_mut().find(|a| a.id == agent) {
                    // Read before the write: the 50 ms state push can fold
                    // a flap, and a reconnect's Snapshot never alerts.
                    turned = turn(a.status, status);
                    a.status = status;
                    a.status_changed_at = changed_at;
                    a.unseen = unseen;
                }
                drop(m);
                self.touch();
                if let Some(turn) = turned {
                    self.notice(Notice::Turned {
                        agent: agent.0,
                        turn,
                    });
                }
            }
            // `kete open` in a session: the user asked to see these files.
            ServerEvent::FilesOpened { root, paths, .. } => {
                if !paths.is_empty() {
                    self.notice(Notice::FilesOpened { root, paths });
                }
            }
            ServerEvent::SubagentChanged { sub } => {
                let mut m = self.model.lock().expect("model lock");
                upsert_subagent(&mut m.subagents, sub);
                drop(m);
                self.touch();
            }
            // A clear, the daemon's retention sweep, or the session
            // restarting took these rows out of its book.
            ServerEvent::SubagentsRemoved { parent, ids } => {
                let mut m = self.model.lock().expect("model lock");
                m.subagents
                    .retain(|s| s.parent_agent != parent || !ids.contains(&s.id));
                drop(m);
                self.touch();
            }
            ServerEvent::WorktreePorts { ports } => {
                self.model.lock().expect("model lock").ports = ports;
                self.touch();
            }
            ServerEvent::WorktreeBases { bases } => {
                self.model.lock().expect("model lock").bases = bases
                    .into_iter()
                    .map(|b| (b.worktree.0.clone(), b))
                    .collect();
                // Kept board bases were derived: read them again.
                *self.bases.lock().expect("bases lock") = Default::default();
                self.touch();
            }
            ServerEvent::SessionChanges { req_id, changes } => {
                if let Some(w) = self.changes.lock().expect("changes lock").remove(&req_id) {
                    let _ = w.send(Ok(changes));
                }
            }
            ServerEvent::AccountUsage { accounts } => {
                self.model.lock().expect("model lock").accounts = accounts
                    .into_iter()
                    .map(|u| (u.account.clone(), u))
                    .collect();
                self.touch();
            }
            ServerEvent::SessionContexts { contexts } => {
                let mut m = self.model.lock().expect("model lock");
                for c in contexts {
                    m.contexts.insert(c.agent.0, c.context);
                }
                drop(m);
                self.touch();
            }
            ServerEvent::SessionStops { stops } => {
                let mut m = self.model.lock().expect("model lock");
                for s in stops {
                    match s.note {
                        Some(note) => m.stops.insert(s.agent.0, note),
                        None => m.stops.remove(&s.agent.0),
                    };
                }
                drop(m);
                self.touch();
            }
            ServerEvent::Ack { req_id, created } => {
                if let Some(w) = self.pending.lock().expect("pending lock").remove(&req_id) {
                    let _ = w.send(Ok(created));
                }
            }
            ServerEvent::Error {
                req_id: Some(req_id),
                message,
            } => {
                if let Some(w) = self.pending.lock().expect("pending lock").remove(&req_id) {
                    let _ = w.send(Err(message));
                } else if let Some(w) = self.answers.lock().expect("answers lock").remove(&req_id) {
                    let _ = w.send(Err(message));
                } else if let Some(w) = self.changes.lock().expect("changes lock").remove(&req_id) {
                    let _ = w.send(Err(message));
                } else if let Some(w) = self.replies.lock().expect("replies lock").remove(&req_id) {
                    let _ = w.send(Err(message));
                }
            }
            // 0.60 (a 66+ daemon): the role sessions, replaced whole.
            ServerEvent::SessionRoles { roles } => {
                self.model.lock().expect("model lock").roles =
                    roles.into_iter().map(|r| (r.agent.0.clone(), r)).collect();
                self.touch();
            }
            // Today's spend, one session at a time.
            ServerEvent::SessionSpend { rows } => {
                let mut m = self.model.lock().expect("model lock");
                for row in rows {
                    m.spend.insert(row.agent.0.clone(), row);
                }
                drop(m);
                self.touch();
            }
            ev @ ServerEvent::StudioQueueState { .. }
            | ev @ ServerEvent::UsageLedgerRows { .. } => {
                let req_id = match &ev {
                    ServerEvent::StudioQueueState { req_id, .. }
                    | ServerEvent::UsageLedgerRows { req_id, .. } => *req_id,
                    _ => unreachable!("matched above"),
                };
                self.reply(req_id, ev);
            }
            ServerEvent::OrchestratorRenewal {
                req_id,
                old,
                new,
                state,
            } => {
                let renewal = Renewal {
                    old: old.0.clone(),
                    new: new.as_ref().map(|n| n.0.clone()),
                    state: state.clone(),
                };
                if let Some(req_id) = req_id {
                    self.reply(
                        req_id,
                        ServerEvent::OrchestratorRenewal {
                            req_id: Some(req_id),
                            old,
                            new,
                            state,
                        },
                    );
                }
                if let Some(sink) = self.renewals.lock().expect("renewals lock").as_ref() {
                    sink(renewal);
                }
            }
            ServerEvent::SessionAnswer { req_id, answer, .. } => {
                if let Some(w) = self.answers.lock().expect("answers lock").remove(&req_id) {
                    let _ = w.send(Ok(answer));
                }
            }
            ServerEvent::SessionText { req_id, text, .. } => {
                if let Some(w) = self.answers.lock().expect("answers lock").remove(&req_id) {
                    let _ = w.send(Ok(text));
                }
            }
            ServerEvent::ReviewOpened {
                opener,
                sessions,
                tabs,
            } => {
                let review = ReviewOpened {
                    opener: opener.0,
                    sessions,
                    tabs: tabs.iter().map(|t| tab_word(*t).to_string()).collect(),
                };
                if let Some(sink) = self.reviews.lock().expect("reviews lock").as_ref() {
                    sink(review);
                }
            }
            ServerEvent::Scrollback { session, data, .. } => {
                if let Some(p) = self.pane(&session) {
                    let _ = p.send(PtyEvent::Scrollback { data: b64(&data) });
                }
            }
            ServerEvent::Output { session, data, .. } => {
                if let Some(p) = self.pane(&session) {
                    let _ = p.send(PtyEvent::Output { data: b64(&data) });
                }
            }
            ServerEvent::SessionExited { session, exit_code } => {
                if let Some(p) = self.pane(&session) {
                    let _ = p.send(PtyEvent::Exited { code: exit_code });
                }
            }
            ServerEvent::AttachRefused { session, message } => {
                if let Some(p) = self.pane(&session) {
                    let _ = p.send(PtyEvent::Refused { message });
                }
            }
            ServerEvent::KittyFlags { session, flags } => {
                if let Some(p) = self.pane(&session) {
                    let _ = p.send(PtyEvent::KittyFlags { flags });
                }
            }
            _ => {}
        }
    }
}

/// A review tab as the window names it.
fn tab_word(t: ReviewTabKind) -> &'static str {
    match t {
        ReviewTabKind::Terminal => "terminal",
        ReviewTabKind::Diff => "diff",
        ReviewTabKind::History => "history",
        ReviewTabKind::PullRequest => "pr",
    }
}

/// A subagent is keyed by its parent and its id: ids are unique only
/// within one session. Each parent keeps at most
/// [`SUBAGENTS_ENDED_KEPT`] ended rows, the oldest going first, as the
/// daemon's book does, so a long session's tree stays bounded here too.
fn upsert_subagent(list: &mut Vec<SubagentSummary>, sub: SubagentSummary) {
    let parent = sub.parent_agent.clone();
    match list
        .iter_mut()
        .find(|s| s.parent_agent == sub.parent_agent && s.id == sub.id)
    {
        Some(slot) => *slot = sub,
        None => list.push(sub),
    }
    let ended =
        |s: &SubagentSummary| s.parent_agent == parent && s.status != SubagentStatus::Running;
    let mut drop = list
        .iter()
        .filter(|s| ended(s))
        .count()
        .saturating_sub(SUBAGENTS_ENDED_KEPT);
    list.retain(|s| {
        if drop > 0 && ended(s) {
            drop -= 1;
            return false;
        }
        true
    });
}

fn upsert<T>(list: &mut Vec<T>, item: T, key: impl Fn(&T) -> String) {
    let k = key(&item);
    match list.iter_mut().find(|x| key(x) == k) {
        Some(slot) => *slot = item,
        None => list.push(item),
    }
}

// ------------------------------------------------------------- connection

trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
type Stream = Box<dyn Duplex>;

/// Connect to the daemon's endpoint for this runtime dir.
#[cfg(windows)]
async fn connect() -> std::io::Result<Stream> {
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    let name = paths::pipe_name_for_socket(&paths::socket_path());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match ClientOptions::new().open(&name) {
            Ok(client) => return Ok(Box::new(client)),
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
async fn connect() -> std::io::Result<Stream> {
    Ok(Box::new(
        tokio::net::UnixStream::connect(paths::socket_path()).await?,
    ))
}

/// The `kete` binary that runs the daemon: `KETE_BIN` (or the older
/// `BRO_BIN`), else a `kete` beside this exe, else one on PATH; `bro`, the
/// old name of the same program, after each `kete`.
pub fn find_kete() -> Option<PathBuf> {
    for var in ["KETE_BIN", "BRO_BIN"] {
        if let Some(p) = std::env::var_os(var).map(PathBuf::from) {
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["kete.exe", "bro.exe"]
    } else {
        &["kete", "bro"]
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    beside
        .into_iter()
        .chain(on_path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|p| p.is_file())
}

/// Start `kete daemon` detached, the way the TUI does: no console, its own
/// process group, out of this process's job when the job allows it.
#[cfg(windows)]
fn spawn_daemon(kete: &std::path::Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
    };
    let flags = CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW;
    let spawn = |flags: u32| {
        std::process::Command::new(kete)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    spawn(flags | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| spawn(flags))
        .context("start the kete daemon")?;
    Ok(())
}

#[cfg(unix)]
fn spawn_daemon(kete: &std::path::Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(kete);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // A session of its own, as the TUI's spawn: no controlling terminal.
    unsafe {
        cmd.pre_exec(|| {
            extern "C" {
                fn setsid() -> i32;
            }
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn().context("start the kete daemon")?;
    Ok(())
}

async fn connect_or_spawn(spawn: bool) -> Result<Stream> {
    if let Ok(s) = connect().await {
        return Ok(s);
    }
    if !spawn {
        bail!("the kete daemon is not running");
    }
    let kete = find_kete().ok_or_else(|| {
        anyhow!("no kete daemon is running, and no kete binary was found beside this app or on PATH to start one")
    })?;
    spawn_daemon(&kete)?;
    let deadline = tokio::time::Instant::now() + BOOT_WAIT;
    loop {
        match connect().await {
            Ok(s) => return Ok(s),
            Err(_) if tokio::time::Instant::now() < deadline => tokio::time::sleep(POLL_STEP).await,
            Err(e) => {
                return Err(anyhow!(e)).with_context(|| {
                    format!(
                        "the daemon did not come up; check {}",
                        paths::daemon_log_path().display()
                    )
                })
            }
        }
    }
}

async fn handshake(stream: &mut Stream) -> Result<(u32, u32)> {
    write_frame(
        stream,
        &ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;
    match read_frame::<ServerEvent, _>(stream).await? {
        Some(ServerEvent::HelloOk {
            protocol_version,
            daemon_pid,
        }) if nebula_core::protocol_compatible_with(protocol_version) => {
            Ok((protocol_version, daemon_pid))
        }
        Some(ServerEvent::HelloOk {
            protocol_version, ..
        })
        | Some(ServerEvent::Incompatible {
            daemon_protocol_version: protocol_version,
        }) => bail!(
            "the running daemon speaks protocol {protocol_version}, this app speaks {PROTOCOL_VERSION}; \
             run `kete kill` and reopen, or update the older side"
        ),
        other => bail!("unexpected handshake reply: {other:?}"),
    }
}

/// One connection's life: handshake, Subscribe, then pump events until the
/// daemon hangs up.
async fn session(client: &Arc<Client>, spawn: bool) -> Result<()> {
    let mut stream = connect_or_spawn(spawn).await?;
    let (version, pid) = handshake(&mut stream).await?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::unbounded_channel::<ClientRequest>();
    tx.send(ClientRequest::Subscribe).ok();
    *client.tx.lock().expect("tx lock") = Some(tx);
    {
        let mut m = client.model.lock().expect("model lock");
        m.connected = true;
        m.error = None;
        m.daemon_pid = pid;
        m.protocol_version = version;
    }
    client.touch();

    let writer_task = tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            if write_frame(&mut writer, &req).await.is_err() {
                break;
            }
        }
    });
    let result = loop {
        match read_frame::<ServerEvent, _>(&mut reader).await {
            Ok(Some(event)) => client.handle(event),
            Ok(None) => break Ok(()),
            Err(e) => break Err(anyhow!(e)),
        }
    };
    writer_task.abort();
    result
}

/// Keep a connection up for the life of the app. The daemon is started
/// only on the first try (and when the user asks): a daemon someone
/// stopped with `kete kill` stays stopped until then.
pub async fn run(client: Arc<Client>) {
    let mut spawn = true;
    loop {
        let outcome = session(&client, spawn).await;
        let message = match outcome {
            Ok(()) => "the daemon closed the connection".to_string(),
            Err(e) => format!("{e:#}"),
        };
        client.set_disconnected(Some(message));
        spawn = false;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = client.retry.notified() => { spawn = true; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{AgentId, AgentKind, AgentRole, AgentStatus, WorktreeId};

    #[test]
    fn a_session_answer_or_error_reaches_its_asker() {
        let client = Client::new();
        let (tx, mut rx) = oneshot::channel();
        client.answers.lock().unwrap().insert(7, tx);
        client.handle(ServerEvent::SessionAnswer {
            req_id: 7,
            session: AgentId("a".into()),
            answer: "It adds a GST rounding fix.".into(),
        });
        assert_eq!(
            rx.try_recv().unwrap(),
            Ok("It adds a GST rounding fix.".into())
        );

        let (tx, mut rx) = oneshot::channel();
        client.answers.lock().unwrap().insert(8, tx);
        client.handle(ServerEvent::Error {
            req_id: Some(8),
            message: "no such session".into(),
        });
        assert_eq!(rx.try_recv().unwrap(), Err("no such session".into()));
    }

    #[tokio::test]
    async fn read_session_sends_read_session_and_takes_the_session_text() {
        let (client, mut rx) = Client::connected_for_test();
        let c = client.clone();
        let call = tokio::spawn(async move { c.read_session("fix-login", 80).await });
        let req = rx.recv().await.unwrap();
        let ClientRequest::ReadSession {
            req_id,
            target,
            max_lines,
        } = req
        else {
            panic!("not a ReadSession: {req:?}");
        };
        assert_eq!((target.as_str(), max_lines), ("fix-login", 80));
        client.handle(ServerEvent::SessionText {
            req_id,
            session: SessionRef::Agent(AgentId("a".into())),
            text: "## user\nfix the login".into(),
        });
        assert_eq!(call.await.unwrap().unwrap(), "## user\nfix the login");
    }

    #[test]
    fn review_opened_reaches_the_sink_with_tab_words() {
        let client = Client::new();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        client.on_review(Box::new(move |r| g.lock().unwrap().push(r)));
        client.handle(ServerEvent::ReviewOpened {
            opener: AgentId("orch".into()),
            sessions: vec![SessionRef::Agent(AgentId("a".into()))],
            tabs: vec![ReviewTabKind::Diff, ReviewTabKind::PullRequest],
        });
        assert_eq!(
            got.lock().unwrap().as_slice(),
            &[ReviewOpened {
                opener: "orch".into(),
                sessions: vec![SessionRef::Agent(AgentId("a".into()))],
                tabs: vec!["diff".into(), "pr".into()],
            }]
        );
    }

    /// The TUI's edges: into finished from running or needs-feedback is
    /// done, into needs-feedback from anything else is needs-you, and a
    /// re-stamp or any other move is nothing.
    #[test]
    fn turns_are_the_tuis_edges() {
        use AgentStatus::*;
        assert_eq!(turn(Running, Finished), Some(Turn::Done));
        assert_eq!(turn(NeedsFeedback, Finished), Some(Turn::Done));
        assert_eq!(turn(Fresh, Finished), None, "never ran a turn here");
        assert_eq!(turn(Finished, Finished), None, "a re-stamp");
        assert_eq!(turn(Running, NeedsFeedback), Some(Turn::NeedsYou));
        assert_eq!(turn(Finished, NeedsFeedback), Some(Turn::NeedsYou));
        assert_eq!(turn(NeedsFeedback, NeedsFeedback), None);
        assert_eq!(turn(NeedsFeedback, Running), None);
    }

    /// Status changes reach the notice sink as turns, read against the
    /// mirrored status before it is overwritten; a Snapshot never does,
    /// and `kete open` arrives as the files with their checkout.
    #[test]
    fn turns_and_opened_files_reach_the_notice_sink() {
        let client = Client::new();
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        client.on_notice(Box::new(move |n| g.lock().unwrap().push(n)));
        client.handle(ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: vec![agent("a", None)],
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
            subagents: Vec::new(),
        });
        let status = |s: AgentStatus| ServerEvent::StatusChanged {
            agent: AgentId("a".into()),
            status: s,
            changed_at: 1,
            unseen: false,
        };
        client.handle(status(AgentStatus::NeedsFeedback));
        client.handle(status(AgentStatus::NeedsFeedback));
        client.handle(status(AgentStatus::Finished));
        client.handle(ServerEvent::StatusChanged {
            agent: AgentId("gone".into()),
            status: AgentStatus::NeedsFeedback,
            changed_at: 1,
            unseen: false,
        });
        client.handle(ServerEvent::FilesOpened {
            agent: AgentId("a".into()),
            root: PathBuf::from("/w"),
            paths: vec![PathBuf::from("/w/README.md")],
        });
        client.handle(ServerEvent::FilesOpened {
            agent: AgentId("a".into()),
            root: PathBuf::from("/w"),
            paths: Vec::new(),
        });
        assert_eq!(
            got.lock().unwrap().as_slice(),
            &[
                Notice::Turned {
                    agent: "a".into(),
                    turn: Turn::NeedsYou
                },
                Notice::Turned {
                    agent: "a".into(),
                    turn: Turn::Done
                },
                Notice::FilesOpened {
                    root: PathBuf::from("/w"),
                    paths: vec![PathBuf::from("/w/README.md")],
                },
            ]
        );
    }

    #[test]
    fn daemon_protocol_is_zero_until_connected() {
        let client = Client::new();
        client.model.lock().unwrap().protocol_version = 54;
        assert_eq!(client.daemon_protocol(), 0);
        client.model.lock().unwrap().connected = true;
        assert_eq!(client.daemon_protocol(), 54);
    }

    fn agent(id: &str, parent: Option<&str>) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status: AgentStatus::Running,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
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

    fn sub(parent: &str, id: &str, status: SubagentStatus) -> SubagentSummary {
        SubagentSummary {
            id: id.into(),
            parent_agent: AgentId(parent.into()),
            kind: "Explore".into(),
            description: "Read the docs".into(),
            started_at: 1,
            ended_at: (status != SubagentStatus::Running).then_some(2),
            status,
            run: None,
        }
    }

    /// The overview's tree reaches the frontend: agents carry `parent`,
    /// and `subagents` follows the snapshot, its deltas, and row deletes.
    #[test]
    fn parents_and_subagents_reach_the_frontend_model() {
        let client = Client::new();
        client.handle(ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: vec![agent("lead", None), agent("child", Some("lead"))],
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
            subagents: vec![sub("child", "s1", SubagentStatus::Running)],
        });
        client.handle(ServerEvent::SubagentChanged {
            sub: SubagentSummary {
                run: nebula_core::SubagentRun::known("haiku".into(), "low".into()),
                ..sub("child", "s1", SubagentStatus::Done)
            },
        });
        client.handle(ServerEvent::SubagentChanged {
            sub: sub("lead", "s1", SubagentStatus::Running),
        });

        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["agents"][0]["parent"], serde_json::Value::Null);
        assert_eq!(json["agents"][1]["parent"], "lead");
        let subs = json["subagents"].as_array().unwrap();
        assert_eq!(subs.len(), 2, "same id under another parent is its own row");
        assert_eq!(subs[0]["parent_agent"], "child");
        assert_eq!(subs[0]["status"], "done");
        assert_eq!(subs[0]["ended_at"], 2);
        assert_eq!(subs[1]["status"], "running");
        // The model and effort, when known; no `run` at all when not.
        assert_eq!(subs[0]["run"]["model"], "haiku");
        assert_eq!(subs[0]["run"]["effort"], "low");
        assert!(subs[1].get("run").is_none());

        client.handle(ServerEvent::EntityRemoved {
            id: EntityId::Agent(AgentId("child".into())),
        });
        let model = client.snapshot();
        assert_eq!(model.subagents.len(), 1);
        assert_eq!(model.subagents[0].parent_agent, AgentId("lead".into()));
    }

    /// `SubagentsRemoved` takes exactly the named rows of that parent out
    /// of the model: the same id under another session stays, and so do
    /// the parent's other rows.
    #[test]
    fn removed_subagents_leave_the_frontend_model() {
        let client = Client::new();
        for s in [
            sub("a", "x", SubagentStatus::Done),
            sub("a", "y", SubagentStatus::Running),
            sub("b", "x", SubagentStatus::Done),
        ] {
            client.handle(ServerEvent::SubagentChanged { sub: s });
        }
        client.handle(ServerEvent::SubagentsRemoved {
            parent: AgentId("a".into()),
            ids: vec!["x".into(), "gone-already".into()],
        });
        let left: Vec<(String, String)> = client
            .snapshot()
            .subagents
            .iter()
            .map(|s| (s.parent_agent.0.clone(), s.id.clone()))
            .collect();
        assert_eq!(
            left,
            vec![("a".into(), "y".into()), ("b".into(), "x".into())]
        );
    }

    /// Usage events reach the frontend model: the account list replaces
    /// the last, contexts fold in by agent and go with their row.
    #[test]
    fn account_usage_and_contexts_reach_the_frontend_model() {
        use nebula_core::usage::{
            AccountUsage, ContextUsage, SessionContext, UsageSource, UsageWindow, WindowKind,
        };
        let client = Client::new();
        client.handle(ServerEvent::AccountUsage {
            accounts: vec![AccountUsage {
                account: "@claude".into(),
                harness: "claude".into(),
                windows: vec![UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent: 34.0,
                    resets_at: Some(10),
                    read_at: 0,
                    estimated: false,
                }],
                source: UsageSource::Statusline,
                at: 1,
                plan: None,
            }],
        });
        let ctx = |p: f32| ContextUsage {
            percent: Some(p),
            tokens: None,
            window: Some(200_000),
            compactions: 2,
            last_compact_at: None,
            source: UsageSource::Statusline,
            at: 1,
        };
        client.handle(ServerEvent::SessionContexts {
            contexts: vec![
                SessionContext {
                    agent: AgentId("a".into()),
                    context: ctx(10.0),
                },
                SessionContext {
                    agent: AgentId("b".into()),
                    context: ctx(20.0),
                },
            ],
        });
        client.handle(ServerEvent::SessionContexts {
            contexts: vec![SessionContext {
                agent: AgentId("a".into()),
                context: ctx(96.0),
            }],
        });
        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["accounts"]["@claude"]["windows"][0]["kind"], "session");
        assert_eq!(json["accounts"]["@claude"]["source"], "statusline");
        assert_eq!(json["contexts"]["a"]["percent"], 96.0);
        assert_eq!(json["contexts"]["b"]["compactions"], 2);
        client.handle(ServerEvent::EntityRemoved {
            id: EntityId::Agent(AgentId("b".into())),
        });
        assert!(!client.snapshot().contexts.contains_key("b"));
    }

    /// Stop notes fold in by agent (`None` clears one), reach the frontend
    /// in the model's snake_case spelling, and go with a Snapshot or their
    /// row: the daemon resends the list after every Snapshot.
    #[test]
    fn stop_notes_reach_the_frontend_model() {
        use nebula_core::stop::{SessionStop, StopNote};
        let note = |reason: &str, waiting: bool| StopNote {
            reason: reason.into(),
            waiting,
            resumes: waiting.then(|| "3:45pm".to_string()),
            resets_at: Some(1_800_000_000),
            detail: None,
            at: 1_700_000_000_000,
        };
        let stop = |agent: &str, note: Option<StopNote>| SessionStop {
            agent: AgentId(agent.into()),
            note,
        };
        let client = Client::new();
        client.handle(ServerEvent::SessionStops {
            stops: vec![
                stop("a", Some(note("rate_limit", true))),
                stop("b", Some(note("authentication_failed", false))),
            ],
        });
        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["stops"]["a"]["reason"], "rate_limit");
        assert_eq!(json["stops"]["a"]["waiting"], true);
        assert_eq!(json["stops"]["a"]["resumes"], "3:45pm");
        assert_eq!(json["stops"]["a"]["resets_at"], 1_800_000_000);
        assert!(json["stops"]["a"]["detail"].is_null());
        assert_eq!(json["stops"]["a"]["at"], 1_700_000_000_000_i64);
        assert_eq!(json["stops"]["b"]["reason"], "authentication_failed");

        // A later note for the same session replaces it; None clears it,
        // and clearing a session with no note is harmless.
        client.handle(ServerEvent::SessionStops {
            stops: vec![
                stop("a", Some(note("overloaded", false))),
                stop("b", None),
                stop("never", None),
            ],
        });
        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["stops"]["a"]["reason"], "overloaded");
        assert!(json["stops"].get("b").is_none());

        // The row going takes its note.
        client.handle(ServerEvent::SessionStops {
            stops: vec![stop("c", Some(note("billing_error", false)))],
        });
        client.handle(ServerEvent::EntityRemoved {
            id: EntityId::Agent(AgentId("c".into())),
        });
        assert!(!client.snapshot().stops.contains_key("c"));
        assert!(client.snapshot().stops.contains_key("a"));

        // A Snapshot clears them all; the daemon sends the list again.
        client.handle(ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: Vec::new(),
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
            subagents: Vec::new(),
        });
        assert!(client.snapshot().stops.is_empty());
    }

    /// Mirrors the daemon's prune: per parent, the newest
    /// `SUBAGENTS_ENDED_KEPT` ended rows stay, running ones always do, and
    /// another parent's rows are never counted against it.
    #[test]
    fn a_long_session_keeps_only_its_newest_ended_subagents() {
        let mut list = Vec::new();
        upsert_subagent(&mut list, sub("p", "open", SubagentStatus::Running));
        upsert_subagent(&mut list, sub("q", "other", SubagentStatus::Done));
        for i in 0..SUBAGENTS_ENDED_KEPT + 3 {
            upsert_subagent(&mut list, sub("p", &format!("s{i}"), SubagentStatus::Done));
        }
        let of_p = list.iter().filter(|s| s.parent_agent.0 == "p").count();
        assert_eq!(of_p, SUBAGENTS_ENDED_KEPT + 1);
        assert!(list.iter().any(|s| s.id == "open"));
        assert!(list.iter().any(|s| s.id == "other"));
        assert!(!list.iter().any(|s| s.id == "s2"), "the oldest ended go");
        assert!(list.iter().any(|s| s.id == "s3"));
    }

    /// `SessionChanges` goes only to a 59+ daemon (an older one would drop
    /// the connection); its reply, or an Error, reaches the asker.
    #[tokio::test]
    async fn session_changes_are_asked_only_of_a_daemon_that_answers() {
        use nebula_core::session_snapshot::SessionChanges;
        let (client, mut rx) = Client::connected_for_test();
        client.set_model(|m| m.protocol_version = nebula_core::SESSION_CHANGES_PROTOCOL - 1);
        assert_eq!(client.session_changes("a1").await.unwrap(), None);
        assert!(rx.try_recv().is_err(), "nothing went out");

        client.set_model(|m| m.protocol_version = nebula_core::SESSION_CHANGES_PROTOCOL);
        let asking = {
            let client = client.clone();
            tokio::spawn(async move { client.session_changes("a1").await })
        };
        let Some(ClientRequest::SessionChanges { req_id, agent }) = rx.recv().await else {
            panic!("a SessionChanges request");
        };
        assert_eq!(agent.0, "a1");
        let changes = SessionChanges {
            agent,
            worktree: Some(WorktreeId("w".into())),
            start: None,
            last: None,
            sessions: 3,
        };
        client.answer(ServerEvent::SessionChanges {
            req_id,
            changes: changes.clone(),
        });
        assert_eq!(asking.await.unwrap().unwrap(), Some(changes));

        let asking = {
            let client = client.clone();
            tokio::spawn(async move { client.session_changes("gone").await })
        };
        let Some(ClientRequest::SessionChanges { req_id, .. }) = rx.recv().await else {
            panic!("a SessionChanges request");
        };
        client.answer(ServerEvent::Error {
            req_id: Some(req_id),
            message: "session not found".into(),
        });
        let err = asking.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    /// The recorded bases land by worktree, and a Snapshot (a reconnect,
    /// maybe to an older daemon) clears them until the next list.
    #[test]
    fn recorded_bases_are_kept_by_worktree_until_the_next_snapshot() {
        let client = Client::new();
        let base = nebula_core::review_base::RecordedBase {
            worktree: WorktreeId("w".into()),
            refname: "origin/main".into(),
            sha: "abc".into(),
        };
        client.handle(ServerEvent::WorktreeBases {
            bases: vec![base.clone()],
        });
        assert_eq!(client.recorded_base("w"), Some(base));
        assert_eq!(client.recorded_base("x"), None);
        client.handle(ServerEvent::Snapshot {
            projects: Vec::new(),
            worktrees: Vec::new(),
            agents: Vec::new(),
            terminals: Vec::new(),
            links: Vec::new(),
            pr_seen: Vec::new(),
            ui_state: None,
            subagents: Vec::new(),
        });
        assert_eq!(client.recorded_base("w"), None);
    }

    /// 0.60: a 66 daemon's role sessions land in the model replaced whole,
    /// its spend rows upserted, both by agent id; a renewal reaches the
    /// window's sink.
    #[test]
    fn roles_spend_and_renewals_reach_the_window() {
        use nebula_core::studio::wire::{RenewState, SessionRole, SpendRow};
        let role = |agent: &str, card: &str| SessionRole {
            agent: AgentId(agent.into()),
            card: card.into(),
            label: card.into(),
            tier: 3,
            department: None,
            permission: "allowlist".into(),
            context_budget: None,
        };
        let spend = |agent: &str, turns: u32| SpendRow {
            agent: AgentId(agent.into()),
            day: "2026-10-21".into(),
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_usd: 0.1,
            tool_uses: 1,
            turns,
        };
        let client = Client::new();
        client.handle(ServerEvent::SessionRoles {
            roles: vec![role("a", "runner"), role("b", "triage")],
        });
        client.handle(ServerEvent::SessionRoles {
            roles: vec![role("a", "lead")],
        });
        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["roles"]["a"]["card"], "lead");
        assert!(json["roles"].get("b").is_none(), "replaced whole");
        client.handle(ServerEvent::SessionSpend {
            rows: vec![spend("a", 1), spend("b", 3)],
        });
        client.handle(ServerEvent::SessionSpend {
            rows: vec![spend("a", 2)],
        });
        let json = serde_json::to_value(client.snapshot()).unwrap();
        assert_eq!(json["spend"]["a"]["turns"], 2);
        assert_eq!(json["spend"]["b"]["turns"], 3);

        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        client.on_renewal(Box::new(move |r| h.lock().unwrap().push(r)));
        client.handle(ServerEvent::OrchestratorRenewal {
            req_id: None,
            old: AgentId("o".into()),
            new: Some(AgentId("n".into())),
            state: RenewState::Done,
        });
        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 1);
        assert_eq!(
            (heard[0].old.as_str(), heard[0].new.as_deref()),
            ("o", Some("n"))
        );
        assert_eq!(heard[0].state, RenewState::Done);
        let wire = serde_json::to_value(&heard[0]).unwrap();
        assert_eq!(wire["state"], "Done");
    }
}
