//! Upgrading from an older release: the data dir it left, the current
//! daemon and TUI started on it.
//!
//! The fixtures (`tests/fixtures/upgrade/<release>/`) were captured from
//! the real release (`tests/fixtures/upgrade/seed048.rs` says how): its
//! database as SQL text and its settings files, with the paths it wrote
//! turned into tokens — `@ROOT@` (the root, `/`-separated, as the database
//! keeps it), `@ROOTW@` (`\`-separated and JSON-escaped, as `reviewed.json`
//! keeps it) and `@AGENT@` (the `claude` harness's program). [`seed`]
//! rebuilds the repos they name, with the same commits, and puts the tokens
//! back for this run's root.

use super::harness::*;
use std::path::Path;

/// Commits dated and signed as the seed's were, so their SHAs (which
/// `reviewed.json`'s marks are scoped to) come out the same.
const FIXED_COMMIT: &[(&str, &str)] = &[
    ("GIT_AUTHOR_NAME", "Kete Fixture"),
    ("GIT_AUTHOR_EMAIL", "fixture@example.invalid"),
    ("GIT_COMMITTER_NAME", "Kete Fixture"),
    ("GIT_COMMITTER_EMAIL", "fixture@example.invalid"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

/// The fixture's "login tests" session.
const TESTS_AGENT: &str = "01M50MKGP1W41N48GC9N9BG0F8";

/// The SHA of alpha's first commit, as the seed made it.
pub const ALPHA_HEAD: &str = "3c63799e00e81d7af0e7f74716f3b0c9d4571e15";

fn commit(dir: &Path, message: &str) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", message])
        .envs(FIXED_COMMIT.iter().copied())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `release`'s repos and data dir under `base` (whose `data` and `repos`
/// the harness made), the `claude` harness pointed at `agent`.
pub fn seed(base: &Path, release: &str, agent: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/upgrade")
        .join(release);
    let repos = base.join("repos");
    let origins = base.join("origins");
    std::fs::create_dir_all(&origins).unwrap();
    for name in ["alpha", "beta"] {
        let repo = repos.join(name);
        let origin = origins.join(format!("{name}.git"));
        std::fs::create_dir_all(&repo).unwrap();
        git(
            base,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                &origin.display().to_string(),
            ],
        );
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "core.autocrlf", "false"]);
        std::fs::write(repo.join("README.md"), format!("# {name}\n")).unwrap();
        git(&repo, &["add", "README.md"]);
        commit(&repo, "init");
        git(
            &repo,
            &["remote", "add", "origin", &origin.display().to_string()],
        );
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["remote", "set-head", "origin", "main"]);
    }
    assert_eq!(
        git(&repos.join("alpha"), &["rev-parse", "HEAD"]).trim(),
        ALPHA_HEAD,
        "the fixture's marks need the seed's commit"
    );
    for (repo, dir, branch) in [
        ("alpha", "feat-login", "feat/login"),
        ("alpha", "fix-typo", "fix-typo"),
        ("beta", "spike", "spike"),
    ] {
        let path = repos.join(format!("{repo}-worktrees")).join(dir);
        git(
            &repos.join(repo),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                &path.display().to_string(),
            ],
        );
    }
    // Before 0.52 an agent's `git add -A` committed the hooks file Kete
    // wrote: feat/login tracks it.
    let login = repos.join("alpha-worktrees").join("feat-login");
    std::fs::create_dir_all(login.join(".claude")).unwrap();
    std::fs::write(login.join(".claude/settings.local.json"), "{}\n").unwrap();
    std::fs::write(login.join("login.rs"), "fn login() {}\n").unwrap();
    git(
        &login,
        &["add", "-f", ".claude/settings.local.json", "login.rs"],
    );
    commit(&login, "login");
    // Snapshot refs a 0.51 daemon could have left beside this one's:
    // another data dir's, an unowned one, and one of this dir's sessions'.
    let alpha = repos.join("alpha");
    let tree = git(&alpha, &["rev-parse", "HEAD^{tree}"]);
    for name in [
        "refs/kete/d/0th3r/sessions/a9/last".to_string(),
        "refs/kete/sessions/stranger/start".to_string(),
        format!("refs/kete/sessions/{TESTS_AGENT}/start"),
    ] {
        git(&alpha, &["update-ref", &name, tree.trim()]);
    }
    // The edit the fixture's reviewed mark is for.
    std::fs::write(repos.join("alpha/README.md"), "# alpha\n\nedited\n").unwrap();

    let root = base.display().to_string();
    let root_fwd = root.replace('\\', "/");
    let json = |s: &str| serde_json::to_string(s).unwrap();
    let unquote = |s: String| s[1..s.len() - 1].to_string();
    let root_json = unquote(json(&root));
    let agent_json = unquote(json(&agent.display().to_string()));
    let data = base.join("data");
    copy_tree(&fixture.join("data"), &data, &|text| {
        text.replace("@ROOTW@", &root_json)
            .replace("@ROOT@", &root_fwd)
            .replace("@AGENT@", &agent_json)
    });
    let sql = std::fs::read_to_string(fixture.join("nebula.sql"))
        .unwrap()
        .replace("@ROOT@", &root_fwd);
    rusqlite::Connection::open(data.join("nebula.db"))
        .unwrap()
        .execute_batch(&sql)
        .unwrap();
}

/// `from` into `to`, each `.json` file through `fill`.
fn copy_tree(from: &Path, to: &Path, fill: &dyn Fn(&str) -> String) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest, fill);
        } else {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            std::fs::write(dest, fill(&text)).unwrap();
        }
    }
}

