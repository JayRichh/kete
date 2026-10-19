//! The QUICK PROMPT: the hotkey that opens a task box anywhere in the TUI
//! and launches an AGENT on what you type, without walking the NEW SESSION
//! PICKER first. The two are the two ways to start a session: `p` starts
//! one on a typed task, `n` starts one bare — a harness pick, and the first
//! prompt typed in the CLI — so the picker never ends in this box.
//!
//! What lives here is the launch spec — [`QuickLaunch`]: which AGENT KIND
//! and MODEL / EFFORT the `quick_prompt_kind` SETTING resolves to, which
//! AGENT PRESET (if any) wraps the text, and the [`QuickTarget`] it lands
//! in — the selected WORKTREE, or one that does not exist yet — plus the
//! two pickers that rewrite it for one launch (`Tab`, `Shift+Tab`), the
//! [`QuickReturn`] they carry so the round trip loses neither the spec
//! nor the typed text, and the `Ctrl+N` toggle that flips the target
//! between the selected checkout and a fresh worktree from any panel
//! (`toggle_new_worktree`). The dialog itself is an ordinary multi-line
//! `PromptDialog` (`PromptKind::QuickPrompt`) drawn by `ui::draw_overlay`,
//! and the create it ends in goes through `event_loop::create_agent` like
//! every other session, with the composed text as the STARTING PROMPT
//! (`event_loop::quick_launch` holds that last step). A box opened on a
//! PROJECT OPEN PRS GROUP row (`e` there, through the preset picker) is
//! the same box for a PR SESSION: it carries the pull request
//! ([`QuickLaunch::pr`]) and ends in a `CreatePrAgent` instead.
//!
//! A box closed without launching does not take what was typed with it:
//! Esc, a click outside and the HARDWIRED UNLOCK park it as a
//! [`QuickDraft`] (`App::quick_draft`), and the next box opened takes it
//! back ([`open_box`]).

use crate::agent_presets::AgentPreset;
use crate::app::{App, Focus, Overlay, PromptDialog, PromptKind};
use crate::config::{fit_effort, Config};
use crate::pull_request::{OpenPr, PrLaunch};
use crate::text_input::TextInput;
use nebula_core::{AgentKind, ProjectId, WorktreeId};

/// Where a QUICK PROMPT launch lands.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickTarget {
    /// The WORKTREE selected when the box opened.
    Worktree(WorktreeId),
    /// A WORKTREE that does not exist yet — `p` on the WORKTREES PANEL.
    /// Enter cuts `branch` off the PROJECT's fetched default base first
    /// (`ClientRequest::CreateWorktree`), and the launch follows into the
    /// checkout the DAEMON made once its Ack lands.
    NewWorktree { project: ProjectId, branch: String },
}

/// Everything one QUICK PROMPT will launch with. Resolved from the config
/// when the box opens and rewritten in place by the box's own pickers —
/// `Tab` (harness, then MODEL / EFFORT) and `Shift+Tab` (an AGENT PRESET).
/// Per-dialog: none of it is written back to CONFIG.JSON, so the next `p`
/// starts from the `quick_prompt_kind` SETTING again.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickLaunch {
    pub target: QuickTarget,
    pub kind: AgentKind,
    /// Registry id when `kind` is [`AgentKind::Custom`].
    pub custom: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The AGENT PRESET `Shift+Tab` picked: its prefix and postfix wrap
    /// the typed text into the STARTING PROMPT, and it pins the harness.
    /// `Tab` picking a harness clears it — a launch spec has one source.
    pub preset: Option<AgentPreset>,
    /// The GitHub issue this launch is for, when the box was opened from
    /// the ISSUES MODAL: named in the title, sent to the DAEMON as the
    /// session's persisted context (`CreateAgent::issue_url`), the name
    /// of the worktree `Ctrl+N` cuts, and the task when the box is sent
    /// empty. Kept across the box's pickers — the harness and the preset
    /// change what runs, not what it is for.
    pub issue: Option<crate::issues::IssueRef>,
    /// The pull request this launch is for, when the box was opened from
    /// a PROJECT OPEN PRS GROUP row (`e` on it): named in the title and
    /// the target row, and sent to the DAEMON as a PR SESSION
    /// (`CreatePrAgent`) — it runs in the PROJECT's checkout of the PR's
    /// head branch, reused when one is there and cut by the DAEMON
    /// otherwise, never in `target`, which only names the PROJECT (its
    /// ROOT WORKTREE). Kept across the box's pickers, as the issue is;
    /// `Ctrl+N` is refused, the checkout being the DAEMON's to pick.
    pub pr: Option<PrLaunch>,
    /// A CLAUDE CLOUD launch: `Tab` on the Claude row of the box's own
    /// `Tab` picker toggles it, as it does in the NEW SESSION PICKER, and
    /// Enter sends the typed text as the cloud task (`claude --cloud
    /// <task>`) rather than as a STARTING PROMPT. Only ever on a plain
    /// Claude launch ([`QuickLaunch::with_cloud`]): the DAEMON refuses a
    /// cloud task beside a preset, an issue or a pull request.
    pub cloud: bool,
    /// The modal the box was opened over — `Enter` / `p` in the ISSUES
    /// MODAL or the PULL REQUESTS MODAL — which it stands on rather than
    /// takes away: drawn under the box, and put back when the box goes
    /// without launching. A launch closes it, the new session's card being
    /// what there is to see. Kept across the box's pickers, as the issue is.
    pub under: Option<ModalUnder>,
    /// The PROFILE the launch runs under: its harness's default when the
    /// box opens (None when the harness has no default — the CLI's own
    /// environment), and whichever `^A` steps to. A `Tab` pick of another
    /// harness starts over from that harness's default.
    pub profile: Option<String>,
}

/// A modal a QUICK PROMPT box stands on ([`QuickLaunch::under`]), as it
/// stood when the box went up: its cursor, and the rects it was drawn in.
/// The rows are the [`App`]'s, so it is drawn fresh under the box.
#[derive(Debug, Clone, PartialEq)]
pub enum ModalUnder {
    Issues(Box<crate::issues::IssuesView>),
    PullRequests(Box<crate::pr_modal::PullRequestsView>),
}

impl ModalUnder {
    /// The modal `overlay` is, when it is one a box can stand on.
    pub fn of(overlay: Option<&Overlay>) -> Option<Self> {
        match overlay? {
            Overlay::Issues(view) => Some(Self::Issues(Box::new(view.clone()))),
            Overlay::PullRequests(view) => Some(Self::PullRequests(Box::new(view.clone()))),
            _ => None,
        }
    }

    /// Put the modal back up, on the row it was left on.
    pub fn reopen(self, app: &mut App) {
        match self {
            Self::Issues(view) => crate::issues::reopen(app, *view),
            Self::PullRequests(view) => crate::pr_modal::reopen(app, *view),
        }
    }
}

/// The modal under `overlay`: the box's own, or the one under the box a
/// picker opened from it is drawn over — `Tab`'s harness list, `^P`'s
/// PROJECT PICKER, `Shift+Tab`'s AGENT PRESETS — so the layers stay put
/// while the box's spec is rewritten. A picker the modal opened itself
/// (`Shift+Tab` in the ISSUES MODAL) stands on it the same way.
pub(crate) fn modal_under(overlay: &Overlay) -> Option<ModalUnder> {
    match overlay {
        Overlay::Prompt(prompt) => match &prompt.kind {
            PromptKind::QuickPrompt(launch) => launch.under.clone(),
            _ => None,
        },
        other => held_return(other)?.launch.under,
    }
}

/// The box a picker `overlay` owes back, when it is one opened for a QUICK
/// PROMPT launch — the menus pin it to their root rows, so a nested
/// submenu is reached through its parent.
pub(crate) fn held_return(overlay: &Overlay) -> Option<QuickReturn> {
    match overlay {
        Overlay::ProjectPicker(picker) => Some(picker.back.clone()),
        Overlay::AgentPresets(view) => view.quick.clone(),
        Overlay::Menu(menu) => {
            let mut menu = menu;
            loop {
                if let Some(back) = crate::event_loop::menu_quick_return(menu) {
                    return Some(back);
                }
                menu = menu.parent.as_ref()?;
            }
        }
        _ => None,
    }
}

/// A refused launch's box coming back (`event_loop::reopen_prompt_with`)
/// stands on the modal it was opened over only while one is still up —
/// that one, its cursor where it is now. Once the modal has closed, the
/// box comes back on its own rather than raising it again.
pub(crate) fn restack(app: &App, launch: &mut QuickLaunch) {
    if launch.under.is_some() {
        launch.under = ModalUnder::of(app.modals.overlay.as_ref());
    }
}

/// What a picker for a QUICK PROMPT launch carries, so the trip loses
/// nothing: the launch as it stood, the text typed so far, and whether a
/// box was up to come back to.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickReturn {
    pub launch: QuickLaunch,
    pub text: String,
    /// Was the box up when the picker opened (`Tab` / `Shift+Tab` in it)?
    /// Esc puts the box back only then. A picker reached with no box up —
    /// `e` on a pull request or an issue — closes on Esc instead, as the
    /// manager does: it used to put up an empty box nobody asked for. A
    /// pick opens the box either way.
    pub from_box: bool,
}

