//! PROFILES: several accounts per harness.
//!
//! A PROFILE is a named account for one harness — a second Claude Max
//! subscription, a work ChatGPT login for Codex, another GitHub account for
//! Copilot. Each lives in `config.json`'s `profiles` list ([`Profile`]) and,
//! unless it adopts the CLI's own default login ([`Profile::use_default`]),
//! owns a PROFILE HOME under `<data dir>/profiles/<name>/` holding the
//! harness's config dirs: `claude/` (`CLAUDE_CONFIG_DIR`), `codex/`
//! (`CODEX_HOME`), `copilot/` (`COPILOT_HOME`) with `gh/` beside it
//! (`GH_CONFIG_DIR`). A session spawned under a profile gets those dirs in
//! its environment and every inherited credential variable stripped
//! ([`Profile::launch_env`]), so two subscriptions of one harness run side
//! by side without either seeing the other's login.
//!
//! The session keeps the profile's name (`Agent::profile`), so a RESUME or
//! a restart boots under the same account. A launch that names no profile
//! takes its project's default account for that harness when the project
//! has one ([`PROJECT_DEFAULTS_KEY`]), else the ACCOUNT SPREAD's pick, which
//! under `manual` is the harness's default one ([`resolve`]) — the profile
//! flagged `default`, else none at all: the CLI runs on whatever the
//! daemon's own environment says, as before profiles existed. The whole
//! order is `accounts::resolve_launch`.
//!
//! Design after agent-office's per-account homes (`src/server/signins.ts`,
//! MIT): the same isolating variables, the same stripped credentials, and
//! the same seeded `.claude.json` that marks onboarding done and carries
//! the default config's folder trust over, so a fresh profile's first
//! session asks for a login and nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The `config.json` key holding the list.
pub const CONFIG_KEY: &str = "profiles";

/// One PROFILE: a named account for one harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Unique across every harness: it names the PROFILE HOME directory
    /// and is what a session stores. Letters, digits, `-` and `_`.
    pub name: String,
    /// The harness id it is an account for (`claude`, `codex`, `copilot`,
    /// or a `harnesses` entry's id). Which config dirs it isolates follows
    /// from the id: a harness with none of its own gets only `env`.
    pub harness: String,
    /// Extra variables set on the profile's sessions after the isolating
    /// ones — an API key for an API-billed account, a proxy.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The profile a launch on this harness takes when it names none. At
    /// most one per harness counts: the first flagged wins.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default: bool,
    /// Adopt the CLI's own default login (`~/.claude`, `~/.codex`, …)
    /// instead of a PROFILE HOME: no dir variable is set, nothing is
    /// copied, and only inherited credential variables are stripped — so
    /// the existing login is one of the profiles a picker cycles through.
    #[serde(default, skip_serializing_if = "is_false")]
    pub use_default: bool,
    /// Switched off in the accounts list: never picked for a launch that
    /// names no profile (no default, no spread). A launch that names it
    /// still runs under it (the desktop's quick prompt lists it, marked
    /// off). A session already running under it keeps it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Kept for the person's own launches (0.60): still the default for a
    /// launch Jay starts, never picked for one the orchestrator or a script
    /// starts (`accounts::Launcher::Automation`). Left off the file when
    /// false, so a 0.59 build reads the entry unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reserved: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A config dir a harness keeps its login in, and the variable that
/// moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeDir {
    /// Sub-directory of the PROFILE HOME.
    pub dir: &'static str,
    /// The variable pointing the CLI at it.
    pub var: &'static str,
}

/// The dirs a harness's PROFILE HOME holds. Copilot keeps `gh/` beside its
/// own home: its sessions push and open PRs as the account they run as.
pub fn home_dirs(harness: &str) -> &'static [HomeDir] {
    match harness.trim() {
        "claude" => &[HomeDir {
            dir: "claude",
            var: crate::env::CLAUDE_CONFIG_DIR,
        }],
        "codex" => &[HomeDir {
            dir: "codex",
            var: crate::env::CODEX_HOME,
        }],
        "copilot" => &[
            HomeDir {
                dir: "copilot",
                var: crate::env::COPILOT_HOME,
            },
            HomeDir {
                dir: "gh",
                var: GH_CONFIG_DIR,
            },
        ],
        _ => &[],
    }
}

/// The GitHub CLI's config-dir override.
pub const GH_CONFIG_DIR: &str = "GH_CONFIG_DIR";

/// Inherited variables that would override a profile's login, per
/// harness: an API key or token in the daemon's environment wins over the
/// credentials file in any config dir, so a profile's sessions never see
/// them (its `env` can set one back on purpose). The dir variables are
/// listed too: an isolated profile replaces them, an adopted one keeps
/// what the environment says (see [`Profile::launch_env`]).
pub fn credential_vars(harness: &str) -> &'static [&'static str] {
    match harness.trim() {
        "claude" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        ],
        "codex" => &["OPENAI_API_KEY", "CODEX_API_KEY"],
        "copilot" => &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"],
        _ => &[],
    }
}

/// What a profile does to a session's environment: variables to remove,
/// then variables to set (in order — `env` overrides come last).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchEnv {
    pub unset: Vec<String>,
    pub set: Vec<(String, String)>,
}

impl LaunchEnv {
    /// `unset` with the names `set` assigns taken out: removing a variable
    /// and then setting it is the set alone.
    pub fn removals(&self) -> Vec<String> {
        self.unset
            .iter()
            .filter(|name| !self.set.iter().any(|(set, _)| set == *name))
            .cloned()
            .collect()
    }
}

/// `<data dir>/profiles` — every PROFILE HOME is a directory in here.
pub fn profiles_root() -> PathBuf {
    crate::paths::data_dir().join("profiles")
}

impl Profile {
    /// Why this entry is unusable, or None.
    pub fn problem(&self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some("profile with an empty name".into());
        }
        if name.len() > 40
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Some(format!(
                "profile `{name}`: use letters, digits, `-` and `_` (40 at most)"
            ));
        }
        if self.harness.trim().is_empty() {
            return Some(format!("profile `{name}` names no harness"));
        }
        None
    }

    /// The PROFILE HOME under `root` (normally [`profiles_root`]), or None
    /// for an adopted default login, which has none.
    pub fn home_in(&self, root: &Path) -> Option<PathBuf> {
        (!self.use_default).then(|| root.join(self.name.trim()))
    }

    /// The PROFILE HOME, or None for an adopted default login.
    pub fn home(&self) -> Option<PathBuf> {
        self.home_in(&profiles_root())
    }

    /// Where this profile's harness keeps its config, by variable — the
    /// PROFILE HOME's sub-directories. Empty for an adopted login.
    pub fn dirs_in(&self, root: &Path) -> Vec<(&'static str, PathBuf)> {
        let Some(home) = self.home_in(root) else {
            return Vec::new();
        };
        home_dirs(&self.harness)
            .iter()
            .map(|d| (d.var, home.join(d.dir)))
            .collect()
    }

    /// The dir a variable points at under this profile, when the profile
    /// moves it.
    pub fn dir_for(&self, root: &Path, var: &str) -> Option<PathBuf> {
        self.dirs_in(root)
            .into_iter()
            .find(|(v, _)| *v == var)
            .map(|(_, dir)| dir)
    }

    /// The environment edit a session under this profile gets: every
    /// inherited credential variable of its harness removed; an isolated
    /// profile's dir variables pointed into its PROFILE HOME (an adopted
    /// one leaves them as the environment has them); then `env`.
    pub fn launch_env_in(&self, root: &Path) -> LaunchEnv {
        let mut out = LaunchEnv::default();
        out.unset
            .extend(credential_vars(&self.harness).iter().map(|v| v.to_string()));
        for (var, dir) in self.dirs_in(root) {
            out.set
                .push((var.to_string(), dir.to_string_lossy().into_owned()));
        }
        for (key, value) in &self.env {
            if !key.trim().is_empty() {
                out.set.push((key.trim().to_string(), value.clone()));
            }
        }
        out
    }

    /// [`Profile::launch_env_in`] under [`profiles_root`].
    pub fn launch_env(&self) -> LaunchEnv {
        self.launch_env_in(&profiles_root())
    }

    /// Make the PROFILE HOME's dirs exist, and seed a Claude profile's
    /// `.claude.json` so its first session goes straight to the login:
    /// onboarding marked done, and folder trust for `worktree` carried
    /// over from the default config when the default trusts it (or a
    /// folder above it). A no-op for an adopted login. Writes only when
    /// something changed.
    pub fn prepare_in(&self, root: &Path, worktree: Option<&Path>) -> std::io::Result<()> {
        for (_, dir) in self.dirs_in(root) {
            std::fs::create_dir_all(&dir)?;
        }
        if let Some(config_dir) = self.dir_for(root, crate::env::CLAUDE_CONFIG_DIR) {
            seed_claude_config(&config_dir, default_claude_json().as_deref(), worktree)?;
        }
        Ok(())
    }

    /// [`Profile::prepare_in`] under [`profiles_root`].
    pub fn prepare(&self, worktree: Option<&Path>) -> std::io::Result<()> {
        self.prepare_in(&profiles_root(), worktree)
    }
}

