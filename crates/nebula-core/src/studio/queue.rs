//! The queue runner's rules (K2): which inbox notes an idle note names,
//! when the runner holds, and the idle note's text.
//!
//! Everything here is pure: the daemon (`registry/studio_queue.rs`) reads
//! the inbox, the clock, the orchestrator and the accounts, and hands this
//! module plain values. [`decide`] says whether an idle note goes out now
//! or why the runner holds; [`compose_note`] writes it. [`QueueState`] is
//! what survives a restart (`<data dir>/studio/queue-state.json`), so a
//! note is announced once per file modification time and never again.

use crate::entities::AgentStatus;
use crate::studio::autonomy::Autonomy;
use crate::studio::inbox::InboxNote;
use crate::studio::wire::QueueNote;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// An idle note is at most this many bytes.
pub const MAX_NOTE_BYTES: usize = 1_200;
/// An idle note lists at most this many inbox notes; the rest wait for a
/// later one.
pub const MAX_LISTED: usize = 5;
/// At most this many idle notes in any 60 minutes.
pub const MAX_PER_HOUR: usize = 6;
/// No idle note until nobody has typed to the orchestrator for this long.
pub const TYPING_QUIET_SECS: i64 = 120;
/// The context nudge fires at or over this percent of the budget.
pub const NUDGE_PERCENT: u64 = 60;
/// The context nudge repeats at most this often.
pub const NUDGE_REPEAT_SECS: i64 = 1_800;
/// The first words of every idle note.
pub const NOTE_HEADER: &str = "[from kete studio]";

/// One inbox note line is cut to this many bytes, so one long title
/// cannot crowd out the rest.
const MAX_LINE_BYTES: usize = 240;
const HOUR_MS: i64 = 3_600_000;

/// What the runner remembers between ticks and across restarts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueState {
    /// Per repo key (`settings::repo_key`), each announced note's id and
    /// the file modification time (epoch ms) it was announced at.
    #[serde(default)]
    pub announced: BTreeMap<String, BTreeMap<String, i64>>,
    /// When the last idle note went out, epoch ms.
    #[serde(default)]
    pub last_note_at: Option<i64>,
    /// When the last idle note for each repo key went out, epoch ms: a
    /// queue row reports its own repo's, not another repo's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last_note_for: BTreeMap<String, i64>,
    /// When each idle note of the last hour went out, epoch ms.
    #[serde(default)]
    pub notes_this_hour: Vec<i64>,
    /// When the last context nudge went out, epoch ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_nudge_at: Option<i64>,
}

impl QueueState {
    /// The state at `path`; a missing or unreadable file is a fresh state.
    pub fn load(path: &Path) -> QueueState {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write the state to `path` (tmp-then-rename), creating the folder.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        crate::settings::write_json(path, &value)
            .map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Idle notes sent in the 60 minutes before `now_ms`.
    pub fn sent_in_last_hour(&self, now_ms: i64) -> usize {
        self.notes_this_hour
            .iter()
            .filter(|at| now_ms - **at < HOUR_MS)
            .count()
    }

    /// The notes announced for `repo`, by id.
    pub fn announced_for(&self, repo: &str) -> Option<&BTreeMap<String, i64>> {
        self.announced.get(repo)
    }

    /// Drop `repo`'s remembered notes that are no longer in its inbox, so
    /// the file stays as small as the inbox.
    pub fn forget_missing(&mut self, repo: &str, present: &BTreeSet<String>) {
        if let Some(seen) = self.announced.get_mut(repo) {
            seen.retain(|id, _| present.contains(id));
            if seen.is_empty() {
                self.announced.remove(repo);
            }
        }
    }

    /// An idle note went out at `now_ms` for `repo`, listing `listed`
    /// (each note's id and modification time); `nudged` when it carried
    /// the context nudge.
    pub fn record(&mut self, repo: &str, listed: &[(String, i64)], now_ms: i64, nudged: bool) {
        let seen = self.announced.entry(repo.to_string()).or_default();
        for (id, mtime) in listed {
            seen.insert(id.clone(), *mtime);
        }
        if seen.is_empty() {
            self.announced.remove(repo);
        }
        self.last_note_at = Some(now_ms);
        self.last_note_for.insert(repo.to_string(), now_ms);
        self.notes_this_hour.retain(|at| now_ms - *at < HOUR_MS);
        self.notes_this_hour.push(now_ms);
        if nudged {
            self.last_nudge_at = Some(now_ms);
        }
    }
}

/// Why the runner is not sending an idle note now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holding {
    /// The repo has no `studio` entry.
    Off,
    /// Autonomy A0: no idle notes.
    AutonomyHold,
    /// No orchestrator, or its session is not running.
    NoOrchestrator,
    /// The orchestrator is in a turn.
    Busy,
    /// The orchestrator is waiting on you.
    NeedsYou,
    /// Someone typed to the orchestrator `secs` ago.
    Typing { secs: i64 },
    /// The orchestrator went idle `secs` ago, less than `studio_idle_after`.
    NotIdleLongEnough { secs: i64 },
    /// [`MAX_PER_HOUR`] idle notes went out in the last hour.
    HourlyCap,
    /// Every eligible note was announced at its current modification time.
    NothingNew,
}

