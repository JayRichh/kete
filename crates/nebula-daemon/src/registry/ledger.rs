//! USAGE LEDGER (K7): tokens, estimated cost, tool uses and turns per
//! session per local day, added at every turn end.
//!
//! Placeholder from the 0.60 foundation lane. The ledger lane owns this
//! file: it fills [`LedgerBook`] and the methods below; their signatures
//! are fixed (the foundation wires the call sites).

use super::*;
use nebula_core::studio::wire::LedgerRow;
use nebula_core::usage::StatuslineReading;

/// Per-session ledger state (transcript offsets, the last cost, tool uses
/// this turn), on `Daemon::ledger`. The ledger lane adds its fields.
#[derive(Debug, Default)]
pub(crate) struct LedgerBook {}

impl Daemon {
    /// A Claude statusLine landed for `agent` (`registry/usage.rs`).
    pub fn ledger_on_statusline(
        &self,
        agent: &AgentId,
        reading: &StatuslineReading,
        raw: &serde_json::Value,
    ) {
        let _ = (agent, reading, raw);
    }

    /// A PostToolUse hook landed for `agent` (`registry/status.rs`).
    pub fn ledger_on_tool_use(&self, agent: &AgentId) {
        let _ = agent;
    }

    /// `agent`'s turn ended (`Finished`, `registry/status.rs`).
    pub fn ledger_on_turn_end(&self, agent: &AgentId) {
        let _ = agent;
    }

    /// `UsageLedger`: the ledger's rows from `since` to `until` (local
    /// dates, inclusive), for one project or all.
    pub fn usage_ledger(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        project: Option<&ProjectId>,
    ) -> std::result::Result<Vec<LedgerRow>, String> {
        self.store
            .spend_rows(since, until, project)
            .map_err(|e| format!("{e:#}"))
    }
}
