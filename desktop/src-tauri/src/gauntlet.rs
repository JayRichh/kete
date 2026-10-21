//! The gauntlet view's window side (K5): verdicts by kind, an evidence
//! folder's listing, and opening the Studio app on a verdict.
//!
//! Placeholder from the 0.60 foundation lane: each command answers "not in
//! this build yet". The gauntlet lane owns this file.

use crate::Res;

/// Every verdict in `repo`'s sign-off folders, by kind.
#[tauri::command]
pub async fn gauntlet_list(repo: String) -> Res<serde_json::Value> {
    let _ = repo;
    Err("not in this build yet".into())
}

/// The listing of one evidence folder `ev` in `repo`.
#[tauri::command]
pub async fn gauntlet_evidence(repo: String, ev: String) -> Res<serde_json::Value> {
    let _ = (repo, ev);
    Err("not in this build yet".into())
}

/// Open the Studio app on verdict `id` of `kind`.
#[tauri::command]
pub async fn gauntlet_open_studio(repo: String, kind: String, id: String) -> Res<()> {
    let _ = (repo, kind, id);
    Err("not in this build yet".into())
}
