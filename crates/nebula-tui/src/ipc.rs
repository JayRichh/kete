//! Client-side IPC: connect to the daemon (auto-spawning it when absent) and
//! perform the version handshake.

use crate::platform::Stream;
use anyhow::{bail, Context, Result};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::{
    env, paths, Agent, AgentId, AgentKind, ClientRequest, EnterOutcome, EntityId, Link,
    OrchestratorSession, Project, ProjectId, ReviewTabKind, ServerEvent, SessionRef,
    SessionSummary, TerminalId, TerminalTab, Worktree, WorktreeId, PROTOCOL_VERSION,
};
use std::time::Duration;
#[cfg(unix)]
use tokio::net::UnixStream;

/// Request id for the one-shot CLIs: each opens a fresh connection, sends a
/// single request and waits for its reply, so there is never a second id.
const ONE_SHOT_REQ_ID: u64 = 1;
const SESSION_CONTEXT_PROTOCOL: u32 = 50;
/// The daemon hung up mid-request — the message every one-shot client shows.
pub(crate) const CLOSED_BEFORE_REPLY: &str = "daemon closed the connection before replying";
/// How often the connect and shutdown waits re-check the daemon.
const POLL_STEP: Duration = Duration::from_millis(50);

pub struct Connection {
    pub stream: Stream,
    pub daemon_pid: u32,
    pub protocol_version: u32,
}

/// Connect, auto-spawning `current_exe() daemon` when nothing is listening.
pub async fn connect_or_spawn() -> Result<Connection> {
    let sock = paths::socket_path();

    if let Ok(conn) = try_connect(&sock).await {
        return handshake(conn).await;
    }

    if matches!(
        nebula_core::host::wsl_flavor(),
        Some(nebula_core::host::WslFlavor::Wsl1)
    ) {
        bail!(
            "kete supports WSL2, but this looks like WSL1. \
             WSL1 does not provide the Linux kernel features the daemon needs \
             for its Unix socket, pidfile lock, and PTYs. Convert the distro \
             with `wsl.exe --set-version <distro> 2`, then run `kete` again."
        );
    }

    spawn_daemon()?;

    // Poll-connect while the daemon boots.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match try_connect(&sock).await {
            Ok(conn) => return handshake(conn).await,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(POLL_STEP).await;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "daemon did not come up on {}: check {}",
                        sock.display(),
                        paths::daemon_log_path().display()
                    )
                })
            }
        }
    }
}

/// Connect to the daemon only when one is already listening: None when
/// nothing is, and none is started.
pub async fn connect_existing() -> Result<Option<Connection>> {
    match try_connect(&paths::socket_path()).await {
        Ok(stream) => handshake(stream).await.map(Some),
        Err(_) => Ok(None),
    }
}

async fn try_connect(sock: &std::path::Path) -> Result<Stream> {
    Ok(crate::platform::connect(sock).await?)
}

/// Windows: a detached process with no console, in a process group of its
/// own so the console's Ctrl+C never reaches it, and out of any job this
/// client runs in (an IDE terminal's, say) when that job allows it — so the
/// daemon outlives the client as it does on unix.
#[cfg(windows)]
fn spawn_daemon() -> Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
    };
    let exe = std::env::current_exe().context("resolve current_exe")?;
    let flags = CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW;
    let spawn = |flags: u32| {
        std::process::Command::new(&exe)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    // A job that forbids breakaway refuses the whole create; then the
    // daemon stays in it.
    spawn(flags | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| spawn(flags))
        .context("spawn kete daemon")?;
    Ok(())
}

#[cfg(unix)]
fn spawn_daemon() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().context("resolve current_exe")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // New *session*, not just a new process group: besides outliving this
    // client and skipping its terminal signals (Ctrl+C etc.), the daemon must
    // hold no controlling terminal. It shells out to the user's interactive
    // shell (CLI probes, login-shell agent wrap), and an interactive zsh that
    // can reach a tty via /dev/tty grabs its foreground process group —
    // SIGTTIN-stopping the TUI running on this terminal mid-frame.
    unsafe {
        cmd.pre_exec(own_session);
    }
    cmd.spawn().context("spawn kete daemon")?;
    Ok(())
}

/// `pre_exec` hook putting the child in a session of its own (`setsid`).
/// Avoids a libc dependency for one call (same pattern as nebula-core's
/// geteuid).
#[cfg(unix)]
pub(crate) fn own_session() -> std::io::Result<()> {
    extern "C" {
        fn setsid() -> i32;
    }
    if unsafe { setsid() } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

async fn handshake(mut stream: Stream) -> Result<Connection> {
    // Asked of the kernel before the daemon gets a word in: a daemon on
    // another protocol hangs up right after its answer.
    let listener_pid = peer_pid(&stream);
    write_frame(
        &mut stream,
        &ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;
    match read_frame::<ServerEvent, _>(&mut stream).await? {
        Some(ServerEvent::HelloOk {
            protocol_version,
            daemon_pid,
        }) => {
            if nebula_core::protocol_compatible_with(protocol_version) {
                Ok(Connection {
                    stream,
                    daemon_pid,
                    protocol_version,
                })
            } else {
                bail!(version_skew_message(protocol_version, listener_pid))
            }
        }
        Some(ServerEvent::Incompatible {
            daemon_protocol_version,
        }) => bail!(version_skew_message(daemon_protocol_version, listener_pid)),
        other => bail!("unexpected handshake reply: {other:?}"),
    }
}

/// The pid of the process listening on the far end of `stream`, from the
/// kernel's peer credentials (`SO_PEERCRED`, `LOCAL_PEEREPID`) — which need
/// neither a protocol the daemon speaks nor a pidfile that is still there.
#[cfg(unix)]
fn peer_pid(stream: &UnixStream) -> Option<i32> {
    let pid = stream.peer_cred().ok()?.pid()?;
    (pid > 1 && pid.unsigned_abs() != std::process::id()).then_some(pid)
}

/// Windows: the pipe server's pid, from `GetNamedPipeServerProcessId`.
#[cfg(windows)]
fn peer_pid(stream: &Stream) -> Option<i32> {
    let pid = crate::platform::server_pid(stream)?;
    (pid > 4 && pid != std::process::id()).then_some(pid as i32)
}

/// Explain a failed version handshake in terms of the fix.
///
/// Which side is stale decides the remedy, and getting it backwards costs a
/// debugging session: when the *daemon* is ahead, the `nebula` that just ran
/// is an older build than the one the daemon was launched from, and `nebula
/// kill` cannot help — the live instance immediately respawns its daemon
/// from its own binary (`spawn_daemon` above uses `current_exe`), so the
/// skew survives every restart. That is the common shape in a checkout,
/// where `make dev` runs `target/debug` while PATH still finds an older
/// `nebula` from the last `make install`.
///
/// `daemon_pid` comes off the socket, so the message can name the process
/// even when its pidfile is gone — and offer the plain SIGTERM that stops it
/// cleanly if `nebula kill` somehow can't, before anyone reaches for `-9`.
fn version_skew_message(daemon_protocol_version: u32, daemon_pid: Option<i32>) -> String {
    let client = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let daemon = match daemon_pid {
        Some(pid) => format!(
            "{} (pid {pid})",
            daemon_exe_path(pid).unwrap_or_else(|| "unknown".into())
        ),
        None => "unknown".into(),
    };
    let header = format!(
        "protocol mismatch: the daemon speaks v{daemon_protocol_version}, this client \
         v{PROTOCOL_VERSION}\n  this client: {client}\n  the daemon:  {daemon}\n"
    );
    if daemon_protocol_version > PROTOCOL_VERSION {
        format!(
            "{header}This client is the older build, so `kete kill` will not fix it, the \
             running instance respawns its daemon from its own binary. Install the daemon's \
             build over this one instead (`make install` from that checkout)."
        )
    } else {
        let by_hand = daemon_pid
            .map(|pid| {
                format!(
                    " Should that fail, `kill {pid}` asks the daemon for the same clean \
                     shutdown, never `kill -9`, which skips flushing its database."
                )
            })
            .unwrap_or_default();
        format!(
            "{header}The daemon is the older build: run `kete kill` and relaunch. That \
             stops every live session.{by_hand}"
        )
    }
}

/// Best-effort path of the binary the daemon at `pid` was launched from, so
/// the mismatch message can name it. Asked of the OS rather than the
/// handshake: `Incompatible` is what a *newer* daemon sends an older client,
/// so adding a field to it would only break decoding on the clients that
/// need this message most. The buildstamp beside the pidfile is no help
/// either — it is a content hash, not a path.
#[cfg(windows)]
fn daemon_exe_path(pid: i32) -> Option<String> {
    crate::platform::process_exe_path(pid as u32)
}

#[cfg(unix)]
fn daemon_exe_path(pid: i32) -> Option<String> {
    if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return Some(path.display().to_string());
    }
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Channel-based IPC handle for the TUI event loop: outbound requests go
/// through `tx`; inbound events arrive on `rx`. Reader/writer tasks own the
/// socket halves.
pub struct IpcChannels {
    pub tx: tokio::sync::mpsc::Sender<ClientRequest>,
    pub rx: tokio::sync::mpsc::Receiver<ServerEvent>,
    /// The protocol the daemon answered the handshake with.
    pub daemon_protocol: u32,
}

pub fn split_connection(conn: Connection) -> IpcChannels {
    let daemon_protocol = conn.protocol_version;
    let (read_half, mut write_half) = crate::platform::split(conn.stream);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<ServerEvent>(1024);
    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<ClientRequest>(256);

    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(read_half);
        while let Ok(Some(ev)) = read_frame::<ServerEvent, _>(&mut reader).await {
            if event_tx.send(ev).await.is_err() {
                break;
            }
        }
        // Dropping event_tx closes the channel, signalling disconnect.
    });

    tokio::spawn(async move {
        while let Some(req) = req_rx.recv().await {
            if write_frame(&mut write_half, &req).await.is_err() {
                break;
            }
        }
    });

    IpcChannels {
        tx: req_tx,
        rx: event_rx,
        daemon_protocol,
    }
}

