//! `kete studio note` (K6): file one inbox note and hand it to the triage
//! desk.
//!
//! Placeholder from the 0.60 foundation lane: it says it is not in this
//! build yet and exits 1. The triage lane owns this file; [`NoteArgs`] is
//! what `main.rs` hands it.

use anyhow::Result;

/// Everything `kete studio note` takes.
// Read by the owning lane once it fills this file.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NoteArgs {
    /// The note, its words joined.
    pub text: String,
    pub project: Option<String>,
    pub to: Option<String>,
    pub about: Option<String>,
    /// `high`, `normal` or `low`.
    pub priority: Option<String>,
    /// Send it to the triage desk to route.
    pub triage: bool,
    pub json: bool,
}

pub fn run(args: NoteArgs) -> Result<()> {
    let _ = args;
    crate::not_in_this_build("kete studio note")
}
