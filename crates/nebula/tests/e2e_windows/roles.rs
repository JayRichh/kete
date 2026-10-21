//! Windows end-to-end flows of the 0.60 roles lane, every one named `roles_*`:
//! a repo with a committed `.kete/roles.toml`, an orchestrator, and the fake
//! agent's argv log read back for what each launch was given.

use super::harness::{git, Options, Tui, WAIT};
use nebula_core::studio::autonomy::Autonomy;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub const TESTS: &[(&str, fn())] = &[
    (
        "roles_orchestrator_spawn_applies_the_runner_card",
        roles_orchestrator_spawn_applies_the_runner_card,
    ),
    (
        "roles_unattended_waits_for_the_account_to_accept_bypass",
        roles_unattended_waits_for_the_account_to_accept_bypass,
    ),
    (
        "roles_a_card_outlives_a_daemon_restart",
        roles_a_card_outlives_a_daemon_restart,
    ),
    (
        "roles_a_studio_spawn_without_a_role_follows_live_autonomy",
        roles_a_studio_spawn_without_a_role_follows_live_autonomy,
    ),
];

const ROLES: &str = r#"version = 1
governance = ["DECISIONS/**", ".kete/**"]

[roles.runner]
label = "Runner"
model = "haiku"
effort = "low"
tier = 3
prompt = "You are a runner. Run the checks you are given."
tools = ["Read", "Write", "Bash"]
allow = ["Bash(npm test)"]
mcp = "none"
permission = "allowlist"

[roles.lead]
label = "Lead"
model = "opus"
tier = 1
prompt = "You lead the department."
permission = "unattended"
"#;

/// A TUI (and its daemon) with `game` registered, its roles committed.
fn studio_repo() -> (Tui, PathBuf) {
    let t = Tui::spawn(Options::default());
    let repo = t.make_repo("game");
    std::fs::create_dir_all(repo.join(".kete")).unwrap();
    std::fs::write(repo.join(".kete/roles.toml"), ROLES).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "roles"]);
    ok(&t.kete(&["add", &repo.display().to_string()]));
    (t, repo)
}

