//! STUDIO POLICY (K2): autonomy A0 refuses orchestrator and sibling spawns
//! into a studio repo.
//!
//! Placeholder from the 0.60 foundation lane. The queue lane owns this
//! file; the signature is fixed (the server's spawn handlers call it).

use super::*;

impl Daemon {
    /// Refuse a spawn into `project` while its autonomy is A0, with the
    /// reason ("autonomy A0 (Hold) for <repo>"). Called by the
    /// `OrchestratorSpawn`, `OrchestratorSpawnWith`, `SpawnSiblingAgent`
    /// and `SpawnSiblingAgentWith` handlers before anything starts.
    pub fn studio_check_spawn(&self, project: &ProjectId) -> std::result::Result<(), String> {
        let _ = project;
        Ok(())
    }
}
