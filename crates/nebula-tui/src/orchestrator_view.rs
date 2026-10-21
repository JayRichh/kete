//! The ORCHESTRATOR VIEW: F10's chat-first layout around the machine's one
//! pinned orchestrator session. Its terminal owns the screen (the same
//! full-screen pane any session gets with `^F`), with a STATUS STRIP of
//! chips across the top — every other live session, in the attention order
//! of the `/` palette — whose click or Enter opens the review modal on it.
//!
//! This module is the view's state, what it reads off the tree, and its
//! drawing; the keys and requests live in `event_loop::orchestrator`.

use nebula_core::{Agent, AgentId, AgentKind, AgentStatus, ReviewTabKind, SessionRef};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, HitTarget};

/// Below this many columns the strip collapses to one summary chip.
pub const STRIP_MIN_WIDTH: u16 = 60;

/// The view's own state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrchState {
    /// F10 put the view up; it holds while the orchestrator's session is
    /// the full-screen pane.
    pub view: bool,
    /// The chip the keyboard is on, with the pane unlocked (`^Q`).
    pub chip: usize,
    /// A review the orchestrator opened while the user was typing into
    /// another session: held, with a toast, until F10 shows it.
    pub pending_review: Option<(AgentId, Vec<SessionRef>, Vec<ReviewTabKind>)>,
    /// What the pane showed before a review moved it onto the session
    /// under review: put back when the review closes, however it closes.
    /// `Some(None)`: the pane showed nothing, and is emptied again.
    pub review_restore: Option<Option<SessionRef>>,
    /// The protocol the daemon answered the handshake with; None before
    /// one (tests). Below [`ORCHESTRATOR_PROTOCOL`] it cannot decode the
    /// provisioning requests, and sending one would drop the connection.
    pub daemon_protocol: Option<u32>,
}

/// The protocol that grew `StartOrchestrator`, `SetOrchestrator` and
/// `ClearOrchestrator`.
pub const ORCHESTRATOR_PROTOCOL: u32 = 54;

/// What F10 and the card menu say to a daemon too old for provisioning.
#[cfg(unix)]
pub const DAEMON_TOO_OLD: &str = "the running daemon predates the orchestrator: run `kete reload` (every session keeps running), then reopen kete";
/// Windows daemons can't restart in place: `kete reload` refuses there.
#[cfg(windows)]
pub const DAEMON_TOO_OLD: &str = "the running daemon predates the orchestrator: run `kete kill` (stops all sessions), then reopen kete";

/// Can the daemon take the provisioning requests?
pub fn daemon_provisions(app: &App) -> bool {
    app.orch
        .daemon_protocol
        .is_none_or(|v| v >= ORCHESTRATOR_PROTOCOL)
}

/// The pinned orchestrator, when one is live (archived rows never count).
pub fn orchestrator(app: &App) -> Option<&Agent> {
    app.tree
        .agents
        .iter()
        .find(|a| a.role.is_orchestrator() && !a.archived)
}

/// Is the ORCHESTRATOR VIEW on screen: F10's flag, a full-screen pane, and
/// the orchestrator's session in it.
pub fn active(app: &App) -> bool {
    let Some(orch) = orchestrator(app) else {
        return false;
    };
    app.orch.view
        && app.pane.collapsed
        && app
            .pane
            .term
            .as_ref()
            .is_some_and(|t| t.sref == SessionRef::Agent(orch.id.clone()))
}

/// One chip on the strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub sref: SessionRef,
    pub name: String,
    pub status: AgentStatus,
    pub unseen: bool,
}

/// Every live session but the orchestrator, in attention order:
/// needs-feedback, running, finished-unread, the rest.
pub fn chips(app: &App) -> Vec<Chip> {
    let orch = orchestrator(app).map(|a| a.id.clone());
    crate::palette::attention_sessions(&app.tree)
        .into_iter()
        .filter(|id| Some(id) != orch.as_ref())
        .filter_map(|id| app.tree.agents.iter().find(|a| a.id == id))
        .map(|a| Chip {
            sref: SessionRef::Agent(a.id.clone()),
            name: a.name.clone(),
            status: a.status,
            unseen: a.unseen,
        })
        .collect()
}

