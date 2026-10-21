//! The STUDIO BOARD overlay (K4): a studio repo's departments, one row
//! each, with its queue, lead, runners, last verdict and spend.
//!
//! Read-only (decision 11). The file half (departments, notes, verdicts,
//! refusals, a failed renewal) is a BACKGROUND READ through
//! `nebula_core::studio::board::read_files`: a job drops what it found in
//! the view's inbox and wakes the loop with a redraw, and the next draw
//! or key lands it ([`sync`]); every landed read queues the next one
//! [`REFRESH`] later. The live half (sessions, roles, spend, contexts) is
//! what the app already holds, joined on every draw (`board::build`). A
//! view with no `Jobs` (the unit tests) reads inline, once.
//!
//! Keys: `↑` `↓` rows; Enter expands a department (or What needs you),
//! attaches a session, opens a note in the file view, or opens the
//! gauntlet view on a verdict; `l` the department's lead, `v` its last
//! verdict, `n` its top note; `m` the row's menu; `r` reloads; Esc or `q`
//! closes. A click on a row is Enter on it; a right-click is its menu.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::{App, ContextMenu, MenuAction, MenuItem, Overlay};
use crate::view_jobs::{Answer, Jobs};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use nebula_core::studio::board::{self, Board, BoardFiles, BoardInputs, LiveSession, NeedsYou};
use nebula_core::{AgentId, ClientRequest, ProjectId, SessionRef};
use ratatui::layout::{Position, Rect};

/// How long a landed read waits before the next one.
pub const REFRESH: Duration = Duration::from_secs(10);
/// An expanded department lists at most this many of its notes.
pub const MAX_NOTES_SHOWN: usize = 10;

/// A read that finished, by the ticket that asked for it.
#[derive(Debug)]
struct Landed {
    ticket: u64,
    files: BoardFiles,
}

/// One row of the board, by what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// The What needs you header.
    NeedsYou,
    /// One entry of What needs you, by its index.
    Need(usize),
    /// A department, by slug.
    Dept(String),
    /// A note of a department: (slug, note id).
    Note(String, String),
    /// A session of a department: (slug, agent).
    Session(String, AgentId),
    /// A department's last verdict, by slug.
    Verdict(String),
}

impl Row {
    /// The department a row belongs to.
    fn dept(&self) -> Option<&str> {
        match self {
            Row::Dept(d) | Row::Note(d, _) | Row::Session(d, _) | Row::Verdict(d) => Some(d),
            Row::NeedsYou | Row::Need(_) => None,
        }
    }
}

/// The key the What needs you group is expanded under.
pub const NEEDS_KEY: &str = "@needs";

/// The board overlay's state.
#[derive(Debug, Clone, Default)]
pub struct BoardView {
    /// The repo it shows; None: the first studio repo.
    pub repo: Option<ProjectId>,
    /// Where it last drew, for outside clicks.
    pub area: Rect,
    /// The repo's name, for the title.
    pub name: String,
    /// The repo's root; None when there is no studio repo to show.
    pub root: Option<PathBuf>,
    /// None until the first read lands. Shared, so the per-frame clone of
    /// the overlay copies a pointer, not every note.
    pub files: Option<Arc<BoardFiles>>,
    /// Expanded departments by slug, and [`NEEDS_KEY`].
    pub expanded: BTreeSet<String>,
    pub cursor: usize,
    /// The rows' rect and first row, as last drawn, for clicks.
    pub list_area: Rect,
    pub scroll: usize,
    /// The read the view waits on; an older one landing is dropped.
    ticket: u64,
    inbox: Arc<Mutex<Option<Landed>>>,
}

/// A fresh view on `repo`, or on the first studio repo.
fn new_view(app: &App, repo: Option<ProjectId>) -> BoardView {
    let project = match &repo {
        Some(id) => app.tree.projects.iter().find(|p| &p.id == id),
        None => {
            let studio = app.studio_repos.get(&app.tree.projects);
            app.tree.projects.iter().find(|p| studio.contains(&p.id))
        }
    };
    BoardView {
        repo: project.map(|p| p.id.clone()).or(repo),
        name: project.map(|p| p.name.clone()).unwrap_or_default(),
        root: project.map(|p| p.repo_path.clone()),
        ..BoardView::default()
    }
}

