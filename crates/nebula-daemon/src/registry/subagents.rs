//! Claude's in-session subagents (the Task / Agent tool), as the
//! SubagentStart and SubagentStop hooks report them, for clients that draw
//! a session's agent tree. Held in daemon memory and carried across an
//! IN-PLACE RESTART in the restart state (`handoff.rs`), so a subagent
//! still running there keeps running and its SubagentStop still lands. A
//! cold start begins empty: every session died with the old daemon.

use super::*;
use nebula_core::{SubagentStatus, SubagentSummary};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

/// Ended subagents kept per parent session: a long session's tree stays
/// bounded. Shared with clients, which prune the same way.
const ENDED_KEPT: usize = nebula_core::SUBAGENTS_ENDED_KEPT;
/// Task descriptions waiting for their subagent's SubagentStart, per
/// parent. More than this many unclaimed means the hooks have drifted.
const PENDING_TASKS_KEPT: usize = 16;
/// The tool a parent calls to start a subagent (`Task` in older Claude
/// Code, `Agent` since).
const SUBAGENT_TOOLS: [&str; 2] = ["Agent", "Task"];
/// A row still running this long after its SubagentStart is presumed
/// killed without a SubagentStop: the stop gate's own limit for one
/// tracked subagent (`status::SUBAGENT_TTL`), so the book never shows a
/// subagent running that the session's status has already given up on.
const STALE_RUNNING_MS: i64 = crate::status::SUBAGENT_TTL.as_millis() as i64;

/// What a hook POST says about subagents, beside its status event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentHookInfo {
    /// The payload's `agent_type` (`Explore`, `general-purpose`, ...).
    pub kind: Option<String>,
    /// The `description` of a Task / Agent tool call's input.
    pub task: Option<String>,
    /// The `subagent_type` of that input: the `agent_type` its
    /// SubagentStart will report.
    pub task_kind: Option<String>,
    /// The first line of that input's `prompt` ([`subagent_title`]): the
    /// row's name when the call gave no description, so two undescribed
    /// `general-purpose` agents do not read the same.
    pub task_prompt: Option<String>,
    /// A SubagentStop's `last_assistant_message`, first line: the name of
    /// a row that still has none when it ends.
    pub last_message: Option<String>,
    /// The descriptions of the `subagent` tasks the payload's
    /// `background_tasks` lists in flight; None without a list.
    pub background_subagents: Option<Vec<String>>,
}

/// The longest a name taken from a prompt or a reply runs.
const TITLE_MAX: usize = 80;

/// The first non-blank line of `text`, trimmed and cut to [`TITLE_MAX`]
/// characters: a subagent's name taken from its prompt or its reply.
pub fn subagent_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    if line.chars().count() <= TITLE_MAX {
        return Some(line.to_string());
    }
    let cut: String = line.chars().take(TITLE_MAX - 1).collect();
    Some(format!("{}\u{2026}", cut.trim_end()))
}

/// A Task / Agent call's description, waiting for its SubagentStart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTask {
    /// The `subagent_type` asked for; empty when the call named none.
    pub kind: String,
    pub description: String,
}

/// The book as the restart state carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentCarry {
    #[serde(default)]
    pub rows: Vec<SubagentSummary>,
    #[serde(default)]
    pub pending: Vec<(AgentId, PendingTask)>,
}

#[derive(Default)]
pub(crate) struct SubagentBook {
    rows: HashMap<AgentId, Vec<SubagentSummary>>,
    pending_tasks: HashMap<AgentId, VecDeque<PendingTask>>,
    /// Rows whose SubagentStop came while the parent's `background_tasks`
    /// still listed them: they wait on their own background work and stay
    /// running until a later list leaves them out.
    waiting: HashSet<(AgentId, String)>,
}

impl SubagentBook {
    fn note_task(&mut self, parent: &AgentId, task: PendingTask) {
        let queue = self.pending_tasks.entry(parent.clone()).or_default();
        queue.push_back(task);
        while queue.len() > PENDING_TASKS_KEPT {
            queue.pop_front();
        }
    }

