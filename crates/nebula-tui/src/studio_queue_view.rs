//! The studio queue's TUI side (K2): the board header's autonomy and
//! runner line, and the autonomy control.
//!
//! The control is inline: [`header_line`] draws it and [`handle_key`]
//! drives it, because there is no overlay or prompt kind for it. The board
//! lane calls `header_line` from the board's header and routes keys here
//! while the board is up. [`open_autonomy`] must be called first, with the
//! board's repo (the board calls it when it opens, and the
//! `studio_autonomy` verb calls it from a menu): it picks the repo the keys
//! act on and says what they are. Until then `handle_key` takes nothing.
//!
//! Keys: `]` or `+` raises one level, `[` or `-` lowers one. Lowering and
//! raising to A1 apply at once. Raising to A2 or A3 asks: `y` or Enter
//! confirms, `n` or Esc cancels, any other key is taken and does nothing.
//!
//! Nothing here waits. The queue rows are read on a background job and
//! kept in module state; a level change is written on one too, and the
//! footer says how it went.

use crate::app::App;
use crate::theme::Theme;
use crate::view_jobs::{Answer, Jobs};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::Autonomy;
use nebula_core::ProjectId;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long the cached rows stand before the next draw reads them again.
const FRESH: Duration = Duration::from_secs(10);

/// The daemon's phrase for a hold that wants Jay; the only one drawn red.
const NEEDS_YOU: &str = "the orchestrator needs you";

/// What one key did to the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Not a key of the control's: the caller may use it.
    Ignored,
    /// A raise to A2 or A3 is waiting for a yes.
    Moved,
    /// Write this level.
    Apply(Autonomy),
    /// The waiting raise was dropped.
    Cancelled,
    /// Taken while a raise waits, and did nothing.
    Swallowed,
}

/// The autonomy control for one repo: the level it reads and the raise it
/// is asking about.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AutonomyControl {
    repo: ProjectId,
    repo_name: String,
    level: Autonomy,
    pending: Option<Autonomy>,
}

impl AutonomyControl {
    fn key(&mut self, key: KeyEvent) -> Step {
        // A chord is not ours: `+` arrives with Shift, so only Ctrl and Alt count.
        let code = if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            KeyCode::Null
        } else {
            key.code
        };
        if let Some(asked) = self.pending {
            return match code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    self.pending = None;
                    self.level = asked;
                    Step::Apply(asked)
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    self.pending = None;
                    Step::Cancelled
                }
                _ => Step::Swallowed,
            };
        }
        let at = Autonomy::ALL
            .iter()
            .position(|l| *l == self.level)
            .unwrap_or(1);
        match code {
            KeyCode::Char(']' | '+') => match Autonomy::ALL.get(at + 1) {
                None => Step::Ignored,
                Some(&Autonomy::A1) => {
                    self.level = Autonomy::A1;
                    Step::Apply(Autonomy::A1)
                }
                Some(&up) => {
                    self.pending = Some(up);
                    Step::Moved
                }
            },
            KeyCode::Char('[' | '-') => match at.checked_sub(1) {
                None => Step::Ignored,
                Some(down) => {
                    self.level = Autonomy::ALL[down];
                    Step::Apply(self.level)
                }
            },
            _ => Step::Ignored,
        }
    }
}

/// What the draws and the keys share until the board lane gives `App` a
/// field for it.
struct State {
    rows: Vec<StudioQueueRow>,
    read_at: Option<Instant>,
    reading: bool,
    /// Bumped by each level change, so a read that began before it never
    /// writes the old level back.
    epoch: u64,
    /// Why the last read failed; None after one that worked.
    failed: Option<String>,
    control: Option<AutonomyControl>,
}

impl State {
    const EMPTY: State = State {
        rows: Vec::new(),
        read_at: None,
        reading: false,
        epoch: 0,
        failed: None,
        control: None,
    };
}

// shortcut: module state until the board lane gives App a field for it, move it then
static STATE: Mutex<State> = Mutex::new(State::EMPTY);

fn state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run a one-shot client on a runtime of its own; only ever called on a
/// background job.
fn block<T>(work: impl std::future::Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(work)
}