impl QuickReturn {
    /// `launch` with nothing typed, reached with no box up — so Esc closes
    /// the picker rather than putting a box back.
    pub fn fresh(launch: QuickLaunch) -> Self {
        Self {
            launch,
            text: String::new(),
            from_box: false,
        }
    }
}

/// A QUICK PROMPT box abandoned with something typed in it: the launch as
/// it stood and the field itself — text, caret and scroll. Nothing typed
/// into the box is lost to the press that closes it; the draft waits in
/// `App::quick_draft` for the next box ([`open_box`]).
///
/// One slot, in memory only: the last box abandoned is the one that comes
/// back, and none of it outlives the process. The next box empties the
/// slot whether or not it was aimed the same way, so a draft is restored
/// once — cleared out of the box it lands in and sent on Enter, or
/// abandoned again and parked again.
#[derive(Debug, Clone)]
pub struct QuickDraft {
    pub launch: QuickLaunch,
    pub input: TextInput,
}

/// The DRAFT a box on its way out leaves behind: a QUICK PROMPT with
/// something other than whitespace in it. Every other prompt leaves
/// nothing, and so does an empty box — closing one is a change of mind,
/// and parking it would drop the draft the slot already holds.
pub(crate) fn draft_of(prompt: &PromptDialog) -> Option<QuickDraft> {
    let PromptKind::QuickPrompt(launch) = &prompt.kind else {
        return None;
    };
    (!prompt.input.trim().is_empty()).then(|| QuickDraft {
        launch: launch.clone(),
        input: prompt.input.clone(),
    })
}

/// The DRAFT a picker closed from under the box leaves behind: the box it
/// owed back, with the text that was in it. The HARDWIRED UNLOCK is the
/// only way out of a picker that does not hand the box back itself.
pub(crate) fn draft_of_return(back: &QuickReturn) -> Option<QuickDraft> {
    (!back.text.trim().is_empty()).then(|| QuickDraft {
        launch: back.launch.clone(),
        input: TextInput::multiline_with_text(back.text.clone()),
    })
}

/// Open a fresh QUICK PROMPT box on `launch`, taking back the DRAFT the
/// last abandoned box left — every way into the box but the ones that
/// carry their own text (a picker's return trip, `Ctrl+N`, a refused
/// create) comes through here.
///
/// The text always comes back: it is the user's, not the target's. The
/// spec comes back with it only when the parked box was aimed at the same
/// place ([`QuickLaunch::aimed_like`]) — coming back to the same box is
/// coming back to the harness or AGENT PRESET picked in it, while a box
/// aimed somewhere else keeps the aim and spec it was opened with. Either
/// way the slot is emptied: the draft is in this box now, and clearing it
/// here and pressing Esc is how it is thrown away.
/// Read the PROFILES list into the app, and every account's latest PLAN
/// USAGE when the DAEMON is too old to push it
/// (`ServerEvent::AccountUsage`), for the box's profile field: once as a
/// box opens and again on `^A`, never per frame. The kept files are the
/// DAEMON's (`nebula_core::usage`), and an older build's screen readings
/// read back from them too.
pub(crate) fn refresh_profiles(app: &mut App) {
    if !app.launcher.usage_pushed {
        app.launcher.account_usage = crate::accounts_overlay::kept_readings();
    }
    let cfg = crate::config::Config::load();
    mirror_accounts(app, &cfg);
}

/// Mirror the settings a launch's account reads — the PROFILES, the
/// ACCOUNT SPREAD and every project's DEFAULT ACCOUNTS — into the app.
pub(crate) fn mirror_accounts(app: &mut App, cfg: &Config) {
    app.launcher.spread = spread_policy(cfg);
    app.launcher.profiles = cfg.profiles.clone();
    app.launcher.project_accounts = cfg
        .projects
        .iter()
        .map(|(repo, s)| (repo.clone(), s.default_profiles()))
        .filter(|(_, m)| !m.is_empty())
        .collect();
}

/// The repo path of the project `target` launches in.
fn target_repo(app: &App, target: &QuickTarget) -> Option<std::path::PathBuf> {
    let id = crate::launcher::project_of(app, target)?;
    app.tree
        .projects
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.repo_path.clone())
}

/// The project default account for `harness` of the project `target`
/// launches in, from the app's mirror (`mirror_accounts`): what a frame
/// reads.
pub fn project_default(app: &App, target: &QuickTarget, harness: &str) -> Option<String> {
    project_accounts(app, target).remove(harness.trim())
}

/// Every default account (harness → profile) of the project `target`
/// launches in, from the app's mirror.
pub fn project_accounts(
    app: &App,
    target: &QuickTarget,
) -> std::collections::BTreeMap<String, String> {
    target_repo(app, target)
        .and_then(|repo| app.launcher.project_accounts.get(&repo).cloned())
        .unwrap_or_default()
}

/// What a launch's profile field or picker offers for `harness`, in
/// order: "auto" (None: no profile sent, the DAEMON's ACCOUNT SPREAD
/// picks) under a spread that is not `manual`, or "none" (None: the
/// CLI's own login) under `manual` while no default stands for it; then
/// each enabled PROFILE in config order; then the switched-off ones,
/// which a launch naming them still runs on. Empty for a harness with no
/// profiles, which keeps the field off the box.
pub fn profile_choices_in(
    profiles: &[nebula_core::profiles::Profile],
    harness: &str,
    policy: nebula_core::accounts::Policy,
    project_default: Option<&str>,
) -> Vec<Option<String>> {
    let usable = nebula_core::profiles::for_harness(profiles, harness);
    if usable.is_empty() {
        return Vec::new();
    }
    // Under `manual` "none" is offered only while it means the own login:
    // with a default standing — the project's, else the harness's — None
    // would run on that one, which its own row already names.
    let lead = (policy.spread != nebula_core::accounts::Spread::Manual
        || (usable_project_default(profiles, harness, project_default).is_none()
            && nebula_core::profiles::default_for(profiles, harness).is_none()))
    .then_some(None);
    // Under `manual` the account a launch naming none runs on leads the
    // named ones: what the box opens on and the `n` picker's first row.
    let first = (policy.spread == nebula_core::accounts::Spread::Manual)
        .then(|| {
            usable_project_default(profiles, harness, project_default)
                .map(str::to_string)
                .or_else(|| {
                    nebula_core::profiles::default_for(profiles, harness)
                        .map(|p| p.name.trim().to_string())
                })
        })
        .flatten();
    let name = |p: &&nebula_core::profiles::Profile| Some(p.name.trim().to_string());
    let enabled = usable.iter().filter(|p| !p.disabled).map(name);
    let (lead_named, rest): (Vec<_>, Vec<_>) = enabled.partition(|n| *n == first);
    lead.into_iter()
        .chain(lead_named)
        .chain(rest)
        .chain(usable.iter().filter(|p| p.disabled).map(name))
        .collect()
}

/// The account a choice launches on: the named profile, or — for None —
/// the ACCOUNT SPREAD's pick now against `readings` (under `manual`, the
/// default; with none, the CLI's own login: None). The readings are the
/// app's own, so a frame never reads the usage files.
pub fn resolved_profile(
    choice: Option<&str>,
    profiles: &[nebula_core::profiles::Profile],
    harness: &str,
    policy: nebula_core::accounts::Policy,
    readings: &[nebula_core::usage::AccountUsage],
    project_default: Option<&str>,
) -> Option<String> {
    match choice {
        Some(name) => Some(name.to_string()),
        None => auto_pick(profiles, harness, policy, readings, project_default)
            .profile
            .map(|p| p.name.trim().to_string()),
    }
}

/// What a launch naming no profile runs on, in the daemon's order
/// (`accounts::pick_for_project`): the project's default account for the
/// harness, else the ACCOUNT SPREAD's pick now against `readings`.
pub fn auto_pick<'a>(
    profiles: &'a [nebula_core::profiles::Profile],
    harness: &str,
    policy: nebula_core::accounts::Policy,
    readings: &[nebula_core::usage::AccountUsage],
    project_default: Option<&str>,
) -> nebula_core::accounts::Pick<'a> {
    nebula_core::accounts::pick_for_project(
        profiles,
        harness,
        project_default,
        policy,
        nebula_core::clock::now_secs() as i64,
        |key| readings.iter().find(|u| u.account == key).cloned(),
    )
}

/// The project default when it is one a launch can run on: enabled and an
/// account of `harness`; a stale one falls through, as at the daemon.
fn usable_project_default<'a>(
    profiles: &[nebula_core::profiles::Profile],
    harness: &str,
    project_default: Option<&'a str>,
) -> Option<&'a str> {
    let name = project_default.map(str::trim).filter(|n| !n.is_empty())?;
    nebula_core::profiles::enabled_for(profiles, harness)
        .iter()
        .any(|p| p.name.trim() == name)
        .then_some(name)
}