/// The agent id a one-shot CLI runs as, from the raw `NEBULA_AGENT_ID`
/// value. Unset and empty are the same miss, and the error names the
/// `nebula <verb>` that needs it. Pure so the message is testable.
fn agent_id_from(value: Option<String>, verb: &str) -> Result<String> {
    value.filter(|v| !v.is_empty()).with_context(|| {
        format!(
            "{} is not set: `kete {verb}` only works from inside a \
             kete agent session",
            env::AGENT_ID
        )
    })
}

/// [`agent_id_from`] over the live environment.
fn current_agent_id(verb: &str) -> Result<String> {
    agent_id_from(env::non_empty(env::AGENT_ID), verb)
}

/// What the daemon said back to a one-shot request.
enum Reply {
    Ack,
    /// The daemon's own message for a request it declined.
    Error(String),
}

/// Read events until the daemon answers `req_id` — an Ack or an Error —
/// skipping anything else it broadcasts meanwhile. Only the hang-up is an
/// `Err`; the daemon's refusal comes back as a [`Reply::Error`] so each
/// caller can decide whether that is a failure.
async fn await_reply(conn: &mut Connection, req_id: u64) -> Result<Reply> {
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Ack { req_id: r, .. }) if r == req_id => return Ok(Reply::Ack),
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => return Ok(Reply::Error(message)),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

fn require_protocol(conn: &Connection, needed: u32, command: &str) -> Result<()> {
    if conn.protocol_version < needed {
        bail!(
            "`kete {command}` needs daemon protocol v{needed}, but the running daemon speaks v{}: run `kete kill` and relaunch",
            conn.protocol_version
        );
    }
    Ok(())
}

/// [`await_reply`] for the callers where the daemon declining *is* the
/// failure: its message becomes the error.
async fn await_ack(conn: &mut Connection, req_id: u64) -> Result<()> {
    match await_reply(conn, req_id).await? {
        Reply::Ack => Ok(()),
        Reply::Error(message) => bail!("{message}"),
    }
}

/// How `nebula rename` treats a session that already carries a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameMode {
    /// Title only an untitled session; the daemon declines otherwise.
    Auto,
    /// Overwrite whatever title the session has (`--force`).
    Force,
}

/// One-shot client for `nebula rename`, run from inside an agent session's
/// CLI: resolve the agent from NEBULA_AGENT_ID and ask the daemon to title
/// it. Never spawns a daemon — no daemon means no session worth titling.
///
/// Daemon-reported outcomes (renamed, or "already titled" on the non-force
/// path) both print and exit 0: for the model running this, a declined
/// auto-title is a settled answer, not a failure to retry.
pub async fn rename_current_agent(title: &str, mode: RenameMode) -> Result<()> {
    let agent_id = current_agent_id("rename")?;
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running: title unchanged");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    let id = AgentId(agent_id);
    let name = title.to_string();
    let request = match mode {
        RenameMode::Force => ClientRequest::RenameAgent { req_id, id, name },
        RenameMode::Auto => ClientRequest::AutoRenameAgent { req_id, id, name },
    };
    write_frame(&mut conn.stream, &request).await?;
    match await_reply(&mut conn, req_id).await? {
        Reply::Ack => println!("session renamed to \"{title}\""),
        Reply::Error(message) => println!("kete: {message}"),
    }
    Ok(())
}

/// CLI: `nebula spawn "<task>" [--kind <claude|codex|cursor>] [--worktree
/// <branch> [--base <ref>]] [--profile <name>]` from inside an agent session — ask the daemon
/// to start a new agent beside this one, in the same worktree or in the
/// project's worktree on `worktree` (created when the branch has none),
/// opening on `task` as its first prompt. The caller is untouched: no
/// relocation, no turn-end wait. Never spawns a daemon: no daemon means no
/// session to sit beside.
///
/// What this prints is read by the model that ran it, so it says what
/// happened and that this session carries on. A daemon-side refusal (a
/// blank task, a missing CLI, a worktree git could not create) is a
/// nonzero exit the model reports.
/// What `kete spawn --profile` says to a daemon older than
/// [`nebula_core::SPAWN_PROFILE_PROTOCOL`]. The model in the session reads
/// it, so the way out must be one the platform has: on unix `kete reload`
/// moves to the new daemon and keeps every session (its own too). Windows
/// has no reload, and `kete kill` stops every session, this one included,
/// so there spawning without --profile is the way on and the restart is
/// the user's to choose.
fn spawn_profile_too_old(daemon: u32, windows: bool) -> String {
    let need = nebula_core::SPAWN_PROFILE_PROTOCOL;
    let way_out = if windows {
        "On Windows the daemon is only replaced by `kete kill`, which stops every session, \
         this one included: spawn without --profile, or tell the user the profile needs a \
         daemon restart (`kete kill`, then reopen kete) when they are ready"
    } else {
        "Ask the user to run `kete reload` (it keeps every session), or spawn without --profile"
    };
    format!(
        "`kete spawn --profile` needs daemon protocol v{need}, but the running daemon speaks \
         v{daemon}: no session started. {way_out}"
    )
}

/// The request a sibling spawn sends a daemon that said `daemon` at
/// Hello. Without a role it is `SpawnSiblingAgent`, 57's frame unless it
/// names a profile (58); a role goes in `SpawnSiblingAgentWith` (66), so a
/// daemon below 66 is never sent one, and is refused here instead.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sibling_spawn_request(
    daemon: u32,
    req_id: u64,
    id: AgentId,
    kind: Option<AgentKind>,
    task: String,
    worktree: Option<String>,
    base: Option<String>,
    profile: Option<String>,
    role_card: Option<String>,
) -> Result<ClientRequest> {
    if let Some(role_card) = role_card {
        crate::ipc_studio::require_studio(daemon, "`kete spawn --role`")?;
        return Ok(ClientRequest::SpawnSiblingAgentWith {
            req_id,
            id,
            kind,
            starting_prompt: task,
            worktree,
            base,
            profile,
            role_card,
        });
    }
    // Without a profile the frame is 57's, which every daemon in range
    // reads; one naming a profile would make an older daemon drop the
    // connection, so it is refused here with the way out.
    if profile.is_some() && daemon < nebula_core::SPAWN_PROFILE_PROTOCOL {
        bail!(spawn_profile_too_old(daemon, cfg!(windows)));
    }
    Ok(ClientRequest::SpawnSiblingAgent {
        req_id,
        id,
        kind,
        starting_prompt: task,
        worktree,
        base,
        profile,
    })
}

