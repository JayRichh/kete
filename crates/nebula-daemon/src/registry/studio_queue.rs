//! STUDIO QUEUE (K2): the runner that hands an idle orchestrator a short
//! `[from kete studio]` note naming new inbox notes and account headroom,
//! and the `SetStudio` and `StudioQueue` requests.
//!
//! Placeholder from the 0.60 foundation lane. The queue lane owns this
//! file: it fills [`QueueBook`], [`tick`] and the methods below; their
//! signatures are fixed (the foundation wires the call sites).

use super::*;
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::StudioSettings;

/// The runner's state (announced notes, the last idle note), on
/// `Daemon::studio_queue`. The queue lane adds its fields.
#[derive(Debug, Default)]
pub(crate) struct QueueBook {}

/// One pass of the runner, from the daemon's 30 s tick (`lib.rs`).
pub fn tick(daemon: &Arc<Daemon>) {
    let _ = daemon;
}

impl Daemon {
    /// `SetStudio`: write (None: remove) `project`'s entry under `studio`
    /// in `config.local.json`. The server refuses it from inside an agent
    /// session before this runs.
    pub fn studio_set(
        &self,
        project: &ProjectId,
        studio: Option<StudioSettings>,
    ) -> std::result::Result<(), String> {
        let _ = (project, studio);
        Err("not in this build yet".into())
    }

    /// `StudioQueue`: what the runner would do now for `project` (or every
    /// studio repo); with `run`, one tick at once.
    pub fn studio_queue(
        self: &Arc<Self>,
        project: Option<&ProjectId>,
        run: bool,
    ) -> std::result::Result<Vec<StudioQueueRow>, String> {
        let _ = (project, run);
        Ok(Vec::new())
    }
}
