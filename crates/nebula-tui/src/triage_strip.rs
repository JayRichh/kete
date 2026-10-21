//! The TRIAGE STRIP (K6): one line at the bottom of the orchestrator view
//! that files an inbox note and hands it to the triage desk.
//!
//! In the view's chip mode (`^Q` from typing) the row says `t triage
//! note`; `t` (or the `triage_note` verb) opens a one-line field there,
//! Enter files and sends, Esc closes. The send runs off the loop
//! ([`file_and_send`], on the view jobs' blocking pool); the row says
//! what it is waiting on, then shows the desk's route line until the next
//! key.
//!
//! [`file_and_send`] is also `kete studio note`'s whole daemon side: write
//! the note (`nebula_core::studio::inbox_write`), pick the desk, ask it (or
//! start it on the note), and read its one line back.

use crate::app::App;
use crate::text_input::TextInput;
use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::studio::inbox_write::{self as iw, Desk, NewNote, Refused, Target};
use nebula_core::studio::wire::SessionRole;
use nebula_core::usage::ContextUsage;
use nebula_core::{
    env, Agent, AgentId, AgentKind, AgentRole, AgentStatus, ClientRequest, EntityId, Project,
    ServerEvent, SessionRef, Worktree, WorktreeId,
};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// What the strip holds between keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strip {
    /// The field is open and takes the keys.
    pub open: bool,
    pub input: TextInput,
    /// The send it is waiting on, by ticket.
    pub pending: Option<u64>,
    /// What the row says: the sending line, then the route line.
    pub line: Option<String>,
}

thread_local! {
    // shortcut: App has no field for the strip (a missing 0.60 contract,
    // named in the lane's PR), so the loop thread keeps it here; move it
    // to `App` once the main thread adds one.
    static STRIP: RefCell<Strip> = RefCell::default();
}

/// Lines the background sends landed, by ticket, for the loop to pick up.
static LANDED: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());

fn with<T>(f: impl FnOnce(&mut Strip) -> T) -> T {
    STRIP.with(|s| {
        let mut strip = s.borrow_mut();
        land(&mut strip);
        f(&mut strip)
    })
}

/// Takes the landed line of the send `strip` waits on.
fn land(strip: &mut Strip) {
    let Some(ticket) = strip.pending else {
        return;
    };
    let mut landed = LANDED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = landed.iter().position(|(t, _)| *t == ticket) {
        let (_, line) = landed.remove(at);
        strip.pending = None;
        strip.line = Some(line);
    }
}

/// Whether the row offers `t`: the chips have the keys and someone can
/// take a note.
fn hint(app: &App) -> bool {
    crate::orchestrator_view::active(app)
        && !app.pane.term_locked
        && crate::event_loop::verbs::triage_available(app)
}

/// Rows the strip takes at the bottom of the orchestrator view.
pub fn rows(app: &App) -> u16 {
    let busy = with(|s| s.open || s.pending.is_some() || s.line.is_some());
    u16::from(busy || hint(app))
}

/// Open the triage line (chip mode `t`, the `triage_note` verb).
pub fn open(app: &mut App) {
    if !crate::event_loop::verbs::triage_available(app) {
        app.chrome.flash = Some("triage: start an orchestrator or a triage desk first".into());
    } else if project_of(app).is_none() {
        app.chrome.flash = Some("triage: no repo to file into".into());
    } else {
        with(|s| {
            s.open = true;
            s.line = None;
        });
    }
    app.chrome.dirty = true;
}

/// A key while the strip has the focus; true when it took it.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    match with(|s| step(s, &key)) {
        Step::Ignored => false,
        Step::Took => true,
        Step::Submit(text) => {
            submit(app, text);
            true
        }
    }
}

/// What a key did to the strip.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Ignored,
    Took,
    Submit(String),
}

