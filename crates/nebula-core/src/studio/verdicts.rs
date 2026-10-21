//! Verdict files: `<signoffs>/<kind>/<id>.json`.
//!
//! Read leniently. A file that is not a JSON object is skipped, a missing
//! field is a blank, and unknown fields are ignored, so a newer Studio app
//! never breaks the page. A kind folder lists at most [`MAX_VERDICT_FILES`]
//! files, and a file over 1 MiB is skipped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A kind folder is read up to this many files.
pub const MAX_VERDICT_FILES: usize = 5_000;
/// A verdict file over this many bytes is skipped.
const MAX_VERDICT_BYTES: u64 = 1_048_576;
/// `<id>-reconciled.json` belongs to `<id>.json`.
const RECONCILED_SUFFIX: &str = "-reconciled";

/// One verdict file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// The `id` field, else the file name without `.json`.
    pub id: String,
    /// The folder the file sits in.
    pub kind: String,
    /// The verdict word as written; empty when the file has none.
    pub verdict: String,
    pub by: Option<String>,
    pub at: Option<String>,
    pub note: Option<String>,
    pub tags: Vec<String>,
    /// Evidence names as written in the file.
    pub evidence: Vec<String>,
    /// The `<id>-reconciled.json` file, when one exists.
    pub reconciled: Option<PathBuf>,
    pub path: PathBuf,
    /// The file's modified time, in epoch seconds.
    pub modified: Option<i64>,
}

