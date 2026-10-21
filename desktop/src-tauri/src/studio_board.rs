//! The studio board's window side (K4): the file-derived half of the
//! board (departments, notes, verdicts, refusals, the last renewal).
//!
//! Placeholder from the 0.60 foundation lane: the command answers "not in
//! this build yet". The board lane owns this file.

use crate::Res;

/// The board's file-derived half for `repo` (a project id).
#[tauri::command]
pub async fn studio_board(repo: String) -> Res<serde_json::Value> {
    let _ = repo;
    Err("not in this build yet".into())
}