/// A key, on the strip alone. While the field is open it takes every key;
/// a route line on show is cleared by the next key, which goes on.
fn step(s: &mut Strip, key: &KeyEvent) -> Step {
    if !s.open {
        if s.pending.is_none() {
            s.line = None;
        }
        return Step::Ignored;
    }
    match key.code {
        KeyCode::Esc => {
            s.open = false;
            s.input.clear();
        }
        KeyCode::Enter => {
            let text = s.input.trim().to_string();
            s.open = false;
            s.input.clear();
            if !text.is_empty() {
                return Step::Submit(text);
            }
        }
        _ => {
            s.input.handle_key(key);
        }
    }
    Step::Took
}

/// The project a note from the strip files into: the orchestrator's, else
/// a live desk's.
fn project_of(app: &App) -> Option<nebula_core::ProjectId> {
    let agent = crate::orchestrator_view::orchestrator(app).or_else(|| {
        app.tree.agents.iter().find(|a| {
            !a.archived
                && app
                    .session_roles
                    .get(&a.id)
                    .is_some_and(|r| r.card == iw::DESK_ROLE)
        })
    })?;
    app.tree
        .worktrees
        .iter()
        .find(|w| w.id == agent.worktree_id)
        .map(|w| w.project_id.clone())
}

fn submit(app: &mut App, text: String) {
    let Some(project) = project_of(app) else {
        app.chrome.flash = Some("triage: no repo to file into".into());
        return;
    };
    let ticket = crate::view_jobs::ticket();
    with(|s| {
        s.pending = Some(ticket);
        s.line = Some(sending_line(Target::Desk));
    });
    let Some(jobs) = app.jobs.view_jobs.clone() else {
        // A unit test's app has no jobs: it stays sending.
        return;
    };
    jobs.run(move || {
        let note = NewNote {
            text,
            ..NewNote::default()
        };
        let line = match tokio::runtime::Handle::current().block_on(file_and_send(
            &project.0,
            note,
            true,
            Target::Desk,
        )) {
            Ok(outcome) => outcome_line(&outcome),
            Err(e) => format!("triage note failed: {e:#}"),
        };
        LANDED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((ticket, line.clone()));
        // The flash wakes the loop to paint the row, and says it when the
        // view is closed.
        Some(crate::view_jobs::Answer::Flash(line))
    });
}

/// Draw the strip into `area` (the rows [`rows`] asked for).
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.chrome.theme;
    let shown = with(|s| s.clone());
    let line = strip_line(&shown, hint(app), area.width as usize, th);
    f.render_widget(Paragraph::new(line), area);
}

/// The row: the open field, else what it says, else the `t` hint.
fn strip_line(s: &Strip, hint: bool, width: usize, th: crate::theme::Theme) -> Line<'static> {
    let label = Span::styled(
        " triage ",
        Style::default().fg(th.muted).add_modifier(Modifier::BOLD),
    );
    if s.open {
        let keys = " Enter files · Esc closes ";
        let room = width.saturating_sub(8 + keys.chars().count()).max(8);
        let mut spans = vec![label, Span::styled("› ", Style::default().fg(th.dim))];
        spans.extend(crate::ui::input_spans(&s.input, room, th.accent, th));
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        if used + keys.chars().count() <= width {
            spans.push(Span::raw(" ".repeat(width - used - keys.chars().count())));
            spans.push(Span::styled(keys, Style::default().fg(th.dim)));
        }
        return Line::from(spans);
    }
    if let Some(text) = &s.line {
        let mark = if s.pending.is_some() { "… " } else { "→ " };
        let text: String = text.chars().take(width.saturating_sub(10)).collect();
        return Line::from(vec![
            label,
            Span::styled(mark, Style::default().fg(th.dim)),
            Span::styled(text, Style::default().fg(th.text)),
        ]);
    }
    if hint {
        return Line::from(vec![
            Span::styled(
                " t",
                Style::default().fg(th.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " triage note: one line for the inbox",
                Style::default().fg(th.dim),
            ),
        ]);
    }
    Line::default()
}

/// What the row says while a send is out.
pub fn sending_line(target: Target) -> String {
    match target {
        Target::Desk => "filing and sending to the triage desk".into(),
        Target::Orchestrator => "filing and sending to the orchestrator".into(),
    }
}