/// Open the board, on `repo` when the verb came from one.
pub fn open(app: &mut App, repo: Option<ProjectId>) {
    let view = new_view(app, repo);
    app.modals.overlay = Some(Overlay::StudioBoard(view));
    let jobs = app.jobs.view_jobs.clone();
    if let Some(Overlay::StudioBoard(view)) = &mut app.modals.overlay {
        read(view, jobs, Duration::ZERO);
    }
    app.chrome.dirty = true;
}

/// Read the file half after `delay`: on the blocking pool when there is
/// one, else now. A job whose view has closed reads nothing.
fn read(view: &mut BoardView, jobs: Option<Jobs>, delay: Duration) {
    let Some(root) = view.root.clone() else {
        return;
    };
    view.ticket = crate::view_jobs::ticket();
    let ticket = view.ticket;
    match jobs {
        Some(jobs) => {
            let inbox = Arc::downgrade(&view.inbox);
            jobs.run(move || {
                // shortcut: the 10 s beat sleeps on a blocking-pool thread,
                // since no loop arm is the board's; a loop tick would free it.
                std::thread::sleep(delay);
                let inbox = inbox.upgrade()?;
                let files = board::read_files(&root);
                *inbox.lock().ok()? = Some(Landed { ticket, files });
                Some(Answer::Slow {
                    ticket: crate::view_jobs::ticket(),
                })
            });
        }
        None => {
            if let Ok(mut slot) = view.inbox.lock() {
                *slot = Some(Landed {
                    ticket,
                    files: board::read_files(&root),
                });
            }
            sync(None, view);
        }
    }
}

/// Land a finished read, and queue the next one. Every draw and key runs it.
pub(crate) fn sync(jobs: Option<Jobs>, view: &mut BoardView) {
    let landed = view.inbox.lock().ok().and_then(|mut slot| slot.take());
    let Some(landed) = landed.filter(|l| l.ticket == view.ticket) else {
        return;
    };
    view.files = Some(Arc::new(landed.files));
    if jobs.is_some() {
        read(view, jobs, REFRESH);
    }
}

/// The repo's live sessions, as the board joins them.
fn live_sessions(app: &App, repo: Option<&ProjectId>) -> Vec<LiveSession> {
    let worktrees: HashSet<_> = app
        .tree
        .worktrees
        .iter()
        .filter(|w| Some(&w.project_id) == repo)
        .map(|w| &w.id)
        .collect();
    app.tree
        .agents
        .iter()
        .filter(|a| !a.archived && a.alive && worktrees.contains(&a.worktree_id))
        .map(|a| LiveSession {
            agent: a.id.clone(),
            name: a.name.clone(),
            status: a.status,
            model: a.model.clone(),
            profile: a.profile.clone(),
            context_tokens: app
                .launcher
                .session_contexts
                .get(&a.id)
                .and_then(|c| c.tokens),
        })
        .collect()
}

/// The board as it stands: the last read joined with the live half.
pub fn board_of(app: &App, view: &BoardView) -> Option<Board> {
    let files = view.files.as_deref()?;
    let sessions = live_sessions(app, view.repo.as_ref());
    let inputs = BoardInputs {
        files,
        sessions: &sessions,
        roles: &app.session_roles,
        spend: &app.session_spend,
    };
    Some(board::build(&inputs, crate::app::now_ms()))
}

/// The rows `board` shows with `expanded` open.
pub fn rows(board: &Board, expanded: &BTreeSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    if !board.needs_you.is_empty() {
        out.push(Row::NeedsYou);
        if expanded.contains(NEEDS_KEY) {
            out.extend((0..board.needs_you.len()).map(Row::Need));
        }
    }
    for c in &board.columns {
        let slug = &c.department.slug;
        out.push(Row::Dept(slug.clone()));
        if !expanded.contains(slug) {
            continue;
        }
        out.extend(
            c.queue
                .iter()
                .take(MAX_NOTES_SHOWN)
                .map(|n| Row::Note(slug.clone(), n.id.clone())),
        );
        out.extend(
            c.lead
                .iter()
                .chain(&c.runners)
                .map(|s| Row::Session(slug.clone(), s.agent.clone())),
        );
        if c.last_verdict.is_some() {
            out.push(Row::Verdict(slug.clone()));
        }
    }
    out
}

/// The board, its rows and the row under the cursor, for a key or a click.
fn current(app: &App) -> Option<(Board, Vec<Row>, Option<Row>)> {
    let Some(Overlay::StudioBoard(view)) = &app.modals.overlay else {
        return None;
    };
    let board = board_of(app, view)?;
    let rows = rows(&board, &view.expanded);
    let at = rows.get(view.cursor).cloned();
    Some((board, rows, at))
}

