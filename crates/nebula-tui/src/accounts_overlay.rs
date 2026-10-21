//! The ACCOUNTS OVERLAY (`⇧U`): every account kete can launch under —
//! each PROFILE, and each harness's own login in use (`@claude`: a usage
//! reading, or a session running with no profile) — with its plan usage
//! as two block bars (session 5h and weekly; a Copilot account's monthly
//! allowances), when the windows reset, where the numbers came from,
//! whether it is signed in, and the live sessions running on it. A
//! profile the settings got wrong is listed with what is wrong, never
//! left out. `Space` (or `e`) switches a profile on or off, `d` (or `*`)
//! makes it its harness's default, `y` keeps it for the person's own
//! sessions (`(yours)`: no launch a script or the orchestrator starts picks
//! it); each writes the settings layer holding the profiles in place
//! (`nebula_core::profiles`), which the config beat then reloads
//! everywhere. A login is never run from here: an account
//! that needs one shows the command to type.
//!
//! `Tab` turns the overlay to its other view, the WAKE tab
//! (`crate::wake_tab`): each Claude account's wake-up switch and hour,
//! on a strip of the day, with Wake now (`w`) from either view.

use std::path::PathBuf;

use nebula_core::accounts::{self, LoginCheck, LoginState};
use nebula_core::profiles::{self, Profile};
use nebula_core::usage::{self, AccountUsage, ContextUsage};
use ratatui::layout::Rect;

/// One row of the overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    /// The account key: the profile's name, or `@<harness>`.
    pub key: String,
    pub harness: String,
    /// None for a harness's own login.
    pub profile: Option<Profile>,
    pub usage: Option<AccountUsage>,
    pub login: LoginState,
    /// The login was there and has run out.
    pub expired: bool,
    /// The harness's default profile.
    pub default: bool,
    /// Flagged default but switched off, so no launch takes it: the star
    /// is drawn dim.
    pub default_off: bool,
    /// What the settings got wrong about the profile; it cannot launch.
    pub problem: Option<String>,
    /// The live sessions running on the account, by name.
    pub sessions: Vec<String>,
}

impl AccountRow {
    /// What the row is called: the profile's name, or `claude (own login)`.
    pub fn label(&self) -> String {
        match &self.profile {
            Some(p) => p.name.trim().to_string(),
            None => format!("{} (own login)", self.harness),
        }
    }

    pub fn disabled(&self) -> bool {
        self.profile.as_ref().is_some_and(|p| p.disabled)
    }

    /// A profile kept for the person's own sessions; an own login never is.
    pub fn reserved(&self) -> bool {
        self.profile.as_ref().is_some_and(|p| p.reserved)
    }

    /// The login hint for an account that needs one.
    pub fn login_text(&self) -> String {
        if self.problem.is_some() {
            return String::new();
        }
        match self.login {
            LoginState::LoggedIn => "signed in".into(),
            LoginState::NeedsLogin => {
                let lead = if self.expired {
                    "login expired"
                } else {
                    "needs login"
                };
                match accounts::login_command(self.profile.as_ref(), &self.harness) {
                    Some(cmd) => format!("{lead}: {cmd}"),
                    None => lead.into(),
                }
            }
            LoginState::Unknown => String::new(),
        }
    }

    /// How the account is used: `2 sessions: fix-auth, docs`, the names
    /// cut after a few; empty with none running.
    pub fn sessions_text(&self) -> String {
        const NAMED: usize = 3;
        let n = self.sessions.len();
        if n == 0 {
            return String::new();
        }
        let mut names = self
            .sessions
            .iter()
            .take(NAMED)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if n > NAMED {
            names.push_str(&format!(" +{} more", n - NAMED));
        }
        format!("{n} session{}: {names}", if n == 1 { "" } else { "s" })
    }
}

/// The kept readings on disk (`<profiles>/.usage`), for a daemon too old
/// to push them. Unit tests never read the machine's real data dir: a
/// reading Jay's own sessions left there would change what they draw.
pub fn kept_readings() -> Vec<nebula_core::usage::AccountUsage> {
    if cfg!(test) {
        return Vec::new();
    }
    nebula_core::usage::read_all_in(&profiles::profiles_root())
}

/// [`nebula_core::accounts::login_check`], except under unit tests, which
/// never look at the machine's real logins.
pub fn login_lookup(profile: Option<&Profile>, harness: &str) -> LoginCheck {
    if cfg!(test) {
        return LoginCheck {
            state: LoginState::Unknown,
            expired: false,
        };
    }
    accounts::login_check(profile, harness)
}

/// The rows for `all` profiles, the `usage` readings and the `agents`
/// running: every profile in config order — one the settings got wrong
/// too, with its problem — then each own login in use
/// (`accounts::own_login_harnesses`). Each row counts the live sessions
/// on it (`accounts::sessions_by_account`).
pub fn build_rows(
    all: &[Profile],
    usage: &[AccountUsage],
    agents: &[nebula_core::Agent],
    login: impl Fn(Option<&Profile>, &str) -> LoginCheck,
) -> Vec<AccountRow> {
    let reading = |key: &str| usage.iter().find(|u| u.account == key).cloned();
    let by_account = accounts::sessions_by_account(agents);
    let sessions = |key: &str| -> Vec<String> {
        by_account
            .get(key)
            .map(|list| list.iter().map(|a| a.name.clone()).collect())
            .unwrap_or_default()
    };
    let mut rows: Vec<AccountRow> = all
        .iter()
        .map(|p| {
            let key = p.name.trim().to_string();
            let harness = p.harness.trim().to_string();
            let problem = p.problem();
            let check = if problem.is_some() {
                LoginCheck {
                    state: LoginState::Unknown,
                    expired: false,
                }
            } else {
                login(Some(p), &harness)
            };
            let flagged =
                profiles::flagged_default(all, &harness).is_some_and(|d| d.name == p.name);
            AccountRow {
                usage: reading(&key),
                sessions: sessions(&key),
                key,
                profile: Some(p.clone()),
                login: check.state,
                expired: check.expired,
                default: flagged && !p.disabled,
                default_off: flagged && p.disabled,
                problem,
                harness,
            }
        })
        .collect();
    let own = accounts::own_login_harnesses(usage.iter().map(|u| u.account.as_str()), agents);
    for harness in own {
        let key = usage::account_key(None, &harness);
        let check = login(None, &harness);
        rows.push(AccountRow {
            usage: reading(&key),
            sessions: sessions(&key),
            key,
            profile: None,
            login: check.state,
            expired: check.expired,
            default: false,
            default_off: false,
            problem: None,
            harness,
        });
    }
    // The desktop's order: grouped by harness in first-seen order, each
    // harness's profiles before its own login (a stable sort keeps the
    // config order inside a group).
    let mut seen: Vec<String> = Vec::new();
    for r in &rows {
        if !seen.contains(&r.harness) {
            seen.push(r.harness.clone());
        }
    }
    rows.sort_by_key(|r| {
        (
            seen.iter().position(|h| *h == r.harness),
            r.profile.is_none(),
        )
    });
    rows
}