/// A filed note and where it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sent {
    pub id: String,
    pub path: PathBuf,
    /// `desk` or `orchestrator`; None when it was not sent.
    pub to: Option<&'static str>,
    /// The desk's one line, when it came within the wait.
    pub route: Option<String>,
    /// The desk did not answer within the wait.
    pub busy: bool,
    /// Why it was filed but sent nowhere.
    pub unsent: Option<String>,
}

/// What [`file_and_send`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Filed(Sent),
    Duplicate(iw::Duplicate),
}

/// The line the strip (and `kete studio note`) shows for `outcome`.
pub fn outcome_line(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Duplicate(d) => format!(
            "already in the inbox: {} ({})",
            d.existing
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            d.reason
        ),
        Outcome::Filed(s) => match (&s.route, s.busy, &s.unsent, s.to) {
            (Some(route), ..) => format!("{}: {route}", s.id),
            (None, true, ..) => iw::busy_line(&s.id),
            (None, false, Some(why), _) => format!("filed {}, not sent: {why}", s.id),
            (None, false, None, Some(_)) => format!("filed {}: the desk gave no line", s.id),
            (None, false, None, None) => {
                format!("filed {}", nebula_core::studio::settings::repo_key(&s.path))
            }
        },
    }
}

/// What a subscribe shows of the sessions.
#[derive(Default)]
struct Seen {
    projects: Vec<Project>,
    worktrees: Vec<Worktree>,
    agents: Vec<Agent>,
    roles: Vec<SessionRole>,
    contexts: HashMap<AgentId, ContextUsage>,
}

const CREATE_REQ: u64 = 1;
const ASK_REQ: u64 = 2;
const READ_REQ: u64 = 3;
/// Lines of the desk's session read back for its answer.
const READ_LINES: u32 = 40;

/// File `note` in `project`'s inbox (an id, a name, or a path inside one of
/// its checkouts) and, with `send`, hand it to the triage desk (or, for
/// [`Target::Orchestrator`], the orchestrator) and wait for its line.
pub async fn file_and_send(
    project: &str,
    note: NewNote,
    send: bool,
    target: Target,
) -> Result<Outcome> {
    let mut conn = crate::ipc::connect_or_spawn().await?;
    let studio = conn.protocol_version >= nebula_core::STUDIO_PROTOCOL;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    let seen = read_seen(&mut conn, studio).await?;
    let root = resolve_project(&seen, project)?;
    let inbox = iw::inbox_dir(
        &root.repo_path,
        nebula_core::studio::settings::for_repo(&root.repo_path).as_ref(),
    )
    .map_err(anyhow::Error::msg)?;
    let now = nebula_core::clock::now_ms() / 1000;
    let written = match iw::write(&inbox, &note, now) {
        Ok(w) => w,
        Err(Refused::Duplicate(d)) => return Ok(Outcome::Duplicate(d)),
        Err(Refused::Failed(why)) => bail!("{why}"),
    };
    let mut sent = Sent {
        id: written.id,
        path: written.path,
        to: None,
        route: None,
        busy: false,
        unsent: None,
    };
    if !send {
        return Ok(Outcome::Filed(sent));
    }
    if !studio {
        sent.unsent =
            Some("the running daemon is older than 0.60: run `kete kill` and relaunch".into());
        return Ok(Outcome::Filed(sent));
    }
    let desk_defined = nebula_core::roles::load_committed(&root.repo_path)
        .ok()
        .flatten()
        .is_some_and(|f| f.roles.contains_key(iw::DESK_ROLE));
    let desk = match pick(&seen, &root.id, desk_defined, target) {
        Ok(desk) => desk,
        Err(why) => {
            sent.unsent = Some(why);
            return Ok(Outcome::Filed(sent));
        }
    };
    let (agent, turn) = match desk {
        Desk::Live(id) => {
            sent.to = Some("desk");
            let turn = ask(&mut conn, &id, &iw::desk_prompt(&sent.path)).await;
            (id, turn)
        }
        Desk::Orchestrator(id) => {
            sent.to = Some("orchestrator");
            let turn = ask(&mut conn, &id, &iw::orchestrator_prompt(&sent.path)).await;
            (id, turn)
        }
        Desk::Start(worktree) => {
            sent.to = Some("desk");
            match start_desk(&mut conn, worktree, &iw::desk_prompt(&sent.path)).await {
                Ok((id, turn)) => (id, turn),
                Err(e) => {
                    sent.unsent = Some(format!("{e:#}"));
                    return Ok(Outcome::Filed(sent));
                }
            }
        }
    };
    match turn {
        Err(e) => {
            sent.unsent = Some(format!("{e:#}"));
        }
        Ok(None) => sent.busy = true,
        Ok(Some(answer)) => {
            let read = read_session(&mut conn, &agent).await.ok();
            sent.route = read
                .as_deref()
                .and_then(iw::reply_line)
                .or_else(|| iw::reply_line(&answer));
        }
    }
    Ok(Outcome::Filed(sent))
}

