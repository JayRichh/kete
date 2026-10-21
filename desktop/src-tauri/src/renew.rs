//! Orchestrator renewal from the window (K9).
//!
//! The form's submit: one `RenewOrchestrator` request, answered by the
//! daemon's `Scheduled` (or its refusal). The swap itself, and the `Done`
//! or `Failed` that ends it, reach the window as the `orchestrator-renewal`
//! event (main.rs).

use crate::daemon::Client;
use crate::files::OpenedFiles;
use crate::{err, Res};
use nebula_core::studio::wire::RenewState;
use nebula_core::{ClientRequest, ServerEvent};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// The note as the daemon is asked for it: an absolute path. The daemon
/// runs elsewhere, so a relative one would mean its folder, not the window's.
fn note_path(note: &str) -> Res<PathBuf> {
    let note = note.trim();
    if note.is_empty() {
        return Err("Name the handover note first.".into());
    }
    let path = PathBuf::from(note);
    if !path.is_absolute() {
        return Err("The note path must be absolute, from the drive or root down.".into());
    }
    Ok(path)
}

/// Renew the orchestrator from the handover note at `note`.
#[tauri::command]
pub async fn orchestrator_renew(
    client: State<'_, Arc<Client>>,
    opened: State<'_, OpenedFiles>,
    note: String,
    profile: Option<String>,
    keep_old: bool,
    now: bool,
) -> Res<RenewState> {
    let note = note_path(&note)?;
    let profile = profile
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let req_id = client.req_id();
    let reply = client
        .rpc_reply(
            req_id,
            ClientRequest::RenewOrchestrator {
                req_id,
                caller: None,
                note: note.clone(),
                profile,
                model: None,
                keep_old,
                now,
            },
        )
        .await
        .map_err(err)?;
    let ServerEvent::OrchestratorRenewal { state, .. } = reply else {
        return Err("the daemon answered the renewal with something else".into());
    };
    // The daemon took the note: the notice's Open note may open it, which
    // is otherwise only for files a session opened.
    if !matches!(state, RenewState::Failed { .. }) {
        opened.allow(&[note]);
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_is_an_absolute_path() {
        let abs = std::env::temp_dir().join("handover.md");
        let said = format!("  {}  ", abs.display());
        assert_eq!(note_path(&said).unwrap(), abs);
        assert!(note_path("").unwrap_err().contains("Name"));
        assert!(note_path("   ").unwrap_err().contains("Name"));
        assert!(note_path("notes/handover.md")
            .unwrap_err()
            .contains("absolute"));
    }
}
