//! The studio queue's window side (K2): what the runner would send, and
//! the autonomy control's writes (`SetStudio`).
//!
//! Placeholder from the 0.60 foundation lane: each command answers "not in
//! this build yet". The queue lane owns this file; the command names and
//! arguments are registered in `main.rs` and called from `api.ts`.

use crate::Res;
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::StudioSettings;

/// What the queue runner would do now, for `repo` (a project id) or every
/// studio repo; with `run`, one tick at once.
#[tauri::command]
pub async fn studio_queue(repo: Option<String>, run: bool) -> Res<Vec<StudioQueueRow>> {
    let _ = (repo, run);
    Err("not in this build yet".into())
}

/// Set (None: remove) what this machine allows `repo`'s studio.
#[tauri::command]
pub async fn studio_set(repo: String, studio: Option<StudioSettings>) -> Res<()> {
    let _ = (repo, studio);
    Err("not in this build yet".into())
}