/// The snapshot, and from a 0.60 daemon the contexts and roles that follow
/// it (the roles last).
async fn read_seen(conn: &mut crate::ipc::Connection, studio: bool) -> Result<Seen> {
    let mut seen = Seen::default();
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                projects,
                worktrees,
                agents,
                ..
            }) => {
                seen.projects = projects;
                seen.worktrees = worktrees;
                seen.agents = agents;
                if !studio {
                    return Ok(seen);
                }
            }
            Some(ServerEvent::SessionContexts { contexts }) => {
                seen.contexts = contexts.into_iter().map(|c| (c.agent, c.context)).collect();
            }
            Some(ServerEvent::SessionRoles { roles }) => {
                seen.roles = roles;
                return Ok(seen);
            }
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its sessions"),
        }
    }
}

fn pick(
    seen: &Seen,
    project: &nebula_core::ProjectId,
    desk_defined: bool,
    target: Target,
) -> std::result::Result<Desk, String> {
    let role = |id: &AgentId| seen.roles.iter().find(|r| &r.agent == id);
    let card = |id: &AgentId| role(id).map(|r| r.card.clone());
    let context = |id: &AgentId| {
        iw::context_percent(
            seen.contexts.get(id),
            role(id).and_then(|r| r.context_budget),
        )
    };
    iw::choose(
        project,
        &iw::Sessions {
            worktrees: &seen.worktrees,
            agents: &seen.agents,
            card: &card,
            context: &context,
        },
        desk_defined,
        target,
    )
}

/// The project `query` names: an id, a name, or a path inside one of its
/// checkouts.
fn resolve_project<'a>(seen: &'a Seen, query: &str) -> Result<&'a Project> {
    let q = query.trim();
    if let Some(p) = seen
        .projects
        .iter()
        .find(|p| p.id.0 == q || p.name.eq_ignore_ascii_case(q))
    {
        return Ok(p);
    }
    let want = normal(Path::new(q));
    let inside = |dir: &Path| {
        let dir = normal(dir);
        want == dir || want.starts_with(&format!("{dir}/"))
    };
    seen.projects
        .iter()
        .find(|p| {
            inside(&p.repo_path)
                || seen
                    .worktrees
                    .iter()
                    .any(|w| w.project_id == p.id && inside(&w.path))
        })
        .with_context(|| {
            format!("no kete project named `{q}` or holding that path: `kete add` adds one")
        })
}

