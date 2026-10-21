//! USAGE LEDGER (K7): tokens, estimated cost, tool uses and turns per
//! session per local day, added at every turn end.
//!
//! Tokens come from the session's transcript (and its subagents'),
//! read incrementally and counted once per message id
//! (`nebula_core::usage_ledger::TranscriptTally`). The cost is the change
//! in the statusLine's `cost.total_cost_usd` since it was last counted.
//! Tool uses are the PostToolUse hooks of the turn.
//!
//! The statusLine runs about 300 ms after the last message, and Claude
//! writes its transcript asynchronously, so a turn's last reading can land
//! after its `Stop`. A reading that lands while the session is not running
//! is added on its own (no turn), so the day's total misses nothing.

use super::*;
use crate::store::SpendDelta;
use nebula_core::studio::wire::LedgerRow;
use nebula_core::usage::StatuslineReading;
use nebula_core::usage_ledger::{self as ledger, TranscriptTally};

/// Per-session ledger state (transcript offsets, the last cost, tool uses
/// this turn), on `Daemon::ledger`.
#[derive(Debug)]
pub(crate) struct LedgerBook {
    /// When this daemon started, epoch ms. A session older than that is
    /// counted from here on: whatever came before was counted by the
    /// daemon that saw it, or is history.
    started_ms: i64,
    sessions: HashMap<AgentId, SessionBook>,
}

impl Default for LedgerBook {
    fn default() -> Self {
        Self {
            started_ms: nebula_core::clock::now_ms(),
            sessions: HashMap::new(),
        }
    }
}

#[derive(Debug, Default)]
struct SessionBook {
    tally: Option<TranscriptTally>,
    /// The transcript the statusLine names.
    transcript: Option<PathBuf>,
    /// The latest `cost.total_cost_usd`.
    cost_now: Option<f64>,
    /// The `cost.total_cost_usd` already in the ledger.
    cost_counted: Option<f64>,
    /// PostToolUse hooks since the last turn end.
    tool_uses: u32,
}

impl LedgerBook {
    /// Epoch ms before which `agent`'s transcript lines are not counted:
    /// its creation (its id is a ULID) or this daemon's start, the later.
    fn since(&self, agent: &AgentId) -> Option<i64> {
        ledger::created_ms(agent.as_str()).map(|created| created.max(self.started_ms))
    }

    /// Whether `agent` was created before this daemon started.
    fn predates(&self, agent: &AgentId) -> bool {
        ledger::created_ms(agent.as_str()).is_some_and(|created| created < self.started_ms)
    }
}

impl Daemon {
    /// A Claude statusLine landed for `agent` (`registry/usage.rs`).
    pub fn ledger_on_statusline(
        &self,
        agent: &AgentId,
        reading: &StatuslineReading,
        raw: &serde_json::Value,
    ) {
        let cost = raw
            .pointer("/cost/total_cost_usd")
            .and_then(serde_json::Value::as_f64);
        let uncounted = {
            let mut book = self.ledger.lock();
            let predates = book.predates(agent);
            let s = book.sessions.entry(agent.clone()).or_default();
            if let Some(path) = reading.transcript_path.as_deref().filter(|p| !p.is_empty()) {
                s.transcript = Some(PathBuf::from(path));
            }
            let Some(cost) = cost else {
                return;
            };
            // A session this daemon first hears of mid-life: its estimate so
            // far was counted before (or is history).
            if s.cost_now.is_none() && s.cost_counted.is_none() && predates {
                s.cost_counted = Some(cost);
            }
            s.cost_now = Some(cost);
            s.cost_counted != Some(cost)
        };
        let idle = matches!(
            self.store.get_agent(agent),
            Ok(Some(row)) if row.status != AgentStatus::Running
        );
        if uncounted && idle {
            self.ledger_record(agent, false, nebula_core::clock::now_ms());
        }
    }

    /// A PostToolUse hook landed for `agent` (`registry/status.rs`).
    pub fn ledger_on_tool_use(&self, agent: &AgentId) {
        let mut book = self.ledger.lock();
        let s = book.sessions.entry(agent.clone()).or_default();
        s.tool_uses = s.tool_uses.saturating_add(1);
    }

    /// `agent`'s turn ended (`Finished`, `registry/status.rs`).
    pub fn ledger_on_turn_end(&self, agent: &AgentId) {
        self.ledger_record(agent, true, nebula_core::clock::now_ms());
    }

