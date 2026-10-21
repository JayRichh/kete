//! Role cards in the window (K3): the roles `repo` defines, for the role
//! pickers.
//!
//! Placeholder from the 0.60 foundation lane: the command answers "not in
//! this build yet". The roles lane owns this file.

use crate::Res;

/// The roles `repo`'s committed `.kete/roles.toml` defines.
#[tauri::command]
pub async fn roles_list(repo: String) -> Res<serde_json::Value> {
    let _ = repo;
    Err("not in this build yet".into())
}