    fn start(&mut self, parent: &AgentId, id: String, kind: String, now: i64) -> SubagentSummary {
        // The oldest unclaimed call that asked for this subagent's type is
        // its own; failing a match (a call that named no type, a CLI that
        // reports none), calls start in the order they were made.
        let description = self
            .pending_tasks
            .get_mut(parent)
            .and_then(|queue| {
                let typed = (!kind.is_empty())
                    .then(|| queue.iter().position(|t| t.kind == kind))
                    .flatten();
                match typed {
                    Some(at) => queue.remove(at),
                    None => queue.pop_front(),
                }
            })
            .map(|t| t.description)
            .unwrap_or_default();
        let row = SubagentSummary {
            id,
            parent_agent: parent.clone(),
            kind,
            description,
            started_at: now,
            ended_at: None,
            status: SubagentStatus::Running,
        };
        let rows = self.rows.entry(parent.clone()).or_default();
        rows.retain(|r| r.id != row.id);
        rows.push(row.clone());
        row
    }

    fn end(
        &mut self,
        parent: &AgentId,
        id: Option<&str>,
        status: SubagentStatus,
        now: i64,
    ) -> Vec<SubagentSummary> {
        let Some(rows) = self.rows.get_mut(parent) else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for row in rows.iter_mut() {
            let named = id.is_none_or(|id| row.id == id);
            if named && row.status == SubagentStatus::Running {
                row.status = status;
                row.ended_at = Some(now);
                changed.push(row.clone());
            }
        }
        let ended = rows
            .iter()
            .filter(|r| r.status != SubagentStatus::Running)
            .count();
        let mut drop = ended.saturating_sub(ENDED_KEPT);
        rows.retain(|r| {
            if drop > 0 && r.status != SubagentStatus::Running {
                drop -= 1;
                return false;
            }
            true
        });
        changed
    }

    fn forget(&mut self, parent: &AgentId) {
        self.rows.remove(parent);
        self.pending_tasks.remove(parent);
        self.waiting.retain(|(p, _)| p != parent);
    }

    /// Name row `id` of `parent` from `title` while it has no name yet.
    fn name_if_blank(&mut self, parent: &AgentId, id: &str, title: Option<&str>) {
        let Some(title) = title else { return };
        if let Some(row) = self
            .rows
            .get_mut(parent)
            .and_then(|rows| rows.iter_mut().find(|r| r.id == id))
        {
            if row.description.is_empty() {
                row.description = title.to_string();
            }
        }
    }

    /// A SubagentStop: done, unless the parent's `background_tasks` still
    /// lists the subagent (by its task), in which case it waits on its own
    /// background work and stays running.
    fn stop(
        &mut self,
        parent: &AgentId,
        id: &str,
        info: &SubagentHookInfo,
        now: i64,
    ) -> Vec<SubagentSummary> {
        self.name_if_blank(parent, id, info.last_message.as_deref());
        let row = self
            .rows
            .get(parent)
            .and_then(|rows| rows.iter().find(|r| r.id == id));
        let listed = row.is_some_and(|r| {
            !r.description.is_empty()
                && info
                    .background_subagents
                    .as_ref()
                    .is_some_and(|list| list.contains(&r.description))
        });
        if listed {
            let row = row.cloned();
            self.waiting.insert((parent.clone(), id.to_string()));
            return row.into_iter().collect();
        }
        self.waiting.remove(&(parent.clone(), id.to_string()));
        self.end(parent, Some(id), SubagentStatus::Done, now)
    }

    /// A later Stop or SubagentStop's list: a waiting row it no longer
    /// names has finished its background work.
    fn settle_waiting(
        &mut self,
        parent: &AgentId,
        listed: &[String],
        now: i64,
    ) -> Vec<SubagentSummary> {
        let done: Vec<String> = self
            .waiting
            .iter()
            .filter(|(p, _)| p == parent)
            .map(|(_, id)| id.clone())
            .filter(|id| {
                self.rows
                    .get(parent)
                    .and_then(|rows| rows.iter().find(|r| &r.id == id))
                    .is_none_or(|r| !listed.contains(&r.description))
            })
            .collect();
        let mut changed = Vec::new();
        for id in done {
            self.waiting.remove(&(parent.clone(), id.clone()));
            changed.extend(self.end(parent, Some(&id), SubagentStatus::Done, now));
        }
        changed
    }