/// The daemon's tree over its pipe, as a client gets it on Subscribe.
pub(crate) fn daemon_tree(
    runtime_dir: &Path,
) -> (
    Vec<nebula_core::Project>,
    Vec<nebula_core::Worktree>,
    Vec<nebula_core::Agent>,
) {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent, PROTOCOL_VERSION};
    let pipe = nebula_core::paths::pipe_name_for_socket(&runtime_dir.join("daemon.sock"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&pipe)
            .unwrap();
        let hello = ClientRequest::Hello {
            protocol_version: PROTOCOL_VERSION,
        };
        write_frame(&mut stream, &hello).await.unwrap();
        write_frame(&mut stream, &ClientRequest::Subscribe)
            .await
            .unwrap();
        loop {
            match read_frame::<ServerEvent, _>(&mut stream).await.unwrap() {
                Some(ServerEvent::Snapshot {
                    projects,
                    worktrees,
                    agents,
                    ..
                }) => return (projects, worktrees, agents),
                Some(_) => continue,
                None => panic!("the daemon hung up"),
            }
        }
    })
}

fn config(tui: &Tui) -> serde_json::Value {
    let text = std::fs::read_to_string(tui.data_dir.join("config.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn kete_refs(repo: &Path) -> Vec<String> {
    git(repo, &["for-each-ref", "--format=%(refname)", "refs/kete/"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// A 0.48 data dir under this build: the database migrates with every
/// project, worktree, session, order and account intact (and the worktree
/// sync keeps every row); What's new shows once, then the first tip; the
/// user's F12 still opens Help; the checkout that committed Kete's hooks
/// file is told how to untrack it, once; another daemon's snapshot refs
/// and an unowned 0.51 one survive the startup sweep, and a 0.51 ref of
/// one of this data dir's sessions is adopted.
pub fn a_048_data_dir_upgrades_in_place() {
    let mut tui = Tui::spawn(Options {
        fixture: Some("v0.48"),
        ..Options::default()
    });
    let alpha = tui.repos.join("alpha");
    let tests_agent = TESTS_AGENT;

    // What's new, once: this is an upgrade (the database was there).
    let releases = nebula_config::learn::WHATS_NEW;
    let whats_new = format!(
        "What's new in {} to {}",
        releases[releases.len() - 1].version,
        releases[0].version
    );
    tui.wait_for_text(&whats_new);
    // The checkout that tracks Kete's hooks file, named with the command.
    tui.wait_for_text(
        "git rm --cached .claude/settings.local.json in alpha feat/login, then commit: git tracks Kete's hooks file",
    );
    tui.send(ESC);
    tui.wait_for_gone(&whats_new);
    let current = nebula_config::learn::current_version();
    tui.wait_for("whats_new_seen written", |_| {
        config(&tui)["whats_new_seen"] == serde_json::json!(current)
    });
    let told: Vec<String> =
        serde_json::from_value(config(&tui)["tracked_settings_noticed"].clone()).unwrap();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].ends_with("feat-login"), "{told:?}");

    // The data, through the daemon.
    let (projects, worktrees, agents) = daemon_tree(&tui.runtime_dir);
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["beta", "alpha"]);
    assert_eq!(worktrees.len(), 5, "{worktrees:?}");
    let alpha_rows: Vec<&str> = worktrees
        .iter()
        .filter(|w| w.project_id == projects[1].id)
        .map(|w| w.branch.as_str())
        .collect();
    assert_eq!(alpha_rows, ["main", "fix-typo", "feat/login"]);
    assert_eq!(agents.len(), 6, "{agents:?}");
    let login = worktrees.iter().find(|w| w.branch == "feat/login").unwrap();
    let in_login: Vec<&str> = agents
        .iter()
        .filter(|a| a.worktree_id == login.id)
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(in_login, ["login tests", "login form (renamed)"]);
    let by_name = |n: &str| agents.iter().find(|a| a.name == n).unwrap();
    assert!(by_name("typo").archived);
    assert_eq!(by_name("spike it").profile.as_deref(), Some("spare"));
    let db = rusqlite::Connection::open_with_flags(
        tui.data_dir.join("nebula.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 35);
    let ns: String = db
        .query_row("SELECT daemon_id FROM daemon_identity", [], |r| r.get(0))
        .unwrap();

    // The startup sweep: another daemon's ref and an unowned 0.51 one
    // stay; this data dir's session's 0.51 ref moves into its namespace.
    tui.wait_for("the 0.51 ref adopted", |_| {
        !kete_refs(&alpha).contains(&format!("refs/kete/sessions/{tests_agent}/start"))
    });
    let refs = kete_refs(&alpha);
    assert!(
        refs.contains(&"refs/kete/d/0th3r/sessions/a9/last".to_string()),
        "{refs:?}"
    );
    assert!(
        refs.contains(&"refs/kete/sessions/stranger/start".to_string()),
        "{refs:?}"
    );
    assert!(
        refs.contains(&format!("refs/kete/d/{ns}/sessions/{tests_agent}/start")),
        "{refs:?}"
    );

    // Tips are on (the 0.48 config predates the key): the first one shows
    // and is written as seen.
    tui.wait_for("a tip recorded", |_| {
        config(&tui)["tips_seen"]
            .as_array()
            .is_some_and(|seen| !seen.is_empty())
    });

    // Their F12 still opens Help.
    tui.send(ESC);
    tui.wait_for_gone(" Help ");
    tui.send(b"\x1b[24~");
    tui.wait_for_text(" Help ");
    tui.send(ESC);
    tui.wait_for_gone(" Help ");

    // The next launch: no What's new and no notice again.
    tui.restart();
    tui.wait_for_text("login tests");
    let back = tui.try_wait_for_text("What's new in 0.49", std::time::Duration::from_secs(3));
    assert!(!back, "What's new came back:\n{}", tui.screen_text());
    let text = tui.screen_text();
    assert!(!text.contains("git tracks Kete's"), "{text}");
}