/// The orchestrator's id, once it has booted.
fn start_orchestrator(t: &Tui) -> String {
    ok(&t.kete(&["orchestrator", "start", "--project", "game"]));
    let line = wait_log(t, |_| true);
    agent_of(&line)
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "kete failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `kete <args>` beside the harness's TUI, run as session `agent` when
/// given (`NEBULA_AGENT_ID`), with `stdin` piped in.
fn kete_as(t: &Tui, agent: Option<&str>, args: &[&str], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kete"));
    cmd.args(args)
        .env(nebula_core::env::RUNTIME_DIR, &t.runtime_dir)
        .env(nebula_core::env::DATA_DIR, &t.data_dir)
        .env(nebula_core::env::WAKE_NO_TASK, "1")
        .env("HOME", t.home())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove(nebula_core::env::CLAUDE_CONFIG_DIR)
        .env_remove(nebula_core::env::CONFIG_FILE)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for var in nebula_core::env::AGENT_SESSION_VARS {
        cmd.env_remove(var);
    }
    if let Some(agent) = agent {
        cmd.env(nebula_core::env::AGENT_ID, agent);
    }
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

/// The guard's answer for `command` in `session`: its exit code and stderr.
fn guard(t: &Tui, session: &str, command: &str) -> (i32, String) {
    let payload = serde_json::json!({
        "session_id": "fake",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command },
    })
    .to_string();
    let out = kete_as(t, None, &["_guard", "--session", session], Some(&payload));
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The first `agent <id> args [...]` line of the fake agent's log that
/// `pred` takes.
fn wait_log(t: &Tui, pred: impl Fn(&[String]) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let log = std::fs::read_to_string(&t.agent_log).unwrap_or_default();
        if let Some(line) = log
            .lines()
            .filter(|l| l.contains(" args ["))
            .find(|l| pred(&args_of(l)))
        {
            return line.to_string();
        }
        assert!(Instant::now() < deadline, "no such launch in:\n{log}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn agent_of(line: &str) -> String {
    line.split(" agent ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string()
}

/// The argv the line logged (its `{:?}` form reads as JSON).
fn args_of(line: &str) -> Vec<String> {
    let list = &line[line.find(" args [").unwrap() + " args ".len()..];
    serde_json::from_str(list).unwrap_or_default()
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).map(String::as_str)
}

fn settings_of(args: &[String]) -> (PathBuf, serde_json::Value) {
    let path = PathBuf::from(flag(args, "--settings").expect("a --settings file"));
    let text = std::fs::read_to_string(&path).unwrap();
    (path, serde_json::from_str(&text).unwrap())
}

fn denies(settings: &serde_json::Value) -> Vec<String> {
    settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect()
}

/// The posix rule form of `dir`, as the settings file writes it.
fn rule(dir: &Path, rel: &str) -> String {
    format!(
        "Edit({}/{rel})",
        nebula_core::roles::permissions::posix_rule_path(dir)
    )
}

fn roles_orchestrator_spawn_applies_the_runner_card() {
    let (t, repo) = studio_repo();
    let orch = start_orchestrator(&t);
    ok(&kete_as(
        &t,
        Some(&orch),
        &[
            "orchestrator",
            "spawn",
            "--project",
            "game",
            "--role",
            "runner",
            "check the build",
        ],
        None,
    ));
    let line = wait_log(&t, |a| a.last().is_some_and(|p| p == "check the build"));
    let args = args_of(&line);
    assert_eq!(flag(&args, "--tools"), Some("Read,Write,Bash"), "{args:?}");
    assert_eq!(flag(&args, "--permission-mode"), Some("dontAsk"));
    assert_eq!(flag(&args, "--model"), Some("haiku"));
    assert_eq!(flag(&args, "--effort"), Some("low"));
    assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    // The variadic --tools is closed by the system prompt before the task.
    let tools = args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(args[tools + 2], "--append-system-prompt");
    assert!(args[tools + 3].contains("You are a runner."));
    let (path, settings) = settings_of(&args);
    let deny = denies(&settings);
    for want in [
        "Bash(git push --force*)",
        "PowerShell(git push --force*)",
        "Bash(git push * +*)",
        "AskUserQuestion",
    ] {
        assert!(deny.iter().any(|d| d == want), "{want} in {deny:?}");
    }
    assert!(deny.contains(&rule(&repo, "DECISIONS/**")), "{deny:?}");
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Bash(npm test)"])
    );
    println!(
        "ROLES-E2E runner settings file {}:\n{}",
        path.display(),
        serde_json::to_string_pretty(&settings).unwrap()
    );
    let runner = agent_of(&line);
    let (code, why) = guard(&t, &runner, "git push --force origin main");
    assert_eq!(code, 2, "{why}");
    assert!(why.contains("force-push"), "{why}");
    assert_eq!(guard(&t, &runner, "git status").0, 0);

    // A sibling spawn with a role: the task is still the last argument.
    ok(&kete_as(
        &t,
        Some(&orch),
        &["spawn", "--role", "runner", "sibling task"],
        None,
    ));
    let line = wait_log(&t, |a| a.last().is_some_and(|p| p == "sibling task"));
    let args = args_of(&line);
    assert_eq!(flag(&args, "--tools"), Some("Read,Write,Bash"));
    assert_eq!(args[args.len() - 3], "--append-system-prompt", "{args:?}");
}

fn roles_unattended_waits_for_the_account_to_accept_bypass() {
    let (t, _repo) = studio_repo();
    ok(&t.kete(&["profile", "add", "max2", "--harness", "claude"]));
    let orch = start_orchestrator(&t);
    let spawn = |task: &str| {
        kete_as(
            &t,
            Some(&orch),
            &[
                "orchestrator",
                "spawn",
                "--project",
                "game",
                "--role",
                "lead",
                "--profile",
                "max2",
                task,
            ],
            None,
        )
    };
    let refused = spawn("lead the shift");
    assert!(!refused.status.success());
    let why = String::from_utf8_lossy(&refused.stderr);
    assert!(
        why.contains("account max2 has not accepted bypass mode")
            && why.contains("kete roles accept-bypass max2"),
        "{why}"
    );
    let log = std::fs::read_to_string(t.data_dir.join("studio/refusals.jsonl")).unwrap();
    assert!(log.contains("\"role\":\"lead\""), "{log}");
    // The account's own Claude settings, as accepting the dialog leaves them.
    let home = t.data_dir.join("profiles/max2/claude");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("settings.json"),
        r#"{"skipDangerousModePermissionPrompt": true}"#,
    )
    .unwrap();
    ok(&spawn("lead the shift now"));
    let line = wait_log(&t, |a| a.last().is_some_and(|p| p == "lead the shift now"));
    let args = args_of(&line);
    assert_eq!(flag(&args, "--permission-mode"), Some("bypassPermissions"));
    let (path, settings) = settings_of(&args);
    assert_eq!(settings["permissions"]["allow"], serde_json::json!([]));
    assert!(denies(&settings).iter().any(|d| d == "AskUserQuestion"));
    println!(
        "ROLES-E2E unattended lead settings file {}:\n{}",
        path.display(),
        serde_json::to_string_pretty(&settings).unwrap()
    );
}

/// The `SessionRoles` a 66 client is sent after its snapshot.
fn session_roles(t: &Tui) -> Vec<nebula_core::studio::wire::SessionRole> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    // The suite runs one test at a time, so this process's env is ours.
    std::env::set_var(nebula_core::env::RUNTIME_DIR, &t.runtime_dir);
    std::env::set_var(nebula_core::env::DATA_DIR, &t.data_dir);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let roles = rt.block_on(async {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok(Some(mut conn)) = nebula_tui::ipc::connect_existing().await {
                write_frame(&mut conn.stream, &ClientRequest::Subscribe)
                    .await
                    .unwrap();
                loop {
                    let next = tokio::time::timeout(
                        Duration::from_secs(10),
                        read_frame::<ServerEvent, _>(&mut conn.stream),
                    )
                    .await;
                    match next {
                        Ok(Ok(Some(ServerEvent::SessionRoles { roles }))) => return roles,
                        Ok(Ok(Some(_))) => continue,
                        _ => panic!("no SessionRoles after the snapshot"),
                    }
                }
            }
            assert!(Instant::now() < deadline, "no daemon to ask");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    std::env::remove_var(nebula_core::env::RUNTIME_DIR);
    std::env::remove_var(nebula_core::env::DATA_DIR);
    roles
}

fn roles_a_card_outlives_a_daemon_restart() {
    let (mut t, _repo) = studio_repo();
    let orch = start_orchestrator(&t);
    ok(&kete_as(
        &t,
        Some(&orch),
        &[
            "orchestrator",
            "spawn",
            "--project",
            "game",
            "--role",
            "runner",
            "keep me",
        ],
        None,
    ));
    let runner = agent_of(&wait_log(&t, |a| a.last().is_some_and(|p| p == "keep me")));
    let label = |t: &Tui| {
        session_roles(t)
            .into_iter()
            .find(|r| r.agent.0 == runner)
            .map(|r| r.label)
    };
    assert_eq!(label(&t).as_deref(), Some("Runner"));
    ok(&t.kete(&["kill"]));
    t.restart();
    assert_eq!(
        label(&t).as_deref(),
        Some("Runner"),
        "the card survives the restart"
    );
}

fn roles_a_studio_spawn_without_a_role_follows_live_autonomy() {
    let (t, repo) = studio_repo();
    let set = |level: Autonomy| {
        nebula_core::studio::settings::write_in(
            &t.data_dir.join(nebula_core::paths::CONFIG_FILE_NAME),
            &t.data_dir.join(nebula_core::paths::CONFIG_LOCAL_FILE_NAME),
            &repo,
            Some(&nebula_core::studio::StudioSettings {
                studio_inbox: "inbox".into(),
                autonomy: level,
                queue_exclude: Vec::new(),
            }),
        )
        .unwrap();
    };
    set(Autonomy::A1);
    let orch = start_orchestrator(&t);
    ok(&kete_as(
        &t,
        Some(&orch),
        &["orchestrator", "spawn", "--project", "game", "x"],
        None,
    ));
    let line = wait_log(&t, |a| a.last().is_some_and(|p| p == "x"));
    let args = args_of(&line);
    assert_eq!(flag(&args, "--permission-mode"), None);
    assert_eq!(flag(&args, "--tools"), None);
    let (_, settings) = settings_of(&args);
    let deny = denies(&settings);
    assert!(deny.iter().any(|d| d == "Bash(git push *)"), "{deny:?}");
    assert!(deny.iter().any(|d| d == "Bash(git push --force*)"));
    let child = agent_of(&line);
    let (code, why) = guard(&t, &child, "git push -u origin x");
    assert_eq!(code, 2, "{why}");
    assert!(why.contains("A1 (Plan)"), "{why}");
    set(Autonomy::A2);
    let (code, why) = guard(&t, &child, "git push -u origin x");
    assert_eq!(code, 0, "live autonomy A2 allows the push: {why}");
    assert_eq!(guard(&t, &child, "git push --force").0, 2);
}