/// How a profile choice reads: `auto → max2` (what the spread would pick
/// now), `auto → work (project default)`, `none`, `work`, `work (project
/// default)` for the project's own, or `old (off)` for a switched-off
/// profile.
pub fn profile_choice_label(
    choice: Option<&str>,
    profiles: &[nebula_core::profiles::Profile],
    harness: &str,
    policy: nebula_core::accounts::Policy,
    readings: &[nebula_core::usage::AccountUsage],
    project_default: Option<&str>,
) -> String {
    let pick = auto_pick(profiles, harness, policy, readings, project_default);
    let by_project = pick.reason == nebula_core::accounts::Reason::ProjectDefault;
    match choice {
        None if by_project || policy.spread != nebula_core::accounts::Spread::Manual => {
            match pick.profile {
                Some(p) if by_project => format!("auto → {} (project default)", p.name.trim()),
                Some(p) => format!("auto → {}", p.name.trim()),
                None => "auto → own login".into(),
            }
        }
        None => "none".into(),
        Some(name) if profiles.iter().any(|p| p.name.trim() == name && p.disabled) => {
            format!("{name} (off)")
        }
        Some(name) if usable_project_default(profiles, harness, project_default) == Some(name) => {
            format!("{name} (project default)")
        }
        Some(name) => name.to_string(),
    }
}

/// The ACCOUNT SPREAD the config sets (Settings → Sessions → Accounts).
pub(crate) fn spread_policy(cfg: &Config) -> nebula_core::accounts::Policy {
    nebula_core::accounts::Policy::from_words(
        Some(&cfg.account_spread),
        Some(&cfg.account_spread_threshold),
    )
}

/// `^A` in the box, or a click on its profile field: the launch steps to
/// the next PROFILE of its harness, in place — the typed text and its
/// caret stay as they are. A harness with no profiles says how to add one.
pub(crate) fn cycle_box_profile(app: &mut App) {
    refresh_profiles(app);
    let profiles = app.launcher.profiles.clone();
    let policy = app.launcher.spread;
    let Some(Overlay::Prompt(prompt)) = &mut app.modals.overlay else {
        return;
    };
    let PromptKind::QuickPrompt(launch) = &mut prompt.kind else {
        return;
    };
    let target = launch.target.clone();
    let harness = launch.harness_id().to_string();
    let project = project_default(app, &target, &harness);
    let Some(Overlay::Prompt(prompt)) = &mut app.modals.overlay else {
        return;
    };
    let PromptKind::QuickPrompt(launch) = &mut prompt.kind else {
        return;
    };
    if !launch.cycle_profile(&profiles, policy, project.as_deref()) {
        let harness = launch.harness_id().to_string();
        app.chrome.flash = Some(format!(
            "{harness} has no profiles: kete profile add <name> --harness {harness}"
        ));
    }
}

pub(crate) fn open_box(app: &mut App, launch: QuickLaunch) {
    refresh_profiles(app);
    let (launch, restored) = match app.modals.quick_draft.take() {
        // What the box stands on is where it is opened now, never where
        // the parked one was.
        Some(draft) if draft.launch.aimed_like(&launch) => (
            QuickLaunch {
                under: launch.under,
                ..draft.launch
            },
            Some(draft.input),
        ),
        Some(draft) => (launch, Some(draft.input)),
        None => (launch, None),
    };
    crate::event_loop::open_prompt(app, PromptKind::QuickPrompt(launch));
    if let (Some(input), Some(Overlay::Prompt(prompt))) = (restored, &mut app.modals.overlay) {
        prompt.input = input;
    }
}

/// Open the box a picker reached with NO box up owes — `n`'s NEW SESSION
/// PICKER (`QuickReturn::from_box` false): the pick is the spec, and the
/// DRAFT the last abandoned box left hands back its text alone. The
/// harness was chosen a moment ago, on purpose, so no parked spec
/// overrides it the way [`open_box`]'s same-aim rule would; the slot is
/// emptied all the same, the text being in this box now.
pub(crate) fn open_picked_box(app: &mut App, launch: QuickLaunch) {
    refresh_profiles(app);
    let restored = app.modals.quick_draft.take().map(|draft| draft.input);
    crate::event_loop::open_prompt(app, PromptKind::QuickPrompt(launch));
    if let (Some(input), Some(Overlay::Prompt(prompt))) = (restored, &mut app.modals.overlay) {
        prompt.input = input;
    }
}

impl QuickLaunch {
    /// The launch the `quick_prompt_kind` SETTING describes: that harness
    /// plus its own MODEL / EFFORT defaults from the AGENTS TAB, no preset.
    /// A Cursor effort is re-fitted to the configured family, as every
    /// other launch surface does — the daemon joins the two into one
    /// `--model` id.
    pub fn from_config(target: QuickTarget, cfg: &Config) -> Self {
        let kind = cfg.quick_prompt_kind();
        Self::of_kind(
            target,
            kind,
            None,
            cfg.default_model(kind),
            cfg.default_effort(kind),
            cfg,
        )
    }

    /// The launch a picked harness (and optional MODEL / EFFORT choice)
    /// describes: anything left unpicked falls back to that kind's
    /// configured default, and the effort is fitted to the model.
    pub fn of_kind(
        target: QuickTarget,
        kind: AgentKind,
        custom: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        cfg: &Config,
    ) -> Self {
        // Defaults resolve from the registry descriptor: its own model
        // default, and its effort default fitted to the model.
        let descriptor = cfg.effective_harness(kind, custom.as_deref());
        let model = model.or_else(|| descriptor.default_model().map(str::to_string));
        let effort = fit_effort(
            kind,
            model.as_deref(),
            effort.or_else(|| descriptor.default_effort().map(str::to_string)),
            custom.as_deref(),
        );
        // Under `manual`, the default profile, named so the field shows
        // the account Enter will use. Under another ACCOUNT SPREAD,
        // "auto": no profile is sent and the DAEMON's spread picks at
        // launch, the field saying what it would pick now.
        let mut launch = Self {
            target,
            kind,
            custom,
            model,
            effort,
            preset: None,
            issue: None,
            pr: None,
            under: None,
            cloud: false,
            profile: None,
        };
        launch.profile = launch.preselected_profile(cfg, None);
        launch
    }

    /// The account the box opens on, as the DAEMON would pick it for a
    /// launch naming none: under `manual` named (the project's default
    /// account for the harness, else the harness default, else none) so
    /// the field shows what Enter uses; under another ACCOUNT SPREAD
    /// "auto" (None): no profile is sent and the daemon picks at launch.
    fn preselected_profile(&self, cfg: &Config, project_default: Option<&str>) -> Option<String> {
        let policy = spread_policy(cfg);
        if policy.spread != nebula_core::accounts::Spread::Manual {
            return None;
        }
        resolved_profile(
            None,
            &cfg.profiles,
            self.harness_id(),
            policy,
            &[],
            project_default,
        )
    }

    /// The same launch, its account preselected for the project it lands
    /// in: that project's default account for the harness ranks ahead of
    /// the harness default, as at the DAEMON (`accounts::resolve_launch`).
    /// Run on a launch just built ([`QuickLaunch::from_config`],
    /// [`QuickLaunch::of_kind`]), before any choice of the user's is put
    /// on it.
    pub fn in_project(mut self, app: &App, cfg: &Config) -> Self {
        let project = target_repo(app, &self.target)
            .and_then(|repo| cfg.project(&repo).default_profile(self.harness_id()));
        self.profile = self.preselected_profile(cfg, project.as_deref());
        self
    }

    /// The same launch on `before`'s PROFILE when it runs the same harness
    /// — what a picker's return trip does, so `^O` picking a model keeps
    /// the account `^A` chose. A different harness keeps its own default.
    pub fn with_profile_of(mut self, before: &QuickLaunch) -> Self {
        if before.harness_id() == self.harness_id() {
            self.profile = before.profile.clone();
        }
        self
    }

    /// The harness id PROFILES are filed under: the custom entry's id, or
    /// the built-in kind's name.
    pub fn harness_id(&self) -> &str {
        self.custom.as_deref().unwrap_or_else(|| self.kind.as_str())
    }

    /// What `^A` steps through for this launch's harness
    /// ([`profile_choices_in`]).
    pub fn profile_choices(
        &self,
        profiles: &[nebula_core::profiles::Profile],
        policy: nebula_core::accounts::Policy,
        project_default: Option<&str>,
    ) -> Vec<Option<String>> {
        profile_choices_in(profiles, self.harness_id(), policy, project_default)
    }

    /// `^A`: step to the next of [`QuickLaunch::profile_choices`],
    /// wrapping. False when the harness has none to step through.
    pub fn cycle_profile(
        &mut self,
        profiles: &[nebula_core::profiles::Profile],
        policy: nebula_core::accounts::Policy,
        project_default: Option<&str>,
    ) -> bool {
        let choices = self.profile_choices(profiles, policy, project_default);
        if choices.is_empty() {
            return false;
        }
        let at = choices.iter().position(|c| *c == self.profile);
        self.profile = match at {
            Some(i) => choices[(i + 1) % choices.len()].clone(),
            None => choices[0].clone(),
        };
        true
    }

    /// The same launch, standing on the modal `under` (or on none). What
    /// every picker's return trip does to the launch it rebuilt, so the box
    /// comes back over the modal it was opened over.
    pub fn with_under(mut self, under: Option<ModalUnder>) -> Self {
        self.under = under;
        self
    }

    /// The same launch, for the pull request `pr` (or for none). What
    /// every picker's return trip does to the launch it rebuilt, so the
    /// PR survives a `Tab` or `Shift+Tab` pick as the issue does.
    pub fn with_pr(mut self, pr: Option<PrLaunch>) -> Self {
        self.pr = pr;
        self
    }