impl std::fmt::Display for Holding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Holding::Off => write!(f, "studio is off"),
            Holding::AutonomyHold => write!(f, "autonomy A0 (Hold)"),
            Holding::NoOrchestrator => write!(f, "no orchestrator is running"),
            Holding::Busy => write!(f, "the orchestrator is working"),
            Holding::NeedsYou => write!(f, "the orchestrator needs you"),
            Holding::Typing { secs } => write!(f, "you typed {secs} s ago"),
            Holding::NotIdleLongEnough { secs } => {
                write!(f, "the orchestrator went idle {secs} s ago")
            }
            Holding::HourlyCap => write!(f, "{MAX_PER_HOUR} idle notes in the last hour"),
            Holding::NothingNew => write!(f, "nothing new"),
        }
    }
}

/// The orchestrator as one tick sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestratorView {
    pub status: AgentStatus,
    /// Epoch ms of its last status change; 0 when unknown.
    pub status_changed_at: i64,
    /// Epoch ms a client last typed into it, when one has.
    pub last_input_at: Option<i64>,
}

/// Everything [`decide`] reads for one repo.
#[derive(Debug, Clone)]
pub struct TickInputs<'a> {
    /// The repo's autonomy; None when it has no `studio` entry.
    pub autonomy: Option<Autonomy>,
    pub orchestrator: Option<OrchestratorView>,
    /// `studio_idle_after`, in seconds.
    pub idle_after_secs: i64,
    /// The eligible notes (`inbox::eligible`, in its order), each with its
    /// file modification time, epoch ms.
    pub notes: &'a [(&'a InboxNote, i64)],
    /// The repo's announced notes ([`QueueState::announced_for`]).
    pub announced: Option<&'a BTreeMap<String, i64>>,
    /// [`QueueState::sent_in_last_hour`].
    pub sent_last_hour: usize,
    /// A context nudge is due ([`nudge`]): it goes out alone when nothing
    /// is new.
    pub nudge_due: bool,
}

/// The eligible notes not yet announced at their current modification
/// time, in the order given.
pub fn unannounced<'a>(
    notes: &[(&'a InboxNote, i64)],
    announced: Option<&BTreeMap<String, i64>>,
) -> Vec<(&'a InboxNote, i64)> {
    notes
        .iter()
        .filter(|(note, mtime)| announced.and_then(|seen| seen.get(&note.id)) != Some(mtime))
        .copied()
        .collect()
}

/// Whether an idle note goes out for this repo now: the notes it would
/// name (empty when only a context nudge is due), or why the runner holds.
pub fn decide<'a>(now_ms: i64, inputs: &TickInputs<'a>) -> Result<Vec<&'a InboxNote>, Holding> {
    match inputs.autonomy {
        None => return Err(Holding::Off),
        Some(Autonomy::A0) => return Err(Holding::AutonomyHold),
        Some(_) => {}
    }
    let fresh: Vec<&InboxNote> = unannounced(inputs.notes, inputs.announced)
        .into_iter()
        .map(|(note, _)| note)
        .collect();
    if fresh.is_empty() && !inputs.nudge_due {
        return Err(Holding::NothingNew);
    }
    let orch = inputs.orchestrator.ok_or(Holding::NoOrchestrator)?;
    match orch.status {
        AgentStatus::Fresh | AgentStatus::Finished => {}
        AgentStatus::Running => return Err(Holding::Busy),
        AgentStatus::NeedsFeedback => return Err(Holding::NeedsYou),
        AgentStatus::Terminated | AgentStatus::Disconnected => return Err(Holding::NoOrchestrator),
    }
    if let Some(typed) = orch.last_input_at {
        let secs = (now_ms - typed).max(0) / 1000;
        if secs < TYPING_QUIET_SECS {
            return Err(Holding::Typing { secs });
        }
    }
    let idle = (now_ms - orch.status_changed_at).max(0) / 1000;
    if idle < inputs.idle_after_secs {
        return Err(Holding::NotIdleLongEnough { secs: idle });
    }
    if inputs.sent_last_hour >= MAX_PER_HOUR {
        return Err(Holding::HourlyCap);
    }
    Ok(fresh)
}