/// Start a read of every studio repo's queue row when the cached ones are
/// stale and none is in flight. With no `jobs` (a test) nothing starts.
fn refresh_if_stale(st: &mut State, jobs: Option<&Jobs>) {
    let Some(jobs) = jobs else { return };
    if st.reading || st.read_at.is_some_and(|at| at.elapsed() < FRESH) {
        return;
    }
    st.reading = true;
    let epoch = st.epoch;
    jobs.run(move || {
        let read = block(crate::ipc_studio::studio_queue(None, false));
        let mut st = state();
        st.reading = false;
        if st.epoch == epoch {
            match read {
                Ok(rows) => {
                    st.rows = rows;
                    st.failed = None;
                }
                Err(why) => st.failed = Some(why.to_string()),
            }
            st.read_at = Some(Instant::now());
        }
        None
    });
}

fn flash(app: &mut App, line: impl Into<String>) {
    app.chrome.flash = Some(line.into());
    app.chrome.dirty = true;
}

/// The lead (drawn bold) and the rest of the header's reading.
fn header_parts(
    level: Option<Autonomy>,
    row: Option<&StudioQueueRow>,
    pending: Option<Autonomy>,
    now_ms: i64,
    tz_offset_secs: i32,
) -> (String, String) {
    if let Some(asked) = pending {
        return (
            format!("Raise to {} {}?", asked.as_str(), asked.label()),
            " y confirm · n cancel".into(),
        );
    }
    let Some(level) = level.or(row.map(|r| r.autonomy)) else {
        return ("Studio off".into(), String::new());
    };
    let lead = format!("{} {}", level.as_str(), level.label());
    let rest = match row {
        None => String::new(),
        Some(r) => match (&r.held, r.last_note_at) {
            (Some(why), _) => format!(" · holding: {why}"),
            (None, Some(at)) => {
                let mut said = format!(" · idle note {}", when(at, now_ms, tz_offset_secs));
                if r.unannounced > 0 {
                    said.push_str(&format!(" · {} new", r.unannounced));
                }
                said
            }
            (None, None) => format!(
                " · ready: {} new {}",
                r.unannounced,
                if r.unannounced == 1 { "note" } else { "notes" }
            ),
        },
    };
    (lead, rest)
}

/// The board header's reading as one string: `A1 Plan · holding: nothing
/// new`, `A1 Plan · idle note at 14:05 · 3 new`, `Studio off`.
#[cfg(test)]
fn header_text(
    level: Option<Autonomy>,
    row: Option<&StudioQueueRow>,
    pending: Option<Autonomy>,
    now_ms: i64,
    tz_offset_secs: i32,
) -> String {
    let (lead, rest) = header_parts(level, row, pending, now_ms, tz_offset_secs);
    lead + &rest
}

/// `at 14:05`, `yesterday at 14:05` or `3 days ago at 14:05`, on the local
/// clock `tz_offset_secs` east of UTC.
fn when(at_ms: i64, now_ms: i64, tz_offset_secs: i32) -> String {
    use nebula_core::wake::LocalTime;
    let then = LocalTime::at(at_ms.div_euclid(1000), tz_offset_secs);
    let today = LocalTime::at(now_ms.div_euclid(1000), tz_offset_secs).day;
    let clock = nebula_core::wake::hhmm(then.minute);
    match today - then.day {
        n if n <= 0 => format!("at {clock}"),
        1 => format!("yesterday at {clock}"),
        n => format!("{n} days ago at {clock}"),
    }
}

/// The header's reading as a line: the lead in bold, and the words red
/// when the hold is the one that needs Jay (the words say it too).
fn line_for(
    theme: Theme,
    level: Option<Autonomy>,
    row: Option<&StudioQueueRow>,
    pending: Option<Autonomy>,
    now_ms: i64,
    tz_offset_secs: i32,
) -> Line<'static> {
    let (lead, rest) = header_parts(level, row, pending, now_ms, tz_offset_secs);
    let urgent = pending.is_none() && row.is_some_and(|r| r.held.as_deref() == Some(NEEDS_YOU));
    let rest_style = if urgent {
        Style::new().fg(theme.err)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(lead, Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(rest, rest_style),
    ])
}

