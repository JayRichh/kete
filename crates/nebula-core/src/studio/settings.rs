//! What this machine allows each studio repo: the `studio` key of
//! `config.local.json`, a map keyed by repo root.
//!
//! ```json
//! { "studio": { "C:/Code/my-game": { "studio_inbox": "Design/Studio/inbox",
//!   "autonomy": "A1", "queue_exclude": ["library-sweep-2"] } } }
//! ```
//!
//! An absent entry is studio off for that repo. The key is machine-local
//! like `wake`: never exported, never sent to a remote, and kept out of
//! the Settings rows (`NOT_ROWS`). The daemon writes it, through
//! `SetStudio`, which it refuses from inside an agent session; `kete
//! _guard` reads it from the session's own process, so both sides go
//! through [`repo_key`] and one spelling of a repo finds one entry.

use crate::studio::autonomy::Autonomy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The key in `config.local.json` holding the map.
pub const LOCAL_KEY: &str = "studio";

/// One repo's studio settings. Positional on the wire (`SetStudio`):
/// fields are only ever appended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioSettings {
    /// The inbox folder, relative to the repo root.
    #[serde(default)]
    pub studio_inbox: String,
    #[serde(default)]
    pub autonomy: Autonomy,
    /// Note ids or `about` values the queue runner leaves alone.
    #[serde(default)]
    pub queue_exclude: Vec<String>,
}

/// The one spelling a repo root is stored and looked up under: forward
/// slashes, no trailing slash, no `\\?\` prefix. On Windows a lookup also
/// ignores case ([`same_repo`]).
pub fn repo_key(repo_root: &Path) -> String {
    let raw = repo_root.to_string_lossy();
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let mut key = raw.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') && !key.ends_with(":/") {
        key.pop();
    }
    key
}