fn view_mut(app: &mut App) -> Option<&mut BoardView> {
    match &mut app.modals.overlay {
        Some(Overlay::StudioBoard(v)) => Some(v),
        _ => None,
    }
}

/// Enter on the cursor's row, or a click on it.
pub(crate) fn activate(app: &mut App, out: &mut Vec<ClientRequest>) {
    let Some((board, _, Some(row))) = current(app) else {
        return;
    };
    match row {
        Row::NeedsYou => toggle(app, NEEDS_KEY),
        Row::Dept(slug) => toggle(app, &slug),
        Row::Need(i) => match board.needs_you.get(i) {
            Some(NeedsYou::Session(c)) => attach(app, c.agent.clone(), out),
            Some(NeedsYou::Note(n)) => open_note(app, n.path.clone()),
            Some(NeedsYou::Refusal(r)) => {
                app.chrome.flash = Some(format!("Refused: {}", r.reason));
            }
            Some(NeedsYou::Renewal(r)) => {
                app.chrome.flash = Some(format!("Renewal failed: {}", r.reason));
            }
            None => {}
        },
        Row::Note(slug, id) => {
            let path = board
                .columns
                .iter()
                .find(|c| c.department.slug == slug)
                .and_then(|c| c.queue.iter().find(|n| n.id == id))
                .map(|n| n.path.clone());
            if let Some(path) = path {
                open_note(app, path);
            }
        }
        Row::Session(_, agent) => attach(app, agent, out),
        Row::Verdict(slug) => review(app, &board, &slug),
    }
}

fn toggle(app: &mut App, key: &str) {
    if let Some(view) = view_mut(app) {
        if !view.expanded.remove(key) {
            view.expanded.insert(key.to_string());
        }
    }
}

/// Close the board and attach `agent`.
fn attach(app: &mut App, agent: AgentId, out: &mut Vec<ClientRequest>) {
    app.modals.overlay = None;
    crate::event_loop::open_session(app, SessionRef::Agent(agent), out);
}

/// The note in the FILE TABS, in place of the board.
fn open_note(app: &mut App, path: PathBuf) {
    let root = match view_mut(app) {
        Some(view) => view.root.clone().unwrap_or_default(),
        None => return,
    };
    crate::file_tabs::open(app, root, vec![path]);
}

/// The gauntlet view on `slug`'s last verdict.
fn review(app: &mut App, board: &Board, slug: &str) {
    let Some(v) = board
        .columns
        .iter()
        .find(|c| c.department.slug == slug)
        .and_then(|c| c.last_verdict.as_ref())
    else {
        app.chrome.flash = Some("No verdict for this department yet".into());
        return;
    };
    let repo = view_mut(app).and_then(|view| view.repo.clone());
    crate::gauntlet_view::open_on(app, repo, &v.kind, &v.id);
}

/// `l`, `v`, `n`: the lead, the last verdict or the top note of the
/// cursor's department.
fn dept_action(app: &mut App, key: char, out: &mut Vec<ClientRequest>) {
    let Some((board, _, Some(row))) = current(app) else {
        return;
    };
    let Some(col) = row
        .dept()
        .and_then(|d| board.columns.iter().find(|c| c.department.slug == d))
    else {
        return;
    };
    match key {
        'l' => match &col.lead {
            Some(lead) => attach(app, lead.agent.clone(), out),
            None => app.chrome.flash = Some("This department has no lead running".into()),
        },
        'v' => review(app, &board, &col.department.slug.clone()),
        _ => match col.queue.first() {
            Some(n) => open_note(app, n.path.clone()),
            None => app.chrome.flash = Some("This department's queue is empty".into()),
        },
    }
}

/// A menu row that presses `code` in the board beneath it.
fn key_row(label: &str, code: KeyCode) -> MenuItem {
    MenuItem {
        key: Some(match code {
            KeyCode::Enter => "Enter".into(),
            KeyCode::Char(c) => c.to_string(),
            _ => String::new(),
        }),
        ..MenuItem::new(label, MenuAction::OverlayKey(code, KeyModifiers::NONE))
    }
}