pub async fn spawn_sibling_for_current_agent(
    task: &str,
    kind: Option<AgentKind>,
    worktree: Option<String>,
    base: Option<String>,
    profile: Option<String>,
    role: Option<String>,
) -> Result<()> {
    let agent_id = current_agent_id("spawn")?;
    let task = task.trim();
    if task.is_empty() {
        bail!(
            "the task is empty: `kete spawn \"<task>\"` needs the work the new session starts on"
        );
    }
    // The words become a branch as `nebula worktree`'s do, so both
    // commands land on the same branch for the same words. Unlike there, no
    // name is invented for a blank one: a spawn names where its work goes.
    let worktree = match worktree.as_deref().map(crate::branch_name::slugify) {
        Some(slug) if slug.is_empty() => {
            bail!("the branch is empty: `kete spawn --worktree <branch>` needs a branch name")
        }
        other => other,
    };
    let base = match base.as_deref().map(str::trim) {
        Some("") => bail!("the base is empty: `--base <ref>` needs a branch, tag or commit"),
        other => other.map(str::to_string),
    };
    let profile = match profile.as_deref().map(str::trim) {
        Some("") => bail!("the profile is empty: `--profile <name>` needs an account's name"),
        other => other.map(str::to_string),
    };
    let role = match role.as_deref().map(str::trim) {
        Some("") => bail!("the role is empty: `--role <id>` needs a role from .kete/roles.toml"),
        other => other.map(str::to_string),
    };
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running, no session started");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    let request = sibling_spawn_request(
        conn.protocol_version,
        req_id,
        AgentId(agent_id),
        kind,
        task.to_string(),
        worktree.clone(),
        base,
        profile.clone(),
        role,
    )?;
    write_frame(&mut conn.stream, &request).await?;
    await_ack(&mut conn, req_id).await?;
    let harness = kind.map(|k| format!("{} ", k.as_str())).unwrap_or_default();
    let account = profile
        .map(|p| format!(" on the {p} account"))
        .unwrap_or_default();
    let place = match &worktree {
        Some(branch) => format!("the worktree on branch \"{branch}\""),
        None => "this worktree".to_string(),
    };
    println!(
        "started a new {harness}session in {place}{account}; it is working on that task now and \
         shows in the sessions list. This session is unaffected: carry on."
    );
    Ok(())
}

/// CLI: `nebula open <file>…` from inside an agent session — resolve the
/// paths here, where the cwd is the agent's, and hand them to the daemon,
/// which raises every attached TUI's FILE TABS on them. Never spawns a
/// daemon: no daemon means nobody is looking. What this prints is read by
/// the model that ran it.
pub async fn open_files_for_current_agent(files: &[String]) -> Result<()> {
    let agent_id = current_agent_id("open")?;
    if files.is_empty() {
        bail!("nothing to open: `kete open <file>…` needs at least one file");
    }
    let mut resolved = Vec::with_capacity(files.len());
    for file in files {
        let path = paths::canonical(std::path::Path::new(file))
            .with_context(|| format!("can't open {file}: no such file"))?;
        if !path.is_file() {
            bail!("can't open {file}: not a file");
        }
        // The preview pane can render text, local images and Mermaid
        // diagrams. Other binaries are refused here, before IPC.
        let openable = crate::tree_browser::is_openable_file(&path)
            .with_context(|| format!("can't open {file}: unreadable"))?;
        if !openable {
            bail!(
                "can't open {file}: unsupported binary file: kete opens text, images and \
                 Mermaid diagrams; name this path in your reply instead"
            );
        }
        resolved.push(path);
    }
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running, nothing opened");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    let count = resolved.len();
    write_frame(
        &mut conn.stream,
        &ClientRequest::OpenFiles {
            req_id,
            id: AgentId(agent_id),
            paths: resolved,
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    let noun = if count == 1 { "file" } else { "files" };
    println!(
        "opened {count} {noun} in kete's file tabs, the user is looking at them now, one tab \
         each, with a preview and an editor. Don't paste their contents into your reply; carry on."
    );
    Ok(())
}

async fn session_context_connection(command: &str) -> Result<Connection> {
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running");
    };
    let conn = handshake(stream).await?;
    require_protocol(&conn, SESSION_CONTEXT_PROTOCOL, command)?;
    Ok(conn)
}

async fn fetch_sessions(conn: &mut Connection) -> Result<Vec<SessionSummary>> {
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(&mut conn.stream, &ClientRequest::ListSessions { req_id }).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionList {
                req_id: r,
                sessions,
            }) if r == req_id => return Ok(sessions),
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

pub async fn list_sessions(json: bool) -> Result<()> {
    let mut conn = session_context_connection("sessions").await?;
    let sessions = fetch_sessions(&mut conn).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "sessions": sessions }))?
        );
    } else {
        print_session_summaries(&sessions);
    }
    Ok(())
}

