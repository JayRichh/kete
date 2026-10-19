//! The rail's ACCOUNTS: every profile (a broken one with what is wrong
//! with it), plus each harness's own login (`@claude`) while it is in use,
//! with its last plan usage, whether it is signed in, and the command that
//! signs it in. Usage here is what is kept on disk (`nebula_core::usage`);
//! the live `AccountUsage` events in the model take over once they arrive.
//! The window signs nobody in itself: at most it opens a terminal and
//! types the login command there, for the user to finish.

use crate::daemon::Client;
use crate::Res;
use nebula_core::accounts::{self, LoginCheck, LoginState};
use nebula_core::profiles::{self, Profile};
use nebula_core::usage::{self, AccountUsage};
use nebula_core::usage_window;
use nebula_core::{ClientRequest, SessionRef, TerminalId};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// One account as the rail lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    /// The usage key: a profile's name, or `@<harness>`; `!<n>` for the
    /// n-th profile entry when it is broken (its name may be the trouble).
    pub key: String,
    /// What the rail shows: the profile's name, or the harness for its
    /// own login.
    pub name: String,
    pub harness: String,
    /// False for a harness's own login (no profile to edit).
    pub profile: bool,
    /// The default a launch naming no profile takes (`default_for`).
    pub default: bool,
    /// Flagged default whether or not it is switched off: the star draws
    /// dimmed, "default (disabled)", on one that is off.
    pub flagged_default: bool,
    pub disabled: bool,
    pub use_default: bool,
    /// Credentials found or not: not proof that they still work.
    pub login: LoginState,
    /// The credentials say the login has run out (Claude's refresh token).
    pub expired: bool,
    /// The command that signs it in, for the user to run: `kete profile
    /// login <name>`, or the CLI's own sign-in for its own login.
    pub login_hint: Option<String>,
    /// Why a profile entry cannot be used; such a row has no usage, login
    /// or switches, only the way to Settings, Profiles.
    pub problem: Option<String>,
    pub usage: Option<AccountUsage>,
    /// The email the login belongs to (Claude's `.claude.json`), shown
    /// beside the meter.
    pub email: Option<String>,
    /// Its PROFILE HOME; None for an own login or an adopted one.
    pub home: Option<String>,
    /// The variable names its profile sets; values never leave Rust.
    pub env_keys: Vec<String>,
}

/// The rows: profiles in config order (broken ones too, with their
/// problem), then each harness's own login in `own` (the ones in use:
/// `accounts::own_login_harnesses`).
pub fn rows(
    all: &[Profile],
    readings: &[AccountUsage],
    own: &[String],
    login: impl Fn(Option<&Profile>, &str) -> LoginCheck,
) -> Vec<AccountRow> {
    let reading = |key: &str| readings.iter().find(|u| u.account == key).cloned();
    let mut out: Vec<AccountRow> = all
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.name.trim().to_string();
            let harness = p.harness.trim().to_string();
            if let Some(problem) = p.problem() {
                return AccountRow {
                    key: format!("!{i}"),
                    name: if name.is_empty() {
                        "(no name)".into()
                    } else {
                        name
                    },
                    harness,
                    profile: true,
                    default: false,
                    flagged_default: false,
                    disabled: p.disabled,
                    use_default: p.use_default,
                    login: LoginState::Unknown,
                    expired: false,
                    login_hint: None,
                    problem: Some(problem),
                    usage: None,
                    email: None,
                    home: None,
                    env_keys: p.env.keys().cloned().collect(),
                };
            }
            let check = login(Some(p), &harness);
            AccountRow {
                key: name.clone(),
                default: profiles::default_for(all, &harness).is_some_and(|d| d.name == p.name),
                flagged_default: profiles::flagged_default(all, &harness)
                    .is_some_and(|d| d.name == p.name),
                disabled: p.disabled,
                use_default: p.use_default,
                login: check.state,
                expired: check.expired,
                login_hint: accounts::login_command(Some(p), &harness),
                problem: None,
                usage: reading(&name),
                email: email_of(p.harness.trim(), p.home().map(|h| h.join("claude"))),
                home: p.home().map(|h| h.display().to_string()),
                env_keys: p.env.keys().cloned().collect(),
                profile: true,
                name,
                harness,
            }
        })
        .collect();
    for harness in own {
        let key = usage::account_key(None, harness);
        let check = login(None, harness);
        out.push(AccountRow {
            usage: reading(&key),
            email: email_of(harness, nebula_core::paths::claude_config_dir()),
            key,
            name: harness.clone(),
            profile: false,
            default: false,
            flagged_default: false,
            disabled: false,
            use_default: true,
            login: check.state,
            expired: check.expired,
            login_hint: accounts::login_command(None, harness),
            problem: None,
            home: None,
            env_keys: Vec::new(),
            harness: harness.clone(),
        });
    }
    out
}

