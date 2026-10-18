//! The settings files both halves of nebula read, and the rules that keep
//! them readable across releases.
//!
//! Settings come in two layers, merged key by key: `config.json`, the
//! portable file — what a backup or `nebula ssh` carries to another machine,
//! and what `NEBULA_CONFIG_FILE` can move into a dotfiles checkout — and
//! `config.local.json` beside the database, for what only makes sense on
//! this machine. The local layer wins, and is never exported, forwarded or
//! overwritten by an import. The TUI's `Config` and the daemon's each
//! deserialize their own fields out of the merged object.
//!
//! Two nebula releases routinely share these files: a remote a few versions
//! behind the machine that sshes into it, a backup restored into a newer
//! build. So a key one build cannot read must cost only that key.
//! [`parse_lenient`] drops what fails and keeps everything else, where a
//! whole-file parse turns one mistyped value into every default — and the
//! next save writes those defaults over the lot.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A JSON object, as the settings files hold one.
pub type Object = Map<String, Value>;

/// The merged settings, and what went into them.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    /// Top-level keys `T` could not read, from either layer. They take
    /// their defaults in `value`; a writer should leave their stored JSON
    /// alone rather than write that default back over it.
    pub skipped: BTreeSet<String>,
    /// A layer that exists but could not be used, one line each. That layer
    /// is ignored as a whole; the other still applies.
    pub problems: Vec<String>,
}

/// Read both layers and deserialize `T` from their merge, `local` over
/// `config`. `T` must be `#[serde(default)]`: a missing key is its default.
/// `session_idle_timeout`'s default, and the DAEMON's fallback when the
/// value is malformed. The daemon and the TUI both default their configs
/// to it.
pub const DEFAULT_SESSION_IDLE_TIMEOUT: &str = "5m";

/// What **Keep finished subagents** (`subagent_retention`) cycles
/// through: how long an ended subagent stays in a session's book before
/// the daemon drops it. `off` keeps them until cleared by hand or until
/// [`crate::SUBAGENTS_ENDED_KEPT`] newer ones push them out.
pub const SUBAGENT_RETENTIONS: &[&str] = &["10m", "1h", "4h", "1d", "off"];

/// `subagent_retention`'s default, and the fallback for a word off the
/// list.
pub const DEFAULT_SUBAGENT_RETENTION: &str = "1h";

/// `subagent_retention` as milliseconds; None for `off`. A word off
/// [`SUBAGENT_RETENTIONS`] reads as [`DEFAULT_SUBAGENT_RETENTION`].
pub fn subagent_retention_ms(word: &str) -> Option<i64> {
    let known = SUBAGENT_RETENTIONS
        .iter()
        .copied()
        .find(|w| w.eq_ignore_ascii_case(word.trim()))
        .unwrap_or(DEFAULT_SUBAGENT_RETENTION);
    const MINUTE: i64 = 60_000;
    match known {
        "10m" => Some(10 * MINUTE),
        "1h" => Some(60 * MINUTE),
        "4h" => Some(4 * 60 * MINUTE),
        "1d" => Some(24 * 60 * MINUTE),
        _ => None,
    }
}

pub fn load<T: DeserializeOwned + Default>(config: &Path, local: &Path) -> Loaded<T> {
    let mut problems = Vec::new();
    let mut merged = read_layer(config, &mut problems);
    merged.extend(read_layer(local, &mut problems));
    let (value, skipped) = parse_lenient(&merged);
    Loaded {
        value,
        skipped,
        problems,
    }
}

fn read_layer(path: &Path, problems: &mut Vec<String>) -> Object {
    read_object(path)
        .unwrap_or_else(|err| {
            problems.push(format!("ignoring {}: {err}", path.display()));
            None
        })
        .unwrap_or_default()
}