/// The agent session `query` names among `sessions`: its id, else its name
/// (any case), else the one name containing it. Two matches are refused
/// with their names, so a move never lands on the wrong session.
fn pick_agent_session<'a>(
    sessions: &'a [SessionSummary],
    query: &str,
) -> Result<&'a SessionSummary> {
    let q = query.trim();
    if q.is_empty() {
        bail!("session name or id is empty");
    }
    let agents: Vec<&SessionSummary> = sessions
        .iter()
        .filter(|s| matches!(s.session, SessionRef::Agent(_)))
        .collect();
    if let Some(s) = agents.iter().find(|s| s.id == q) {
        return Ok(s);
    }
    let lower = q.to_lowercase();
    for matches in [
        |s: &SessionSummary, q: &str| s.name.to_lowercase() == q,
        |s: &SessionSummary, q: &str| s.name.to_lowercase().contains(q),
    ] {
        let found: Vec<&&SessionSummary> = agents.iter().filter(|s| matches(s, &lower)).collect();
        match found.as_slice() {
            [one] => return Ok(one),
            [] => continue,
            many => bail!(
                "`{q}` matches {} sessions ({}): give its id (kete sessions lists them)",
                many.len(),
                many.iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    bail!("no agent session named `{q}`: kete sessions lists them")
}

/// CLI: `kete session move-account <session> [<profile>]` — MOVE TO
/// ACCOUNT (`SwitchAgentAccount`), as the apps' menus send it: the
/// session restarts fresh on `profile` (None: its harness's own login) in
/// the same worktree. The daemon refuses an undefined profile, one of
/// another harness, and an archived or Cloud row, before killing anything.
pub async fn move_session_account(session: &str, profile: Option<String>) -> Result<()> {
    let mut conn = session_context_connection("session move-account").await?;
    require_protocol(
        &conn,
        nebula_core::SWITCH_ACCOUNT_PROTOCOL,
        "session move-account",
    )?;
    let sessions = fetch_sessions(&mut conn).await?;
    let target = pick_agent_session(&sessions, session)?;
    let SessionRef::Agent(id) = target.session.clone() else {
        bail!("only agent sessions run on an account");
    };
    let profile = profile
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if target.profile.as_deref().map(str::trim) == profile.as_deref() {
        bail!(
            "{} already runs on {}",
            target.name,
            profile.as_deref().unwrap_or("its own login")
        );
    }
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::SwitchAgentAccount {
            req_id,
            id,
            profile: profile.clone(),
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!(
        "{} restarted fresh on {} (its conversation stays with the old account)",
        target.name,
        match &profile {
            Some(p) => format!("`{p}`"),
            None => format!("{}'s own login", target.harness),
        }
    );
    Ok(())
}

pub async fn read_session(session: &str, lines: u32) -> Result<()> {
    let mut conn = session_context_connection("read").await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::ReadSession {
            req_id,
            target: session.to_string(),
            max_lines: lines,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionText {
                req_id: r, text, ..
            }) if r == req_id => {
                print!("{text}");
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

pub async fn ask_session(
    session: &str,
    question: &str,
    timeout_secs: u64,
    wait: bool,
) -> Result<()> {
    let mut conn = session_context_connection("ask").await?;
    let question = question.trim();
    if question.is_empty() {
        bail!("question is empty");
    }
    let caller = env::non_empty(env::AGENT_ID).map(AgentId);
    let timeout_ms = timeout_secs.saturating_mul(1000);
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::AskSession {
            req_id,
            caller,
            target: session.to_string(),
            question: question.to_string(),
            timeout_ms,
            wait,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionAnswer {
                req_id: r, answer, ..
            }) if r == req_id => {
                println!("{answer}");
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

fn print_session_summaries(sessions: &[SessionSummary]) {
    for session in sessions {
        // WORKTREE PORTS: the block's first port, and what is listening.
        let mut ports = String::new();
        if let Some(port) = session.port {
            ports.push_str(&format!(" port={port}"));
        }
        if !session.live_ports.is_empty() {
            let live: Vec<String> = session.live_ports.iter().map(u16::to_string).collect();
            ports.push_str(&format!(" listening={}", live.join(",")));
        }
        println!(
            "{} name={} project={} branch={} worktree={} harness={} status={}{}{ports} summary={}",
            session.id,
            shell_word(&session.name),
            shell_word(&session.project),
            shell_word(&session.branch),
            session.worktree.display(),
            session.harness,
            session.status,
            if session.alive { " alive" } else { "" },
            shell_word(&session.summary)
        );
    }
}

fn shell_word(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        value.to_string()
    } else {
        format!("{value:?}")
    }
}

#[derive(Debug, Clone)]
struct OrchestratorTree {
    projects: Vec<Project>,
    worktrees: Vec<Worktree>,
    agents: Vec<Agent>,
    terminals: Vec<TerminalTab>,
    links: Vec<Link>,
}

async fn orchestrator_connection() -> Result<(AgentId, Connection)> {
    let agent_id = AgentId(current_agent_id("orchestrator")?);
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running");
    };
    Ok((agent_id, handshake(stream).await?))
}

async fn fetch_orchestrator_tree(
    caller: &AgentId,
    conn: &mut Connection,
) -> Result<OrchestratorTree> {
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorList {
            req_id,
            caller: caller.clone(),
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::OrchestratorList {
                req_id: r,
                projects,
                worktrees,
                agents,
                terminals,
                links,
            }) if r == req_id => {
                return Ok(OrchestratorTree {
                    projects,
                    worktrees,
                    agents,
                    terminals,
                    links,
                });
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

fn parse_session_ref(raw: &str) -> SessionRef {
    if let Some(id) = raw
        .strip_prefix("terminal:")
        .or_else(|| raw.strip_prefix("term:"))
    {
        SessionRef::Terminal(TerminalId(id.to_string()))
    } else {
        SessionRef::Agent(AgentId(raw.to_string()))
    }
}

fn resolve_project(tree: &OrchestratorTree, raw: &str) -> Result<ProjectId> {
    let matches = tree
        .projects
        .iter()
        .filter(|p| p.id.as_str() == raw || p.name == raw)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [project] => Ok(project.id.clone()),
        [] => bail!("project not found: {raw}"),
        _ => bail!("project name is ambiguous: {raw}; use its id"),
    }
}

fn resolve_worktree_or_branch(
    tree: &OrchestratorTree,
    project: &ProjectId,
    raw: &str,
) -> (Option<WorktreeId>, Option<String>) {
    match tree
        .worktrees
        .iter()
        .find(|w| &w.project_id == project && (w.id.as_str() == raw || w.branch == raw))
    {
        Some(worktree) => (Some(worktree.id.clone()), None),
        None => (None, Some(raw.to_string())),
    }
}

/// The protocol that grew orchestrator provisioning and the richer verbs.
const ORCHESTRATOR_PROTOCOL: u32 = 54;

/// What `kete orchestrator read` asks a protocol-53 daemon for: its raw
/// bounded tail, the only read it has.
const LEGACY_READ_BYTES: u32 = 8192;

async fn fetch_orchestrator_sessions(
    caller: &AgentId,
    conn: &mut Connection,
    req_id: u64,
) -> Result<Vec<OrchestratorSession>> {
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorSessions {
            req_id,
            caller: caller.clone(),
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::OrchestratorSessions {
                req_id: r,
                sessions,
            }) if r == req_id => return Ok(sessions),
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// `kete orchestrator list`: every live session in attention order. The
/// `--json` shape keeps its tree fields (`projects`, `worktrees`, `agents`,
/// `terminals`, `links`) and adds `sessions`, the attention-ordered rows.
pub async fn orchestrator_list(json: bool) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let tree = fetch_orchestrator_tree(&caller, &mut conn).await?;
    // A 53 daemon has no attention rows; the tree alone still answers.
    let sessions = if conn.protocol_version >= ORCHESTRATOR_PROTOCOL {
        Some(fetch_orchestrator_sessions(&caller, &mut conn, ONE_SHOT_REQ_ID + 1).await?)
    } else {
        None
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "projects": tree.projects,
                "worktrees": tree.worktrees,
                "agents": tree.agents,
                "terminals": tree.terminals,
                "links": tree.links,
                "sessions": sessions.unwrap_or_default(),
            }))?
        );
    } else if let Some(sessions) = sessions {
        print_attention_rows(&sessions, &caller);
    } else {
        print_orchestrator_tree(&tree);
    }
    Ok(())
}

/// `kete orchestrator read`: transcript-aware recent context by id or
/// name, falling back to the cleaned terminal tail. `--bytes` asks for
/// the raw bounded tail instead.
pub async fn orchestrator_read(session: &str, lines: u32, bytes: Option<u32>) -> Result<()> {
    if let Some(bytes) = bytes {
        return orchestrator_read_tail(session, bytes).await;
    }
    let (caller, mut conn) = orchestrator_connection().await?;
    // A 53 daemon has no transcript-aware read: its raw tail, by id.
    if conn.protocol_version < ORCHESTRATOR_PROTOCOL {
        drop(conn);
        return orchestrator_read_tail(session, LEGACY_READ_BYTES).await;
    }
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorReadSession {
            req_id,
            caller,
            target: session.to_string(),
            max_lines: lines,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::SessionText {
                req_id: r, text, ..
            }) if r == req_id => {
                print!("{text}");
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

async fn orchestrator_read_tail(session: &str, bytes: u32) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let session = parse_session_ref(session);
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorRead {
            req_id,
            caller,
            session: session.clone(),
            max_bytes: bytes,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::OutputTail {
                req_id: r,
                session: _,
                tail,
            }) if r == req_id => {
                match tail {
                    Some(tail) => print!("{}", String::from_utf8_lossy(&tail.data)),
                    None => println!("(session is not live)"),
                }
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// The reply to a request answered by `Ack { created }`: what it created.
async fn await_created(conn: &mut Connection, req_id: u64) -> Result<Option<EntityId>> {
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Ack { req_id: r, created }) if r == req_id => return Ok(created),
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// `kete orchestrator send`: a labelled follow-up, by id or name. The
/// daemon waits for a busy target (bounded by `timeout_secs`) unless
/// `wait` is off, and refuses the orchestrator itself.
pub async fn orchestrator_send(
    session: &str,
    prompt: &str,
    timeout_secs: u64,
    wait: bool,
) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let req_id = ONE_SHOT_REQ_ID;
    // A 53 daemon only knows the id-addressed send, without waiting.
    if conn.protocol_version < ORCHESTRATOR_PROTOCOL {
        write_frame(
            &mut conn.stream,
            &ClientRequest::OrchestratorSend {
                req_id,
                caller,
                target: AgentId(session.to_string()),
                message: prompt.to_string(),
            },
        )
        .await?;
        await_ack(&mut conn, req_id).await?;
        println!("sent to {session} (older daemon: by id, without waiting for idle)");
        return Ok(());
    }
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorTell {
            req_id,
            caller,
            target: session.to_string(),
            message: prompt.to_string(),
            timeout_ms: timeout_secs.saturating_mul(1000),
            wait,
        },
    )
    .await?;
    match await_created(&mut conn, req_id).await? {
        Some(EntityId::Agent(id)) => println!("sent to {id}"),
        _ => println!("sent to {session}"),
    }
    Ok(())
}

/// Everything `kete orchestrator spawn` can say about the worker.
pub struct SpawnArgs<'a> {
    pub project: &'a str,
    pub worktree: Option<&'a str>,
    pub base: Option<String>,
    pub kind: AgentKind,
    pub name: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub profile: Option<String>,
    pub task: &'a str,
    /// `--role <id>`: the role card the worker launches with. Sent only to
    /// a 66 daemon (`STUDIO_PROTOCOL`).
    pub role_card: Option<String>,
}

/// The request an orchestrator spawn sends a daemon that said `daemon`
/// at Hello: `OrchestratorSpawnWith` from 54, carrying a role card only
/// to a 66 daemon (an older one is refused here, never sent the longer
/// frame), else `OrchestratorSpawn`.
pub(crate) fn orchestrator_spawn_request(
    daemon: u32,
    req_id: u64,
    caller: AgentId,
    project: ProjectId,
    worktree: Option<WorktreeId>,
    branch: Option<String>,
    args: SpawnArgs<'_>,
) -> Result<ClientRequest> {
    if args.role_card.is_some() {
        crate::ipc_studio::require_studio(daemon, "`kete orchestrator spawn --role`")?;
    }
    let name = args.name.unwrap_or_default();
    if daemon >= ORCHESTRATOR_PROTOCOL {
        return Ok(ClientRequest::OrchestratorSpawnWith {
            req_id,
            caller,
            project,
            worktree,
            branch,
            base: args.base,
            name,
            kind: args.kind,
            custom_harness: None,
            model: args.model,
            effort: args.effort,
            profile: args.profile,
            starting_prompt: args.task.to_string(),
            role_card: args.role_card,
        });
    }
    if args.profile.is_some() {
        bail!(
            "`kete orchestrator spawn --profile` needs daemon protocol v{ORCHESTRATOR_PROTOCOL}, but the running daemon speaks v{daemon}: run `kete kill` and relaunch"
        );
    }
    Ok(ClientRequest::OrchestratorSpawn {
        req_id,
        caller,
        project,
        worktree,
        branch,
        base: args.base,
        name,
        kind: args.kind,
        custom_harness: None,
        model: args.model,
        effort: args.effort,
        starting_prompt: args.task.to_string(),
    })
}

pub async fn orchestrator_spawn(args: SpawnArgs<'_>) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let tree = fetch_orchestrator_tree(&caller, &mut conn).await?;
    let project_id = resolve_project(&tree, args.project)?;
    let (worktree_id, branch) = match args.worktree {
        Some(target) => resolve_worktree_or_branch(&tree, &project_id, target),
        None => (None, None),
    };
    let req_id = ONE_SHOT_REQ_ID + 1;
    let kind = args.kind;
    let request = orchestrator_spawn_request(
        conn.protocol_version,
        req_id,
        caller,
        project_id,
        worktree_id,
        branch,
        args,
    )?;
    write_frame(&mut conn.stream, &request).await?;
    match await_created(&mut conn, req_id).await? {
        Some(EntityId::Agent(id)) => {
            println!(
                "started {id}, a new {} session for that task",
                kind.as_str()
            )
        }
        _ => println!("started a new {} session for that task", kind.as_str()),
    }
    Ok(())
}

/// `kete orchestrator review`: one modal over every named session, each
/// by id, `terminal:<id>`, or unambiguous name.
pub async fn orchestrator_review(sessions: &[String], tabs: Vec<ReviewTabKind>) -> Result<()> {
    let (caller, mut conn) = orchestrator_connection().await?;
    let rows = if conn.protocol_version >= ORCHESTRATOR_PROTOCOL {
        fetch_orchestrator_sessions(&caller, &mut conn, ONE_SHOT_REQ_ID + 1).await?
    } else {
        Vec::new()
    };
    let refs = sessions
        .iter()
        .map(|raw| resolve_review_target(&rows, raw))
        .collect::<Result<Vec<_>>>()?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::OrchestratorOpenReview {
            req_id,
            caller,
            sessions: refs,
            tabs,
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!(
        "opened review in kete for {} session{}",
        sessions.len(),
        if sessions.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

/// A review target as the daemon names it: an exact id (or
/// `terminal:<id>`) as given, else the one row whose name matches
/// exactly, else the one whose id, name, project or branch contains it.
fn resolve_review_target(rows: &[OrchestratorSession], raw: &str) -> Result<SessionRef> {
    if rows.is_empty() || rows.iter().any(|r| r.summary.id == raw) {
        return Ok(parse_session_ref(raw));
    }
    let exact: Vec<_> = rows
        .iter()
        .filter(|r| r.summary.name.eq_ignore_ascii_case(raw))
        .collect();
    match exact.as_slice() {
        [row] => return Ok(row.summary.session.clone()),
        [] => {}
        _ => bail!("session name is ambiguous: {raw}; use an id"),
    }
    let needle = raw.to_ascii_lowercase();
    let fuzzy: Vec<_> = rows
        .iter()
        .filter(|r| {
            [
                &r.summary.id,
                &r.summary.name,
                &r.summary.project,
                &r.summary.branch,
            ]
            .iter()
            .any(|f| f.to_ascii_lowercase().contains(&needle))
        })
        .collect();
    match fuzzy.as_slice() {
        [row] => Ok(row.summary.session.clone()),
        [] => bail!("session not found: {raw}"),
        _ => bail!("session match is ambiguous: {raw}; use an id"),
    }
}

/// One line per session, attention first.
fn print_attention_rows(rows: &[OrchestratorSession], caller: &AgentId) {
    for row in rows {
        println!("{}", attention_line(row, caller));
    }
}

fn attention_line(row: &OrchestratorSession, caller: &AgentId) -> String {
    let s = &row.summary;
    let mut line = format!(
        "{:>2} {:<14} {} name={} project={} branch={} harness={}",
        row.attention,
        row.state,
        s.id,
        shell_word(&s.name),
        shell_word(&s.project),
        shell_word(&s.branch),
        s.harness,
    );
    if row.role.is_orchestrator() {
        line.push_str(if s.id == caller.as_str() {
            " role=orchestrator(you)"
        } else {
            " role=orchestrator"
        });
    }
    if let Some(parent) = &row.parent {
        line.push_str(&format!(" parent={parent}"));
    }
    if !row.subagents.is_empty() {
        let running = row
            .subagents
            .iter()
            .filter(|sub| sub.status == nebula_core::SubagentStatus::Running)
            .count();
        line.push_str(&format!(
            " subagents={running}/{} running",
            row.subagents.len()
        ));
    }
    line.push_str(&format!(" summary={}", shell_word(&s.summary)));
    line
}

fn print_orchestrator_tree(tree: &OrchestratorTree) {
    for project in &tree.projects {
        println!(
            "project {} name={} path={}",
            project.id,
            project.name,
            project.repo_path.display()
        );
        for worktree in tree.worktrees.iter().filter(|w| w.project_id == project.id) {
            println!(
                "  worktree {} branch={}{} path={}",
                worktree.id,
                worktree.branch,
                if worktree.is_main { " root" } else { "" },
                worktree.path.display()
            );
            for agent in tree.agents.iter().filter(|a| a.worktree_id == worktree.id) {
                println!(
                    "    agent {} name={} kind={} status={}{}{} role={}",
                    agent.id,
                    agent.name,
                    agent.kind.as_str(),
                    agent.status.as_str(),
                    if agent.alive { " alive" } else { "" },
                    if agent.unseen { " unseen" } else { "" },
                    agent.role.as_str()
                );
            }
            for term in tree
                .terminals
                .iter()
                .filter(|t| t.worktree_id == worktree.id)
            {
                println!(
                    "    terminal {} name={}{}{}",
                    term.id,
                    term.name,
                    if term.alive { " alive" } else { "" },
                    if term.run_command.is_some() {
                        " run"
                    } else {
                        ""
                    }
                );
            }
        }
    }
}

// ---- provisioning, from the user's own shell ----

/// The first Snapshot a Subscribe answers with: the whole tree.
async fn fetch_snapshot(
    conn: &mut Connection,
) -> Result<(Vec<Project>, Vec<Worktree>, Vec<Agent>)> {
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                projects,
                worktrees,
                agents,
                ..
            }) => return Ok((projects, worktrees, agents)),
            Some(_) => continue,
            None => bail!("daemon closed the connection before sending a snapshot"),
        }
    }
}

/// Provisioning is the user's call: refuse it from inside an agent
/// session before asking (the daemon refuses it too).
fn refuse_inside_agent_session(verb: &str) -> Result<()> {
    if env::non_empty(env::AGENT_ID).is_some() {
        bail!(
            "`kete orchestrator {verb}` is the user's call and is refused inside an agent \
             session; run it from your own shell, or press F10 in kete"
        );
    }
    Ok(())
}

/// What `kete orchestrator start` asks for.
pub struct StartArgs {
    pub kind: AgentKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub profile: Option<String>,
    pub project: Option<String>,
    pub name: Option<String>,
    pub replace: bool,
    /// `--role <id>`: the role card the orchestrator launches with. Sent
    /// only to a 66 daemon (`STUDIO_PROTOCOL`).
    pub role_card: Option<String>,
}

/// The `StartOrchestrator` a start sends a daemon that said `daemon` at
/// Hello: a role card only to a 66 daemon, an older one refused here.
pub(crate) fn start_orchestrator_request(
    daemon: u32,
    req_id: u64,
    args: &StartArgs,
    project: Option<ProjectId>,
) -> Result<ClientRequest> {
    if args.role_card.is_some() {
        crate::ipc_studio::require_studio(daemon, "`kete orchestrator start --role`")?;
    }
    Ok(ClientRequest::StartOrchestrator {
        req_id,
        kind: args.kind,
        custom_harness: None,
        model: args.model.clone(),
        effort: args.effort.clone(),
        profile: args.profile.clone(),
        project,
        name: args.name.clone(),
        replace: args.replace,
        role_card: args.role_card.clone(),
    })
}

/// `kete orchestrator start`: start the machine's one orchestrator, in
/// the named project's root worktree or its own workspace under the data
/// dir. Spawns the daemon when none runs.
pub async fn orchestrator_start(args: StartArgs) -> Result<()> {
    refuse_inside_agent_session("start")?;
    let mut conn = connect_or_spawn().await?;
    require_protocol(&conn, ORCHESTRATOR_PROTOCOL, "orchestrator start")?;
    let project = match args.project.as_deref() {
        Some(raw) => {
            let (projects, _, _) = fetch_snapshot(&mut conn).await?;
            let matches: Vec<&Project> = projects
                .iter()
                .filter(|p| p.id.as_str() == raw || p.name == raw)
                .collect();
            match matches.as_slice() {
                [p] => Some(p.id.clone()),
                [] => bail!("project not found: {raw}"),
                _ => bail!("project name is ambiguous: {raw}; use its id"),
            }
        }
        None => None,
    };
    let req_id = ONE_SHOT_REQ_ID + 1;
    let request = start_orchestrator_request(conn.protocol_version, req_id, &args, project)?;
    write_frame(&mut conn.stream, &request).await?;
    match await_created(&mut conn, req_id).await? {
        Some(EntityId::Agent(id)) => println!(
            "started the orchestrator {id} ({}); press F10 in kete to open it",
            args.kind.as_str()
        ),
        _ => println!("started the orchestrator; press F10 in kete to open it"),
    }
    Ok(())
}

/// `kete orchestrator clear`: unpin the orchestrator. Its session keeps
/// running, as an ordinary worker.
pub async fn orchestrator_clear() -> Result<()> {
    refuse_inside_agent_session("clear")?;
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running");
    };
    let mut conn = handshake(stream).await?;
    require_protocol(&conn, ORCHESTRATOR_PROTOCOL, "orchestrator clear")?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::ClearOrchestrator { req_id },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!("no session is the orchestrator now");
    Ok(())
}