    /// The same launch, for `issue` (or for nothing, with `None`). What
    /// every picker's return trip does to the launch it rebuilt, so the
    /// issue survives a `Tab` or `Shift+Tab` pick.
    pub fn with_issue(mut self, issue: Option<crate::issues::IssueRef>) -> Self {
        self.issue = issue;
        self
    }

    /// The same launch, sent to Claude Cloud when `cloud` — and when the
    /// launch can go there at all: plain Claude, no preset, no issue, no
    /// pull request. What the `Tab` picker's pick does last, after the
    /// issue and the PR are back on the launch it rebuilt.
    pub fn with_cloud(mut self, cloud: bool) -> Self {
        self.cloud = cloud
            && self.kind == AgentKind::Claude
            && self.custom.is_none()
            && self.preset.is_none()
            && self.takes_cloud();
        self
    }

    /// Could this box launch in Claude Cloud once its harness is Claude?
    /// Not one for an issue or a pull request — the DAEMON refuses either
    /// context beside a cloud task — so their `Tab` picker offers no
    /// toggle. (A preset does not count: the `Tab` pick clears it.)
    pub fn takes_cloud(&self) -> bool {
        self.issue.is_none() && self.pr.is_none()
    }

    /// The task Enter sends when the box is empty: an ISSUE SESSION's box
    /// may be sent as it is, the issue being the task. `None` for every
    /// other launch, whose empty box sends no task at all — the CLI starts
    /// bare (`launches_empty`).
    pub fn default_task(&self) -> Option<String> {
        self.issue.as_ref().map(|issue| issue.default_task())
    }

    /// Does Enter on an empty box launch? Every box but a CLAUDE CLOUD
    /// one does: the session starts on the harness, MODEL and EFFORT the
    /// title names with no first prompt — the CLI's own input is it, as
    /// after the NEW SESSION PICKER (`n`) — and a box an AGENT PRESET is
    /// on sends the prefix and postfix alone (nothing at all for a bare
    /// preset). A cloud box cannot: `claude --cloud` takes its task on the
    /// command line, so its empty box is a change of mind. (An ISSUE
    /// SESSION's empty box is `default_task`'s: the issue is the task.)
    pub fn launches_empty(&self) -> bool {
        !self.cloud
    }

    /// The launch an AGENT PRESET describes: its harness, its pinned
    /// MODEL / EFFORT where it has them and that kind's defaults where it
    /// does not — the same resolution an AGENT PRESET launch from the
    /// SESSIONS PANEL does — and its prefix/postfix kept for the compose.
    pub fn of_preset(target: QuickTarget, preset: AgentPreset, cfg: &Config) -> Self {
        let mut launch = Self::of_kind(
            target,
            preset.kind,
            preset.custom_harness.clone(),
            preset.model.clone(),
            preset.effort.clone(),
            cfg,
        );
        launch.preset = Some(preset);
        launch
    }

    /// The checkout this launch is addressed to, to rewrite when a
    /// stand-in becomes the real row: None for a launch that cuts its
    /// own (`QuickTarget::NewWorktree`).
    pub fn worktree_mut(&mut self) -> Option<&mut WorktreeId> {
        match &mut self.target {
            QuickTarget::Worktree(worktree) => Some(worktree),
            QuickTarget::NewWorktree { .. } => None,
        }
    }

    /// The STARTING PROMPT this launch sends for `task`: the text itself,
    /// or the preset's prefix + task + postfix.
    pub fn compose(&self, task: &str) -> String {
        match &self.preset {
            Some(preset) => preset.compose(task),
            None => task.to_string(),
        }
    }

    /// The dialog's title: the issue (when the box is for one), the preset
    /// (when one is applied), the worktree Enter will cut first (when it
    /// is a new one) and the flags it will actually launch with, so Enter
    /// is never a surprise —
    /// `Quick prompt · reviewer (claude · opus · high)`,
    /// `Quick prompt · new worktree yellow-fox-jumps (claude)`,
    /// `Quick prompt · issue #15 · reviewer (claude · opus)`,
    /// `Quick prompt · PR #42 · reviewer (claude · opus)`,
    /// `Quick prompt (claude · cloud · opus)`.
    pub fn title(&self) -> String {
        let harness = self.custom.as_deref().unwrap_or_else(|| self.kind.as_str());
        let opts: Vec<&str> = std::iter::once(harness)
            .chain(self.cloud.then_some("cloud"))
            .chain(self.model.as_deref())
            .chain(self.effort.as_deref())
            .collect();
        let mut head = vec!["Quick prompt".to_string()];
        if let Some(issue) = &self.issue {
            head.push(format!("issue #{}", issue.number));
        }
        if let Some(pr) = &self.pr {
            head.push(format!("PR #{}", pr.number));
        }
        if let Some(preset) = &self.preset {
            head.push(preset.name.clone());
        }
        if let QuickTarget::NewWorktree { branch, .. } = &self.target {
            head.push(format!("new worktree {branch}"));
        }
        format!("{} ({})", head.join(" · "), opts.join(" · "))
    }

    /// The line under the title: what Enter will send.
    pub fn label(&self) -> String {
        match (&self.preset, &self.issue) {
            (Some(preset), issue) => {
                let (sends, empty) = match (preset.has_wrapping(), issue) {
                    (true, Some(_)) => ("prefix + your task + postfix", "fix the issue"),
                    (true, None) => ("prefix + your task + postfix", "prefix + postfix only"),
                    (false, Some(_)) => ("sent as the first prompt", "fix the issue"),
                    (false, None) => ("sent as the first prompt", "start with no prompt"),
                };
                format!("{}: {sends} (empty = {empty})", preset.name)
            }
            (None, Some(issue)) => format!(
                "what should the agent do about #{}? (empty = fix the issue)",
                issue.number
            ),
            (None, None) => match &self.pr {
                Some(pr) => format!(
                    "what should the agent do about PR #{}? (empty = start with no prompt)",
                    pr.number
                ),
                None if self.cloud => "what should Claude do in the cloud?".into(),
                None => "what should the agent do? (empty = start with no prompt)".into(),
            },
        }
    }

    /// Are two launches aimed at the same place — the same checkout, or a
    /// fresh one in the same PROJECT, for the same issue and the same pull
    /// request? The branch a fresh worktree gets is minted when the box
    /// opens, so two boxes aimed at a new checkout in one project are aimed
    /// alike however their names differ. What a parked [`QuickDraft`] is
    /// matched on: the same aim is the same box reopened, so the whole of
    /// it comes back.
    pub fn aimed_like(&self, other: &Self) -> bool {
        let same_place = match (&self.target, &other.target) {
            (QuickTarget::Worktree(a), QuickTarget::Worktree(b)) => a == b,
            (
                QuickTarget::NewWorktree { project: a, .. },
                QuickTarget::NewWorktree { project: b, .. },
            ) => a == b,
            _ => false,
        };
        same_place && self.issue == other.issue && self.pr == other.pr
    }

    /// Does Enter cut a fresh worktree before it launches? The box's frame
    /// turns green and its target row wears a NEW WORKTREE chip while so,
    /// whether the target came from the WORKTREES PANEL or from `Ctrl+N`.
    pub fn is_new_worktree(&self) -> bool {
        matches!(self.target, QuickTarget::NewWorktree { .. })
    }
}

/// The hotkey: open the task box for the selected WORKTREE. Unlike the
/// AGENT PRESETS list this does not ask for FOCUS on the SESSIONS PANEL —
/// the point of a quick prompt is that it works from wherever you are —
/// but it still needs a checkout to run in, so a PROJECT with no worktree
/// selected flashes instead.
///
/// The one exception is the WORKTREES PANEL: `p` there means "a fresh
/// worktree, then this task in it", whatever checkout the cursor is
/// parked on (the root, another checkout) — the checkout does not exist
/// yet, so only the PROJECT has to be selected. Its branch is the same
/// random name the `n` prompt would have offered.
///
/// A cursor parked on an OPEN PRS row — in that panel or, the row still
/// selected, from any other — makes the box a PR SESSION's, the one `e`
/// there hands back less the preset (`open_for_pr`): Enter sends a
/// `CreatePrAgent`, and the fresh worktree is the pull request's own,
/// on its head branch, its stand-in row up under the pull request from
/// the moment Enter is pressed. It used to be the random-branch checkout
/// above, launched with no PR context at all: the session had to check
/// the pull request out by hand, and its row only moved under the pull
/// request once the DAEMON's reconcile noticed the branch — the "slow
/// nesting" that was really a launch aimed at the wrong place.
pub(crate) fn open_quick_prompt(app: &mut App) {
    if app.selected_worktree_pr().is_some() {
        open_for_pr(app);
        return;
    }
    // An issue row is the ISSUES MODAL's row, in the panel: the box
    // carries the issue, into the project's root checkout.
    if app.selected_worktree_issue().is_some() {
        crate::issues::open_prompt_for_row(app);
        return;
    }
    if app.nav.focus == Focus::Worktrees {
        let Some(project) = app.selected_project().map(|p| p.id.clone()) else {
            app.chrome.flash = Some("quick prompt: select a project first".into());
            return;
        };
        let branch = crate::branch_name::random_name(&app.project_branches(&project));
        open_for(app, QuickTarget::NewWorktree { project, branch });
        return;
    }
    let Some(worktree) = app.selected_worktree().map(|w| w.id.clone()) else {
        app.chrome.flash = Some("quick prompt: select a worktree first".into());
        return;
    };
    // The stand-in a previous `p` put up: git is still cutting it, and
    // the box would only be refused at Enter.
    if app.is_placeholder_worktree(&worktree) {
        app.chrome.flash = Some("quick prompt: worktree is still being created".into());
        return;
    }
    open_for(app, QuickTarget::Worktree(worktree));
}

