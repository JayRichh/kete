//! STUDIO POLICY (K2): autonomy A0 refuses orchestrator and sibling spawns
//! into a studio repo.
//!
//! The level is read fresh from `config.local.json` on every spawn, so a
//! drop to A0 holds the next spawn at once. Each refusal adds a line to
//! `<data dir>/studio/refusals.jsonl`, which the board shows.

use super::*;
use nebula_core::studio::refusals::{self, Refusal};
use nebula_core::studio::{settings, Autonomy};

impl Daemon {
    /// Refuse a spawn into `project` while its autonomy is A0, with the
    /// reason ("autonomy A0 (Hold) for <repo>"). Called by the
    /// `OrchestratorSpawn`, `OrchestratorSpawnWith`, `SpawnSiblingAgent`
    /// and `SpawnSiblingAgentWith` handlers before anything starts. An
    /// unknown project passes: the spawn refuses it itself.
    pub fn studio_check_spawn(&self, project: &ProjectId) -> std::result::Result<(), String> {
        let Some(project) = self.store.get_project(project).ok().flatten() else {
            return Ok(());
        };
        let (local, log) = {
            let book = self.studio_queue.lock();
            (book.local_path(), book.refusals_path())
        };
        let held = settings::for_repo_in(&local, &project.repo_path)
            .is_some_and(|s| s.autonomy == Autonomy::A0);
        if !held {
            return Ok(());
        }
        let reason = format!("autonomy A0 (Hold) for {}", project.name);
        let refusal = Refusal {
            at: nebula_core::clock::now_ms(),
            project: settings::repo_key(&project.repo_path),
            agent: None,
            role: None,
            reason: reason.clone(),
        };
        if let Err(error) = refusals::append_in(&log, &refusal) {
            tracing::warn!(%error, "could not log a studio refusal");
        }
        Err(reason)
    }
}