/// The cursor row's menu: only the rows that apply to it now.
pub fn menu_rows(board: &Board, row: &Row, expanded: &BTreeSet<String>) -> (String, Vec<MenuItem>) {
    let col = row
        .dept()
        .and_then(|d| board.columns.iter().find(|c| c.department.slug == d));
    let mut items = Vec::new();
    let title = match (row, col) {
        (Row::NeedsYou, _) => {
            let open = expanded.contains(NEEDS_KEY);
            items.push(key_row(
                if open { "Collapse" } else { "Expand" },
                KeyCode::Enter,
            ));
            "What needs you".to_string()
        }
        (Row::Need(i), _) => {
            match board.needs_you.get(*i) {
                Some(NeedsYou::Session(_)) => items.push(key_row("Open session", KeyCode::Enter)),
                Some(NeedsYou::Note(_)) => items.push(key_row("Open note", KeyCode::Enter)),
                Some(NeedsYou::Refusal(_) | NeedsYou::Renewal(_)) => {
                    items.push(key_row("Show reason", KeyCode::Enter))
                }
                None => {}
            }
            "What needs you".to_string()
        }
        (_, Some(col)) => {
            match row {
                Row::Dept(slug) => {
                    let open = expanded.contains(slug);
                    items.push(key_row(
                        if open { "Collapse" } else { "Expand" },
                        KeyCode::Enter,
                    ));
                }
                Row::Note(..) => items.push(key_row("Open note", KeyCode::Enter)),
                Row::Session(..) => items.push(key_row("Open session", KeyCode::Enter)),
                Row::Verdict(_) => items.push(key_row("Review verdict", KeyCode::Enter)),
                _ => {}
            }
            let on_lead = matches!(row, Row::Session(_, a) if col.lead.as_ref().is_some_and(|l| &l.agent == a));
            if col.lead.is_some() && !on_lead {
                items.push(key_row("Open lead", KeyCode::Char('l')));
            }
            if col.last_verdict.is_some() && !matches!(row, Row::Verdict(_)) {
                items.push(key_row("Review last verdict", KeyCode::Char('v')));
            }
            if !col.queue.is_empty() && !matches!(row, Row::Note(..)) {
                items.push(key_row("Open top note", KeyCode::Char('n')));
            }
            col.department.label.clone()
        }
        _ => String::new(),
    };
    items.push(key_row("Reload", KeyCode::Char('r')));
    (title, items)
}

/// Put the cursor row's menu up over the board, which waits beneath it.
fn open_menu(app: &mut App, at: Option<(u16, u16)>) {
    let Some((board, _, Some(row))) = current(app) else {
        return;
    };
    let expanded = view_mut(app)
        .map(|v| v.expanded.clone())
        .unwrap_or_default();
    let (title, items) = menu_rows(&board, &row, &expanded);
    let under = app.modals.overlay.take().map(Box::new);
    app.modals.overlay = Some(Overlay::Menu(ContextMenu {
        title: Some(title),
        items,
        at,
        hover: 0,
        area: Rect::default(),
        scroll: 0,
        parent: None,
        filter: None,
    }));
    app.modals.verbs_under = under;
}

fn reload(app: &mut App) {
    let jobs = app.jobs.view_jobs.clone();
    if let Some(view) = view_mut(app) {
        read(view, jobs, Duration::ZERO);
    }
}

/// Move the cursor by `by`, kept on a row.
fn step(app: &mut App, by: i64) {
    let n = current(app).map_or(0, |(_, rows, _)| rows.len());
    if let Some(view) = view_mut(app) {
        let last = n.saturating_sub(1) as i64;
        view.cursor = (view.cursor as i64 + by).clamp(0, last) as usize;
    }
}

/// Keys over the board.
pub fn handle_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let jobs = app.jobs.view_jobs.clone();
    let Some(view) = view_mut(app) else {
        return;
    };
    sync(jobs, view);
    let page = i64::from(view.list_area.height.max(1));
    app.chrome.dirty = true;
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.modals.overlay = None,
        KeyCode::Up | KeyCode::Char('k') => step(app, -1),
        KeyCode::Down | KeyCode::Char('j') => step(app, 1),
        KeyCode::PageUp => step(app, -page),
        KeyCode::PageDown => step(app, page),
        KeyCode::Home => step(app, i64::MIN / 2),
        KeyCode::End => step(app, i64::MAX / 2),
        KeyCode::Enter => activate(app, out),
        KeyCode::Char(c @ ('l' | 'v' | 'n')) => dept_action(app, c, out),
        KeyCode::Char('m') => open_menu(app, None),
        KeyCode::Char('r') => reload(app),
        _ => {}
    }
}

