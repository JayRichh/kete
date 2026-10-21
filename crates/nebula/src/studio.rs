//! `kete studio` (K2): turn a repo's studio mode on and off, set its
//! autonomy and exclusions, and see what the queue runner would send.
//!
//! Placeholder from the 0.60 foundation lane: every subcommand says it is
//! not in this build yet and exits 1. The queue lane owns this file
//! (`kete studio note` is the triage lane's `studio_note.rs`).

use crate::cli::StudioCommand;
use anyhow::Result;

pub fn run(command: StudioCommand) -> Result<()> {
    let what = match command {
        StudioCommand::Enable { .. } => "kete studio enable",
        StudioCommand::Disable { .. } => "kete studio disable",
        StudioCommand::Status { .. } => "kete studio status",
        StudioCommand::Autonomy { .. } => "kete studio autonomy",
        StudioCommand::Exclude { .. } => "kete studio exclude",
        StudioCommand::Include { .. } => "kete studio include",
        StudioCommand::Queue { .. } => "kete studio queue",
        StudioCommand::Note { .. } => "kete studio note",
    };
    crate::not_in_this_build(what)
}