/// Open the box for a known target, resolving the launch options now so
/// the title can name what Enter is about to start.
pub(crate) fn open_for(app: &mut App, target: QuickTarget) {
    let cfg = Config::load();
    let launch = QuickLaunch::from_config(target, &cfg).in_project(app, &cfg);
    open_box(app, launch);
}

/// `p` with the Worktrees cursor on an OPEN PRS row: the box for a PR
/// SESSION on that pull request, titled for it (`Quick prompt · PR #42`),
/// its target row naming the PR and its head branch. Enter sends one
/// `CreatePrAgent` — the typed text its STARTING PROMPT — and, when the
/// PROJECT has no checkout on the head branch yet, puts the stand-in rows
/// up at once, nested under the pull request where the DAEMON's real row
/// will list (`create_agent`, through `placeholder::stage`). Nothing to
/// open when the project has no ROOT WORKTREE to address it to; the
/// footer says so.
fn open_for_pr(app: &mut App) {
    if let Some(launch) = pr_launch(app) {
        open_pr_box(app, launch);
    }
}

/// Put up the box for a PR SESSION `launch` — the PROJECT OPEN PRS GROUP
/// row's `p` and the PULL REQUESTS MODAL's `Enter` alike — starting from
/// the text of a launch on the same pull request the DAEMON refused while
/// another modal was up, when there is one.
pub(crate) fn open_pr_box(app: &mut App, launch: QuickLaunch) {
    // The text of a launch the DAEMON refused while another modal was up
    // (`App::parked_pr_prompt`): this box is where it was headed.
    let url = launch.pr.as_ref().map(|pr| pr.url.clone());
    let parked = match app.modals.parked_pr_prompt.take() {
        Some((for_url, text)) if Some(&for_url) == url.as_ref() => Some(text),
        other => {
            app.modals.parked_pr_prompt = other;
            None
        }
    };
    match parked {
        Some(text) => reopen(app, launch, &text),
        None => open_box(app, launch),
    }
}

/// The launch every PR SESSION box starts from — `p`'s and `e`'s alike:
/// the `quick_prompt_kind` SETTING's harness, the pull request under the
/// Worktrees cursor carried as `QuickLaunch::pr`, and the PROJECT's ROOT
/// WORKTREE as the target, which only names the project the create is
/// addressed to (as the `n` picker's does) — the DAEMON picks the
/// checkout, the PR head branch's own. None off a pull request row, and,
/// with a flash, when the project has no root to address it to.
fn pr_launch(app: &mut App) -> Option<QuickLaunch> {
    let pr = app.selected_worktree_pr().cloned()?;
    let project = app.selected_project()?.id.clone();
    pr_launch_for(app, &project, &pr)
}

/// The PR SESSION launch for `pr` on `project` — what [`pr_launch`] builds
/// for the Worktrees cursor's row, for any open pull request: the PULL
/// REQUESTS MODAL's rows launch through it too. None, with a flash, when
/// the project has no ROOT WORKTREE to address the create to.
pub(crate) fn pr_launch_for(
    app: &mut App,
    project: &ProjectId,
    pr: &OpenPr,
) -> Option<QuickLaunch> {
    let Some(root) = app.root_worktree(project) else {
        app.chrome.flash = Some("the project has no ROOT WORKTREE for this PR session".into());
        return None;
    };
    Some(
        QuickLaunch::from_config(QuickTarget::Worktree(root), &Config::load())
            .in_project(app, &Config::load())
            .with_pr(Some(PrLaunch::of(pr))),
    )
}

/// The checkout a picker opened from the box is built against — the
/// `KindPicker` and the `AgentPresetsView` each carry one. For a WORKTREE
/// that does not exist yet it is the PROJECT's ROOT WORKTREE, which every
/// project has whether the panel shows it or not: in quick mode neither
/// picker launches into it, they hand the pick back and the launch keeps
/// its own `target`. None only if the project vanished meanwhile.
pub(crate) fn picker_context(app: &App, launch: &QuickLaunch) -> Option<WorktreeId> {
    match &launch.target {
        QuickTarget::Worktree(id) => Some(id.clone()),
        QuickTarget::NewWorktree { project, .. } => app.root_worktree(project),
    }
}

/// Put the box back after one of its pickers — on a pick, with the new
/// launch, and on Esc with the one it left with. The text is restored
/// either way; that is the whole point of the round trip.
pub(crate) fn reopen(app: &mut App, launch: QuickLaunch, text: &str) {
    refresh_profiles(app);
    crate::event_loop::open_prompt(app, PromptKind::QuickPrompt(launch));
    if let Some(crate::app::Overlay::Prompt(prompt)) = &mut app.modals.overlay {
        prompt.input.insert_str(text);
    }
}

/// The box a picker draws under itself: the launch as it stood with the
/// text that was typed into it, built to be drawn and never opened as an
/// overlay. `^P` layers the PROJECT PICKER over this rather than taking
/// the box off the screen, so the task is still in front of you while you
/// pick where it lands; [`reopen`] is what actually hands the box back.
pub(crate) fn backdrop_box(back: &QuickReturn) -> PromptDialog {
    let launch = back.launch.clone();
    PromptDialog::new(
        launch.title(),
        launch.label(),
        back.text.clone(),
        PromptKind::QuickPrompt(launch),
    )
}

/// `Ctrl+N` in the box: flip this one launch between the selected
/// WORKTREE and a fresh one, from whichever panel `p` was pressed in —
/// the WORKTREES PANEL's "cut a worktree first" without walking over to
/// it, and the way back into the checkout under the cursor from there.
/// Flipping on mints the same random branch `n` would offer — or, for an
/// ISSUE SESSION, one named after the issue (`issue-15-fix-login`); flipping off
/// needs a real checkout under the cursor (not an OPEN PRS row, not a
/// stand-in git is still cutting) and says so while keeping the fresh one
/// otherwise. A PR SESSION's box has nothing to flip: the DAEMON picks
/// its checkout (the PR head branch's own), so the key only says so. The
/// box is rebuilt so its title and frame follow the target, with the
/// typed text and the caret exactly where they were.
pub(crate) fn toggle_new_worktree(app: &mut App, launch: QuickLaunch, input: TextInput) {
    if launch.pr.is_some() {
        app.chrome.flash =
            Some("quick prompt: a PR session runs in the pull request's own checkout".into());
        return;
    }
    let target = match &launch.target {
        QuickTarget::NewWorktree { .. } => match app.selected_worktree().map(|w| w.id.clone()) {
            None => Err("quick prompt: no worktree under the cursor: keeping the new one"),
            Some(worktree) if app.is_placeholder_worktree(&worktree) => {
                Err("quick prompt: worktree is still being created: keeping the new one")
            }
            Some(worktree) => Ok(QuickTarget::Worktree(worktree)),
        },
        QuickTarget::Worktree(id) => match app
            .tree
            .worktrees
            .iter()
            .find(|w| &w.id == id)
            .map(|w| w.project_id.clone())
        {
            None => Err("quick prompt: worktree no longer exists"),
            Some(project) => {
                let taken = app.project_branches(&project);
                let branch = match &launch.issue {
                    Some(issue) => {
                        crate::branch_name::issue_name(issue.number, &issue.title, &taken)
                    }
                    None => crate::branch_name::random_name(&taken),
                };
                Ok(QuickTarget::NewWorktree { project, branch })
            }
        },
    };
    match target {
        Err(msg) => app.chrome.flash = Some(msg.into()),
        Ok(target) => {
            let launch = QuickLaunch { target, ..launch };
            crate::event_loop::open_prompt(app, PromptKind::QuickPrompt(launch));
            if let Some(Overlay::Prompt(prompt)) = &mut app.modals.overlay {
                prompt.input = input;
            }
        }
    }
}

/// The branch the box's target row names: the selected checkout's, or the
/// one Enter will cut. None only if the selected worktree vanished while
/// the box was up.
pub(crate) fn target_branch(app: &App, launch: &QuickLaunch) -> Option<String> {
    match &launch.target {
        QuickTarget::Worktree(id) => app
            .tree
            .worktrees
            .iter()
            .find(|w| &w.id == id)
            .map(|w| w.branch.clone()),
        QuickTarget::NewWorktree { branch, .. } => Some(branch.clone()),
    }
}

/// `Tab` in the box: which harness this one launch uses. The same AGENT
/// KIND rows the NEW SESSION PICKER offers — so `→` drills into the same
/// MODEL / EFFORT submenus with the same TYPE-AHEAD — but the pick comes
/// back here instead of creating a session, and it clears any AGENT PRESET
/// (a launch spec has one source).
pub(crate) fn open_launch_picker(app: &mut App, back: QuickReturn) {
    let Some(context) = picker_context(app, &back.launch) else {
        app.chrome.flash = Some("project no longer exists".into());
        return;
    };
    crate::agent_picker::open_kind_picker(
        app,
        crate::agent_picker::KindPicker::quick_prompt(context, back),
    );
}

