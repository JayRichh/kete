//! The rail's ACCOUNTS: every profile, plus each harness's own login
//! (`@claude`) that is in use but has no profile, with its last plan
//! usage, whether it is signed in, and the command that signs it in.
//! Usage here is what is kept on disk (`nebula_core::usage`); the live
//! `AccountUsage` events in the model take over once they arrive. The
//! window never signs anyone in.

use crate::Res;
use nebula_core::accounts::{self, LoginState};
use nebula_core::profiles::{self, Profile};
use nebula_core::usage::{self, AccountUsage};
use serde::Serialize;
use std::path::Path;

/// One account as the rail lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    /// The usage key: a profile's name, or `@<harness>`.
    pub key: String,
    /// What the rail shows: the profile's name, or the harness for its
    /// own login.
    pub name: String,
    pub harness: String,
    /// False for a harness's own login (no profile to edit).
    pub profile: bool,
    pub default: bool,
    pub disabled: bool,
    pub use_default: bool,
    pub login: LoginState,
    /// `kete profile login <name>`, for the user to run.
    pub login_hint: Option<String>,
    pub usage: Option<AccountUsage>,
}

/// The rows: profiles in config order, then a harness's own login for
/// each harness with no profile that has a reading or a session.
pub fn rows(
    all: &[Profile],
    readings: &[AccountUsage],
    session_harnesses: &[String],
    login: impl Fn(Option<&Profile>, &str) -> LoginState,
) -> Vec<AccountRow> {
    let reading = |key: &str| readings.iter().find(|u| u.account == key).cloned();
    let mut out: Vec<AccountRow> = all
        .iter()
        .filter(|p| p.problem().is_none())
        .map(|p| AccountRow {
            key: p.name.trim().to_string(),
            name: p.name.trim().to_string(),
            harness: p.harness.trim().to_string(),
            profile: true,
            default: profiles::default_for(all, &p.harness).is_some_and(|d| d.name == p.name),
            disabled: p.disabled,
            use_default: p.use_default,
            login: login(Some(p), &p.harness),
            login_hint: profiles::login_hint(p),
            usage: reading(p.name.trim()),
        })
        .collect();
    let mut own: Vec<String> = readings
        .iter()
        .filter_map(|u| usage::default_login_harness(&u.account).map(str::to_string))
        .chain(session_harnesses.iter().cloned())
        .collect();
    own.sort();
    own.dedup();
    for harness in own {
        if all.iter().any(|p| p.harness.trim() == harness) {
            continue;
        }
        let key = usage::account_key(None, &harness);
        out.push(AccountRow {
            usage: reading(&key),
            key,
            name: harness.clone(),
            profile: false,
            default: false,
            disabled: false,
            use_default: true,
            login: login(None, &harness),
            login_hint: None,
            harness,
        });
    }
    out
}

/// The harnesses kete has sessions of that accounts track (the ones with
/// a known login: claude, codex, copilot).
fn session_harnesses(model: &crate::daemon::Model) -> Vec<String> {
    let mut out: Vec<String> = model
        .agents
        .iter()
        .filter(|a| !a.archived)
        .map(|a| a.kind.as_str().to_string())
        .filter(|h| matches!(h.as_str(), "claude" | "codex" | "copilot"))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
fn list_at(config: &Path, root: &Path, model: &crate::daemon::Model) -> Vec<AccountRow> {
    let all = nebula_core::settings::read_object(config)
        .ok()
        .flatten()
        .map(|obj| profiles::from_settings(&obj))
        .unwrap_or_default();
    rows(
        &all,
        &usage::read_all_in(root),
        &session_harnesses(model),
        |p, h| accounts::login_state_in(p, h, root),
    )
}

pub fn set_enabled_at(config: &Path, name: &str, enabled: bool) -> Res<()> {
    profiles::set_enabled_in_file(config, name, enabled)
}

#[tauri::command]
pub fn accounts_list(
    client: tauri::State<'_, std::sync::Arc<crate::daemon::Client>>,
) -> Vec<AccountRow> {
    let all = profiles::load();
    let root = profiles::profiles_root();
    rows(
        &all,
        &usage::read_all_in(&root),
        &session_harnesses(&client.snapshot()),
        accounts::login_state,
    )
}

#[tauri::command]
pub fn profile_set_enabled(name: String, enabled: bool) -> Res<()> {
    set_enabled_at(&nebula_core::paths::config_path(), &name, enabled)
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
            }],
            source: UsageSource::Statusline,
            at: 1,
            plan: None,
        }
    }

    #[test]
    fn with_no_profiles_the_harness_own_login_is_the_account() {
        let rows = rows(
            &[],
            &[reading("@claude", "claude")],
            &["claude".into(), "codex".into()],
            |_, _| LoginState::LoggedIn,
        );
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["@claude", "@codex"]);
        assert_eq!(rows[0].name, "claude");
        assert!(!rows[0].profile);
        assert_eq!(rows[0].usage.as_ref().unwrap().windows[0].percent, 34.0);
        assert!(rows[1].usage.is_none(), "a session but no reading yet");
        assert_eq!(rows[0].login_hint, None, "an own login has no kete login");
    }

    #[test]
    fn profiles_come_first_and_hide_their_harness_own_login() {
        let mut work = profile("work", "claude");
        work.default = true;
        let mut off = profile("off", "claude");
        off.disabled = true;
        let rows = rows(
            &[work, off, profile("gh2", "copilot")],
            &[reading("@claude", "claude"), reading("work", "claude")],
            &["claude".into()],
            |p, _| match p {
                Some(p) if p.name == "gh2" => LoginState::NeedsLogin,
                _ => LoginState::LoggedIn,
            },
        );
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            ["work", "off", "gh2"],
            "no @claude beside claude profiles"
        );
        assert!(rows[0].default && rows[0].usage.is_some());
        assert!(rows[1].disabled);
        assert_eq!(rows[2].login, LoginState::NeedsLogin);
        assert_eq!(
            rows[2].login_hint.as_deref(),
            Some("kete profile login gh2")
        );
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
}