/// The profile a launch on `harness` runs under: `name` when one was
/// picked (it must exist and be an account for that harness), else the
/// harness's default profile, else none. `Err` says why a named profile
/// cannot be used.
pub fn resolve<'a>(
    all: &'a [Profile],
    harness: &str,
    name: Option<&str>,
) -> Result<Option<&'a Profile>, String> {
    let harness = harness.trim();
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => {
            let Some(profile) = all.iter().find(|p| p.name.trim() == name) else {
                return Err(format!("profile `{name}` is no longer defined"));
            };
            if let Some(problem) = profile.problem() {
                return Err(problem);
            }
            if profile.harness.trim() != harness {
                return Err(format!(
                    "profile `{name}` is a {} account, not {harness}",
                    profile.harness.trim()
                ));
            }
            Ok(Some(profile))
        }
        None => Ok(default_for(all, harness)),
    }
}

/// The harness's own sign-in, as its argv after the program — what
/// `kete profile login` runs. None for a harness kete knows no sign-in for.
pub fn login_args(harness: &str) -> Option<&'static [&'static str]> {
    match harness.trim() {
        // The subscription flow, as agent-office signs accounts in: Claude
        // opens the browser and takes the callback, or prints the URL and
        // asks for the code when it cannot.
        "claude" => Some(&["auth", "login", "--claudeai"]),
        "codex" => Some(&["login"]),
        "copilot" => Some(&["login"]),
        _ => None,
    }
}

/// The harness's default profile: the first usable one flagged `default`.
/// A disabled default is no default: a launch naming no profile runs on
/// the CLI's own login, as when none is flagged.
pub fn default_for<'a>(all: &'a [Profile], harness: &str) -> Option<&'a Profile> {
    flagged_default(all, harness).filter(|p| !p.disabled)
}

/// The profile flagged default for `harness`, switched off or not: what a
/// list draws the star on (dimmed, "default (disabled)", when it is off
/// and so [`default_for`] passes it over).
pub fn flagged_default<'a>(all: &'a [Profile], harness: &str) -> Option<&'a Profile> {
    all.iter()
        .find(|p| p.default && p.harness.trim() == harness.trim() && p.problem().is_none())
}

/// Every usable, enabled profile for `harness`, in config order: what a
/// picker offers and a spread chooses from.
pub fn enabled_for<'a>(all: &'a [Profile], harness: &str) -> Vec<&'a Profile> {
    for_harness(all, harness)
        .into_iter()
        .filter(|p| !p.disabled)
        .collect()
}

/// Switch `name` on or off in the settings file, in place.
pub fn set_enabled_in_file(path: &Path, name: &str, enabled: bool) -> Result<(), String> {
    edit_file(path, |entries| {
        let Some(entry) = entries
            .iter_mut()
            .find(|e| e.get("name").and_then(Value::as_str) == Some(name.trim()))
            .and_then(Value::as_object_mut)
        else {
            return Err(format!("no profile named `{}`", name.trim()));
        };
        if enabled {
            entry.remove("disabled");
        } else {
            entry.insert("disabled".into(), json!(true));
        }
        Ok(())
    })
}

/// Every usable profile for `harness`, in config order.
pub fn for_harness<'a>(all: &'a [Profile], harness: &str) -> Vec<&'a Profile> {
    all.iter()
        .filter(|p| p.harness.trim() == harness.trim() && p.problem().is_none())
        .collect()
}

/// The `profiles` list of a raw settings object, entry by entry: an entry
/// this build cannot read is skipped, not fatal.
pub fn from_settings(obj: &serde_json::Map<String, Value>) -> Vec<Profile> {
    obj.get(CONFIG_KEY)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| serde_json::from_value(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The profiles `config.json` (with `config.local.json` over it) defines.
pub fn load() -> Vec<Profile> {
    let loaded = crate::settings::load::<ProfilesOnly>(
        &crate::paths::config_path(),
        &crate::paths::config_local_path(),
    );
    loaded.value.profiles
}

/// The one key [`load`] reads.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProfilesOnly {
    profiles: Vec<Profile>,
}

/// Add `profile` to the settings file at `path`, keeping every other key
/// and every entry as written. Refuses a duplicate name. A `default` entry
/// takes the flag off the harness's other entries.
pub fn add_to_file(path: &Path, profile: &Profile) -> Result<(), String> {
    if let Some(problem) = profile.problem() {
        return Err(problem);
    }
    edit_file(path, |entries| {
        if entries
            .iter()
            .any(|e| e.get("name").and_then(Value::as_str) == Some(profile.name.trim()))
        {
            return Err(format!("profile `{}` already exists", profile.name.trim()));
        }
        if profile.default {
            clear_default(entries, &profile.harness);
        }
        entries.push(serde_json::to_value(profile).map_err(|e| e.to_string())?);
        Ok(())
    })
}

/// Rewrite the entry named `profile.name` in the settings file at `path`,
/// in place: its position and any field this build has no name for stay,
/// the known fields take `profile`'s values. The name itself is what is
/// looked up (it names the PROFILE HOME), so it never changes here. A
/// `default` entry takes the flag off the harness's other entries.
pub fn update_in_file(path: &Path, profile: &Profile) -> Result<(), String> {
    if let Some(problem) = profile.problem() {
        return Err(problem);
    }
    let name = profile.name.trim();
    edit_file(path, |entries| {
        let Some(at) = entries
            .iter()
            .position(|e| e.get("name").and_then(Value::as_str) == Some(name))
        else {
            return Err(format!("no profile named `{name}`"));
        };
        if profile.default {
            clear_default(entries, &profile.harness);
        }
        let Value::Object(known) = serde_json::to_value(profile).map_err(|e| e.to_string())? else {
            return Err("a profile serializes to an object".into());
        };
        let mut entry = entries[at].as_object().cloned().unwrap_or_default();
        // Fields left out when empty or off must go, not linger.
        for key in [
            "name",
            "harness",
            "env",
            "default",
            "use_default",
            "disabled",
        ] {
            entry.remove(key);
        }
        entry.extend(known);
        entries[at] = Value::Object(entry);
        Ok(())
    })
}

/// Remove the profile named `name` from the settings file at `path`;
/// `Ok(false)` when there was none.
pub fn remove_from_file(path: &Path, name: &str) -> Result<bool, String> {
    let mut removed = false;
    edit_file(path, |entries| {
        let before = entries.len();
        entries.retain(|e| e.get("name").and_then(Value::as_str) != Some(name.trim()));
        removed = entries.len() != before;
        Ok(())
    })?;
    Ok(removed)
}

/// Make `name` its harness's default profile in the settings file.
pub fn set_default_in_file(path: &Path, name: &str) -> Result<(), String> {
    edit_file(path, |entries| {
        let Some(harness) = entries
            .iter()
            .find(|e| e.get("name").and_then(Value::as_str) == Some(name.trim()))
            .and_then(|e| e.get("harness").and_then(Value::as_str))
            .map(str::to_string)
        else {
            return Err(format!("no profile named `{}`", name.trim()));
        };
        clear_default(entries, &harness);
        for entry in entries.iter_mut() {
            if entry.get("name").and_then(Value::as_str) == Some(name.trim()) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("default".into(), json!(true));
                }
            }
        }
        Ok(())
    })
}

fn clear_default(entries: &mut [Value], harness: &str) {
    for entry in entries.iter_mut() {
        if entry.get("harness").and_then(Value::as_str).map(str::trim) == Some(harness.trim()) {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("default");
            }
        }
    }
}