/// `Shift+Tab` in the box: the saved AGENT PRESETS as a picker. The list is
/// the one `e` opens in the SESSIONS PANEL, in picker mode — Enter adopts
/// the row's harness, MODEL / EFFORT and prefix/postfix for this launch and
/// Esc comes back unchanged, while `Ctrl+a` / `Ctrl+e` / `Ctrl+d` manage
/// the presets as they do there and come back to this picker. With none
/// saved yet it opens empty, on the same `Ctrl+a` hint the manager shows.
pub(crate) fn open_preset_picker(app: &mut App, back: QuickReturn) {
    let presets = crate::agent_presets::load();
    let selected = back
        .launch
        .preset
        .as_ref()
        .and_then(|p| presets.iter().position(|row| row.name == p.name))
        .unwrap_or(0);
    let Some(context) = picker_context(app, &back.launch) else {
        app.chrome.flash = Some("project no longer exists".into());
        return;
    };
    let mut view = crate::preset_overlays::AgentPresetsView::new(context, presets);
    view.selected = selected;
    view.quick = Some(back);
    app.modals.overlay = Some(Overlay::AgentPresets(view));
}

/// `e` on a PROJECT OPEN PRS GROUP row: the saved AGENT PRESETS as a
/// picker for a PR SESSION on that pull request. The pick hands the
/// QUICK PROMPT box back with the preset applied and the PR carried
/// (`QuickLaunch::pr`) — or, for a `skip`-task preset, launches at once
/// — and Enter sends a `CreatePrAgent`: the DAEMON runs it in the
/// PROJECT's checkout of the PR's head branch, reusing one already there
/// and cutting one otherwise, with the PR URL and its work rule in the
/// system prompt and the preset's composed text as the first prompt. The
/// box's target is the PROJECT's ROOT WORKTREE, which only names the
/// PROJECT the create is addressed to (as the `n` picker's is). No box
/// was up when the list opened, so Esc closes it rather than putting up
/// an empty prompt nobody asked for.
pub(crate) fn open_preset_picker_for_pr(app: &mut App) {
    let Some(launch) = pr_launch(app) else {
        return;
    };
    open_preset_picker(app, QuickReturn::fresh(launch));
}

// ---- recent checkouts ----

/// How many checkouts the box's recent keys reach (`Alt+1` to `Alt+5`).
pub const RECENT_KEPT: usize = 5;

/// A checkout a session last ran in, for the box's recent keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCheckout {
    pub worktree: WorktreeId,
    /// `project ▸ branch`.
    pub label: String,
    /// The newest status change of a session there, epoch ms.
    pub at: i64,
}

/// Per checkout, the newest `status_changed_at` of its sessions (archived
/// ones count: they were started there), newest first, at most
/// [`RECENT_KEPT`]. Checkouts and projects gone from the tree drop out.
/// The desktop's quick prompt offers the same list (`desktop/src/lib/recent.ts`).
pub fn recent_checkouts(tree: &crate::app::Tree) -> Vec<RecentCheckout> {
    let mut newest: std::collections::HashMap<&WorktreeId, i64> = Default::default();
    for a in &tree.agents {
        let at = newest.entry(&a.worktree_id).or_insert(a.status_changed_at);
        *at = (*at).max(a.status_changed_at);
    }
    let mut out: Vec<RecentCheckout> = newest
        .into_iter()
        .filter_map(|(id, at)| {
            let w = tree.worktrees.iter().find(|w| &w.id == id)?;
            let p = tree.projects.iter().find(|p| p.id == w.project_id)?;
            Some(RecentCheckout {
                worktree: w.id.clone(),
                label: format!("{} ▸ {}", p.name, w.branch),
                at,
            })
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.label.cmp(&b.label)));
    out.truncate(RECENT_KEPT);
    out
}

/// The recent checkouts the box can be aimed at: [`recent_checkouts`] less
/// the stand-ins git is still cutting.
fn usable_recent(app: &App) -> Vec<RecentCheckout> {
    recent_checkouts(&app.tree)
        .into_iter()
        .filter(|r| !app.is_placeholder_worktree(&r.worktree))
        .collect()
}

/// `Alt+<n>` in the box: aim it at the `n`th recent checkout (1-based),
/// the text kept. False when there is no such checkout, or the box is a
/// PR SESSION's, which runs in the pull request's own checkout.
pub(crate) fn pick_recent(app: &mut App, back: QuickReturn, n: usize) -> bool {
    if back.launch.pr.is_some() {
        return false;
    }
    let Some(r) = n
        .checked_sub(1)
        .and_then(|i| usable_recent(app).into_iter().nth(i))
    else {
        return false;
    };
    let launch = QuickLaunch {
        target: QuickTarget::Worktree(r.worktree),
        ..back.launch
    };
    reopen(app, launch, &back.text);
    true
}

