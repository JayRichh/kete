//! `kete usage` (K7): the usage ledger as a table or CSV.
//!
//! Placeholder from the 0.60 foundation lane: it says it is not in this
//! build yet and exits 1. The ledger lane owns this file; [`UsageArgs`]
//! is what `main.rs` hands it.

use anyhow::Result;

/// Everything `kete usage` takes.
// Read by the owning lane once it fills this file.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct UsageArgs {
    /// First local date, `YYYY-MM-DD`.
    pub since: Option<String>,
    /// Last local date, `YYYY-MM-DD`.
    pub until: Option<String>,
    pub project: Option<String>,
    /// `session` (default), `day` or `role`.
    pub by: Option<String>,
    pub csv: bool,
}

pub fn run(args: UsageArgs) -> Result<()> {
    let _ = args;
    crate::not_in_this_build("kete usage")
}