/// `T` out of `obj`, one top-level key at a time: a key whose value `T`
/// rejects is left out, so it takes its default, and is named in the second
/// half. Keys `T` has no field for are ignored, as serde always ignores them.
pub fn parse_lenient<T: DeserializeOwned + Default>(obj: &Object) -> (T, BTreeSet<String>) {
    if let Ok(value) = serde_json::from_value(Value::Object(obj.clone())) {
        return (value, BTreeSet::new());
    }
    let mut skipped: BTreeSet<String> = obj
        .iter()
        .filter(|(key, value)| {
            let single = Object::from_iter([((*key).clone(), (*value).clone())]);
            serde_json::from_value::<T>(Value::Object(single)).is_err()
        })
        .map(|(key, _)| key.clone())
        .collect();
    let readable: Object = obj
        .iter()
        .filter(|(key, _)| !skipped.contains(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if let Ok(value) = serde_json::from_value(Value::Object(readable.clone())) {
        return (value, skipped);
    }
    // Every key reads alone, yet not all together: two of them clash — a
    // field under its name and under its `alias` both, the old key an
    // older build wrote still beside the new one. Add the keys back one at
    // a time and leave out each that breaks what already reads, so the
    // clash costs the later key of the pair rather than every value.
    let mut kept = Object::new();
    for (key, value) in readable {
        kept.insert(key.clone(), value);
        if serde_json::from_value::<T>(Value::Object(kept.clone())).is_err() {
            kept.remove(&key);
            skipped.insert(key);
        }
    }
    let value = serde_json::from_value(Value::Object(kept)).unwrap_or_default();
    (value, skipped)
}

/// A JSON object file: `Ok(None)` when there is no file, `Err` when it can't
/// be read or holds something other than an object.
pub fn read_object(path: &Path) -> Result<Option<Object>, String> {
    match read_json(path)? {
        None => Ok(None),
        Some(Value::Object(obj)) => Ok(Some(obj)),
        Some(_) => Err("not a JSON object".into()),
    }
}

/// A JSON list file, on the same terms as [`read_object`].
pub fn read_array(path: &Path) -> Result<Option<Vec<Value>>, String> {
    match read_json(path)? {
        None => Ok(None),
        Some(Value::Array(entries)) => Ok(Some(entries)),
        Some(_) => Err("not a JSON list".into()),
    }
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    serde_json::from_str(strip_bom(&raw))
        .map(Some)
        .map_err(|err| format!("not valid JSON ({err})"))
}

/// `raw` without a leading UTF-8 byte order mark. Notepad and Windows
/// PowerShell 5's `Set-Content -Encoding utf8` both write one, and
/// serde_json rejects it as the first character of a document.
pub fn strip_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

/// A list file read entry by entry: the entries `T` reads, then the raw ones
/// it doesn't — most likely a newer nebula's, and worth keeping on the next
/// save. A missing or unreadable file reads as empty: the list stores are
/// conveniences, never load-bearing.
pub fn read_list<T: DeserializeOwned>(path: &Path) -> (Vec<T>, Vec<Value>) {
    let mut parsed = Vec::new();
    let mut unread = Vec::new();
    for entry in read_array(path).ok().flatten().unwrap_or_default() {
        match serde_json::from_value::<T>(entry.clone()) {
            Ok(value) => parsed.push(value),
            Err(_) => unread.push(entry),
        }
    }
    (parsed, unread)
}

/// Pretty-print `value` into `path` through [`write_atomic`], ending in a
/// newline.
pub fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// Replace a file's contents atomically: write a temp file beside it and
/// rename that into place, so a crash mid-write never leaves half a file. A
/// symlink at `path` is written through rather than replaced — a dotfiles
/// setup links `config.json` into a repo, and the rename would otherwise
/// swap the link for a plain file (#50).
///
/// The temp file's name is this writer's own (`<name>.<pid>-<n>.tmp`):
/// the TUI and the desktop window save the same settings files, and two
/// saves sharing one temp name could rename the other's half-written bytes
/// into place. On Windows a rename onto a file another process has open
/// (an editor, a virus scanner, the other app reading it) fails for a
/// moment with access denied (5) or a sharing violation (32); it is retried
/// for about a second before giving up. A successful write also sweeps
/// temp files of this name more than a minute old — what a writer killed
/// between its write and its rename leaves behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let target = resolve_links(path);
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp_name = target.file_name().map(OsString::from).unwrap_or_default();
    tmp_name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = target.with_file_name(tmp_name);
    std::fs::write(&tmp, bytes)?;
    let mut tries = 0;
    loop {
        match std::fs::rename(&tmp, &target) {
            Ok(()) => {
                sweep_stale_temps(&target, STALE_TEMP);
                return Ok(());
            }
            Err(err) if rename_blocked(&err) && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(err);
            }
        }
    }
}