/// `kete orchestrator status`: which session is the orchestrator, where it
/// runs and what it is doing. Works from any shell and on any protocol.
pub async fn orchestrator_status(json: bool) -> Result<()> {
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running");
    };
    let mut conn = handshake(stream).await?;
    let (projects, worktrees, agents) = fetch_snapshot(&mut conn).await?;
    let pinned = agents.iter().find(|a| a.role.is_orchestrator());
    let worktree = pinned.and_then(|a| worktrees.iter().find(|w| w.id == a.worktree_id));
    let project = worktree.and_then(|w| projects.iter().find(|p| p.id == w.project_id));
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "orchestrator": pinned,
                "worktree": worktree,
                "project": project,
            }))?
        );
        return Ok(());
    }
    match pinned {
        None => println!(
            "no orchestrator: start one with `kete orchestrator start`, or press F10 in kete"
        ),
        Some(a) => println!(
            "orchestrator {} name={} harness={} status={}{}{} project={} branch={} path={}",
            a.id,
            shell_word(&a.name),
            a.kind.as_str(),
            a.status.as_str(),
            if a.alive { " alive" } else { "" },
            if a.archived { " archived" } else { "" },
            project.map(|p| shell_word(&p.name)).unwrap_or_default(),
            worktree.map(|w| shell_word(&w.branch)).unwrap_or_default(),
            worktree
                .map(|w| w.path.display().to_string())
                .unwrap_or_default(),
        ),
    }
    Ok(())
}

