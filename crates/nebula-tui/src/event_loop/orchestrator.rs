//! The ORCHESTRATOR VIEW's keys and requests, and the REVIEW MODAL's side
//! effects. Drawing and pure state live in `crate::orchestrator_view` and
//! `crate::review_modal`.

use super::*;
use crate::orchestrator_view::{self as view, StartView};
use crate::review_modal::Loaded;
use nebula_core::ReviewTabKind;

/// The toast a held-back review leaves up until F10 shows it.
pub(crate) const PENDING_REVIEW: &str = "the orchestrator opened a review: F10 to see it";
/// The same, from a locked terminal tab.
pub(crate) const PENDING_REVIEW_FROM_TERMINAL: &str =
    "the orchestrator opened a review: ^q then F10 to see it";

/// Lines of history a review asks for.
const REVIEW_HISTORY_LINES: u32 = 400;

/// F10: show a review the orchestrator held back; else leave the view, or
/// enter it — or, with no orchestrator yet, the prompt that starts one.
pub(crate) fn toggle(app: &mut App, out: &mut Vec<ClientRequest>) {
    app.chrome.dirty = true;
    if let Some((opener, sessions, tabs)) = app.orch.pending_review.take() {
        open_review(app, Some(opener), sessions, tabs, out);
        return;
    }
    if view::active(app) {
        leave_view(app);
        return;
    }
    match view::orchestrator(app).map(|a| a.id.clone()) {
        Some(id) => open_view(app, id, out),
        None => open_start(app),
    }
}

/// The orchestrator's session full-screen, keys in it, strip on top.
pub(crate) fn open_view(app: &mut App, id: AgentId, out: &mut Vec<ClientRequest>) {
    attach_now(app, SessionRef::Agent(id), out);
    app.orch.view = true;
    app.pane.collapsed = true;
    app.pane.term_locked = true;
    app.nav.focus = Focus::Terminal;
    app.chrome.dirty = true;
}

/// The first snapshot: open on the orchestrator's view when the
/// `orchestrator_view` setting says so for this terminal's width.
pub(crate) fn startup_view(app: &mut App, out: &mut Vec<ClientRequest>) {
    // A unit test never reads the developer's own config or terminal.
    #[cfg(not(test))]
    let (cfg, width) = (
        crate::config::Config::load(),
        crossterm::terminal::size().ok().map(|(w, _)| w),
    );
    #[cfg(test)]
    let (cfg, width) = (crate::config::Config::default(), None);
    startup_view_with(app, &cfg, width, out);
}

pub(crate) fn startup_view_with(
    app: &mut App,
    cfg: &crate::config::Config,
    width: Option<u16>,
    out: &mut Vec<ClientRequest>,
) {
    if view::active(app) || !cfg.opens_on_orchestrator(width) {
        return;
    }
    if let Some(id) = view::orchestrator(app).map(|a| a.id.clone()) {
        open_view(app, id, out);
    }
}

/// Back to the grid.
fn leave_view(app: &mut App) {
    app.orch.view = false;
    leave_terminal_lock(app);
}

/// Flash why not, when the daemon is too old to take a provisioning
/// request: a protocol-53 daemon cannot decode one and would drop the
/// connection.
fn refuse_old_daemon(app: &mut App) -> bool {
    if view::daemon_provisions(app) {
        return false;
    }
    app.chrome.flash = Some(view::DAEMON_TOO_OLD.into());
    app.chrome.dirty = true;
    true
}

fn open_start(app: &mut App) {
    if refuse_old_daemon(app) {
        return;
    }
    let cfg = crate::config::Config::load();
    // Each harness's configured model and effort, as a new session gets.
    let configured = nebula_core::AgentKind::ALL
        .into_iter()
        .map(|k| (k, cfg.default_model(k), cfg.default_effort(k)))
        .collect();
    let projects = app
        .tree
        .projects
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    app.modals.overlay = Some(Overlay::OrchestratorStart(
        StartView::new(cfg.profiles.clone()).with(configured, projects),
    ));
}