    /// Take every row of `parent` out of the book, running or not, and
    /// say which ids went. A restarted session is a new process: nothing
    /// the old one ran can report again.
    fn reap(&mut self, parent: &AgentId) -> Vec<String> {
        self.pending_tasks.remove(parent);
        self.waiting.retain(|(p, _)| p != parent);
        self.rows
            .remove(parent)
            .map(|rows| rows.into_iter().map(|r| r.id).collect())
            .unwrap_or_default()
    }

    /// Drop the ended rows (done, failed, closed) that `gone` picks, of
    /// `parent` or of every session; running ones always stay. Returns
    /// what went, by parent.
    fn drop_ended(
        &mut self,
        parent: Option<&AgentId>,
        gone: impl Fn(&SubagentSummary) -> bool,
    ) -> Vec<(AgentId, Vec<String>)> {
        let mut removed = Vec::new();
        for (owner, rows) in self.rows.iter_mut() {
            if parent.is_some_and(|p| p != owner) {
                continue;
            }
            let mut ids = Vec::new();
            rows.retain(|r| {
                let drop = r.status != SubagentStatus::Running && gone(r);
                if drop {
                    ids.push(r.id.clone());
                }
                !drop
            });
            if !ids.is_empty() {
                removed.push((owner.clone(), ids));
            }
        }
        self.rows.retain(|_, rows| !rows.is_empty());
        removed.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));
        removed
    }

    /// The periodic sweep: a row still running after [`STALE_RUNNING_MS`]
    /// never got its SubagentStop (the stop gate gave up on it long ago,
    /// `status::SUBAGENT_TTL`), so it ends as unknown; an ended row older
    /// than `retention_ms` (None keeps them) leaves the book.
    fn sweep(
        &mut self,
        now: i64,
        retention_ms: Option<i64>,
    ) -> (Vec<SubagentSummary>, Vec<(AgentId, Vec<String>)>) {
        let mut closed = Vec::new();
        for row in self.rows.values_mut().flatten() {
            if row.status == SubagentStatus::Running && now - row.started_at >= STALE_RUNNING_MS {
                row.status = SubagentStatus::EndedUnknown;
                row.ended_at = Some(now);
                closed.push(row.clone());
            }
        }
        let removed = match retention_ms {
            Some(keep) => {
                self.drop_ended(None, |r| now - r.ended_at.unwrap_or(r.started_at) >= keep)
            }
            None => Vec::new(),
        };
        (closed, removed)
    }

    fn all(&self) -> Vec<SubagentSummary> {
        let mut all: Vec<_> = self.rows.values().flatten().cloned().collect();
        all.sort_by(|a, b| {
            (a.started_at, &a.parent_agent.0, &a.id).cmp(&(b.started_at, &b.parent_agent.0, &b.id))
        });
        all
    }
}

impl Daemon {
    /// Every subagent the daemon knows of, oldest start first: the
    /// Snapshot's `subagents`.
    pub fn subagent_rows(&self) -> Vec<SubagentSummary> {
        self.subagents.lock().all()
    }