/// What the add, edit, duplicate and rename forms ask.
#[derive(Debug, Clone, PartialEq)]
pub enum FormKind {
    /// A new account: a harness, a name, whether to adopt the CLI's
    /// current login, whether to make it the harness's default, its
    /// variables.
    Add,
    /// The profile `name`: its harness, its kind of login, its variables.
    Edit { name: String },
    /// A copy of the profile `from` under a new name.
    Duplicate { from: String },
    /// The profile `from` under a new name (`profiles::rename`).
    Rename { from: String },
}

/// One field of a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Harness,
    Name,
    Adopt,
    Default,
    Env,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Harness => "Harness",
            Field::Name => "Name",
            Field::Adopt => "Adopt current login",
            Field::Default => "Make default",
            Field::Env => "Variables",
        }
    }
}

/// The add / edit / duplicate / rename form, in the overlay's place.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountForm {
    pub kind: FormKind,
    pub harness: String,
    pub name: String,
    /// `use_default`: run on the CLI's own login instead of a home of its
    /// own.
    pub adopt: bool,
    pub make_default: bool,
    /// `KEY=VALUE` pairs split by `;`: a new account's variables, or an
    /// edit's replacements; empty at edit keeps them (their values are
    /// never shown) unless `clear_env`.
    pub env: String,
    /// Edit: the names of the variables it sets now.
    pub env_keys: Vec<String>,
    /// Edit: `Delete` on the variables asked to drop them all; typing new
    /// ones takes it back.
    pub clear_env: bool,
    /// The field the keys type into.
    pub field: usize,
    /// The name was typed: a harness switch no longer suggests one.
    pub touched: bool,
    pub error: Option<String>,
}

impl AccountForm {
    pub fn fields(&self) -> &'static [Field] {
        match self.kind {
            FormKind::Add => &[
                Field::Harness,
                Field::Name,
                Field::Adopt,
                Field::Default,
                Field::Env,
            ],
            FormKind::Edit { .. } => &[Field::Harness, Field::Adopt, Field::Env],
            FormKind::Duplicate { .. } | FormKind::Rename { .. } => &[Field::Name],
        }
    }

    /// The edit form's variables can be cleared: it has some, and the
    /// clear is not already asked.
    pub fn can_clear_env(&self) -> bool {
        matches!(self.kind, FormKind::Edit { .. }) && !self.env_keys.is_empty() && !self.clear_env
    }

    pub fn current(&self) -> Field {
        let fields = self.fields();
        fields[self.field.min(fields.len() - 1)]
    }

    pub fn title(&self) -> String {
        match &self.kind {
            FormKind::Add => "Add account".into(),
            FormKind::Edit { name } => format!("Edit {name}"),
            FormKind::Duplicate { from } => format!("Duplicate {from}"),
            FormKind::Rename { from } => format!("Rename {from}"),
        }
    }

    /// The field's value as the form draws it.
    pub fn value(&self, field: Field) -> String {
        let tick = |on: bool| if on { "[x]" } else { "[ ]" }.to_string();
        match field {
            Field::Harness => format!("\u{2039} {} \u{203a}", self.harness),
            Field::Name => self.name.clone(),
            Field::Adopt => tick(self.adopt),
            Field::Default => tick(self.make_default),
            Field::Env if self.clear_env && self.env.is_empty() => format!(
                "clear: drops {} (type KEY=VALUE; ... to set new ones instead)",
                self.env_keys.join(", ")
            ),
            Field::Env if self.env.is_empty() && self.env_keys.is_empty() => {
                "none (KEY=VALUE; KEY2=VALUE)".into()
            }
            Field::Env if self.env.is_empty() => {
                format!(
                    "keep {} (type KEY=VALUE; ... to replace, Del clears)",
                    self.env_keys.join(", ")
                )
            }
            Field::Env => self.env.clone(),
        }
    }

    /// Does the field take typing?
    pub fn is_text(field: Field) -> bool {
        matches!(field, Field::Name | Field::Env)
    }
}

/// A free name to start the add form on: `claude-2`, `claude-3`, ...
pub fn suggest_name(base: &str, taken: &[String]) -> String {
    let base: String = base
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let base = if base.is_empty() {
        "account".into()
    } else {
        base
    };
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|n| !taken.iter().any(|t| t.trim() == n))
        .expect("a free name")
}

/// What an ACCOUNT SPREAD does to a launch naming no account, in words
/// (the desktop's `SPREADS`).
pub fn spread_text(word: &str) -> &'static str {
    match accounts::Spread::parse(word) {
        accounts::Spread::Manual => {
            "manual: each harness runs on its starred default, or its own login when none is starred"
        }
        accounts::Spread::Failover => {
            "failover: the default until its 5h or weekly use reaches the threshold, then the next account switched on"
        }
        accounts::Spread::LeastUsed => {
            "least used: whichever switched-on account has used the least; one never measured counts as unused"
        }
    }
}

/// `KEY=VALUE; KEY2=VALUE` as a map, by `kete profile add --env`'s rules.
pub fn env_of(text: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut vars = std::collections::BTreeMap::new();
    for pair in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("variables are KEY=VALUE, not `{pair}`"));
        };
        if key.trim().is_empty() {
            return Err(format!("`{pair}` names no variable"));
        }
        vars.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(vars)
}

