//! Profiles from the window: list, add, edit, duplicate, rename, remove
//! (and purge) and pick the default,
//! through `nebula_core::profiles` exactly as `kete profile` does (the
//! harness checked against the registry, the PROFILE HOME made on add).
//! The window never signs a profile in: where one needs a login it shows
//! the command for the user to run (`kete profile login <name>`).

use crate::Res;
use nebula_config::Config;
use nebula_core::profiles::{self, Profile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One profile as the window lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRow {
    pub name: String,
    pub harness: String,
    pub default: bool,
    pub use_default: bool,
    /// The variable names it sets; values stay out of the list.
    pub env_keys: Vec<String>,
    /// Its PROFILE HOME; None for an adopted default login.
    pub home: Option<String>,
    /// Why the entry is unusable, if it is.
    pub problem: Option<String>,
    /// The command that signs it in, for the user to run.
    pub login: Option<String>,
    /// The last plan usage its sessions printed (`91% of weekly`).
    pub usage: Option<String>,
}

/// What the add and edit forms send.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileForm {
    pub name: String,
    pub harness: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub use_default: bool,
    /// `KEY=VALUE` lines; blank lines skipped.
    #[serde(default)]
    pub env: String,
}

/// What an add or edit did, for the window to say.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSaved {
    pub rows: Vec<ProfileRow>,
    /// The sign-in command to run next, when the profile needs one.
    pub login: Option<String>,
    pub home: Option<String>,
    /// The remove deleted the home.
    pub purged: bool,
}

fn rows(all: &[Profile], root: &Path) -> Vec<ProfileRow> {
    all.iter()
        .map(|p| ProfileRow {
            name: p.name.clone(),
            harness: p.harness.clone(),
            default: profiles::default_for(all, &p.harness).is_some_and(|d| d.name == p.name),
            use_default: p.use_default,
            env_keys: p.env.keys().cloned().collect(),
            home: p.home_in(root).map(|h| h.display().to_string()),
            problem: p.problem(),
            login: nebula_core::accounts::login_command(Some(p), &p.harness),
            usage: profiles::read_usage_in(root, p.name.trim()).map(|u| u.summary()),
        })
        .collect()
}

fn env_of(text: &str) -> Res<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("Environment lines are KEY=VALUE, not \"{line}\"."));
        };
        if key.trim().is_empty() {
            return Err(format!("\"{line}\" has no variable name."));
        }
        vars.insert(key.trim().to_string(), value.to_string());
    }
    Ok(vars)
}

/// The profile a form describes, its harness checked against `registry`
/// as `kete profile add` checks it.
fn profile_of(form: ProfileForm, registry: &[String]) -> Res<Profile> {
    let harness = form.harness.trim();
    if !registry.iter().any(|h| h == harness) {
        return Err(format!(
            "Unknown harness \"{harness}\": expected one of {}.",
            registry.join(", ")
        ));
    }
    Ok(Profile {
        name: form.name.trim().to_string(),
        harness: harness.to_string(),
        env: env_of(&form.env)?,
        default: form.default,
        use_default: form.use_default,
        disabled: false,
        reserved: false,
    })
}

fn load_at(config: &Path) -> Vec<Profile> {
    nebula_core::settings::read_object(config)
        .ok()
        .flatten()
        .map(|obj| profiles::from_settings(&obj))
        .unwrap_or_default()
}

pub fn list_at(config: &Path, root: &Path) -> Vec<ProfileRow> {
    rows(&load_at(config), root)
}

pub fn add_at(
    config: &Path,
    root: &Path,
    registry: &[String],
    form: ProfileForm,
) -> Res<ProfileSaved> {
    let profile = profile_of(form, registry)?;
    profiles::add_to_file(config, &profile)?;
    let home = profile.home_in(root);
    if let Some(home) = &home {
        profile
            .prepare_in(root, None)
            .map_err(|e| format!("Added, but could not make {}: {e}", home.display()))?;
    }
    Ok(ProfileSaved {
        rows: list_at(config, root),
        login: sign_in_next(&profile, home.as_deref()),
        home: home.map(|h| h.display().to_string()),
        purged: false,
    })
}