/// The start prompt's keys: ↑/↓ (or Tab) pick the row, ←/→ its value,
/// Enter starts, Esc cancels. On the name row the letters type.
pub(crate) fn handle_start_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let Some(Overlay::OrchestratorStart(start)) = &mut app.modals.overlay else {
        return;
    };
    let typing = start.focused() == crate::orchestrator_view::StartRow::Name;
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => app.modals.overlay = None,
        KeyCode::Up | KeyCode::BackTab => start.move_row(-1),
        KeyCode::Down | KeyCode::Tab => start.move_row(1),
        KeyCode::Enter => {
            let start = start.clone();
            start_orchestrator(app, &start, out);
        }
        KeyCode::Backspace if typing => {
            start.name.pop();
        }
        KeyCode::Char(c) if typing && plain => start.name.push(c),
        KeyCode::Char('k') if !typing => start.move_row(-1),
        KeyCode::Char('j') if !typing => start.move_row(1),
        KeyCode::Left | KeyCode::Char('h') if !typing => start.step(-1),
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') if !typing => start.step(1),
        _ => {}
    }
    app.chrome.dirty = true;
}

/// A click on the start prompt: a row takes the keys, its ‹ or › steps
/// it, and the Start button is Enter.
pub(crate) fn handle_start_mouse(
    app: &mut App,
    mouse: MouseEvent,
    out: &mut Vec<ClientRequest>,
) -> bool {
    let Some(Overlay::OrchestratorStart(start)) = &mut app.modals.overlay else {
        return false;
    };
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return true;
    }
    let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
    app.chrome.dirty = true;
    if start.start_hit.contains(pos) {
        let start = start.clone();
        start_orchestrator(app, &start, out);
        return true;
    }
    if let Some((rect, row)) = start
        .row_hits
        .iter()
        .find(|(r, _)| r.contains(pos))
        .copied()
    {
        start.row = row;
        // The value sits after the label: its left end steps back, its
        // right end on.
        let value_x = rect.x + 11;
        if mouse.column >= value_x {
            let mid = value_x + (rect.right().saturating_sub(value_x)) / 2;
            start.step(if mouse.column < mid { -1 } else { 1 });
        }
    }
    true
}

fn start_orchestrator(app: &mut App, start: &StartView, out: &mut Vec<ClientRequest>) {
    let kind = start.kind();
    let profile = start.profile_name();
    app.modals.overlay = None;
    if refuse_old_daemon(app) {
        return;
    }
    // The rows start on the harness's configured model and effort.
    let (model, effort) = (start.model_name(), start.effort_name());
    app.chrome.flash = Some(format!("starting the orchestrator on {}…", kind.as_str()));
    send_with(app, out, PendingIntent::OpenOrchestrator, |req_id| {
        ClientRequest::StartOrchestrator {
            req_id,
            kind,
            custom_harness: None,
            model,
            effort,
            profile,
            project: start.project_id(),
            name: start.name_value(),
            replace: false,
            role_card: None,
        }
    });
}

/// The view's own keys, with the pane unlocked (`^Q` from typing): ←/→
/// walk the chips, Enter reviews the chip, Esc goes back to typing, and
/// `^Q` again leaves for the grid. Everything else falls through.
pub(crate) fn handle_view_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) -> bool {
    if !view::active(app) || app.pane.term_locked || app.modals.overlay.is_some() {
        return false;
    }
    // The TRIAGE STRIP (0.60) takes the keys while its line is open.
    if crate::triage_strip::handle_key(app, key) {
        app.chrome.dirty = true;
        return true;
    }
    let chips = view::chips(app);
    let chord = crate::keymap::KeyChord::from_event(&key);
    match key.code {
        _ if chord == HARDWIRED_UNLOCK => leave_view(app),
        // `t`: one line for the studio's triage desk (`triage_strip`).
        KeyCode::Char('t') => crate::triage_strip::open(app),
        KeyCode::Left | KeyCode::Char('h') if !chips.is_empty() => {
            app.orch.chip = (app.orch.chip + chips.len() - 1) % chips.len();
        }
        KeyCode::Right | KeyCode::Char('l') if !chips.is_empty() => {
            app.orch.chip = (app.orch.chip + 1) % chips.len();
        }
        KeyCode::Enter if !chips.is_empty() => {
            let sref = chips[app.orch.chip.min(chips.len() - 1)].sref.clone();
            open_review(app, None, vec![sref], Vec::new(), out);
        }
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('i') => {
            app.pane.term_locked = true;
            app.nav.focus = Focus::Terminal;
        }
        _ => return false,
    }
    app.chrome.dirty = true;
    true
}

/// In the view, the unlock hatch steps out to the strip rather than out
/// of full screen: the keys leave the orchestrator, the chips take them.
pub(crate) fn unlock_to_strip(app: &mut App) -> bool {
    if !view::active(app) {
        return false;
    }
    app.pane.term_locked = false;
    app.nav.focus = Focus::Terminal;
    app.chrome.dirty = true;
    true
}