/// `^R` in the box: the recent checkouts as a menu, each row with its
/// `Alt+<n>`. A pick aims the box there, as the worktree picker's do.
pub(crate) fn open_recent_picker(app: &mut App, back: QuickReturn) {
    use crate::app::{ContextMenu, MenuAction, MenuItem};
    if back.launch.pr.is_some() {
        app.chrome.flash =
            Some("quick prompt: a PR session runs in the pull request's own checkout".into());
        return;
    }
    let recent = usable_recent(app);
    if recent.is_empty() {
        app.chrome.flash = Some("quick prompt: no session has run anywhere yet".into());
        return;
    }
    let items: Vec<MenuItem> = recent
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let aimed = back.launch.target == QuickTarget::Worktree(r.worktree.clone());
            let mut item = MenuItem::new(
                format!("{}{}", r.label, if aimed { " ✓" } else { "" }),
                MenuAction::PickLaunchWorktree {
                    target: QuickTarget::Worktree(r.worktree),
                    back: Box::new(back.clone()),
                },
            );
            item.key = Some(format!("Alt+{}", i + 1));
            item
        })
        .collect();
    app.modals.overlay = Some(Overlay::Menu(ContextMenu {
        title: Some("Recent".into()),
        items,
        at: None,
        hover: 0,
        area: ratatui::layout::Rect::default(),
        scroll: 0,
        parent: None,
        filter: None,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    mod recent {
        use super::super::*;
        use crate::app::Tree;
        use nebula_core::{AgentStatus, Project, Worktree};

        fn tree(worktrees: &[(&str, &str)], agents: &[(&str, &str, i64, bool)]) -> Tree {
            Tree {
                projects: vec![Project {
                    id: ProjectId("p".into()),
                    name: "repo".into(),
                    repo_path: "/tmp/repo".into(),
                    sort_order: 0,
                }],
                worktrees: worktrees
                    .iter()
                    .map(|(id, project)| Worktree {
                        id: WorktreeId((*id).into()),
                        project_id: ProjectId((*project).into()),
                        path: format!("/tmp/{id}").into(),
                        branch: (*id).into(),
                        is_main: false,
                        sort_order: 0,
                    })
                    .collect(),
                agents: agents
                    .iter()
                    .map(|(id, wt, at, archived)| {
                        let mut a =
                            crate::subagents::tests::agent(id, None, AgentStatus::Finished, 0);
                        a.worktree_id = WorktreeId((*wt).into());
                        a.status_changed_at = *at;
                        a.archived = *archived;
                        a
                    })
                    .collect(),
                ..Tree::default()
            }
        }

        fn ids(t: &Tree) -> Vec<String> {
            recent_checkouts(t)
                .into_iter()
                .map(|r| r.worktree.0)
                .collect()
        }

        #[test]
        fn checkouts_come_newest_session_first_archived_ones_included() {
            let t = tree(
                &[("a", "p"), ("b", "p"), ("c", "p")],
                &[
                    ("1", "a", 10, false),
                    ("2", "b", 30, true),
                    ("3", "a", 20, false),
                ],
            );
            let got = recent_checkouts(&t);
            assert_eq!(
                got.iter()
                    .map(|r| (r.worktree.0.as_str(), r.at, r.label.as_str()))
                    .collect::<Vec<_>>(),
                vec![("b", 30, "repo ▸ b"), ("a", 20, "repo ▸ a")]
            );
        }

        #[test]
        fn gone_checkouts_drop_out_and_five_at_most_are_kept() {
            let t = tree(
                &[("a", "p"), ("x", "gone")],
                &[
                    ("1", "a", 1, false),
                    ("2", "x", 5, false),
                    ("3", "missing", 9, false),
                ],
            );
            assert_eq!(ids(&t), vec!["a"]);
            let names = ["a", "b", "c", "d", "e", "f"];
            let wts: Vec<_> = names.iter().map(|n| (*n, "p")).collect();
            let agents: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(i, n)| (*n, *n, i as i64, false))
                .collect();
            assert_eq!(ids(&tree(&wts, &agents)), vec!["f", "e", "d", "c", "b"]);
        }
    }

    fn worktree() -> QuickTarget {
        QuickTarget::Worktree(WorktreeId::from("wt-1".to_string()))
    }

    fn new_worktree(branch: &str) -> QuickTarget {
        QuickTarget::NewWorktree {
            project: ProjectId::from("p-1".to_string()),
            branch: branch.into(),
        }
    }

    fn preset(name: &str, kind: AgentKind) -> AgentPreset {
        AgentPreset {
            name: name.into(),
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            prefix: String::new(),
            postfix: String::new(),
            skip_task: false,
        }
    }

    #[test]
    fn the_setting_picks_the_harness_and_its_own_model_and_effort() {
        let cfg = Config {
            quick_prompt_kind: "codex".into(),
            codex_model: "gpt-5.1-codex".into(),
            codex_effort: "high".into(),
            claude_model: "opus".into(),
            ..Config::default()
        };
        let launch = QuickLaunch::from_config(worktree(), &cfg);
        assert_eq!(launch.kind, AgentKind::Codex);
        assert_eq!(launch.model.as_deref(), Some("gpt-5.1-codex"));
        assert_eq!(launch.effort.as_deref(), Some("high"));
        assert!(launch.preset.is_none());
    }

    #[test]
    fn default_model_and_effort_pass_no_flags() {
        let launch = QuickLaunch::from_config(worktree(), &Config::default());
        assert_eq!(
            (launch.kind, launch.model, launch.effort),
            (AgentKind::Claude, None, None)
        );
    }

    /// A harness switched off on the AGENTS TAB after it was chosen would
    /// otherwise launch a kind the NEW SESSION PICKER no longer offers.
    #[test]
    fn a_disabled_harness_steps_on_to_an_enabled_one() {
        let cfg = Config {
            quick_prompt_kind: "claude".into(),
            claude_enabled: false,
            ..Config::default()
        };
        assert_eq!(
            QuickLaunch::from_config(worktree(), &cfg).kind,
            AgentKind::Codex
        );
    }

    /// A `Tab` pick names only the harness; the rest still comes from that
    /// kind's AGENTS TAB defaults, and a drilled-into submenu wins.
    #[test]
    fn a_picked_harness_fills_the_rest_from_its_own_defaults() {
        let cfg = Config {
            codex_model: "gpt-5.5".into(),
            codex_effort: "high".into(),
            ..Config::default()
        };
        let launch = QuickLaunch::of_kind(worktree(), AgentKind::Codex, None, None, None, &cfg);
        assert_eq!(launch.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(launch.effort.as_deref(), Some("high"));
        let launch = QuickLaunch::of_kind(
            worktree(),
            AgentKind::Codex,
            None,
            Some("gpt-5.1-codex".into()),
            Some("low".into()),
            &cfg,
        );
        assert_eq!(launch.model.as_deref(), Some("gpt-5.1-codex"));
        assert_eq!(launch.effort.as_deref(), Some("low"));
    }

    /// A preset pins what it names and inherits the rest, exactly as an
    /// AGENT PRESET launch from the SESSIONS PANEL does.
    #[test]
    fn a_preset_pins_what_it_names_and_wraps_the_task() {
        let cfg = Config {
            claude_effort: "high".into(),
            ..Config::default()
        };
        let reviewer = AgentPreset {
            model: Some("opus".into()),
            prefix: "Be strict.".into(),
            postfix: "Run the tests.".into(),
            ..preset("reviewer", AgentKind::Claude)
        };
        let launch = QuickLaunch::of_preset(worktree(), reviewer, &cfg);
        assert_eq!(launch.kind, AgentKind::Claude);
        assert_eq!(launch.model.as_deref(), Some("opus"));
        assert_eq!(
            launch.effort.as_deref(),
            Some("high"),
            "an unpinned effort falls back to the AGENTS TAB default"
        );
        assert_eq!(
            launch.compose("Fix auth"),
            "Be strict.\n\nFix auth\n\nRun the tests."
        );
    }

    /// A parked DRAFT comes back whole only into the box it left: the
    /// same checkout, issue and pull request. The branch a fresh worktree
    /// would get is minted per box, so it never decides the match.
    #[test]
    fn a_draft_matches_the_box_it_was_typed_in() {
        let cfg = Config::default();
        let here = QuickLaunch::from_config(worktree(), &cfg);
        assert!(here.aimed_like(&QuickLaunch::of_preset(
            worktree(),
            preset("reviewer", AgentKind::Codex),
            &cfg
        )));
        assert!(!here.aimed_like(&QuickLaunch::from_config(
            QuickTarget::Worktree(WorktreeId::from("wt-2".to_string())),
            &cfg
        )));
        assert!(!here.aimed_like(&QuickLaunch::from_config(new_worktree("a"), &cfg)));

        // Two boxes aimed at a fresh checkout in one project are the same
        // box, however the random branch came out.
        let fresh = QuickLaunch::from_config(new_worktree("blue-fox-runs"), &cfg);
        assert!(fresh.aimed_like(&QuickLaunch::from_config(
            new_worktree("red-owl-digs"),
            &cfg
        )));

        // The issue and the pull request are part of the aim: a box for
        // one is not the box for another, nor for none.
        let issue = |number| crate::issues::IssueRef {
            url: format!("https://github.com/o/r/issues/{number}"),
            number,
            title: "Fix login".into(),
        };
        let for_15 = here.clone().with_issue(Some(issue(15)));
        assert!(for_15.aimed_like(&here.clone().with_issue(Some(issue(15)))));
        assert!(!for_15.aimed_like(&here.clone().with_issue(Some(issue(16)))));
        assert!(!for_15.aimed_like(&here));
        let for_pr = here.clone().with_pr(Some(PrLaunch {
            url: "https://github.com/o/r/pull/42".into(),
            head: "feat".into(),
            number: 42,
        }));
        assert!(!for_pr.aimed_like(&here));
    }

    #[test]
    fn the_title_and_label_name_the_launch() {
        let cfg = Config::default();
        let plain = QuickLaunch::of_kind(
            worktree(),
            AgentKind::Claude,
            None,
            Some("opus".into()),
            Some("high".into()),
            &cfg,
        );
        assert_eq!(plain.title(), "Quick prompt (claude · opus · high)");
        assert_eq!(
            plain.label(),
            "what should the agent do? (empty = start with no prompt)"
        );
        assert_eq!(plain.compose("do it"), "do it", "no preset, no wrapping");

        let wrapped = QuickLaunch::of_preset(
            worktree(),
            AgentPreset {
                prefix: "Be strict.".into(),
                ..preset("reviewer", AgentKind::Cursor)
            },
            &cfg,
        );
        assert_eq!(wrapped.title(), "Quick prompt · reviewer (cursor)");
        assert_eq!(
            wrapped.label(),
            "reviewer: prefix + your task + postfix (empty = prefix + postfix only)"
        );

        let bare = QuickLaunch::of_preset(worktree(), preset("scratch", AgentKind::Cursor), &cfg);
        assert_eq!(
            bare.label(),
            "scratch: sent as the first prompt (empty = start with no prompt)"
        );
    }

    /// An ISSUE SESSION's box names the issue first, offers the issue as
    /// the task when sent empty, and keeps the issue through a preset.
    #[test]
    fn an_issue_launch_names_the_issue_and_has_a_default_task() {
        let cfg = Config::default();
        let issue = crate::issues::IssueRef {
            url: "https://github.com/o/r/issues/15".into(),
            number: 15,
            title: "Fix login redirect".into(),
        };
        let plain = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg)
            .with_issue(Some(issue.clone()));
        assert_eq!(plain.title(), "Quick prompt · issue #15 (claude)");
        assert_eq!(
            plain.label(),
            "what should the agent do about #15? (empty = fix the issue)"
        );
        assert_eq!(
            plain.default_task().as_deref(),
            Some("Fix GitHub issue #15: Fix login redirect (https://github.com/o/r/issues/15)")
        );
        let wrapped = QuickLaunch::of_preset(
            new_worktree("issue-15-fix-login-redirect"),
            preset("reviewer", AgentKind::Cursor),
            &cfg,
        )
        .with_issue(Some(issue));
        assert_eq!(
            wrapped.title(),
            "Quick prompt · issue #15 · reviewer · new worktree issue-15-fix-login-redirect (cursor)"
        );
        assert_eq!(
            wrapped.label(),
            "reviewer: sent as the first prompt (empty = fix the issue)"
        );
        assert!(wrapped.default_task().is_some());
        let none = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg);
        assert_eq!(
            none.default_task(),
            None,
            "an empty ordinary box sends no task"
        );
    }

    /// A PR SESSION's box names the pull request, keeps it through a
    /// preset, and — unlike an issue's — offers no task when sent empty:
    /// the PR rides the system prompt, the preset's text is the task.
    #[test]
    fn a_pr_launch_names_the_pull_request_and_keeps_it_through_a_preset() {
        let cfg = Config::default();
        let pr = PrLaunch {
            url: "https://github.com/o/r/pull/42".into(),
            head: "fix-login".into(),
            number: 42,
        };
        let plain = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg)
            .with_pr(Some(pr.clone()));
        assert_eq!(plain.title(), "Quick prompt · PR #42 (claude)");
        assert_eq!(
            plain.label(),
            "what should the agent do about PR #42? (empty = start with no prompt)"
        );
        assert_eq!(plain.default_task(), None);
        assert!(
            plain.launches_empty(),
            "an empty box starts the CLI bare in the PR's checkout"
        );
        assert!(!plain.is_new_worktree());

        let wrapped = QuickLaunch::of_preset(
            worktree(),
            AgentPreset {
                prefix: "Be strict.".into(),
                ..preset("reviewer", AgentKind::Cursor)
            },
            &cfg,
        )
        .with_pr(Some(pr.clone()));
        assert_eq!(wrapped.pr.as_ref(), Some(&pr));
        assert_eq!(wrapped.title(), "Quick prompt · PR #42 · reviewer (cursor)");
        assert_eq!(
            wrapped.label(),
            "reviewer: prefix + your task + postfix (empty = prefix + postfix only)"
        );
        assert!(wrapped.launches_empty(), "a preset's task is optional");
        assert_eq!(wrapped.compose("fix it"), "Be strict.\n\nfix it");
    }

    /// A launch into a worktree that does not exist yet says so — and
    /// names the branch Enter is about to cut — before the flags.
    #[test]
    fn the_title_names_the_worktree_a_launch_will_cut_first() {
        let cfg = Config::default();
        let fresh = QuickLaunch::of_kind(
            new_worktree("yellow-fox-jumps"),
            AgentKind::Claude,
            None,
            Some("opus".into()),
            None,
            &cfg,
        );
        assert_eq!(
            fresh.title(),
            "Quick prompt · new worktree yellow-fox-jumps (claude · opus)"
        );
        assert_eq!(
            fresh.label(),
            "what should the agent do? (empty = start with no prompt)"
        );

        let wrapped = QuickLaunch::of_preset(
            new_worktree("yellow-fox-jumps"),
            preset("reviewer", AgentKind::Cursor),
            &cfg,
        );
        assert_eq!(
            wrapped.title(),
            "Quick prompt · reviewer · new worktree yellow-fox-jumps (cursor)"
        );
    }

    /// An empty box launches: with no preset the CLI starts with no first
    /// prompt — the session `n` would start, on the harness and model the
    /// title names — and with an AGENT PRESET on it, whose task is
    /// optional, on the prefix and postfix alone; into a fresh worktree as
    /// much as into the selected one. The label says so.
    #[test]
    fn an_empty_box_launches_with_or_without_a_preset() {
        let cfg = Config::default();
        let plain = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg);
        assert_eq!(plain.title(), "Quick prompt (claude)");
        assert_eq!(
            plain.label(),
            "what should the agent do? (empty = start with no prompt)"
        );
        assert!(plain.launches_empty(), "an empty box starts the CLI bare");
        assert_eq!(plain.compose(""), "", "and sends it no first prompt");

        let wrapped = QuickLaunch {
            preset: Some(preset("reviewer", AgentKind::Claude)),
            ..plain.clone()
        };
        assert!(wrapped.launches_empty());

        // `Ctrl+N` rebuilds the launch around a new target; neither
        // answer changes with it.
        let flipped = QuickLaunch {
            target: new_worktree("fix-login"),
            ..plain
        };
        assert!(flipped.launches_empty());
        let flipped_wrapped = QuickLaunch {
            target: new_worktree("fix-login"),
            ..wrapped
        };
        assert!(flipped_wrapped.launches_empty());
    }

    /// Cloud is a plain Claude launch's alone: another harness, a preset,
    /// an issue or a pull request keeps it off whatever the picker said,
    /// and a cloud box names it in the title and asks for the task.
    #[test]
    fn only_a_plain_claude_launch_goes_to_the_cloud() {
        let cfg = Config::default();
        let claude = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg);
        let cloud = claude.clone().with_cloud(true);
        assert!(cloud.cloud);
        assert_eq!(cloud.title(), "Quick prompt (claude · cloud)");
        assert_eq!(cloud.label(), "what should Claude do in the cloud?");
        assert!(!cloud.launches_empty(), "a cloud launch needs its task");
        assert!(
            claude.launches_empty(),
            "off the cloud the same box starts the CLI bare"
        );
        assert!(!cloud.clone().with_cloud(false).cloud);

        let codex = QuickLaunch::of_kind(worktree(), AgentKind::Codex, None, None, None, &cfg);
        assert!(!codex.with_cloud(true).cloud);
        let wrapped =
            QuickLaunch::of_preset(worktree(), preset("reviewer", AgentKind::Claude), &cfg);
        assert!(!wrapped.with_cloud(true).cloud);
        let issue = claude.clone().with_issue(Some(crate::issues::IssueRef {
            number: 15,
            title: "Login fails".into(),
            url: "https://github.com/o/r/issues/15".into(),
        }));
        assert!(!issue.takes_cloud());
        assert!(!issue.with_cloud(true).cloud);
    }

    /// A switched-off profile is offered last, marked off (a launch naming
    /// it still runs on it); a non-manual ACCOUNT SPREAD opens the box on
    /// "auto" — no profile sent — which says what it would pick.
    #[test]
    fn a_disabled_profile_comes_last_and_a_spread_offers_auto() {
        let profile = |name: &str, default: bool, disabled: bool| nebula_core::profiles::Profile {
            name: name.into(),
            harness: "claude".into(),
            env: Default::default(),
            default,
            use_default: false,
            disabled,
        };
        let cfg = Config {
            profiles: vec![
                profile("main", true, false),
                profile("off", false, true),
                profile("max2", false, false),
            ],
            ..Config::default()
        };
        let claude = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg);
        let manual = nebula_core::accounts::Policy::default();
        assert_eq!(claude.profile.as_deref(), Some("main"));
        assert_eq!(
            claude.profile_choices(&cfg.profiles, manual, None),
            vec![
                Some("main".to_string()),
                Some("max2".to_string()),
                Some("off".to_string())
            ]
        );
        let label = |choice: Option<&str>, policy| {
            profile_choice_label(choice, &cfg.profiles, "claude", policy, &[], None)
        };
        assert_eq!(label(Some("off"), manual), "off (off)");
        assert_eq!(label(Some("max2"), manual), "max2");
        // least_used with no readings anywhere: the default leads the ties.
        let spread = Config {
            account_spread: "least_used".into(),
            ..cfg.clone()
        };
        let policy = spread_policy(&spread);
        assert_eq!(policy.spread, nebula_core::accounts::Spread::LeastUsed);
        let auto = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &spread);
        assert_eq!(auto.profile, None, "auto: the daemon's spread picks");
        assert_eq!(
            auto.profile_choices(&spread.profiles, policy, None),
            vec![
                None,
                Some("main".to_string()),
                Some("max2".to_string()),
                Some("off".to_string())
            ]
        );
        assert_eq!(label(None, policy), "auto → main");
        assert_eq!(label(None, manual), "none");
        let mut all_off = cfg.clone();
        all_off.profiles[0].disabled = true;
        all_off.profiles[2].disabled = true;
        let none = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &all_off);
        assert_eq!(none.profile, None, "a disabled default is no default");
    }

    /// A launch opens on its harness's default PROFILE, `^A` steps through
    /// the harness's own accounts (led by "none" while no default stands
    /// for it), and a model pick on the same harness keeps the choice.
    #[test]
    fn a_launch_opens_on_the_default_profile_and_steps_through_its_harness() {
        let profile = |name: &str, harness: &str, default: bool| nebula_core::profiles::Profile {
            name: name.into(),
            harness: harness.into(),
            env: Default::default(),
            default,
            use_default: false,
            disabled: false,
        };
        let cfg = Config {
            profiles: vec![
                profile("main", "claude", true),
                profile("max2", "claude", false),
                profile("work", "codex", false),
            ],
            ..Config::default()
        };
        let mut claude =
            QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg);
        assert_eq!(claude.profile.as_deref(), Some("main"));
        assert_eq!(
            claude.profile_choices(&cfg.profiles, Default::default(), None),
            vec![Some("main".to_string()), Some("max2".to_string())]
        );
        assert!(claude.cycle_profile(&cfg.profiles, Default::default(), None));
        assert_eq!(claude.profile.as_deref(), Some("max2"));
        assert!(claude.cycle_profile(&cfg.profiles, Default::default(), None));
        assert_eq!(claude.profile.as_deref(), Some("main"), "it wraps");

        let mut codex = QuickLaunch::of_kind(worktree(), AgentKind::Codex, None, None, None, &cfg);
        assert_eq!(codex.profile, None, "no default: the CLI's own login");
        assert_eq!(
            codex.profile_choices(&cfg.profiles, Default::default(), None),
            vec![None, Some("work".to_string())]
        );
        assert!(codex.cycle_profile(&cfg.profiles, Default::default(), None));
        assert_eq!(codex.profile.as_deref(), Some("work"));

        let mut pi = QuickLaunch::of_kind(worktree(), AgentKind::Pi, None, None, None, &cfg);
        assert!(
            !pi.cycle_profile(&cfg.profiles, Default::default(), None),
            "no profiles, no field"
        );

        let picked = QuickLaunch::of_kind(
            worktree(),
            AgentKind::Codex,
            None,
            Some("gpt-5.5".into()),
            None,
            &cfg,
        )
        .with_profile_of(&codex);
        assert_eq!(
            picked.profile.as_deref(),
            Some("work"),
            "same harness keeps it"
        );
        let other = QuickLaunch::of_kind(worktree(), AgentKind::Claude, None, None, None, &cfg)
            .with_profile_of(&codex);
        assert_eq!(
            other.profile.as_deref(),
            Some("main"),
            "another takes its own"
        );
    }
}