/// A path for comparing: canonical when it exists, `/` separators, and on
/// Windows case folded.
fn normal(p: &Path) -> String {
    let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let text = nebula_core::studio::settings::repo_key(&p);
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// One labelled turn to a live session, waited on up to the reply wait.
/// `Ok(None)`: it did not answer in time.
async fn ask(
    conn: &mut crate::ipc::Connection,
    target: &AgentId,
    question: &str,
) -> Result<Option<String>> {
    write_frame(
        &mut conn.stream,
        &ClientRequest::AskSession {
            req_id: ASK_REQ,
            caller: env::non_empty(env::AGENT_ID).map(AgentId),
            target: target.0.clone(),
            question: question.to_string(),
            timeout_ms: iw::REPLY_WAIT.as_millis() as u64,
            wait: true,
        },
    )
    .await?;
    let reply = async {
        loop {
            match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
                Some(ServerEvent::SessionAnswer { req_id, answer, .. }) if req_id == ASK_REQ => {
                    return Ok(Some(answer));
                }
                Some(ServerEvent::Error {
                    req_id: Some(r),
                    message,
                }) if r == ASK_REQ => {
                    if message.contains("timed out") {
                        return Ok(None);
                    }
                    bail!("{message}");
                }
                Some(_) => continue,
                None => bail!("{}", crate::ipc::CLOSED_BEFORE_REPLY),
            }
        }
    };
    // A little past the daemon's own wait, so its answer wins.
    tokio::time::timeout(iw::REPLY_WAIT + Duration::from_secs(10), reply)
        .await
        .unwrap_or(Ok(None))
}

/// A new desk in the root worktree, started on `prompt`, and its first
/// turn waited on. The turn is `Ok(None)` when it outlasts the wait.
async fn start_desk(
    conn: &mut crate::ipc::Connection,
    worktree: WorktreeId,
    prompt: &str,
) -> Result<(AgentId, Result<Option<String>>)> {
    let deadline = tokio::time::Instant::now() + iw::REPLY_WAIT;
    write_frame(
        &mut conn.stream,
        &ClientRequest::CreateAgent {
            req_id: CREATE_REQ,
            worktree,
            name: iw::DESK_NAME.into(),
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            auto_title: false,
            cloud_prompt: None,
            starting_prompt: Some(prompt.to_string()),
            issue_url: None,
            role: AgentRole::Worker,
            profile: None,
            role_card: Some(iw::DESK_ROLE.into()),
        },
    )
    .await?;
    // Status changes can land before the Ack names the new row.
    let mut early: Vec<(AgentId, AgentStatus)> = Vec::new();
    let id = loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Ack {
                req_id,
                created: Some(EntityId::Agent(id)),
            }) if req_id == CREATE_REQ => break id,
            Some(ServerEvent::Ack { req_id, .. }) if req_id == CREATE_REQ => {
                bail!("the daemon created no desk")
            }
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == CREATE_REQ => bail!("{message}"),
            Some(ev) => early.extend(status_of(&ev)),
            None => bail!("{}", crate::ipc::CLOSED_BEFORE_REPLY),
        }
    };
    let mut turn = TurnWatch::default();
    for (agent, status) in early {
        if agent == id && turn.see(status) {
            return Ok((id, Ok(Some(String::new()))));
        }
    }
    let wait = async {
        loop {
            match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
                Some(ServerEvent::SessionExited {
                    session: SessionRef::Agent(agent),
                    ..
                }) if agent == id => return Ok(Some(String::new())),
                Some(ev) => {
                    let over = status_of(&ev)
                        .into_iter()
                        .any(|(agent, status)| agent == id && turn.see(status));
                    if over {
                        return Ok(Some(String::new()));
                    }
                }
                None => bail!("{}", crate::ipc::CLOSED_BEFORE_REPLY),
            }
        }
    };
    let turn = tokio::time::timeout_at(deadline, wait)
        .await
        .unwrap_or(Ok(None));
    Ok((id, turn))
}

/// The status an event says a session has: a `StatusChanged`, or the row
/// itself upserted (a starting prompt's Running arrives that way).
fn status_of(ev: &ServerEvent) -> Option<(AgentId, AgentStatus)> {
    match ev {
        ServerEvent::StatusChanged { agent, status, .. } => Some((agent.clone(), *status)),
        ServerEvent::EntityUpserted {
            entity: nebula_core::Entity::Agent(a),
        } => Some((a.id.clone(), a.status)),
        _ => None,
    }
}

/// A turn's end, from its status changes: activity, then idle.
#[derive(Debug, Default)]
struct TurnWatch {
    active: bool,
}