/// The own logins in use: a kept reading under `@<harness>`, or a live
/// session of the harness with no profile.
fn own_logins(readings: &[AccountUsage], model: &crate::daemon::Model) -> Vec<String> {
    accounts::own_login_harnesses(readings.iter().map(|u| u.account.as_str()), &model.agents)
}

fn list_in(root: &Path, all: &[Profile], model: &crate::daemon::Model) -> Vec<AccountRow> {
    let readings = usage::read_all_in(root);
    let now = nebula_core::clock::now_ms();
    rows(all, &readings, &own_logins(&readings, model), |p, h| {
        accounts::login_check_in(p, h, root, now)
    })
}

#[cfg(test)]
fn list_at(config: &Path, root: &Path, model: &crate::daemon::Model) -> Vec<AccountRow> {
    let all = nebula_core::settings::read_object(config)
        .ok()
        .flatten()
        .map(|obj| profiles::from_settings(&obj))
        .unwrap_or_default();
    list_in(root, &all, model)
}

pub fn set_enabled_at(config: &Path, name: &str, enabled: bool) -> Res<()> {
    profiles::set_enabled_in_file(config, name, enabled)
}

/// The account email of a Claude login whose config dir is `dir`.
fn email_of(harness: &str, dir: Option<std::path::PathBuf>) -> Option<String> {
    (harness == "claude")
        .then_some(dir)
        .flatten()
        .and_then(|dir| nebula_core::claude_usage::account_email(&dir))
}

/// A window coming back to the front: ask the daemon for fresh plan usage
/// now instead of within the minute.
#[tauri::command]
pub fn accounts_usage_nudge() {
    nebula_core::claude_usage::nudge();
}

/// The shared formatter (`usage_window::format`) for the windows the
/// view is showing, in the OS time zone. One call per refresh, not one per
/// bar, so the view and the window title cannot word a reset differently.
#[tauri::command]
pub fn usage_labels(windows: Vec<usage::UsageWindow>, now_secs: i64) -> Vec<usage_window::Label> {
    windows
        .iter()
        .map(|w| usage_window::format(w, now_secs, usage_window::local))
        .collect()
}