/// The warning a remove asks under: the sessions that name the profile
/// (`desktop/src/lib/accounts/pick.ts`'s `removeWarning`), the repos that
/// start on it, and where its home stays.
pub fn remove_warning(
    name: &str,
    agents: &[nebula_core::Agent],
    repos: &[String],
    home: Option<&std::path::Path>,
) -> String {
    let under: Vec<&nebula_core::Agent> = agents
        .iter()
        .filter(|a| a.profile.as_deref().map(str::trim) == Some(name))
        .collect();
    let live: Vec<&str> = under
        .iter()
        .filter(|a| !a.archived)
        .map(|a| a.name.as_str())
        .collect();
    let archived = under.len() - live.len();
    let mut out = format!("Remove the profile \"{name}\"?");
    if under.is_empty() {
        out.push_str(" No session runs under it.");
    } else {
        let mut count = Vec::new();
        if !live.is_empty() {
            count.push(format!(
                "{} {} under it ({})",
                live.len(),
                if live.len() == 1 {
                    "session runs"
                } else {
                    "sessions run"
                },
                live.join(", ")
            ));
        }
        if archived > 0 {
            count.push(format!(
                "{archived} archived {} it",
                if archived == 1 {
                    "one names"
                } else {
                    "ones name"
                }
            ));
        }
        out.push_str(&format!(
            " {}. A running session keeps going, but none of them can start again (resume, restart): kete refuses with \"profile `{name}` is no longer defined\" until a profile of that name is added back.",
            count.join(", ")
        ));
    }
    if !repos.is_empty() {
        out.push_str(&format!(
            " Default account of {}: those fall back to the spread.",
            repos.join(", ")
        ));
    }
    if let Some(home) = home.filter(|h| h.exists()) {
        out.push_str(&format!(
            " Its home is {}: y keeps it there, p deletes it and the login in it.",
            home.display()
        ));
    }
    out
}

/// What the overlay is doing.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Mode {
    #[default]
    List,
    Form(AccountForm),
    /// The remove's confirm, its warning said. `home` is the PROFILE HOME
    /// on disk, which `p` deletes with it.
    Remove {
        name: String,
        message: String,
        home: Option<PathBuf>,
    },
    /// "Use for repo": the projects, the cursor on one; Enter makes the
    /// selected account its default for the harness, or clears it.
    UseFor {
        cursor: usize,
    },
}

/// The overlay's two views. Forms, confirms and the repo picker belong to
/// the Accounts one: the Wake view is only ever up with the list's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Accounts,
    /// WHAKAOHO (wake-up), for the Claude accounts.
    Wake,
}

/// A clickable spot on the overlay, written at draw time: a click on a
/// key's button is that key, pressed, so the click and the key can never
/// do different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountsHit {
    Key(crossterm::event::KeyCode),
    /// A tab of the header.
    Tab(Tab),
    /// An account's row on the Wake view: the cursor goes there.
    WakeRow(usize),
    /// A Wake row's switch: the cursor goes there and the switch flips.
    WakeSwitch(usize),
    /// A Wake row's arrow, earlier (-1) or later (1): the cursor goes there
    /// and the hour moves a quarter hour.
    WakeAnchor(usize, i8),
    /// An account's row: the cursor goes there.
    Row(usize),
    /// A form's field: the form's cursor goes there.
    Field(usize),
    /// A project in "use for repo".
    Project(usize),
    /// A spread, by its place in `accounts::SPREADS`.
    Spread(usize),
}

/// What an edit did, for the footer, and the account to sign in next.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outcome {
    pub message: String,
    /// The account key whose login runs next, in a new terminal.
    pub sign_in: Option<String>,
    /// A rename the daemon's sessions follow: (old, new), sent as
    /// `ClientRequest::RenameProfileSessions`.
    pub rename_sessions: Option<(String, String)>,
}

impl Outcome {
    fn said(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
}

/// The overlay's state.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountsView {
    pub rows: Vec<AccountRow>,
    pub cursor: usize,
    /// Which view is up.
    pub tab: Tab,
    /// The Wake view's state: the rows of the last wake-up status, its
    /// cursor, and the inbox its background reads and saves answer into.
    pub wake: crate::wake_tab::WakeTab,
    /// config.json, and config.local.json beside it: edits go to the
    /// layer whose `profiles` list is in force (`Self::on_profiles_layer`).
    pub config: PathBuf,
    pub local: PathBuf,
    /// Where PROFILE HOMES are made (`profiles::profiles_root`); a temp
    /// dir beside the routed config under unit tests.
    pub root: PathBuf,
    /// What the overlay is doing: the list, a form, a confirm, a picker.
    pub mode: Mode,
    /// Every harness id a profile can be for (the registry's).
    pub harnesses: Vec<String>,
    /// The projects, `(name, repo path)`, for "use for repo" and the
    /// remove's warning.
    pub projects: Vec<(String, PathBuf)>,
    /// Each project's default accounts (repo → harness → profile).
    pub project_accounts:
        std::collections::BTreeMap<PathBuf, std::collections::BTreeMap<String, String>>,
    /// The ACCOUNT SPREAD and its failover threshold, as config.json
    /// spells them.
    pub spread: String,
    pub threshold: String,
    /// The buttons and rows drawn, for clicks.
    pub hits: Vec<(Rect, AccountsHit)>,
    /// An account just added: the cursor lands on it once the rows are
    /// read again.
    pub pending_focus: Option<String>,
    /// The daemon rewrites the sessions' stored profile name on a rename
    /// (`RENAME_PROFILE_PROTOCOL`): a session on the profile no longer
    /// refuses one.
    pub sessions_follow: bool,
    /// Drawn rect, for click-outside.
    pub area: Rect,
}

/// Where profile homes go for an overlay editing `config`: the real
/// profiles root, except under unit tests, which never touch it.
fn root_for(config: &std::path::Path) -> PathBuf {
    if cfg!(test) {
        return config
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("profiles");
    }
    profiles::profiles_root()
}

impl AccountsView {
    pub fn new(rows: Vec<AccountRow>, config: PathBuf, local: PathBuf) -> Self {
        Self {
            rows,
            cursor: 0,
            tab: Tab::Accounts,
            wake: crate::wake_tab::WakeTab::default(),
            root: root_for(&config),
            config,
            local,
            mode: Mode::List,
            harnesses: Vec::new(),
            projects: Vec::new(),
            project_accounts: Default::default(),
            spread: accounts::SPREADS[0].into(),
            threshold: accounts::DEFAULT_THRESHOLD.to_string(),
            hits: Vec::new(),
            pending_focus: None,
            sessions_follow: false,
            area: Rect::default(),
        }
    }

