//! Windows end-to-end flows of the 0.60 triage lane, every one named
//! `triage_*`: a repo with a studio manifest, `kete studio note --triage`
//! run beside the harness's TUI, and the fake agent's argv log (the desk)
//! or its echo log (the orchestrator) read back for the turn it was sent.

use super::harness::{git, Options, Tui, WAIT};
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

pub const TESTS: &[(&str, fn())] = &[
    (
        "triage_a_note_starts_the_desk_on_its_path",
        triage_a_note_starts_the_desk_on_its_path,
    ),
    (
        "triage_without_a_desk_role_the_orchestrator_takes_it",
        triage_without_a_desk_role_the_orchestrator_takes_it,
    ),
];

const MANIFEST: &str = "version = 1\ninbox = \"notes/inbox\"\n";

const ROLES: &str = r#"version = 1

[roles.triage]
label = "Triage"
model = "sonnet"
effort = "low"
prompt = "You are the triage desk. Edit only the note's frontmatter."
tools = ["Read", "Edit", "Glob", "Grep"]
permission = "prompt"
"#;

const LINE: &str = "storm sky bands at dusk";

/// A TUI (and its daemon) with `game` registered: its manifest, and its
/// roles file when `roles`, committed.
fn studio_repo(roles: bool) -> (Tui, PathBuf) {
    let t = Tui::spawn(Options::default());
    let repo = t.make_repo("game");
    std::fs::create_dir_all(repo.join(".kete")).unwrap();
    std::fs::write(repo.join(".kete/studio.toml"), MANIFEST).unwrap();
    if roles {
        std::fs::write(repo.join(".kete/roles.toml"), ROLES).unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "studio"]);
    ok(&t.kete(&["add", &repo.display().to_string()]));
    (t, repo)
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "kete failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `kete <args>` beside the harness's TUI, left running.
fn kete_child(t: &Tui, args: &[&str]) -> Child {
    let mut cmd = t.kete_command();
    cmd.args(args)
        .env(nebula_core::env::RUNTIME_DIR, &t.runtime_dir)
        .env(nebula_core::env::DATA_DIR, &t.data_dir)
        .env(nebula_core::env::WAKE_NO_TASK, "1")
        .env("HOME", t.home())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove(nebula_core::env::CLAUDE_CONFIG_DIR)
        .env_remove(nebula_core::env::CONFIG_FILE)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for var in nebula_core::env::AGENT_SESSION_VARS {
        cmd.env_remove(var);
    }
    cmd.spawn().unwrap()
}

/// The one note in the inbox whose file name ends with `stem`.
fn note_file(repo: &Path, stem: &str) -> PathBuf {
    let dir = repo.join("notes/inbox");
    let found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(&format!("{stem}.md")))
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one {stem} note in {}: {found:?}",
        dir.display()
    );
    found.into_iter().next().unwrap()
}

/// The first `agent <id> args [...]` line of the fake agent's log whose
/// argv `pred` takes.
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

/// The argv the line logged (its `{:?}` form reads as JSON).
fn args_of(line: &str) -> Vec<String> {
    let list = &line[line.find(" args [").unwrap() + " args ".len()..];
    serde_json::from_str(list).unwrap_or_default()
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).map(String::as_str)
}

fn triage_a_note_starts_the_desk_on_its_path() {
    let (t, repo) = studio_repo(true);
    let out = t.kete(&["studio", "note", LINE, "--triage", "--project", "game"]);
    ok(&out);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let path = note_file(&repo, "storm-sky-bands-at-dusk");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("status: new\n") && text.contains("to: triage\n"),
        "{text}"
    );
    let id = path.file_stem().unwrap().to_string_lossy().into_owned();
    assert!(stdout.starts_with(&format!("{id}: ")), "{stdout}");
    println!("TRIAGE-E2E note {}:\n{text}", path.display());
    println!("TRIAGE-E2E route line: {stdout}");

    // The desk's first prompt is the note's path, with the triage card's
    // flags from the roles lane. Windows paths compare without case: the
    // daemon's spelling of the repo root may differ from the harness's.
    let want = nebula_core::studio::inbox_write::desk_prompt(&path).to_lowercase();
    let line = wait_log(&t, |a| a.last().is_some_and(|p| p.to_lowercase() == want));
    let args = args_of(&line);
    assert_eq!(flag(&args, "--model"), Some("sonnet"), "{args:?}");
    assert_eq!(flag(&args, "--effort"), Some("low"));
    assert_eq!(flag(&args, "--tools"), Some("Read,Edit,Glob,Grep"));
    assert_eq!(
        flag(&args, "--permission-mode"),
        None,
        "prompt adds no mode"
    );
    let tools = args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(args[tools + 2], "--append-system-prompt");
    assert!(args[tools + 3].contains("You are the triage desk."));
    let settings = PathBuf::from(flag(&args, "--settings").expect("a --settings file"));
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    let deny = settings["permissions"]["deny"].to_string();
    assert!(deny.contains("git push --force"), "{deny}");
    let sessions = String::from_utf8_lossy(&t.kete(&["sessions"]).stdout).into_owned();
    assert!(sessions.contains("name=\"triage desk\""), "{sessions}");

    // The same line again repeats the open note: exit 3, naming it.
    let again = t.kete(&["studio", "note", LINE, "--triage", "--project", "game"]);
    assert_eq!(again.status.code(), Some(3), "{again:?}");
    let why = String::from_utf8_lossy(&again.stderr);
    assert!(why.contains(&format!("{id}.md")), "{why}");
    assert_eq!(
        std::fs::read_dir(repo.join("notes/inbox")).unwrap().count(),
        1,
        "nothing more was filed"
    );
}

fn triage_without_a_desk_role_the_orchestrator_takes_it() {
    let (t, repo) = studio_repo(false);
    ok(&t.kete(&["orchestrator", "start", "--project", "game"]));
    wait_log(&t, |_| true);
    // What the fake orchestrator was typed lands in its agent log (taken as
    // its prompt) or its echo log (once at its idle prompt). The CLI may
    // finish or wait out its 120 s, so it is stopped either way.
    let mut child = kete_child(
        &t,
        &["studio", "note", LINE, "--triage", "--project", "game"],
    );
    let deadline = Instant::now() + WAIT;
    let typed = loop {
        let mut echo = std::fs::read_to_string(&t.agent_log).unwrap_or_default();
        echo.push_str(&String::from_utf8_lossy(
            &std::fs::read(&t.echo_log).unwrap_or_default(),
        ));
        if echo.contains("[triage] Triage the inbox note ") {
            break echo;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("the orchestrator was not sent the note: {echo:?} {out:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = child.kill();
    let _ = child.wait();
    let path = note_file(&repo, "storm-sky-bands-at-dusk");
    let posix = nebula_core::studio::settings::repo_key(&path).to_lowercase();
    assert!(
        typed.to_lowercase().contains(&posix),
        "{typed:?} names {posix}"
    );
    let sessions = String::from_utf8_lossy(&t.kete(&["sessions"]).stdout).into_owned();
    assert!(
        !sessions.contains("triage desk"),
        "no desk was started: {sessions}"
    );
}
