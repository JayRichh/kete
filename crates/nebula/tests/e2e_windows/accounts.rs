//! Windows end-to-end flows of the 0.60 accounts lane, every one named
//! `accounts_*`: two Claude profiles, `jay` (the default, reserved, 20%
//! used) and `max2` (70% used). The orchestrator's spawn naming no profile
//! lands on `max2`; a quick prompt Jay launches still takes `jay`.

use super::harness::*;
use nebula_core::entities::{Agent, AgentRole};
use nebula_core::usage::{AccountUsage, UsageSource, UsageWindow, WindowKind};

type Test = fn();

pub const TESTS: &[(&str, Test)] = &[
    (
        "accounts_orchestrator_spawn_skips_the_reserved_account",
        accounts_orchestrator_spawn_skips_the_reserved_account,
    ),
    (
        "accounts_quick_prompt_keeps_the_reserved_default",
        accounts_quick_prompt_keeps_the_reserved_default,
    ),
];

/// A TUI over the two profiles and their readings, with the project
/// `name` open.
fn seeded(name: &str) -> (Tui, std::path::PathBuf) {
    let mut tui = Tui::spawn(Options::default());
    let config = tui.data_dir.join(nebula_core::paths::CONFIG_FILE_NAME);
    let mut settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    settings["profiles"] = serde_json::json!([
        { "name": "jay", "harness": "claude", "default": true, "reserved": true },
        { "name": "max2", "harness": "claude" },
    ]);
    std::fs::write(&config, settings.to_string()).unwrap();
    reading(&tui, "jay", 20.0);
    reading(&tui, "max2", 70.0);
    // Started again so the TUI's launcher reads the profiles from its start.
    tui.restart();
    let repo = tui.make_repo(name);
    tui.wait_for_text("create your first project");
    tui.add_project(&repo, name);
    (tui, repo)
}

/// A reading for `account` at `percent` on both windows, resetting in an hour.
fn reading(tui: &Tui, account: &str, percent: f32) {
    let now = nebula_core::clock::now_secs() as i64;
    let window = |kind| UsageWindow {
        kind,
        label: None,
        percent,
        resets_at: Some(now + 3600),
        read_at: now,
        estimated: false,
    };
    let usage = AccountUsage {
        account: account.into(),
        harness: "claude".into(),
        windows: vec![window(WindowKind::Session), window(WindowKind::Weekly)],
        source: UsageSource::Statusline,
        at: now * 1000,
        plan: None,
    };
    nebula_core::usage::write_account_in(&tui.data_dir.join("profiles"), &usage).unwrap();
}

/// The first agent `pick` finds in the daemon's tree, waited for.
fn wait_for_agent_row(tui: &Tui, what: &str, pick: impl Fn(&Agent) -> bool) -> Agent {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let (_, _, agents) = super::upgrade::daemon_tree(&tui.runtime_dir);
        if let Some(agent) = agents.into_iter().find(|a| pick(a)) {
            return agent;
        }
        if std::time::Instant::now() > deadline {
            tui.dump(what);
            panic!("timed out waiting for {what}");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// `kete <args>` as the session `agent` runs it: the harness's command
/// with that session's id, which the orchestrator verbs read.
fn kete_as(tui: &Tui, agent: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = tui.kete_command();
    cmd.args(args)
        .env(nebula_core::env::RUNTIME_DIR, &tui.runtime_dir)
        .env(nebula_core::env::DATA_DIR, &tui.data_dir)
        .env(nebula_core::env::WAKE_NO_TASK, "1")
        .env("HOME", tui.home())
        .env_remove(nebula_core::env::CLAUDE_CONFIG_DIR)
        .env(nebula_core::env::AGENT_ID, agent)
        .stdin(std::process::Stdio::null());
    cmd.output().unwrap()
}

fn said(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The orchestrator Jay starts takes his reserved default; the worker it
/// spawns naming no profile lands on `max2`, and its card says so.
fn accounts_orchestrator_spawn_skips_the_reserved_account() {
    let (tui, _) = seeded("acct-proj");
    let out = tui.kete(&["orchestrator", "start"]);
    assert!(out.status.success(), "{}", said(&out));
    let orchestrator = wait_for_agent_row(&tui, "the orchestrator", |a| {
        a.role == AgentRole::Orchestrator
    });
    assert_eq!(
        orchestrator.profile.as_deref(),
        Some("jay"),
        "a person's start keeps the reserved default"
    );
    let out = kete_as(
        &tui,
        &orchestrator.id.0,
        &[
            "orchestrator",
            "spawn",
            "--project",
            "acct-proj",
            "run the lane",
        ],
    );
    assert!(out.status.success(), "{}", said(&out));
    let worker = wait_for_agent_row(&tui, "the spawned worker", |a| a.role == AgentRole::Worker);
    assert_eq!(worker.profile.as_deref(), Some("max2"), "{worker:?}");
    tui.wait_for_agent("prompt \"run the lane\"");
    tui.wait_for_text("claude@max2");
}

/// A quick prompt is Jay's own launch: the reserved default stays his.
fn accounts_quick_prompt_keeps_the_reserved_default() {
    let (tui, _) = seeded("acct-quick");
    tui.send(b"p");
    tui.wait_for_text("what should the agent do?");
    tui.type_str("tidy the readme");
    tui.wait_for_text("│ tidy the readme");
    tui.send(ENTER);
    tui.wait_for_gone("what should the agent do?");
    let agent = wait_for_agent_row(&tui, "the quick prompt's session", |a| {
        a.role == AgentRole::Worker
    });
    assert_eq!(agent.profile.as_deref(), Some("jay"), "{agent:?}");
    tui.wait_for_text("claude@jay");
}
