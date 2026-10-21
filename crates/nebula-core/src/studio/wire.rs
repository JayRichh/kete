//! The structs protocol 66 carries for studio mode (`STUDIO_PROTOCOL`).
//!
//! The codec is positional, so these are new structs that only a 66 peer
//! ever receives (`ServerEvent::decodable_by`). A field added to one later
//! goes last, behind a protocol bump of its own.

use crate::entities::AgentStatus;
use crate::ids::{AgentId, ProjectId};
use crate::studio::autonomy::Autonomy;
use serde::{Deserialize, Serialize};

/// One studio repo as the queue runner sees it (`StudioQueueState`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudioQueueRow {
    pub project: ProjectId,
    /// The project's name.
    pub name: String,
    pub autonomy: Autonomy,
    /// The inbox folder, repo-relative.
    pub inbox: String,
    /// Notes whose status is new or open.
    pub open: u32,
    /// Open notes addressed to Jay.
    pub to_jay: u32,
    /// Eligible notes not yet announced at their current mtime.
    pub unannounced: u32,
    /// What the next idle note would list, highest priority first.
    pub next: Vec<QueueNote>,
    /// Why the runner is holding, in a line; None when it would send now.
    pub held: Option<String>,
    /// When the last idle note went out, epoch ms.
    pub last_note_at: Option<i64>,
}

/// One inbox note in a queue row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueNote {
    pub id: String,
    pub title: String,
    pub to: String,
    /// `high`, `normal` or `low`.
    pub priority: String,
    /// Repo-relative path.
    pub path: String,
}

/// Where an orchestrator renewal stands (`OrchestratorRenewal`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenewState {
    /// Accepted: the swap waits for the old session's turn to end.
    Scheduled,
    /// The new orchestrator holds the role.
    Done,
    /// Nothing changed hands; `reason` says why.
    Failed { reason: String },
}

/// A session launched with a role card (`SessionRoles`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRole {
    pub agent: AgentId,
    /// The role card's id in `.kete/roles.toml`.
    pub card: String,
    /// What its chip says.
    pub label: String,
    /// 0 to 3.
    pub tier: u8,
    /// The board column it belongs to, as a department slug.
    pub department: Option<String>,
    /// `prompt`, `allowlist` or `unattended`.
    pub permission: String,
    /// Tokens; the board warns at 60 and 90 percent of it.
    pub context_budget: Option<u64>,
}

/// One session's spend on one local day (`SessionSpend`, and the ledger's
/// rows).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendRow {
    pub agent: AgentId,
    /// Local date, `YYYY-MM-DD`.
    pub day: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Claude Code's list-price estimate, not a bill.
    pub cost_usd: f64,
    pub tool_uses: u32,
    pub turns: u32,
}

/// One row of `kete usage` (`UsageLedgerRows`): the day's spend and what
/// the session was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRow {
    pub spend: SpendRow,
    /// The session's name.
    pub session: String,
    /// The project's name.
    pub project: String,
    pub branch: Option<String>,
    pub role_card: Option<String>,
    pub profile: Option<String>,
    pub model: Option<String>,
    pub status: AgentStatus,
    pub pr_url: Option<String>,
}