/// The strip's row, carved off the top of `body` while the view is up,
/// and the TRIAGE STRIP's rows (`triage_strip::rows`, 0.60) carved off its
/// bottom; None leaves that part out. The middle is the orchestrator's
/// pane.
pub fn split(app: &App, body: Rect) -> (Option<Rect>, Rect, Option<Rect>) {
    if !active(app) || body.height < 4 {
        return (None, body, None);
    }
    let strip = Rect { height: 1, ..body };
    let rest = Rect {
        y: body.y + 1,
        height: body.height - 1,
        ..body
    };
    // The pane keeps at least 3 rows whatever the triage strip asks.
    let triage = crate::triage_strip::rows(app).min(rest.height.saturating_sub(3));
    if triage == 0 {
        return (Some(strip), rest, None);
    }
    let pane = Rect {
        height: rest.height - triage,
        ..rest
    };
    let bottom = Rect {
        y: rest.y + pane.height,
        height: triage,
        ..rest
    };
    (Some(strip), pane, Some(bottom))
}

/// What a narrow strip says instead of chips: the count, and how many
/// need the user.
fn summary(chips: &[Chip]) -> String {
    let asking = chips
        .iter()
        .filter(|c| c.status == AgentStatus::NeedsFeedback)
        .count();
    let sessions = if chips.len() == 1 {
        "session"
    } else {
        "sessions"
    };
    if asking > 0 {
        format!(" ◆ {} {sessions} · {asking} need you ", chips.len())
    } else {
        format!(" ◆ {} {sessions} ", chips.len())
    }
}