/// A chip clicked: review that session.
pub(crate) fn click_chip(app: &mut App, sref: SessionRef, out: &mut Vec<ClientRequest>) {
    open_review(app, None, vec![sref], Vec::new(), out);
}

/// "Make orchestrator" on a card.
pub(crate) fn make_orchestrator(app: &mut App, id: AgentId, out: &mut Vec<ClientRequest>) {
    if refuse_old_daemon(app) {
        return;
    }
    let name = app
        .tree
        .agents
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.name.clone())
        .unwrap_or_default();
    // Said when the daemon has done it; a refusal flashes its own reason.
    let note = format!("{name} is the orchestrator now: F10 opens it");
    send_with(app, out, PendingIntent::FlashOnAck(note), |req_id| {
        ClientRequest::SetOrchestrator { req_id, agent: id }
    });
}

/// "Unpin orchestrator" on the orchestrator's card.
pub(crate) fn clear_orchestrator(app: &mut App, out: &mut Vec<ClientRequest>) {
    if refuse_old_daemon(app) {
        return;
    }
    let note = "no session is the orchestrator now".to_string();
    send_with(app, out, PendingIntent::FlashOnAck(note), |req_id| {
        ClientRequest::ClearOrchestrator { req_id }
    });
    app.orch.view = false;
}

/// `ReviewOpened` from the daemon. Up at once, unless the user is typing
/// into a locked pane (the orchestrator's own included: the request is
/// often the answer to what they just typed, and the rest of the line must
/// not land in the modal) or has another modal up. Then it waits, with a
/// toast, for F10. Opened at once, it ignores Enter and a click on the
/// screen for a moment ([`UNSOLICITED_GUARD`]).
pub(crate) fn review_opened(
    app: &mut App,
    opener: AgentId,
    sessions: Vec<SessionRef>,
    tabs: Vec<ReviewTabKind>,
    out: &mut Vec<ClientRequest>,
) {
    let typing =
        app.nav.focus == Focus::Terminal && app.pane.term_locked && app.pane.term.is_some();
    let busy = app.modals.overlay.is_some() || app.pane.vim.is_some();
    if typing || busy {
        app.orch.pending_review = Some((opener, sessions, tabs));
        app.chrome.flash = Some(pending_review_toast(app).into());
        app.chrome.dirty = true;
        return;
    }
    open_review(app, Some(opener), sessions, tabs, out);
    if let Some(Overlay::Review(review)) = &mut app.modals.overlay {
        review.guard_until = Some(std::time::Instant::now() + UNSOLICITED_GUARD);
    }
}

/// How long an unsolicited review ignores Enter and a click on the screen.
const UNSOLICITED_GUARD: std::time::Duration = std::time::Duration::from_millis(750);

/// The toast a held-back review shows: F10 where it reaches, `^q` first
/// from a locked terminal tab, where F10 is the shell program's.
pub(crate) fn pending_review_toast(app: &App) -> &'static str {
    let in_terminal_tab = app.nav.focus == Focus::Terminal
        && app.pane.term_locked
        && app
            .pane
            .term
            .as_ref()
            .is_some_and(|t| matches!(t.sref, SessionRef::Terminal(_)));
    if in_terminal_tab {
        PENDING_REVIEW_FROM_TERMINAL
    } else {
        PENDING_REVIEW
    }
}

/// The review modal up, remembering what the pane showed so closing it
/// puts that back.
pub(crate) fn open_review(
    app: &mut App,
    opener: Option<AgentId>,
    sessions: Vec<SessionRef>,
    tabs: Vec<ReviewTabKind>,
    out: &mut Vec<ClientRequest>,
) {
    if sessions.is_empty() {
        return;
    }
    if app.orch.review_restore.is_none() {
        app.orch.review_restore = Some(app.pane.term.as_ref().map(|t| t.sref.clone()));
    }
    crate::review_modal::open(app, opener, sessions, tabs);
    settle_review(app, out);
}

