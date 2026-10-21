//! Windows end-to-end flows of the 0.60 queue lane, every one named `queue_*`.
//!
//! A repo with three inbox notes (high, normal, one for Jay), studio mode
//! on, and the fake agent as an idle orchestrator. The idle note reaches
//! the orchestrator's pane: the first one is its first prompt (the agent's
//! log), later ones arrive at its `echo` prompt (the echo log).

use super::agent;
use super::harness::*;
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::{Agent, AgentId, AgentRole, ClientRequest, ProjectId, ServerEvent, SessionRef};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

pub const TESTS: &[(&str, fn())] = &[
    (
        "queue_idle_note_once_then_on_an_edit",
        queue_idle_note_once_then_on_an_edit,
    ),
    ("queue_typing_holds_the_note", queue_typing_holds_the_note),
    ("queue_a0_refuses_every_spawn", queue_a0_refuses_every_spawn),
    (
        "queue_status_json_shows_the_counts",
        queue_status_json_shows_the_counts,
    ),
];

/// How long a flow waits for the runner to send: the 30 s idle wait the
/// flows set, plus a cold start.
const SEND_WAIT: Duration = Duration::from_secs(90);

fn text(out: &std::process::Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `kete <args>`, which must succeed; its stdout.
fn ok(tui: &Tui, args: &[&str]) -> String {
    let out = tui.kete(args);
    assert!(out.status.success(), "kete {args:?}\n{}", text(&out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn note(dir: &Path, id: &str, to: &str, priority: &str) {
    std::fs::write(
        dir.join(format!("{id}.md")),
        format!(
            "---\nid: {id}\nstatus: open\nto: {to}\npriority: {priority}\ntitle: Note {id}\n---\nBody.\n"
        ),
    )
    .unwrap();
}

/// A TUI over a repo `game` with three inbox notes, added to Kete, studio
/// on at A1 with a 30 s idle wait.
fn studio_repo() -> (Tui, std::path::PathBuf) {
    let tui = Tui::spawn(Options::default());
    tui.wait_for_text("create your first project");
    let repo = tui.make_repo("game");
    let inbox = repo.join("notes").join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    note(&inbox, "2026-10-21-storm-sky", "orchestrator", "high");
    note(&inbox, "2026-10-21-ledger", "orchestrator", "normal");
    note(&inbox, "2026-10-21-ask-jay", "jay", "high");
    // The runner reads the settings on every pass: the shortest idle wait.
    let config = tui.data_dir.join(nebula_core::paths::CONFIG_FILE_NAME);
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    json["studio_idle_after"] = "30s".into();
    std::fs::write(&config, json.to_string()).unwrap();
    ok(&tui, &["add", &repo.display().to_string()]);
    tui.wait_for_text("game");
    let said = ok(
        &tui,
        &["studio", "enable", "game", "--inbox", "notes/inbox"],
    );
    assert!(said.contains("Studio on for game at A1 (Plan)"), "{said}");
    (tui, repo)
}

/// Start the orchestrator in `game` and return its id.
fn start_orchestrator(tui: &Tui) -> String {
    ok(tui, &["orchestrator", "start", "--project", "game"]);
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(orch) = agents(&tui.runtime_dir)
            .into_iter()
            .find(|a| a.role == AgentRole::Orchestrator)
        {
            return orch.id.0;
        }
        assert!(Instant::now() < deadline, "no orchestrator came up");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `kete studio queue --now` until it says it sent, within [`SEND_WAIT`].
fn send_now(tui: &Tui) {
    let deadline = Instant::now() + SEND_WAIT;
    loop {
        let said = ok(tui, &["studio", "queue", "--now"]);
        if said.contains("sent the idle note") {
            return;
        }
        assert!(Instant::now() < deadline, "the runner never sent: {said}");
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Bump a note's modification time: an edit, as far as the runner knows.
fn touch(path: &Path) {
    let later = std::time::SystemTime::now() + Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

fn read(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned()
}

/// Wait until `path` holds `needle`; its text.
fn wait_in_file(path: &Path, needle: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let got = read(path);
        if got.contains(needle) {
            return got;
        }
        assert!(
            Instant::now() < deadline,
            "{needle:?} never reached {path:?}:\n{got}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn queue_idle_note_once_then_on_an_edit() {
    let (tui, repo) = studio_repo();
    start_orchestrator(&tui);

    // The first idle note is the orchestrator's first prompt, high first,
    // nothing for Jay.
    send_now(&tui);
    // The agent logs its prompt as `prompt "<text>"`; its args line also
    // names the studio, in the orchestrator's guidance.
    let deadline = Instant::now() + WAIT;
    let prompt = loop {
        let log = read(&tui.agent_log);
        if let Some(line) = log
            .lines()
            .find(|l| l.contains("] prompt ") && l.contains("[from kete studio]"))
        {
            break line.to_string();
        }
        assert!(
            Instant::now() < deadline,
            "no idle note as a prompt:
{log}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    println!("idle note as the e2e saw it: {prompt}");
    assert!(prompt.contains("game A1 · 2 new inbox notes"), "{prompt}");
    let high = prompt
        .find("notes/inbox/2026-10-21-storm-sky.md")
        .expect(&prompt);
    let normal = prompt
        .find("notes/inbox/2026-10-21-ledger.md")
        .expect(&prompt);
    assert!(high < normal, "{prompt}");
    assert!(!prompt.contains("ask-jay"), "{prompt}");
    assert!(prompt.contains("Act within A1 (Plan)."), "{prompt}");

    // A second --now sends nothing.
    let said = ok(&tui, &["studio", "queue", "--now"]);
    assert!(said.contains("not sent, holding: nothing new"), "{said}");

    // An edited note is announced again, alone, at the echo prompt.
    touch(&repo.join("notes/inbox/2026-10-21-ledger.md"));
    send_now(&tui);
    let echo = wait_in_file(&tui.echo_log, "[from kete studio]");
    println!("re-announced idle note as the e2e saw it: {echo:?}");
    assert!(echo.contains("game A1 · 1 new inbox note"), "{echo}");
    assert!(echo.contains("notes/inbox/2026-10-21-ledger.md"), "{echo}");
    assert!(!echo.contains("storm-sky"), "{echo}");
}

fn queue_typing_holds_the_note() {
    let (tui, repo) = studio_repo();
    let orch = start_orchestrator(&tui);
    send_now(&tui);
    // Its turn over: at the echo prompt, idle again.
    wait_in_file(&tui.agent_log, "step echo");
    touch(&repo.join("notes/inbox/2026-10-21-storm-sky.md"));
    // A client types into the orchestrator: the runner holds for 120 s.
    call(
        &tui.runtime_dir,
        vec![ClientRequest::Input {
            session: SessionRef::Agent(AgentId(orch)),
            data: b"x".to_vec(),
        }],
    );
    let said = ok(&tui, &["studio", "queue", "--now"]);
    assert!(said.contains("not sent, holding: you typed"), "{said}");
}

fn queue_a0_refuses_every_spawn() {
    let (tui, _repo) = studio_repo();
    let orch = start_orchestrator(&tui);
    let said = ok(&tui, &["studio", "autonomy", "game", "A0"]);
    assert!(said.contains("game: A0 (Hold)"), "{said}");
    let before = agents(&tui.runtime_dir).len();
    let project = agents_project(&tui.runtime_dir);
    let caller = AgentId(orch.clone());

    // Each of the four spawn requests the A0 gate stands in front of.
    let spawn = |req_id| ClientRequest::OrchestratorSpawn {
        req_id,
        caller: caller.clone(),
        project: project.clone(),
        worktree: None,
        branch: None,
        base: None,
        name: String::new(),
        kind: nebula_core::AgentKind::Claude,
        custom_harness: None,
        model: None,
        effort: None,
        starting_prompt: "x".into(),
    };
    let spawn_with = |req_id| ClientRequest::OrchestratorSpawnWith {
        req_id,
        caller: caller.clone(),
        project: project.clone(),
        worktree: None,
        branch: None,
        base: None,
        name: String::new(),
        kind: nebula_core::AgentKind::Claude,
        custom_harness: None,
        model: None,
        effort: None,
        profile: None,
        starting_prompt: "x".into(),
        role_card: None,
    };
    let sibling = |req_id| ClientRequest::SpawnSiblingAgent {
        req_id,
        id: caller.clone(),
        kind: None,
        starting_prompt: "x".into(),
        worktree: None,
        base: None,
        profile: None,
    };
    let sibling_with = |req_id| ClientRequest::SpawnSiblingAgentWith {
        req_id,
        id: caller.clone(),
        kind: None,
        starting_prompt: "x".into(),
        worktree: None,
        base: None,
        profile: None,
        role_card: "runner".into(),
    };
    let replies = call(
        &tui.runtime_dir,
        vec![spawn(11), spawn_with(12), sibling(13), sibling_with(14)],
    );
    for req in [11, 12, 13, 14] {
        let err = replies.get(&req).cloned().flatten();
        assert_eq!(
            err.as_deref(),
            Some("autonomy A0 (Hold) for game"),
            "request {req}: {replies:?}"
        );
    }

    // The orchestrator's own command, as it runs it.
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_kete"));
    cmd.args(["orchestrator", "spawn", "--project", "game", "x"])
        .env(nebula_core::env::RUNTIME_DIR, &tui.runtime_dir)
        .env(nebula_core::env::DATA_DIR, &tui.data_dir)
        .env(nebula_core::env::WAKE_NO_TASK, "1")
        .env("HOME", tui.home())
        .env(nebula_core::env::AGENT_ID, &orch)
        .env(agent::ROLE, "1")
        .stdin(std::process::Stdio::null());
    let out = cmd.output().unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("autonomy A0 (Hold) for game"),
        "{}",
        text(&out)
    );

    assert_eq!(agents(&tui.runtime_dir).len(), before, "nothing started");
    let log = read(&tui.data_dir.join("studio").join("refusals.jsonl"));
    assert_eq!(log.lines().count(), 5, "{log}");

    // No idle note at A0 either.
    let said = ok(&tui, &["studio", "queue", "--now"]);
    assert!(said.contains("holding: autonomy A0 (Hold)"), "{said}");
}

fn queue_status_json_shows_the_counts() {
    let (tui, _repo) = studio_repo();
    let out = ok(&tui, &["studio", "status", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out).expect(&out);
    let row = &rows[0];
    assert_eq!(row["name"], "game", "{out}");
    assert_eq!(row["autonomy"], "A1", "{out}");
    assert_eq!(row["inbox"], "notes/inbox", "{out}");
    assert_eq!(row["open"], 3, "{out}");
    assert_eq!(row["to_jay"], 1, "{out}");
    assert_eq!(row["unannounced"], 2, "{out}");
    assert_eq!(row["next"][0]["id"], "2026-10-21-storm-sky", "{out}");
    assert_eq!(row["held"], "no orchestrator is running", "{out}");

    ok(&tui, &["studio", "exclude", "game", "2026-10-21-storm-sky"]);
    let out = ok(&tui, &["studio", "status", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&out).expect(&out);
    assert_eq!(rows[0]["unannounced"], 1, "{out}");
}

/// The daemon's sessions, from a snapshot over its pipe.
fn agents(runtime_dir: &Path) -> Vec<Agent> {
    snapshot(runtime_dir).1
}

/// The only project's id.
fn agents_project(runtime_dir: &Path) -> ProjectId {
    snapshot(runtime_dir).0.remove(0).id
}

fn snapshot(runtime_dir: &Path) -> (Vec<nebula_core::Project>, Vec<Agent>) {
    pipe(runtime_dir, |mut stream| async move {
        write_frame(&mut stream, &ClientRequest::Subscribe)
            .await
            .unwrap();
        loop {
            match read_frame::<ServerEvent, _>(&mut stream).await.unwrap() {
                Some(ServerEvent::Snapshot {
                    projects, agents, ..
                }) => return (projects, agents),
                Some(_) => continue,
                None => panic!("the daemon hung up"),
            }
        }
    })
}

/// Send `reqs` over one connection; each request id's error, or None for
/// its Ack. Requests without an id (Input) are sent and not waited on.
fn call(runtime_dir: &Path, reqs: Vec<ClientRequest>) -> HashMap<u64, Option<String>> {
    pipe(runtime_dir, |mut stream| async move {
        let mut waiting = Vec::new();
        for req in reqs {
            if let Some(id) = req_id(&req) {
                waiting.push(id);
            }
            write_frame(&mut stream, &req).await.unwrap();
        }
        let mut out = HashMap::new();
        while out.len() < waiting.len() {
            let ev = tokio::time::timeout(WAIT, read_frame::<ServerEvent, _>(&mut stream))
                .await
                .expect("no reply")
                .unwrap();
            match ev {
                Some(ServerEvent::Error {
                    req_id: Some(r),
                    message,
                }) if waiting.contains(&r) => {
                    out.insert(r, Some(message));
                }
                Some(ServerEvent::Ack { req_id, .. }) if waiting.contains(&req_id) => {
                    out.insert(req_id, None);
                }
                Some(_) => continue,
                None => panic!("the daemon hung up"),
            }
        }
        // Let the last frame land before the pipe closes.
        tokio::time::sleep(Duration::from_millis(200)).await;
        out
    })
}

fn req_id(req: &ClientRequest) -> Option<u64> {
    match req {
        ClientRequest::OrchestratorSpawn { req_id, .. }
        | ClientRequest::OrchestratorSpawnWith { req_id, .. }
        | ClientRequest::SpawnSiblingAgent { req_id, .. }
        | ClientRequest::SpawnSiblingAgentWith { req_id, .. } => Some(*req_id),
        _ => None,
    }
}

/// Run `talk` on a connection to the harness's daemon, after Hello.
fn pipe<T, F>(
    runtime_dir: &Path,
    talk: impl FnOnce(tokio::net::windows::named_pipe::NamedPipeClient) -> F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    let name = nebula_core::paths::pipe_name_for_socket(&runtime_dir.join("daemon.sock"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .unwrap();
        let hello = ClientRequest::Hello {
            protocol_version: nebula_core::PROTOCOL_VERSION,
        };
        write_frame(&mut stream, &hello).await.unwrap();
        talk(stream).await
    })
}