/// Whether two keys name one repo: equal, ignoring case on Windows.
pub fn same_repo(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// The entries in a raw settings object, keyed by [`repo_key`]. An entry
/// this build can't read is left out, not fatal.
pub fn entries_from(obj: &serde_json::Map<String, Value>) -> BTreeMap<PathBuf, StudioSettings> {
    obj.get(LOCAL_KEY)
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(key, _)| !key.trim().is_empty())
                .filter_map(|(key, value)| {
                    serde_json::from_value(value.clone())
                        .ok()
                        .map(|s| (PathBuf::from(repo_key(Path::new(key.trim()))), s))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every studio entry in `local` (a `config.local.json`). A missing or
/// unreadable file has none.
pub fn load_all_in(local: &Path) -> BTreeMap<PathBuf, StudioSettings> {
    crate::settings::read_object(local)
        .ok()
        .flatten()
        .map(|obj| entries_from(&obj))
        .unwrap_or_default()
}

/// [`load_all_in`] on this machine's `config.local.json`.
pub fn load_all() -> BTreeMap<PathBuf, StudioSettings> {
    load_all_in(&crate::paths::config_local_path())
}

/// `repo_root`'s entry in `local`, under any spelling of the root.
pub fn for_repo_in(local: &Path, repo_root: &Path) -> Option<StudioSettings> {
    let want = repo_key(repo_root);
    load_all_in(local)
        .into_iter()
        .find(|(key, _)| same_repo(&key.to_string_lossy(), &want))
        .map(|(_, settings)| settings)
}

/// [`for_repo_in`] on this machine's `config.local.json`.
pub fn for_repo(repo_root: &Path) -> Option<StudioSettings> {
    for_repo_in(&crate::paths::config_local_path(), repo_root)
}

/// Set (or with None, remove) `repo_root`'s entry in `local`, touching
/// nothing but the `studio` key: every other key of the file, and any
/// field of the entry this build has no name for, is kept. Taken under the
/// lock on `config` (`config.json`, the settings writers' one lock), which
/// is never created; the write is tmp-then-rename. A local file that is
/// there but can't be parsed is refused, never replaced.
pub fn write_in(
    config: &Path,
    local: &Path,
    repo_root: &Path,
    studio: Option<&StudioSettings>,
) -> Result<(), String> {
    let key = repo_key(repo_root);
    if key.trim().is_empty() {
        return Err("no repo named".into());
    }
    let _lock = crate::settings::lock(config).map_err(|e| e.to_string())?;
    let mut obj = crate::settings::read_object(local)
        .map_err(|e| format!("{}: {e}: fix it, then try again", local.display()))?
        .unwrap_or_default();
    let mut map = match obj.remove(LOCAL_KEY) {
        None => serde_json::Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(format!(
                "{}: `{LOCAL_KEY}` is not an object: fix it, then try again",
                local.display()
            ))
        }
    };
    // Whatever spelling the entry was stored under, it is this repo's.
    let mut entry = serde_json::Map::new();
    let stale: Vec<String> = map
        .keys()
        .filter(|k| same_repo(&repo_key(Path::new(k.trim())), &key))
        .cloned()
        .collect();
    for old in stale {
        if let Some(Value::Object(fields)) = map.remove(&old) {
            entry = fields;
        }
    }
    if let Some(studio) = studio {
        let Value::Object(known) = serde_json::to_value(studio).map_err(|e| e.to_string())? else {
            unreachable!("studio settings serialize to an object");
        };
        entry.extend(known);
        map.insert(key, Value::Object(entry));
    }
    if !map.is_empty() {
        obj.insert(LOCAL_KEY.into(), Value::Object(map));
    }
    crate::settings::write_json(local, &Value::Object(obj))
        .map_err(|e| format!("write {}: {e}", local.display()))
}

/// [`write_in`] on this machine's settings files.
pub fn write(repo_root: &Path, studio: Option<&StudioSettings>) -> Result<(), String> {
    write_in(
        &crate::paths::config_path(),
        &crate::paths::config_local_path(),
        repo_root,
        studio,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(autonomy: Autonomy) -> StudioSettings {
        StudioSettings {
            studio_inbox: "notes/inbox".into(),
            autonomy,
            queue_exclude: vec!["library-sweep-2".into()],
        }
    }

    #[test]
    fn a_repo_has_one_key_whatever_its_spelling() {
        assert_eq!(repo_key(Path::new(r"C:\Code\x")), "C:/Code/x");
        assert_eq!(repo_key(Path::new("C:/Code/x/")), "C:/Code/x");
        assert_eq!(repo_key(Path::new(r"\\?\C:\Code\x")), "C:/Code/x");
        assert_eq!(repo_key(Path::new("/home/j/x/")), "/home/j/x");
        assert_eq!(repo_key(Path::new("C:/")), "C:/");
        assert_eq!(repo_key(Path::new("/")), "/");
    }

    #[test]
    fn a_write_touches_only_studio_and_never_creates_config_json() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &local,
            r#"{"wake": {"work": {"enabled": true}}, "theme": "soot", "studio": {"C:/Other": {"studio_inbox": "in", "autonomy": "A3", "colour": "red"}}}"#,
        )
        .unwrap();
        write_in(
            &config,
            &local,
            Path::new(r"C:\Code\x"),
            Some(&entry(Autonomy::A2)),
        )
        .unwrap();
        assert!(!config.exists(), "config.json is never created");
        assert!(!dir.path().join("config.json.lock").exists());
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        assert_eq!(saved["wake"]["work"]["enabled"], true);
        assert_eq!(saved["theme"], "soot");
        assert_eq!(
            saved["studio"]["C:/Other"]["colour"], "red",
            "another repo kept whole"
        );
        assert_eq!(saved["studio"]["C:/Code/x"]["autonomy"], "A2");
        assert_eq!(saved["studio"]["C:/Code/x"]["studio_inbox"], "notes/inbox");
        assert_eq!(
            for_repo_in(&local, Path::new("C:/Code/x/")),
            Some(entry(Autonomy::A2))
        );
        assert_eq!(load_all_in(&local).len(), 2);
    }

    #[test]
    fn a_rewrite_keeps_unknown_fields_and_one_entry_per_repo() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &local,
            r#"{"studio": {"C:\\Code\\x\\": {"studio_inbox": "in", "autonomy": "A1", "note": "keep"}}}"#,
        )
        .unwrap();
        write_in(
            &config,
            &local,
            Path::new("C:/Code/x"),
            Some(&entry(Autonomy::A0)),
        )
        .unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        let map = saved["studio"].as_object().unwrap();
        assert_eq!(map.len(), 1, "{map:?}");
        assert_eq!(map["C:/Code/x"]["note"], "keep");
        assert_eq!(map["C:/Code/x"]["autonomy"], "A0");
    }

    #[test]
    fn removing_the_last_entry_drops_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        write_in(&config, &local, Path::new("/r"), Some(&entry(Autonomy::A1))).unwrap();
        assert!(for_repo_in(&local, Path::new("/r")).is_some());
        write_in(&config, &local, Path::new("/r"), None).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        assert!(saved.get("studio").is_none(), "{saved}");
        assert_eq!(for_repo_in(&local, Path::new("/r")), None);
    }

    #[test]
    fn an_unreadable_local_file_is_refused_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(&local, "{not json").unwrap();
        assert!(write_in(&config, &local, Path::new("/r"), Some(&entry(Autonomy::A1))).is_err());
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "{not json");
        assert!(load_all_in(&local).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_lookups_ignore_case() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        write_in(
            &config,
            &local,
            Path::new(r"C:\Code\My-game"),
            Some(&entry(Autonomy::A2)),
        )
        .unwrap();
        assert!(for_repo_in(&local, Path::new("c:/code/my-game")).is_some());
    }

    #[test]
    fn a_settings_entry_reads_with_missing_fields() {
        let s: StudioSettings = serde_json::from_str(r#"{"studio_inbox": "in"}"#).unwrap();
        assert_eq!(s.autonomy, Autonomy::A1);
        assert!(s.queue_exclude.is_empty());
        let wire = rmp_serde::to_vec(&entry(Autonomy::A3)).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<StudioSettings>(&wire).unwrap(),
            entry(Autonomy::A3)
        );
    }
}
