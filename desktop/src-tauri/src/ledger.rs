//! The usage ledger in the window (K7): its rows, and the CSV export.
//!
//! Placeholder from the 0.60 foundation lane: each command answers "not in
//! this build yet". The ledger lane owns this file.

use crate::Res;
use nebula_core::studio::wire::LedgerRow;

/// The ledger's rows from `since` to `until` (local dates, inclusive),
/// for `repo` (a project id) or all.
#[tauri::command]
pub async fn usage_ledger(
    since: Option<String>,
    until: Option<String>,
    repo: Option<String>,
) -> Res<Vec<LedgerRow>> {
    let _ = (since, until, repo);
    Err("not in this build yet".into())
}

/// Write the ledger to `<data dir>/exports/usage-<date>.csv`; its path.
#[tauri::command]
pub async fn usage_export() -> Res<String> {
    Err("not in this build yet".into())
}
