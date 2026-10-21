//! ORCHESTRATOR RENEWAL (K9): a fresh orchestrator started from a handover
//! note once the current one's turn ends, holding the role, the old one
//! archived.
//!
//! Placeholder from the 0.60 foundation lane. The renew lane owns this
//! file: it fills [`RenewBook`] and [`Daemon::renew_orchestrator`]; the
//! signature is fixed (the server's `RenewOrchestrator` arm calls it).

use std::path::PathBuf;
use std::sync::Arc;

use nebula_core::studio::wire::RenewState;
use nebula_core::AgentId;

use crate::registry::Daemon;

/// What `RenewOrchestrator` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewRequest {
    /// The orchestrator renewing itself; None for a person (the server
    /// refuses that from inside an agent session).
    pub caller: Option<AgentId>,
    /// The handover note, the new orchestrator's first prompt.
    pub note: PathBuf,
    /// The account the new one runs on; None keeps the old one's.
    pub profile: Option<String>,
    /// The model the new one runs; None keeps the old one's.
    pub model: Option<String>,
    /// Leave the old session running as a worker.
    pub keep_old: bool,
    /// Swap at once instead of at the old session's turn end.
    pub now: bool,
}

/// Renewals waiting on a turn end, on `Daemon::renewals`. The renew lane
/// adds its fields.
#[derive(Debug, Default)]
pub struct RenewBook {}

impl Daemon {
    /// Schedule a renewal; answered `Scheduled`, then `Done` or `Failed`
    /// is broadcast as `ServerEvent::OrchestratorRenewal`.
    pub fn renew_orchestrator(self: &Arc<Self>, req: RenewRequest) -> Result<RenewState, String> {
        let _ = req;
        Err("not in this build yet".into())
    }
}
