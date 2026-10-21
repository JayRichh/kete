//! `kete roles` (K3): what each role card gives a session, and accepting
//! bypass mode for an account.
//!
//! Placeholder from the 0.60 foundation lane: every subcommand says it is
//! not in this build yet and exits 1. The roles lane owns this file.

use crate::cli::RolesCommand;
use anyhow::Result;

pub fn run(command: RolesCommand) -> Result<()> {
    let what = match command {
        RolesCommand::Check { .. } => "kete roles check",
        RolesCommand::AcceptBypass { .. } => "kete roles accept-bypass",
    };
    crate::not_in_this_build(what)
}