/// Edit a profile. The name is the lookup and stays (it names the home).
/// A form that sends an empty env keeps the stored variables only when
/// `keep_env` says so, since the list never carries their values.
pub fn update_at(
    config: &Path,
    root: &Path,
    registry: &[String],
    form: ProfileForm,
    keep_env: bool,
) -> Res<ProfileSaved> {
    let mut profile = profile_of(form, registry)?;
    if let Some(old) = load_at(config).into_iter().find(|p| p.name == profile.name) {
        if keep_env {
            profile.env = old.env;
        }
        // The form has no enabled box: the accounts list's switch stays.
        profile.disabled = old.disabled;
    }
    profiles::update_in_file(config, &profile)?;
    let home = profile.home_in(root);
    if home.is_some() {
        profile
            .prepare_in(root, None)
            .map_err(|e| format!("Saved, but could not make the profile home: {e}"))?;
    }
    Ok(ProfileSaved {
        rows: list_at(config, root),
        login: sign_in_next(&profile, home.as_deref()),
        home: home.map(|h| h.display().to_string()),
        purged: false,
    })
}

/// Remove a profile, and with `purge` its home (the login in it) too:
/// `profiles::remove_in_file`, the path `kete profile remove [--purge]`
/// takes. Kept, the answer says where the home stays.
pub fn remove_at(config: &Path, root: &Path, name: &str, purge: bool) -> Res<ProfileSaved> {
    let removed = profiles::remove_in_file(config, root, name, purge)?;
    Ok(ProfileSaved {
        rows: list_at(config, root),
        login: None,
        home: removed.home.map(|h| h.display().to_string()),
        purged: removed.purged,
    })
}

/// The sign-in to run next for a profile just made: a fresh home always
/// needs one; an adopted login is left to its row's login state.
fn sign_in_next(profile: &Profile, home: Option<&Path>) -> Option<String> {
    home?;
    nebula_core::accounts::login_command(Some(profile), &profile.harness)
}

pub fn set_default_at(config: &Path, root: &Path, name: &str) -> Res<Vec<ProfileRow>> {
    profiles::set_default_in_file(config, name)?;
    Ok(list_at(config, root))
}

fn registry() -> Vec<String> {
    Config::load()
        .harness_registry()
        .iter()
        .map(|h| h.id.clone())
        .collect()
}

/// Add `name` as a copy of `from` (`profiles::duplicate_in_file`): its
/// harness, its variables (values included: they never pass through the
/// window) and its kind of login, but neither its default star nor its
/// switch, and a home of its own (empty: the copy signs in separately).
pub fn duplicate_at(config: &Path, root: &Path, from: &str, name: &str) -> Res<ProfileSaved> {
    let copy = profiles::duplicate_in_file(config, from, name)?;
    let home = copy.home_in(root);
    if let Some(home) = &home {
        copy.prepare_in(root, None)
            .map_err(|e| format!("Added, but could not make {}: {e}", home.display()))?;
    }
    Ok(ProfileSaved {
        rows: list_at(config, root),
        login: sign_in_next(&copy, home.as_deref()),
        home: home.map(|h| h.display().to_string()),
        purged: false,
    })
}

#[tauri::command]
pub fn profile_duplicate(from: String, name: String) -> Res<ProfileSaved> {
    on_layer(|config, root| duplicate_at(config, root, &from, &name))
}

/// What a rename did, for the window to say.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRenamed {
    pub name: String,
    /// Where its home is now; None for an adopted login.
    pub home: Option<String>,
    /// The repos (as written) whose default account it is.
    pub projects: Vec<String>,
    /// Archived sessions that still name the old one.
    pub archived: Vec<String>,
    /// Sessions the daemon moved onto the new name (protocol 60).
    #[serde(default)]
    pub followed: usize,
}