/// CLI: `nebula worktree [name] [--base <ref>]` from inside an agent
/// session — take (or create) the named worktree of this session's project
/// and have the daemon move the session into it. A blank name is invented
/// the way the TUI's new-worktree prompt invents one, and spaces slugify to
/// hyphens the same way. Never spawns a daemon: no daemon means no session
/// to move.
///
/// What this prints is read by the model that ran it, so the relocating
/// case spells out what happens next: the daemon kills and resumes this
/// very process once the turn ends, and the answer has to be finished by
/// then. A daemon-side failure is a nonzero exit — the model reports it and
/// stays put.
pub async fn enter_worktree_for_current_agent(name: &str, base: Option<String>) -> Result<()> {
    let agent_id = current_agent_id("worktree")?;
    let branch = match crate::branch_name::slugify(name) {
        slug if slug.is_empty() => crate::branch_name::random_name(&[]),
        slug => slug,
    };
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        bail!("no kete daemon is running, nothing to move");
    };
    let mut conn = handshake(stream).await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::EnterWorktree {
            req_id,
            id: AgentId(agent_id),
            branch,
            base,
        },
    )
    .await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::WorktreeEntered {
                req_id: r,
                worktree,
                outcome,
            }) if r == req_id => {
                println!(
                    "worktree \"{}\" is ready at {}",
                    worktree.branch,
                    worktree.path.display()
                );
                match outcome {
                    EnterOutcome::AlreadyThere => {
                        println!("this session already runs inside it, nothing to move.");
                    }
                    EnterOutcome::Relocating => {
                        println!(
                            "this session is now associated with it; kete will relocate the \
                             session into it the moment this turn ends."
                        );
                        println!(
                            "Finish now: tell the user in one line that the session is moving \
                             into the worktree, and make no further tool calls or edits: you \
                             will be resumed inside the worktree with a prompt to continue."
                        );
                    }
                    EnterOutcome::NextLaunch => {
                        println!(
                            "this session is now associated with it and runs there from its \
                             next launch."
                        );
                    }
                }
                return Ok(());
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == req_id => bail!("{message}"),
            Some(_) => continue,
            None => bail!("{CLOSED_BEFORE_REPLY}"),
        }
    }
}

/// One-shot client for `nebula add <dir>` (and bare `nebula <dir>`): resolve
/// the path locally — the daemon's cwd is not ours, so relative paths must be
/// absolutized here — and ask the daemon to register it as a project. The
/// daemon owns the rest: normalizing to the repo toplevel, naming the project
/// after the directory, rejecting non-repos and duplicates. Spawns a daemon
/// when none is running, same as launching the TUI would.
pub async fn add_project(path: &str) -> Result<()> {
    let expanded = match (path.strip_prefix("~/"), env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => std::path::PathBuf::from(path),
    };
    let dir = paths::canonical(&expanded)
        .with_context(|| format!("{} does not exist", expanded.display()))?;
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    let mut conn = connect_or_spawn().await?;
    let req_id = ONE_SHOT_REQ_ID;
    write_frame(
        &mut conn.stream,
        &ClientRequest::AddProject {
            req_id,
            path: dir.clone(),
            name: None,
            create_missing: false,
        },
    )
    .await?;
    await_ack(&mut conn, req_id).await?;
    println!("added project {}", dir.display());
    Ok(())
}

/// Ask a running daemon to shut down. Ok(false) when none is running.
///
/// A daemon on a different protocol version closes the socket right after
/// the handshake, so `Shutdown` can never reach it — exactly the situation
/// `nebula kill` exists to fix. Fall back to SIGTERM, which its handler turns
/// into the same clean shutdown, at the pid the kernel says is listening on
/// the socket — the very daemon the handshake just failed with. Only when the
/// kernel can't say does the pidfile decide, guarded by the daemon's flock so
/// a stale pid is never signalled. The pidfile can't come first: macOS's
/// tmp_cleaner deletes regular files in /tmp untouched for three days but
/// spares sockets, so a daemon that has been up that long — on any version
/// before the one that refreshes its pidfile — is still listening with no
/// pidfile beside it (#68), and a refreshing daemon re-creates the file
/// locked a moment before it writes the pid in.
pub async fn kill_daemon() -> Result<bool> {
    kill_daemon_at(&paths::socket_path(), &paths::pidfile_path()).await
}

async fn kill_daemon_at(sock: &std::path::Path, pidfile: &std::path::Path) -> Result<bool> {
    let Ok(stream) = try_connect(sock).await else {
        // Nothing listening — but a wedged or mid-boot daemon may still hold
        // the pidfile lock; fall through to the same check.
        return kill_by_pidfile(pidfile).await;
    };
    let listener_pid = peer_pid(&stream);
    if let Ok(mut conn) = handshake(stream).await {
        write_frame(&mut conn.stream, &ClientRequest::Shutdown).await?;
        wait_for_daemon_exit(pidfile, conn.daemon_pid as i32).await;
        return Ok(true);
    }
    let Some(pid) = listener_pid else {
        if kill_by_pidfile(pidfile).await? {
            return Ok(true);
        }
        bail!(
            "a kete daemon is listening on {} but this build can't talk to it or find its \
             pid: look it up with `pgrep -f 'kete daemon'` and stop it with `kill <pid>` \
             (never `kill -9`, which skips flushing its database)",
            sock.display()
        );
    };
    terminate(pid)?;
    wait_for_daemon_exit(pidfile, pid).await;
    Ok(true)
}

/// Outcome of `shutdown_if_idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleShutdown {
    /// Nothing listening on the socket.
    NoDaemon,
    /// The daemon held no live PTYs and was shut down cleanly.
    ShutDown,
    /// Live sessions exist; the daemon was left running.
    SessionsLive { count: usize },
    /// A daemon is listening but its protocol version differs, so its
    /// session state can't be inspected.
    Skewed,
}

/// Shut the daemon down only when it holds no live PTYs — the post-upgrade
/// handoff. An idle daemon can die safely (the next client launch spawns one
/// from the new binary on disk); live sessions would be killed with it, so
/// their daemon is left alone and the restart stays the user's call.
pub async fn shutdown_if_idle() -> Result<IdleShutdown> {
    let sock = paths::socket_path();
    let Ok(stream) = try_connect(&sock).await else {
        return Ok(IdleShutdown::NoDaemon);
    };
    let Ok(mut conn) = handshake(stream).await else {
        return Ok(IdleShutdown::Skewed);
    };
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                agents, terminals, ..
            }) => {
                let live = agents.iter().filter(|a| a.alive).count()
                    + terminals.iter().filter(|t| t.alive).count();
                if live > 0 {
                    return Ok(IdleShutdown::SessionsLive { count: live });
                }
                write_frame(&mut conn.stream, &ClientRequest::Shutdown).await?;
                wait_for_daemon_exit(&paths::pidfile_path(), conn.daemon_pid as i32).await;
                return Ok(IdleShutdown::ShutDown);
            }
            Some(_) => continue,
            None => bail!("daemon closed the connection before sending a snapshot"),
        }
    }
}

