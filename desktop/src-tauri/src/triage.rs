//! The triage strip's window side (K6): file a note and hand it to the
//! desk, and the notes filed lately.
//!
//! Placeholder from the 0.60 foundation lane: each command answers "not in
//! this build yet". The triage lane owns this file.

use crate::Res;

/// File `text` as an inbox note in `repo` and send it to `target` (the
/// triage desk, or the orchestrator).
#[tauri::command]
pub async fn triage_submit(
    repo: String,
    text: String,
    target: Option<String>,
) -> Res<serde_json::Value> {
    let _ = (repo, text, target);
    Err("not in this build yet".into())
}

/// The route line and the last notes filed in `repo`.
#[tauri::command]
pub async fn triage_tail(repo: String) -> Res<serde_json::Value> {
    let _ = repo;
    Err("not in this build yet".into())
}