/// Read the settings object at `path` (missing = empty), let `edit` change
/// its `profiles` list, and write it back atomically, under the settings
/// lock the TUI's and the window's saves take. A file that is there but
/// can't be parsed is an error, never replaced.
fn edit_file(
    path: &Path,
    edit: impl FnOnce(&mut Vec<Value>) -> Result<(), String>,
) -> Result<(), String> {
    let _lock = crate::settings::lock(path).map_err(|e| e.to_string())?;
    let mut obj = crate::settings::read_object(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_default();
    let mut entries = match obj.remove(CONFIG_KEY) {
        None => Vec::new(),
        Some(Value::Array(entries)) => entries,
        Some(_) => return Err(format!("{}: `profiles` is not a list", path.display())),
    };
    edit(&mut entries)?;
    obj.insert(CONFIG_KEY.into(), Value::Array(entries));
    crate::settings::write_json(path, &Value::Object(obj))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// The `config.json` key holding PROJECT SETTINGS, one entry per repo path
/// as the daemon stores it (`nebula-config`'s `ProjectSettings`).
pub const PROJECTS_KEY: &str = "projects";

/// The key, inside a `projects` entry, holding its PROJECT DEFAULT
/// ACCOUNTS: harness id to profile name. A launch in that project naming
/// no profile runs under its harness's entry, ahead of the ACCOUNT SPREAD
/// and the harness's default (`accounts::resolve_launch`). One entry per
/// harness, so a Codex default never displaces the Claude one.
pub const PROJECT_DEFAULTS_KEY: &str = "default_profiles";

/// Every project's default accounts in a raw settings object, by repo path
/// as written: an entry this build cannot read is skipped.
pub fn project_defaults_from(
    obj: &serde_json::Map<String, Value>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let Some(projects) = obj.get(PROJECTS_KEY).and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    projects
        .iter()
        .filter_map(|(repo, entry)| {
            let map: BTreeMap<String, String> = entry
                .get(PROJECT_DEFAULTS_KEY)
                .and_then(Value::as_object)?
                .iter()
                .filter_map(|(h, n)| Some((h.trim().to_string(), n.as_str()?.trim().to_string())))
                .filter(|(h, n)| !h.is_empty() && !n.is_empty())
                .collect();
            (!map.is_empty()).then(|| (repo.clone(), map))
        })
        .collect()
}

/// [`project_defaults_from`] of `config.json` with `config.local.json` over
/// it (the `projects` key of the local layer replaces the base's whole).
pub fn load_project_defaults() -> BTreeMap<String, BTreeMap<String, String>> {
    let read = |p: &Path| {
        crate::settings::read_object(p)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let mut merged = read(&crate::paths::config_path());
    merged.extend(read(&crate::paths::config_local_path()));
    project_defaults_from(&merged)
}

/// The profile `profile` would be the project default of: it must exist,
/// be usable, and is for `harness` then. `Err` says why not.
pub fn project_default_harness(all: &[Profile], name: &str) -> Result<String, String> {
    let Some(profile) = all.iter().find(|p| p.name.trim() == name.trim()) else {
        return Err(format!("no profile named `{}`", name.trim()));
    };
    if let Some(problem) = profile.problem() {
        return Err(problem);
    }
    Ok(profile.harness.trim().to_string())
}

/// Set (or with `name` None, clear) the default account of `harness` for
/// the project at `repo` in the settings file at `path`, in place: every
/// other key of the entry and of the file stays. An entry left saying
/// nothing goes, as `Config::set_project` drops an all-default one.
pub fn set_project_default_in_file(
    path: &Path,
    repo: &str,
    harness: &str,
    name: Option<&str>,
) -> Result<(), String> {
    let harness = harness.trim();
    if harness.is_empty() {
        return Err("no harness named".into());
    }
    if repo.trim().is_empty() {
        return Err("no project named".into());
    }
    let _lock = crate::settings::lock(path).map_err(|e| e.to_string())?;
    let mut obj = crate::settings::read_object(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_default();
    let mut projects = match obj.remove(PROJECTS_KEY) {
        None => serde_json::Map::new(),
        Some(Value::Object(projects)) => projects,
        Some(_) => return Err(format!("{}: `projects` is not an object", path.display())),
    };
    let mut entry = match projects.remove(repo) {
        None => serde_json::Map::new(),
        Some(Value::Object(entry)) => entry,
        Some(_) => {
            return Err(format!(
                "{}: project `{repo}` is not an object",
                path.display()
            ))
        }
    };
    let mut defaults = match entry.remove(PROJECT_DEFAULTS_KEY) {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => {
            defaults.insert(harness.to_string(), json!(name));
        }
        None => {
            defaults.remove(harness);
        }
    }
    if !defaults.is_empty() {
        entry.insert(PROJECT_DEFAULTS_KEY.into(), Value::Object(defaults));
    }
    if !entry.is_empty() {
        projects.insert(repo.to_string(), Value::Object(entry));
    }
    if !projects.is_empty() {
        obj.insert(PROJECTS_KEY.into(), Value::Object(projects));
    }
    crate::settings::write_json(path, &Value::Object(obj))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// The settings layer an edit of `key` goes to: the one whose `key` is in
/// force. Layers merge key by key, so a `config.local.json` holding `key`
/// replaces `config.json`'s whole, and an edit there is the only one that
/// shows. A local layer that is there but unreadable is refused: there is
/// no telling which value is in force.
pub fn layer_for(config: &Path, local: &Path, key: &str) -> Result<PathBuf, String> {
    let held = crate::settings::read_object(local)
        .map_err(|e| format!("{}: {e}: fix it, then try again", local.display()))?;
    Ok(if held.is_some_and(|obj| obj.contains_key(key)) {
        local.to_path_buf()
    } else {
        config.to_path_buf()
    })
}

/// Add `name` to the settings file at `path` as a copy of the profile
/// `from`: its harness, its variables and its kind of login, but neither
/// its default star nor its switch. The copy's home (made by the caller,
/// [`Profile::prepare_in`]) starts empty: it signs in separately. What
/// `kete profile duplicate` and both apps' Duplicate run.
pub fn duplicate_in_file(path: &Path, from: &str, name: &str) -> Result<Profile, String> {
    let held = crate::settings::read_object(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_default();
    let Some(source) = from_settings(&held)
        .into_iter()
        .find(|p| p.name.trim() == from.trim())
    else {
        return Err(format!("no profile named `{}`", from.trim()));
    };
    let copy = Profile {
        name: name.trim().to_string(),
        harness: source.harness.trim().to_string(),
        env: source.env,
        default: false,
        use_default: source.use_default,
        disabled: false,
        reserved: false,
    };
    add_to_file(path, &copy)?;
    Ok(copy)
}

/// What [`remove_in_file`] did with the profile's home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The PROFILE HOME, when there is one on disk (kept), or was
    /// (purged). None for an adopted login, or a home never made.
    pub home: Option<PathBuf>,
    /// The home was deleted.
    pub purged: bool,
}

/// Remove the profile `name` from the settings file at `path`, and with
/// `purge` delete its PROFILE HOME under `root` (the login in it) too.
/// Without, the home stays, so adding the name back signs nothing in
/// again. An adopted login's config is the CLI's own and never touched.
/// What `kete profile remove [--purge]` and both apps' Remove run.
pub fn remove_in_file(
    path: &Path,
    root: &Path,
    name: &str,
    purge: bool,
) -> Result<Removed, String> {
    let held = crate::settings::read_object(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_default();
    let home = from_settings(&held)
        .into_iter()
        .find(|p| p.name.trim() == name.trim())
        .and_then(|p| p.home_in(root))
        .filter(|h| h.exists());
    if !remove_from_file(path, name)? {
        return Err(format!("no profile named `{}`", name.trim()));
    }
    let mut purged = false;
    if let (true, Some(home)) = (purge, &home) {
        purge_home_in(root, home)
            .map_err(|e| format!("removed `{}`, but its home stays: {e}", name.trim()))?;
        purged = true;
    }
    Ok(Removed { home, purged })
}

/// Delete a PROFILE HOME: only ever a directory directly inside `root`,
/// checked on the resolved paths, so a profile name can never reach
/// outside it.
pub fn purge_home_in(root: &Path, home: &Path) -> Result<(), String> {
    if !home.exists() {
        return Ok(());
    }
    let root =
        crate::paths::canonical(root).map_err(|e| format!("resolve {}: {e}", root.display()))?;
    let target =
        crate::paths::canonical(home).map_err(|e| format!("resolve {}: {e}", home.display()))?;
    if target.parent() != Some(root.as_path()) {
        return Err(format!(
            "refusing to delete {}: not a profile home under {}",
            target.display(),
            root.display()
        ));
    }
    std::fs::remove_dir_all(&target).map_err(|e| format!("delete {}: {e}", target.display()))
}

/// Why the profile `name` cannot be renamed while these sessions exist,
/// or None. Every unarchived session naming it blocks: a live one runs
/// in its home, and a stopped one would fail to resume ("profile is no
/// longer defined"), since only the daemon can change the name a session
/// stores. Moving them to another account (or archiving them) first
/// clears the way.
pub fn rename_blocker(agents: &[crate::entities::Agent], name: &str) -> Option<String> {
    let name = name.trim();
    let on: Vec<&crate::entities::Agent> = agents
        .iter()
        .filter(|a| !a.archived && a.profile.as_deref().map(str::trim) == Some(name))
        .collect();
    if on.is_empty() {
        return None;
    }
    let names = |live: bool| -> Vec<&str> {
        on.iter()
            .filter(|a| a.alive == live)
            .map(|a| a.name.as_str())
            .collect()
    };
    let (live, stopped) = (names(true), names(false));
    let mut parts = Vec::new();
    if !live.is_empty() {
        parts.push(format!("{} running ({})", live.len(), live.join(", ")));
    }
    if !stopped.is_empty() {
        parts.push(format!(
            "{} stopped that would resume on it ({})",
            stopped.len(),
            stopped.join(", ")
        ));
    }
    Some(format!(
        "`{name}` has sessions: {}. Move them to another account or archive them, then rename",
        parts.join("; ")
    ))
}

/// What [`rename`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// The profile under its new name.
    pub profile: Profile,
    /// The PROFILE HOME moved, from and to.
    pub home: Option<(PathBuf, PathBuf)>,
    /// The repos (as written) whose default account it was, now under the
    /// new name.
    pub projects: Vec<String>,
    /// Archived sessions that still name the old one: restoring one means
    /// moving it to an account.
    pub archived: Vec<String>,
}

/// Rename the profile `old` to `new`, everywhere kete keys it by name: the
/// `profiles` entry (in the layer in force, its position and every other
/// field kept), its PROFILE HOME under `root`, each project's default
/// account naming it (in the layer whose `projects` key is in force), and
/// its kept usage reading. Refused while a session names it
/// ([`rename_blocker`]), for a name already taken, or when a directory of
/// the new name already sits in `root` (a home a remove left behind). The
/// home moves first and is moved back when the settings cannot be
/// written. Takes the settings locks itself: the caller holds none.
///
/// What `kete profile rename` and both apps' Rename run against a
/// daemon older than [`crate::RENAME_PROFILE_PROTOCOL`]; a newer one
/// takes [`rename_following`].
pub fn rename(
    config: &Path,
    local: &Path,
    root: &Path,
    old: &str,
    new: &str,
    agents: &[crate::entities::Agent],
) -> Result<Renamed, String> {
    rename_in(config, local, root, old, new, Some(agents))
}

/// [`rename`] where the daemon rewrites the sessions' stored name
/// (`ClientRequest::RenameProfileSessions`, protocol 60): no session
/// refuses it, and none is left naming the old one, archived ones
/// included. The caller sends that request once this returns Ok, so a
/// refused rename never leaves sessions naming a profile that is not
/// there. A live session keeps running under the home it started in
/// until it restarts; where the system refuses to move a home in use
/// (Windows), the rename fails with nothing changed.
pub fn rename_following(
    config: &Path,
    local: &Path,
    root: &Path,
    old: &str,
    new: &str,
) -> Result<Renamed, String> {
    rename_in(config, local, root, old, new, None)
}

/// `agents`: the sessions to refuse over and to report archived ones
/// from; None when the daemon rewrites them all.
fn rename_in(
    config: &Path,
    local: &Path,
    root: &Path,
    old: &str,
    new: &str,
    agents: Option<&[crate::entities::Agent]>,
) -> Result<Renamed, String> {
    let (old, new) = (old.trim(), new.trim());
    if old == new {
        return Err(format!("`{old}` already has that name"));
    }
    if let Some(why) = agents.and_then(|agents| rename_blocker(agents, old)) {
        return Err(why);
    }
    // config.json's lock covers both layers, as every settings writer
    // takes it; the local layer's own is taken too when it is written.
    let _base = crate::settings::lock(config).map_err(|e| e.to_string())?;
    let profiles_at = layer_for(config, local, CONFIG_KEY)?;
    let projects_at = layer_for(config, local, PROJECTS_KEY)?;
    let _local = if profiles_at == local || projects_at == local {
        Some(crate::settings::lock(local).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let read = |path: &Path| {
        crate::settings::read_object(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .map(Option::unwrap_or_default)
    };
    let mut profiles_obj = read(&profiles_at)?;
    let all = from_settings(&profiles_obj);
    let Some(current) = all.iter().find(|p| p.name.trim() == old) else {
        return Err(format!("no profile named `{old}`"));
    };
    if all.iter().any(|p| p.name.trim() == new) {
        return Err(format!("profile `{new}` already exists"));
    }
    let mut renamed = current.clone();
    renamed.name = new.to_string();
    if let Some(problem) = renamed.problem() {
        return Err(problem);
    }

    // The settings, edited in memory: the entry's name, then (in the same
    // object when one layer holds both) each project default naming it.
    if let Some(Value::Array(entries)) = profiles_obj.get_mut(CONFIG_KEY) {
        for entry in entries.iter_mut() {
            if entry.get("name").and_then(Value::as_str).map(str::trim) == Some(old) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("name".into(), json!(new));
                }
            }
        }
    }
    let same_layer = projects_at == profiles_at;
    let mut projects_obj = if same_layer {
        None
    } else {
        Some(read(&projects_at)?)
    };
    let projects = rename_project_defaults(
        projects_obj.as_mut().unwrap_or(&mut profiles_obj),
        &current.harness,
        old,
        new,
    );

    // A directory of the new name holds someone's login (a remove that kept
    // its home): never taken over, whether or not there is a home to move.
    if let Some(to) = renamed.home_in(root).filter(|to| to.exists()) {
        return Err(format!(
            "{} already exists (a removed profile's home?): delete it or pick another name",
            to.display()
        ));
    }
    let home = match (current.home_in(root), renamed.home_in(root)) {
        (Some(from), Some(to)) if from.exists() => {
            std::fs::rename(&from, &to).map_err(|e| {
                format!(
                    "could not move {} to {}: {e} (is a program still using it?)",
                    from.display(),
                    to.display()
                )
            })?;
            Some((from, to))
        }
        _ => None,
    };
    let write = |path: &Path, obj: serde_json::Map<String, Value>| {
        crate::settings::write_json(path, &Value::Object(obj))
            .map_err(|e| format!("write {}: {e}", path.display()))
    };
    let undo_home = |e: String| {
        if let Some((from, to)) = &home {
            let _ = std::fs::rename(to, from);
        }
        e
    };
    let profiles_before = read(&profiles_at).map_err(undo_home)?;
    write(&profiles_at, profiles_obj).map_err(undo_home)?;
    if let Some(obj) = projects_obj {
        if let Err(e) = write(&projects_at, obj) {
            let _ = write(&profiles_at, profiles_before);
            return Err(undo_home(e));
        }
    }
    // The reading follows the name; a failure here only loses the bars
    // until the next session's turn.
    // A stale reading under the new name is replaced, not kept beside it
    // (Windows refuses a rename onto an existing file).
    if usage_path_in(root, old).exists() {
        let _ = std::fs::remove_file(usage_path_in(root, new));
        let _ = std::fs::rename(usage_path_in(root, old), usage_path_in(root, new));
    }
    let archived = agents
        .unwrap_or_default()
        .iter()
        .filter(|a| a.archived && a.profile.as_deref().map(str::trim) == Some(old))
        .map(|a| a.name.clone())
        .collect();
    Ok(Renamed {
        profile: renamed,
        home,
        projects,
        archived,
    })
}

/// Point every project default of `harness` naming `old` at `new`, in a
/// raw settings object; the repos changed.
fn rename_project_defaults(
    obj: &mut serde_json::Map<String, Value>,
    harness: &str,
    old: &str,
    new: &str,
) -> Vec<String> {
    let mut changed = Vec::new();
    let Some(Value::Object(projects)) = obj.get_mut(PROJECTS_KEY) else {
        return changed;
    };
    for (repo, entry) in projects.iter_mut() {
        let Some(Value::Object(defaults)) = entry.get_mut(PROJECT_DEFAULTS_KEY) else {
            continue;
        };
        if let Some(value) = defaults.get_mut(harness.trim()) {
            if value.as_str().map(str::trim) == Some(old) {
                *value = json!(new);
                changed.push(repo.clone());
            }
        }
    }
    changed
}

/// The latest PLAN USAGE a profile's sessions printed — Claude's
/// `You've used 91% of your weekly limit · resets 2pm` — read off a
/// session's screen by the daemon (`pty::usage`) and kept per profile, so
/// the QUICK PROMPT's profile field and `kete profile list` can say how
/// much of each account is left before you pick one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Percent of the limit used.
    pub percent: u8,
    /// Which limit: `weekly`, `5-hour`, `Opus weekly` — as printed.
    pub window: String,
    /// When it resets, as printed (`2pm`, `Oct 14 at 9am`); None when the
    /// line said nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets: Option<String>,
    /// Epoch ms it was read.
    #[serde(default)]
    pub at: i64,
}

impl Usage {
    /// `91% weekly · resets 2pm`.
    pub fn summary(&self) -> String {
        let mut out = format!("{}% {}", self.percent, self.window);
        if let Some(resets) = &self.resets {
            out.push_str(" · resets ");
            out.push_str(resets);
        }
        out
    }

    /// The usage a line of a session's screen reports, when it is Claude's
    /// usage notice: `…used <n>% of your <window> limit[ · resets <when>]`.
    /// The screen text arrives with its escape sequences already taken
    /// out; anything else reads as None.
    pub fn parse(line: &str) -> Option<Usage> {
        let at = line.find("used ")?;
        let rest = &line[at + "used ".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || digits.len() > 3 {
            return None;
        }
        let percent: u16 = digits.parse().ok()?;
        let rest = rest[digits.len()..].strip_prefix("% of your ")?;
        let end = rest.find(" limit")?;
        let window = rest[..end].trim();
        if window.is_empty() || window.len() > 32 {
            return None;
        }
        let tail = &rest[end + " limit".len()..];
        let resets = tail
            .find("resets ")
            .map(|i| &tail[i + "resets ".len()..])
            .map(|r| {
                r.split(['\u{b7}', '(', ')', '|', '\r', '\n'])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches('.')
                    .to_string()
            })
            .filter(|r| !r.is_empty() && r.len() <= 40);
        Some(Usage {
            percent: percent.min(100) as u8,
            window: window.to_string(),
            resets,
            at: 0,
        })
    }
}

/// Where a profile's latest [`Usage`] is kept: `<profiles>/.usage/<name>.json`
/// — beside the homes rather than in one, so an adopted login has a place
/// too.
pub fn usage_path_in(root: &Path, name: &str) -> PathBuf {
    root.join(".usage").join(format!("{}.json", name.trim()))
}

/// The latest usage `profile`'s sessions printed, if any has.
pub fn read_usage(profile: &Profile) -> Option<Usage> {
    read_usage_in(&profiles_root(), &profile.name)
}

/// [`read_usage`] under `root`.
pub fn read_usage_in(root: &Path, name: &str) -> Option<Usage> {
    let text = std::fs::read_to_string(usage_path_in(root, name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Record `usage` as profile `name`'s latest.
pub fn write_usage_in(root: &Path, name: &str, usage: &Usage) -> std::io::Result<()> {
    let value = serde_json::to_value(usage)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::settings::write_json(&usage_path_in(root, name), &value)
}

/// The default Claude config's `.claude.json`: in `$CLAUDE_CONFIG_DIR`
/// when the environment moves it, else in the home directory (Claude keeps
/// it beside `~/.claude`, not in it).
fn default_claude_json() -> Option<PathBuf> {
    match crate::env::non_empty(crate::env::CLAUDE_CONFIG_DIR) {
        Some(dir) => Some(PathBuf::from(dir).join(".claude.json")),
        None => crate::env::home_dir().map(|home| home.join(".claude.json")),
    }
}

/// The key Claude files a folder under in `.claude.json`'s `projects`:
/// the path with `/` separators (`C:/Code/app` on Windows).
fn claude_project_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.replace('\\', "/")
}

/// Do two `.claude.json` project keys name the same folder? Windows keys
/// differ in drive-letter (and other) case for one folder.
fn same_project_key(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Seed `<config_dir>/.claude.json`: `hasCompletedOnboarding`, and folder
/// trust for `worktree` and each folder above it that `default_json`
/// trusts. Leaves the file untouched when nothing would change, and
/// refuses to touch one that is not a JSON object.
pub fn seed_claude_config(
    config_dir: &Path,
    default_json: Option<&Path>,
    worktree: Option<&Path>,
) -> std::io::Result<()> {
    let file = config_dir.join(".claude.json");
    let mut root = match std::fs::read_to_string(&file) {
        Ok(text) if text.trim().is_empty() => json!({}),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => v,
            // Claude's own file, unreadable to us: never clobber it.
            _ => return Ok(()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e),
    };
    let before = root.clone();
    let obj = root.as_object_mut().expect("checked an object above");
    obj.insert("hasCompletedOnboarding".into(), json!(true));

    let trusted: Vec<String> = match (worktree, default_json) {
        (Some(worktree), Some(default_json)) => {
            let default = std::fs::read_to_string(default_json)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok());
            let projects = default
                .as_ref()
                .and_then(|d| d.get("projects"))
                .and_then(Value::as_object);
            worktree
                .ancestors()
                .map(claude_project_key)
                .filter(|key| {
                    projects.is_some_and(|projects| {
                        projects.iter().any(|(k, v)| {
                            same_project_key(k, key)
                                && v.get("hasTrustDialogAccepted").and_then(Value::as_bool)
                                    == Some(true)
                        })
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    };
    if !trusted.is_empty() {
        let projects = obj
            .entry("projects")
            .or_insert_with(|| json!({}))
            .as_object_mut();
        if let Some(projects) = projects {
            for key in trusted {
                let entry = projects.entry(key).or_insert_with(|| json!({}));
                if let Some(entry) = entry.as_object_mut() {
                    entry.insert("hasTrustDialogAccepted".into(), json!(true));
                }
            }
        }
    }
    if root == before {
        return Ok(());
    }
    std::fs::create_dir_all(config_dir)?;
    crate::settings::write_json(&file, &root)
}

/// The `.claude.json` Claude keeps an account's state in: inside a
/// profile's config dir; for the CLI's own login (or an adopted one) in
/// `$CLAUDE_CONFIG_DIR` when that is set, else beside `~/.claude`.
pub fn claude_json_for(profile: Option<&Profile>, root: &Path) -> Option<PathBuf> {
    let var = crate::env::CLAUDE_CONFIG_DIR;
    if let Some(dir) = profile.and_then(|p| p.dir_for(root, var)) {
        return Some(dir.join(".claude.json"));
    }
    let set = profile
        .and_then(|p| p.env.get(var).cloned())
        .filter(|d| !d.trim().is_empty())
        .or_else(|| crate::env::non_empty(var));
    match set {
        Some(dir) => Some(PathBuf::from(dir).join(".claude.json")),
        None => crate::env::home_dir().map(|home| home.join(".claude.json")),
    }
}

/// Mark `folder` trusted in the `.claude.json` at `file`, as answering
/// Claude's trust dialog there once would: that folder's entry alone,
/// written only when it isn't trusted yet, and never into a file that is
/// missing (no login lives there) or isn't a JSON object. Kete's own
/// empty wake-up folder needs it (`wake::wake_dir`): a hidden session has
/// no one to answer the dialog. Whether it wrote.
pub fn trust_folder(file: &Path, folder: &Path) -> std::io::Result<bool> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let Ok(mut root) = serde_json::from_str::<Value>(crate::settings::strip_bom(&text)) else {
        return Ok(false);
    };
    let Some(obj) = root.as_object_mut() else {
        return Ok(false);
    };
    let key = claude_project_key(folder);
    let projects = obj.entry("projects").or_insert_with(|| json!({}));
    let Some(projects) = projects.as_object_mut() else {
        return Ok(false);
    };
    let existing = projects
        .keys()
        .find(|k| same_project_key(k, &key))
        .cloned()
        .unwrap_or(key);
    let entry = projects.entry(existing).or_insert_with(|| json!({}));
    let Some(entry) = entry.as_object_mut() else {
        return Ok(false);
    };
    if entry.get("hasTrustDialogAccepted").and_then(Value::as_bool) == Some(true) {
        return Ok(false);
    }
    entry.insert("hasTrustDialogAccepted".into(), json!(true));
    crate::settings::write_json(file, &root)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, harness: &str) -> Profile {
        Profile {
            name: name.into(),
            harness: harness.into(),
            env: BTreeMap::new(),
            default: false,
            use_default: false,
            disabled: false,
            reserved: false,
        }
    }

    /// 0.60's `reserved` round-trips, reads as false from a 0.59 entry,
    /// and is left off the file when false, so 0.59 reads the entry as
    /// it wrote it.
    #[test]
    fn reserved_round_trips_and_is_absent_when_false() {
        let old: Profile =
            serde_json::from_str(r#"{"name": "jayrich", "harness": "claude", "default": true}"#)
                .unwrap();
        assert!(!old.reserved);
        assert!(!serde_json::to_string(&old).unwrap().contains("reserved"));
        let mine = Profile {
            reserved: true,
            ..old
        };
        let text = serde_json::to_string(&mine).unwrap();
        assert!(text.contains("\"reserved\":true"), "{text}");
        let back: Profile = serde_json::from_str(&text).unwrap();
        assert_eq!(back, mine);
    }

    #[test]
    fn an_update_rewrites_the_entry_in_place_and_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme": "ocean", "profiles": [
                {"name": "work", "harness": "claude", "default": true, "env": {"A": "1"}, "colour": "red"},
                {"name": "home", "harness": "claude"}
            ]}"#,
        )
        .unwrap();
        let mut home = profile("home", "claude");
        home.default = true;
        update_in_file(&path, &home).unwrap();
        let mut work = profile("work", "claude");
        work.use_default = true;
        update_in_file(&path, &work).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["theme"], "ocean");
        let list = saved["profiles"].as_array().unwrap();
        assert_eq!(list[0]["name"], "work", "position kept");
        assert_eq!(list[0]["colour"], "red", "unknown field kept");
        assert_eq!(list[0]["use_default"], true);
        assert!(list[0].get("env").is_none(), "an emptied env goes");
        assert!(
            list[0].get("default").is_none(),
            "the default moved to home"
        );
        assert_eq!(list[1]["default"], true);
        assert!(update_in_file(&path, &profile("nope", "claude")).is_err());
        assert!(update_in_file(&path, &profile("bad name", "claude")).is_err());
    }

    #[test]
    fn the_login_command_covers_adopted_logins_too() {
        use crate::accounts::login_command;
        assert_eq!(
            login_command(Some(&profile("work", "claude")), "claude").as_deref(),
            Some("kete profile login work")
        );
        // An adopted login signs in under its profile's environment too.
        let mut adopted = profile("main", "codex");
        adopted.use_default = true;
        assert_eq!(
            login_command(Some(&adopted), "codex").as_deref(),
            Some("kete profile login main")
        );
        assert_eq!(login_command(Some(&profile("x", "cursor")), "cursor"), None);
        assert_eq!(login_args("copilot"), Some(&["login"][..]));
    }

    fn agent(name: &str, profile: Option<&str>, alive: bool, archived: bool) -> crate::Agent {
        crate::Agent {
            id: crate::AgentId(name.into()),
            worktree_id: crate::WorktreeId("w".into()),
            name: name.into(),
            status: crate::AgentStatus::Finished,
            archived,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind: crate::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role: crate::entities::AgentRole::Worker,
            sort_order: 0,
            alive,
            recent_prompts: Vec::new(),
            profile: profile.map(str::to_string),
            parent: None,
        }
    }

    /// config.json with `work` (claude, a home, a project default and a
    /// kept reading) and `main` (adopted), in a temp dir.
    fn rename_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let root = dir.path().join("profiles");
        std::fs::write(
            &config,
            r#"{"theme": "ocean",
                "profiles": [
                  {"name": "main", "harness": "claude", "use_default": true},
                  {"name": "work", "harness": "claude", "default": true, "colour": "red"}
                ],
                "projects": {
                  "C:/app": {"run_command": "x", "default_profiles": {"claude": "work", "codex": "work"}},
                  "C:/other": {"default_profiles": {"claude": "main"}}
                }}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("work").join("claude")).unwrap();
        std::fs::write(
            root.join("work").join("claude").join(".credentials.json"),
            "{}",
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".usage")).unwrap();
        std::fs::write(usage_path_in(&root, "work"), "{}").unwrap();
        (dir, config, local, root)
    }

    /// Against a daemon that rewrites the sessions' stored name, a session
    /// on the profile no longer refuses the rename, and none is reported
    /// left behind; the old path still refuses for any of them.
    #[test]
    fn a_rename_the_daemon_follows_is_not_refused_by_sessions() {
        let (_d, config, local, root) = rename_fixture();
        let live = agent("live", Some("work"), true, false);
        assert!(rename(&config, &local, &root, "work", "max2", &[live]).is_err());
        let done = rename_following(&config, &local, &root, "work", "max2").unwrap();
        assert_eq!(done.profile.name, "max2");
        assert!(
            done.archived.is_empty(),
            "the daemon takes archived ones too"
        );
        assert!(root.join("max2").join("claude").is_dir());
    }

    #[test]
    fn a_rename_moves_the_home_the_entry_the_project_defaults_and_the_reading() {
        let (_d, config, local, root) = rename_fixture();
        let archived = agent("old-run", Some("work"), false, true);
        let done = rename(&config, &local, &root, "work", "max2", &[archived]).unwrap();
        assert_eq!(done.profile.name, "max2");
        assert_eq!(done.projects, vec!["C:/app".to_string()]);
        assert_eq!(done.archived, vec!["old-run".to_string()]);
        assert_eq!(
            done.home,
            Some((root.join("work"), root.join("max2"))),
            "the home moved"
        );
        assert!(root
            .join("max2")
            .join("claude")
            .join(".credentials.json")
            .is_file());
        assert!(!root.join("work").exists());
        assert!(usage_path_in(&root, "max2").is_file());
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(raw["theme"], "ocean");
        assert_eq!(raw["profiles"][1]["name"], "max2", "position kept");
        assert_eq!(raw["profiles"][1]["colour"], "red", "unknown field kept");
        assert_eq!(raw["profiles"][1]["default"], true);
        let app = &raw["projects"]["C:/app"];
        assert_eq!(app["default_profiles"]["claude"], "max2");
        // A default of another harness that happens to share the name is
        // not this profile's.
        assert_eq!(app["default_profiles"]["codex"], "work");
        assert_eq!(app["run_command"], "x");
        assert_eq!(
            raw["projects"]["C:/other"]["default_profiles"]["claude"],
            "main"
        );
        assert!(!config.with_file_name("config.json.lock").exists());

        // An adopted login has no home to move.
        let done = rename(&config, &local, &root, "main", "mine", &[]).unwrap();
        assert_eq!(done.home, None);
        assert_eq!(done.projects, vec!["C:/other".to_string()]);
    }

    #[test]
    fn a_rename_is_refused_before_anything_moves() {
        let (_d, config, local, root) = rename_fixture();
        let before = std::fs::read_to_string(&config).unwrap();
        let refused = |new: &str, agents: &[crate::Agent]| {
            rename(&config, &local, &root, "work", new, agents).unwrap_err()
        };
        let live = agent("fix-auth", Some("work"), true, false);
        let stopped = agent("docs", Some("work"), false, false);
        let why = refused("max2", &[live, stopped]);
        assert!(why.contains("1 running (fix-auth)"), "{why}");
        assert!(
            why.contains("1 stopped that would resume on it (docs)"),
            "{why}"
        );
        assert!(refused("main", &[]).contains("already exists"));
        assert!(refused("bad name", &[]).contains("letters"));
        assert!(refused("work", &[]).contains("already has"));
        assert!(rename(&config, &local, &root, "gone", "x", &[]).is_err());
        // A home a remove left behind under the new name is not taken over.
        std::fs::create_dir_all(root.join("left")).unwrap();
        assert!(refused("left", &[]).contains("already exists"));
        // ... nor by a profile with no home of its own on disk yet.
        std::fs::remove_dir_all(root.join("work")).unwrap();
        assert!(refused("left", &[]).contains("already exists"));
        std::fs::create_dir_all(root.join("work").join("claude")).unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
        assert!(root.join("work").join("claude").is_dir(), "nothing moved");
    }

    #[test]
    fn a_rename_edits_the_layers_in_force() {
        let (_d, config, local, root) = rename_fixture();
        std::fs::write(
            &local,
            r#"{"profiles": [{"name": "work", "harness": "claude"}]}"#,
        )
        .unwrap();
        rename(&config, &local, &root, "work", "max2", &[]).unwrap();
        let held: Value = serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        assert_eq!(held["profiles"][0]["name"], "max2");
        let base: Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(
            base["profiles"][1]["name"], "work",
            "the shadowed list stays"
        );
        assert_eq!(
            base["projects"]["C:/app"]["default_profiles"]["claude"],
            "max2"
        );
        assert!(!local.with_file_name("config.local.json.lock").exists());
    }

    #[test]
    fn a_duplicate_copies_all_but_the_star_and_the_switch() {
        let (_d, config, _local, _root) = rename_fixture();
        let raw = std::fs::read_to_string(&config).unwrap();
        std::fs::write(
            &config,
            raw.replace(
                r#""default": true,"#,
                r#""default": true, "disabled": true, "env": {"K": "v"},"#,
            ),
        )
        .unwrap();
        let copy = duplicate_in_file(&config, "work", "work-2").unwrap();
        assert_eq!(copy.harness, "claude");
        assert_eq!(copy.env.get("K").map(String::as_str), Some("v"));
        assert!(!copy.default && !copy.disabled && !copy.use_default);
        assert!(duplicate_in_file(&config, "work", "work-2")
            .unwrap_err()
            .contains("exists"));
        assert!(duplicate_in_file(&config, "gone", "x").is_err());
        assert!(
            duplicate_in_file(&config, "main", "main-2")
                .unwrap()
                .use_default
        );
    }

    #[test]
    fn a_remove_keeps_the_home_unless_purged() {
        let (_d, config, _local, root) = rename_fixture();
        let kept = remove_in_file(&config, &root, "work", false).unwrap();
        assert_eq!(
            (kept.home.clone(), kept.purged),
            (Some(root.join("work")), false)
        );
        assert!(root.join("work").is_dir());
        assert!(
            remove_in_file(&config, &root, "work", true).is_err(),
            "gone already"
        );

        let (_d, config, _local, root) = rename_fixture();
        let purged = remove_in_file(&config, &root, "work", true).unwrap();
        assert_eq!(
            (purged.home, purged.purged),
            (Some(root.join("work")), true)
        );
        assert!(!root.join("work").exists());
        assert!(root.join(".usage").is_dir(), "only the home goes");
        // An adopted login's config is the CLI's own: never deleted.
        let adopted = remove_in_file(&config, &root, "main", true).unwrap();
        assert_eq!((adopted.home, adopted.purged), (None, false));
    }

    #[test]
    fn a_purge_never_leaves_the_profiles_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("profiles");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(outside.join("deep")).unwrap();
        assert!(purge_home_in(&root, &outside)
            .unwrap_err()
            .contains("refusing"));
        assert!(purge_home_in(&root, &root.join("..").join("elsewhere")).is_err());
        assert!(outside.join("deep").is_dir());
        assert!(purge_home_in(&root, &root.join("never-made")).is_ok());
    }

    #[test]
    fn an_isolated_profile_strips_credentials_and_points_its_dirs_home() {
        let root = Path::new("/data/profiles");
        let mut work = profile("work", "claude");
        work.env.insert("HTTPS_PROXY".into(), "http://proxy".into());
        let env = work.launch_env_in(root);
        assert!(env.unset.contains(&"ANTHROPIC_API_KEY".to_string()));
        assert!(env.unset.contains(&"CLAUDE_CODE_OAUTH_TOKEN".to_string()));
        assert_eq!(
            env.set,
            vec![
                (
                    "CLAUDE_CONFIG_DIR".to_string(),
                    root.join("work")
                        .join("claude")
                        .to_string_lossy()
                        .into_owned()
                ),
                ("HTTPS_PROXY".to_string(), "http://proxy".to_string()),
            ]
        );

        let copilot = profile("gh2", "copilot").launch_env_in(root);
        let vars: Vec<&str> = copilot.set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(vars, vec!["COPILOT_HOME", "GH_CONFIG_DIR"]);
        assert!(copilot.unset.contains(&"GH_TOKEN".to_string()));
    }

    #[test]
    fn an_adopted_login_moves_no_dir_and_an_env_key_survives_its_strip() {
        let root = Path::new("/data/profiles");
        let mut main = profile("main", "codex");
        main.use_default = true;
        main.env.insert("OPENAI_API_KEY".into(), "sk-mine".into());
        assert_eq!(main.home_in(root), None);
        let env = main.launch_env_in(root);
        assert_eq!(
            env.set,
            vec![("OPENAI_API_KEY".to_string(), "sk-mine".to_string())]
        );
        assert!(
            !env.removals().contains(&"OPENAI_API_KEY".to_string()),
            "a variable the profile sets is not also removed"
        );
        assert!(env.removals().contains(&"CODEX_API_KEY".to_string()));
    }

    #[test]
    fn resolve_takes_the_named_profile_or_the_harness_default() {
        let mut a = profile("a", "claude");
        let mut b = profile("b", "claude");
        b.default = true;
        let c = profile("c", "codex");
        a.env.clear();
        let all = vec![a, b, c];
        assert_eq!(resolve(&all, "claude", None).unwrap().unwrap().name, "b");
        assert_eq!(
            resolve(&all, "claude", Some("a")).unwrap().unwrap().name,
            "a"
        );
        assert_eq!(resolve(&all, "codex", None).unwrap(), None, "no default");
        assert!(resolve(&all, "claude", Some("gone")).is_err());
        assert!(
            resolve(&all, "claude", Some("c")).is_err(),
            "a codex account on a claude launch"
        );
        assert_eq!(for_harness(&all, "claude").len(), 2);
    }

    #[test]
    fn claudes_usage_notice_parses_and_anything_else_does_not() {
        let usage =
            Usage::parse("  You've used 91% of your weekly limit \u{b7} resets 2pm").unwrap();
        assert_eq!(usage.percent, 91);
        assert_eq!(usage.window, "weekly");
        assert_eq!(usage.resets.as_deref(), Some("2pm"));
        assert_eq!(usage.summary(), "91% weekly \u{b7} resets 2pm");
        let usage = Usage::parse("You've used 100% of your Opus weekly limit").unwrap();
        assert_eq!((usage.percent, usage.window.as_str()), (100, "Opus weekly"));
        assert_eq!(usage.resets, None);
        assert_eq!(Usage::parse("I used 3 of your tools"), None);
        assert_eq!(Usage::parse("used % of your weekly limit"), None);

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_usage_in(dir.path(), "work"), None);
        write_usage_in(dir.path(), "work", &usage).unwrap();
        assert_eq!(read_usage_in(dir.path(), "work"), Some(usage));
    }

    #[test]
    fn a_disabled_profile_is_never_the_default_and_its_flag_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut a = profile("a", "claude");
        a.default = true;
        add_to_file(&path, &a).unwrap();
        add_to_file(&path, &profile("b", "claude")).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains("disabled"),
            "an enabled profile writes no flag"
        );
        set_enabled_in_file(&path, "a", false).unwrap();
        let obj = crate::settings::read_object(&path).unwrap().unwrap();
        let all = from_settings(&obj);
        assert!(all[0].disabled);
        assert_eq!(
            default_for(&all, "claude"),
            None,
            "a disabled default is none"
        );
        assert_eq!(
            resolve(&all, "claude", Some("a")).unwrap().unwrap().name,
            "a",
            "a session naming it still resolves (restart, resume)"
        );
        let names: Vec<&str> = enabled_for(&all, "claude")
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["b"]);
        // An edit that leaves it enabled takes the flag off.
        let mut back = all[0].clone();
        back.disabled = false;
        update_in_file(&path, &back).unwrap();
        let obj = crate::settings::read_object(&path).unwrap().unwrap();
        assert!(!from_settings(&obj)[0].disabled);
        assert!(set_enabled_in_file(&path, "nope", true).is_err());
    }

    #[test]
    fn a_project_default_is_set_per_harness_and_cleared_without_a_trace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme": "dark", "projects": {"/repo": {"run_command": "make"}}}"#,
        )
        .unwrap();
        set_project_default_in_file(&path, "/repo", "claude", Some("work")).unwrap();
        set_project_default_in_file(&path, "/repo", "codex", Some("gpt")).unwrap();
        set_project_default_in_file(&path, "/other", "claude", Some("home")).unwrap();
        let obj = crate::settings::read_object(&path).unwrap().unwrap();
        let all = project_defaults_from(&obj);
        assert_eq!(all["/repo"]["claude"], "work");
        assert_eq!(all["/repo"]["codex"], "gpt", "one entry per harness");
        assert_eq!(all["/other"]["claude"], "home");
        assert_eq!(obj["theme"], "dark");
        assert_eq!(obj["projects"]["/repo"]["run_command"], "make");

        set_project_default_in_file(&path, "/repo", "claude", None).unwrap();
        set_project_default_in_file(&path, "/repo", "codex", None).unwrap();
        set_project_default_in_file(&path, "/other", "claude", None).unwrap();
        let obj = crate::settings::read_object(&path).unwrap().unwrap();
        assert!(project_defaults_from(&obj).is_empty());
        assert_eq!(
            obj["projects"]["/repo"],
            json!({"run_command": "make"}),
            "the rest of the entry stays"
        );
        assert!(
            obj["projects"].get("/other").is_none(),
            "an emptied entry goes"
        );
        assert!(set_project_default_in_file(&path, "", "claude", Some("x")).is_err());
    }

    #[test]
    fn a_project_default_must_name_a_usable_profile() {
        let all = vec![profile("work", "claude"), profile("bad name", "claude")];
        assert_eq!(project_default_harness(&all, "work").unwrap(), "claude");
        assert!(project_default_harness(&all, "gone").is_err());
        assert!(project_default_harness(&all, "bad name").is_err());
    }

    #[test]
    fn the_layer_holding_a_key_takes_its_edits() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        assert_eq!(layer_for(&config, &local, PROJECTS_KEY).unwrap(), config);
        std::fs::write(&local, r#"{"projects": {}}"#).unwrap();
        assert_eq!(layer_for(&config, &local, PROJECTS_KEY).unwrap(), local);
        assert_eq!(layer_for(&config, &local, CONFIG_KEY).unwrap(), config);
        std::fs::write(&local, "{ broken").unwrap();
        assert!(layer_for(&config, &local, PROJECTS_KEY).is_err());
    }

    #[test]
    fn names_are_checked() {
        assert!(profile("", "claude").problem().is_some());
        assert!(profile("a b", "claude").problem().is_some());
        assert!(profile("../x", "claude").problem().is_some());
        assert!(profile("work_2", "").problem().is_some());
        assert_eq!(profile("work-2", "claude").problem(), None);
    }

    #[test]
    fn file_edits_keep_other_keys_and_one_default_per_harness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme":"dark","profiles":[{"name":"keep","harness":"claude","future":1}]}"#,
        )
        .unwrap();
        let mut a = profile("a", "claude");
        a.default = true;
        add_to_file(&path, &a).unwrap();
        assert!(add_to_file(&path, &a).is_err(), "duplicate name");
        add_to_file(&path, &profile("b", "claude")).unwrap();
        set_default_in_file(&path, "b").unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dark");
        assert_eq!(saved["profiles"][0]["future"], 1, "unknown fields kept");
        assert_eq!(saved["profiles"][1].get("default"), None);
        assert_eq!(saved["profiles"][2]["default"], true);
        assert!(remove_from_file(&path, "a").unwrap());
        assert!(!remove_from_file(&path, "a").unwrap());
        let obj = crate::settings::read_object(&path).unwrap().unwrap();
        let names: Vec<String> = from_settings(&obj).into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["keep", "b"]);
    }

    #[test]
    fn a_file_edit_reads_past_a_bom_and_never_replaces_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "\u{feff}{\"theme\": \"dark\"}").unwrap();
        add_to_file(&path, &profile("a", "claude")).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dark");

        let broken = "\u{feff}{\"theme\": \"dark\",}";
        std::fs::write(&path, broken).unwrap();
        assert!(add_to_file(&path, &profile("b", "claude")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        assert!(!dir.path().join("config.json.lock").exists());
    }

    #[test]
    fn seeding_marks_onboarding_and_carries_trust_over() {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("code").join("app");
        let default_json = dir.path().join("default.json");
        let trusted_key = claude_project_key(&dir.path().join("code"));
        std::fs::write(
            &default_json,
            json!({"projects": {
                trusted_key.clone(): {"hasTrustDialogAccepted": true},
                "/elsewhere": {"hasTrustDialogAccepted": true},
            }})
            .to_string(),
        )
        .unwrap();
        let config = dir.path().join("profile-claude");
        seed_claude_config(&config, Some(&default_json), Some(&worktree)).unwrap();
        let seeded: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join(".claude.json")).unwrap())
                .unwrap();
        assert_eq!(seeded["hasCompletedOnboarding"], true);
        assert_eq!(
            seeded["projects"][&trusted_key]["hasTrustDialogAccepted"], true,
            "the trusted folder above the worktree carries over"
        );
        assert!(seeded["projects"].get("/elsewhere").is_none());

        // Unchanged on a second seed; a file Claude wrote keeps its keys.
        std::fs::write(
            config.join(".claude.json"),
            json!({"hasCompletedOnboarding": true, "oauthAccount": {"x": 1}}).to_string(),
        )
        .unwrap();
        seed_claude_config(&config, None, Some(&worktree)).unwrap();
        let kept: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join(".claude.json")).unwrap())
                .unwrap();
        assert_eq!(kept["oauthAccount"]["x"], 1);
    }

    #[test]
    fn the_wake_folder_is_trusted_once_and_nothing_else_moves() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let folder = dir.path().join("data").join("wake");
        assert!(!trust_folder(&file, &folder).unwrap(), "no file, no login");
        assert!(!file.exists(), "never created");
        std::fs::write(
            &file,
            json!({"oauthAccount": {"x": 1}, "projects": {"C:/other": {"hasTrustDialogAccepted": false}}})
                .to_string(),
        )
        .unwrap();
        assert!(trust_folder(&file, &folder).unwrap());
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let key = claude_project_key(&folder);
        assert_eq!(saved["projects"][&key]["hasTrustDialogAccepted"], true);
        assert_eq!(
            saved["projects"]["C:/other"]["hasTrustDialogAccepted"],
            false
        );
        assert_eq!(saved["oauthAccount"]["x"], 1);
        assert!(!trust_folder(&file, &folder).unwrap(), "already trusted");
        std::fs::write(&file, "[1]").unwrap();
        assert!(
            !trust_folder(&file, &folder).unwrap(),
            "not an object: left alone"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[1]");
    }

    #[test]
    fn a_profile_keeps_its_claude_json_in_its_home() {
        let root = Path::new("/kete/profiles");
        let work = profile("work", "claude");
        assert_eq!(
            claude_json_for(Some(&work), root),
            Some(root.join("work").join("claude").join(".claude.json"))
        );
        let mut adopted = profile("main", "claude");
        adopted.use_default = true;
        adopted
            .env
            .insert(crate::env::CLAUDE_CONFIG_DIR.into(), "/elsewhere".into());
        assert_eq!(
            claude_json_for(Some(&adopted), root),
            Some(PathBuf::from("/elsewhere").join(".claude.json"))
        );
    }
}
