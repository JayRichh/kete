//! PICK HOLDS: `kete profile pick` holds the account it names for
//! [`HOLD_SECS`], so the next automation pick spreads before the usage
//! readings move. Kept in `<data dir>/profiles/.picks.json` as
//! `[{"profile": "max2", "at": <epoch secs>}]`, written temp-then-rename
//! under the file's lock. A missing or unreadable file holds nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How long one pick holds its account, seconds.
pub const HOLD_SECS: i64 = 600;

#[derive(Debug, Serialize, Deserialize)]
struct Hold {
    profile: String,
    at: i64,
}

/// The holds file under the profiles root.
pub fn path_in(root: &Path) -> PathBuf {
    root.join(".picks.json")
}

fn read_in(root: &Path) -> Vec<Hold> {
    std::fs::read_to_string(path_in(root))
        .ok()
        .and_then(|text| serde_json::from_str(crate::settings::strip_bom(&text)).ok())
        .unwrap_or_default()
}

fn live(hold: &Hold, now_secs: i64) -> bool {
    hold.at <= now_secs && now_secs < hold.at + HOLD_SECS
}

/// Live holds per profile name at `now_secs`.
pub fn active_in(root: &Path, now_secs: i64) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for hold in read_in(root).iter().filter(|h| live(h, now_secs)) {
        *out.entry(hold.profile.clone()).or_insert(0) += 1;
    }
    out
}

/// Hold `profile` from `now_secs`, dropping holds that have run out.
pub fn add_in(root: &Path, profile: &str, now_secs: i64) -> std::io::Result<()> {
    let path = path_in(root);
    let _lock = crate::settings::lock(&path)?;
    let mut holds: Vec<Hold> = read_in(root)
        .into_iter()
        .filter(|h| live(h, now_secs))
        .collect();
    holds.push(Hold {
        profile: profile.trim().to_string(),
        at: now_secs,
    });
    let bytes = serde_json::to_vec_pretty(&holds)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::settings::write_atomic(&path, &bytes)
}

/// [`active_in`] under the profiles root.
pub fn active(now_secs: i64) -> BTreeMap<String, u32> {
    active_in(&crate::profiles::profiles_root(), now_secs)
}

/// [`add_in`] under the profiles root.
pub fn add(profile: &str, now_secs: i64) -> std::io::Result<()> {
    add_in(&crate::profiles::profiles_root(), profile, now_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_counts_until_it_runs_out_at_600_seconds() {
        let dir = tempfile::tempdir().unwrap();
        add_in(dir.path(), "max2", 1_000).unwrap();
        add_in(dir.path(), "max2", 1_100).unwrap();
        add_in(dir.path(), "max3", 1_100).unwrap();
        let at = |now| active_in(dir.path(), now);
        assert_eq!(at(1_000).get("max2"), Some(&1));
        assert_eq!(at(1_599).get("max2"), Some(&2));
        assert_eq!(at(1_600).get("max2"), Some(&1), "the first ran out");
        assert_eq!(at(1_700), BTreeMap::new(), "every hold ran out");
        // A later add drops what ran out.
        add_in(dir.path(), "max4", 2_000).unwrap();
        let raw = std::fs::read_to_string(path_in(dir.path())).unwrap();
        assert!(!raw.contains("max2") && raw.contains("max4"), "{raw}");
    }

    #[test]
    fn a_missing_or_corrupt_file_holds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(active_in(dir.path(), 1_000).is_empty());
        std::fs::write(path_in(dir.path()), "{not json").unwrap();
        assert!(active_in(dir.path(), 1_000).is_empty());
        add_in(dir.path(), "max2", 1_000).unwrap();
        assert_eq!(active_in(dir.path(), 1_000).get("max2"), Some(&1));
    }
}