/// SIGTERM the daemon recorded in the pidfile (its SIGTERM handler runs the
/// same clean shutdown as `Shutdown`). Ok(false) when no daemon holds it.
async fn kill_by_pidfile(path: &std::path::Path) -> Result<bool> {
    if !daemon_holds_pidfile_lock(path) {
        return Ok(false);
    }
    let pid: i32 = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|pid| *pid > 0)
        .context("daemon is running but its pidfile is unreadable: kill it manually")?;
    terminate(pid)?;
    wait_for_daemon_exit(path, pid).await;
    Ok(true)
}

/// SIGTERM `pid`. A daemon that exits between being found and being
/// signalled — mid-shutdown under a second `nebula kill` — has done what
/// was asked, so only a process still running makes a failed signal an error.
#[cfg(unix)]
fn terminate(pid: i32) -> Result<()> {
    if send_signal(pid, SIGTERM) != 0 && process_running(pid) {
        bail!("failed to signal daemon pid {pid}: kill it manually");
    }
    Ok(())
}

/// Liveness = flock possession (mirrors the daemon's PidfileLock): if we can
/// take the lock ourselves, nobody holds it. Released on drop.
#[cfg(unix)]
fn daemon_holds_pidfile_lock(path: &std::path::Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    else {
        return false;
    };
    flock_try_exclusive(file.as_raw_fd()) != 0
}

/// Poll until the daemon has released its pidfile lock and exited, so a
/// relaunch right after `nebula kill` can't race the old daemon's teardown —
/// which ends by unlinking the socket path, a new daemon's socket included.
/// The process is watched as well as the lock because the lock may be on a
/// pidfile the tmp cleaner already deleted, where no client can see it.
async fn wait_for_daemon_exit(pidfile: &std::path::Path, pid: i32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while (daemon_holds_pidfile_lock(pidfile) || process_running(pid))
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(POLL_STEP).await;
    }
}

/// Whether `pid` has yet to exit. A zombie still answers signal 0, but it
/// has exited and holds nothing — it is only waiting for its parent to reap
/// it, and a daemon's parent is whichever TUI spawned it, possibly still open.
#[cfg(unix)]
fn process_running(pid: i32) -> bool {
    if send_signal(pid, 0) != 0 {
        return false;
    }
    // Linux: the state letter follows the parenthesised command name, which
    // can itself hold spaces and parens — hence the last `)`. No `ps` needed.
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next());
        return state != Some('Z');
    }
    match std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) => !String::from_utf8_lossy(&out.stdout)
            .trim_start()
            .starts_with('Z'),
        Err(_) => true,
    }
}

/// Windows: there is no SIGTERM for a detached process, so a daemon this
/// build cannot talk to is ended outright. Only reached once `Shutdown`
/// over the pipe has failed — a daemon on another protocol.
#[cfg(windows)]
fn terminate(pid: i32) -> Result<()> {
    let pid = pid as u32;
    if nebula_core::platform::terminate_process(pid).is_err()
        && nebula_core::platform::process_running(pid)
    {
        bail!("failed to stop daemon pid {pid}: end it from Task Manager");
    }
    Ok(())
}