/// The orchestrator's context against its budget, when it is due a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextNudge {
    /// Percent of the budget used, floored.
    pub percent: u64,
    /// The budget in tokens: the role card's `context_budget`, else the
    /// model window.
    pub budget: u64,
}

/// A nudge when `tokens` is at or over [`NUDGE_PERCENT`] of `budget` and
/// none went out in the last [`NUDGE_REPEAT_SECS`].
pub fn nudge(
    now_ms: i64,
    tokens: u64,
    budget: u64,
    last_nudge_at: Option<i64>,
) -> Option<ContextNudge> {
    if budget == 0 {
        return None;
    }
    let percent = tokens.saturating_mul(100) / budget;
    if percent < NUDGE_PERCENT {
        return None;
    }
    if last_nudge_at.is_some_and(|at| now_ms - at < NUDGE_REPEAT_SECS * 1000) {
        return None;
    }
    Some(ContextNudge { percent, budget })
}

/// An inbox note as a queue row and an idle note name it: its path
/// relative to the repo, with forward slashes.
pub fn queue_note(note: &InboxNote, repo_root: &Path) -> QueueNote {
    let path = note.path.strip_prefix(repo_root).unwrap_or(&note.path);
    QueueNote {
        id: note.id.clone(),
        title: note.title.clone(),
        to: note.to.clone().unwrap_or_default(),
        priority: note.priority.as_str().to_string(),
        path: path.to_string_lossy().replace('\\', "/"),
    }
}

/// The accounts line: each account with its 5-hour window, then the
/// reserved ones by name. `accounts` holds (name, percent used, the reset
/// as a local clock). None when there is nothing to say.
pub fn headroom_line(
    accounts: &[(String, Option<f32>, Option<String>)],
    reserved: &[String],
) -> Option<String> {
    let mut parts: Vec<String> = accounts
        .iter()
        .filter(|(name, _, _)| !reserved.contains(name))
        .filter_map(|(name, percent, clock)| {
            let percent = (*percent)?;
            Some(match clock {
                Some(clock) => format!("{name} 5h {percent:.0}% (resets {clock})"),
                None => format!("{name} 5h {percent:.0}%"),
            })
        })
        .collect();
    parts.extend(reserved.iter().map(|name| format!("{name} reserved")));
    (!parts.is_empty()).then(|| format!("{}.", parts.join("; ")))
}

/// An idle note, and how many of the notes it was given it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleNote {
    pub text: String,
    /// The first `listed` notes are named; the rest wait for a later note.
    pub listed: usize,
}

/// The idle note for `repo_name`: a header at the local time `now_local`
/// (epoch seconds shifted by the local offset), up to [`MAX_LISTED`] of
/// `notes` (highest priority first, as given), the accounts line, the
/// autonomy reminder, and the context nudge. Fewer notes are listed until
/// the text fits [`MAX_NOTE_BYTES`]. With no notes, only the nudge.
pub fn compose_note(
    now_local: i64,
    repo_name: &str,
    autonomy: Autonomy,
    notes: &[QueueNote],
    headroom: Option<&str>,
    nudge: Option<ContextNudge>,
) -> IdleNote {
    let secs = now_local.rem_euclid(86_400);
    let clock = format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60);
    let nudge_line = nudge.map(|n| {
        format!(
            "Context {}% of {}: write your handover note, then run `kete orchestrator renew --from <note>`.",
            n.percent,
            tokens_short(n.budget)
        )
    });
    let build = |listed: usize| -> String {
        let mut lines = Vec::new();
        if notes.is_empty() {
            lines.push(format!(
                "{NOTE_HEADER} {clock} · {repo_name} {}",
                autonomy.as_str()
            ));
        } else {
            let count = notes.len();
            let noun = if count == 1 { "note" } else { "notes" };
            lines.push(format!(
                "{NOTE_HEADER} {clock} · {repo_name} {} · {count} new inbox {noun}",
                autonomy.as_str()
            ));
            for (i, n) in notes.iter().take(listed).enumerate() {
                let to = if n.to.is_empty() { "-" } else { &n.to };
                lines.push(cut(
                    &format!(
                        "{}. {} · {to} · {} · {}",
                        i + 1,
                        n.priority,
                        n.path,
                        n.title
                    ),
                    MAX_LINE_BYTES,
                ));
            }
            if count > listed {
                lines.push(format!("and {} more", count - listed));
            }
            if let Some(headroom) = headroom {
                lines.push(cut(&format!("Accounts: {headroom}"), MAX_LINE_BYTES));
            }
            lines.push(format!(
                "Act within {autonomy}. Set each note's status when you take it."
            ));
        }
        if let Some(line) = &nudge_line {
            lines.push(line.clone());
        }
        lines.join("\n")
    };
    let mut listed = notes.len().min(MAX_LISTED);
    let mut text = build(listed);
    while text.len() > MAX_NOTE_BYTES && listed > 1 {
        listed -= 1;
        text = build(listed);
    }
    IdleNote {
        text: cut(&text, MAX_NOTE_BYTES),
        listed,
    }
}