/// After every input and every review opened: carry out what the review
/// modal asked for — the pane on the session under review, its diff or
/// history fetched, a jump into it, a PR opened — and, once the modal is
/// gone, put the pane back on what it showed before.
pub(crate) fn settle_review(app: &mut App, out: &mut Vec<ClientRequest>) {
    let Some(Overlay::Review(review)) = &mut app.modals.overlay else {
        match app.orch.review_restore.take() {
            Some(Some(sref)) if !pane_shows(app, &sref) => attach_now(app, sref, out),
            // The pane showed nothing before the review: nothing again.
            Some(None) if app.pane.term.is_some() => detach_pane(app, out),
            _ => {}
        }
        return;
    };
    if let Some(sref) = review.jump.take() {
        app.modals.overlay = None;
        app.orch.review_restore = None;
        app.orch.view = false;
        attach_now(app, sref, out);
        app.nav.focus = Focus::Terminal;
        app.pane.term_locked = true;
        app.chrome.dirty = true;
        return;
    }
    let open_url = review.open_url.take();
    let Some(sref) = review.selected_session().cloned() else {
        return;
    };
    let tab = review.selected_tab();
    let wants_diff = tab == Some(ReviewTabKind::Diff) && !review.diffs.contains_key(&sref);
    let wants_history =
        tab == Some(ReviewTabKind::History) && !review.histories.contains_key(&sref);
    if wants_diff {
        review.diffs.insert(sref.clone(), Loaded::Loading);
    }
    if wants_history {
        review.histories.insert(sref.clone(), Loaded::Loading);
    }
    if let Some(url) = open_url {
        open_link(app, &url, out);
    }
    if tab == Some(ReviewTabKind::Terminal) && !pane_shows(app, &sref) {
        attach_now(app, sref.clone(), out);
    }
    if wants_diff {
        request_diff(app, sref.clone());
    }
    if wants_history {
        let req_id = app.requests.next_req_id;
        app.requests.next_req_id += 1;
        if let Some(Overlay::Review(review)) = &mut app.modals.overlay {
            review.history_reqs.insert(req_id, sref.clone());
        }
        out.push(ClientRequest::ReadSession {
            req_id,
            target: session_target(&sref),
            max_lines: REVIEW_HISTORY_LINES,
        });
    }
}

fn pane_shows(app: &App, sref: &SessionRef) -> bool {
    app.pane.term.as_ref().is_some_and(|t| &t.sref == sref)
}

/// How `ReadSession` names a session: an agent by id, a terminal as
/// `terminal:<id>` (the `kete sessions` id).
fn session_target(sref: &SessionRef) -> String {
    match sref {
        SessionRef::Agent(id) => id.to_string(),
        SessionRef::Terminal(id) => format!("terminal:{id}"),
    }
}

/// The checkout a session runs in.
fn session_worktree(app: &App, sref: &SessionRef) -> Option<nebula_core::Worktree> {
    let wid = match sref {
        SessionRef::Agent(id) => app
            .tree
            .agents
            .iter()
            .find(|a| &a.id == id)?
            .worktree_id
            .clone(),
        SessionRef::Terminal(id) => app
            .tree
            .terminals
            .iter()
            .find(|t| &t.id == id)?
            .worktree_id
            .clone(),
    };
    app.tree.worktrees.iter().find(|w| w.id == wid).cloned()
}

/// Read the session's worktree diff off the loop (inline with none, as in
/// unit tests).
fn request_diff(app: &mut App, sref: SessionRef) {
    let Some(worktree) = session_worktree(app, &sref) else {
        land_review_diff(app, sref, Err("the session's checkout is not known".into()));
        return;
    };
    let path = worktree.path.clone();
    match app.jobs.view_jobs.clone() {
        Some(jobs) => jobs.run(move || {
            Some(crate::view_jobs::Answer::ReviewDiff {
                session: sref,
                result: crate::review_modal::read_worktree_diff(&path),
            })
        }),
        None => land_review_diff(app, sref, crate::review_modal::read_worktree_diff(&path)),
    }
}

/// A review's diff, read.
pub(crate) fn land_review_diff(app: &mut App, sref: SessionRef, result: Result<String, String>) {
    if let Some(Overlay::Review(review)) = &mut app.modals.overlay {
        review.diffs.insert(
            sref,
            match result {
                Ok(text) => Loaded::Ready(text),
                Err(why) => Loaded::Failed(why),
            },
        );
        app.chrome.dirty = true;
    }
}

/// `SessionText` for the review's history; true when it was that.
pub(crate) fn land_review_history(app: &mut App, req_id: u64, text: String) -> bool {
    land_history(app, req_id, Loaded::Ready(text))
}

/// An `Error` for the review's history; true when it was that.
pub(crate) fn fail_review_history(app: &mut App, req_id: u64, message: &str) -> bool {
    land_history(app, req_id, Loaded::Failed(message.to_string()))
}

fn land_history(app: &mut App, req_id: u64, loaded: Loaded) -> bool {
    let Some(Overlay::Review(review)) = &mut app.modals.overlay else {
        return false;
    };
    match review.history_reqs.remove(&req_id) {
        Some(sref) => {
            review.histories.insert(sref, loaded);
            app.chrome.dirty = true;
            true
        }
        None => false,
    }
}
