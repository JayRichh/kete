//! Where the daemon's hook receiver listens right now, written to the data
//! dir at every start. A session's env (`NEBULA_API_URL`, `NEBULA_API_TOKEN`)
//! is fixed when it spawns, but a Claude background job outlives the daemon
//! that spawned it: after a daemon restart the job's hooks would POST to a
//! dead port with a dead token, and its subagents, status and cwd would never
//! reach kete again. `kete _hook` falls back to this file when the env's
//! endpoint does not answer.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEndpoint {
    pub port: u16,
    pub token: String,
}

impl HookEndpoint {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub fn path() -> PathBuf {
    paths::data_dir().join("hook-endpoint.json")
}

/// Record the receiver's endpoint. Best effort: the env of freshly spawned
/// sessions still carries it, so a failed write only costs the fallback.
pub fn write(endpoint: &HookEndpoint) {
    write_to(&path(), endpoint);
}

pub fn read() -> Option<HookEndpoint> {
    read_from(&path())
}

fn write_to(file: &Path, endpoint: &HookEndpoint) {
    let Ok(json) = serde_json::to_string(endpoint) else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(file, json).is_ok() {
        restrict_to_owner(file);
    }
}

fn read_from(file: &Path) -> Option<HookEndpoint> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

#[cfg(unix)]
fn restrict_to_owner(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_file: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_survives_a_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hook-endpoint.json");
        let endpoint = HookEndpoint {
            port: 51667,
            token: "abc".into(),
        };
        write_to(&file, &endpoint);
        assert_eq!(read_from(&file), Some(endpoint.clone()));
        assert_eq!(endpoint.url(), "http://127.0.0.1:51667");
    }

    #[test]
    fn a_missing_or_garbled_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hook-endpoint.json");
        assert_eq!(read_from(&file), None);
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(read_from(&file), None);
    }
}
