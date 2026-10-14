//! kete-desktop: Kete's native window over the same daemon the TUI drives.
//! The Rust side owns the daemon connection (`daemon`) and the git reads
//! (`git`); the web view draws the kanban, the live terminals and the
//! diffs, and talks to this side through the commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod ado;
mod attribution;
mod daemon;
mod git;
mod launch;
mod presets;
mod profiles;
mod projects;
mod sessions;
mod settings;

use ado::client::CmdError;
use ado::prs::{self, PrCtx};
use ado::{AdoContext, AdoState};
use daemon::{Client, Model, PtyEvent};
use nebula_core::{AgentId, ClientRequest, EntityId, SessionRef, WorktreeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    client
        .send(ClientRequest::Input { session, data })
        .map_err(err)
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewAgent {
    /// Empty when the worktree does not exist yet (Start task fills it).
    #[serde(default)]
    worktree: String,
    kind: String,
    profile: Option<String>,
    prompt: Option<String>,
    /// A model picked for this one launch; None takes the Agents tab's.
    #[serde(default)]
    model: Option<String>,
    /// An effort picked for this one launch; None takes the Agents tab's.
    #[serde(default)]
    effort: Option<String>,
}

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
struct ProfileInfo {
    name: String,
    harness: String,
    default: bool,
}

#[derive(Serialize)]
struct Launchables {
    harnesses: Vec<settings::HarnessInfo>,
    profiles: Vec<ProfileInfo>,
    /// The harness the quick prompt starts on: the Agents tab's Quick
    /// prompt Agent, stepped past one switched off since.
    default_harness: String,
    /// Quick prompt: start in a new worktree by default (Sessions tab).
    new_worktree: bool,
    /// Open the session a launch starts (Sessions tab).
    follow: bool,
    /// Archive asks first (Sessions tab, Confirm on archive).
    ask_before_archive: bool,
}

/// What the quick prompt offers: the harnesses the TUI's New session
/// picker would (enabled, valid, built-in and custom), and every profile.
#[tauri::command]
fn launchables() -> Launchables {
    let cfg = nebula_config::Config::load();
    let harnesses = settings::offered(&cfg);
    let default_harness = cfg.quick_prompt_kind().as_str().to_string();
    let profiles = nebula_core::profiles::load()
        .into_iter()
        .filter(|p| !p.disabled)
        .map(|p| ProfileInfo {
            name: p.name,
            harness: p.harness,
            default: p.default,
        })
        .collect();
    Launchables {
        harnesses,
        profiles,
        default_harness,
        new_worktree: cfg.quick_prompt_new_worktree,
        follow: cfg.follow_new_session,
        ask_before_archive: cfg.ask_before_archive,
    }
}

/// Command-line options: `--view board|session|diff`, `--session <name or
/// id>`, `--theme light|dark|system`. They pick what the window opens on.
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

/// The open ADO pull request from `branch`, for the PR open lane. Its
/// client is silent: the poll never opens a sign-in window, and finds a
/// token once a user-initiated command has signed in. A sign-in failure
/// pauses the org's poll until a command works for it again.
async fn ado_open_pr(ado: &AdoState, ctx: AdoContext, branch: &str) -> Option<daemon::PrInfo> {
    let org = ctx.remote.org_url.clone();
    if ado.is_paused(&org) {
        return None;
    }
    let res = async {
        let ctx = PrCtx::new(ctx).await?;
        prs::for_source(&ctx.ado.client, &ctx.repo_id, branch).await
    }
    .await;
    match res {
        Ok(list) => list.into_iter().next().map(|p| daemon::PrInfo {
            number: p.id.into(),
            url: p.url,
            title: p.title,
            draft: p.is_draft,
        }),
        Err(e) => {
            if prs::is_auth_error(&e) {
                ado.pause(&org);
            }
            None
        }
    }
}

/// Ask for each worktree's open pull request, now and then, for the PR
/// open lane: Azure DevOps for ADO remotes, `gh` for the rest. Only
/// checkouts with agent sessions are asked.
async fn poll_prs(client: Arc<Client>, app: tauri::AppHandle) {
    let ado = app.state::<AdoState>();
    let have_gh = tauri::async_runtime::spawn_blocking(git::have_gh)
        .await
        .unwrap_or(false);
    loop {
        let model = client.snapshot();
        let wanted: Vec<(String, PathBuf, String)> = model
            .worktrees
            .iter()
            .filter(|w| !w.is_main && model.agents.iter().any(|a| a.worktree_id == w.id))
            .map(|w| (w.id.0.clone(), w.path.clone(), w.branch.clone()))
            .collect();
        let mut out = HashMap::new();
        for (id, path, branch) in wanted {
            // Several ADO remotes and none of them origin reads as none.
            let pr = match ado.silent_context(&path).await.ok().flatten() {
                Some(ctx) => ado_open_pr(&ado, ctx, &branch).await,
                None if have_gh => {
                    tauri::async_runtime::spawn_blocking(move || git::open_pr(&path, &branch))
                        .await
                        .ok()
                        .flatten()
                }
                None => None,
            };
            if let Some(pr) = pr {
                out.insert(id, pr);
            }
        }
        client.set_prs(out);
        tokio::time::sleep(Duration::from_secs(90)).await;
    }
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
    let client = Client::new();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(client.clone())
        // One token source, one ado.json store and one set of clients for
        // every ADO command and the PR poll.
        .manage(AdoState::system())
        .setup(move |app| {
            let handle = app.handle().clone();
            // The Agents tab's model lists, as the TUI's: Claude Code's own
            // availableModels when its settings carry one, and the cached
            // cursor-agent list (never a fresh probe from a GUI window).
            let cfg = nebula_config::Config::load();
            nebula_config::claude_catalogue::bootstrap(cfg.claude_enabled);
            if cfg.cursor_enabled {
                nebula_config::cursor_catalogue::install_cached();
            }
            tauri::async_runtime::spawn(daemon::run(client.clone()));
            tauri::async_runtime::spawn(emit_state(handle.clone(), client.clone()));
            tauri::async_runtime::spawn(poll_prs(client.clone(), handle));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("kete");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            sessions::rename_terminal,
            sessions::close_terminal,
            sessions::follow_up,
            projects::suggest_branch,
            projects::create_worktree,
            projects::delete_worktree,
            projects::rename_project,
            projects::remove_project,
            projects::set_project_path,
            projects::open_worktree,
            projects::open_remote,
            launch::quick_launch,
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
            accounts::profile_set_enabled,
            settings::settings_schema,
            settings::settings_set,
            settings::settings_reset,
            settings::desktop_prefs,
            settings::run_commands,
            diff_files,
            diff_pair,
            diff_range,
            diff_blob,
            diff_hunks,
            diff_apply_hunk,
            open_url,
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