/// Windows liveness mirrors the daemon's `PidfileLock`: the lock is on the
/// sibling `daemon.lock`, and taking it ourselves means nobody holds it.
#[cfg(windows)]
fn daemon_holds_pidfile_lock(path: &std::path::Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(paths::lock_path_for(path))
    else {
        return false;
    };
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

#[cfg(windows)]
fn process_running(pid: i32) -> bool {
    pid > 0 && nebula_core::platform::process_running(pid as u32)
}

// Tiny extern shims, same dep-light idiom as nebula_core::paths.
#[cfg(unix)]
fn flock_try_exclusive(fd: i32) -> i32 {
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    unsafe { flock(fd, LOCK_EX | LOCK_NB) }
}

#[cfg(unix)]
const SIGTERM: i32 = 15;

#[cfg(unix)]
fn send_signal(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, sig) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal names a way out the platform has: Windows has no
    /// `kete reload`, and its `kete kill` stops the asking session too.
    #[test]
    fn spawn_profile_on_an_old_daemon_names_the_platforms_way_out() {
        let unix = spawn_profile_too_old(57, false);
        assert!(unix.contains("v57") && unix.contains("no session started"));
        assert!(unix.contains("`kete reload`") && !unix.contains("kete kill"));
        let windows = spawn_profile_too_old(57, true);
        assert!(!windows.contains("kete reload"), "{windows}");
        assert!(windows.contains("`kete kill`") && windows.contains("stops every session"));
        assert!(windows.contains("without --profile"));
    }

    /// 66 widens three requests by a trailing `role_card` and adds
    /// `SpawnSiblingAgentWith`: a launch without a role is the frame an
    /// older daemon reads, and a 65 daemon is never sent a role card.
    #[test]
    fn a_65_daemon_is_never_sent_a_role_card() {
        let old = nebula_core::STUDIO_PROTOCOL - 1;
        let new = nebula_core::STUDIO_PROTOCOL;
        let sibling = |daemon: u32, profile: Option<&str>, role: Option<&str>| {
            sibling_spawn_request(
                daemon,
                1,
                AgentId("a1".into()),
                None,
                "task".into(),
                None,
                None,
                profile.map(str::to_string),
                role.map(str::to_string),
            )
        };
        assert!(matches!(
            sibling(old, Some("work"), None).unwrap(),
            ClientRequest::SpawnSiblingAgent {
                profile: Some(_),
                ..
            }
        ));
        let refused = sibling(old, None, Some("runner")).unwrap_err().to_string();
        assert!(refused.contains("needs the 0.60 daemon"), "{refused}");
        let ClientRequest::SpawnSiblingAgentWith {
            role_card, profile, ..
        } = sibling(new, None, Some("runner")).unwrap()
        else {
            panic!("a role goes in SpawnSiblingAgentWith");
        };
        assert_eq!((role_card.as_str(), profile), ("runner", None));
        assert!(matches!(
            sibling(new, None, None).unwrap(),
            ClientRequest::SpawnSiblingAgent { .. }
        ));

        let spawn = |daemon: u32, role: Option<&str>| {
            orchestrator_spawn_request(
                daemon,
                2,
                AgentId("o".into()),
                ProjectId("p".into()),
                None,
                None,
                SpawnArgs {
                    project: "p",
                    worktree: None,
                    base: None,
                    kind: AgentKind::Claude,
                    name: None,
                    model: None,
                    effort: None,
                    profile: None,
                    task: "task",
                    role_card: role.map(str::to_string),
                },
            )
        };
        let ClientRequest::OrchestratorSpawnWith { role_card, .. } = spawn(old, None).unwrap()
        else {
            panic!("a 54+ daemon takes OrchestratorSpawnWith");
        };
        assert_eq!(role_card, None, "the 65 frame");
        assert!(spawn(old, Some("runner")).is_err());
        let ClientRequest::OrchestratorSpawnWith { role_card, .. } =
            spawn(new, Some("runner")).unwrap()
        else {
            panic!("not an OrchestratorSpawnWith");
        };
        assert_eq!(role_card.as_deref(), Some("runner"));

        let start = |role: Option<&str>| StartArgs {
            kind: AgentKind::Claude,
            model: None,
            effort: None,
            profile: None,
            project: None,
            name: None,
            replace: false,
            role_card: role.map(str::to_string),
        };
        let ClientRequest::StartOrchestrator { role_card, .. } =
            start_orchestrator_request(old, 3, &start(None), None).unwrap()
        else {
            panic!("not a StartOrchestrator");
        };
        assert_eq!(role_card, None);
        assert!(start_orchestrator_request(old, 3, &start(Some("manager")), None).is_err());
        let ClientRequest::StartOrchestrator { role_card, .. } =
            start_orchestrator_request(new, 3, &start(Some("manager")), None).unwrap()
        else {
            panic!("not a StartOrchestrator");
        };
        assert_eq!(role_card.as_deref(), Some("manager"));
    }

    #[test]
    fn a_move_names_one_agent_session_or_refuses() {
        let row = |id: &str, name: &str| attention_row(id, name, "main", 0).summary;
        let mut term = row("t1", "fix shell");
        term.session = SessionRef::Terminal(TerminalId("t1".into()));
        let sessions = [
            row("a1", "fix-auth"),
            row("a2", "fix-auth-tests"),
            row("a3", "Docs"),
            term,
        ];
        let pick = |q: &str| pick_agent_session(&sessions, q).map(|s| s.id.clone());
        assert_eq!(pick("a2").unwrap(), "a2", "by id");
        assert_eq!(
            pick("fix-auth").unwrap(),
            "a1",
            "an exact name beats a longer one"
        );
        assert_eq!(pick("docs").unwrap(), "a3", "any case");
        assert_eq!(pick("tests").unwrap(), "a2", "the one name containing it");
        let many = pick("fix").unwrap_err().to_string();
        assert!(many.contains("matches 2 sessions"), "{many}");
        assert!(pick("shell").is_err(), "a terminal runs on no account");
        assert!(pick("  ").is_err());
    }

    fn attention_row(id: &str, name: &str, branch: &str, attention: u32) -> OrchestratorSession {
        OrchestratorSession {
            summary: SessionSummary {
                session: SessionRef::Agent(AgentId(id.into())),
                id: id.into(),
                name: name.into(),
                project_id: ProjectId("p".into()),
                project: "demo".into(),
                worktree_id: WorktreeId("w".into()),
                worktree: "/w".into(),
                branch: branch.into(),
                harness: "claude".into(),
                status: "idle".into(),
                alive: true,
                summary: "fix the login".into(),
                profile: None,
                port: None,
                live_ports: Vec::new(),
            },
            attention,
            state: "unread".into(),
            role: nebula_core::AgentRole::Worker,
            parent: Some(AgentId("orch".into())),
            status_changed_at: 0,
            subagents: Vec::new(),
        }
    }

    #[test]
    fn review_targets_resolve_by_id_name_or_unambiguous_fragment() {
        let rows = vec![
            attention_row("a1", "agent-1", "fix-login", 0),
            attention_row("a2", "agent-2", "main", 1),
        ];
        let agent = |id: &str| SessionRef::Agent(AgentId(id.into()));
        assert_eq!(resolve_review_target(&rows, "a2").unwrap(), agent("a2"));
        assert_eq!(
            resolve_review_target(&rows, "AGENT-1").unwrap(),
            agent("a1")
        );
        assert_eq!(resolve_review_target(&rows, "login").unwrap(), agent("a1"));
        assert!(resolve_review_target(&rows, "agent")
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
        assert!(resolve_review_target(&rows, "nope")
            .unwrap_err()
            .to_string()
            .contains("not found"));
        assert_eq!(
            resolve_review_target(&rows, "terminal:t1")
                .unwrap_err()
                .to_string(),
            "session not found: terminal:t1"
        );
        // A 53 daemon gave no rows: the raw id goes through as before.
        assert_eq!(
            resolve_review_target(&[], "terminal:t1").unwrap(),
            SessionRef::Terminal(TerminalId("t1".into()))
        );
    }

    #[test]
    fn attention_lines_lead_with_rank_and_state_and_name_the_parent() {
        let mut row = attention_row("a1", "agent-1", "fix-login", 3);
        let line = attention_line(&row, &AgentId("orch".into()));
        assert!(line.starts_with(" 3 unread"), "{line}");
        assert!(line.contains("a1 name=agent-1 project=demo branch=fix-login harness=claude"));
        assert!(line.contains("parent=orch"));
        row.role = nebula_core::AgentRole::Orchestrator;
        let line = attention_line(&row, &AgentId("a1".into()));
        assert!(line.contains("role=orchestrator(you)"), "{line}");
    }

    // Unset and empty are the same miss, and the error has to name the
    // command the model just ran so it knows why it can't work here.
    #[test]
    fn agent_id_requires_a_non_empty_value_and_names_the_verb() {
        let err = agent_id_from(None, "rename").unwrap_err().to_string();
        assert!(err.contains("`kete rename`"), "{err}");
        assert!(err.contains(env::AGENT_ID), "{err}");
        let err = agent_id_from(Some(String::new()), "worktree")
            .unwrap_err()
            .to_string();
        assert!(err.contains("`kete worktree`"), "{err}");
        assert_eq!(agent_id_from(Some("a1".into()), "rename").unwrap(), "a1");
    }

    // The whole point of the message: `nebula kill` is the fix for exactly
    // one of the two skews, and recommending it for the other sends the user
    // in a circle (kill the daemon, the live TUI respawns the same one).
    #[test]
    fn skew_message_blames_the_older_side() {
        let daemon_ahead = version_skew_message(PROTOCOL_VERSION + 2, None);
        assert!(daemon_ahead.contains("This client is the older build"));
        assert!(
            !daemon_ahead.contains("run `kete kill` and relaunch"),
            "must not send the user to kill a daemon that is not the stale side: {daemon_ahead}"
        );
        assert!(daemon_ahead.contains("make install"));

        let daemon_behind = version_skew_message(PROTOCOL_VERSION - 1, None);
        assert!(daemon_behind.contains("The daemon is the older build"));
        assert!(daemon_behind.contains("run `kete kill` and relaunch"));
    }

    // #68: with the daemon's pid known, the stale-daemon message names it
    // and hands over the manual fallback — SIGTERM, with -9 ruled out.
    #[test]
    fn skew_message_offers_a_clean_manual_kill_by_pid() {
        let msg = version_skew_message(PROTOCOL_VERSION - 1, Some(4242));
        assert!(msg.contains("(pid 4242)"), "{msg}");
        assert!(msg.contains("`kill 4242`"), "{msg}");
        assert!(msg.contains("never `kill -9`"), "{msg}");

        let unknown = version_skew_message(PROTOCOL_VERSION - 1, None);
        assert!(unknown.contains("the daemon:  unknown"), "{unknown}");
        assert!(!unknown.contains("`kill "), "no pid, no command: {unknown}");
    }

    /// Env var that turns [`fake_skewed_daemon`] from a no-op into a daemon.
    #[cfg(unix)]
    const FAKE_DAEMON_SOCK: &str = "NEBULA_TEST_FAKE_DAEMON_SOCK";

    /// A child re-exec of this test binary playing the daemon from #68: on
    /// an older protocol, with no pidfile, answering every `Hello` with
    /// `Incompatible` and hanging up — so only a signal stops it. A no-op
    /// when run any other way.
    #[cfg(unix)]
    #[test]
    #[ignore = "run as a child process by kill_stops_a_skewed_daemon_whose_pidfile_is_gone"]
    fn fake_skewed_daemon() {
        let Some(sock) = std::env::var_os(FAKE_DAEMON_SOCK) else {
            return;
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener = tokio::net::UnixListener::bind(sock).unwrap();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let _ = read_frame::<ClientRequest, _>(&mut stream).await;
                let _ = write_frame(
                    &mut stream,
                    &ServerEvent::Incompatible {
                        daemon_protocol_version: PROTOCOL_VERSION - 1,
                    },
                )
                .await;
            }
        });
    }

    /// Kills the child if the test fails before it has exited.
    #[cfg(unix)]
    struct Reap(std::process::Child);

    #[cfg(unix)]
    impl Drop for Reap {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    // #68: the pidfile is gone (macOS's tmp_cleaner) and the daemon speaks a
    // protocol this build doesn't, so neither `Shutdown` over the socket nor
    // SIGTERM via the pidfile can reach it — and `nebula kill` used to report
    // "no nebula daemon running" about a daemon still listening.
    #[cfg(unix)]
    #[tokio::test]
    async fn kill_stops_a_skewed_daemon_whose_pidfile_is_gone() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("daemon.sock");
        let pidfile = dir.path().join("daemon.pid");
        let mut daemon = Reap(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ipc::tests::fake_skewed_daemon", "--ignored"])
                .env(FAKE_DAEMON_SOCK, &sock)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = daemon.0.id();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match UnixStream::connect(&sock).await {
                Ok(stream) => break stream,
                Err(e) if tokio::time::Instant::now() > deadline => {
                    panic!("fake daemon never listened: {e}")
                }
                Err(_) => tokio::time::sleep(POLL_STEP).await,
            }
        };

        // What the TUI shows first: the pid, straight off the socket.
        let err = handshake(stream).await.err().expect("skewed").to_string();
        assert!(err.contains(&format!("`kill {pid}`")), "{err}");

        let started = std::time::Instant::now();
        assert!(
            kill_daemon_at(&sock, &pidfile).await.unwrap(),
            "a listening daemon is not \"no daemon running\""
        );
        // Its parent — this test — hasn't reaped it, so it is a zombie now:
        // exited, but still answering signal 0. The wait must count that as
        // gone rather than sit out its deadline.
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "kill waited {:?}",
            started.elapsed()
        );
        let status = {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(status) = daemon.0.try_wait().unwrap() {
                    break status;
                }
                assert!(tokio::time::Instant::now() < deadline, "daemon exited");
                tokio::time::sleep(POLL_STEP).await;
            }
        };
        assert_eq!(status.signal(), Some(SIGTERM), "{status:?}");
    }

    #[test]
    fn skew_message_names_both_binaries() {
        let msg = version_skew_message(PROTOCOL_VERSION + 1, None);
        assert!(msg.contains("this client: "), "{msg}");
        assert!(msg.contains("the daemon:  "), "{msg}");
        // current_exe resolves under a test binary, so this half is never
        // the "unknown" fallback.
        assert!(msg.contains(&format!("v{PROTOCOL_VERSION}")), "{msg}");
    }
}