    /// Feed one hook delivery's subagent news: a Task / Agent tool call's
    /// description, a SubagentStart, a SubagentStop. Everything else is
    /// not this book's business.
    pub fn track_subagent(&self, parent: &AgentId, event: &HookEvent, info: SubagentHookInfo) {
        let now = nebula_core::clock::now_ms();
        let changed = {
            let mut book = self.subagents.lock();
            match event {
                HookEvent::PreToolUse {
                    tool_name: Some(tool),
                    subagent_id: None,
                } if SUBAGENT_TOOLS.contains(&tool.as_str()) => {
                    book.note_task(
                        parent,
                        PendingTask {
                            kind: info.task_kind.unwrap_or_default(),
                            description: info
                                .task
                                .filter(|t| !t.trim().is_empty())
                                .or(info.task_prompt)
                                .unwrap_or_default(),
                        },
                    );
                    Vec::new()
                }
                HookEvent::SubagentStart {
                    subagent_id: Some(id),
                } => vec![book.start(parent, id.clone(), info.kind.unwrap_or_default(), now)],
                HookEvent::SubagentStop {
                    subagent_id: Some(id),
                    ..
                } => {
                    let mut changed = book.stop(parent, id, &info, now);
                    if let Some(list) = &info.background_subagents {
                        changed.extend(book.settle_waiting(parent, list, now));
                    }
                    changed
                }
                HookEvent::Stop { .. } => match &info.background_subagents {
                    Some(list) => book.settle_waiting(parent, list, now),
                    None => Vec::new(),
                },
                _ => Vec::new(),
            }
        };
        for sub in changed {
            self.broadcast(ServerEvent::SubagentChanged { sub });
        }
    }

    /// The parent session's process is gone: whatever it still had running
    /// can never report its end, so it ends as unknown.
    pub(crate) fn close_subagents(&self, parent: &AgentId) {
        let now = nebula_core::clock::now_ms();
        let changed = {
            let mut book = self.subagents.lock();
            book.pending_tasks.remove(parent);
            book.waiting.retain(|(p, _)| p != parent);
            book.end(parent, None, SubagentStatus::EndedUnknown, now)
        };
        for sub in changed {
            self.broadcast(ServerEvent::SubagentChanged { sub });
        }
    }

    /// The parent row is deleted: drop its subagents with it.
    pub(crate) fn forget_subagents(&self, parent: &AgentId) {
        self.subagents.lock().forget(parent);
    }

    /// The parent session is about to restart (restart, account switch, a
    /// failed resume's respawn): every row its old process left, running
    /// or ended, leaves the book now, before the new process can start
    /// subagents of its own. Without this the old rows stayed, the
    /// running ones closed only when the old PTY's exit landed, after the
    /// new session was up.
    pub(crate) fn reap_subagents(&self, parent: &AgentId) {
        let ids = self.subagents.lock().reap(parent);
        if !ids.is_empty() {
            self.broadcast(ServerEvent::SubagentsRemoved {
                parent: parent.clone(),
                ids,
            });
        }
    }

    /// `ClientRequest::ClearEndedSubagents`: drop the ended rows of
    /// `parent`, or of every session; running ones stay.
    pub fn clear_ended_subagents(&self, parent: Option<&AgentId>) {
        let removed = self.subagents.lock().drop_ended(parent, |_| true);
        self.broadcast_removed(removed);
    }

    /// The 30 s sweep: stale running rows end as unknown, and ended rows
    /// past the `subagent_retention` setting leave the book.
    pub fn sweep_subagents(&self) {
        let retention = crate::config::Config::load().subagent_retention_ms();
        self.sweep_subagents_at(nebula_core::clock::now_ms(), retention);
    }

    fn sweep_subagents_at(&self, now: i64, retention_ms: Option<i64>) {
        let (closed, removed) = self.subagents.lock().sweep(now, retention_ms);
        for sub in closed {
            self.broadcast(ServerEvent::SubagentChanged { sub });
        }
        self.broadcast_removed(removed);
    }

    fn broadcast_removed(&self, removed: Vec<(AgentId, Vec<String>)>) {
        for (parent, ids) in removed {
            self.broadcast(ServerEvent::SubagentsRemoved { parent, ids });
        }
    }

    /// The whole book, for an IN-PLACE RESTART to hand the next image.
    pub fn carry_subagents(&self) -> SubagentCarry {
        let book = self.subagents.lock();
        SubagentCarry {
            rows: book.all(),
            pending: book
                .pending_tasks
                .iter()
                .flat_map(|(parent, queue)| queue.iter().map(|t| (parent.clone(), t.clone())))
                .collect(),
        }
    }

