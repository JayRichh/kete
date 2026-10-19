//! kete-desktop: Kete's native window over the same daemon the TUI drives.
//! The Rust side owns the daemon connection (`daemon`) and the git reads
//! (`git`); the web view draws the kanban, the live terminals and the
//! diffs, and talks to this side through the commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod ado;
mod alerts;
mod board;
mod branches;
mod critters;
mod daemon;
mod editors;
mod files;
mod fonts;
mod git;
mod launch;
mod learn;
mod look;
mod orchestrator;
mod order;
mod pr_poll;
mod presets;
mod profiles;
mod projects;
#[cfg(windows)]
mod quiet;
mod sessions;
mod settings;
mod settings_io;
mod ship;
mod subagents;
mod tree;
mod update;
mod wake;

use ado::client::CmdError;
use ado::prs::{self, PrCtx, PrCtxExt};
use ado::{AdoState, AdoStateExt};
use daemon::{Client, Model, PtyEvent};
use nebula_core::{AgentId, ClientRequest, EntityId, SessionRef, WorktreeId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

pub(crate) type Res<T> = Result<T, String>;
/// An ADO command's result: errors carry a kind for the window.
type AdoRes<T> = Result<T, CmdError>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn get_state(client: State<'_, Arc<Client>>) -> Model {
    client.snapshot()
}

#[tauri::command]
fn retry_connect(client: State<'_, Arc<Client>>) {
    client.retry.notify_one();
}

#[tauri::command]
fn attach(
    client: State<'_, Arc<Client>>,
    session: SessionRef,
    cols: u16,
    rows: u16,
    on_event: Channel<PtyEvent>,
) -> Res<()> {
    client
        .attach(session, cols.max(2), rows.max(2), on_event)
        .map_err(err)
}

#[tauri::command]
fn detach(client: State<'_, Arc<Client>>, session: SessionRef) -> Res<()> {
    client.detach(session).map_err(err)
}

/// Keystrokes from the pane. `binary` input arrives as a string of
/// byte-valued code units (xterm.js's `onBinary`); anything else is UTF-8.
#[tauri::command]
fn input(
    client: State<'_, Arc<Client>>,
    session: SessionRef,
    data: String,
    binary: bool,
) -> Res<()> {
    let data = if binary {
        data.chars().map(|c| c as u32 as u8).collect()
    } else {
        data.into_bytes()
    };
    // A paste bigger than a frame goes as several (`input_requests`).
    for req in nebula_core::input_requests(&session, data) {
        client.send(req).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn resize(client: State<'_, Arc<Client>>, session: SessionRef, cols: u16, rows: u16) -> Res<()> {
    client
        .send(ClientRequest::Resize {
            session,
            cols: cols.max(2),
            rows: rows.max(2),
        })
        .map_err(err)
}

#[tauri::command]
fn mark_seen(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    client
        .send(ClientRequest::MarkAgentSeen { id: AgentId(agent) })
        .map_err(err)
}

fn created_id(created: Option<EntityId>) -> String {
    match created {
        Some(EntityId::Agent(id)) => id.0,
        Some(EntityId::Terminal(id)) => id.0,
        Some(EntityId::Project(id)) => id.0,
        Some(EntityId::Worktree(id)) => id.0,
        Some(EntityId::Link(id)) => id.0,
        None => String::new(),
    }
}

/// The next free `agent-N`, the default name a session takes until its
/// agent titles it.
fn default_agent_name(model: &Model) -> String {
    let n = model
        .agents
        .iter()
        .filter_map(|a| a.name.strip_prefix("agent-")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("agent-{}", n + 1)
}

/// An agent to start (`create_agent`, Start task), shared with `kete ado`.
pub(crate) use kete_ado::NewAgent;

#[tauri::command]
async fn create_agent(client: State<'_, Arc<Client>>, spec: NewAgent) -> Res<String> {
    spawn_agent(&client, spec).await
}

/// Create a session as the TUI's New session picker would: the harness by
/// id (a custom one as `Custom` with its id), and the model and effort the
/// caller picked, else the Agents tab's defaults ("default" = no flag).
async fn spawn_agent(client: &Client, spec: NewAgent) -> Res<String> {
    spawn_agent_with(client, &nebula_config::Config::load(), spec).await
}

/// [`spawn_agent`] against a config already read.
async fn spawn_agent_with(
    client: &Client,
    cfg: &nebula_config::Config,
    spec: NewAgent,
) -> Res<String> {
    let launch = settings::launch_args(cfg, spec.kind.trim(), spec.model, spec.effort)?;
    let name = default_agent_name(&client.snapshot());
    let prompt = spec
        .prompt
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let req_id = client.req_id();
    let created = client
        .rpc(
            req_id,
            ClientRequest::CreateAgent {
                req_id,
                worktree: WorktreeId(spec.worktree),
                name,
                kind: launch.kind,
                custom_harness: launch.custom,
                model: launch.model,
                effort: launch.effort,
                auto_title: true,
                cloud_prompt: None,
                starting_prompt: prompt,
                issue_url: None,
                role: Default::default(),
                profile: spec.profile.filter(|p| !p.is_empty()),
            },
        )
        .await
        .map_err(err)?;
    Ok(created_id(created))
}

#[tauri::command]
async fn create_terminal(client: State<'_, Arc<Client>>, worktree: String) -> Res<String> {
    new_terminal(&client, worktree).await
}

/// Open a shell in `worktree`; its terminal id.
async fn new_terminal(client: &Client, worktree: String) -> Res<String> {
    let req_id = client.req_id();
    let created = client
        .rpc(
            req_id,
            ClientRequest::CreateTerminal {
                req_id,
                worktree: WorktreeId(worktree),
                name: None,
            },
        )
        .await
        .map_err(err)?;
    Ok(created_id(created))
}

#[tauri::command]
async fn add_project(client: State<'_, Arc<Client>>, path: String) -> Res<String> {
    let req_id = client.req_id();
    let created = client
        .rpc(
            req_id,
            ClientRequest::AddProject {
                req_id,
                path: PathBuf::from(path.trim()),
                name: None,
                create_missing: false,
            },
        )
        .await
        .map_err(err)?;
    Ok(created_id(created))
}

#[tauri::command]
async fn start_run(client: State<'_, Arc<Client>>, worktree: String) -> Res<String> {
    let req_id = client.req_id();
    let created = client
        .rpc(
            req_id,
            ClientRequest::StartRun {
                req_id,
                worktree: WorktreeId(worktree),
            },
        )
        .await
        .map_err(err)?;
    Ok(created_id(created))
}

/// Stop the worktree's run terminal (the Run button's other half).
#[tauri::command]
async fn stop_run(client: State<'_, Arc<Client>>, worktree: String) -> Res<()> {
    let req_id = client.req_id();
    client
        .rpc(
            req_id,
            ClientRequest::StopRun {
                req_id,
                worktree: WorktreeId(worktree),
            },
        )
        .await
        .map_err(err)?;
    Ok(())
}

fn worktree_path(client: &Client, worktree: &str) -> Res<PathBuf> {
    client
        .snapshot()
        .worktrees
        .iter()
        .find(|w| w.id.0 == worktree)
        .map(|w| w.path.clone())
        .ok_or_else(|| "that worktree is gone".to_string())
}

#[tauri::command]
async fn diff_files(client: State<'_, Arc<Client>>, worktree: String) -> Res<Vec<git::DiffFile>> {
    let root = worktree_path(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || git::changed_files(&root))
        .await
        .map_err(err)?
}

/// Where the worktree forked from its base branch, for the header.
#[derive(Serialize)]
struct ReviewBaseInfo {
    sha: String,
    short: String,
    branch: String,
}

#[derive(Serialize)]
struct ReviewCommit {
    sha: String,
    short: String,
    subject: String,
}

/// A worktree's diff read against its base or HEAD
/// (`nebula_core::review_base`). `against` is what was read: `base` only
/// when there is a base to read from. `commits` are those since the base,
/// newest first, whichever way it was read.
#[derive(Serialize)]
struct ReviewDiff {
    against: &'static str,
    base: Option<ReviewBaseInfo>,
    commits: Vec<ReviewCommit>,
    files: Vec<git::DiffFile>,
}

/// `against`: `base` or `head` as the reader picked it; absent, the base
/// when there is one.
#[tauri::command]
async fn diff_review(
    client: State<'_, Arc<Client>>,
    worktree: String,
    against: Option<String>,
) -> Res<ReviewDiff> {
    use nebula_core::review_base::{self as rb, Against};
    let root = worktree_path(&client, &worktree)?;
    let recorded = client.recorded_base(&worktree);
    tauri::async_runtime::spawn_blocking(move || {
        let setting = nebula_config::Config::load().worktree_base_branch;
        let base = rb::resolve_with(&root, &setting, recorded.as_ref());
        let asked = against.as_deref().and_then(Against::parse);
        let against = rb::default_against(base.as_ref(), asked);
        let files = match (&base, against) {
            (Some(b), Against::Base) => git::files_since(&root, &b.sha)?,
            _ => git::changed_files(&root)?,
        };
        let commits = base
            .as_ref()
            .map(|b| rb::commits_since(&root, &b.sha, rb::MAX_COMMITS))
            .unwrap_or_default()
            .into_iter()
            .map(|c| ReviewCommit {
                sha: c.sha,
                short: c.short,
                subject: c.subject,
            })
            .collect();
        Ok(ReviewDiff {
            against: against.as_str(),
            base: base.map(|b| ReviewBaseInfo {
                short: b.short().to_string(),
                sha: b.sha,
                branch: b.branch,
            }),
            commits,
            files,
        })
    })
    .await
    .map_err(err)?
}

/// A session's own changes in its worktree: what differs between the
/// snapshot taken when it started and the one at its last turn end
/// (`nebula_core::session_snapshot`), as the review's "this session"
/// filter reads them. `sessions` is how many sessions in the worktree have
/// snapshots: the filter is offered only past one. `start`/`last` are the
/// tree SHAs the files were read between, and the blobs are read from;
/// both None (and no files) when either snapshot is missing or could not
/// be taken.
#[derive(Serialize)]
struct SessionDiff {
    sessions: u32,
    start: Option<String>,
    last: Option<String>,
    /// Why there is no pair to read: no turn ended yet, or too big.
    note: Option<&'static str>,
    files: Vec<git::DiffFile>,
}

/// None: the daemon is older than the snapshots, and says nothing of them.
#[tauri::command]
async fn diff_session(
    client: State<'_, Arc<Client>>,
    worktree: String,
    agent: String,
) -> Res<Option<SessionDiff>> {
    let root = worktree_path(&client, &worktree)?;
    let Some(changes) = client.session_changes(&agent).await.map_err(err)? else {
        return Ok(None);
    };
    let trees = changes.trees().map(|(a, b)| (a.to_string(), b.to_string()));
    let note = match (&trees, &changes.last) {
        (Some(_), _) => None,
        (None, None) => Some("No turn of this session has ended yet."),
        (None, Some(_)) => Some("This session's snapshot was too big to take."),
    };
    let sessions = changes.sessions;
    tauri::async_runtime::spawn_blocking(move || {
        let files = match &trees {
            Some((a, b)) => git::diff_files_between(&root, a, b)?,
            None => Vec::new(),
        };
        let (start, last) = trees.unzip();
        Ok(Some(SessionDiff {
            sessions,
            start,
            last,
            note,
            files,
        }))
    })
    .await
    .map_err(err)?
}

#[derive(Deserialize)]
struct FileRef {
    path: String,
    from: Option<String>,
    status: char,
}

#[tauri::command]
async fn diff_pair(
    client: State<'_, Arc<Client>>,
    worktree: String,
    file: FileRef,
) -> Res<git::DiffPair> {
    let root = worktree_path(&client, &worktree)?;
    let file = git::DiffFile {
        path: file.path,
        from: file.from,
        status: file.status,
        added: None,
        removed: None,
    };
    tauri::async_runtime::spawn_blocking(move || git::file_pair(&root, &file))
        .await
        .map_err(err)?
}

/// A range diff pinned to SHAs: `base` is the merge-base, not the target.
/// `offline` means a fetch failed and cached refs were used.
#[derive(Serialize)]
struct RangeDiff {
    base: String,
    tip: String,
    offline: bool,
    files: Vec<git::DiffFile>,
}

/// `pr`: the pull request the range belongs to, whose `refs/pull/{pr}/merge`
/// is fetched first so the PR's pinned SHAs are there to read.
#[tauri::command]
async fn diff_range(
    client: State<'_, Arc<Client>>,
    worktree: String,
    base: String,
    tip: String,
    pr: Option<u32>,
) -> Res<RangeDiff> {
    let root = worktree_path(&client, &worktree)?;
    // The repo's ADO remote, which has the PR refs, even when origin is
    // another host.
    let remote = ado::range_remote(&root).await;
    tauri::async_runtime::spawn_blocking(move || {
        let merge = pr.map(|id| format!("refs/pull/{id}/merge"));
        let prefetch: Vec<&str> = merge.iter().map(String::as_str).collect();
        let refs = git::prepare_range_at(&root, &remote, &base, &tip, &prefetch)?;
        let files = git::diff_files_between(&root, &refs.base, &refs.tip)?;
        Ok(RangeDiff {
            base: refs.base,
            tip: refs.tip,
            offline: refs.offline,
            files,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn diff_blob(
    client: State<'_, Arc<Client>>,
    worktree: String,
    side: git::DiffSide,
    path: String,
) -> Res<git::Blob> {
    let root = worktree_path(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || git::blob(&root, &side, &path))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn diff_hunks(
    client: State<'_, Arc<Client>>,
    worktree: String,
    path: String,
    staged: bool,
) -> Res<Vec<git::Hunk>> {
    let root = worktree_path(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || git::hunks(&root, &path, staged))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn diff_apply_hunk(
    client: State<'_, Arc<Client>>,
    worktree: String,
    hunk: git::Hunk,
    action: git::HunkAction,
) -> Res<()> {
    let root = worktree_path(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || git::apply_hunk(&root, &hunk, action))
        .await
        .map_err(err)?
}

#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Res<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) links open from here".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[derive(Serialize)]
struct Launchables {
    harnesses: Vec<settings::HarnessInfo>,
    profiles: Vec<launch::ProfileInfo>,
    /// The harness the quick prompt starts on: the Agents tab's Quick
    /// prompt Agent, stepped past one switched off since.
    default_harness: String,
    /// Quick prompt: start in a new worktree by default (Sessions tab).
    new_worktree: bool,
    /// Open the session a launch starts (Sessions tab).
    follow: bool,
    /// Archive asks first (Sessions tab, Confirm on archive).
    ask_before_archive: bool,
    /// Every worktree gets a row in the overview, an empty one too (Show all
    /// worktrees, shared with the terminal app's grid).
    show_all_worktrees: bool,
    /// Finished subagents fold into a count under their session (Sessions
    /// tab, Finished subagents; shared with the terminal app).
    hide_done_subagents: bool,
    /// The ACCOUNT SPREAD (`manual`, `failover`, `least_used`): off
    /// `manual`, the quick prompt starts on "auto" so the spread chooses.
    spread: String,
    /// The failover threshold, percent, as config.json writes it.
    spread_threshold: String,
    /// PROJECT DEFAULT ACCOUNTS by repo path as the daemon stores it:
    /// harness id to profile name.
    project_accounts:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// What the quick prompt offers: the harnesses the TUI's New session
/// picker would (enabled, valid, built-in and custom), and every profile.
#[tauri::command]
fn launchables() -> Launchables {
    let cfg = nebula_config::Config::load();
    let harnesses = settings::offered(&cfg);
    let default_harness = cfg.quick_prompt_kind().as_str().to_string();
    let profiles = launch::launch_profiles(&nebula_core::profiles::load());
    let spread = nebula_core::accounts::Spread::parse(&cfg.account_spread);
    Launchables {
        spread_threshold: cfg.account_spread_threshold.clone(),
        project_accounts: nebula_core::profiles::load_project_defaults(),
        spread: spread.as_str().into(),
        harnesses,
        profiles,
        default_harness,
        new_worktree: cfg.quick_prompt_new_worktree,
        follow: cfg.follow_new_session,
        ask_before_archive: cfg.ask_before_archive,
        show_all_worktrees: cfg.show_all_worktrees,
        hide_done_subagents: cfg.hide_done_subagents,
    }
}

/// Command-line options: `--view board|session|diff|orchestrator`, `--session <name or
/// id>`, `--theme light|dark|system`, `--diff side|inline|replacement`. They
/// pick what the window opens on.
#[derive(Serialize, Default)]
struct Startup {
    view: Option<String>,
    session: Option<String>,
    theme: Option<String>,
    diff_mode: Option<String>,
}

#[tauri::command]
fn startup() -> Startup {
    let mut out = Startup::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let slot = match a.as_str() {
            "--view" => &mut out.view,
            "--session" => &mut out.session,
            "--theme" => &mut out.theme,
            "--diff" => &mut out.diff_mode,
            _ => continue,
        };
        *slot = args.next();
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrLists {
    mine: Vec<prs::PrSummary>,
    review: Vec<prs::PrSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrDetail {
    pr: prs::PrSummary,
    policies: Vec<prs::PolicyRow>,
    threads: Vec<prs::Thread>,
}

#[tauri::command]
async fn ado_prs(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
) -> AdoRes<PrLists> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    let me = prs::my_id(&ctx.ado.client).await?;
    let (mine, review) = prs::list(&ctx.ado.client, &ctx.repo_id, &me).await?;
    Ok(PrLists { mine, review })
}

#[tauri::command]
async fn ado_pr_detail(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    id: u32,
) -> AdoRes<PrDetail> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    let (pr, threads) = prs::get(&ctx.ado.client, &ctx.repo_id, id).await?;
    // Unreadable policies are one "unavailable" row, not a failed detail.
    let policies = prs::policies_or_note(&ctx.ado.client, &ctx.project_id, id, pr.is_draft).await;
    Ok(PrDetail {
        pr,
        policies,
        threads,
    })
}

#[tauri::command]
async fn ado_create_pr(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    pr: prs::NewPr,
) -> AdoRes<prs::CreateResult> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    let reviewers = pr.reviewers.clone();
    let out = prs::create(&ctx, pr).await?;
    // An existing PR was not made with these reviewers: nothing to remember.
    if !matches!(out.outcome, prs::CreateOutcome::Existing(_)) {
        for r in reviewers.into_iter().rev() {
            ctx.ado.store.remember_reviewer(&ctx.ado.key, r);
        }
    }
    Ok(out)
}

/// Retry for "Created, task not linked".
#[tauri::command]
async fn ado_link_pr_task(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    id: u32,
    task: u32,
) -> AdoRes<()> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    prs::link_task(&ctx, id, task).await.map_err(CmdError::from)
}

#[tauri::command]
async fn ado_publish_pr(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    id: u32,
) -> AdoRes<()> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    prs::publish(&ctx, id).await.map_err(CmdError::from)
}

#[tauri::command]
async fn ado_vote(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    id: u32,
    vote: i8,
) -> AdoRes<()> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    prs::vote(&ctx, id, vote).await.map_err(CmdError::from)
}

#[tauri::command]
async fn ado_reply(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    id: u32,
    thread: u32,
    text: String,
) -> AdoRes<prs::Comment> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    prs::reply(&ctx, id, thread, &text)
        .await
        .map_err(CmdError::from)
}

#[tauri::command]
async fn ado_search_identities(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    q: String,
) -> AdoRes<Vec<ado::store::Reviewer>> {
    let ctx = PrCtx::for_command(&ado, &client, &worktree).await?;
    prs::search_identities(&ctx, &q)
        .await
        .map_err(CmdError::from)
}

/// Reviewers picked before in this repo, newest first. Local only: a
/// checkout without an ADO remote has none.
#[tauri::command]
async fn ado_recent_reviewers(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
) -> AdoRes<Vec<ado::store::Reviewer>> {
    let path = ado::checkout_path(&client, &worktree)?;
    let Some(ctx) = ado.silent_context(&path).await? else {
        return Ok(Vec::new());
    };
    Ok(ctx
        .store
        .read_repo(&ctx.key, |s| s.recent_reviewers.clone()))
}

/// What the PR composer starts from for `branch`: target, commit
/// subjects in range, and the branch's task.
#[tauri::command]
async fn ado_compose_seed(
    client: State<'_, Arc<Client>>,
    ado: State<'_, AdoState>,
    worktree: String,
    branch: String,
) -> AdoRes<prs::ComposeSeed> {
    let ctx = ado.context_for(&client, &worktree).await?;
    Ok(prs::compose_seed(&ctx, &branch).await)
}

/// Ask a session a question (`kete ask`) and hand back its answer, for
/// the composer's "Ask the session to summarise".
#[tauri::command]
async fn session_ask(
    client: State<'_, Arc<Client>>,
    agent: String,
    question: String,
) -> Res<String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("The question is empty.".into());
    }
    client.ask(&agent, question).await.map_err(err)
}

/// The main window has the user's eyes: alerts stand down.
fn window_focused(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// What the daemon says outside the model: a session's turn gathers into
/// the alert batch (posted [`alerts::DEBOUNCE`] after it opens, unless the
/// window has come forward by then), and `kete open` raises the file sheet.
fn on_notice(
    app: tauri::AppHandle,
    client: std::sync::Weak<Client>,
) -> impl Fn(daemon::Notice) + Send + Sync {
    let batch = Arc::new(std::sync::Mutex::new(alerts::Batch::default()));
    move |notice| match notice {
        daemon::Notice::Turned { agent, turn } => {
            if window_focused(&app) {
                return;
            }
            let Some(client) = client.upgrade() else {
                return;
            };
            let Some(alert) = alerts::alert_for(&client.snapshot(), &agent, turn) else {
                return;
            };
            if !batch.lock().expect("alert batch").push(alert) {
                return;
            }
            let (app, batch) = (app.clone(), batch.clone());
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(alerts::DEBOUNCE).await;
                let gathered = batch.lock().expect("alert batch").take();
                if window_focused(&app) {
                    return;
                }
                let identifier = app.config().identifier.clone();
                let click = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    alerts::post(
                        &gathered,
                        &identifier,
                        Box::new(move |agent| bring_up(&click, agent)),
                    );
                });
            });
        }
        daemon::Notice::FilesOpened { root, paths } => {
            app.state::<files::OpenedFiles>().allow(&paths);
            let _ = app.emit("kete://files-opened", files::opened(&root, &paths));
        }
    }
}

/// A notification clicked: the window comes forward on that session.
fn bring_up(app: &tauri::AppHandle, agent: String) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit("kete://open-session", agent);
}

/// Push the model to the window whenever it changes, at most every 50 ms.
async fn emit_state(app: tauri::AppHandle, client: Arc<Client>) {
    loop {
        client.changed.notified().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = app.emit("kete://state", client.snapshot());
    }
}

fn main() {
    // Kete was Bro: carry Bro's dirs over (or keep using them while Bro's
    // daemon runs) before the client works out where the daemon is. This
    // window has no console to say so on; `kete` says it when it moves them.
    let _ = nebula_core::paths::migrate_legacy_dirs();
    // Before anything writes a settings file: is this a first install?
    learn::note_startup();
    let client = Client::new();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(client.clone())
        // One token source, one ado.json store and one set of clients for
        // every ADO command and the PR poll.
        .manage(AdoState::system())
        .manage(pr_poll::PrRefresh::default())
        .manage(branches::Switching::default())
        .manage(files::OpenedFiles::default())
        .manage(update::UpdateState::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            // The Agents tab's model lists, as the TUI's: Claude Code's own
            // availableModels when its settings carry one, and the cached
            // cursor-agent list (never a fresh probe from a GUI window).
            let cfg = nebula_config::Config::load();
            nebula_config::claude_catalogue::bootstrap(cfg.claude_enabled);
            // VS Code and Visual Studio found off the main thread (vswhere
            // takes a moment), so the first menu already lists them.
            nebula_core::editors::detect_in_background();
            if cfg.cursor_enabled {
                nebula_config::cursor_catalogue::install_cached();
            }
            // The orchestrator's review requests reach the window as an event;
            // the window decides when to show them (never stealing focus).
            let reviews = handle.clone();
            client.on_review(Box::new(move |r| {
                let _ = reviews.emit("kete://review", r);
            }));
            // Done / needs-you alerts and `kete open`.
            client.on_notice(Box::new(on_notice(handle.clone(), Arc::downgrade(&client))));
            tauri::async_runtime::spawn(update::run(handle.clone()));
            tauri::async_runtime::spawn(daemon::run(client.clone()));
            tauri::async_runtime::spawn(emit_state(handle.clone(), client.clone()));
            tauri::async_runtime::spawn(pr_poll::poll_prs(client.clone(), handle));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("kete");
                // WebView2's own menu and browser keys off (quiet.rs).
                #[cfg(windows)]
                quiet::quiet(&w);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Worktrees, branches and pull requests
            pr_poll::refresh_prs,
            branches::worktree_settings,
            branches::checkout_state,
            branches::move_check,
            branches::worktree_git,
            branches::git_stamps,
            board::board_scope,
            branches::worktree_reveal,
            branches::branch_list,
            branches::branch_fetch,
            branches::branch_switch,
            get_state,
            retry_connect,
            attach,
            detach,
            input,
            resize,
            mark_seen,
            create_agent,
            create_terminal,
            add_project,
            start_run,
            stop_run,
            sessions::rename_agent,
            sessions::archive_agent,
            sessions::unarchive_agent,
            sessions::delete_agent,
            sessions::restart_agent,
            sessions::move_agent,
            order::reorder,
            subagents::clear_finished_subagents,
            sessions::rename_terminal,
            sessions::close_terminal,
            sessions::follow_up,
            sessions::spawn_follow_up,
            sessions::ask_aside,
            orchestrator::start_orchestrator,
            orchestrator::set_orchestrator,
            orchestrator::clear_orchestrator,
            orchestrator::read_session,
            projects::suggest_branch,
            projects::create_worktree,
            projects::delete_worktree,
            projects::rename_project,
            projects::remove_project,
            projects::set_project_path,
            projects::open_worktree,
            projects::open_remote,
            launch::quick_launch,
            launch::account_pick,
            presets::presets_list,
            presets::preset_save,
            presets::preset_delete,
            profiles::profiles_list,
            profiles::profile_harnesses,
            profiles::profile_add,
            profiles::profile_update,
            profiles::profile_remove,
            profiles::profile_set_default,
            accounts::accounts_list,
            accounts::accounts_usage_nudge,
            // Whakaoho (wake-up)
            wake::wake_overview,
            wake::wake_now,
            wake::wake_set,
            accounts::account_switch,
            accounts::project_set_account,
            profiles::profile_duplicate,
            profiles::profile_rename,
            accounts::profile_set_enabled,
            accounts::account_login_run,
            settings::settings_schema,
            settings::settings_set,
            settings::settings_reset,
            settings_io::settings_export,
            settings_io::settings_import_preview,
            settings_io::settings_import,
            settings::desktop_prefs,
            fonts::system_fonts,
            look::look_vscode,
            look::look_vscode_install,
            look::look_installed_themes,
            look::look_import_installed,
            look::look_import_file,
            look::look_remove_theme,
            look::look_reset,
            look::look_use_vscode_fonts,
            look::look_zoom,
            settings::desktop_set_collapsed,
            learn::learn_state,
            learn::learn_seen,
            learn::learn_tips,
            settings::config_problem,
            settings::actions_list,
            settings::desktop_key_set,
            settings::run_commands,
            critters::critters_log,
            critters::critters_log_update,
            critters::critters_prefs,
            diff_files,
            diff_review,
            diff_session,
            diff_pair,
            diff_range,
            diff_blob,
            diff_hunks,
            diff_apply_hunk,
            open_url,
            files::opened_file_read,
            files::opened_file_open,
            // The user's own editors (editors.rs)
            editors::editors_list,
            editors::editors_rescan,
            editors::editor_icon,
            editors::editor_targets,
            editors::editor_windows,
            editors::editor_open,
            editors::editor_dock,
            editors::path_reveal,
            editors::path_open_default,
            editors::worktree_file_read,
            tree::worktree_tree,
            update::update_status,
            launchables,
            startup,
            ado_prs,
            ado_pr_detail,
            ado_create_pr,
            ado_link_pr_task,
            ado_publish_pr,
            ado_vote,
            ado_reply,
            ado_search_identities,
            ado_recent_reviewers,
            // PR composer (Task 17)
            ado_compose_seed,
            // Ship: a worktree's PR on GitHub or Azure DevOps
            ship::ship_status,
            ship::ship_create_pr,
            ship::ship_merge,
            ship::ship_gh_sign_in,
            session_ask,
            ado::branches::ado_branches,
            ado::branches::ado_fetch,
            ado::branches::ado_checkout_worktree,
            ado::branches::ado_delete_local,
            // ADO tasks (Tasks 6 and 10)
            ado::tasks::ado_teams,
            ado::tasks::ado_iterations,
            ado::tasks::ado_tasks,
            ado::tasks::ado_set_team,
            ado::tasks::ado_task_prompt,
            ado::tasks::ado_start_task,
            // Workshop shell (Task 13)
            ado::ui::ado_context,
            ado::ui::ado_save_ui,
            ado::ui::ado_store_error,
            ado::ui::ado_sign_in,
        ])
        .run(tauri::generate_context!())
        .expect("kete-desktop failed to start");
}