impl TurnWatch {
    /// True once the turn is over.
    fn see(&mut self, status: AgentStatus) -> bool {
        match status {
            AgentStatus::Running | AgentStatus::NeedsFeedback => {
                self.active = true;
                false
            }
            AgentStatus::Fresh
            | AgentStatus::Finished
            | AgentStatus::Terminated
            | AgentStatus::Disconnected => self.active,
        }
    }
}

/// The desk's session text (its transcript when the daemon knows it).
async fn read_session(conn: &mut crate::ipc::Connection, agent: &AgentId) -> Result<String> {
    write_frame(
        &mut conn.stream,
        &ClientRequest::ReadSession {
            req_id: READ_REQ,
            target: agent.0.clone(),
            max_lines: READ_LINES,
        },
    )
    .await?;
    let read = async {
        loop {
            match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
                Some(ServerEvent::SessionText { req_id, text, .. }) if req_id == READ_REQ => {
                    return Ok(text)
                }
                Some(ServerEvent::Error {
                    req_id: Some(r),
                    message,
                }) if r == READ_REQ => bail!("{message}"),
                Some(_) => continue,
                None => bail!("{}", crate::ipc::CLOSED_BEFORE_REPLY),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), read)
        .await
        .context("the daemon did not send the desk's text in time")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(s: &mut Strip, text: &str) {
        for c in text.chars() {
            assert_eq!(step(s, &key(KeyCode::Char(c))), Step::Took);
        }
    }

    fn row(s: &Strip, hint: bool) -> String {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let th = crate::theme::Theme::default();
        term.draw(|f| {
            let area = Rect::new(0, 23, 80, 1);
            f.render_widget(Paragraph::new(strip_line(s, hint, 80, th)), area);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..80).map(|x| buf[(x, 23)].symbol().to_string()).collect()
    }

    #[test]
    fn the_row_at_80x24_open_typing_and_routed() {
        let mut s = Strip::default();
        assert_eq!(row(&s, false).trim(), "");
        assert!(row(&s, true).starts_with(" t triage note: one line for the inbox"));
        s.open = true;
        let open = row(&s, true);
        assert!(open.starts_with(" triage › "), "{open}");
        assert!(
            open.trim_end().ends_with("Enter files · Esc closes"),
            "{open}"
        );
        typed(&mut s, "storm sky bands at dusk");
        assert!(row(&s, true).contains("storm sky bands at dusk"));
        assert_eq!(
            step(&mut s, &key(KeyCode::Enter)),
            Step::Submit("storm sky bands at dusk".into())
        );
        s.pending = Some(7);
        s.line = Some(sending_line(Target::Desk));
        assert!(row(&s, true).starts_with(" triage … filing and sending to the triage desk"));
        s.pending = None;
        s.line = Some("2026-10-22-storm: art, high: the dusk sky is art's".into());
        assert!(row(&s, true).starts_with(" triage → 2026-10-22-storm: art, high"));
    }

    #[test]
    fn esc_closes_and_the_next_key_clears_the_route() {
        let mut s = Strip {
            open: true,
            ..Strip::default()
        };
        typed(&mut s, "x");
        assert_eq!(step(&mut s, &key(KeyCode::Esc)), Step::Took);
        assert!(!s.open && s.input.is_empty());
        s.line = Some("routed".into());
        assert_eq!(step(&mut s, &key(KeyCode::Char('l'))), Step::Ignored);
        assert_eq!(s.line, None);
        // A line still being sent stays.
        s.pending = Some(1);
        s.line = Some("sending".into());
        assert_eq!(step(&mut s, &key(KeyCode::Char('l'))), Step::Ignored);
        assert_eq!(s.line.as_deref(), Some("sending"));
        // Enter on an empty field just closes.
        s.open = true;
        assert_eq!(step(&mut s, &key(KeyCode::Enter)), Step::Took);
        assert!(!s.open);
    }

    #[test]
    fn a_landed_line_replaces_the_sending_one() {
        let mut s = Strip {
            pending: Some(u64::MAX - 3),
            line: Some("sending".into()),
            ..Strip::default()
        };
        LANDED.lock().unwrap().push((u64::MAX - 3, "routed".into()));
        land(&mut s);
        assert_eq!((s.pending, s.line.as_deref()), (None, Some("routed")));
    }

    fn app_with_orchestrator() -> App {
        use nebula_core::{ProjectId, WorktreeId};
        let mut app = App::new();
        app.tree.projects.push(Project {
            id: ProjectId("p1".into()),
            name: "game".into(),
            repo_path: "/tmp/game".into(),
            sort_order: 0,
        });
        app.tree.worktrees.push(Worktree {
            id: WorktreeId("w1".into()),
            project_id: ProjectId("p1".into()),
            path: "/tmp/game".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        });
        app
    }

    fn orchestrator(id: &str) -> Agent {
        Agent {
            parent: None,
            profile: None,
            id: AgentId(id.into()),
            worktree_id: nebula_core::WorktreeId("w1".into()),
            name: id.into(),
            status: AgentStatus::Finished,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: true,
            issue_url: None,
            role: AgentRole::Orchestrator,
            recent_prompts: Vec::new(),
        }
    }

    /// INPUT PARITY: the `triage_note` verb and chip mode's `t` both land on
    /// [`open`] (event_loop/orchestrator.rs calls it for `t`); each leaves
    /// the same strip and flash.
    #[test]
    fn the_verb_and_t_open_the_same_line() {
        let run = |via_verb: bool, with_orch: bool| {
            STRIP.with(|s| *s.borrow_mut() = Strip::default());
            let mut app = app_with_orchestrator();
            if with_orch {
                app.tree.agents.push(orchestrator("orch"));
            }
            if via_verb {
                crate::event_loop::verbs::run_verb(
                    &mut app,
                    crate::keymap::Action::TriageNote,
                    &crate::event_loop::verbs::Target::Global,
                    &mut Vec::new(),
                );
            } else {
                open(&mut app);
            }
            (with(|s| s.clone()), app.chrome.flash.clone())
        };
        let (strip, flash) = run(true, true);
        assert!(strip.open && flash.is_none(), "{flash:?}");
        assert_eq!(run(false, true), (strip, flash));
        let refused = run(true, false);
        assert!(!refused.0.open);
        assert_eq!(
            refused.1.as_deref(),
            Some("triage: start an orchestrator or a triage desk first")
        );
        assert_eq!(run(false, false), refused);
    }

    #[test]
    fn a_turn_ends_on_idle_after_activity() {
        let mut t = TurnWatch::default();
        assert!(!t.see(AgentStatus::Fresh), "booting is not the answer");
        assert!(!t.see(AgentStatus::Running));
        assert!(!t.see(AgentStatus::NeedsFeedback));
        assert!(t.see(AgentStatus::Finished));
    }

    #[test]
    fn outcome_lines() {
        let sent = Sent {
            id: "2026-10-22-storm".into(),
            path: PathBuf::from("/r/notes/inbox/2026-10-22-storm.md"),
            to: Some("desk"),
            route: Some("art, high".into()),
            busy: false,
            unsent: None,
        };
        assert_eq!(
            outcome_line(&Outcome::Filed(sent.clone())),
            "2026-10-22-storm: art, high"
        );
        let busy = Sent {
            route: None,
            busy: true,
            ..sent.clone()
        };
        assert_eq!(
            outcome_line(&Outcome::Filed(busy)),
            "filed 2026-10-22-storm: the desk is busy, so the note waits in the inbox"
        );
        let unsent = Sent {
            route: None,
            to: None,
            unsent: Some("no orchestrator".into()),
            ..sent.clone()
        };
        assert_eq!(
            outcome_line(&Outcome::Filed(unsent)),
            "filed 2026-10-22-storm, not sent: no orchestrator"
        );
        let dup = iw::Duplicate {
            existing: PathBuf::from("/r/notes/inbox/2026-10-21-storm.md"),
            reason: "a note with its id is open".into(),
        };
        assert_eq!(
            outcome_line(&Outcome::Duplicate(dup)),
            "already in the inbox: 2026-10-21-storm.md (a note with its id is open)"
        );
    }
}