    /// New rows (a push, an edit): the cursor stays, or lands on the
    /// account just added.
    pub fn set_rows(&mut self, rows: Vec<AccountRow>) {
        self.rows = rows;
        if let Some(key) = self.pending_focus.take() {
            match self.rows.iter().position(|r| r.key == key) {
                Some(i) => self.cursor = i,
                None => self.pending_focus = Some(key),
            }
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// Every profile's name, across harnesses (names are unique there).
    fn taken(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| r.profile.as_ref().map(|p| p.name.trim().to_string()))
            .collect()
    }

    fn all_profiles(&self) -> Vec<Profile> {
        self.rows.iter().filter_map(|r| r.profile.clone()).collect()
    }

    /// `a`: the add form, on the selected row's harness (else the first
    /// one the registry lists), its name suggested.
    pub fn open_add(&mut self) {
        let harness = self
            .selected()
            .map(|r| r.harness.clone())
            .filter(|h| self.harnesses.contains(h))
            .or_else(|| self.harnesses.first().cloned())
            .unwrap_or_else(|| "claude".into());
        let name = suggest_name(&harness, &self.taken());
        self.mode = Mode::Form(AccountForm {
            kind: FormKind::Add,
            harness,
            name,
            adopt: false,
            make_default: false,
            env: String::new(),
            env_keys: Vec::new(),
            clear_env: false,
            field: 1,
            touched: false,
            error: None,
        });
    }

    /// `Enter`: the edit form on the selected profile.
    pub fn open_edit(&mut self) -> Result<(), String> {
        let profile = self.selected_profile()?;
        self.mode = Mode::Form(AccountForm {
            kind: FormKind::Edit {
                name: profile.name.trim().to_string(),
            },
            harness: profile.harness.trim().to_string(),
            name: profile.name.trim().to_string(),
            adopt: profile.use_default,
            make_default: profile.default,
            env: String::new(),
            env_keys: profile.env.keys().cloned().collect(),
            clear_env: false,
            field: 0,
            touched: true,
            error: None,
        });
        Ok(())
    }

    /// `c`: the duplicate form on the selected profile.
    pub fn open_duplicate(&mut self) -> Result<(), String> {
        let profile = self.selected_profile()?;
        let from = profile.name.trim().to_string();
        self.mode = Mode::Form(AccountForm {
            name: suggest_name(&from, &self.taken()),
            kind: FormKind::Duplicate { from },
            harness: profile.harness.trim().to_string(),
            adopt: profile.use_default,
            make_default: false,
            env: String::new(),
            env_keys: Vec::new(),
            clear_env: false,
            field: 0,
            touched: false,
            error: None,
        });
        Ok(())
    }

    /// `x`: the remove's confirm, warning of the sessions and repos that
    /// use the account.
    pub fn open_remove(&mut self, agents: &[nebula_core::Agent]) -> Result<(), String> {
        let profile = self.selected_profile()?;
        let name = profile.name.trim().to_string();
        let harness = profile.harness.trim().to_string();
        let repos: Vec<String> = self
            .projects
            .iter()
            .filter(|(_, repo)| {
                self.project_accounts
                    .get(repo)
                    .and_then(|m| m.get(&harness))
                    .is_some_and(|n| n.trim() == name)
            })
            .map(|(n, _)| n.clone())
            .collect();
        let home = profile.home_in(&self.root).filter(|h| h.exists());
        let message = remove_warning(&name, agents, &repos, home.as_deref());
        self.mode = Mode::Remove {
            name,
            message,
            home,
        };
        Ok(())
    }

    /// `r`: the rename form on the selected profile, refused up front
    /// while a session names it (`profiles::rename_blocker`) unless the
    /// daemon's sessions follow a rename.
    pub fn open_rename(&mut self, agents: &[nebula_core::Agent]) -> Result<(), String> {
        let profile = self.selected_profile()?;
        let from = profile.name.trim().to_string();
        if let Some(why) = profiles::rename_blocker(agents, &from).filter(|_| !self.sessions_follow)
        {
            return Err(why);
        }
        self.mode = Mode::Form(AccountForm {
            name: from.clone(),
            kind: FormKind::Rename { from },
            harness: profile.harness.trim().to_string(),
            adopt: profile.use_default,
            make_default: false,
            env: String::new(),
            env_keys: Vec::new(),
            clear_env: false,
            field: 0,
            touched: true,
            error: None,
        });
        Ok(())
    }

    /// `p`: "use for repo", the projects listed.
    pub fn open_use_for(&mut self) -> Result<(), String> {
        let profile = self.selected_profile()?;
        if let Some(problem) = profile.problem() {
            return Err(format!("{}: {problem}", profile.name.trim()));
        }
        if self.projects.is_empty() {
            return Err("no repos yet: add one first".into());
        }
        self.mode = Mode::UseFor { cursor: 0 };
        Ok(())
    }

    fn selected_profile(&self) -> Result<Profile, String> {
        let row = self.selected().ok_or("no account selected")?;
        row.profile.clone().ok_or_else(|| {
            format!(
                "{} is the CLI's own login: add it as a profile first (a: add, adopt current login)",
                row.label()
            )
        })
    }

    /// The registry check `kete profile add` makes.
    fn check_harness(&self, harness: &str) -> Result<(), String> {
        if self.harnesses.is_empty() || self.harnesses.iter().any(|h| h == harness.trim()) {
            return Ok(());
        }
        Err(format!(
            "unknown harness `{}`: expected one of {}",
            harness.trim(),
            self.harnesses.join(", ")
        ))
    }

    /// Make the profile's home, as `kete profile add` does.
    fn prepare(&self, profile: &Profile) -> Result<Option<PathBuf>, String> {
        let home = profile.home_in(&self.root);
        if let Some(home) = &home {
            profile
                .prepare_in(&self.root, None)
                .map_err(|e| format!("added, but could not make {}: {e}", home.display()))?;
        }
        Ok(home)
    }

    /// Enter on a form: add, save, duplicate or rename. The form stays up
    /// with the error on a refusal. `agents` are the sessions a rename
    /// checks.
    pub fn submit_form(&mut self, agents: &[nebula_core::Agent]) -> Result<Outcome, String> {
        let Mode::Form(form) = &self.mode else {
            return Err("no form open".into());
        };
        let form = form.clone();
        let done = match &form.kind {
            FormKind::Add => self.add(&form),
            FormKind::Edit { name } => self.save_edit(name, &form),
            FormKind::Duplicate { from } => self.duplicate(from, &form),
            FormKind::Rename { from } => self.rename(from, &form, agents),
        };
        match done {
            Ok(outcome) => {
                self.mode = Mode::List;
                Ok(outcome)
            }
            Err(e) => {
                if let Mode::Form(f) = &mut self.mode {
                    f.error = Some(e.clone());
                }
                Err(e)
            }
        }
    }

    /// The account just made, signed in next when it needs it: a home of
    /// its own always does; an adopted login only when it reads signed
    /// out. None for a harness kete knows no sign-in for.
    fn sign_in_next(profile: &Profile, home: Option<&std::path::Path>) -> Option<String> {
        accounts::login_command(Some(profile), &profile.harness)?;
        let needed = home.is_some()
            || login_lookup(Some(profile), &profile.harness).state == LoginState::NeedsLogin;
        needed.then(|| profile.name.trim().to_string())
    }

    fn add(&mut self, form: &AccountForm) -> Result<Outcome, String> {
        self.check_harness(&form.harness)?;
        let profile = Profile {
            name: form.name.trim().to_string(),
            harness: form.harness.trim().to_string(),
            env: env_of(&form.env)?,
            default: form.make_default,
            use_default: form.adopt,
            disabled: false,
            reserved: false,
        };
        if let Some(problem) = profile.problem() {
            return Err(problem);
        }
        self.on_profiles_layer(|layer| profiles::add_to_file(layer, &profile))?;
        let home = self.prepare(&profile)?;
        self.focus_key(&profile.name);
        // Signing in is the point of adding one: it runs next, unless the
        // profile adopted a login that is already there.
        Ok(match Self::sign_in_next(&profile, home.as_deref()) {
            Some(account) => Outcome {
                message: format!("added {}: signing it in", profile.name),
                sign_in: Some(account),
                ..Outcome::default()
            },
            None if form.adopt => Outcome::said(format!(
                "added {} on {}'s current login",
                profile.name, profile.harness
            )),
            None => Outcome::said(format!(
                "added {}{}: kete knows no sign-in for {}, sign in from a session under it",
                profile.name,
                home.map(|h| format!(", home {}", h.display()))
                    .unwrap_or_default(),
                profile.harness
            )),
        })
    }

    fn save_edit(&mut self, name: &str, form: &AccountForm) -> Result<Outcome, String> {
        self.check_harness(&form.harness)?;
        let Some(mut profile) = self
            .all_profiles()
            .into_iter()
            .find(|p| p.name.trim() == name)
        else {
            return Err(format!("no profile named `{name}`"));
        };
        // The name and the switch stay; the rest is the form's. Typed
        // variables replace the old ones, a clear drops them, neither
        // keeps them.
        profile.harness = form.harness.trim().to_string();
        profile.use_default = form.adopt;
        if !form.env.trim().is_empty() {
            profile.env = env_of(&form.env)?;
        } else if form.clear_env {
            profile.env.clear();
        }
        if let Some(problem) = profile.problem() {
            return Err(problem);
        }
        self.on_profiles_layer(|layer| profiles::update_in_file(layer, &profile))?;
        self.prepare(&profile)?;
        Ok(Outcome::said(format!("saved {name}")))
    }

    fn duplicate(&mut self, from: &str, form: &AccountForm) -> Result<Outcome, String> {
        // Its harness, variables and kind of login; neither its star nor
        // its switch, and a home of its own (it signs in separately).
        let copy =
            self.on_profiles_layer(|layer| profiles::duplicate_in_file(layer, from, &form.name))?;
        let home = self.prepare(&copy)?;
        self.focus_key(&copy.name);
        Ok(match Self::sign_in_next(&copy, home.as_deref()) {
            Some(account) => Outcome {
                message: format!("added {}: signing it in", copy.name),
                sign_in: Some(account),
                ..Outcome::default()
            },
            None => Outcome::said(format!("added {}", copy.name)),
        })
    }

    /// The rename's Enter: `profiles::rename` moves the home and every
    /// name in the settings (it takes the settings locks itself). Where
    /// the daemon's sessions follow, `profiles::rename_following`, and
    /// the outcome carries the request that moves them.
    fn rename(
        &mut self,
        from: &str,
        form: &AccountForm,
        agents: &[nebula_core::Agent],
    ) -> Result<Outcome, String> {
        let done = if self.sessions_follow {
            profiles::rename_following(&self.config, &self.local, &self.root, from, &form.name)?
        } else {
            profiles::rename(
                &self.config,
                &self.local,
                &self.root,
                from,
                &form.name,
                agents,
            )?
        };
        let new = done.profile.name.clone();
        self.focus_key(&new);
        let named = agents
            .iter()
            .filter(|a| a.profile.as_deref().map(str::trim) == Some(from.trim()))
            .count();
        let mut message = format!("renamed {from} to {new}");
        if self.sessions_follow && named > 0 {
            message.push_str(&format!(
                "; {named} session{} follow{} (a running one on its next restart)",
                if named == 1 { "" } else { "s" },
                if named == 1 { "s" } else { "" }
            ));
        }
        if !done.projects.is_empty() {
            message.push_str(&format!(
                ", the default of {} repo{}",
                done.projects.len(),
                if done.projects.len() == 1 { "" } else { "s" }
            ));
        }
        if !done.archived.is_empty() {
            message.push_str(&format!(
                "; {} archived session{} still name {from}",
                done.archived.len(),
                if done.archived.len() == 1 { "" } else { "s" }
            ));
        }
        let mut outcome = Outcome::said(message);
        if self.sessions_follow && named > 0 {
            outcome.rename_sessions = Some((from.trim().to_string(), new));
        }
        Ok(outcome)
    }

    /// The remove's yes; with `purge` its home (the login) goes too. The
    /// same core path as `kete profile remove [--purge]`.
    pub fn confirm_remove(&mut self, purge: bool) -> Result<Outcome, String> {
        let Mode::Remove { name, .. } = &self.mode else {
            return Err("nothing to remove".into());
        };
        let name = name.clone();
        self.mode = Mode::List;
        let root = self.root.clone();
        let removed =
            self.on_profiles_layer(|layer| profiles::remove_in_file(layer, &root, &name, purge))?;
        self.rows.retain(|r| r.key != name);
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        Ok(Outcome::said(match removed.home {
            Some(h) if removed.purged => format!("removed {name} and deleted {}", h.display()),
            Some(h) => format!("removed {name}: its home stays at {}", h.display()),
            None => format!("removed {name}"),
        }))
    }

    /// Enter in "use for repo": the selected account becomes the picked
    /// project's default for its harness, or, already it, is cleared.
    pub fn toggle_use_for(&mut self) -> Result<Outcome, String> {
        let Mode::UseFor { cursor } = self.mode else {
            return Err("no repo picked".into());
        };
        let profile = self.selected_profile()?;
        let (project, repo) = self.projects.get(cursor).cloned().ok_or("no repo picked")?;
        let name = profile.name.trim().to_string();
        let harness = profiles::project_default_harness(&self.all_profiles(), &name)?;
        let mine = self
            .project_accounts
            .get(&repo)
            .and_then(|m| m.get(&harness))
            .is_some_and(|n| n.trim() == name);
        let set = (!mine).then_some(name.as_str());
        let repo_key = repo.to_string_lossy().into_owned();
        let layer = profiles::layer_for(&self.config, &self.local, profiles::PROJECTS_KEY)?;
        {
            let _lock = if layer == self.local {
                Some(nebula_core::settings::lock(&self.config).map_err(|e| e.to_string())?)
            } else {
                None
            };
            profiles::set_project_default_in_file(&layer, &repo_key, &harness, set)?;
        }
        let entry = self.project_accounts.entry(repo).or_default();
        match set {
            Some(n) => {
                entry.insert(harness.clone(), n.to_string());
            }
            None => {
                entry.remove(&harness);
            }
        }
        Ok(Outcome::said(match set {
            Some(n) => {
                format!("{harness} sessions started in {project} without a pick now run on {n}")
            }
            None => format!("{project} has no default {harness} account now: the spread picks"),
        }))
    }

    /// `s` / a click on a spread: the ACCOUNT SPREAD set to `word`.
    pub fn set_spread(&mut self, word: &str) -> Result<Outcome, String> {
        let word = accounts::Spread::parse(word).as_str();
        crate::config::Config::update_in(&self.config, &self.local, |c| {
            c.account_spread = word.to_string()
        })
        .map_err(|e| format!("couldn't save the spread: {e}"))?;
        self.spread = word.to_string();
        Ok(Outcome::said(format!(
            "account spread: {}",
            spread_text(word)
        )))
    }

    /// `s`: the next spread.
    pub fn cycle_spread(&mut self) -> Result<Outcome, String> {
        let at = accounts::SPREADS
            .iter()
            .position(|s| *s == accounts::Spread::parse(&self.spread).as_str())
            .unwrap_or(0);
        self.set_spread(accounts::SPREADS[(at + 1) % accounts::SPREADS.len()])
    }

    /// `t`: the next failover threshold.
    pub fn cycle_threshold(&mut self) -> Result<Outcome, String> {
        let at = accounts::THRESHOLDS
            .iter()
            .position(|t| *t == self.threshold.trim())
            .map_or(0, |i| (i + 1) % accounts::THRESHOLDS.len());
        let next = accounts::THRESHOLDS[at];
        crate::config::Config::update_in(&self.config, &self.local, |c| {
            c.account_spread_threshold = next.to_string()
        })
        .map_err(|e| format!("couldn't save the threshold: {e}"))?;
        self.threshold = next.to_string();
        Ok(Outcome::said(format!("failover threshold: {next}%")))
    }

    /// Put the cursor on the account `key` once the rows hold it.
    fn focus_key(&mut self, key: &str) {
        self.pending_focus = Some(key.trim().to_string());
    }

    pub fn selected(&self) -> Option<&AccountRow> {
        self.rows.get(self.cursor)
    }

    /// Is an account picked on the view that is up?
    pub fn has_account(&self) -> bool {
        match self.tab {
            Tab::Wake => self.wake.selected().is_some(),
            Tab::Accounts => self.selected().is_some(),
        }
    }

    /// The account `w` wakes now: the Wake view's row, or the Accounts
    /// view's picked one when it is a Claude account (wake-up runs those
    /// only).
    pub fn wake_target(&self) -> Result<String, String> {
        match self.tab {
            Tab::Wake => {
                let row = self
                    .wake
                    .selected()
                    .ok_or("no account to wake yet: the list is still loading")?;
                // A session that can't sign in would sit at Claude's login
                // until the two minutes run out: say so now instead.
                if !row.logged_in {
                    return Err(format!("log {} in first: {}", row.account, row.login_hint));
                }
                Ok(row.account.clone())
            }
            Tab::Accounts => {
                let row = self.selected().ok_or("no account selected")?;
                if row.harness != "claude" {
                    return Err(format!(
                        "wake-up runs Claude accounts only: {} is a {} account",
                        row.label(),
                        row.harness
                    ));
                }
                if let Some(problem) = &row.problem {
                    return Err(format!("{}: {problem}", row.label()));
                }
                if row.login == LoginState::NeedsLogin {
                    let how = accounts::login_command(row.profile.as_ref(), &row.harness)
                        .unwrap_or_else(|| "sign it in".into());
                    return Err(format!("log {} in first: {how}", row.label()));
                }
                Ok(row.key.clone())
            }
        }
    }

    pub fn step(&mut self, delta: i32) {
        if self.tab == Tab::Wake {
            self.wake.step(delta);
            return;
        }
        if self.rows.is_empty() {
            self.cursor = 0;
            return;
        }
        let n = self.rows.len() as i32;
        self.cursor = (self.cursor as i32 + delta).clamp(0, n - 1) as usize;
    }

    /// Run a profile edit on the layer the profiles come from: a
    /// config.local.json holding a `profiles` key replaces config.json's
    /// list whole, so the edit goes there, under config.json's lock too (the
    /// settings writers take that one for both layers). A local layer that
    /// is there but unreadable is refused: there is no telling which list
    /// is in force.
    fn on_profiles_layer<T>(
        &self,
        edit: impl FnOnce(&std::path::Path) -> Result<T, String>,
    ) -> Result<T, String> {
        let held = nebula_core::settings::read_object(&self.local)
            .map_err(|e| format!("{}: {e}: fix it, then try again", self.local.display()))?;
        if held.is_some_and(|obj| obj.contains_key(profiles::CONFIG_KEY)) {
            let _lock = nebula_core::settings::lock(&self.config).map_err(|e| e.to_string())?;
            edit(&self.local)
        } else {
            edit(&self.config)
        }
    }

    /// Switch the selected profile on or off in the settings file. The
    /// message for the footer, or why not.
    pub fn toggle_enabled(&mut self) -> Result<String, String> {
        let row = self.selected().ok_or("no account selected")?;
        if let Some(problem) = &row.problem {
            return Err(format!("{}: {problem}", row.label()));
        }
        let Some(profile) = &row.profile else {
            return Err(format!(
                "{} is the CLI's own login: add it as a profile to switch it off (kete profile add <name> --harness {} --use-default)",
                row.label(),
                row.harness
            ));
        };
        let name = profile.name.trim().to_string();
        let on = profile.disabled;
        self.on_profiles_layer(|layer| profiles::set_enabled_in_file(layer, &name, on))?;
        if let Some(row) = self.rows.get_mut(self.cursor) {
            if let Some(p) = row.profile.as_mut() {
                p.disabled = !on;
            }
            // The star stays where it is, lit only while the profile is on.
            let flagged = row.default || row.default_off;
            row.default = flagged && on;
            row.default_off = flagged && !on;
        }
        Ok(if on {
            format!("{name} is on")
        } else {
            format!("{name} is off: launches that pick no profile skip it")
        })
    }

    /// Keep the selected profile for the person's own sessions, or give it
    /// back to scripts, in the settings file. The message for the footer,
    /// or why not.
    pub fn toggle_reserved(&mut self) -> Result<String, String> {
        let row = self.selected().ok_or("no account selected")?;
        if let Some(problem) = &row.problem {
            return Err(format!("{}: {problem}", row.label()));
        }
        let Some(profile) = &row.profile else {
            return Err(format!(
                "{} is the CLI's own login: add it as a profile to reserve it (kete profile add <name> --harness {} --use-default)",
                row.label(),
                row.harness
            ));
        };
        let name = profile.name.trim().to_string();
        let on = !profile.reserved;
        self.on_profiles_layer(|layer| profiles::set_reserved_in_file(layer, &name, on))?;
        if let Some(p) = self
            .rows
            .get_mut(self.cursor)
            .and_then(|r| r.profile.as_mut())
        {
            p.reserved = on;
        }
        Ok(if on {
            format!("{name} is reserved: kept for your own sessions")
        } else {
            format!("{name} can be picked by scripts again")
        })
    }

    /// Make the selected profile its harness's default.
    pub fn set_default(&mut self) -> Result<String, String> {
        let row = self.selected().ok_or("no account selected")?;
        if let Some(problem) = &row.problem {
            return Err(format!("{}: {problem}", row.label()));
        }
        let Some(profile) = &row.profile else {
            return Err(format!("{} is not a profile", row.label()));
        };
        let (name, harness) = (profile.name.trim().to_string(), row.harness.clone());
        self.on_profiles_layer(|layer| profiles::set_default_in_file(layer, &name))?;
        for r in &mut self.rows {
            if r.harness == harness && r.profile.is_some() {
                let flagged = r.key == name;
                r.default = flagged && !r.disabled();
                r.default_off = flagged && r.disabled();
            }
        }
        Ok(format!("{name} is now the {harness} default"))
    }
}

/// `ctx 42%` (with `⟳2` after two compactions) for a session card; None
/// when nothing is known.
pub fn context_label(ctx: &ContextUsage) -> Option<String> {
    let p = ctx.percent?;
    let mut out = format!("ctx {}", usage::pct_text(Some(p)));
    if ctx.compactions > 0 {
        out.push_str(&format!(" \u{27f3}{}", ctx.compactions));
    }
    Some(out)
}

/// The QUICK PROMPT's profile field: `work ▰▰▱▱ 21% ▰▱▱▱ 3%`, or the
/// name alone when no reading is known.
pub fn field_text(name: &str, reading: Option<&AccountUsage>, now: i64) -> String {
    let Some(u) = reading else {
        return name.to_string();
    };
    let mut out = name.to_string();
    for w in u.bars() {
        let p = w.and_then(|w| w.shown_at(now));
        out.push(' ');
        out.push_str(&usage::block_bar(p, 4));
        out.push(' ');
        out.push_str(&usage::pct_text(p));
    }
    out
}

/// The worst tone of a reading's two bars.
pub fn reading_tone(reading: Option<&AccountUsage>, now: i64) -> usage::Tone {
    reading
        .and_then(|u| u.worst_at(now))
        .map_or(usage::Tone::Ok, usage::tone)
}

/// `5m ago`, `2h ago` — how old a reading is.
pub fn age(at_ms: i64, now_ms: i64) -> String {
    if at_ms <= 0 {
        return "time unknown".into();
    }
    let secs = ((now_ms - at_ms) / 1000).max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::usage::{UsageSource, UsageWindow, WindowKind};
    use std::collections::BTreeMap;

    fn profile(name: &str, default: bool) -> Profile {
        Profile {
            name: name.into(),
            harness: "claude".into(),
            env: BTreeMap::new(),
            default,
            use_default: false,
            disabled: false,
            reserved: false,
        }
    }

    fn reading(key: &str, s: f32, w: f32) -> AccountUsage {
        AccountUsage {
            account: key.into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent: s,
                    resets_at: None,
                    read_at: 1,
                    estimated: false,
                },
                UsageWindow {
                    kind: WindowKind::Weekly,
                    label: None,
                    percent: w,
                    resets_at: None,
                    read_at: 1,
                    estimated: false,
                },
            ],
            source: UsageSource::Statusline,
            at: 0,
            plan: None,
        }
    }

    fn check(state: LoginState) -> LoginCheck {
        LoginCheck {
            state,
            expired: false,
        }
    }

    fn signed_in(_: Option<&Profile>, _: &str) -> LoginCheck {
        check(LoginState::LoggedIn)
    }

    fn session(name: &str, profile: Option<&str>, archived: bool) -> nebula_core::Agent {
        nebula_core::Agent {
            parent: None,
            profile: profile.map(str::to_string),
            id: nebula_core::AgentId(name.into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: name.into(),
            status: nebula_core::AgentStatus::Running,
            archived,
            archived_at: 0,
            unseen: false,
            kind: nebula_core::AgentKind::Claude,
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
        }
    }

    #[test]
    fn rows_are_profiles_then_own_logins_with_readings() {
        let all = vec![profile("work", true), profile("home", false)];
        let usage = vec![reading("work", 10.0, 20.0), reading("@claude", 5.0, 1.0)];
        let rows = build_rows(&all, &usage, &[], |p, _| match p {
            Some(p) if p.name == "home" => check(LoginState::NeedsLogin),
            Some(p) if p.name == "work" => LoginCheck {
                state: LoginState::NeedsLogin,
                expired: true,
            },
            _ => check(LoginState::LoggedIn),
        });
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["work", "home", "@claude"]);
        assert!(rows[0].default && !rows[1].default);
        assert!(rows[0].usage.is_some() && rows[1].usage.is_none());
        assert_eq!(rows[2].label(), "claude (own login)");
        assert_eq!(rows[1].login_text(), "needs login: kete profile login home");
        assert_eq!(
            rows[0].login_text(),
            "login expired: kete profile login work"
        );
        assert_eq!(rows[2].login_text(), "signed in");
        // Jay's case: no profiles, one own-login reading.
        let rows = build_rows(&[], &[reading("@claude", 1.0, 1.0)], &[], signed_in);
        assert_eq!(rows.len(), 1);
    }

    /// How each account is used: its live sessions, counted and named; an
    /// own login with a session on it and no reading still gets a row.
    #[test]
    fn rows_count_the_live_sessions_on_each_account() {
        let all = vec![profile("work", true), profile("home", false)];
        let agents = vec![
            session("fix-auth", Some("work"), false),
            session("docs", Some("work"), false),
            session("old", Some("work"), true),
            session("bare", None, false),
        ];
        let rows = build_rows(&all, &[], &agents, signed_in);
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["work", "home", "@claude"]);
        assert_eq!(rows[0].sessions, ["fix-auth", "docs"]);
        assert_eq!(rows[0].sessions_text(), "2 sessions: fix-auth, docs");
        assert_eq!(rows[1].sessions_text(), "");
        assert_eq!(rows[2].sessions_text(), "1 session: bare");
    }

    /// A profile the settings got wrong is listed with its problem, and
    /// neither switch nor default touches it; a default switched off keeps
    /// a dim star.
    #[test]
    fn a_broken_profile_is_listed_with_its_problem() {
        let mut broken = profile("bad", false);
        broken.harness = String::new();
        let mut off = profile("off", true);
        off.disabled = true;
        let all = vec![broken, off];
        let rows = build_rows(&all, &[], &[], signed_in);
        assert_eq!(rows.len(), 2, "nothing filtered out");
        assert!(rows[0].problem.is_some(), "{:?}", rows[0]);
        assert_eq!(rows[0].login_text(), "");
        assert!(!rows[1].default && rows[1].default_off);
        let dir = tempfile::tempdir().unwrap();
        let mut view = AccountsView::new(
            rows,
            dir.path().join("config.json"),
            dir.path().join("config.local.json"),
        );
        assert!(view.toggle_enabled().is_err());
        assert!(view.set_default().is_err());
    }

    /// A config.local.json holding `profiles` is the list in force: the
    /// switch and the default land there, config.json is left alone.
    #[test]
    fn edits_go_to_the_local_layer_when_it_holds_the_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &path,
            r#"{"profiles":[{"name":"base","harness":"claude"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"{"profiles":[{"name":"work","harness":"claude"},{"name":"home","harness":"claude"}]}"#,
        )
        .unwrap();
        let all = vec![profile("work", false), profile("home", false)];
        let mut view = AccountsView::new(
            build_rows(&all, &[], &[], signed_in),
            path.clone(),
            local.clone(),
        );
        view.toggle_enabled().unwrap();
        view.step(1);
        view.set_default().unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        assert_eq!(saved["profiles"][0]["disabled"], true);
        assert_eq!(saved["profiles"][1]["default"], true);
        let base = std::fs::read_to_string(&path).unwrap();
        assert!(
            !base.contains("disabled") && !base.contains("default"),
            "{base}"
        );
        assert!(
            !dir.path().join("config.json.lock").exists(),
            "lock released"
        );
        std::fs::write(&local, "{ not json").unwrap();
        assert!(view.toggle_enabled().is_err());
    }

    #[test]
    fn toggle_and_default_write_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme":"x","profiles":[{"name":"work","harness":"claude","default":true},{"name":"home","harness":"claude"}]}"#,
        )
        .unwrap();
        let all = vec![profile("work", true), profile("home", false)];
        let mut view = AccountsView::new(
            build_rows(&all, &[reading("@claude", 0.0, 0.0)], &[], signed_in),
            path.clone(),
            dir.path().join("config.local.json"),
        );
        view.step(1);
        assert!(view.toggle_enabled().unwrap().contains("off"));
        assert!(view.rows[1].disabled());
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["profiles"][1]["disabled"], true);
        assert_eq!(saved["theme"], "x", "other keys kept");
        assert!(view.toggle_enabled().unwrap().contains("on"));
        view.set_default().unwrap();
        assert!(view.rows[1].default && !view.rows[0].default);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["profiles"][1]["default"], true);
        assert!(saved["profiles"][0].get("default").is_none());
        assert!(saved["profiles"][1].get("disabled").is_none());
        // The own-login row is not a profile.
        view.step(5);
        assert_eq!(view.cursor, 2, "clamped");
        assert!(view.toggle_enabled().is_err());
        assert!(view.set_default().is_err());
    }

    /// `y` keeps a profile for the person's own sessions: reserved lands in
    /// the file, and off again leaves no key; own logins and broken
    /// profiles are refused.
    #[test]
    fn toggle_reserved_writes_the_settings_file_and_reads_back_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme":"x","profiles":[{"name":"work","harness":"claude"},{"name":"home","harness":"claude"}]}"#,
        )
        .unwrap();
        let mut broken = profile("bad", false);
        broken.harness = String::new();
        // The broken profile's harness is empty: its group sorts first.
        let all = vec![broken, profile("work", false), profile("home", false)];
        let mut view = AccountsView::new(
            build_rows(&all, &[reading("@claude", 0.0, 0.0)], &[], signed_in),
            path.clone(),
            dir.path().join("config.local.json"),
        );
        let saved = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        };
        assert!(view.toggle_reserved().is_err(), "a broken profile");
        view.step(2);
        let said = view.toggle_reserved().unwrap();
        assert_eq!(said, "home is reserved: kept for your own sessions");
        assert!(view.rows[2].reserved() && !view.rows[1].reserved());
        assert_eq!(saved()["profiles"][1]["reserved"], true);
        assert_eq!(saved()["theme"], "x", "other keys kept");
        assert_eq!(
            view.toggle_reserved().unwrap(),
            "home can be picked by scripts again"
        );
        assert!(!view.rows[2].reserved());
        assert!(saved()["profiles"][1].get("reserved").is_none());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("reserved"));
        // The own-login row is refused.
        view.step(5);
        assert!(view.rows[view.cursor].profile.is_none());
        assert!(!view.rows[view.cursor].reserved());
        assert!(view.toggle_reserved().is_err());
    }

    #[test]
    fn field_and_card_labels() {
        assert_eq!(field_text("work", None, 0), "work");
        assert_eq!(
            field_text("work", Some(&reading("work", 50.0, 10.0)), 0),
            "work \u{25b0}\u{25b0}\u{25b1}\u{25b1} 50% \u{25b0}\u{25b1}\u{25b1}\u{25b1} 10%"
        );
        assert_eq!(
            reading_tone(Some(&reading("w", 86.0, 0.0)), 0),
            usage::Tone::Warn
        );
        assert_eq!(reading_tone(None, 0), usage::Tone::Ok);
        let ctx = ContextUsage {
            percent: Some(42.4),
            tokens: None,
            window: None,
            compactions: 2,
            last_compact_at: None,
            source: UsageSource::Statusline,
            at: 0,
        };
        assert_eq!(context_label(&ctx).as_deref(), Some("ctx 42% \u{27f3}2"));
        assert_eq!(
            context_label(&ContextUsage {
                percent: None,
                ..ctx
            }),
            None
        );
        assert_eq!(age(1, 30_000), "just now");
        assert_eq!(age(1, 3 * 3_600_000 + 1), "3h ago");
        assert_eq!(age(0, 5), "time unknown");
    }
}