/// How old a leftover `<name>.*.tmp` must be before a write sweeps it: far
/// longer than any live writer holds its temp file.
const STALE_TEMP: Duration = Duration::from_secs(60);

/// A rename Windows refuses only while another handle is open on the file.
fn rename_blocked(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(5 | 32))
}

/// Remove `<name>.<anything>.tmp` beside `target` last modified more than
/// `age` ago. Best effort: a file another writer holds is skipped.
fn sweep_stale_temps(target: &Path, age: Duration) {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        if !(file.starts_with(&prefix) && file.ends_with(".tmp")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|elapsed| elapsed > age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// How long [`lock`] waits for another writer before giving up.
pub const LOCK_WAIT: Duration = Duration::from_secs(6);
/// How old a lock file must be before [`lock`] takes it for one its holder
/// left behind (a crash) and breaks it. A real hold is one read and one
/// write of a small file — milliseconds, a second at worst on Windows.
pub const LOCK_STALE: Duration = Duration::from_secs(4);

/// A held lock on a settings file: `<name>.lock` beside it, holding this
/// holder's own token. Dropping it removes the lock file — only while it
/// still holds that token, so a holder whose lock was broken as stale never
/// removes the next holder's.
#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    token: String,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|held| held == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Lock the settings file at `path` against the other kete writers — the
/// TUI's saves, the desktop window's, `kete profile` — for one
/// read-modify-write. Waits up to [`LOCK_WAIT`] for a holder, breaking a
/// lock older than [`LOCK_STALE`]; then fails rather than write unlocked.
/// Not reentrant: take it once, at the outermost write.
pub fn lock(path: &Path) -> io::Result<FileLock> {
    lock_with(path, LOCK_WAIT, LOCK_STALE)
}

/// [`lock`] with its waits spelled out.
pub fn lock_with(path: &Path, wait: Duration, stale: Duration) -> io::Result<FileLock> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let target = resolve_links(path);
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = target.file_name().map(OsString::from).unwrap_or_default();
    name.push(".lock");
    let lock_path = target.with_file_name(name);
    let token = format!(
        "{} {} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let started = std::time::Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                let lock = FileLock {
                    path: lock_path,
                    token,
                };
                file.write_all(lock.token.as_bytes())?;
                return Ok(lock);
            }
            // On Windows a lock file another writer is removing reads as
            // access denied for a moment: as held as an existing one.
            Err(err)
                if err.kind() == io::ErrorKind::AlreadyExists
                    || err.kind() == io::ErrorKind::PermissionDenied => {}
            Err(err) => return Err(err),
        }
        if lock_is_stale(&lock_path, stale) {
            let seen = std::fs::read_to_string(&lock_path).ok();
            // Break it only if it is still the lock judged stale.
            if lock_is_stale(&lock_path, stale) && std::fs::read_to_string(&lock_path).ok() == seen
            {
                let _ = std::fs::remove_file(&lock_path);
            }
            continue;
        }
        if started.elapsed() >= wait {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "another kete is writing {} (its lock {} is held); nothing was written: try again",
                    target.display(),
                    lock_path.display()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn lock_is_stale(lock_path: &Path, stale: Duration) -> bool {
    std::fs::metadata(lock_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|elapsed| elapsed > stale)
}

/// `path` followed through any symlinks to the file they name, which need
/// not exist yet. Gives up after 40 hops — the kernel's own loop bound — and
/// leaves a loop to fail at the write.
fn resolve_links(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    for _ in 0..40 {
        let is_link = std::fs::symlink_metadata(&current)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false);
        let Some(link) = is_link.then(|| std::fs::read_link(&current).ok()).flatten() else {
            break;
        };
        current = match current.parent() {
            Some(parent) if link.is_relative() => parent.join(link),
            _ => link,
        };
    }
    current
}