/// The window title and taskbar tooltip: `kete · 87% · resets 9:50 pm`
/// for the fullest account's 5-hour window, plain `kete` with no reading.
pub fn window_title(
    accounts: &std::collections::HashMap<String, AccountUsage>,
    now_secs: i64,
    tz: impl Fn(i64) -> i32 + Copy,
) -> String {
    let fullest = accounts
        .values()
        .filter_map(|u| usage_window::headline(&u.windows, now_secs))
        .filter(|w| w.shown_at(now_secs).is_some())
        .max_by(|a, b| {
            a.percent_at(now_secs)
                .partial_cmp(&b.percent_at(now_secs))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    match fullest {
        Some(w) => format!("kete · {}", usage_window::format(w, now_secs, tz).compact),
        None => "kete".into(),
    }
}

#[tauri::command]
pub fn accounts_list(client: tauri::State<'_, Arc<Client>>) -> Vec<AccountRow> {
    list_in(
        &profiles::profiles_root(),
        &profiles::load(),
        &client.snapshot(),
    )
}

#[tauri::command]
pub fn profile_set_enabled(name: String, enabled: bool) -> Res<()> {
    // The layer the profiles come from, under the config lock, as every
    // other profile edit.
    crate::profiles::on_layer(|config, _| set_enabled_at(config, &name, enabled))
}

/// A new shell in `worktree` with `line` typed into it and Enter pressed;
/// its terminal id. The shell buffers what arrives before its prompt.
pub async fn run_in_new_terminal(client: &Client, worktree: String, line: &str) -> Res<String> {
    let terminal = crate::new_terminal(client, worktree).await?;
    client
        .send(ClientRequest::Input {
            session: SessionRef::Terminal(TerminalId(terminal.clone())),
            data: format!("{line}\r").into_bytes(),
        })
        .map_err(|e| e.to_string())?;
    Ok(terminal)
}

/// The rail's "Run login": the account's login command (looked up here,
/// never taken from the window), run in a new terminal in `worktree`.
#[tauri::command]
pub async fn account_login_run(
    client: tauri::State<'_, Arc<Client>>,
    worktree: String,
    key: String,
) -> Res<String> {
    let line = list_in(
        &profiles::profiles_root(),
        &profiles::load(),
        &client.snapshot(),
    )
    .into_iter()
    .find(|r| r.key == key)
    .and_then(|r| r.login_hint)
    .ok_or_else(|| format!("no login command for `{key}`"))?;
    run_in_new_terminal(&client, worktree, &line).await
}

/// Refuse unless the connected daemon takes `SwitchAgentAccount`: an older
/// one would drop the whole connection on the unknown request.
pub fn require_switch(client: &Client) -> Res<()> {
    match client.daemon_protocol() {
        0 => Err("Not connected to the kete daemon.".into()),
        v if v < nebula_core::SWITCH_ACCOUNT_PROTOCOL => {
            Err(crate::orchestrator::RELOAD_HINT.into())
        }
        _ => Ok(()),
    }
}

/// Move a session to another account: the daemon restarts it fresh on
/// `profile` (None: its harness's own login) in the same worktree.
pub async fn switch_account_in(client: &Client, agent: String, profile: Option<String>) -> Res<()> {
    require_switch(client)?;
    let profile = profile
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    crate::sessions::ack(client, |req_id| ClientRequest::SwitchAgentAccount {
        req_id,
        id: nebula_core::AgentId(agent),
        profile,
    })
    .await
    .map(drop)
}

#[tauri::command]
pub async fn account_switch(
    client: tauri::State<'_, Arc<Client>>,
    agent: String,
    profile: Option<String>,
) -> Res<()> {
    switch_account_in(&client, agent, profile).await
}

/// Set (or with `profile` None, clear) the default account of `harness`
/// for the project at `repo` in the settings file at `config`. A profile
/// must exist and be an account for `harness`.
pub fn set_project_account_at(
    config: &Path,
    all: &[Profile],
    repo: &str,
    harness: &str,
    profile: Option<&str>,
) -> Res<()> {
    let profile = profile.map(str::trim).filter(|p| !p.is_empty());
    if let Some(name) = profile {
        let of = profiles::project_default_harness(all, name)?;
        if of != harness.trim() {
            return Err(format!("{name} is a {of} account, not {}.", harness.trim()));
        }
    }
    profiles::set_project_default_in_file(config, repo, harness, profile)
}

/// The window's "Default account" for a project (by id): written under its
/// repo path in the layer whose `projects` key is in force, under the
/// settings lock.
#[tauri::command]
pub fn project_set_account(
    client: tauri::State<'_, Arc<Client>>,
    project: String,
    harness: String,
    profile: Option<String>,
) -> Res<()> {
    let repo = client
        .snapshot()
        .projects
        .iter()
        .find(|p| p.id.0 == project)
        .map(|p| p.repo_path.to_string_lossy().into_owned())
        .ok_or_else(|| "That repo is gone.".to_string())?;
    let config = nebula_core::paths::config_path();
    let local = nebula_core::paths::config_local_path();
    let layer = profiles::layer_for(&config, &local, profiles::PROJECTS_KEY)?;
    let _lock = if layer == local {
        Some(nebula_core::settings::lock(&config).map_err(|e| e.to_string())?)
    } else {
        None
    };
    set_project_account_at(
        &layer,
        &profiles::load(),
        &repo,
        &harness,
        profile.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::usage::{UsageSource, UsageWindow, WindowKind};
    use std::collections::BTreeMap;

    fn profile(name: &str, harness: &str) -> Profile {
        Profile {
            name: name.into(),
            harness: harness.into(),
            env: BTreeMap::new(),
            default: false,
            use_default: false,
            disabled: false,
        }
    }

    fn reading(key: &str, harness: &str) -> AccountUsage {
        AccountUsage {
            account: key.into(),
            harness: harness.into(),
            windows: vec![UsageWindow {
                kind: WindowKind::Session,
                label: None,
                percent: 34.0,
                resets_at: None,
                read_at: 0,
                estimated: false,
            }],
            source: UsageSource::Statusline,
            at: 1,
            plan: None,
        }
    }

    fn signed_in(_: Option<&Profile>, _: &str) -> LoginCheck {
        LoginCheck {
            state: LoginState::LoggedIn,
            expired: false,
        }
    }

    #[test]
    fn with_no_profiles_the_harness_own_login_is_the_account() {
        let rows = rows(
            &[],
            &[reading("@claude", "claude")],
            &["claude".into(), "codex".into()],
            signed_in,
        );
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["@claude", "@codex"]);
        assert_eq!(rows[0].name, "claude");
        assert!(!rows[0].profile);
        assert_eq!(rows[0].usage.as_ref().unwrap().windows[0].percent, 34.0);
        assert!(rows[1].usage.is_none(), "a session but no reading yet");
        assert_eq!(
            rows[0].login_hint.as_deref(),
            Some("claude auth login --claudeai"),
            "an own login signs in with the CLI's own command"
        );
    }

    #[test]
    fn profiles_come_first_then_the_own_logins_in_use() {
        let mut work = profile("work", "claude");
        work.default = true;
        let mut off = profile("off", "claude");
        off.disabled = true;
        let rows = rows(
            &[work, off, profile("gh2", "copilot")],
            &[reading("@claude", "claude"), reading("work", "claude")],
            &["claude".into()],
            |p, _| LoginCheck {
                state: match p {
                    Some(p) if p.name == "gh2" => LoginState::NeedsLogin,
                    _ => LoginState::LoggedIn,
                },
                expired: false,
            },
        );
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            ["work", "off", "gh2", "@claude"],
            "the own login in use shows beside the claude profiles"
        );
        assert!(rows[0].default && rows[0].usage.is_some());
        assert!(rows[1].disabled);
        assert_eq!(rows[2].login, LoginState::NeedsLogin);
        assert_eq!(
            rows[2].login_hint.as_deref(),
            Some("kete profile login gh2")
        );
    }

    /// A switched-off default keeps a dimmed star: flagged, not in force.
    /// An adopted login has a login command too.
    #[test]
    fn a_disabled_default_stays_flagged_and_every_profile_can_sign_in() {
        let mut main = profile("main", "claude");
        main.default = true;
        main.disabled = true;
        main.use_default = true;
        let rows = rows(&[main], &[], &[], signed_in);
        assert!(!rows[0].default && rows[0].flagged_default);
        assert_eq!(
            rows[0].login_hint.as_deref(),
            Some("kete profile login main")
        );
    }

    #[test]
    fn a_broken_profile_is_listed_with_its_problem() {
        let rows = rows(
            &[profile("bad name", "claude"), profile("", "claude")],
            &[reading("bad name", "claude")],
            &[],
            |_, _| panic!("a broken profile's login is never looked up"),
        );
        assert_eq!(rows[0].key, "!0");
        assert_eq!(rows[0].name, "bad name");
        assert!(rows[0].problem.as_deref().unwrap().contains("letters"));
        assert!(rows[0].usage.is_none() && rows[0].login_hint.is_none());
        assert_eq!(rows[1].key, "!1");
        assert_eq!(rows[1].name, "(no name)");
    }

    #[test]
    fn enabling_and_disabling_writes_the_flag_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(
            &config,
            r#"{"profiles":[{"name":"work","harness":"claude","colour":"red"}]}"#,
        )
        .unwrap();
        set_enabled_at(&config, "work", false).unwrap();
        let model = crate::daemon::Model::default();
        let listed = list_at(&config, dir.path(), &model);
        assert!(listed[0].disabled);
        set_enabled_at(&config, "work", true).unwrap();
        let raw = std::fs::read_to_string(&config).unwrap();
        assert!(!raw.contains("disabled") && raw.contains("colour"), "{raw}");
        assert!(set_enabled_at(&config, "nope", true).is_err());
    }

    /// The switch lands in config.local.json when that layer's list is the
    /// one in force, as the other profile edits do.
    #[test]
    fn the_switch_edits_the_layer_the_profiles_come_from() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &config,
            r#"{"profiles":[{"name":"base","harness":"claude"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"{"profiles":[{"name":"work","harness":"claude"}]}"#,
        )
        .unwrap();
        let layer = crate::profiles::profiles_layer(&config, &local);
        assert_eq!(layer, local);
        set_enabled_at(&layer, "work", false).unwrap();
        assert!(std::fs::read_to_string(&local)
            .unwrap()
            .contains("disabled"));
        assert!(!std::fs::read_to_string(&config)
            .unwrap()
            .contains("disabled"));
    }

    /// A 56 daemon never gets the switch (it would drop the connection);
    /// a 56 one gets the request with the profile trimmed, or none.
    #[tokio::test]
    async fn a_switch_goes_only_to_a_daemon_that_takes_it() {
        use crate::sessions::testkit::{ok, run};
        let (client, mut rx) = Client::connected_for_test();
        client.set_model(|m| m.protocol_version = 56);
        let refused = switch_account_in(&client, "a1".into(), Some("work".into())).await;
        assert_eq!(refused.unwrap_err(), crate::orchestrator::RELOAD_HINT);
        assert!(rx.try_recv().is_err(), "nothing may reach a 56 daemon");

        client.set_model(|m| m.protocol_version = nebula_core::SWITCH_ACCOUNT_PROTOCOL);
        for (asked, sent) in [
            (Some(" work "), Some("work")),
            (Some(""), None),
            (None, None),
        ] {
            let (out, seen) = run(
                &client,
                &mut rx,
                switch_account_in(&client, "a1".into(), asked.map(str::to_string)),
                ok,
            )
            .await;
            out.unwrap();
            let ClientRequest::SwitchAgentAccount { id, profile, .. } = &seen[0] else {
                panic!("{seen:?}")
            };
            assert_eq!((id.0.as_str(), profile.as_deref()), ("a1", sent));
        }
    }

    #[test]
    fn a_project_account_must_be_one_of_its_harness() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let mut gpt = profile("gpt", "codex");
        gpt.disabled = true;
        let all = vec![profile("work", "claude"), gpt];
        set_project_account_at(&config, &all, "/repo", "claude", Some("work")).unwrap();
        let err =
            set_project_account_at(&config, &all, "/repo", "claude", Some("gpt")).unwrap_err();
        assert!(err.contains("codex account"), "{err}");
        assert!(set_project_account_at(&config, &all, "/repo", "claude", Some("gone")).is_err());
        set_project_account_at(&config, &all, "/repo", "codex", Some("gpt")).unwrap();
        let obj = nebula_core::settings::read_object(&config)
            .unwrap()
            .unwrap();
        let defaults = profiles::project_defaults_from(&obj);
        assert_eq!(defaults["/repo"]["claude"], "work");
        assert_eq!(
            defaults["/repo"]["codex"], "gpt",
            "a switched-off one may be set"
        );
        set_project_account_at(&config, &all, "/repo", "claude", None).unwrap();
        set_project_account_at(&config, &all, "/repo", "codex", Some(" ")).unwrap();
        let obj = nebula_core::settings::read_object(&config)
            .unwrap()
            .unwrap();
        assert!(profiles::project_defaults_from(&obj).is_empty());
    }

    /// Run login: a shell in the worktree, the command typed into it.
    #[tokio::test]
    async fn run_login_opens_a_shell_and_types_the_command() {
        use crate::sessions::testkit::run;
        use nebula_core::EntityId;
        let (client, mut rx) = Client::connected_for_test();
        let (out, seen) = run(
            &client,
            &mut rx,
            run_in_new_terminal(&client, "w1".into(), "kete profile login work"),
            |_| Ok(Some(EntityId::Terminal(TerminalId("t9".into())))),
        )
        .await;
        assert_eq!(out.unwrap(), "t9");
        assert!(
            matches!(&seen[0], ClientRequest::CreateTerminal { worktree, .. } if worktree.0 == "w1")
        );
        let ClientRequest::Input { session, data } = &seen[1] else {
            panic!("{seen:?}")
        };
        assert_eq!(*session, SessionRef::Terminal(TerminalId("t9".into())));
        assert_eq!(data, b"kete profile login work\r");
    }

    #[test]
    fn the_window_title_carries_the_fullest_five_hour_window_in_local_time() {
        let now = nebula_core::claude_usage::epoch_secs("2026-10-19T07:38:00Z").unwrap();
        let reset = nebula_core::claude_usage::epoch_secs("2026-10-19T08:50:00Z").unwrap();
        let account = |name: &str, percent: f32| {
            (
                name.to_string(),
                AccountUsage {
                    account: name.into(),
                    harness: "claude".into(),
                    windows: vec![UsageWindow {
                        kind: WindowKind::Session,
                        label: None,
                        percent,
                        resets_at: Some(reset),
                        read_at: now - 60,
                        estimated: false,
                    }],
                    source: UsageSource::Statusline,
                    at: 0,
                    plan: None,
                },
            )
        };
        let tz = |_| 13 * 3600;
        let none = std::collections::HashMap::new();
        assert_eq!(window_title(&none, now, tz), "kete");
        let two = [account("a", 40.0), account("b", 87.0)]
            .into_iter()
            .collect();
        assert_eq!(window_title(&two, now, tz), "kete · 87% · resets 9:50 pm");
    }
}
