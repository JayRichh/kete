//! ROLE CARDS (K3, and K9's permission profiles): the launch overlay a role
//! session gets, its settings file and guard, and `SessionRoles`.
//!
//! Placeholder from the 0.60 foundation lane. The roles lane owns this
//! file: it fills [`RolesBook`] and the methods below; their signatures are
//! fixed (the foundation wires the call sites).

use super::*;
use crate::launch_overlay::LaunchOverlay;
use nebula_core::studio::wire::SessionRole;

/// Parsed role cards per project, keyed by the root worktree's HEAD, on
/// `Daemon::roles`. The roles lane adds its fields.
#[derive(Debug, Default)]
pub(crate) struct RolesBook {}

impl Daemon {
    /// What `agent`'s launch adds (flags, prompt, settings file), built on
    /// every spawn and resume (`registry/spawn.rs`). `Err` refuses the
    /// spawn with that reason. The default adds nothing.
    pub fn launch_overlay(
        &self,
        agent: &Agent,
        project: &Project,
    ) -> std::result::Result<LaunchOverlay, String> {
        let _ = (agent, project);
        Ok(LaunchOverlay::default())
    }

    /// Every live role session, for `ServerEvent::SessionRoles`.
    pub fn session_roles(&self) -> Vec<SessionRole> {
        Vec::new()
    }

    /// The role card `agent` launched with (migration 36).
    pub fn role_card_of(&self, agent: &AgentId) -> Option<String> {
        self.store
            .agent_role_cards()
            .ok()?
            .into_iter()
            .find(|(id, _)| id == agent)
            .map(|(_, card)| card)
    }
}