/// `400k`, `1M`: a token count as an idle note writes it.
fn tokens_short(tokens: u64) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(1_000_000) {
        format!("{}M", tokens / 1_000_000)
    } else {
        format!("{}k", tokens.div_ceil(1000))
    }
}

/// `s` cut to at most `max` bytes on a character boundary, ending in
/// `...` when cut.
fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub(3);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::inbox::{eligible, NoteStatus, Priority};
    use std::path::PathBuf;

    const NOW: i64 = 1_800_000_000_000;

    fn note(id: &str, priority: Priority, at: Option<&str>) -> InboxNote {
        InboxNote {
            id: id.into(),
            path: PathBuf::from(format!("/repo/notes/inbox/{id}.md")),
            title: format!("Title of {id}"),
            status: NoteStatus::Open,
            to: Some("orchestrator".into()),
            about: None,
            priority,
            at: at.map(String::from),
            by: None,
        }
    }

    fn idle_orch() -> OrchestratorView {
        OrchestratorView {
            status: AgentStatus::Finished,
            status_changed_at: NOW - 60_000,
            last_input_at: None,
        }
    }

    fn inputs<'a>(notes: &'a [(&'a InboxNote, i64)]) -> TickInputs<'a> {
        TickInputs {
            autonomy: Some(Autonomy::A1),
            orchestrator: Some(idle_orch()),
            idle_after_secs: 45,
            notes,
            announced: None,
            sent_last_hour: 0,
            nudge_due: false,
        }
    }

    fn ids(notes: &[&InboxNote]) -> Vec<String> {
        notes.iter().map(|n| n.id.clone()).collect()
    }

    #[test]
    fn eligible_notes_go_out_by_priority_then_oldest_first() {
        let all = vec![
            note(
                "b-normal-new",
                Priority::Normal,
                Some("2026-10-21T09:00:00Z"),
            ),
            note("c-low", Priority::Low, Some("2026-10-20T09:00:00Z")),
            note(
                "a-normal-old",
                Priority::Normal,
                Some("2026-10-20T09:00:00Z"),
            ),
            note("d-high", Priority::High, Some("2026-10-21T10:00:00Z")),
        ];
        let picked = eligible(&all, &[], &[]);
        let with_mtime: Vec<(&InboxNote, i64)> = picked.into_iter().map(|n| (n, 1)).collect();
        let out = decide(NOW, &inputs(&with_mtime)).unwrap();
        assert_eq!(
            ids(&out),
            ["d-high", "a-normal-old", "b-normal-new", "c-low"]
        );
    }

    #[test]
    fn a_note_is_announced_once_per_modification_time() {
        let a = note("a", Priority::Normal, None);
        let b = note("b", Priority::Normal, None);
        let mut state = QueueState::default();
        state.record("repo", &[("a".into(), 10), ("b".into(), 20)], NOW, false);

        // Untouched: never repeated.
        let notes = [(&a, 10), (&b, 20)];
        let mut tick = inputs(&notes);
        tick.announced = state.announced_for("repo");
        assert_eq!(decide(NOW, &tick), Err(Holding::NothingNew));

        // Edited (a new mtime): announced again, alone.
        let notes = [(&a, 11), (&b, 20)];
        let mut tick = inputs(&notes);
        tick.announced = state.announced_for("repo");
        assert_eq!(ids(&decide(NOW, &tick).unwrap()), ["a"]);
    }

    #[test]
    fn exclusion_by_id_and_by_about() {
        let mut by_id = note("paused-lane", Priority::High, None);
        by_id.about = Some("Library".into());
        let mut by_about = note("other", Priority::High, None);
        by_about.about = Some("Mech Econ".into());
        let kept = note("kept", Priority::Normal, None);
        let all = vec![by_id, by_about, kept];
        let out = eligible(&all, &[], &["PAUSED-LANE".into(), " mech econ ".into()]);
        assert_eq!(ids(&out), ["kept"]);
    }

    #[test]
    fn each_holding_reason() {
        let a = note("a", Priority::Normal, None);
        let notes = [(&a, 1)];
        let check = |edit: &dyn Fn(&mut TickInputs), want: Holding| {
            let mut tick = inputs(&notes);
            edit(&mut tick);
            assert_eq!(decide(NOW, &tick), Err(want));
        };
        check(&|t| t.autonomy = None, Holding::Off);
        check(&|t| t.autonomy = Some(Autonomy::A0), Holding::AutonomyHold);
        check(&|t| t.orchestrator = None, Holding::NoOrchestrator);
        for (status, want) in [
            (AgentStatus::Running, Holding::Busy),
            (AgentStatus::NeedsFeedback, Holding::NeedsYou),
            (AgentStatus::Terminated, Holding::NoOrchestrator),
            (AgentStatus::Disconnected, Holding::NoOrchestrator),
        ] {
            check(&|t| t.orchestrator.as_mut().unwrap().status = status, want);
        }
        check(
            &|t| t.orchestrator.as_mut().unwrap().last_input_at = Some(NOW - 40_000),
            Holding::Typing { secs: 40 },
        );
        check(
            &|t| t.orchestrator.as_mut().unwrap().status_changed_at = NOW - 20_000,
            Holding::NotIdleLongEnough { secs: 20 },
        );
        check(&|t| t.sent_last_hour = MAX_PER_HOUR, Holding::HourlyCap);
        check(&|t| t.notes = &[], Holding::NothingNew);

        // Typing 120 s ago is quiet enough; Fresh counts as idle.
        let mut tick = inputs(&notes);
        tick.orchestrator.as_mut().unwrap().last_input_at = Some(NOW - 120_000);
        tick.orchestrator.as_mut().unwrap().status = AgentStatus::Fresh;
        assert!(decide(NOW, &tick).is_ok());

        // The words the status line and `kete studio status` print.
        assert_eq!(
            Holding::Typing { secs: 40 }.to_string(),
            "you typed 40 s ago"
        );
        assert_eq!(
            Holding::HourlyCap.to_string(),
            "6 idle notes in the last hour"
        );
    }

    #[test]
    fn a0_never_injects_even_with_news_or_a_nudge() {
        let a = note("a", Priority::High, None);
        let notes = [(&a, 1)];
        let mut tick = inputs(&notes);
        tick.autonomy = Some(Autonomy::A0);
        tick.nudge_due = true;
        assert_eq!(decide(NOW, &tick), Err(Holding::AutonomyHold));
    }

    #[test]
    fn six_idle_notes_an_hour_at_most() {
        let mut state = QueueState::default();
        for i in 0..6 {
            state.record("repo", &[], NOW - 3_000_000 + i * 60_000, false);
        }
        assert_eq!(state.sent_in_last_hour(NOW), 6);
        let a = note("a", Priority::Normal, None);
        let notes = [(&a, 1)];
        let mut tick = inputs(&notes);
        tick.sent_last_hour = state.sent_in_last_hour(NOW);
        assert_eq!(decide(NOW, &tick), Err(Holding::HourlyCap));
        // An hour after the first, one slot is free again.
        let later = NOW - 3_000_000 + HOUR_MS;
        assert_eq!(state.sent_in_last_hour(later), 5);
    }

    fn qn(i: usize, title_len: usize) -> QueueNote {
        QueueNote {
            id: format!("n{i}"),
            title: "x".repeat(title_len),
            to: "orchestrator".into(),
            priority: "normal".into(),
            path: format!("notes/inbox/n{i}.md"),
        }
    }

    #[test]
    fn the_note_reads_as_the_spec_shows() {
        let notes = vec![
            QueueNote {
                id: "storm".into(),
                title: "Storm sky banding at dusk".into(),
                to: "world-and-weather".into(),
                priority: "high".into(),
                path: "notes/inbox/2026-10-21-storm-sky.md".into(),
            },
            QueueNote {
                id: "ledger".into(),
                title: "Shift ledger".into(),
                to: "orchestrator".into(),
                priority: "normal".into(),
                path: "notes/inbox/2026-10-21-ledger.md".into(),
            },
        ];
        let headroom = headroom_line(
            &[
                ("max2".into(), Some(34.0), Some("3:10 pm".into())),
                ("jayrich".into(), Some(80.0), None),
            ],
            &["jayrich".into()],
        );
        let note = compose_note(
            14 * 3600 + 5 * 60,
            "wheako",
            Autonomy::A2,
            &notes,
            headroom.as_deref(),
            None,
        );
        assert_eq!(note.listed, 2);
        assert_eq!(
            note.text,
            "[from kete studio] 14:05 · wheako A2 · 2 new inbox notes\n\
             1. high · world-and-weather · notes/inbox/2026-10-21-storm-sky.md · Storm sky banding at dusk\n\
             2. normal · orchestrator · notes/inbox/2026-10-21-ledger.md · Shift ledger\n\
             Accounts: max2 5h 34% (resets 3:10 pm); jayrich reserved.\n\
             Act within A2 (Deliver). Set each note's status when you take it."
        );
    }

    #[test]
    fn a_long_note_lists_five_then_and_n_more_and_fits_1200_bytes() {
        let notes: Vec<QueueNote> = (0..8).map(|i| qn(i, 20)).collect();
        let note = compose_note(0, "repo", Autonomy::A1, &notes, None, None);
        assert_eq!(note.listed, MAX_LISTED);
        assert!(note.text.contains("\nand 3 more\n"), "{}", note.text);
        assert!(note.text.contains("5. normal"));
        assert!(!note.text.contains("6. normal"));

        // Titles long enough that five do not fit: fewer are listed.
        let notes: Vec<QueueNote> = (0..8).map(|i| qn(i, 1_000)).collect();
        let note = compose_note(0, "repo", Autonomy::A1, &notes, None, None);
        assert!(note.text.len() <= MAX_NOTE_BYTES, "{}", note.text.len());
        assert!(note.listed < MAX_LISTED && note.listed >= 1);
        assert!(note.text.contains(&format!("and {} more", 8 - note.listed)));
    }

    #[test]
    fn the_context_nudge_at_60_percent_and_its_30_minute_repeat() {
        assert_eq!(nudge(NOW, 59_999, 100_000, None), None);
        assert_eq!(
            nudge(NOW, 60_000, 100_000, None),
            Some(ContextNudge {
                percent: 60,
                budget: 100_000
            })
        );
        // Sent 29 minutes ago: not again; 30 minutes: again.
        assert_eq!(nudge(NOW, 90_000, 100_000, Some(NOW - 29 * 60_000)), None);
        assert!(nudge(NOW, 90_000, 100_000, Some(NOW - 30 * 60_000)).is_some());
        assert_eq!(nudge(NOW, 10, 0, None), None);

        // Alone, when nothing is new, under the same conditions.
        let mut tick = inputs(&[]);
        tick.nudge_due = true;
        assert_eq!(decide(NOW, &tick), Ok(vec![]));
        let text = compose_note(
            0,
            "wheako",
            Autonomy::A1,
            &[],
            None,
            nudge(NOW, 244_000, 400_000, None),
        )
        .text;
        assert_eq!(
            text,
            "[from kete studio] 00:00 · wheako A1\n\
             Context 61% of 400k: write your handover note, then run `kete orchestrator renew --from <note>`."
        );
    }

    #[test]
    fn state_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio").join("queue-state.json");
        assert_eq!(QueueState::load(&path), QueueState::default());
        let mut state = QueueState::default();
        state.record("C:/repo", &[("a".into(), 7)], NOW, true);
        state.save(&path).unwrap();
        let back = QueueState::load(&path);
        assert_eq!(back, state);
        assert_eq!(back.last_nudge_at, Some(NOW));
        assert_eq!(back.last_note_for.get("C:/repo"), Some(&NOW));

        let mut present = BTreeSet::new();
        present.insert("b".to_string());
        let mut trimmed = back.clone();
        trimmed.forget_missing("C:/repo", &present);
        assert!(trimmed.announced.is_empty());
    }

    #[test]
    fn a_queue_note_names_its_path_inside_the_repo() {
        let n = note("a", Priority::High, None);
        let q = queue_note(&n, Path::new("/repo"));
        assert_eq!(q.path, "notes/inbox/a.md");
        assert_eq!(q.priority, "high");
        assert_eq!(q.to, "orchestrator");
    }
}