/// Rename `old` to `new` through `profiles::rename` (the path `kete
/// profile rename` takes): refused while one of `agents` names it, the
/// home moved, the project defaults and the reading following. It takes
/// the settings locks itself.
pub fn rename_at(
    config: &Path,
    local: &Path,
    root: &Path,
    old: &str,
    new: &str,
    agents: &[nebula_core::Agent],
) -> Res<ProfileRenamed> {
    let done = profiles::rename(config, local, root, old, new, agents)?;
    Ok(renamed(done))
}

fn renamed(done: profiles::Renamed) -> ProfileRenamed {
    ProfileRenamed {
        name: done.profile.name,
        home: done.home.map(|(_, to)| to.display().to_string()),
        projects: done.projects,
        archived: done.archived,
        followed: 0,
    }
}

/// The rename against `client`'s daemon: one that rewrites the sessions'
/// stored name (`RENAME_PROFILE_PROTOCOL`) is not refused by them, and
/// is asked to move them once the settings say the new name; an older one
/// refuses while one names it (`rename_at`).
pub async fn rename_on(
    client: &crate::daemon::Client,
    config: &Path,
    local: &Path,
    root: &Path,
    old: &str,
    new: &str,
) -> Res<ProfileRenamed> {
    let model = client.snapshot();
    if !model.connected {
        return Err(
            "Not connected to the kete daemon: a rename checks the sessions on the account first."
                .into(),
        );
    }
    if client.daemon_protocol() < nebula_core::RENAME_PROFILE_PROTOCOL {
        return rename_at(config, local, root, old, new, &model.agents);
    }
    let mut out = renamed(profiles::rename_following(config, local, root, old, new)?);
    let followed = model
        .agents
        .iter()
        .filter(|a| a.profile.as_deref().map(str::trim) == Some(old.trim()))
        .count();
    if followed > 0 {
        let (old, new) = (old.trim().to_string(), out.name.clone());
        crate::sessions::ack(client, |req_id| {
            nebula_core::ClientRequest::RenameProfileSessions { req_id, old, new }
        })
        .await
        .map_err(|e| format!("Renamed, but the sessions still name the old one: {e}"))?;
        out.followed = followed;
    }
    Ok(out)
}

/// Rename an account, checked against the daemon's sessions: refused
/// while the window is not connected, since the sessions naming it are
/// unknown then (`rename_on`).
#[tauri::command]
pub async fn profile_rename(
    client: tauri::State<'_, std::sync::Arc<crate::daemon::Client>>,
    old: String,
    new: String,
) -> Res<ProfileRenamed> {
    rename_on(
        &client,
        &nebula_core::paths::config_path(),
        &nebula_core::paths::config_local_path(),
        &profiles::profiles_root(),
        &old,
        &new,
    )
    .await
}

/// The layer the profiles come from, so the one an edit must change: the
/// local layer's `profiles` list replaces config.json's whole (layers
/// merge key by key), so when config.local.json holds one, every listed
/// profile is from there.
pub fn profiles_layer(config: &Path, local: &Path) -> std::path::PathBuf {
    let local_holds = nebula_core::settings::read_object(local)
        .ok()
        .flatten()
        .is_some_and(|obj| obj.contains_key(profiles::CONFIG_KEY));
    if local_holds { local } else { config }.to_path_buf()
}

/// Run a profile edit on the layer the profiles come from. An edit of the
/// local layer also holds config.json's lock, which the settings writers
/// take for both layers.
pub(crate) fn on_layer<T>(edit: impl FnOnce(&Path, &Path) -> Res<T>) -> Res<T> {
    let config = nebula_core::paths::config_path();
    let local = nebula_core::paths::config_local_path();
    let layer = profiles_layer(&config, &local);
    let _lock = if layer == local {
        Some(nebula_core::settings::lock(&config).map_err(|e| e.to_string())?)
    } else {
        None
    };
    edit(&layer, &profiles::profiles_root())
}

/// Every profile, as sessions see them (the local layer over config.json).
#[tauri::command]
pub fn profiles_list() -> Vec<ProfileRow> {
    rows(&profiles::load(), &profiles::profiles_root())
}

