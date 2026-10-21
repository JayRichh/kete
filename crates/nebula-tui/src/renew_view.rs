//! ORCHESTRATOR RENEWAL in the TUI (K9): the prompt for the handover
//! note, and what the view does when a renewal lands.
//!
//! Placeholder from the 0.60 foundation lane. The renew lane owns this
//! file; the signatures below are fixed (the verb dispatch, the
//! `PromptKind::RenewNote` submit and the `OrchestratorRenewal` arm call
//! them).

use crate::app::App;
use nebula_core::studio::wire::RenewState;
use nebula_core::AgentId;

/// Open the renew prompt (the `renew_orchestrator` verb).
pub fn open(app: &mut App) {
    app.chrome.flash = Some("Renew orchestrator: not in this build yet".into());
    app.chrome.dirty = true;
}

/// The renew prompt's Enter, with the note path typed.
pub fn submit(app: &mut App, text: String) {
    let _ = text;
    app.chrome.flash = Some("Renew orchestrator: not in this build yet".into());
    app.chrome.dirty = true;
}

/// `ServerEvent::OrchestratorRenewal`: `Scheduled` answering this
/// client's request, then `Done` or `Failed` for everyone.
pub fn on_renewal(
    app: &mut App,
    req_id: Option<u64>,
    old: AgentId,
    new: Option<AgentId>,
    state: RenewState,
) {
    let _ = (app, req_id, old, new, state);
}