/// The kind folders under `signoffs`, sorted. A missing folder is empty.
pub fn kinds(signoffs: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(signoffs) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Every verdict in one kind folder, newest first: by `at` (latest first,
/// none last), then by modified time, then by id.
pub fn read_kind(signoffs: &Path, kind: &str) -> Vec<Verdict> {
    let dir = signoffs.join(kind);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_json = Path::new(&name)
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"));
            let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
            (is_json && is_file).then(|| (name, e.path()))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.truncate(MAX_VERDICT_FILES);

    // (file stem, verdict) for every file that parsed.
    let parsed: Vec<(String, Verdict)> = files
        .into_iter()
        .filter_map(|(name, path)| {
            let stem = name
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(&name)
                .to_string();
            let verdict = read_file(&path, &stem, kind)?;
            Some((stem, verdict))
        })
        .collect();

    // A reconciled file is folded into its base when the base is there.
    let stems: HashMap<&str, ()> = parsed.iter().map(|(s, _)| (s.as_str(), ())).collect();
    let mut linked: HashMap<String, PathBuf> = HashMap::new();
    let mut folded: Vec<&str> = Vec::new();
    for (stem, v) in &parsed {
        if let Some(base) = stem.strip_suffix(RECONCILED_SUFFIX) {
            if !base.is_empty() && stems.contains_key(base) {
                linked.insert(base.to_string(), v.path.clone());
                folded.push(stem.as_str());
            }
        }
    }

    let mut out: Vec<Verdict> = parsed
        .iter()
        .filter(|(stem, _)| !folded.contains(&stem.as_str()))
        .map(|(stem, v)| {
            let mut v = v.clone();
            v.reconciled = linked.get(stem).cloned();
            v
        })
        .collect();
    out.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| b.modified.cmp(&a.modified))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// The newest verdict in one kind folder, if there is any.
pub fn newest(signoffs: &Path, kind: &str) -> Option<Verdict> {
    read_kind(signoffs, kind).into_iter().next()
}

fn read_file(path: &Path, stem: &str, kind: &str) -> Option<Verdict> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_VERDICT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let obj = value.as_object()?;

    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    Some(Verdict {
        id: text(obj.get("id"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| stem.to_string()),
        kind: kind.to_string(),
        verdict: text(obj.get("verdict")).unwrap_or_default(),
        // Sound sign-offs write `reviewer` and `date` where art writes `by` and `at`.
        by: text(obj.get("by"))
            .filter(|s| !s.is_empty())
            .or_else(|| text(obj.get("reviewer")).filter(|s| !s.is_empty())),
        at: text(obj.get("at"))
            .filter(|s| !s.is_empty())
            .or_else(|| text(obj.get("date")).filter(|s| !s.is_empty())),
        note: text(obj.get("note")).filter(|s| !s.is_empty()),
        tags: list(obj.get("tags")),
        evidence: list(obj.get("evidence")),
        reconciled: None,
        path: path.to_path_buf(),
        modified,
    })
}

/// A string field; a number is written out. Anything else is absent.
fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A list field: one string is one item, an array keeps its strings and
/// numbers.
fn list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| text(Some(i)))
            .filter(|s| !s.is_empty())
            .collect(),
        Some(other) => text(Some(other))
            .filter(|s| !s.is_empty())
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, kind: &str, name: &str, body: &str) {
        let dir = root.join(kind);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn the_studio_shape_reads() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "art",
            "C02.json",
            r#"{"id":"C02","verdict":"regenerate","by":"third-reviewer","at":"2026-10-19","note":"too soft"}"#,
        );
        let v = newest(dir.path(), "art").unwrap();
        assert_eq!(v.id, "C02");
        assert_eq!(v.kind, "art");
        assert_eq!(v.verdict, "regenerate");
        assert_eq!(v.by.as_deref(), Some("third-reviewer"));
        assert_eq!(v.at.as_deref(), Some("2026-10-19"));
        assert_eq!(v.note.as_deref(), Some("too soft"));
        assert!(v.tags.is_empty() && v.evidence.is_empty());
        assert!(v.modified.is_some());
        assert_eq!(v.reconciled, None);
    }

    #[test]
    fn the_sound_shape_reads_reviewer_and_date() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "sound",
            "call.json",
            r#"{"id":"call","verdict":"rejected","reviewer":"Jay","date":"2026-10-19"}"#,
        );
        put(
            dir.path(),
            "sound",
            "both.json",
            r#"{"by":"first","reviewer":"second","at":"2026-10-20","date":"2026-10-01"}"#,
        );
        let got = read_kind(dir.path(), "sound");
        let call = got.iter().find(|v| v.id == "call").unwrap();
        assert_eq!(call.by.as_deref(), Some("Jay"));
        assert_eq!(call.at.as_deref(), Some("2026-10-19"));
        // `by` and `at` win when a file has both spellings.
        let both = got.iter().find(|v| v.id == "both").unwrap();
        assert_eq!(both.by.as_deref(), Some("first"));
        assert_eq!(both.at.as_deref(), Some("2026-10-20"));
        // The fallback date takes part in the newest-first order.
        assert_eq!(got[0].id, "both");
    }

    #[test]
    fn lenient_fields() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "art", "bare.json", "{}");
        put(
            dir.path(),
            "art",
            "numeric.json",
            r#"{"id": 17, "verdict": "approve", "by": 4, "tags": "solo", "evidence": ["EV1", 2, ""],
                "surprise": {"a": 1}}"#,
        );
        put(
            dir.path(),
            "art",
            "list.json",
            r#"{"tags": ["a","b"], "evidence": "EV9"}"#,
        );
        let got = read_kind(dir.path(), "art");
        assert_eq!(got.len(), 3);
        let bare = got.iter().find(|v| v.id == "bare").unwrap();
        assert_eq!(bare.verdict, "");
        assert_eq!(bare.by, None);
        let numeric = got.iter().find(|v| v.id == "17").unwrap();
        assert_eq!(numeric.verdict, "approve");
        assert_eq!(numeric.by.as_deref(), Some("4"));
        assert_eq!(numeric.tags, vec!["solo"]);
        assert_eq!(numeric.evidence, vec!["EV1", "2"]);
        let list = got.iter().find(|v| v.id == "list").unwrap();
        assert_eq!(list.tags, vec!["a", "b"]);
        assert_eq!(list.evidence, vec!["EV9"]);
    }

    #[test]
    fn a_reconciled_pair_is_one_row() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "art",
            "C02.json",
            r#"{"verdict":"split","at":"2026-10-18"}"#,
        );
        put(
            dir.path(),
            "art",
            "C02-reconciled.json",
            r#"{"verdict":"approve"}"#,
        );
        put(
            dir.path(),
            "art",
            "C09-reconciled.json",
            r#"{"verdict":"approve","at":"2026-10-20"}"#,
        );
        let got = read_kind(dir.path(), "art");
        assert_eq!(got.len(), 2);
        let base = got.iter().find(|v| v.id == "C02").unwrap();
        assert_eq!(base.verdict, "split");
        assert_eq!(
            base.reconciled.as_deref(),
            Some(dir.path().join("art").join("C02-reconciled.json").as_path())
        );
        // No base for C09: it stands alone.
        let alone = got.iter().find(|v| v.id == "C09-reconciled").unwrap();
        assert_eq!(alone.reconciled, None);
    }

    #[test]
    fn non_json_and_non_objects_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "art", "ok.json", r#"{"verdict":"pass"}"#);
        put(dir.path(), "art", "broken.json", "{ not json");
        put(dir.path(), "art", "array.json", "[1,2,3]");
        put(dir.path(), "art", "notes.txt", r#"{"verdict":"pass"}"#);
        let got = read_kind(dir.path(), "art");
        assert_eq!(
            got.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            vec!["ok"]
        );
    }

    #[test]
    fn kinds_are_sorted_and_missing_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "sound", "a.json", "{}");
        put(dir.path(), "art", "a.json", "{}");
        put(dir.path(), "story", "a.json", "{}");
        std::fs::write(dir.path().join("README.md"), "x").unwrap();
        assert_eq!(kinds(dir.path()), vec!["art", "sound", "story"]);
        assert!(kinds(&dir.path().join("nope")).is_empty());
        assert!(read_kind(dir.path(), "nope").is_empty());
        assert!(newest(dir.path(), "nope").is_none());
    }

    #[test]
    fn newest_first_by_at_then_id() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "art", "a.json", r#"{"at":"2026-10-01"}"#);
        put(dir.path(), "art", "b.json", r#"{"at":"2026-10-19"}"#);
        put(dir.path(), "art", "c.json", "{}");
        put(dir.path(), "art", "d.json", r#"{"at":"2026-10-19"}"#);
        let got = read_kind(dir.path(), "art");
        let order: Vec<&str> = got.iter().map(|v| v.id.as_str()).collect();
        // Equal `at` ties break on modified time (same here, or newer
        // first), then id; a file with no `at` comes last.
        assert_eq!(order[2], "a");
        assert_eq!(order[3], "c");
        assert!(order[..2].contains(&"b") && order[..2].contains(&"d"));
        assert_eq!(
            newest(dir.path(), "art").unwrap().at.as_deref(),
            Some("2026-10-19")
        );
    }

    #[test]
    fn a_file_over_one_mib_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let pad = " ".repeat(MAX_VERDICT_BYTES as usize);
        put(
            dir.path(),
            "art",
            "big.json",
            &format!("{{\"verdict\":\"pass\"}}{pad}"),
        );
        put(dir.path(), "art", "small.json", r#"{"verdict":"pass"}"#);
        let got = read_kind(dir.path(), "art");
        assert_eq!(
            got.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            vec!["small"]
        );
    }
}