/// The board header's line for `repo`: its autonomy and what the runner
/// is doing ("idle note at 14:05", "holding: you typed 40 s ago"). Reads
/// the cached rows only; a stale cache starts a read in the background.
pub fn header_line(app: &App, repo: Option<&ProjectId>) -> Line<'static> {
    let (row, pending, read, failed) = {
        let mut st = state();
        refresh_if_stale(&mut st, app.jobs.view_jobs.as_ref());
        let row = repo.and_then(|id| st.rows.iter().find(|r| &r.project == id).cloned());
        let pending = st
            .control
            .as_ref()
            .filter(|c| Some(&c.repo) == repo)
            .and_then(|c| c.pending);
        (row, pending, st.read_at.is_some(), st.failed.clone())
    };
    if row.is_none() && pending.is_none() {
        if let Some(why) = failed {
            return Line::from(format!("Studio: could not read the queue ({why})"));
        }
        if !read {
            return Line::from("Studio: reading the queue");
        }
    }
    let level = row.as_ref().map(|r| r.autonomy);
    line_for(
        app.chrome.theme,
        level,
        row.as_ref(),
        pending,
        nebula_core::clock::now_ms(),
        nebula_core::clock::local_offset_secs(),
    )
}

/// Open the autonomy control for `repo` (the `studio_autonomy` verb, and
/// the board when it opens): the keys then act on this repo.
pub fn open_autonomy(app: &mut App, repo: Option<ProjectId>) {
    let Some(repo) = repo else {
        return flash(app, "Pick a repo first");
    };
    let Some(name) = app
        .tree
        .projects
        .iter()
        .find(|p| p.id == repo)
        .map(|p| p.name.clone())
    else {
        return flash(app, "That repo is not in Kete");
    };
    let level = {
        let mut st = state();
        refresh_if_stale(&mut st, app.jobs.view_jobs.as_ref());
        let level = st
            .rows
            .iter()
            .find(|r| r.project == repo)
            .map_or(Autonomy::A1, |r| r.autonomy);
        st.control = Some(AutonomyControl {
            repo,
            repo_name: name.clone(),
            level,
            pending: None,
        });
        level
    };
    flash(
        app,
        format!(
            "{name}: {level}. [ lower · ] raise, on the studio board, or kete studio autonomy {name} A2"
        ),
    );
}

/// A key while the autonomy control is up; true when it took it.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    let (step, repo, name) = {
        let mut st = state();
        let State { rows, control, .. } = &mut *st;
        let Some(c) = control.as_mut() else {
            return false;
        };
        // The cached row is the truth about the level, between asks.
        if c.pending.is_none() {
            if let Some(row) = rows.iter().find(|r| r.project == c.repo) {
                c.level = row.autonomy;
            }
        }
        (c.key(key), c.repo.clone(), c.repo_name.clone())
    };
    match step {
        Step::Ignored => return false,
        Step::Apply(level) => apply(app, repo, name, level),
        Step::Moved | Step::Cancelled | Step::Swallowed => {}
    }
    app.chrome.dirty = true;
    true
}

/// A key on the studio board for `repo`: `[` and `]` (or `-` and `+`)
/// work the control, opened on `repo` when it is not already, and a raise
/// waiting for a yes on `repo` takes every key. True when it took the key.
pub fn board_key(app: &mut App, repo: Option<&ProjectId>, key: KeyEvent) -> bool {
    let bracket = matches!(key.code, KeyCode::Char('[' | ']' | '-' | '+'))
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let (here, asking) = state().control.as_ref().map_or((false, false), |c| {
        let here = Some(&c.repo) == repo;
        (here, here && c.pending.is_some())
    });
    if !asking && !bracket {
        return false;
    }
    if !here {
        open_autonomy(app, repo.cloned());
    }
    handle_key(app, key)
}

/// Write `level` for `repo` on a background job, show it at once, and say
/// in the footer how it went.
fn apply(app: &mut App, repo: ProjectId, name: String, level: Autonomy) {
    let Some(root) = app
        .tree
        .projects
        .iter()
        .find(|p| p.id == repo)
        .map(|p| p.repo_path.clone())
    else {
        return flash(app, format!("{name} is not in Kete"));
    };
    let Some(jobs) = app.jobs.view_jobs.clone() else {
        return flash(app, "Autonomy can only change in the running Kete");
    };
    {
        let mut st = state();
        st.epoch += 1;
        if let Some(row) = st.rows.iter_mut().find(|r| r.project == repo) {
            row.autonomy = level;
        }
    }
    jobs.run(move || {
        let said = set_level(&repo, &root, &name, level);
        // Read the daemon's rows again, whether it took the level or not.
        state().read_at = None;
        Some(Answer::Flash(said))
    });
}