    /// Count what `agent` spent since the last count, at `now_ms`: add it
    /// to the day's row and push the row to clients. A turn end counts one
    /// turn and the turn's tool uses; a late reading counts tokens and
    /// cost only, and nothing when it adds nothing. With `usage_ledger`
    /// off the state still moves on, so turning it back on never counts
    /// what was spent while it was off.
    pub(crate) fn ledger_record(&self, agent: &AgentId, turn_end: bool, now_ms: i64) {
        // Claude sessions only; a wake-up session has no row.
        let Ok(Some(row)) = self.store.get_agent(agent) else {
            return;
        };
        if row.kind != AgentKind::Claude {
            return;
        }
        let hooked = self
            .transcripts
            .lock()
            .get(agent)
            .map(|t| t.transcript_path.clone());
        let on = crate::config::Config::load_quiet().usage_ledger();
        let (tokens, cost, tool_uses) = {
            let mut book = self.ledger.lock();
            let since = book.since(agent);
            let s = book.sessions.entry(agent.clone()).or_default();
            let path = hooked.or_else(|| s.transcript.clone());
            let tally = s.tally.get_or_insert_with(|| TranscriptTally::new(since));
            let tokens = path.map(|p| tally.read_new(&p)).unwrap_or_default();
            let cost = s
                .cost_now
                .map_or(0.0, |now| ledger::cost_delta(s.cost_counted, now));
            if s.cost_now.is_some() {
                s.cost_counted = s.cost_now;
            }
            let tool_uses = if turn_end {
                std::mem::take(&mut s.tool_uses)
            } else {
                0
            };
            (tokens, cost, tool_uses)
        };
        if !on || (!turn_end && tokens.is_zero() && cost == 0.0) {
            return;
        }
        let worktree = self.store.get_worktree(&row.worktree_id).ok().flatten();
        let project = worktree
            .as_ref()
            .and_then(|w| self.store.get_project(&w.project_id).ok().flatten());
        let delta = SpendDelta {
            agent: agent.clone(),
            day: ledger::local_day(now_ms.div_euclid(1000)),
            project: project.map(|p| p.name),
            branch: worktree.map(|w| w.branch),
            session: Some(row.name),
            model: row.model,
            profile: row.profile,
            role_card: self.role_card_of(agent),
            input_tokens: tokens.input_tokens,
            output_tokens: tokens.output_tokens,
            cache_read_tokens: tokens.cache_read_tokens,
            cache_write_tokens: tokens.cache_write_tokens,
            cost_usd: cost,
            tool_uses,
            turns: u32::from(turn_end),
            at: now_ms,
        };
        match self.store.add_spend(&delta) {
            Ok(spend) => self.broadcast(ServerEvent::SessionSpend { rows: vec![spend] }),
            Err(e) => tracing::warn!(agent = %agent, error = %e, "usage ledger row not added"),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tests::EnvGuard;
    use crate::session_title::TranscriptRef;

    /// A daemon on a fresh store holding one session, `a`, of `kind`,
    /// whose transcript is `<dir>/sid.jsonl`, with `usage_ledger` as given.
    fn daemon_with_session(
        dir: &Path,
        ledger_on: bool,
        kind: AgentKind,
    ) -> (Arc<Daemon>, PathBuf, EnvGuard) {
        let config = dir.join("config.json");
        std::fs::write(
            &config,
            serde_json::json!({ "usage_ledger": ledger_on }).to_string(),
        )
        .unwrap();
        let guard = EnvGuard::set_all(&[
            (env::DATA_DIR, dir.to_str().unwrap()),
            (env::CONFIG_FILE, config.to_str().unwrap()),
        ]);
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "demo".into(),
                repo_path: dir.to_path_buf(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: dir.to_path_buf(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: AgentId("a".into()),
                worktree_id: WorktreeId("w".into()),
                name: "fix login".into(),
                status: AgentStatus::Running,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: true,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            })
            .unwrap();
        let transcript = dir.join("sid.jsonl");
        daemon.transcripts.lock().insert(
            AgentId("a".into()),
            TranscriptRef::from_payload(transcript.to_str(), Some("sid")).unwrap(),
        );
        (daemon, transcript, guard)
    }

    /// One turn's traffic: an assistant message in the transcript, `tools`
    /// tool uses, and a statusLine whose estimate is `cost`.
    fn turn(daemon: &Daemon, transcript: &Path, id: &str, input: u64, cost: f64, tools: u32) {
        let line = serde_json::json!({
            "type": "assistant",
            "message": { "id": id, "usage": { "input_tokens": input, "output_tokens": input / 10 } },
        });
        let mut text = std::fs::read_to_string(transcript).unwrap_or_default();
        text.push_str(&format!("{line}\n"));
        std::fs::write(transcript, text).unwrap();
        let a = AgentId("a".into());
        for _ in 0..tools {
            daemon.ledger_on_tool_use(&a);
        }
        let raw = serde_json::json!({ "cost": { "total_cost_usd": cost } });
        daemon.ledger_on_statusline(&a, &nebula_core::usage::claude_statusline(&raw), &raw);
    }

    fn rows(daemon: &Daemon) -> Vec<LedgerRow> {
        daemon.usage_ledger(None, None, None).unwrap()
    }

    #[test]
    fn two_turns_sum_into_one_row_and_tool_uses_reset_each_turn() {
        let dir = tempfile::tempdir().unwrap();
        let (daemon, transcript, _env) = daemon_with_session(dir.path(), true, AgentKind::Claude);
        let mut events = daemon.events.subscribe();
        let a = AgentId("a".into());
        turn(&daemon, &transcript, "msg_1", 1000, 0.05, 3);
        daemon.ledger_on_turn_end(&a);
        let pushed = std::iter::from_fn(|| events.try_recv().ok()).find_map(|e| match e {
            ServerEvent::SessionSpend { rows } => Some(rows),
            _ => None,
        });
        assert_eq!(
            pushed.map(|r| (r[0].turns, r[0].tool_uses)),
            Some((1, 3)),
            "a turn end pushes SessionSpend"
        );
        turn(&daemon, &transcript, "msg_2", 500, 0.08, 2);
        daemon.ledger_on_turn_end(&a);
        let rows = rows(&daemon);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let s = &rows[0].spend;
        assert_eq!((s.input_tokens, s.output_tokens), (1500, 150));
        assert!((s.cost_usd - 0.08).abs() < 1e-9, "{}", s.cost_usd);
        assert_eq!((s.tool_uses, s.turns), (5, 2));
        assert_eq!(rows[0].project, "demo");
        assert_eq!(rows[0].session, "fix login");
        assert_eq!(rows[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn a_turn_across_local_midnight_lands_on_the_new_day() {
        let dir = tempfile::tempdir().unwrap();
        let (daemon, transcript, _env) = daemon_with_session(dir.path(), true, AgentKind::Claude);
        let a = AgentId("a".into());
        let t = nebula_core::clock::now_secs() as i64;
        let off = nebula_core::clock::local_offset_secs_at(t) as i64;
        let midnight = t - (t + off).rem_euclid(86_400) + 86_400;
        turn(&daemon, &transcript, "msg_1", 10, 0.01, 0);
        daemon.ledger_record(&a, true, (midnight - 60) * 1000);
        turn(&daemon, &transcript, "msg_2", 20, 0.03, 0);
        daemon.ledger_record(&a, true, (midnight + 60) * 1000);
        let days: Vec<(String, u64)> = rows(&daemon)
            .into_iter()
            .map(|r| (r.spend.day, r.spend.input_tokens))
            .collect();
        assert_eq!(
            days,
            [
                (ledger::local_day(midnight - 60), 10),
                (ledger::local_day(midnight + 60), 20)
            ]
        );
        assert_ne!(days[0].0, days[1].0);
    }

    #[test]
    fn with_the_ledger_off_nothing_is_recorded_or_counted_later() {
        let dir = tempfile::tempdir().unwrap();
        let (daemon, transcript, _env) = daemon_with_session(dir.path(), false, AgentKind::Claude);
        let a = AgentId("a".into());
        turn(&daemon, &transcript, "msg_1", 1000, 0.05, 4);
        daemon.ledger_on_turn_end(&a);
        assert!(rows(&daemon).is_empty());
        // Back on: only what the next turn spends counts.
        std::fs::write(
            dir.path().join("config.json"),
            serde_json::json!({ "usage_ledger": true }).to_string(),
        )
        .unwrap();
        turn(&daemon, &transcript, "msg_2", 500, 0.08, 1);
        daemon.ledger_on_turn_end(&a);
        let s = &rows(&daemon)[0].spend;
        assert_eq!((s.input_tokens, s.tool_uses, s.turns), (500, 1, 1));
        assert!((s.cost_usd - 0.03).abs() < 1e-9, "{}", s.cost_usd);
    }

    #[test]
    fn a_reading_after_the_turn_ended_is_counted_without_a_turn() {
        let dir = tempfile::tempdir().unwrap();
        let (daemon, transcript, _env) = daemon_with_session(dir.path(), true, AgentKind::Claude);
        let a = AgentId("a".into());
        turn(&daemon, &transcript, "msg_1", 100, 0.05, 0);
        daemon.ledger_on_turn_end(&a);
        daemon
            .store
            .set_agent_status(&a, AgentStatus::Finished)
            .unwrap();
        // The turn's last statusLine lands after its Stop.
        turn(&daemon, &transcript, "msg_2", 40, 0.07, 0);
        let s = &rows(&daemon)[0].spend;
        assert_eq!((s.input_tokens, s.turns), (140, 1));
        assert!((s.cost_usd - 0.07).abs() < 1e-9, "{}", s.cost_usd);
    }

    #[test]
    fn other_harnesses_are_not_in_the_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let (daemon, transcript, _env) = daemon_with_session(dir.path(), true, AgentKind::Codex);
        turn(&daemon, &transcript, "msg_1", 100, 0.05, 1);
        daemon.ledger_on_turn_end(&AgentId("a".into()));
        assert!(rows(&daemon).is_empty());
    }
}