    /// Take back what the old image carried. A subagent whose parent's
    /// session came across (`live`) keeps running, and its SubagentStop
    /// will find it. One whose parent did not, has lost the process it ran
    /// in, so it ends as unknown, as a parent's exit would have ended it.
    /// Called while the daemon boots, before any client subscribes: the
    /// first Snapshot carries the result, so nothing is broadcast.
    pub fn restore_subagents(&self, carry: SubagentCarry, live: &HashSet<AgentId>) {
        let now = nebula_core::clock::now_ms();
        let mut book = self.subagents.lock();
        for mut row in carry.rows {
            if row.status == SubagentStatus::Running && !live.contains(&row.parent_agent) {
                row.status = SubagentStatus::EndedUnknown;
                row.ended_at = Some(now);
            }
            let rows = book.rows.entry(row.parent_agent.clone()).or_default();
            rows.retain(|r| r.id != row.id);
            rows.push(row);
        }
        for (parent, task) in carry.pending {
            if live.contains(&parent) {
                book.note_task(&parent, task);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon() -> Arc<Daemon> {
        Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        )
    }

    fn changes(rx: &mut broadcast::Receiver<ServerEvent>) -> Vec<SubagentSummary> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                ServerEvent::SubagentChanged { sub } => Some(sub),
                _ => None,
            })
            .collect()
    }

    fn start(id: &str) -> HookEvent {
        HookEvent::SubagentStart {
            subagent_id: Some(id.into()),
        }
    }

    fn stop(id: &str) -> HookEvent {
        HookEvent::SubagentStop {
            subagent_id: Some(id.into()),
            background: None,
        }
    }

    fn explore() -> SubagentHookInfo {
        SubagentHookInfo {
            kind: Some("Explore".into()),
            task: None,
            task_kind: None,
            ..Default::default()
        }
    }

    #[test]
    fn a_start_then_a_stop_reads_running_then_done() {
        let daemon = daemon();
        let parent = AgentId("p".into());
        let mut rx = daemon.events.subscribe();

        // The parent's Agent tool call names the task before the start.
        daemon.track_subagent(
            &parent,
            &HookEvent::PreToolUse {
                tool_name: Some("Agent".into()),
                subagent_id: None,
            },
            SubagentHookInfo {
                kind: None,
                task: Some("Find the hook fixtures".into()),
                task_kind: Some("Explore".into()),
                ..Default::default()
            },
        );
        assert!(
            changes(&mut rx).is_empty(),
            "a tool call alone starts nothing"
        );

        daemon.track_subagent(&parent, &start("agent-1"), explore());
        let started = changes(&mut rx);
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].status, SubagentStatus::Running);
        assert_eq!(started[0].id, "agent-1");
        assert_eq!(started[0].parent_agent, parent);
        assert_eq!(started[0].kind, "Explore");
        assert_eq!(started[0].description, "Find the hook fixtures");
        assert_eq!(started[0].ended_at, None);
        assert_eq!(daemon.subagent_rows(), started);

        daemon.track_subagent(&parent, &stop("agent-1"), explore());
        let stopped = changes(&mut rx);
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].status, SubagentStatus::Done);
        assert!(stopped[0].ended_at.is_some());
        assert_eq!(daemon.subagent_rows(), stopped);

        // A stop for a subagent never seen starting changes nothing.
        daemon.track_subagent(&parent, &stop("ghost"), explore());
        assert!(changes(&mut rx).is_empty());
    }

    #[test]
    fn the_parent_exiting_ends_its_open_subagents_as_unknown() {
        let daemon = daemon();
        let parent = AgentId("p".into());
        let other = AgentId("o".into());
        daemon.track_subagent(&parent, &start("a"), explore());
        daemon.track_subagent(&parent, &start("b"), explore());
        daemon.track_subagent(&parent, &stop("b"), explore());
        daemon.track_subagent(&other, &start("c"), explore());
        let mut rx = daemon.events.subscribe();

        daemon.close_subagents(&parent);

        let closed = changes(&mut rx);
        assert_eq!(closed.len(), 1, "only the open one is closed");
        assert_eq!(closed[0].id, "a");
        assert_eq!(closed[0].status, SubagentStatus::EndedUnknown);
        assert!(closed[0].ended_at.is_some());
        let rows = daemon.subagent_rows();
        let status = |id: &str| rows.iter().find(|r| r.id == id).unwrap().status;
        assert_eq!(
            status("b"),
            SubagentStatus::Done,
            "a finished one stays done"
        );
        assert_eq!(
            status("c"),
            SubagentStatus::Running,
            "another session's are untouched"
        );

        daemon.forget_subagents(&parent);
        assert_eq!(daemon.subagent_rows().len(), 1);
    }

    #[test]
    fn a_long_session_keeps_only_its_newest_ended_subagents() {
        let daemon = daemon();
        let parent = AgentId("p".into());
        daemon.track_subagent(&parent, &start("open"), explore());
        for i in 0..ENDED_KEPT + 5 {
            let id = format!("s{i}");
            daemon.track_subagent(&parent, &start(&id), explore());
            daemon.track_subagent(&parent, &stop(&id), explore());
        }
        let rows = daemon.subagent_rows();
        assert_eq!(rows.len(), ENDED_KEPT + 1);
        assert!(rows.iter().any(|r| r.id == "open"), "running ones are kept");
        assert!(!rows.iter().any(|r| r.id == "s0"), "the oldest ended go");
    }

    fn removals(rx: &mut broadcast::Receiver<ServerEvent>) -> Vec<(AgentId, Vec<String>)> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                ServerEvent::SubagentsRemoved { parent, ids } => Some((parent, ids)),
                _ => None,
            })
            .collect()
    }

    /// The board's cleanup: ended rows go, of one session or of all,
    /// and running ones stay wherever they are.
    #[test]
    fn a_clear_drops_ended_rows_and_keeps_running_ones() {
        let daemon = daemon();
        let p = AgentId("p".into());
        let q = AgentId("q".into());
        daemon.track_subagent(&p, &start("live"), explore());
        daemon.track_subagent(&p, &start("done"), explore());
        daemon.track_subagent(&p, &stop("done"), explore());
        daemon.track_subagent(&q, &start("other"), explore());
        daemon.track_subagent(&q, &stop("other"), explore());
        let mut rx = daemon.events.subscribe();

        daemon.clear_ended_subagents(Some(&p));
        assert_eq!(
            removals(&mut rx),
            vec![(p.clone(), vec!["done".to_string()])]
        );
        let ids: Vec<String> = daemon.subagent_rows().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["live".to_string(), "other".to_string()]);

        daemon.clear_ended_subagents(None);
        assert_eq!(removals(&mut rx), vec![(q, vec!["other".to_string()])]);
        let ids: Vec<String> = daemon.subagent_rows().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["live".to_string()]);
    }

    /// A restart reaps everything the old process left, running rows
    /// too, and says so; the new process starts with an empty book.
    #[test]
    fn a_restart_reaps_every_row_of_its_session() {
        let daemon = daemon();
        let p = AgentId("p".into());
        daemon.track_subagent(&p, &start("a"), explore());
        daemon.track_subagent(&p, &start("b"), explore());
        daemon.track_subagent(&p, &stop("b"), explore());
        let mut rx = daemon.events.subscribe();
        daemon.reap_subagents(&p);
        let gone = removals(&mut rx);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].0, p);
        assert_eq!(gone[0].1.len(), 2);
        assert!(daemon.subagent_rows().is_empty());
        daemon.reap_subagents(&p);
        assert!(removals(&mut rx).is_empty(), "nothing left to say");
    }

    /// The sweep ends a row whose stop never came and drops ended rows
    /// past the retention; `off` keeps ended rows.
    #[test]
    fn the_sweep_closes_stale_rows_and_drops_old_ended_ones() {
        let daemon = daemon();
        let p = AgentId("p".into());
        daemon.track_subagent(&p, &start("old"), explore());
        daemon.track_subagent(&p, &start("fresh"), explore());
        daemon.track_subagent(&p, &stop("fresh"), explore());
        let started = daemon.subagent_rows()[0].started_at;
        let mut rx = daemon.events.subscribe();

        // An hour on: nothing stale, the ended row kept with retention off.
        daemon.sweep_subagents_at(started + 3_600_000, None);
        assert!(changes(&mut rx).is_empty());
        assert!(removals(&mut rx).is_empty());

        // Past the stop gate's limit, retention one hour: the stale row
        // closes as unknown and the finished one goes.
        daemon.sweep_subagents_at(started + STALE_RUNNING_MS, Some(3_600_000));
        let events: Vec<ServerEvent> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let closed: Vec<&SubagentSummary> = events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::SubagentChanged { sub } => Some(sub),
                _ => None,
            })
            .collect();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, "old");
        assert_eq!(closed[0].status, SubagentStatus::EndedUnknown);
        let removed: Vec<(&AgentId, &Vec<String>)> = events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::SubagentsRemoved { parent, ids } => Some((parent, ids)),
                _ => None,
            })
            .collect();
        assert_eq!(removed, vec![(&p, &vec!["fresh".to_string()])]);

        // The closed row ages out on a later sweep like any ended one.
        daemon.sweep_subagents_at(started + STALE_RUNNING_MS + 3_600_000, Some(3_600_000));
        assert_eq!(removals(&mut rx), vec![(p, vec!["old".to_string()])]);
        assert!(daemon.subagent_rows().is_empty());
    }

    /// A subagent whose SubagentStop lands while the parent's
    /// `background_tasks` still lists it waits on its own background work:
    /// it stays running, and a later list that leaves it out ends it done.
    #[test]
    fn a_subagent_waiting_on_its_background_work_stays_running() {
        let daemon = daemon();
        let p = AgentId("p".into());
        let (event, info) = launch(Some("general-purpose"), "Rename the server");
        daemon.track_subagent(&p, &event, info);
        daemon.track_subagent(&p, &start("r"), typed("general-purpose"));
        let listed = |names: &[&str]| SubagentHookInfo {
            background_subagents: Some(names.iter().map(|n| n.to_string()).collect()),
            ..Default::default()
        };
        daemon.track_subagent(&p, &stop("r"), listed(&["Rename the server"]));
        let status = |d: &Daemon| d.subagent_rows()[0].status;
        assert_eq!(
            status(&daemon),
            SubagentStatus::Running,
            "waiting, not done"
        );

        // The parent's next Stop no longer lists it: its work is over.
        daemon.track_subagent(&p, &HookEvent::Stop { background: None }, listed(&[]));
        assert_eq!(status(&daemon), SubagentStatus::Done);
    }

    /// A call with no description names its row from the first line of
    /// its prompt; a row still unnamed at its stop takes its reply's.
    #[test]
    fn an_undescribed_subagent_is_named_from_its_prompt_or_its_reply() {
        let daemon = daemon();
        let p = AgentId("p".into());
        daemon.track_subagent(
            &p,
            &HookEvent::PreToolUse {
                tool_name: Some("Agent".into()),
                subagent_id: None,
            },
            SubagentHookInfo {
                task_kind: Some("general-purpose".into()),
                task_prompt: subagent_title("\n  Build WhekoServer and report errors\nmore"),
                ..Default::default()
            },
        );
        daemon.track_subagent(&p, &start("a"), typed("general-purpose"));
        daemon.track_subagent(&p, &start("b"), typed("general-purpose"));
        daemon.track_subagent(
            &p,
            &stop("b"),
            SubagentHookInfo {
                last_message: subagent_title("All tests pass."),
                ..Default::default()
            },
        );
        let rows = daemon.subagent_rows();
        let name = |id: &str| {
            rows.iter()
                .find(|r| r.id == id)
                .unwrap()
                .description
                .clone()
        };
        assert_eq!(name("a"), "Build WhekoServer and report errors");
        assert_eq!(name("b"), "All tests pass.");
        let long = "x".repeat(200);
        assert_eq!(subagent_title(&long).unwrap().chars().count(), TITLE_MAX);
        assert_eq!(subagent_title("  \n "), None);
    }

    fn launch(kind: Option<&str>, task: &str) -> (HookEvent, SubagentHookInfo) {
        (
            HookEvent::PreToolUse {
                tool_name: Some("Agent".into()),
                subagent_id: None,
            },
            SubagentHookInfo {
                kind: None,
                task: Some(task.into()),
                task_kind: kind.map(String::from),
                ..Default::default()
            },
        )
    }

    fn typed(kind: &str) -> SubagentHookInfo {
        SubagentHookInfo {
            kind: Some(kind.into()),
            task: None,
            task_kind: None,
            ..Default::default()
        }
    }

    /// Parallel calls can start out of order: a start takes the oldest
    /// waiting call that asked for its type, and falls back to the oldest
    /// call of all when none did.
    #[test]
    fn a_start_takes_the_task_that_asked_for_its_type() {
        let daemon = daemon();
        let parent = AgentId("p".into());
        for (kind, task) in [
            (Some("Plan"), "Plan the migration"),
            (Some("Explore"), "Find the callers"),
            (None, "Anything at all"),
        ] {
            let (event, info) = launch(kind, task);
            daemon.track_subagent(&parent, &event, info);
        }
        daemon.track_subagent(&parent, &start("e"), typed("Explore"));
        daemon.track_subagent(&parent, &start("x"), typed("general-purpose"));
        daemon.track_subagent(&parent, &start("p"), typed("Plan"));
        let rows = daemon.subagent_rows();
        let task = |id: &str| {
            rows.iter()
                .find(|r| r.id == id)
                .unwrap()
                .description
                .clone()
        };
        assert_eq!(task("e"), "Find the callers");
        assert_eq!(task("x"), "Plan the migration", "no match: oldest first");
        assert_eq!(task("p"), "Anything at all");
    }

    /// IN-PLACE RESTART: the book rides the restart state (encoded as the
    /// state file is, `to_vec_named`). A carried session's subagent keeps
    /// running and its later Stop lands; one whose session did not come
    /// across ends as unknown; finished ones stay as they were.
    #[test]
    fn the_book_survives_an_in_place_restart() {
        let old = daemon();
        let carried = AgentId("carried".into());
        let lost = AgentId("lost".into());
        old.track_subagent(&carried, &start("a"), explore());
        old.track_subagent(&carried, &start("b"), explore());
        old.track_subagent(&carried, &stop("b"), explore());
        old.track_subagent(&lost, &start("c"), explore());
        // A call made just before the restart, its subagent not started.
        let (event, info) = launch(Some("Plan"), "Plan the next step");
        old.track_subagent(&carried, &event, info);

        let bytes = rmp_serde::to_vec_named(&old.carry_subagents()).unwrap();
        let carry: SubagentCarry = rmp_serde::from_slice(&bytes).unwrap();
        let new = daemon();
        new.restore_subagents(carry, &[carried.clone()].into_iter().collect());

        let rows = new.subagent_rows();
        let status = |id: &str| rows.iter().find(|r| r.id == id).unwrap().status;
        assert_eq!(status("a"), SubagentStatus::Running);
        assert_eq!(status("b"), SubagentStatus::Done);
        assert_eq!(status("c"), SubagentStatus::EndedUnknown);
        assert!(rows
            .iter()
            .find(|r| r.id == "c")
            .unwrap()
            .ended_at
            .is_some());

        let mut rx = new.events.subscribe();
        new.track_subagent(&carried, &stop("a"), explore());
        let stopped = changes(&mut rx);
        assert_eq!(stopped.len(), 1, "the carried subagent's Stop lands");
        assert_eq!(stopped[0].status, SubagentStatus::Done);

        // The waiting task came across with its session.
        new.track_subagent(&carried, &start("d"), typed("Plan"));
        let rows = new.subagent_rows();
        let d = rows.iter().find(|r| r.id == "d").unwrap();
        assert_eq!(d.description, "Plan the next step");
    }
}