/// Send `level` for the repo at `root`, keeping its inbox and exclusions.
fn set_level(repo: &ProjectId, root: &Path, name: &str, level: Autonomy) -> String {
    let Some(mut settings) = nebula_core::studio::settings::for_repo(root) else {
        return format!("Studio is off for {name}: run kete studio enable {name}");
    };
    settings.autonomy = level;
    match block(crate::ipc_studio::set_studio(repo.clone(), Some(settings))) {
        Ok(()) => format!("Autonomy for {name}: {level}"),
        Err(why) => format!("Autonomy for {name} not changed: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        k(KeyCode::Char(c))
    }

    fn control(level: Autonomy) -> AutonomyControl {
        AutonomyControl {
            repo: ProjectId("p".into()),
            repo_name: "wheako".into(),
            level,
            pending: None,
        }
    }

    fn row(level: Autonomy) -> StudioQueueRow {
        StudioQueueRow {
            project: ProjectId("p".into()),
            name: "wheako".into(),
            autonomy: level,
            inbox: "notes/inbox".into(),
            open: 3,
            to_jay: 0,
            unannounced: 3,
            next: Vec::new(),
            held: None,
            last_note_at: None,
        }
    }

    fn held(why: &str) -> StudioQueueRow {
        StudioQueueRow {
            held: Some(why.into()),
            ..row(Autonomy::A1)
        }
    }

    /// 2026-10-21 14:05:00 UTC, in epoch ms.
    const AT_1405: i64 = 1_792_591_500_000;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn lowering_applies_at_once() {
        let mut c = control(Autonomy::A3);
        assert_eq!(c.key(ch('[')), Step::Apply(Autonomy::A2));
        assert_eq!(c.level, Autonomy::A2);
        assert_eq!(c.key(ch('[')), Step::Apply(Autonomy::A1));
        assert_eq!(c.key(ch('[')), Step::Apply(Autonomy::A0));
        assert_eq!(c.pending, None);
    }

    #[test]
    fn raising_to_plan_applies_and_past_it_asks() {
        let mut c = control(Autonomy::A0);
        assert_eq!(c.key(ch(']')), Step::Apply(Autonomy::A1));
        assert_eq!(c.key(ch(']')), Step::Moved);
        assert_eq!(c.pending, Some(Autonomy::A2));
        assert_eq!(c.level, Autonomy::A1, "the level moves on the yes");
        assert_eq!(c.key(ch('y')), Step::Apply(Autonomy::A2));
        assert_eq!((c.level, c.pending), (Autonomy::A2, None));
        assert_eq!(c.key(ch(']')), Step::Moved);
        assert_eq!(c.key(k(KeyCode::Enter)), Step::Apply(Autonomy::A3));
    }

    #[test]
    fn a_no_drops_the_raise() {
        for no in [ch('n'), k(KeyCode::Esc)] {
            let mut c = control(Autonomy::A1);
            assert_eq!(c.key(ch(']')), Step::Moved);
            assert_eq!(c.key(no), Step::Cancelled);
            assert_eq!((c.level, c.pending), (Autonomy::A1, None));
        }
    }

    #[test]
    fn the_ends_of_the_scale_ignore_the_key() {
        assert_eq!(control(Autonomy::A3).key(ch(']')), Step::Ignored);
        assert_eq!(control(Autonomy::A0).key(ch('[')), Step::Ignored);
        assert_eq!(control(Autonomy::A1).key(ch('x')), Step::Ignored);
    }

    #[test]
    fn other_keys_are_taken_while_a_raise_waits() {
        let mut c = control(Autonomy::A1);
        c.key(ch(']'));
        for other in [ch('x'), ch('['), ch(']'), k(KeyCode::Down), ch(' ')] {
            assert_eq!(c.key(other), Step::Swallowed);
            assert_eq!((c.level, c.pending), (Autonomy::A1, Some(Autonomy::A2)));
        }
        let ctrl_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(c.key(ctrl_y), Step::Swallowed, "a chord is not a yes");
    }

    /// INPUT PARITY: the twin keys do what each other do, from every level.
    #[test]
    fn twin_keys_do_the_same() {
        let both = |a: KeyEvent, b: KeyEvent| {
            for level in Autonomy::ALL {
                for ask_first in [false, true] {
                    let (mut x, mut y) = (control(level), control(level));
                    if ask_first {
                        x.key(ch(']'));
                        y.key(ch(']'));
                    }
                    assert_eq!(x.key(a), y.key(b), "{level} ask_first={ask_first}");
                    assert_eq!(x, y, "{level} ask_first={ask_first}");
                }
            }
        };
        both(ch(']'), ch('+'));
        both(ch('['), ch('-'));
        both(ch('y'), k(KeyCode::Enter));
        both(ch('n'), k(KeyCode::Esc));
        // `+` comes with Shift on most layouts.
        both(
            ch('+'),
            KeyEvent::new(KeyCode::Char('+'), KeyModifiers::SHIFT),
        );
    }

    #[test]
    fn the_header_reads_each_state() {
        let hold = |why: &str| header_text(None, Some(&held(why)), None, AT_1405, 0);
        assert_eq!(header_text(None, None, None, AT_1405, 0), "Studio off");
        assert_eq!(
            header_text(Some(Autonomy::A1), None, Some(Autonomy::A2), AT_1405, 0),
            "Raise to A2 Deliver? y confirm · n cancel"
        );
        assert_eq!(
            header_text(
                None,
                Some(&row(Autonomy::A3)),
                Some(Autonomy::A3),
                AT_1405,
                0
            ),
            "Raise to A3 Release? y confirm · n cancel",
            "a raise being asked outranks the hold"
        );
        assert_eq!(
            hold("you typed 40 s ago"),
            "A1 Plan · holding: you typed 40 s ago"
        );
        let mut noted = held("nothing new");
        noted.last_note_at = Some(AT_1405);
        assert_eq!(
            header_text(None, Some(&noted), None, AT_1405, 0),
            "A1 Plan · holding: nothing new",
            "a hold outranks the last note"
        );
        let mut noted = row(Autonomy::A1);
        noted.last_note_at = Some(AT_1405);
        assert_eq!(
            header_text(None, Some(&noted), None, AT_1405, 0),
            "A1 Plan · idle note at 14:05 · 3 new"
        );
        noted.unannounced = 0;
        assert_eq!(
            header_text(None, Some(&noted), None, AT_1405, 0),
            "A1 Plan · idle note at 14:05"
        );
        assert_eq!(
            header_text(None, Some(&row(Autonomy::A1)), None, AT_1405, 0),
            "A1 Plan · ready: 3 new notes"
        );
        assert_eq!(
            header_text(None, Some(&row(Autonomy::A0)), None, AT_1405, 0),
            "A0 Hold · ready: 3 new notes"
        );
    }

    #[test]
    fn the_note_time_is_local_and_says_when_it_is_not_today() {
        let mut noted = row(Autonomy::A2);
        noted.unannounced = 0;
        noted.last_note_at = Some(AT_1405);
        let at = |now: i64, tz: i32| header_text(None, Some(&noted), None, now, tz);
        assert_eq!(at(AT_1405, 13 * 3600), "A2 Deliver · idle note at 03:05");
        assert_eq!(at(AT_1405, -5 * 3600), "A2 Deliver · idle note at 09:05");
        assert_eq!(
            at(AT_1405 + 86_400_000, 0),
            "A2 Deliver · idle note yesterday at 14:05"
        );
        assert_eq!(
            at(AT_1405 + 2 * 86_400_000, 0),
            "A2 Deliver · idle note 2 days ago at 14:05"
        );
    }

    #[test]
    fn one_new_note_is_singular() {
        let mut one = row(Autonomy::A1);
        one.unannounced = 1;
        assert_eq!(
            header_text(None, Some(&one), None, AT_1405, 0),
            "A1 Plan · ready: 1 new note"
        );
    }

    #[test]
    fn only_the_hold_that_needs_you_is_red() {
        let theme = App::new().chrome.theme;
        let line = |why: &str| line_for(theme, None, Some(&held(why)), None, AT_1405, 0);
        let needs = line(NEEDS_YOU);
        assert_eq!(needs.spans[1].style.fg, Some(theme.err));
        assert!(needs.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line("nothing new").spans[1].style.fg, None);
        assert_eq!(
            text(&needs),
            "A1 Plan · holding: the orchestrator needs you"
        );
    }

    /// The header at 80x24: every hold the daemon can say fits whole.
    #[test]
    fn the_header_draws_whole_at_80_columns() {
        let theme = App::new().chrome.theme;
        for why in [
            "studio is off",
            "autonomy A0 (Hold)",
            "no orchestrator is running",
            "the orchestrator is working",
            NEEDS_YOU,
            "you typed 40 s ago",
            "the orchestrator went idle 20 s ago",
            "6 idle notes in the last hour",
            "nothing new",
        ] {
            let line = line_for(theme, None, Some(&held(why)), None, AT_1405, 0);
            let want = format!("A1 Plan · holding: {why}");
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|f| f.render_widget(Paragraph::new(line.clone()), f.area()))
                .unwrap();
            let buf = terminal.backend().buffer();
            let top: String = (0..80).map(|x| buf[(x, 0)].symbol()).collect();
            assert_eq!(top.trim_end(), want);
        }
    }

    /// The one test that touches the module state: the cache is seeded by
    /// hand and the test has no jobs, so nothing is read or written.
    #[test]
    fn the_keys_act_on_the_repo_the_control_was_opened_for() {
        let mut app = App::new();
        app.tree.projects.push(nebula_core::Project {
            id: ProjectId("p".into()),
            name: "wheako".into(),
            repo_path: "/tmp/kete-queue-view-test".into(),
            sort_order: 0,
        });
        let p = ProjectId("p".into());
        *state() = State::EMPTY;

        assert!(!handle_key(&mut app, ch(']')), "no control, no key taken");
        open_autonomy(&mut app, None);
        assert_eq!(app.chrome.flash.as_deref(), Some("Pick a repo first"));
        assert_eq!(
            text(&header_line(&app, Some(&p))),
            "Studio: reading the queue"
        );

        {
            let mut st = state();
            st.rows = vec![row(Autonomy::A2)];
            st.read_at = Some(Instant::now());
        }
        let ready = "A2 Deliver · ready: 3 new notes";
        assert_eq!(text(&header_line(&app, Some(&p))), ready);
        assert_eq!(
            text(&header_line(&app, Some(&ProjectId("other".into())))),
            "Studio off"
        );

        open_autonomy(&mut app, Some(p.clone()));
        assert_eq!(
            app.chrome.flash.as_deref(),
            Some("wheako: A2 (Deliver). [ lower · ] raise, on the studio board, or kete studio autonomy wheako A2")
        );
        assert!(
            !handle_key(&mut app, ch('x')),
            "idle, other keys are the caller's"
        );
        assert!(handle_key(&mut app, ch(']')));
        assert_eq!(
            text(&header_line(&app, Some(&p))),
            "Raise to A3 Release? y confirm · n cancel"
        );
        assert!(handle_key(&mut app, ch('z')), "taken while it asks");
        assert!(handle_key(&mut app, k(KeyCode::Esc)));
        assert_eq!(text(&header_line(&app, Some(&p))), ready);

        // There is no running Kete to ask here: it says so, and the row stands.
        assert!(handle_key(&mut app, ch('-')));
        assert_eq!(
            app.chrome.flash.as_deref(),
            Some("Autonomy can only change in the running Kete")
        );
        assert_eq!(state().rows[0].autonomy, Autonomy::A2);

        // On the board: a raise asks there, and its yes or no is the
        // control's, never the board's `n` note; other keys pass.
        let other = ProjectId("other".into());
        assert!(
            !board_key(&mut app, Some(&p), ch('n')),
            "idle, n is the board's"
        );
        state().control = None;
        assert!(
            board_key(&mut app, Some(&p), ch(']')),
            "] opens the control and raises"
        );
        assert_eq!(
            text(&header_line(&app, Some(&p))),
            "Raise to A3 Release? y confirm · n cancel"
        );
        assert!(
            !board_key(&mut app, Some(&other), ch('n')),
            "another repo's board keeps its keys"
        );
        assert!(
            board_key(&mut app, Some(&p), ch('n')),
            "n answers the raise"
        );
        assert_eq!(text(&header_line(&app, Some(&p))), ready);
        assert!(board_key(&mut app, Some(&p), ch('+')), "+ raises as ] does");
        assert!(
            board_key(&mut app, Some(&p), k(KeyCode::Esc)),
            "Esc drops it"
        );
        assert_eq!(text(&header_line(&app, Some(&p))), ready);
        *state() = State::EMPTY;
    }
}
