//! Windows end-to-end flows of the 0.60 renew lane, every one named `renew_*`:
//! a fake orchestrator started with a role card, a handover note in its
//! workspace, `kete orchestrator renew --from <note> --now` from outside
//! every session, and the fake agent's log and the daemon's answers read
//! back for what the new orchestrator was given.

use super::harness::{git, Options, Tui, WAIT};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

pub const TESTS: &[(&str, fn())] = &[
    (
        "renew_hands_over_to_a_new_orchestrator_on_the_note",
        renew_hands_over_to_a_new_orchestrator_on_the_note,
    ),
    (
        "renew_keep_old_leaves_the_old_one_as_a_worker",
        renew_keep_old_leaves_the_old_one_as_a_worker,
    ),
    (
        "renew_a_partial_log_line_is_skipped",
        renew_a_partial_log_line_is_skipped,
    ),
];

const ROLES: &str = r#"version = 1

[roles.manager]
label = "Manager"
model = "opus"
tier = 1
prompt = "You manage the studio."
"#;

/// The note's marker line, which the new orchestrator's first prompt
/// must carry.
const NOTE: &str = "In flight: the 0.60 gate. Next: tag it.";

/// A TUI (and its daemon) with `game` registered, its roles committed, and
/// an orchestrator started on it with the `manager` card.
fn orchestrated() -> (Tui, PathBuf, String) {
    let t = Tui::spawn(Options::default());
    let repo = t.make_repo("game");
    std::fs::create_dir_all(repo.join(".kete")).unwrap();
    std::fs::write(repo.join(".kete/roles.toml"), ROLES).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "roles"]);
    ok(&t.kete(&["add", &repo.display().to_string()]));
    ok(&t.kete(&[
        "orchestrator",
        "start",
        "--project",
        "game",
        "--role",
        "manager",
    ]));
    let orch = agent_of(&wait_launch(&t, None));
    (t, repo, orch)
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "kete failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `kete <args> --json`, parsed.
fn json(t: &Tui, args: &[&str]) -> serde_json::Value {
    let mut args = args.to_vec();
    args.push("--json");
    let out = t.kete(&args);
    ok(&out);
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The handover note, written into the orchestrator's workspace.
fn note(repo: &Path, name: &str) -> PathBuf {
    let path = repo.join(name);
    std::fs::write(&path, format!("# Handover\n\n{NOTE}\n")).unwrap();
    path
}

/// The pinned orchestrator, once it is no longer `old`.
fn wait_new_orchestrator(t: &Tui, old: &str) -> serde_json::Value {
    let deadline = Instant::now() + WAIT;
    loop {
        let status = json(t, &["orchestrator", "status"]);
        let pinned = &status["orchestrator"];
        if pinned["id"].as_str().is_some_and(|id| id != old) {
            return pinned.clone();
        }
        assert!(Instant::now() < deadline, "no new orchestrator: {status}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The renewal log's last line, once there is one naming `new`.
fn wait_record(t: &Tui, new: &str) -> serde_json::Value {
    let file = t.data_dir.join("orchestrator-renewals.jsonl");
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if let Some(line) = record_for(&text, new) {
            return line;
        }
        assert!(Instant::now() < deadline, "no renewal record in:\n{text}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The log's last line when it is the finished record for `new`; None
/// while it is not there yet, or is only partly written.
fn record_for(text: &str, new: &str) -> Option<serde_json::Value> {
    let line: serde_json::Value = serde_json::from_str(text.lines().last()?).ok()?;
    (line["new"] == new && line["state"] != "scheduled").then_some(line)
}

fn renew_a_partial_log_line_is_skipped() {
    let done = r#"{"at":1,"old":"a","new":"b","note":"n","state":"done","reason":null}"#;
    assert_eq!(record_for(done, "b").unwrap()["state"], "done");
    assert_eq!(record_for(done, "c"), None);
    // The next record half written: not read, not a panic.
    let partial = format!("{done}\n{{\"at\":2,\"old\":\"b\",\"ne");
    assert_eq!(record_for(&partial, "b"), None);
    assert_eq!(record_for("", "b"), None);
}

/// The `agent <id> args [...]` line of the fake agent's log for a launch
/// other than `but`'s.
fn wait_launch(t: &Tui, but: Option<&str>) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let log = std::fs::read_to_string(&t.agent_log).unwrap_or_default();
        if let Some(line) = log
            .lines()
            .filter(|l| l.contains(" args ["))
            .find(|l| but.is_none_or(|b| agent_of(l) != b))
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

fn args_of(line: &str) -> Vec<String> {
    let list = &line[line.find(" args [").unwrap() + " args ".len()..];
    serde_json::from_str(list).unwrap_or_default()
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).map(String::as_str)
}

/// Every session `kete sessions` lists (archived ones never are).
fn listed(t: &Tui) -> Vec<String> {
    json(t, &["sessions"])["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect()
}

fn renew_hands_over_to_a_new_orchestrator_on_the_note() {
    let (t, repo, old) = orchestrated();
    let handover = note(&repo, "handover.md");
    let out = json(
        &t,
        &[
            "orchestrator",
            "renew",
            "--from",
            &handover.display().to_string(),
            "--now",
        ],
    );
    assert_eq!(out["state"], "scheduled", "{out}");
    let pinned = wait_new_orchestrator(&t, &old);
    let new = pinned["id"].as_str().unwrap().to_string();
    // Its first prompt (the fake agent's argv) is the spec's line, then
    // the note.
    let launch = wait_launch(&t, Some(&old));
    assert_eq!(agent_of(&launch), new);
    let args = args_of(&launch);
    let prompt = args.last().unwrap();
    assert!(
        prompt.starts_with("[from kete renew] You are the orchestrator, renewed from "),
        "{prompt}"
    );
    assert!(prompt.contains(NOTE), "{prompt}");
    assert!(prompt.contains("handover.md"), "{prompt}");
    // The role card came across: its model and its prompt.
    assert_eq!(flag(&args, "--model"), Some("opus"), "{args:?}");
    assert!(
        flag(&args, "--append-system-prompt").is_some_and(|p| p.contains("You manage the studio.")),
        "{args:?}"
    );
    let record = wait_record(&t, &new);
    assert_eq!(record["state"], "done", "{record}");
    assert_eq!(record["old"], old.as_str());
    // `kete orchestrator status` names the new one; the old one is
    // archived, so `kete sessions` no longer lists it.
    let status = t.kete(&["orchestrator", "status"]);
    ok(&status);
    assert!(
        String::from_utf8_lossy(&status.stdout).contains(&new),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let deadline = Instant::now() + WAIT;
    while listed(&t).contains(&old) {
        assert!(
            Instant::now() < deadline,
            "the old orchestrator is still listed"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(listed(&t).contains(&new));
}

fn renew_keep_old_leaves_the_old_one_as_a_worker() {
    let (t, repo, old) = orchestrated();
    let handover = note(&repo, "handover.md");
    ok(&t.kete(&[
        "orchestrator",
        "renew",
        "--from",
        &handover.display().to_string(),
        "--now",
        "--keep-old",
    ]));
    let pinned = wait_new_orchestrator(&t, &old);
    let new = pinned["id"].as_str().unwrap().to_string();
    assert_eq!(wait_record(&t, &new)["state"], "done");
    let sessions = listed(&t);
    assert!(sessions.contains(&old), "{sessions:?}");
    assert!(sessions.contains(&new), "{sessions:?}");
    // The old one runs on as a worker; the new one is the orchestrator.
    let out = t.kete_with(
        &["orchestrator", "list", "--json"],
        &[(nebula_core::env::AGENT_ID, &new)],
    );
    ok(&out);
    let tree: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let role = |id: &str| {
        tree["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .map(|a| a["role"].clone())
            .unwrap_or_else(|| panic!("{id} is not in the tree: {tree}"))
    };
    assert_eq!(role(&old), "worker");
    assert_eq!(role(&new), "orchestrator");
    // A note outside the workspace and every project is refused, and the
    // new orchestrator keeps the role.
    let elsewhere = t.home().join("note.md");
    std::fs::write(&elsewhere, NOTE).unwrap();
    let out = t.kete(&[
        "orchestrator",
        "renew",
        "--from",
        &elsewhere.display().to_string(),
        "--now",
    ]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("outside the orchestrator's workspace"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        json(&t, &["orchestrator", "status"])["orchestrator"]["id"],
        new.as_str()
    );
}
