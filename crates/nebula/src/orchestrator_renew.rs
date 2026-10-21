//! `kete orchestrator renew --from <note>` (K9): hand the orchestrator over
//! to a fresh session that starts on a handover note.
//!
//! Placeholder from the 0.60 foundation lane: it says it is not in this
//! build yet and exits 1. The renew lane owns this file; [`RenewArgs`] is
//! what `main.rs` hands it.

use anyhow::Result;
use std::path::PathBuf;

/// Everything `kete orchestrator renew` takes.
// Read by the owning lane once it fills this file.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RenewArgs {
    /// The handover note.
    pub from: PathBuf,
    pub profile: Option<String>,
    /// Let kete pick the account (the automation pick).
    pub pick: bool,
    pub model: Option<String>,
    pub keep_old: bool,
    pub now: bool,
    pub json: bool,
}

pub fn run(args: RenewArgs) -> Result<()> {
    let _ = args;
    crate::not_in_this_build("kete orchestrator renew")
}