/// Draw the STATUS STRIP into `area`, registering a click target per chip.
pub fn draw_strip(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.chrome.theme;
    let chips = chips(app);
    if app.orch.chip >= chips.len() {
        app.orch.chip = chips.len().saturating_sub(1);
    }
    let keyboard = !app.pane.term_locked;
    let mut spans = vec![Span::styled(
        " orchestrator ",
        Style::default()
            .fg(th.on_accent)
            .bg(th.accent)
            .add_modifier(Modifier::BOLD),
    )];
    let mut x = area.x + Span::raw(" orchestrator ").width() as u16;
    if chips.is_empty() {
        spans.push(Span::styled(
            "  no other sessions",
            Style::default().fg(th.dim),
        ));
    } else if area.width < STRIP_MIN_WIDTH {
        let text = summary(&chips);
        let w = Span::raw(text.as_str()).width() as u16;
        // The summary chip opens the review on the first session in need.
        app.chrome.hits.push((
            Rect {
                x,
                y: area.y,
                width: w.min(area.right().saturating_sub(x)),
                height: 1,
            },
            HitTarget::OrchestratorChip(chips[0].sref.clone()),
        ));
        spans.push(Span::styled(text, Style::default().fg(th.text)));
    } else {
        for (i, chip) in chips.iter().enumerate() {
            let dot = crate::ui::status_color(Some(chip.status), chip.unseen, th);
            let name = crate::ui::fit_to_width(&chip.name, 18);
            let label = format!(" ● {name} ");
            let w = Span::raw(label.as_str()).width() as u16 + 1;
            if x + w > area.right() {
                let more = format!(" +{}", chips.len() - i);
                spans.push(Span::styled(more, Style::default().fg(th.dim)));
                break;
            }
            spans.push(Span::raw(" "));
            let selected = keyboard && i == app.orch.chip;
            let base = if selected {
                Style::default().bg(th.accent).fg(th.on_accent)
            } else {
                Style::default().fg(th.text)
            };
            spans.push(Span::styled(
                " ● ",
                base.fg(if selected { th.on_accent } else { dot }),
            ));
            spans.push(Span::styled(format!("{name} "), base));
            app.chrome.hits.push((
                Rect {
                    x: x + 1,
                    y: area.y,
                    width: w - 1,
                    height: 1,
                },
                HitTarget::OrchestratorChip(chip.sref.clone()),
            ));
            x += w;
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The F10 prompt that starts the first orchestrator: its harness,
/// profile, model and effort, the project it runs in and its name, as
/// `kete orchestrator start` takes them. Enter starts.
#[derive(Debug, Clone, PartialEq)]
pub struct StartView {
    pub harnesses: Vec<AgentKind>,
    pub harness: usize,
    /// Profile names for the chosen harness; index 0 is the harness's own
    /// default (no profile named).
    pub profiles: Vec<nebula_core::profiles::Profile>,
    pub profile: usize,
    /// Each harness's configured model and effort (the Agents tab's), what
    /// its rows start on.
    pub configured: Vec<(AgentKind, Option<String>, Option<String>)>,
    /// The chosen harness's model rows (`default` first) and the pick.
    pub models: Vec<String>,
    pub model: usize,
    /// Its effort rows for that model (empty: it takes none) and the pick.
    pub efforts: Vec<String>,
    pub effort: usize,
    /// The projects it can run in; index 0 is a workspace of its own.
    pub projects: Vec<(nebula_core::ProjectId, String)>,
    pub project: usize,
    /// The session's name, typed on its row; empty takes the daemon's.
    pub name: String,
    /// Which row has the keys, by [`StartRow::ALL`].
    pub row: usize,
    pub area: Rect,
    /// Each row's rect, and the Start button's, written back during draw.
    pub row_hits: Vec<(Rect, usize)>,
    pub start_hit: Rect,
}

/// The prompt's rows, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRow {
    Harness,
    Profile,
    Model,
    Effort,
    Project,
    Name,
}

impl StartRow {
    pub const ALL: [StartRow; 6] = [
        StartRow::Harness,
        StartRow::Profile,
        StartRow::Model,
        StartRow::Effort,
        StartRow::Project,
        StartRow::Name,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StartRow::Harness => "Harness",
            StartRow::Profile => "Profile",
            StartRow::Model => "Model",
            StartRow::Effort => "Effort",
            StartRow::Project => "Project",
            StartRow::Name => "Name",
        }
    }
}

impl StartView {
    pub fn new(profiles: Vec<nebula_core::profiles::Profile>) -> Self {
        let mut view = Self {
            harnesses: AgentKind::ALL
                .into_iter()
                .filter(|k| *k != AgentKind::Custom)
                .collect(),
            harness: 0,
            profiles,
            profile: 0,
            configured: Vec::new(),
            models: Vec::new(),
            model: 0,
            efforts: Vec::new(),
            effort: 0,
            projects: Vec::new(),
            project: 0,
            name: String::new(),
            row: 0,
            area: Rect::default(),
            row_hits: Vec::new(),
            start_hit: Rect::default(),
        };
        view.load_models();
        view
    }

    /// The configured model and effort of each harness, and the projects
    /// to offer: the rows start on them.
    pub fn with(
        mut self,
        configured: Vec<(AgentKind, Option<String>, Option<String>)>,
        projects: Vec<(nebula_core::ProjectId, String)>,
    ) -> Self {
        self.configured = configured;
        self.projects = projects;
        self.load_models();
        self
    }

    pub fn kind(&self) -> AgentKind {
        self.harnesses
            .get(self.harness)
            .copied()
            .unwrap_or(AgentKind::Claude)
    }

    pub fn focused(&self) -> StartRow {
        StartRow::ALL[self.row.min(StartRow::ALL.len() - 1)]
    }

    fn configured_for(&self, kind: AgentKind) -> (Option<String>, Option<String>) {
        self.configured
            .iter()
            .find(|(k, _, _)| *k == kind)
            .map(|(_, m, e)| (m.clone(), e.clone()))
            .unwrap_or_default()
    }

    /// The harness's model rows, on its configured model.
    fn load_models(&mut self) {
        let kind = self.kind();
        let (model, _) = self.configured_for(kind);
        self.models = crate::config::model_choices(kind, None);
        self.model = model
            .and_then(|m| self.models.iter().position(|x| *x == m))
            .unwrap_or(0);
        // A new harness's efforts start on its configured one.
        self.efforts.clear();
        self.load_efforts();
    }

    /// The model's effort rows, on the configured effort when offered.
    fn load_efforts(&mut self) {
        let kind = self.kind();
        let (_, effort) = self.configured_for(kind);
        let keep = self.efforts.get(self.effort).cloned();
        self.efforts = crate::config::effort_choices(kind, self.model_choice(), None);
        self.effort = keep
            .or(effort)
            .and_then(|e| self.efforts.iter().position(|x| *x == e))
            .unwrap_or(0);
    }

    /// The model row's value as picked (`default` included).
    fn model_choice(&self) -> Option<&str> {
        self.models.get(self.model).map(String::as_str)
    }

    /// The model to send: None for `default`, the CLI's own pick.
    pub fn model_name(&self) -> Option<String> {
        self.model_choice()
            .filter(|m| !m.eq_ignore_ascii_case(crate::config::DEFAULT_CHOICE))
            .map(str::to_string)
    }

    /// The effort to send: None for `default`, or a harness with none.
    pub fn effort_name(&self) -> Option<String> {
        self.efforts
            .get(self.effort)
            .filter(|e| !e.eq_ignore_ascii_case(crate::config::DEFAULT_CHOICE))
            .cloned()
    }

    /// The project it runs in; None: a workspace of its own.
    pub fn project_id(&self) -> Option<nebula_core::ProjectId> {
        self.project
            .checked_sub(1)
            .and_then(|i| self.projects.get(i))
            .map(|(id, _)| id.clone())
    }

    /// The name to send; None for the daemon's.
    pub fn name_value(&self) -> Option<String> {
        Some(self.name.trim().to_string()).filter(|n| !n.is_empty())
    }

    /// The profiles the chosen harness can run under: the enabled ones in
    /// config order, then the switched-off ones (a start naming one still
    /// runs on it).
    pub fn harness_profiles(&self) -> Vec<&nebula_core::profiles::Profile> {
        let mut all = nebula_core::profiles::for_harness(&self.profiles, self.kind().as_str());
        all.sort_by_key(|p| p.disabled);
        all
    }

    /// The profile named for the start; None = the harness's default.
    pub fn profile_name(&self) -> Option<String> {
        if self.profile == 0 {
            return None;
        }
        self.harness_profiles()
            .get(self.profile - 1)
            .map(|p| p.name.clone())
    }

    pub fn profile_label(&self) -> String {
        let off = self.profile > 0
            && self
                .harness_profiles()
                .get(self.profile - 1)
                .is_some_and(|p| p.disabled);
        match self.profile_name() {
            Some(name) if off => format!("{name} (off)"),
            Some(name) => name,
            None => "harness default".to_string(),
        }
    }

    /// What a row reads.
    pub fn value(&self, row: StartRow) -> String {
        match row {
            StartRow::Harness => self.kind().as_str().to_string(),
            StartRow::Profile => self.profile_label(),
            StartRow::Model => self.model_choice().unwrap_or("default").to_string(),
            StartRow::Effort => self
                .efforts
                .get(self.effort)
                .cloned()
                .unwrap_or_else(|| "n/a".into()),
            StartRow::Project => match self.project.checked_sub(1) {
                Some(i) => self
                    .projects
                    .get(i)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_default(),
                None => "its own workspace".into(),
            },
            StartRow::Name => self.name.clone(),
        }
    }

    /// Move the keys `delta` rows, wrapping.
    pub fn move_row(&mut self, delta: i64) {
        let n = StartRow::ALL.len() as i64;
        self.row = (self.row as i64 + delta).rem_euclid(n) as usize;
    }

    /// Step the focused row's value by `delta`, wrapping. The name row is
    /// typed, not stepped.
    pub fn step(&mut self, delta: i64) {
        let wrap = |at: usize, n: usize| (at as i64 + delta).rem_euclid(n.max(1) as i64) as usize;
        match self.focused() {
            StartRow::Harness => {
                self.harness = wrap(self.harness, self.harnesses.len());
                self.profile = 0;
                self.efforts.clear();
                self.load_models();
            }
            StartRow::Profile => {
                self.profile = wrap(self.profile, self.harness_profiles().len() + 1);
            }
            StartRow::Model => {
                self.model = wrap(self.model, self.models.len());
                self.load_efforts();
            }
            StartRow::Effort => self.effort = wrap(self.effort, self.efforts.len()),
            StartRow::Project => self.project = wrap(self.project, self.projects.len() + 1),
            StartRow::Name => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, harness: &str) -> nebula_core::profiles::Profile {
        nebula_core::profiles::Profile {
            name: name.into(),
            harness: harness.into(),
            env: Default::default(),
            default: false,
            use_default: false,
            disabled: false,
            reserved: false,
        }
    }

    #[test]
    fn the_start_rows_start_on_the_configured_model_and_follow_the_harness() {
        let configured = vec![
            (
                AgentKind::Claude,
                Some("sonnet".into()),
                Some("high".into()),
            ),
            (AgentKind::Codex, None, None),
        ];
        let p = nebula_core::ProjectId("p1".into());
        let mut view =
            StartView::new(Vec::new()).with(configured, vec![(p.clone(), "demo".into())]);
        assert_eq!(view.model_name().as_deref(), Some("sonnet"));
        assert_eq!(view.effort_name().as_deref(), Some("high"));
        assert_eq!((view.project_id(), view.name_value()), (None, None));
        view.row = 2;
        while view.model_choice() != Some(crate::config::DEFAULT_CHOICE) {
            view.step(1);
        }
        assert_eq!(view.model_name(), None, "default sends no model");
        view.row = 0;
        view.step(1);
        assert_eq!(view.kind(), AgentKind::Codex);
        assert_eq!((view.model_name(), view.effort_name()), (None, None));
        view.row = 4;
        view.step(1);
        assert_eq!(view.project_id(), Some(p));
        view.step(1);
        assert_eq!(view.project_id(), None, "wraps to its own workspace");
        view.move_row(-5);
        assert_eq!(view.focused(), StartRow::Name);
        view.name = "  lead ".into();
        assert_eq!(view.name_value().as_deref(), Some("lead"));
    }

    #[test]
    fn the_start_prompt_cycles_harnesses_and_their_profiles() {
        let mut view = StartView::new(vec![profile("work", "claude"), profile("alt", "codex")]);
        assert_eq!(view.kind(), AgentKind::Claude);
        assert_eq!(view.profile_name(), None);
        view.row = 1;
        view.step(1);
        assert_eq!(view.profile_name().as_deref(), Some("work"));
        view.step(1);
        assert_eq!(view.profile_name(), None, "wraps back to the default");
        view.row = 0;
        view.step(1);
        assert_eq!(view.kind(), AgentKind::Codex);
        view.row = 1;
        view.step(1);
        assert_eq!(view.profile_name().as_deref(), Some("alt"));
        view.row = 0;
        view.step(-2);
        assert!(!view.harnesses.contains(&AgentKind::Custom));
        assert_eq!(view.profile, 0, "a harness change drops the profile");
    }

    #[test]
    fn a_narrow_strip_counts_sessions_and_who_needs_you() {
        let chip = |status| Chip {
            sref: SessionRef::Agent(AgentId("a".into())),
            name: "a".into(),
            status,
            unseen: false,
        };
        assert_eq!(summary(&[chip(AgentStatus::Running)]), " ◆ 1 session ");
        assert_eq!(
            summary(&[chip(AgentStatus::NeedsFeedback), chip(AgentStatus::Running)]),
            " ◆ 2 sessions · 1 need you "
        );
    }
}
