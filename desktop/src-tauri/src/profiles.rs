//! Profiles from the window: list, add, edit, remove and pick the default,
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
            login: profiles::login_hint(p),
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
        login: profiles::login_hint(&profile),
        home: home.map(|h| h.display().to_string()),
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
        login: profiles::login_hint(&profile),
        home: home.map(|h| h.display().to_string()),
    })
}

/// Remove a profile. Its home (the login) stays on disk; the answer says
/// where, and the CLI's `--purge` is how to delete it.
pub fn remove_at(config: &Path, root: &Path, name: &str) -> Res<ProfileSaved> {
    let profile = load_at(config)
        .into_iter()
        .find(|p| p.name.trim() == name.trim());
    if !profiles::remove_from_file(config, name)? {
        return Err(format!("No profile named \"{}\".", name.trim()));
    }
    let home = profile
        .and_then(|p| p.home_in(root))
        .filter(|h| h.exists())
        .map(|h| h.display().to_string());
    Ok(ProfileSaved {
        rows: list_at(config, root),
        login: None,
        home,
    })
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

fn paths() -> (std::path::PathBuf, std::path::PathBuf) {
    (nebula_core::paths::config_path(), profiles::profiles_root())
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
    let (config, root) = paths();
    add_at(&config, &root, &registry(), form)
}

#[tauri::command]
pub fn profile_update(form: ProfileForm, keep_env: bool) -> Res<ProfileSaved> {
    let (config, root) = paths();
    update_at(&config, &root, &registry(), form, keep_env)
}

#[tauri::command]
pub fn profile_remove(name: String) -> Res<ProfileSaved> {
    let (config, root) = paths();
    remove_at(&config, &root, &name)
}

#[tauri::command]
pub fn profile_set_default(name: String) -> Res<Vec<ProfileRow>> {
    let (config, root) = paths();
    set_default_at(&config, &root, &name)
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
    fn a_remove_leaves_the_home_and_says_where() {
        let (_d, config, root, registry) = setup();
        add_at(&config, &root, &registry, form("work", "claude")).unwrap();
        let saved = remove_at(&config, &root, "work").unwrap();
        assert!(saved.rows.is_empty());
        assert!(saved.home.unwrap().ends_with("work"));
        assert!(root.join("work").is_dir(), "never purged from the window");
        assert!(remove_at(&config, &root, "work").is_err());
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
            super::profile_remove("work".into()).unwrap();
            assert!(super::profiles_list().is_empty());
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