/// The pointer over the board: a click on a row is Enter on it, a
/// right-click its menu, the wheel moves the cursor. True: it took it.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) -> bool {
    let jobs = app.jobs.view_jobs.clone();
    let Some(view) = view_mut(app) else {
        return false;
    };
    sync(jobs, view);
    let pos = Position::new(mouse.column, mouse.row);
    let hit = view
        .list_area
        .contains(pos)
        .then(|| view.scroll + usize::from(pos.y - view.list_area.y));
    app.chrome.dirty = true;
    let n = current(app).map_or(0, |(_, rows, _)| rows.len());
    let hit = hit.filter(|&i| i < n);
    match mouse.kind {
        MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) => {
            let Some(i) = hit else {
                return true;
            };
            if let Some(view) = view_mut(app) {
                view.cursor = i;
            }
            if button == MouseButton::Left {
                activate(app, out);
            } else {
                open_menu(app, Some((mouse.column, mouse.row)));
            }
        }
        MouseEventKind::ScrollDown => step(app, 1),
        MouseEventKind::ScrollUp => step(app, -1),
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};
    use nebula_core::studio::wire::{SessionRole, SpendRow};
    use nebula_core::{Agent, AgentStatus, Project, Worktree, WorktreeId};
    use ratatui::backend::TestBackend;
    use std::path::Path;

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn agent(id: &str, name: &str, status: AgentStatus) -> Agent {
        let mut a: Agent = serde_json::from_value(serde_json::json!({
            "id": id, "worktree_id": "w1", "name": name, "status": status,
            "archived": false, "archived_at": 0, "unseen": false,
            "status_changed_at": 0, "kind": "claude", "custom_harness": null,
            "model": "opus", "effort": null, "session_id": null,
            "cloud_session_id": null, "issue_url": null, "role": "worker",
            "sort_order": 0, "alive": true,
        }))
        .expect("an agent row");
        a.profile = Some("main".into());
        a
    }

    fn role(id: &str, label: &str, tier: u8, dept: Option<&str>) -> SessionRole {
        SessionRole {
            agent: AgentId(id.into()),
            card: label.into(),
            label: label.into(),
            tier,
            department: dept.map(str::to_string),
            permission: "allowlist".into(),
            context_budget: Some(200_000),
        }
    }

    /// A studio repo with three departments, notes, a verdict, and three
    /// live sessions, as the one project of an app with the board open.
    fn fixture() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        put(
            r,
            ".kete/studio.toml",
            "version = 1\ninbox = \"notes\"\ndepartments = \"DEPTS.md\"\n\
             signoffs = \"signoffs\"\n[kinds]\nsound = \"audio\"\n",
        );
        put(
            r,
            "DEPTS.md",
            "# Departments\n\n| Department | Owns |\n|---|---|\n| Audio | sound |\n\
             | Art and 3D | models |\n| Animation (sub-department of Art and 3D) | rigs |\n",
        );
        put(
            r,
            "notes/storm.md",
            "---\nstatus: open\nto: audio\npriority: high\ntitle: Storm bed too loud\n---\n",
        );
        put(
            r,
            "notes/wind.md",
            "---\nstatus: new\nto: audio\ntitle: Wind loop seam\n---\n",
        );
        put(
            r,
            "notes/ask.md",
            "---\nstatus: open\nto: jay\nabout: Audio\ntitle: Pick the bell\n---\n",
        );
        put(
            r,
            "signoffs/sound/S01.json",
            r#"{"verdict":"approve","at":"2026-10-20"}"#,
        );
        let mut app = App::new();
        app.tree.projects.push(Project {
            id: ProjectId("p1".into()),
            name: "game".into(),
            repo_path: r.to_path_buf(),
            sort_order: 0,
        });
        app.tree.worktrees.push(Worktree {
            id: WorktreeId("w1".into()),
            project_id: ProjectId("p1".into()),
            path: r.to_path_buf(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        });
        app.tree
            .agents
            .push(agent("a1", "audio lead", AgentStatus::Running));
        app.tree
            .agents
            .push(agent("a2", "runner", AgentStatus::NeedsFeedback));
        app.tree
            .agents
            .push(agent("a3", "loose", AgentStatus::Finished));
        app.session_roles.insert(
            AgentId("a1".into()),
            role("a1", "lead audio", 1, Some("audio")),
        );
        app.session_roles
            .insert(AgentId("a2".into()), role("a2", "runner", 3, Some("audio")));
        app.launcher.session_contexts.insert(
            AgentId("a1".into()),
            serde_json::from_value(serde_json::json!({
                "percent": 70.0, "tokens": 130_000, "window": 200_000, "compactions": 0,
                "last_compact_at": null, "source": "transcript", "at": 0,
            }))
            .expect("a context reading"),
        );
        let today = nebula_core::usage_ledger::local_day(crate::app::now_ms() / 1000);
        app.session_spend.insert(
            AgentId("a1".into()),
            SpendRow {
                agent: AgentId("a1".into()),
                day: today,
                input_tokens: 1_000,
                output_tokens: 500,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_usd: 1.25,
                tool_uses: 3,
                turns: 2,
            },
        );
        open(&mut app, Some(ProjectId("p1".into())));
        (dir, app)
    }

    fn view(app: &App) -> &BoardView {
        match &app.modals.overlay {
            Some(Overlay::StudioBoard(v)) => v,
            other => panic!("the board is not open: {other:?}"),
        }
    }

    fn key(app: &mut App, code: KeyCode) -> Vec<ClientRequest> {
        let ev = KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let mut out = Vec::new();
        handle_key(app, ev, &mut out);
        out
    }

    fn draw(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| crate::ui::studio_board::draw(f, app, f.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn rows_of(app: &App) -> Vec<Row> {
        let v = view(app);
        rows(&board_of(app, v).unwrap(), &v.expanded)
    }

    fn cursor_to(app: &mut App, row: &Row) {
        let i = rows_of(app).iter().position(|r| r == row).expect("the row");
        if let Some(v) = view_mut(app) {
            v.cursor = i;
        }
    }

    #[test]
    fn rows_list_needs_you_then_one_per_department_and_unassigned_last() {
        let (_dir, app) = fixture();
        assert_eq!(
            rows_of(&app),
            vec![
                Row::NeedsYou,
                Row::Dept("audio".into()),
                Row::Dept("art-and-3d".into()),
                Row::Dept("animation".into()),
                Row::Dept("unassigned".into()),
            ]
        );
    }

    #[test]
    fn enter_expands_a_department_to_its_notes_sessions_and_verdict() {
        let (_dir, mut app) = fixture();
        cursor_to(&mut app, &Row::Dept("audio".into()));
        key(&mut app, KeyCode::Enter);
        let rows = rows_of(&app);
        assert_eq!(
            rows[1..7],
            [
                Row::Dept("audio".into()),
                Row::Note("audio".into(), "storm".into()),
                Row::Note("audio".into(), "wind".into()),
                Row::Session("audio".into(), AgentId("a1".into())),
                Row::Session("audio".into(), AgentId("a2".into())),
                Row::Verdict("audio".into()),
            ][..]
        );
        cursor_to(&mut app, &Row::Dept("audio".into()));
        key(&mut app, KeyCode::Enter);
        assert_eq!(rows_of(&app).len(), 5, "Enter again collapses it");
    }

    #[test]
    fn enter_on_a_note_opens_it_in_the_file_view() {
        let (dir, mut app) = fixture();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        cursor_to(&mut app, &Row::Note("audio".into(), "storm".into()));
        key(&mut app, KeyCode::Enter);
        match &app.modals.overlay {
            Some(Overlay::FileTabs(v)) => {
                assert_eq!(v.tabs[0].path, dir.path().join("notes").join("storm.md"));
            }
            other => panic!("not the file view: {other:?}"),
        }
    }

    #[test]
    fn enter_on_a_verdict_opens_the_gauntlet_on_it() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('v'));
        match &app.modals.overlay {
            Some(Overlay::Gauntlet(g)) => {
                assert_eq!(g.repo, Some(ProjectId("p1".into())));
                assert_eq!(g.kind().map(|k| k.kind.as_str()), Some("sound"));
                assert_eq!(g.selected().map(|v| v.id.as_str()), Some("S01"));
                assert!(g.detail, "opened on the verdict, its detail is up");
            }
            other => panic!("not the gauntlet: {other:?}"),
        }
    }

    #[test]
    fn enter_on_a_session_closes_the_board_to_attach_it() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('l'));
        assert!(
            !matches!(app.modals.overlay, Some(Overlay::StudioBoard(_))),
            "the board gave way to the session"
        );
    }

    #[test]
    fn the_menu_offers_only_what_applies() {
        let (_dir, app) = fixture();
        let v = view(&app);
        let board = board_of(&app, v).unwrap();
        let labels = |row: Row| -> Vec<String> {
            menu_rows(&board, &row, &v.expanded)
                .1
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(
            labels(Row::Dept("audio".into())),
            [
                "Expand",
                "Open lead",
                "Review last verdict",
                "Open top note",
                "Reload"
            ]
        );
        assert_eq!(labels(Row::Dept("art-and-3d".into())), ["Expand", "Reload"]);
        assert_eq!(
            labels(Row::Session("audio".into(), AgentId("a1".into()))),
            [
                "Open session",
                "Review last verdict",
                "Open top note",
                "Reload"
            ],
            "no Open lead on the lead itself"
        );
        assert_eq!(labels(Row::Need(0)), ["Open session", "Reload"]);
    }

    #[test]
    fn m_puts_the_menu_over_the_board() {
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('m'));
        assert!(matches!(app.modals.overlay, Some(Overlay::Menu(_))));
        assert!(matches!(
            app.modals.verbs_under.as_deref(),
            Some(Overlay::StudioBoard(_))
        ));
    }

    #[test]
    fn draws_at_80x24_and_120x40_collapsed_and_expanded() {
        for (w, h) in [(80, 24), (120, 40)] {
            let (_dir, mut app) = fixture();
            let text = draw(&mut app, w, h);
            assert!(text.contains("Studio board · game"), "{text}");
            assert!(text.contains("What needs you"), "{text}");
            assert!(text.contains("Audio"), "{text}");
            assert!(
                text.contains("Art and 3D") && text.contains("Animation"),
                "{text}"
            );
            assert!(text.contains("Unassigned"), "{text}");
            assert!(text.contains("lead audio"), "{text}");
            assert!(text.contains("approve"), "{text}");
            assert!(text.contains("$1.25"), "{text}");
            assert!(text.contains("Enter open"), "{text}");
            for line in text.lines() {
                assert!(line.chars().count() <= w as usize);
            }
            cursor_to(&mut app, &Row::Dept("audio".into()));
            key(&mut app, KeyCode::Enter);
            let text = draw(&mut app, w, h);
            assert!(text.contains("Storm bed too loud"), "{text}");
            assert!(text.contains("Wind loop seam"), "{text}");
            assert!(text.contains("needs you"), "{text}");
            assert!(text.contains("65%"), "{text}");
        }
    }

    #[test]
    fn a_repo_without_a_manifest_or_any_studio_repo_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.tree.projects.push(Project {
            id: ProjectId("p1".into()),
            name: "plain".into(),
            repo_path: dir.path().to_path_buf(),
            sort_order: 0,
        });
        open(&mut app, Some(ProjectId("p1".into())));
        assert!(draw(&mut app, 80, 24).contains("no .kete/studio.toml"));
        open(&mut app, None);
        assert!(draw(&mut app, 80, 24).contains("No repo here has studio mode"));
    }

    #[test]
    fn a_click_on_a_row_is_enter_on_it() {
        let (_a, mut by_key) = fixture();
        let (_b, mut by_click) = fixture();
        draw(&mut by_key, 100, 30);
        let text = draw(&mut by_click, 100, 30);
        cursor_to(&mut by_key, &Row::Dept("audio".into()));
        key(&mut by_key, KeyCode::Enter);
        let (x, y) = text
            .lines()
            .enumerate()
            .find_map(|(y, l)| {
                l.find("Audio")
                    .map(|b| (l[..b].chars().count() as u16, y as u16))
            })
            .unwrap();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(handle_mouse(&mut by_click, click, &mut Vec::new()));
        let (k, c) = (view(&by_key), view(&by_click));
        assert_eq!(k.expanded, c.expanded);
        assert_eq!(k.cursor, c.cursor);
        assert!(c.expanded.contains("audio"));
    }

    #[test]
    fn a_stale_read_is_dropped() {
        let (_dir, mut app) = fixture();
        let v = view_mut(&mut app).unwrap();
        let before = v.files.clone();
        *v.inbox.lock().unwrap() = Some(Landed {
            ticket: v.ticket.wrapping_sub(1),
            files: BoardFiles::default(),
        });
        sync(None, v);
        assert_eq!(v.files, before);
    }
}
