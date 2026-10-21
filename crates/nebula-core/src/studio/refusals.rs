//! The refusal log: why Kete turned away a studio launch.
//!
//! One JSON object per line in `<data>/studio/refusals.jsonl`. The board
//! shows the last few. The file is capped: once it passes 1 MiB the oldest
//! half of its lines are dropped.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The log is trimmed when it grows past this many bytes.
pub const MAX_REFUSALS_BYTES: u64 = 1_048_576;

/// One refused launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// When it happened, in epoch milliseconds.
    pub at: i64,
    /// The project root the launch was for.
    pub project: String,
    /// The agent that was refused, when there was one.
    #[serde(default)]
    pub agent: Option<String>,
    /// The role card the launch named, when there was one.
    #[serde(default)]
    pub role: Option<String>,
    /// Why it was refused, in plain words.
    pub reason: String,
}

/// `<data dir>/studio/refusals.jsonl`.
pub fn refusals_path() -> PathBuf {
    crate::paths::data_dir()
        .join("studio")
        .join("refusals.jsonl")
}

/// Appends a refusal to the real log.
pub fn append(refusal: &Refusal) -> Result<(), String> {
    append_in(&refusals_path(), refusal)
}

/// The last `n` readable refusals from the real log, oldest first.
pub fn read_last(n: usize) -> Vec<Refusal> {
    read_last_in(&refusals_path(), n)
}

/// Appends a refusal to the log at `path`, creating the folder. When the
/// file is then over [`MAX_REFUSALS_BYTES`], the oldest half of its lines
/// are dropped.
pub fn append_in(path: &Path, refusal: &Refusal) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let mut line =
        serde_json::to_string(refusal).map_err(|e| format!("could not write the refusal: {e}"))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    drop(file);

    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_REFUSALS_BYTES {
        trim(path)?;
    }
    Ok(())
}

/// Drops the oldest half of the lines in `path`.
fn trim(path: &Path) -> Result<(), String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 {
        return Ok(());
    }
    let mut kept = lines[lines.len() / 2..].join("\n");
    kept.push('\n');
    crate::settings::write_atomic(path, kept.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// The last `n` readable refusals in the log at `path`, oldest first. A
/// missing file is empty and a line that does not parse is skipped.
pub fn read_last_in(path: &Path, n: usize) -> Vec<Refusal> {
    if n == 0 {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut all: Vec<Refusal> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Refusal>(l.trim()).ok())
        .collect();
    if all.len() > n {
        all.drain(..all.len() - n);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(at: i64, reason: &str) -> Refusal {
        Refusal {
            at,
            project: "C:\\Code\\game".into(),
            agent: Some("a1".into()),
            role: None,
            reason: reason.into(),
        }
    }

    #[test]
    fn append_then_read_last() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio").join("refusals.jsonl");
        append_in(&path, &refusal(1, "first")).unwrap();
        append_in(&path, &refusal(2, "second")).unwrap();
        let got = read_last_in(&path, 10);
        assert_eq!(got, vec![refusal(1, "first"), refusal(2, "second")]);
    }

    #[test]
    fn read_last_takes_the_newest_n_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        for i in 0..5 {
            append_in(&path, &refusal(i, "x")).unwrap();
        }
        let got = read_last_in(&path, 2);
        assert_eq!(got.iter().map(|r| r.at).collect::<Vec<_>>(), vec![3, 4]);
        assert!(read_last_in(&path, 0).is_empty());
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_last_in(&dir.path().join("none.jsonl"), 5).is_empty());
    }

    #[test]
    fn a_garbage_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        append_in(&path, &refusal(1, "a")).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"not json at all\n{\"at\":1}\n").unwrap();
        drop(f);
        append_in(&path, &refusal(2, "b")).unwrap();
        let got = read_last_in(&path, 10);
        assert_eq!(got.iter().map(|r| r.at).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        std::fs::write(&path, "{\"at\":9,\"project\":\"p\",\"reason\":\"why\"}\n").unwrap();
        let got = read_last_in(&path, 1);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].agent, None);
        assert_eq!(got[0].role, None);
    }

    #[test]
    fn the_log_is_trimmed_past_one_mib() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.jsonl");
        let reason = "y".repeat(10_000);
        let total = 130;
        for i in 0..total {
            append_in(&path, &refusal(i, &reason)).unwrap();
        }
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size <= MAX_REFUSALS_BYTES, "size {size}");
        let all = read_last_in(&path, usize::MAX);
        assert!(all.len() < total as usize);
        assert_eq!(all.last().unwrap().at, total - 1);
        assert!(all.first().unwrap().at > 0);
        // What is kept is a run of the newest lines, with no gaps.
        for pair in all.windows(2) {
            assert_eq!(pair[1].at, pair[0].at + 1);
        }
    }
}
