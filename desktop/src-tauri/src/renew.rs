//! Orchestrator renewal from the window (K9).
//!
//! Placeholder from the 0.60 foundation lane: the command answers "not in
//! this build yet". The renew lane owns this file.

use crate::Res;
use nebula_core::studio::wire::RenewState;

/// Renew the orchestrator from the handover note at `note`.
#[tauri::command]
pub async fn orchestrator_renew(
    note: String,
    profile: Option<String>,
    keep_old: bool,
    now: bool,
) -> Res<RenewState> {
    let _ = (note, profile, keep_old, now);
    Err("not in this build yet".into())
}