/// Every harness id a profile can be for.
#[tauri::command]
pub fn profile_harnesses() -> Vec<String> {
    registry()
}

#[tauri::command]
pub fn profile_add(form: ProfileForm) -> Res<ProfileSaved> {
    let registry = registry();
    on_layer(|config, root| add_at(config, root, &registry, form))
}

#[tauri::command]
pub fn profile_update(form: ProfileForm, keep_env: bool) -> Res<ProfileSaved> {
    let registry = registry();
    on_layer(|config, root| update_at(config, root, &registry, form, keep_env))
}

#[tauri::command]
pub fn profile_remove(name: String, purge: Option<bool>) -> Res<ProfileSaved> {
    let purge = purge.unwrap_or(false);
    on_layer(|config, root| remove_at(config, root, &name, purge))
}

#[tauri::command]
pub fn profile_set_default(name: String) -> Res<Vec<ProfileRow>> {
    on_layer(|config, root| set_default_at(config, root, &name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn form(name: &str, harness: &str) -> ProfileForm {
        ProfileForm {
            name: name.into(),
            harness: harness.into(),
            default: false,
            use_default: false,
            env: String::new(),
        }
    }

    fn setup() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        Vec<String>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, r#"{"theme": "ocean"}"#).unwrap();
        let root = dir.path().join("profiles");
        let registry = vec![
            "claude".to_string(),
            "codex".to_string(),
            "cursor".to_string(),
        ];
        (dir, config, root, registry)
    }

    #[test]
    fn an_add_writes_the_list_makes_the_home_and_names_the_login_command() {
        let (_d, config, root, registry) = setup();
        let mut work = form("work", "claude");
        work.default = true;
        work.env = "HTTPS_PROXY=http://proxy\n\n".into();
        let saved = add_at(&config, &root, &registry, work).unwrap();
        assert_eq!(saved.login.as_deref(), Some("kete profile login work"));
        assert!(root.join("work").join("claude").is_dir(), "home prepared");
        let row = &saved.rows[0];
        assert!(row.default);
        assert_eq!(row.env_keys, vec!["HTTPS_PROXY"]);
        assert!(row.problem.is_none());

        // The file the CLI and the daemon read: other keys kept.
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(raw["theme"], "ocean");
        assert_eq!(raw["profiles"][0]["env"]["HTTPS_PROXY"], "http://proxy");

        // An adopted login has no home and no command to run.
        let mut main = form("main", "codex");
        main.use_default = true;
        let saved = add_at(&config, &root, &registry, main).unwrap();
        assert_eq!(saved.login, None);
        assert_eq!(saved.home, None);
        assert!(!root.join("main").exists());
    }

    #[test]
    fn an_edit_keeps_a_disabled_profile_disabled() {
        let (_d, config, root, registry) = setup();
        add_at(&config, &root, &registry, form("off", "claude")).unwrap();
        profiles::set_enabled_in_file(&config, "off", false).unwrap();
        let mut edit = form("off", "claude");
        edit.default = true;
        update_at(&config, &root, &registry, edit, true).unwrap();
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(
            raw["profiles"][0]["disabled"], true,
            "the form has no enabled box"
        );
        assert_eq!(raw["profiles"][0]["default"], true);
    }

    #[test]
    fn bad_input_is_refused_and_nothing_is_written() {
        let (_d, config, root, registry) = setup();
        let before = std::fs::read_to_string(&config).unwrap();
        assert!(add_at(&config, &root, &registry, form("x", "nope"))
            .unwrap_err()
            .contains("Unknown harness"));
        assert!(add_at(&config, &root, &registry, form("bad name", "claude")).is_err());
        let mut env = form("y", "claude");
        env.env = "NOEQUALS".into();
        assert!(add_at(&config, &root, &registry, env).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
        add_at(&config, &root, &registry, form("z", "claude")).unwrap();
        assert!(add_at(&config, &root, &registry, form("z", "codex"))
            .unwrap_err()
            .contains("exists"));
    }

    #[test]
    fn an_edit_keeps_the_env_unless_replaced_and_moves_the_default() {
        let (_d, config, root, registry) = setup();
        let mut a = form("a", "claude");
        a.default = true;
        a.env = "K=secret".into();
        add_at(&config, &root, &registry, a).unwrap();
        add_at(&config, &root, &registry, form("b", "claude")).unwrap();

        let mut b = form("b", "claude");
        b.default = true;
        let saved = update_at(&config, &root, &registry, b, true).unwrap();
        assert!(!saved.rows[0].default && saved.rows[1].default);

        let saved = update_at(&config, &root, &registry, form("a", "claude"), true).unwrap();
        assert_eq!(saved.rows[0].env_keys, vec!["K"], "kept");
        let saved = update_at(&config, &root, &registry, form("a", "claude"), false).unwrap();
        assert!(saved.rows[0].env_keys.is_empty(), "replaced by nothing");

        let rows = set_default_at(&config, &root, "a").unwrap();
        assert!(rows[0].default && !rows[1].default);
    }

    #[test]
    fn a_duplicate_copies_the_harness_and_variables_but_not_the_star() {
        let (_d, config, root, registry) = setup();
        let mut work = form("work", "claude");
        work.default = true;
        work.env = "K=secret".into();
        add_at(&config, &root, &registry, work).unwrap();
        profiles::set_enabled_in_file(&config, "work", false).unwrap();
        let saved = duplicate_at(&config, &root, "work", "work-2").unwrap();
        assert_eq!(saved.login.as_deref(), Some("kete profile login work-2"));
        assert!(root.join("work-2").join("claude").is_dir(), "its own home");
        let copy = load_at(&config)
            .into_iter()
            .find(|p| p.name == "work-2")
            .unwrap();
        assert_eq!(copy.harness, "claude");
        assert_eq!(copy.env.get("K").map(String::as_str), Some("secret"));
        assert!(!copy.default && !copy.disabled);
        assert!(duplicate_at(&config, &root, "work", "work-2")
            .unwrap_err()
            .contains("exists"));
        assert!(duplicate_at(&config, &root, "gone", "x").is_err());
        assert!(duplicate_at(&config, &root, "work", "bad name").is_err());
    }

    #[test]
    fn a_remove_keeps_the_home_unless_purged_and_says_where() {
        let (_d, config, root, registry) = setup();
        add_at(&config, &root, &registry, form("work", "claude")).unwrap();
        let saved = remove_at(&config, &root, "work", false).unwrap();
        assert!(saved.rows.is_empty());
        assert!(saved.home.unwrap().ends_with("work"));
        assert!(root.join("work").is_dir(), "kept unless purged");
        assert!(remove_at(&config, &root, "work", false).is_err());
    }

    fn agent_on(profile: &str) -> nebula_core::Agent {
        nebula_core::Agent {
            id: nebula_core::AgentId("a".into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: "fix".into(),
            status: nebula_core::AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: nebula_core::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
            profile: Some(profile.into()),
            parent: None,
        }
    }

    /// A session on the profile refuses the rename on a 59 daemon; a 60
    /// one takes it and is asked to move the session onto the new name.
    #[tokio::test]
    async fn a_new_daemon_moves_the_sessions_on_a_renamed_profile() {
        let (_d, config, root, registry) = setup();
        let local = config.with_file_name("config.local.json");
        add_at(&config, &root, &registry, form("work", "claude")).unwrap();
        let (client, mut rx) = crate::daemon::Client::connected_for_test();
        client.set_model(|m| {
            m.protocol_version = 59;
            m.agents.push(agent_on("work"));
        });
        let err = rename_on(&client, &config, &local, &root, "work", "max2")
            .await
            .unwrap_err();
        assert!(err.contains("has sessions"), "{err}");

        client.set_model(|m| m.protocol_version = nebula_core::RENAME_PROFILE_PROTOCOL);
        let (out, seen) = crate::sessions::testkit::run(
            &client,
            &mut rx,
            rename_on(&client, &config, &local, &root, "work", "max2"),
            crate::sessions::testkit::ok,
        )
        .await;
        let out = out.unwrap();
        assert_eq!((out.name.as_str(), out.followed), ("max2", 1));
        assert!(matches!(
            seen.as_slice(),
            [nebula_core::ClientRequest::RenameProfileSessions { old, new, .. }]
                if old == "work" && new == "max2"
        ));
        assert!(root.join("max2").join("claude").is_dir());
    }

    #[test]
    fn a_purge_deletes_the_home_and_a_rename_moves_it() {
        let (_d, config, root, registry) = setup();
        let local = config.with_file_name("config.local.json");
        add_at(&config, &root, &registry, form("work", "claude")).unwrap();
        add_at(&config, &root, &registry, form("gone", "claude")).unwrap();
        let saved = remove_at(&config, &root, "gone", true).unwrap();
        assert!(saved.purged);
        assert!(!root.join("gone").exists(), "purged");

        let done = rename_at(&config, &local, &root, "work", "max2", &[]).unwrap();
        assert_eq!(done.name, "max2");
        assert!(done.home.unwrap().ends_with("max2"));
        assert!(root.join("max2").join("claude").is_dir());
        assert_eq!(list_at(&config, &root)[0].name, "max2");
        assert!(rename_at(&config, &local, &root, "work", "x", &[]).is_err());
    }
}

/// The real command wrappers resolve every file under `NEBULA_DATA_DIR`
/// (never the user's own config): presets and profiles written through
/// them land in a temp data dir.
#[cfg(test)]
mod wrappers {
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn preset_and_profile_commands_write_under_the_data_dir() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let keep_data = std::env::var_os("NEBULA_DATA_DIR");
        let keep_file = std::env::var_os("NEBULA_CONFIG_FILE");
        std::env::set_var("NEBULA_DATA_DIR", dir.path());
        std::env::remove_var("NEBULA_CONFIG_FILE");

        let result = std::panic::catch_unwind(|| {
            assert!(crate::presets::presets_list().is_empty());
            crate::presets::preset_save(
                None,
                serde_json::from_value(serde_json::json!({"name": "r", "harness": "claude"}))
                    .unwrap(),
            )
            .unwrap();
            assert!(dir.path().join("agent_presets.json").is_file());
            assert_eq!(crate::presets::presets_list().len(), 1);

            let saved = super::profile_add(super::ProfileForm {
                name: "work".into(),
                harness: "claude".into(),
                default: true,
                use_default: false,
                env: String::new(),
            })
            .unwrap();
            assert_eq!(saved.login.as_deref(), Some("kete profile login work"));
            assert!(dir.path().join("config.json").is_file());
            assert!(dir.path().join("profiles").join("work").is_dir());
            assert_eq!(super::profiles_list().len(), 1);
            super::profile_remove("work".into(), None).unwrap();
            assert!(super::profiles_list().is_empty());

            // Profiles the local layer defines are edited there.
            let config = dir.path().join("config.json");
            let local = dir.path().join("config.local.json");
            let config_before = std::fs::read_to_string(&config).unwrap();
            std::fs::write(
                &local,
                r#"{"profiles": [{"name": "loc", "harness": "claude", "use_default": true}]}"#,
            )
            .unwrap();
            assert_eq!(super::profiles_list()[0].name, "loc");
            super::profile_set_default("loc".into()).unwrap();
            let held: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
            assert_eq!(held["profiles"][0]["default"], true);
            super::profile_remove("loc".into(), None).unwrap();
            assert!(super::profiles_list().is_empty());
            assert_eq!(std::fs::read_to_string(&config).unwrap(), config_before);
            assert!(!dir.path().join("config.json.lock").exists());
        });

        match keep_data {
            Some(v) => std::env::set_var("NEBULA_DATA_DIR", v),
            None => std::env::remove_var("NEBULA_DATA_DIR"),
        }
        if let Some(v) = keep_file {
            std::env::set_var("NEBULA_CONFIG_FILE", v);
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }
}