#[cfg(test)]
mod tests {
    #[test]
    fn subagent_retention_reads_its_words_and_falls_back_to_an_hour() {
        assert_eq!(subagent_retention_ms("10m"), Some(600_000));
        assert_eq!(subagent_retention_ms("1D"), Some(86_400_000));
        assert_eq!(subagent_retention_ms("off"), None);
        assert_eq!(subagent_retention_ms("soon"), Some(3_600_000));
        assert_eq!(subagent_retention_ms(""), Some(3_600_000));
    }

    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(default)]
    struct Sample {
        on: bool,
        count: u32,
        /// Renamed: a file an older build wrote says `old_name`.
        #[serde(alias = "old_name")]
        name: String,
    }

    impl Default for Sample {
        fn default() -> Self {
            Self {
                on: true,
                count: 3,
                name: "x".into(),
            }
        }
    }

    fn object(value: Value) -> Object {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn one_unreadable_key_costs_only_that_key() {
        let obj = object(json!({
            "on": false,
            "count": "three",
            "name": "y",
            "a_newer_key": [1, 2],
        }));
        let (sample, skipped) = parse_lenient::<Sample>(&obj);
        assert_eq!(
            sample,
            Sample {
                on: false,
                count: 3,
                name: "y".into()
            },
            "the mistyped count takes its default, the rest keep their values"
        );
        assert_eq!(skipped, BTreeSet::from(["count".to_string()]));

        let (_, skipped) = parse_lenient::<Sample>(&object(json!({"on": false})));
        assert!(skipped.is_empty(), "a readable object skips nothing");
    }

    /// A file holding a field under both its name and its old alias —
    /// the old key left behind by an older build, the new one patched in
    /// beside it — is a `duplicate field` to serde as a whole, though each
    /// key reads fine alone. That must cost the one key, not every value.
    #[test]
    fn a_renamed_key_beside_its_old_name_costs_only_the_old_name() {
        let obj = object(json!({
            "on": false,
            "count": 7,
            "name": "new",
            "old_name": "stale",
        }));
        let (sample, skipped) = parse_lenient::<Sample>(&obj);
        assert_eq!(
            sample,
            Sample {
                on: false,
                count: 7,
                name: "new".into()
            },
            "the other values survive the clash"
        );
        assert_eq!(skipped, BTreeSet::from(["old_name".to_string()]));

        let (sample, skipped) = parse_lenient::<Sample>(&object(json!({"old_name": "old"})));
        assert_eq!(sample.name, "old", "the old name alone still reads");
        assert!(skipped.is_empty());
    }

    #[test]
    fn the_local_layer_wins_key_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let (config, local) = (
            dir.path().join("config.json"),
            dir.path().join("config.local.json"),
        );
        std::fs::write(&config, r#"{"on": false, "count": 5}"#).unwrap();
        std::fs::write(&local, r#"{"count": 9}"#).unwrap();
        let loaded = load::<Sample>(&config, &local);
        assert_eq!(
            loaded.value,
            Sample {
                on: false,
                count: 9,
                name: "x".into()
            }
        );
        assert!(loaded.skipped.is_empty() && loaded.problems.is_empty());
    }

    #[test]
    fn a_broken_layer_is_reported_and_the_other_still_applies() {
        let dir = tempfile::tempdir().unwrap();
        let (config, local) = (
            dir.path().join("config.json"),
            dir.path().join("config.local.json"),
        );
        std::fs::write(&config, "{ not json").unwrap();
        std::fs::write(&local, r#"{"count": 9}"#).unwrap();
        let loaded = load::<Sample>(&config, &local);
        assert_eq!(loaded.value.count, 9);
        assert!(loaded.value.on, "the broken layer's keys are defaults");
        assert_eq!(loaded.problems.len(), 1, "{:?}", loaded.problems);
        assert!(loaded.problems[0].contains("config.json"));

        std::fs::write(&local, "[1, 2]").unwrap();
        let loaded = load::<Sample>(&config, &local);
        assert_eq!(loaded.value, Sample::default());
        assert_eq!(loaded.problems.len(), 2, "{:?}", loaded.problems);
    }

    #[test]
    fn a_byte_order_mark_is_read_past() {
        // Notepad and PowerShell 5's Set-Content -Encoding utf8 both start
        // the file with one; serde_json rejects it as the first character.
        let dir = tempfile::tempdir().unwrap();
        let (config, local) = (
            dir.path().join("config.json"),
            dir.path().join("config.local.json"),
        );
        std::fs::write(&config, "\u{feff}{\"count\": 4}").unwrap();
        std::fs::write(&local, "\u{feff}{\"on\": false}").unwrap();
        let loaded = load::<Sample>(&config, &local);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        assert_eq!(loaded.value.count, 4);
        assert!(!loaded.value.on);
        assert_eq!(read_object(&config).unwrap().unwrap()["count"], 4);
        let list = dir.path().join("list.json");
        std::fs::write(&list, "\u{feff}[{\"count\": 1}]").unwrap();
        assert_eq!(read_list::<Sample>(&list).0.len(), 1);
    }

    #[test]
    fn missing_layers_are_all_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load::<Sample>(&dir.path().join("a.json"), &dir.path().join("b.json"));
        assert_eq!(loaded.value, Sample::default());
        assert!(loaded.problems.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_write_goes_through_a_symlink_instead_of_replacing_it() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("nebula.json"), "{}\n").unwrap();
        let link = dir.path().join("config.json");
        std::os::unix::fs::symlink("dotfiles/nebula.json", &link).unwrap();

        write_json(&link, &json!({"theme": "ocean"})).unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link survives the save"
        );
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(real.join("nebula.json")).unwrap())
                .unwrap();
        assert_eq!(saved["theme"], "ocean", "the file it names took the write");
        assert!(no_temp_files(&real) && no_temp_files(dir.path()));
    }

    #[test]
    fn a_write_creates_missing_directories_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("list.json");
        write_atomic(&path, b"[]\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]\n");
        assert!(no_temp_files(path.parent().unwrap()));
    }

    fn no_temp_files(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
    }

    #[test]
    fn writers_racing_on_one_file_each_land_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        write_json(&path, &json!({ "writer": n, "i": i })).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let last: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(last["i"], 24, "a whole document, never a torn one");
        assert!(no_temp_files(dir.path()));
    }

    fn age(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn a_lock_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let lock_file = dir.path().join("config.json.lock");
        let held = lock(&path).unwrap();
        assert!(lock_file.exists());
        let err = lock_with(&path, Duration::from_millis(50), Duration::from_secs(60)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("nothing was written"), "{err}");
        drop(held);
        assert!(!lock_file.exists(), "dropping the lock removes its file");
        drop(lock(&path).unwrap());
    }

    #[test]
    fn a_waiting_writer_gets_the_lock_once_it_is_let_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let held = lock(&path).unwrap();
        let waiter = {
            let path = path.clone();
            std::thread::spawn(move || lock_with(&path, Duration::from_secs(5), LOCK_STALE).is_ok())
        };
        std::thread::sleep(Duration::from_millis(100));
        drop(held);
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn a_stale_lock_is_broken_and_its_late_holder_leaves_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let lock_file = dir.path().join("config.json.lock");
        let crashed = lock(&path).unwrap();
        age(&lock_file, 30);
        let next = lock_with(&path, Duration::from_millis(500), Duration::from_secs(5)).unwrap();
        // The old holder wakes up and lets go: the new holder's lock stays.
        drop(crashed);
        assert!(lock_file.exists());
        drop(next);
        assert!(!lock_file.exists());
    }

    #[test]
    fn a_write_sweeps_temp_files_a_killed_writer_left() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let old = dir.path().join("config.json.999-1.tmp");
        let fresh = dir.path().join("config.json.999-2.tmp");
        let other = dir.path().join("hosts.json.999-1.tmp");
        for f in [&old, &fresh, &other] {
            std::fs::write(f, "{").unwrap();
        }
        age(&old, 120);
        age(&other, 120);
        write_json(&path, &json!({})).unwrap();
        assert!(!old.exists(), "a minute-old temp of this file goes");
        assert!(fresh.exists(), "a live writer's temp stays");
        assert!(other.exists(), "another file's temp is not ours");
    }

    #[cfg(windows)]
    #[test]
    fn a_rename_blocked_by_an_open_handle_is_retried() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();
        // Share read only: a rename onto it fails until the handle closes.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        write_json(&path, &json!({"after": true})).unwrap();
        release.join().unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["after"], true);
    }

    #[test]
    fn a_list_keeps_the_entries_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        std::fs::write(&path, r#"[{"count": 1}, {"count": "many"}, {"on": false}]"#).unwrap();
        let (parsed, unread) = read_list::<Sample>(&path);
        assert_eq!(parsed.len(), 2);
        assert_eq!(unread, vec![json!({"count": "many"})]);

        std::fs::write(&path, "not json").unwrap();
        let (parsed, unread) = read_list::<Sample>(&path);
        assert!(parsed.is_empty() && unread.is_empty());
    }
}
