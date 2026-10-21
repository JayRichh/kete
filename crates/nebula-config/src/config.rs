//! User settings — the TUI's, and the desktop window's view of them — read from the same two layers the daemon reads —
//! `paths::config_path()` with `paths::config_local_path()` over it, see
//! `nebula_core::settings` (each side deserializes only its own fields;
//! serde ignores the rest). Loaded fresh at each use so edits apply without
//! restarting the TUI. A missing file or unknown fields fall back to
//! defaults; a value this build can't read costs only its own key, which is
//! logged and left as stored.
//!
//! The settings overlay is the writer: it patches known keys and leaves
//! any other JSON fields (including future daemon keys) untouched, and a
//! key `config.local.json` holds is written back there, never into the
//! portable file.

use crate::choices::PresetText;
use nebula_core::harness::{CustomHarness, HarnessDescriptor};
use nebula_core::AgentKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Values the settings overlay cycles through for `session_idle_timeout`
/// (daemon-owned: how long unwatched idle sessions live before their PTY
/// is reaped).
pub const SESSION_IDLE_TIMEOUTS: &[&str] = &["off", "1m", "5m", "15m", "30m", "1h"];

/// Editor commands the settings overlay cycles through. Every entry
/// accepts `+<line> <file>`, which is how the overlays launch it. As with
/// models, hand-edited configs can name any command the list doesn't.
pub const EDITORS: &[&str] = &["vim", "nvim", "nano", "emacs", "hx"];

/// The **Session pane** choices (Settings → Appearance), in the order the
/// row cycles them: the TUI `launcher::PaneSide` sides by name, the
/// default first.
pub use crate::choices::PANE_SIDES;

/// The **Worktree layout** choices (Settings → Appearance), in the order
/// the row cycles them: the GRID's row of cards per worktree, the default,
/// then the compact LIST — each worktree's sessions stacked one line apiece,
/// the most recent few shown until Tab opens the rest
/// (TUI `launcher::LIST_RECENT`) — then the NESTED layout: each
/// worktree a header, and under it, along a rail, a full-width card per
/// session and terminal, every worktree open until Tab folds it — then
/// COLUMNS: projects, worktrees and sessions beside the pane.
pub const WORKTREE_LAYOUTS: &[&str] = &["cards", "list", "nested", "columns"];

/// What **Kanban scope** cycles through.
pub const KANBAN_SCOPES: &[&str] = &["project", "all"];
/// ORCHESTRATOR VIEW at startup: `auto` (only in a narrow terminal),
/// `always` or `never`. Anything else reads as `auto`.
pub const ORCHESTRATOR_VIEWS: &[&str] = &["auto", "always", "never"];
/// Narrower than this, `auto` opens on the orchestrator: too narrow for the
/// grid and a pane side by side, so the one chat is the useful screen.
pub const ORCHESTRATOR_NARROW_COLS: u16 = 100;

fn is_auto(word: &str) -> bool {
    word.trim().is_empty() || word.trim().eq_ignore_ascii_case(ORCHESTRATOR_VIEWS[0])
}

/// What **Diff context** cycles through: the three lines git gives each
/// side of a change, or the whole file.
pub const DIFF_CONTEXTS: &[&str] = &["collapsed", "full"];

/// What **Critter size** cycles through, the default in the middle.
pub const CRITTER_SIZES: &[&str] = &["small", "medium", "large"];
/// What **Critters visit** cycles through: how often a visitor turns up.
pub const CRITTER_VISITS: &[&str] = &["rare", "normal", "busy"];
/// What **Keep finished subagents** cycles through (the daemon reads the
/// same key): how long an ended subagent stays listed.
pub use nebula_core::settings::{
    DEFAULT_STUDIO_IDLE_AFTER, DEFAULT_SUBAGENT_RETENTION, STUDIO_IDLE_AFTERS, SUBAGENT_RETENTIONS,
};

/// Inline graphics choices (Settings → Appearance).
pub const INLINE_GRAPHICS: &[&str] = crate::choices::INLINE_GRAPHICS_MODES;

/// The **Preset text** choices (Settings → Sessions), in the order the row
/// cycles them: the [`PresetText`] sides by label.
pub const PRESET_TEXTS: &[&str] = &[
    PresetText::Prefix.as_str(),
    PresetText::Postfix.as_str(),
    PresetText::Both.as_str(),
];

/// Values the settings overlay cycles through for `done_sound` (what rings
/// when a turn reaches FINISHED) and `feedback_sound` (what rings when one
/// stops at NEEDS FEEDBACK). `off` is silence, `bell` the terminal BEL
/// (the one sound that reaches the local terminal over `nebula ssh` — but
/// silent in Ghostty out of the box, whose `bell-features` default to
/// `no-audio`), the rest are macOS system sounds in `/System/Library/Sounds`,
/// played with `afplay`; see [`Config::done_sound`] for where a name falls
/// back to the bell. Hand-edited configs can name any sound in that folder.
pub const SOUNDS: &[&str] = &[
    "off",
    "bell",
    "Glass",
    "Ping",
    "Pop",
    "Hero",
    "Purr",
    "Tink",
    "Submarine",
    "Funk",
    "Blow",
    "Bottle",
    "Frog",
    "Morse",
    "Sosumi",
    "Basso",
];

/// Where the macOS system sounds live; `<name>.aiff` inside it.
pub const MACOS_SOUNDS_DIR: &str = "/System/Library/Sounds";

pub use nebula_core::harness::DEFAULT_CHOICE;

/// What the overlay shows for an empty `worktree_base_branch`: the daemon
/// picks origin's default branch itself. Display only — the file holds
/// `""`, never this word.
pub const AUTO_CHOICE: &str = "auto";
/// What the Project tab's **Run command** row shows while it is empty:
/// the checkout's `.nebula.json` is what `r` reads then.
pub const PROJECT_FILE_CHOICE: &str = nebula_core::project_file::FILE_NAME;

// The static model/effort lists live in the core registry table now
// ([`nebula_core::harness::builtin`]); what the pickers show is built
// below from the effective descriptor, so a `harnesses` override renames
// the rows everywhere at once. Claude's models still come from
// `claude_catalogue.rs` at runtime — CONFIG.JSON's `claude_models`, else
// Claude Code's own `availableModels`, else the aliases — and Cursor's
// from its catalogue (a seed plus a cached `cursor-agent --list-models`).

/// The `quick_prompt_kind` choices: every built-in harness id, by the name
/// the config file stores. Derived from the registry table rather than
/// spelled out, so a new built-in joins the cycle without a second edit.
pub fn agent_kind_names() -> Vec<String> {
    nebula_core::harness::builtins()
        .iter()
        .map(|descriptor| descriptor.id.clone())
        .collect()
}

/// The model rows for a harness: [`DEFAULT_CHOICE`] first ("don't pass the
/// flag — let the CLI pick", what the daemon sees as None), then the
/// runtime catalogue (Claude/Cursor, already headed) or the descriptor's
/// static list. Pi's `--model` takes a fuzzy pattern across providers, so
/// its list is families, not ids; a hand-edited `provider/id` passes
/// through verbatim.
pub fn model_choices(kind: AgentKind, custom: Option<&str>) -> Vec<String> {
    model_choices_in(&describe(kind, custom))
}

/// [`model_choices`] against an explicit descriptor, for callers that
/// already resolved one (the Agents tab, the launch sites).
pub fn model_choices_in(descriptor: &nebula_core::harness::HarnessDescriptor) -> Vec<String> {
    match descriptor.model.catalog {
        Some(nebula_core::harness::HarnessCatalog::Claude) => crate::claude_catalogue::models()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        Some(nebula_core::harness::HarnessCatalog::Cursor) => crate::cursor_catalogue::models()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        None => headed(
            descriptor
                .model
                .models
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
        ),
    }
}

/// Effort rows for a harness given its chosen model (None or "default" =
/// the CLI's pick). Empty — no Effort row, no effort submenu — while the
/// harness offers no effort. Cursor's list follows the family (`-fast`
/// variants ride in the effort, `high-fast`); any other harness takes its
/// static list with any model.
pub fn effort_choices(kind: AgentKind, model: Option<&str>, custom: Option<&str>) -> Vec<String> {
    effort_choices_in(&describe(kind, custom), model)
}

/// [`effort_choices`] against an explicit descriptor, for callers that
/// already resolved one (the Agents tab, the launch sites).
pub fn effort_choices_in(
    descriptor: &nebula_core::harness::HarnessDescriptor,
    model: Option<&str>,
) -> Vec<String> {
    if !descriptor.effort.offered {
        return Vec::new();
    }
    if descriptor.model.catalog == Some(nebula_core::harness::HarnessCatalog::Cursor) {
        return crate::cursor_catalogue::efforts(model)
            .iter()
            .map(|s| s.to_string())
            .collect();
    }
    headed(descriptor.effort.efforts.clone())
}

/// [`DEFAULT_CHOICE`] heading a choice list, without doubling a default
/// the source already carries.
fn headed(mut rest: Vec<String>) -> Vec<String> {
    rest.retain(|choice| !choice.eq_ignore_ascii_case(DEFAULT_CHOICE));
    let mut out = vec![DEFAULT_CHOICE.to_string()];
    out.append(&mut rest);
    out
}

/// Whether `value` is one of `choices`, case-insensitively and trimmed —
/// how a form decides a saved or cycled choice still has a row.
pub fn fits(value: &str, choices: &[impl AsRef<str>]) -> bool {
    choices
        .iter()
        .any(|c| c.as_ref().eq_ignore_ascii_case(value.trim()))
}

/// Step `current` through an owned choice list, wrapping around; a value
/// off the list steps onto it. The owned twin of [`cycle_choice`], for
/// rows the registry builds at runtime.
pub fn cycle_owned(current: &str, choices: &[String], delta: i32) -> String {
    if choices.is_empty() {
        return current.to_string();
    }
    choices[cycled_index(current, choices, delta)].clone()
}

/// Where `current` lands in `choices` after `delta` steps, wrapping; a
/// value off the list (matched ignoring case) counts as the first.
/// Panics on an empty list.
fn cycled_index<S: AsRef<str>>(current: &str, choices: &[S], delta: i32) -> usize {
    let n = choices.len() as i32;
    let pos = choices
        .iter()
        .position(|c| c.as_ref().eq_ignore_ascii_case(current.trim()))
        .unwrap_or(0) as i32;
    (pos + delta).rem_euclid(n) as usize
}

/// The effort to launch with, given the harness, its model and the picked
/// effort. Most harnesses pass through. A composing harness (Cursor's
/// family-suffix shape: `--model <family>-<effort>`) fits instead: no
/// family → None; an effort the family ships → itself; anything else
/// ("default", unset, a suffix the family lacks) → None when the bare
/// family id exists, otherwise the family's fallback — most families have
/// no bare id, and a bare `--model claude-fable-5` is refused at spawn.
pub fn fit_effort(
    kind: AgentKind,
    model: Option<&str>,
    effort: Option<String>,
    custom: Option<&str>,
) -> Option<String> {
    fit_effort_in(&describe(kind, custom), model, effort)
}

/// [`fit_effort`] against an explicit descriptor, for callers that
/// already resolved one (the Agents tab, the launch sites).
pub fn fit_effort_in(
    descriptor: &nebula_core::harness::HarnessDescriptor,
    model: Option<&str>,
    effort: Option<String>,
) -> Option<String> {
    if !descriptor.compose_model_effort {
        return effort;
    }
    let family = model
        .map(str::trim)
        .filter(|m| !m.eq_ignore_ascii_case(DEFAULT_CHOICE))?;
    if descriptor.model.catalog == Some(nebula_core::harness::HarnessCatalog::Cursor) {
        let choices = crate::cursor_catalogue::efforts(Some(family));
        if choices.is_empty() {
            return None;
        }
        let picked = effort
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| e != DEFAULT_CHOICE);
        return match picked {
            Some(e) if fits(&e, choices) => Some(e),
            _ if choices[0] == DEFAULT_CHOICE => None,
            _ => crate::cursor_catalogue::fallback_effort(family).map(String::from),
        };
    }
    if descriptor.effort.efforts.is_empty() {
        return None;
    }
    let picked = effort
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e != DEFAULT_CHOICE);
    match picked {
        Some(e) if fits(&e, &descriptor.effort.efforts) => Some(e),
        _ => static_fallback_effort(&descriptor.effort.efforts).map(String::from),
    }
}

/// The fallback effort for a static list: `high`, else `medium`, else the
/// first the harness ships.
fn static_fallback_effort(efforts: &[String]) -> Option<&str> {
    efforts
        .iter()
        .find(|e| e.as_str() == "high")
        .or_else(|| efforts.iter().find(|e| e.as_str() == "medium"))
        .or_else(|| efforts.first())
        .map(String::as_str)
}

/// The effective descriptor `(kind, custom)` reads as: the registry row
/// with the `harnesses` map, the legacy list entry, and the legacy
/// per-harness keys folded in. Loads the config fresh, like every other
/// reader here. A broken entry still resolves (the picker, not the read,
/// hides it); launches refuse it with its reason. An id the registry no
/// longer names degrades to a placeholder under its own name, so rows
/// outliving their entry still render.
fn describe(kind: AgentKind, custom: Option<&str>) -> nebula_core::harness::HarnessDescriptor {
    let id = match kind {
        AgentKind::Custom => custom.unwrap_or_default().trim(),
        _ => kind.as_str(),
    };
    let cfg = Config::load();
    if let Some(descriptor) = cfg
        .harness_registry()
        .into_iter()
        .find(|entry| entry.id == id)
    {
        return descriptor;
    }
    nebula_core::harness::CustomHarness {
        id: id.to_string(),
        label: String::new(),
        program: id.to_string(),
        enabled: true,
        model: DEFAULT_CHOICE.into(),
        model_flag: "--model".into(),
        hooks: None,
    }
    .as_descriptor()
}

/// One setting row in the overlay; rows live inside a [`SettingsTab`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    pub kind: SettingKind,
    pub label: &'static str,
    pub hint: &'static str,
    /// Section header the row sits under, as `keymap::ActionSpec::group`
    /// is for the Hotkeys tab. Empty means the tab lists the row bare;
    /// a tab whose rows all say so stays a flat list.
    pub group: &'static str,
}

/// What a tab shows. Ordinary tabs are a list of value settings. The
/// Project tab is a list too, but its rows are one project's — the one
/// selected in the PROJECTS PANEL, named on the tab's first line — and
/// read and write that project's entry instead of a top-level key. The
/// Hotkeys tab is generated from [`crate::actions::ACTIONS`] instead, so a
/// new action shows up there without being declared twice — and the Agents
/// tab is generated from the harness registry, so a new CLI shows up
/// there without being declared twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBody {
    Values(&'static [SettingSpec]),
    Project(&'static [SettingSpec]),
    Hotkeys,
    Agents,
}

/// The Agents tab's static head: the cross-harness quick-prompt rows. The
/// per-harness sections below them are generated from the registry (see
/// [`Config::agent_rows`]).
pub const AGENTS_HEAD: &[SettingSpec] = &[
    SettingSpec {
        kind: SettingKind::QuickPromptKind,
        label: "Agent",
        hint: "Harness the quick prompt hotkey launches, with that kind's model/effort",
        group: "Quick prompt",
    },
    SettingSpec {
        kind: SettingKind::QuickPromptFocus,
        label: "Focus",
        hint: "Enter the new session's terminal on launch (off = just select its row)",
        group: "Quick prompt",
    },
    SettingSpec {
        kind: SettingKind::FollowNewSession,
        label: "Follow new",
        hint: "Move the cursor onto the new session's card, the grid scrolled to it, without entering it (off = stay on the card you're on)",
        group: "Quick prompt",
    },
    SettingSpec {
        kind: SettingKind::QuickPromptNewWorktree,
        label: "New worktree",
        hint: "Each new session's box starts on a fresh worktree (off = the project's root branch; ^N flips one box)",
        group: "Quick prompt",
    },
    SettingSpec {
        kind: SettingKind::HideUninstalledHarnesses,
        label: "Hide missing CLIs",
        hint: "List only harnesses found on PATH in the New session picker (daemon still checks at launch)",
        group: "Quick prompt",
    },
];

/// One tab of the settings overlay. Selection indices are per-tab: within
/// a `Values` tab they index its settings, within `Hotkeys` they index
/// `keymap::ACTIONS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsTab {
    pub title: &'static str,
    pub body: TabBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    PaletteEnterAttaches,
    WorktreeBaseBranch,
    Editor,
    CloseFinderOnOpen,
    SshSyncConfig,
    SessionIdleTimeout,
    PrewarmAgents,
    PrewarmSessions,
    DoneSound,
    FeedbackSound,
    PresetText,
    DeleteEmptyWorktree,
    AutoCleanupMerged,
    ShowAllWorktrees,
    Theme,
    Icons,
    Animations,
    BlackBackground,
    HideCardMarks,
    HighlightCurrentCard,
    SourceControlGraph,
    DiffMode,
    DiffContext,
    SessionPane,
    WorktreeLayout,
    Kanban,
    KanbanScope,
    OrchestratorView,
    InlineGraphics,
    ExpandAllWorktrees,
    CardIssueNumber,
    HideDraftPrs,
    QuickPromptKind,
    QuickPromptFocus,
    FollowNewSession,
    QuickPromptNewWorktree,
    RunCommand,
    OpenCommand,
    RememberHarness,
    HideUninstalledHarnesses,
    AskBeforeArchive,
    ConfirmDragMove,
    AccountSpread,
    AccountSpreadThreshold,
    UsageStatusline,
    ShowTips,
    Critters,
    CrittersSize,
    CrittersOften,
    HideDoneSubagents,
    SubagentRetention,
    StudioIdleAfter,
    UsageLedger,
}

/// One harness field row in the Agents tab. The tab renders one section
/// per registry entry — built-ins, legacy customs and `harnesses` map ids
/// alike — with an Enabled row, a Model row, and an Effort row while the
/// harness offers effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessField {
    Enabled,
    Model,
    Effort,
}

impl HarnessField {
    pub fn label(self) -> &'static str {
        match self {
            HarnessField::Enabled => "Enabled",
            HarnessField::Model => "Model",
            HarnessField::Effort => "Effort",
        }
    }
}

impl SettingKind {
    /// A row whose value is typed, not toggled or cycled: Enter on it opens
    /// a one-line prompt pre-filled with the current value, and ←/→ have
    /// nothing to step through. [`Config::cycle`] leaves such a row alone;
    /// [`Config::set_text`] is what writes it.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            SettingKind::WorktreeBaseBranch | SettingKind::RunCommand | SettingKind::OpenCommand
        )
    }

    /// A row on the PROJECT TAB: its value is the focused project's, kept
    /// in that project's `projects` entry rather than at the top level of
    /// the file. Every one of them is typed ([`SettingKind::is_text`]):
    /// [`Config::cycle`] leaves such a row alone,
    /// [`Config::set_project_text`] is what writes it, and
    /// [`ProjectSettings::value_label`] what reads it.
    pub fn is_project(self) -> bool {
        matches!(self, SettingKind::RunCommand | SettingKind::OpenCommand)
    }

    /// The day the row first shipped, `(year, month, day)`: the date of
    /// the first release tag whose settings overlay lists it, or the day
    /// it was written for a row no release carries yet. The overlay marks
    /// a row `(new)` for [`NEW_SETTING_DAYS`] from here. The match is
    /// exhaustive on purpose — a new row can't compile without its date.
    pub fn added_on(self) -> (i32, u32, u32) {
        match self {
            // v0.1.0
            SettingKind::PaletteEnterAttaches
            | SettingKind::Editor
            | SettingKind::SessionIdleTimeout
            | SettingKind::Theme
            | SettingKind::Animations => (2026, 8, 22),
            // v0.16.0
            SettingKind::DoneSound => (2026, 8, 28),
            // v0.19.0 – v0.21.0
            SettingKind::CloseFinderOnOpen
            | SettingKind::QuickPromptKind
            | SettingKind::QuickPromptFocus => (2026, 8, 29),
            // v0.23.0 / v0.24.0
            SettingKind::FeedbackSound
            | SettingKind::WorktreeBaseBranch
            | SettingKind::PrewarmAgents
            | SettingKind::PrewarmSessions => (2026, 9, 9),
            // v0.27.0 / v0.28.0
            SettingKind::SshSyncConfig
            | SettingKind::HideDraftPrs
            | SettingKind::HideUninstalledHarnesses => (2026, 9, 15),
            // v0.29.0 / v0.31.0
            SettingKind::RunCommand | SettingKind::OpenCommand | SettingKind::RememberHarness => {
                (2026, 9, 17)
            }
            // v0.33.0
            SettingKind::PresetText => (2026, 9, 18),
            // v0.34.0
            SettingKind::BlackBackground
            | SettingKind::SessionPane
            | SettingKind::QuickPromptNewWorktree => (2026, 9, 22),
            // v0.38.0, then rows not yet in a release
            SettingKind::DeleteEmptyWorktree
            | SettingKind::AutoCleanupMerged
            | SettingKind::ShowAllWorktrees
            | SettingKind::HideCardMarks
            | SettingKind::WorktreeLayout
            | SettingKind::CardIssueNumber => (2026, 9, 24),
            SettingKind::ExpandAllWorktrees | SettingKind::FollowNewSession => (2026, 9, 26),
            SettingKind::HighlightCurrentCard => (2026, 9, 28),
            SettingKind::AskBeforeArchive => (2026, 10, 3),
            SettingKind::AccountSpread
            | SettingKind::AccountSpreadThreshold
            | SettingKind::UsageStatusline => (2026, 10, 14),
            SettingKind::ConfirmDragMove => (2026, 10, 2),
            SettingKind::SourceControlGraph | SettingKind::InlineGraphics => (2026, 10, 9),
            SettingKind::DiffMode
            | SettingKind::DiffContext
            | SettingKind::Kanban
            | SettingKind::KanbanScope
            | SettingKind::Icons => (2026, 10, 13),
            SettingKind::OrchestratorView => (2026, 10, 14),
            SettingKind::ShowTips => (2026, 10, 15),
            SettingKind::Critters | SettingKind::CrittersSize | SettingKind::CrittersOften => {
                (2026, 10, 16)
            }
            SettingKind::HideDoneSubagents | SettingKind::SubagentRetention => (2026, 10, 18),
            SettingKind::StudioIdleAfter | SettingKind::UsageLedger => (2026, 10, 21),
        }
    }

    /// The row shipped fewer than [`NEW_SETTING_DAYS`] days before
    /// `today` (days since the Unix epoch, see [`today_days`]).
    pub fn is_new(self, today: i64) -> bool {
        let (y, m, d) = self.added_on();
        (0..NEW_SETTING_DAYS).contains(&(today - days_from_civil(y, m, d)))
    }
}

/// How many days a settings row wears its `(new)` prefix.
pub const NEW_SETTING_DAYS: i64 = 7;

/// What a newly shipped settings row is prefixed with.
pub const NEW_SETTING_PREFIX: &str = "(new) ";

/// Today as days since the Unix epoch, in UTC.
pub fn today_days() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

/// Days since the Unix epoch of a proleptic Gregorian date (Howard
/// Hinnant's `days_from_civil`).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The tab strip, left to right. Ordered by how often a setting gets
/// touched, with Hotkeys last because it is the biggest and the least
/// casual.
pub const SETTINGS_TABS: &[SettingsTab] = &[
    SettingsTab {
        title: "General",
        body: TabBody::Values(&[
            SettingSpec {
                kind: SettingKind::PaletteEnterAttaches,
                label: "Search Enter attaches",
                hint: "Enter in / search opens the session in the terminal (a red one always does)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::WorktreeBaseBranch,
                label: "Worktree base branch",
                hint: "Branch new worktrees start from; Enter types one (empty = origin's default)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::Editor,
                label: "File editor",
                hint: "Editor f/b/F and ⌥click launch (NEBULA_EDITOR overrides)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::CloseFinderOnOpen,
                label: "Finder closes on open",
                hint: "Opening a file closes f/F, so quitting the editor is one Esc",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::SshSyncConfig,
                label: "Sync settings over ssh",
                hint: "kete ssh / tunnel carry config.json and presets to the remote (config.local.json stays)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::OrchestratorView,
                label: "Open on the orchestrator",
                hint: "Start on the orchestrator's view (F10) when there is one: auto = only in a narrow terminal, always, or never",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::ShowTips,
                label: "Show tips",
                hint: "A one-line tip the first time you meet something (the ? menu, the scope chip, the board's lanes, drag, the review), each once; off hides them all",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::StudioIdleAfter,
                label: "Idle note after",
                hint: "How long the orchestrator sits idle before kete hands it a note naming new studio inbox notes (only for repos with studio mode on)",
                group: "Studio",
            },
        ]),
    },
    SettingsTab {
        title: "Sessions",
        body: TabBody::Values(&[
            SettingSpec {
                kind: SettingKind::SessionIdleTimeout,
                label: "Idle session timeout",
                hint: "Kill idle sessions in unviewed worktrees (busy ones spared; off disables)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::PrewarmAgents,
                label: "Warm spare agent",
                hint: "Boot a spare CLI in the selected worktree for instant creates (a peer in /list-agents)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::PrewarmSessions,
                label: "Prewarm dead sessions",
                hint: "Boot a worktree's dead sessions while the cursor rests on it, so attaching is instant",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::DoneSound,
                label: "Done sound",
                hint: "Ding when a turn finishes: off, the terminal bell, or a macOS system sound",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::FeedbackSound,
                label: "Feedback sound",
                hint: "Ring, and notify an unfocused window, when a turn stops to ask you (off silences both)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::PresetText,
                label: "Preset text",
                hint: "Where a new agent preset's text goes: a prefix before the task, a postfix after it, or both (its Text row can change one)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::DeleteEmptyWorktree,
                label: "Delete emptied worktree",
                hint: "Deleting a worktree's last session or terminal deletes the worktree with it, no question asked (off = that delete's confirm asks first)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::AutoCleanupMerged,
                label: "Auto-clean merged worktrees",
                hint: "When a linked worktree's PR is safely detected as merged, archive idle sessions and remove the worktree (root/default/base branches are never touched)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::ShowAllWorktrees,
                label: "Show all worktrees",
                hint: "Every worktree gets a band on the grid and a row in the window's overview, even an empty one (d deletes it); deleting a last card keeps the worktree unless Delete emptied worktree is on",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::AskBeforeArchive,
                label: "Confirm on archive",
                hint: "a and the card menu's Archive ask before archiving a session (off archives at once; u brings it back)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::ConfirmDragMove,
                label: "Confirm drag move",
                hint: "A session card dropped on another worktree or project tab asks before it moves (off = the drop moves it at once; m's picker never asks)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::HideDoneSubagents,
                label: "Finished subagents",
                hint: "Fold a session's done and closed subagents into a count under it on the board, the cards and the session tree (click the count or press d in the tree to show them); running ones always show",
                group: "Subagents",
            },
            SettingSpec {
                kind: SettingKind::SubagentRetention,
                label: "Keep finished subagents",
                hint: "How long a done or closed subagent stays listed before kete drops it (off = until you clear them); restarting a session clears its old ones at once",
                group: "Subagents",
            },
            SettingSpec {
                kind: SettingKind::AccountSpread,
                label: "Account spread",
                hint: "Which profile a new session takes when none is picked: manual = the harness's default; failover = the default until its 5h or weekly use reaches the threshold, then the next enabled one; least_used = the enabled one with the least use",
                group: "Accounts",
            },
            SettingSpec {
                kind: SettingKind::AccountSpreadThreshold,
                label: "Failover at",
                hint: "Percent of an account's 5h or weekly limit at which failover moves to the next enabled profile",
                group: "Accounts",
            },
            SettingSpec {
                kind: SettingKind::UsageStatusline,
                label: "Usage status line",
                hint: "Claude sessions get kete's status line, which reads plan usage and context for the accounts list (your own statusLine still runs and prints; off = only the on-screen usage notice)",
                group: "Accounts",
            },
            SettingSpec {
                kind: SettingKind::UsageLedger,
                label: "Usage ledger",
                hint: "Record each session's tokens, estimated cost, tool uses and turns per day for kete usage (off keeps what is recorded and adds nothing)",
                group: "Usage",
            },
        ]),
    },
    SettingsTab {
        title: "Appearance",
        body: TabBody::Values(&[
            SettingSpec {
                kind: SettingKind::Theme,
                label: "Color theme",
                hint: "Colors across the grid, overlays, previews and diffs. one-dark / one-light paint their own canvas; auto follows the OS light or dark; vscode reads the theme VS Code is set to; Enter picks any installed VS Code theme (vscode:<name>)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::Icons,
                label: "File icons",
                hint: "Per-file-type icons where files are listed: nerd (Nerd Font glyphs), ascii (a two-letter tag), off; auto picks nerd when the Windows Terminal profile's font is a Nerd Font (kete setup terminal installs one)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::Animations,
                label: "Animations",
                hint: "Status text sweep and splash motion (off = fewer repaints)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::BlackBackground,
                label: "Black background",
                hint: "Paint the window pure black instead of the terminal's own background (off keeps the terminal's, transparency included)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::SessionPane,
                label: "Session pane",
                hint: "Where the session under the cursor is read: beside the cards on the right, or under them",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::WorktreeLayout,
                label: "Worktree layout",
                hint: "Each worktree as a row of cards, a compact list, or a thread of prompts",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::InlineGraphics,
                label: "Inline graphics",
                hint: "Render images and Mermaid diagrams inline: auto, protocol names, halfblocks, or off",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::ExpandAllWorktrees,
                label: "Expand all worktrees",
                hint: "Show every worktree's sessions and terminals at once, with no Tab to open one (off = one worktree opens at a time, with Tab)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::Kanban,
                label: "Kanban",
                hint: "Lay the grid out as lanes by status (Running, Needs you, Review, Ship, Done) instead of a band per worktree (⇧K)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::KanbanScope,
                label: "Kanban scope",
                hint: "Whose sessions the kanban lanes hold: the project on screen, or every project (Tab on the board)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::CardIssueNumber,
                label: "Card issue number",
                hint: "Show the #number of the GitHub issue a session was started from on its card; click it to open the issue",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::HighlightCurrentCard,
                label: "Highlight current card",
                hint: "Wash the cursor's card faintly in its status color, breathing while it runs, asks or waits unread, and keep it lit while you type in its pane (off = the plain gray fill)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::HideCardMarks,
                label: "Card marks",
                hint: "Show or hide the ▶ (run terminal) and ❯ (shell) before a terminal card's name and the › before a session card's prompt",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::HideDraftPrs,
                label: "Draft pull requests",
                hint: "Show or hide draft pull requests in / search; checkouts always stay",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::DiffMode,
                label: "Diff layout",
                hint: "How the g diff viewer lays a diff out: side by side, inline with two line-number gutters, or the new text alone (Ctrl+s in the viewer)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::DiffContext,
                label: "Diff context",
                hint: "The unchanged lines around a change: three each side with the rest folded, or the whole file (Ctrl+o in the viewer)",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::SourceControlGraph,
                label: "Show commit graph",
                hint: "Add the read-only git commit graph under the g diff view's changed files",
                group: "",
            },
            // Both apps' (the window draws them too): after the terminal
            // app's rows here, in a group of their own.
            SettingSpec {
                kind: SettingKind::Critters,
                label: "Critters",
                hint: "Little pixel visitors walk along the bottom of the window. Click, drag, or right-click them.",
                group: "Critters",
            },
            SettingSpec {
                kind: SettingKind::CrittersSize,
                label: "Critter size",
                hint: "How big the critters are drawn: small, medium or large",
                group: "Critters",
            },
            SettingSpec {
                kind: SettingKind::CrittersOften,
                label: "Critters visit",
                hint: "How often a critter turns up: rare, normal or busy",
                group: "Critters",
            },
        ]),
    },
    // Generated from the harness registry: the static quick-prompt head
    // first, then one section per entry holding its enabled toggle and
    // its model / effort defaults, in registry order. A new CLI in the
    // `harnesses` map grows its own section with no code change.
    SettingsTab {
        title: "Agents",
        body: TabBody::Agents,
    },
    // Settings that belong to one project rather than to nebula. The tab
    // edits the selected project's entry in `projects` and names that
    // project on its first line, so a row here never reads as a switch
    // for every project at once.
    SettingsTab {
        title: "Project",
        body: TabBody::Project(&[
            SettingSpec {
                kind: SettingKind::RunCommand,
                label: "Run command",
                hint: "Shell line a menu's Run starts in this project's worktrees (empty = its .kete.json \"run\")",
                group: "",
            },
            SettingSpec {
                kind: SettingKind::OpenCommand,
                label: "Open command",
                hint: "Shell line ⇧Enter / ⇧O runs to open a worktree of this project, e.g. open http://localhost:3000 (empty = its .kete.json \"open\")",
                group: "",
            },
        ]),
    },
    // Behaviors that change how the tree is worked, off by default until
    // they have earned a tab of their own. Before Hotkeys, which stays last
    // for the reason above.
    SettingsTab {
        title: "Experimental",
        body: TabBody::Values(&[
            SettingSpec {
                kind: SettingKind::RememberHarness,
                label: "Remember harness",
                hint: "A harness (and model) picked for a session becomes the Agents tab default the next launch starts on",
                group: "",
            },
        ]),
    },
    SettingsTab {
        title: "Hotkeys",
        body: TabBody::Hotkeys,
    },
];

/// Index of the Hotkeys tab, which the overlay special-cases.
/// Is `tab` the Appearance tab, where `⇧D` puts Kete's own look back?
pub fn is_appearance_tab(tab: usize) -> bool {
    SETTINGS_TABS
        .get(tab)
        .is_some_and(|t| t.title == "Appearance")
}

pub fn hotkeys_tab() -> usize {
    SETTINGS_TABS
        .iter()
        .position(|t| t.body == TabBody::Hotkeys)
        .expect("SETTINGS_TABS declares a Hotkeys tab")
}

/// Index of the Agents tab, generated from the harness registry.
pub fn agents_tab() -> usize {
    SETTINGS_TABS
        .iter()
        .position(|t| t.body == TabBody::Agents)
        .expect("SETTINGS_TABS declares an Agents tab")
}

pub fn tab_count() -> usize {
    SETTINGS_TABS.len()
}

/// The static value settings of a tab: the declared list, or the Agents
/// head (its per-harness sections are generated — see
/// [`Config::agent_rows`]). Empty for the Hotkeys tab.
pub fn tab_settings(tab: usize) -> &'static [SettingSpec] {
    match SETTINGS_TABS.get(tab).map(|t| t.body) {
        Some(TabBody::Values(settings) | TabBody::Project(settings)) => settings,
        Some(TabBody::Agents) => AGENTS_HEAD,
        _ => &[],
    }
}

/// How many selectable rows a tab holds. The Agents tab reads the
/// registry, so a new CLI grows it without a code change.
pub fn tab_len(tab: usize) -> usize {
    match SETTINGS_TABS.get(tab).map(|t| t.body) {
        Some(TabBody::Values(settings)) => settings.len(),
        // The declared rows, then a **Default account** row per harness
        // with profiles ([`Config::project_account_harnesses`]).
        Some(TabBody::Project(settings)) => {
            settings.len() + Config::load().project_account_harnesses().len()
        }
        Some(TabBody::Hotkeys) => crate::actions::ACTIONS.len(),
        Some(TabBody::Agents) => AGENTS_HEAD.len() + Config::load().agent_rows().len(),
        None => 0,
    }
}

/// The static value setting at a tab-local index, if the tab declares one
/// there: the full list on ordinary tabs, the head on the Agents tab
/// (its harness rows resolve through [`Config::agent_row`]), never on
/// Hotkeys.
pub fn setting_at(tab: usize, index: usize) -> Option<&'static SettingSpec> {
    tab_settings(tab).get(index)
}

/// Where an Agents tab harness row lives, as `(tab, row)`. Reads the
/// registry, like the tab itself.
pub fn locate_agent(id: &str, field: HarnessField) -> Option<(usize, usize)> {
    let tab = agents_tab();
    let rows = Config::load().agent_rows();
    rows.iter()
        .position(|(row_id, row_field)| row_id == id && *row_field == field)
        .map(|i| (tab, AGENTS_HEAD.len() + i))
}

/// The row declared for `kind`, wherever it sits — for anything that
/// wants its label or hint by name (the typed-row prompt's title).
pub fn spec_for(kind: SettingKind) -> Option<&'static SettingSpec> {
    all_settings()
        .map(|(_, _, spec)| spec)
        .find(|spec| spec.kind == kind)
}

/// Every static value setting, tab by tab, for coverage checks. The
/// Agents tab contributes its head; its harness rows are covered through
/// [`Config::agent_rows`].
pub fn all_settings() -> impl Iterator<Item = (usize, usize, &'static SettingSpec)> {
    SETTINGS_TABS.iter().enumerate().flat_map(|(t, tab)| {
        tab_settings(t).iter().enumerate().map(move |(i, s)| {
            let _ = tab;
            (t, i, s)
        })
    })
}

/// The one-line hint under the selected row, whatever kind of row it is.
/// The Agents tab reads the registry for its harness rows.
pub fn hint_at(tab: usize, index: usize) -> String {
    match SETTINGS_TABS.get(tab).map(|t| t.body) {
        Some(TabBody::Values(settings)) => settings
            .get(index)
            .map(|s| s.hint)
            .unwrap_or("")
            .to_string(),
        Some(TabBody::Project(settings)) => match settings.get(index) {
            Some(spec) => spec.hint.to_string(),
            None => Config::load()
                .project_account_row(index)
                .map(|h| {
                    format!(
                        "The {h} account a launch in this project naming none runs on, ahead of the account spread and the {h} default (←/→ or Enter: the next account; none: the spread picks)"
                    )
                })
                .unwrap_or_default(),
        },
        Some(TabBody::Hotkeys) => crate::actions::spec_at(index)
            .map(|s| s.hint)
            .unwrap_or("")
            .to_string(),
        Some(TabBody::Agents) => Config::load().agent_hint_by_index(index),
        None => String::new(),
    }
}

/// One terminal row of the settings overlay body, in display order.
/// Shared by the renderer and mouse hit-testing so they can't drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsRow {
    Blank,
    Header(String),
    /// The Project tab's first line: the selected project's name and repo
    /// path, which the renderer reads off the app — the row map is static
    /// and only knows there is such a line. Not selectable.
    Project,
    /// Label + value line for the value setting at this tab-local index.
    Setting(usize),
    /// Label + chord list for `keymap::ACTIONS[index]`.
    Hotkey(usize),
}

impl SettingsRow {
    /// The tab-local selection index this row stands for, if it's one the
    /// cursor can land on.
    pub fn index(&self) -> Option<usize> {
        match self {
            SettingsRow::Setting(i) | SettingsRow::Hotkey(i) => Some(*i),
            _ => None,
        }
    }
}

pub fn settings_rows(tab: usize) -> Vec<SettingsRow> {
    match SETTINGS_TABS.get(tab).map(|t| t.body) {
        Some(TabBody::Values(settings)) => grouped(
            settings.iter().map(|s| s.group.to_string()),
            SettingsRow::Setting,
        ),
        Some(TabBody::Project(settings)) => {
            let mut rows = vec![SettingsRow::Project];
            rows.extend(grouped(
                settings.iter().map(|s| s.group.to_string()),
                SettingsRow::Setting,
            ));
            // A **Default account** row per harness with profiles, under
            // its own header ([`Config::project_account_row`]).
            let harnesses = Config::load().project_account_harnesses();
            if !harnesses.is_empty() {
                rows.push(SettingsRow::Blank);
                rows.push(SettingsRow::Header(PROJECT_ACCOUNTS_HEADER.into()));
                rows.extend((0..harnesses.len()).map(|i| SettingsRow::Setting(settings.len() + i)));
            }
            rows
        }
        Some(TabBody::Hotkeys) => grouped(
            crate::actions::ACTIONS.iter().map(|s| s.group.to_string()),
            SettingsRow::Hotkey,
        ),
        Some(TabBody::Agents) => {
            let cfg = Config::load();
            let head = AGENTS_HEAD.iter().map(|s| s.group.to_string());
            let rows = cfg.agent_rows();
            let groups = rows
                .iter()
                .map(|(id, _)| cfg.effective_harness_by_id(id).display_label().to_string());
            grouped(head.chain(groups), SettingsRow::Setting)
        }
        None => Vec::new(),
    }
}

/// Lays a table that is already in group order out under its section
/// headers: a header whenever the group name changes, a blank line
/// before every header but the first, and no header at all for a row
/// whose group is empty — so a tab with no groups is the bare list it
/// always was.
fn grouped(
    groups: impl Iterator<Item = String>,
    row: fn(usize) -> SettingsRow,
) -> Vec<SettingsRow> {
    let mut rows = Vec::new();
    let mut current: Option<String> = None;
    for (i, group) in groups.enumerate() {
        if !group.is_empty() && current.as_deref() != Some(&group) {
            if !rows.is_empty() {
                rows.push(SettingsRow::Blank);
            }
            rows.push(SettingsRow::Header(group.clone()));
            current = Some(group);
        }
        rows.push(row(i));
    }
    rows
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// `/` palette: Enter on a session attaches and focuses the terminal.
    /// When false, Enter only lands on the session's row in the Sessions
    /// panel (previewing it in the pane) — except on a session that NEEDS
    /// FEEDBACK, the red row, which always attaches. Ctrl+O / Ctrl+F
    /// always pick open / focus explicitly, regardless of this setting.
    pub palette_enter_attaches: bool,
    /// RETIRED with every project a git repository. Through 0.37 the
    /// daemon-owned **git init new projects** SETTING (Settings → General,
    /// on by default) could leave a directory the open-project prompt
    /// created without a repository, which then failed to open. The
    /// daemon now always runs `git init` in a folder the user confirmed —
    /// a new one, or an existing one outside any repository — so no tab
    /// shows the row and nothing reads it. Still loaded and written back
    /// as stored, so an older build sharing the file keeps the choice its
    /// user made.
    pub git_init_on_create: bool,
    /// The branch every new WORKTREE nobody named a base for starts from
    /// (a project's **New worktree**, a bare `nebula worktree`, the QUICK
    /// PROMPT's auto-created one). Empty — the default, shown as `auto` —
    /// is origin's own default branch, `origin/HEAD` freshly fetched; a
    /// name (`master`, `develop`) is origin's fetched copy of that branch
    /// when origin has one, else the checkout's local branch of that name,
    /// else the default again. Owned by the daemon, which does the
    /// resolving (`git::add_worktree_off_configured`); the TUI writes it so
    /// the settings overlay can edit every key in the shared file.
    pub worktree_base_branch: String,
    /// Editor command the file finder (`f`), tree browser (`b`),
    /// find-in-files (`F`), and ⌥click file links launch, invoked as
    /// `<editor> +<line> <file>`. Any command passes through verbatim, so
    /// hand-edited configs can name editors the picker doesn't list. The
    /// `NEBULA_EDITOR` env var overrides it for the process; see
    /// [`Config::editor_command`].
    pub editor: String,
    /// Opening a file from the file finder (`f`) or find-in-files (`F`)
    /// closes that overlay as the editor modal opens, so quitting the
    /// editor lands back on the panels instead of on the finder the user
    /// then has to Esc a second time. When false the finder stays open
    /// underneath and quitting the editor returns to the results. Does not
    /// touch the tree browser (`b`), whose editor is embedded in its own
    /// preview pane, or ⌥click, which has no overlay to close.
    pub close_finder_on_open: bool,
    /// `nebula ssh` and `nebula tunnel` carry this machine's `config.json`
    /// and AGENT PRESETS to the remote nebula, which merges them into its
    /// own on every connect — its `config.local.json` still wins there. On
    /// by default; `--no-sync-config` leaves them behind for one connection.
    pub ssh_sync_config: bool,
    /// How long an idle session in an unviewed worktree lives before the
    /// daemon reaps its PTY: "1m", "5m", "15m", "30m", "1h"; "off"
    /// disables. Owned by the daemon (which does the parsing and reaping);
    /// the TUI writes it so the settings overlay can cycle it.
    pub session_idle_timeout: String,
    /// PREWARM POOL: keep one booted agent CLI standing by in the selected
    /// worktree, so creating a session there adopts it instead of waiting
    /// on a cold start. Owned by the daemon (which spawns, adopts and reaps
    /// the spare); the TUI writes it so the settings overlay can toggle it.
    /// A spare is a real CLI process — Claude's own `/list-agents` lists
    /// it beside the sessions you made, named after the directory — and
    /// switching this off drains the pool on the daemon's next sweep.
    pub prewarm_agents: bool,
    /// SESSION PREWARM: boot a worktree's dead sessions while the selection
    /// rests on it, so attaching lands on a booted screen. Daemon-owned and
    /// TUI-written, same as above.
    pub prewarm_sessions: bool,
    /// What rings when a turn reaches FINISHED: "off", "bell" (terminal
    /// BEL) or the name of a macOS system sound (`Glass` by default,
    /// `Ping`, …; see [`SOUNDS`]). Resolved by [`Config::done_sound`],
    /// which falls back to the bell wherever `afplay` can't reach the
    /// user's speakers.
    pub done_sound: String,
    /// What rings when a turn stops at NEEDS FEEDBACK — a permission
    /// prompt or a question the agent is parked on. Same values and
    /// resolution as `done_sound`; `Sosumi` by default so red and green
    /// sound different from the next room. The one knob for both the
    /// FEEDBACK SOUND and the desktop notification an unfocused terminal
    /// window gets: "off" silences the pair.
    pub feedback_sound: String,
    /// PRESET TEXT: which side of the task a new AGENT PRESET's text goes
    /// — `prefix` (one box, sent before the task), `postfix` (one box,
    /// sent after it) or `prefix & postfix` (both). The PRESET EDITOR
    /// opens a new preset on the box(es) named here, and its Text row
    /// changes one preset; a stored side with text always shows. Resolved
    /// by [`Config::preset_text`]; `prefix` by default, the framing most
    /// people reach for and one box to fill.
    pub preset_text: String,
    /// DELETE EMPTIED WORKTREE: what the delete of a linked worktree's
    /// last live card asks — its last session's `d`, its last terminal's
    /// close, or a `D` that takes them all. Off (the default), the card's
    /// own CONFIRM DIALOG carries the question too, before anything is
    /// deleted: `Enter`/`y` deletes the card and then the worktree, `n`
    /// the card alone, `Esc` nothing. On, the question is skipped and the
    /// card's ordinary confirm deletes both — as long as no archived
    /// session is still filed under the checkout: those hold history the
    /// delete would take, so they always get the question. The ROOT
    /// WORKTREE is never offered, whatever this says. On holds with
    /// [`Config::show_all_worktrees`] on too: only the question is that
    /// setting's to drop.
    pub delete_empty_worktree: bool,
    /// AUTO-CLEAN MERGED WORKTREES: when the PR row for a linked worktree is
    /// a trustworthy merge, ask the daemon to archive idle sessions there
    /// and remove the checkout from disk. Off by default; root/default/base
    /// branches, dirty checkouts, active sessions and mismatched heads are
    /// skipped.
    pub auto_cleanup_merged: bool,
    /// SHOW ALL WORKTREES: every checkout of the project gets a BAND on
    /// the grid, one with nothing running in it too — an overview of the
    /// checkouts, any of them a place to aim `p`/`n` at. On by default.
    /// Off, the grid is only what is running. On, deleting a
    /// worktree's last card never asks about the worktree: the emptied
    /// band stays, and `d` on it — behind its own confirm — is the way to
    /// delete it. [`Config::delete_empty_worktree`] on still takes the
    /// worktree with its last card.
    pub show_all_worktrees: bool,
    /// Color theme name (see `theme::THEMES`). Unknown names fall back to
    /// the default theme.
    pub theme: String,
    /// The ICON STYLE of the FILE ICONS (`crate::icons`): `nerd`,
    /// `ascii`, `off`, or `auto` — the default — which picks `nerd` when
    /// the Windows Terminal profile Kete runs in names a Nerd Font, else
    /// `ascii`. Resolved when the setting is applied; the file keeps the
    /// word, so `auto` looks again on the next start.
    pub icons: String,
    /// Master switch for the TUI's animations (the running/needs-feedback
    /// status-text sweep and the splash's motion). Off trades them for
    /// fewer repaints on constrained machines.
    pub animations: bool,
    /// BLACK BACKGROUND: paint every cell nothing else colored pure black
    /// — the grid, the cards, the session pane, the overlays — instead of
    /// leaving it on the terminal's own background, which in a stock
    /// Ghostty is a dark gray. On by default; off lets a transparency or
    /// image configured in the terminal show through.
    pub black_background: bool,
    /// HIDE CARD MARKS: leave the `▶` (a RUN TERMINAL) and the `❯` (a
    /// plain shell) off the front of a terminal card's name, and the `›`
    /// off the front of a session card's prompt, each of which then starts
    /// where its mark did. Off by default. Read under the key it had
    /// before, `hide_terminal_glyphs`, too.
    #[serde(alias = "hide_terminal_glyphs")]
    pub hide_card_marks: bool,
    /// HIGHLIGHT CURRENT CARD: the card under the cursor — the one the
    /// pane reads — trades its gray fill for a faint wash of its status
    /// color, breathing while something is going on, and keeps it while
    /// the pane has the keys. On by default; off leaves the gray fill.
    pub highlight_current_card: bool,
    /// Where the LAUNCHER VIEW's PANE — the session under the cursor, live
    /// — sits against the GRID of cards: `right` (down that side of them,
    /// the default) or `bottom` (under them). Also written by the SIDE
    /// BUTTON on the pane's own TAB STRIP. Read through
    /// [`Config::pane_side`], so a word off the list — the `left` older
    /// builds offered too — is the right; a window too narrow for the
    /// pane beside the cards lays it out along the bottom until there is
    /// room (`launcher::fitted_side`).
    pub session_pane: String,
    /// How the LAUNCHER VIEW lays out each worktree's BAND: `cards` (a row
    /// of cards, the default) or `list` (every session one line, stacked
    /// under the band's rule, the TUI `launcher::LIST_RECENT` most
    /// recent shown until Tab — the ACCORDION — opens the rest) or `nested`
    /// (a thread per worktree: its first prompt the root row, every later
    /// prompt and terminal a one-line child under it). Read through
    /// [`Config::worktree_layout_word`], so a word off the list is the
    /// cards.
    pub worktree_layout: String,
    /// KANBAN: the grid laid out as five lanes by status — Running,
    /// Needs you, Review, Ship, Done — instead of a band per worktree.
    /// `⇧K` flips it and writes it back. Off by default.
    pub kanban: bool,
    /// KANBAN SCOPE: `project` (the project on screen's sessions, the
    /// default) or `all` (every project's). `Tab` on the board flips it
    /// and writes it back. Anything else reads as `project`.
    pub kanban_scope: String,
    /// ORCHESTRATOR VIEW at startup ([`ORCHESTRATOR_VIEWS`]). Left out of a
    /// save while it is `auto`, so a config that never touched it saves as
    /// it did before the key existed.
    #[serde(skip_serializing_if = "is_auto")]
    pub orchestrator_view: String,
    /// INLINE GRAPHICS: images and Mermaid diagrams in file previews.
    /// `off` shows source/hints only; enabled values currently use the
    /// Unicode half-block renderer, which works in native terminals,
    /// ttyd/xterm.js and over ssh. Protocol names are accepted as stable
    /// config values for future native protocol renderers.
    pub inline_graphics: String,
    /// EXPAND ALL WORKTREES: every BAND on the GRID laid out open at once
    /// — each worktree's sessions and terminals wrapped into rows under
    /// its rule, every entry of the compact LIST listed — so there is no
    /// ACCORDION for Tab to open, and `j`/`k` walk down every card of
    /// every worktree as one column of rows. Off by default, a config
    /// predating the key too: one band opens at a time, with Tab.
    pub expand_all_worktrees: bool,
    /// RETIRED with every card carrying its last prompt. Through 0.40 the
    /// **Card prompt** SETTING (Settings → Appearance, `shown` by default)
    /// could leave the last prompt off every session card on the GRID.
    /// Every card shows it now, whatever this says, so no tab shows the
    /// row and nothing reads it. Still loaded and written back as stored,
    /// so an older build sharing the file keeps the choice its user made.
    pub hide_card_prompt: bool,
    /// Show the `#15` of the GitHub issue an ISSUE SESSION was started
    /// from on its card on the GRID, a link a click opens in the browser
    /// (the very `⇧I` the card runs). On by default, a config predating
    /// the key too.
    pub card_issue_number: bool,
    /// Leave draft pull requests out of the PROJECT OPEN PRS GROUP and the
    /// `/` PALETTE's pull-request rows, so browsing what's open shows only
    /// the rows asking for a reviewer. A view filter, not a fetch filter:
    /// the open list's query still returns the drafts and the cache holds
    /// them, so switching this off shows them again at once, and a draft
    /// marked ready on GitHub joins the rows on the next refresh. Never
    /// touches a checkout, its sessions, or the checkout's own PR ROW in
    /// the SESSIONS PANEL — those describe work you have, not work you are
    /// browsing. Off by default: a config predating the key hides nothing.
    pub hide_draft_prs: bool,
    /// SOURCE CONTROL GRAPH: when on, the `g` DIFF VIEWER reads a
    /// read-only commit graph under the changed files. Off by default, and
    /// with it off the DIFF VIEWER does not run `git log` or `rev-list`.
    pub source_control_graph: bool,
    /// The DIFF LAYOUT the `g` DIFF VIEWER opens in: `side-by-side`,
    /// `inline` or `replacement` (`crate::choices::DiffMode`), which
    /// `Ctrl+s` in the viewer cycles and writes back. Empty — a config
    /// that never set it — reads as side by side, unless the side-by-side
    /// switch older builds kept in state.json says unified
    /// ([`Config::diff_mode`]).
    pub diff_mode: String,
    /// UNCHANGED CONTEXT in the DIFF VIEWER: `collapsed` (three lines each
    /// side of a change, the rest folded into the hunk's row) or `full`
    /// (the whole file). `Ctrl+o` in the viewer flips it and writes it
    /// back. Anything else reads as collapsed.
    pub diff_context: String,
    /// RETIRED with the line counts always drawn. Through 0.37 the **Card
    /// line counts** SETTING (Settings → Appearance, off by default)
    /// switched each card's `+3 files` to `+3 files +120 -45`. Every card
    /// counts its lines now, whatever this says, so no tab shows the row
    /// and nothing reads it. Still loaded and written back as stored, so
    /// an older build sharing the file keeps the choice its user made.
    pub card_line_changes: bool,
    /// The key of the **Skip starting prompt** SETTING (Settings →
    /// Sessions, through 0.30): on, `n` created the session straight from
    /// the NEW SESSION PICKER instead of putting a task box up first.
    /// Every `n` does that now — a launch that starts from a typed task is
    /// the QUICK PROMPT's — so this build never reads it and no tab edits
    /// it any more. Still loaded and written back as stored, so an older
    /// build sharing the file keeps the behavior its user chose.
    pub skip_session_naming: bool,
    /// RETIRED. Through 0.34, on, it put a CONFIRM DIALOG in front of
    /// archiving a session — the `a` key and the row menu's Archive alike
    /// — and off (the default) archived at once; 0.35 through 0.42 asked
    /// always. The **Confirm on archive** row is back, on
    /// [`Config::ask_before_archive`]: a new key, because every file an
    /// earlier build saved holds this one's old default `false`, which
    /// would have switched the confirm off for nearly everyone. Nothing
    /// reads it; still loaded and written back as stored, so an older
    /// build sharing the file keeps the behavior its user chose.
    pub confirm_on_archive: bool,
    /// CONFIRM ON ARCHIVE (Settings → Sessions): on, the default, `a` and
    /// a card's menu Archive put a CONFIRM DIALOG in front of archiving
    /// the session — Enter/`y` archives, Esc/`n` keeps it — so a letter
    /// aimed at an agent that lands on the grid archives nothing. Off
    /// archives at once (`u` in the ARCHIVED VIEW brings it back), and a
    /// held `a` still archives one card per press (the RELEASE WATCH).
    pub ask_before_archive: bool,
    /// ACCOUNT SPREAD (Settings → Sessions → Accounts): which profile a
    /// launch naming none takes — `manual`, `failover`, `least_used`
    /// (`nebula_core::accounts`).
    pub account_spread: String,
    /// The percent at which `failover` moves on.
    pub account_spread_threshold: String,
    /// Install kete's statusLine wrapper into Claude sessions, the source
    /// of their plan usage and context (`kete _statusline`).
    pub usage_statusline: bool,
    /// FINISHED SUBAGENTS: a session's ended subagents (done, failed,
    /// closed with their session) fold into a count under it on the board,
    /// the grid's cards and the session tree, one click or key from
    /// showing; running ones always show. On by default, a config
    /// predating the key too.
    pub hide_done_subagents: bool,
    /// KEEP FINISHED SUBAGENTS ([`SUBAGENT_RETENTIONS`]): how long the
    /// daemon keeps an ended subagent before its sweep drops it. Read
    /// through [`Config::subagent_retention_word`]; default `1h`.
    pub subagent_retention: String,
    /// IDLE NOTE AFTER ([`STUDIO_IDLE_AFTERS`]): how long the orchestrator
    /// sits idle before the studio queue runner hands it an idle note.
    /// Read through [`Config::studio_idle_after`]; default `45s`.
    pub studio_idle_after: String,
    /// USAGE LEDGER: record each session's spend per day (`kete usage`).
    /// On by default; off keeps the rows already recorded.
    pub usage_ledger: bool,
    /// The key of the **Focused panel tint** SETTING (Settings →
    /// Appearance, through 0.34): whether the faint accent wash behind
    /// whatever keys land in — the card under the cursor, or the session
    /// pane — was painted at all. It always is now: the wash is the one
    /// cue that says which surface keys land in, so this build never
    /// reads the key and no tab edits it. Still loaded and written back
    /// as stored, so an older build sharing the file keeps the choice its
    /// user made.
    pub focus_tint: bool,
    /// The key of the **Workspaces bar** SETTING (Settings → Appearance,
    /// through 0.33): whether the bar of WORKSPACE tabs was drawn across
    /// the top. Workspaces are gone — every project is in the one list the
    /// PROJECT TABS open from — so this build never reads it and no tab
    /// edits it. Still loaded and written back as stored, so an older
    /// build sharing the file keeps the bar its user chose.
    pub show_workspaces: bool,
    /// RETIRED with the three-panel layout. Through 0.37 the **Projects panel**
    /// SETTING (Settings → Appearance) collapsed that panel to a rail; the
    /// GRID has no panels, so no tab shows the row and nothing reads it.
    /// Still loaded and written back as stored, so an older build sharing
    /// the file keeps the layout its user chose.
    pub hide_projects: bool,
    /// RETIRED with the three-panel layout, as `hide_projects` is: the
    /// **Worktrees panel** SETTING through 0.37.
    pub hide_worktrees: bool,
    /// RETIRED with the three-panel layout, as `hide_projects` is: the
    /// **Sessions panel** SETTING through 0.37.
    pub hide_sessions: bool,
    /// RETIRED with the root always listed. Through 0.27 one switch for
    /// every project (Settings → Experimental), then through 0.35 the
    /// fallback for a project whose `projects` entry had no **Hide root
    /// worktree** row of its own: on, a project's ROOT WORKTREE was left
    /// out of everything the grid launched into. Nothing hides the root
    /// now — a launch that must not land in the shared checkout cuts a
    /// fresh worktree instead — so no tab shows the row and nothing reads
    /// this key, nor the one an entry may still carry (which rides through
    /// [`ProjectSettings::other`]). Still loaded and written back as
    /// stored, so an older build sharing the file keeps the choice its
    /// user made.
    pub hide_root_worktree: bool,
    /// RETIRED with every card carrying its session's last prompt. Through
    /// 0.37 the **Recent prompts** SETTING (Settings → Experimental) listed a
    /// session's last prompts under its row; the card shows the newest one
    /// whatever this says, so no tab shows the row and nothing reads it.
    /// Still loaded and written back as stored, so an older build sharing
    /// the file keeps the rows its user chose.
    pub recent_prompts: bool,
    /// RETIRED with `recent_prompts`: how many prompts that build listed
    /// (`1` to `5` in its overlay). Loaded and written back as stored.
    pub recent_prompts_count: usize,
    /// RETIRED with the KEY COMBO DISPLAY always on. Through 0.37 the
    /// **Key combo display** SETTING (Settings → Experimental) switched
    /// the readout — each key pressed spelled at the bottom left of the
    /// screen with what it did, `j - Move down` — on, off by default.
    /// Every key shows now, whatever this says (`key_combo.rs`), so no
    /// tab shows the row and nothing reads it. Still loaded and written
    /// back as stored, so an older build sharing the file keeps the
    /// choice its user made.
    pub show_key_combos: bool,
    /// PROJECT SETTINGS: one [`ProjectSettings`] per project set up
    /// differently from the rest, keyed by the project's repo path as the
    /// DAEMON stores it — what the Settings → Project tab edits for the
    /// selected project. A project with no entry reads as the defaults
    /// ([`ProjectSettings::default`]), and an entry that says nothing they
    /// don't is dropped on save ([`Config::set_project`]), so
    /// the map names only the projects that differ. One key to the file's
    /// rules: a value in here this build can't read costs the whole map,
    /// not one project.
    pub projects: BTreeMap<PathBuf, ProjectSettings>,
    /// Experimental: REMEMBER HARNESS — a launch walked through the NEW
    /// SESSION PICKER, the PR SESSION picker or the QUICK PROMPT's `Tab`
    /// picker writes its harness into `quick_prompt_kind`, and a model or
    /// effort a submenu chose into that harness's own rows, so the next
    /// picker starts on it and the next `p` launches it
    /// ([`Config::remember_launch`]). Off by default: a pick is one
    /// session's, and the AGENTS TAB is where the defaults are set.
    pub remember_harness: bool,
    /// RETIRED with the counts always drawn. Through 0.37 the **PR & issue
    /// counts** SETTING (Settings → Experimental, on by default) switched
    /// the GRID header's `3 prs · 2 issues` off, and with it the issue
    /// sweep over the other projects. The header always counts now — what
    /// is waiting on a repo is the one thing the grid must say — so no tab
    /// shows the row and nothing reads it. Still loaded and written back
    /// as stored, so an older build sharing the file keeps the choice its
    /// user made.
    pub pr_issue_counts: bool,
    /// Default model/effort for new Claude / Codex / Cursor sessions.
    /// "default" means "don't pass the flag" (the CLI picks); any other
    /// value is passed through verbatim, so hand-edited configs can name
    /// models the pickers don't list. Cursor's pair is a family plus the
    /// effort suffix the daemon joins onto it (see `cursor_catalogue.rs`).
    pub claude_model: String,
    /// The Claude model rows the pickers, the AGENTS TAB and the PRESET
    /// EDITOR offer, verbatim, in place of the built-in aliases — for an
    /// organization allowlist or a provider (Bedrock, Vertex, a gateway)
    /// whose ids the aliases don't reach: `["claude-sonnet-5",
    /// "us.anthropic.claude-opus-5-v1:0"]`. Empty (the default) means the
    /// list follows Claude Code's own `availableModels` when one is on
    /// disk, else the aliases; see `claude_catalogue.rs`. Hand-edited only.
    pub claude_models: Vec<String>,
    pub claude_effort: String,
    pub codex_model: String,
    pub codex_effort: String,
    pub cursor_model: String,
    pub cursor_effort: String,
    /// Pi's pair: a `--model` pattern and a `--thinking` level.
    pub pi_model: String,
    pub pi_effort: String,
    /// Muse's `--model` id. `muse_effort` is reserved until the CLI
    /// documents a reasoning flag; it stores but sends nothing.
    pub muse_model: String,
    pub muse_effort: String,
    /// OpenCode's `--model` id (`provider/model`, passed verbatim). No
    /// effort key: OpenCode has no effort flag — reasoning is a per-model
    /// variant picked inside its own TUI — so its Agents section has no
    /// Effort row and nothing to store for one.
    pub opencode_model: String,
    /// Which AGENT KINDS the NEW SESSION PICKER offers. Off leaves that
    /// harness out of the picker and the PR SESSION picker (and, for
    /// Claude, out of the standing PREWARM POOL slot); sessions that already
    /// exist keep attaching, resuming and restarting as before. All on by
    /// default, so a config predating the keys hides nothing.
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    pub cursor_enabled: bool,
    pub pi_enabled: bool,
    pub muse_enabled: bool,
    pub opencode_enabled: bool,
    /// When on, the New session picker lists only enabled harnesses whose
    /// CLI is found on this machine's PATH. Off by default: a login shell
    /// (mise, brew shims) can see CLIs a plain PATH lookup misses, and the
    /// daemon re-checks through the login shell at launch anyway.
    pub hide_uninstalled_harnesses: bool,
    /// User-defined harnesses (`custom_harnesses` in config.json): offered
    /// in the New session picker after the built-ins when enabled, launched
    /// with the entry's program and model flag, with process-based status
    /// unless the entry names a hook dialect. Empty by default. Legacy:
    /// new harnesses belong in `harnesses` as full descriptors, where
    /// they also gain resume, effort, system-prompt and hook-dialect rows.
    pub custom_harnesses: Vec<CustomHarness>,
    /// Per-harness deltas over the compiled-in registry (`harnesses` in
    /// config.json): disable a built-in, repoint a program, rename a
    /// flag, or define a whole new CLI. Merged by
    /// [`Config::harness_registry`]; the Agents tab, the `n` picker, the
    /// `e` presets, spawn and hooks all read the merged rows. A hand edit
    /// that breaks one entry refuses its launches with the reason, never
    /// the whole file (see `nebula_core::settings`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub harnesses: BTreeMap<String, nebula_core::harness::HarnessOverride>,
    /// PROFILES (`profiles` in config.json): the accounts a launch can run
    /// under, several per harness. `kete profile` edits the list; the
    /// QUICK PROMPT's `^A` cycles a launch through its harness's ones, and
    /// a card names the one its session runs on when it is not the
    /// harness's default. See `nebula_core::profiles`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<nebula_core::profiles::Profile>,
    /// LOCALHOST PREVIEWS: a browser profile per browser, by id
    /// (`chrome`, `edge`, `firefox`) — `{"chrome": {"profile": "Profile
    /// 1"}}` — for `w`'s chooser to open the dev server in. Passed as
    /// `--profile-directory` to Chrome and Edge, `-P` to Firefox.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub browsers: BTreeMap<String, BrowserSettings>,
    /// Which AGENT KIND the QUICK PROMPT hotkey launches. Its model and
    /// effort come from that kind's own defaults above, so the setting is
    /// one name, not a third model/effort pair. Read through
    /// [`Config::quick_prompt_kind`], which steps around a harness that has
    /// since been switched off.
    pub quick_prompt_kind: String,
    /// Whether a QUICK PROMPT launch takes FOCUS into the TERMINAL PANE and
    /// locks it. Off by default: FOCUS stays on the panel the prompt was
    /// fired from, so firing one off does not interrupt what you were
    /// doing — and where the cursor goes is [`Config::follow_new_session`]'s
    /// to say. Only the QUICK PROMPT reads this — every other launch
    /// (the NEW SESSION PICKER, an AGENT PRESET, a PR SESSION, a Cloud task)
    /// still enters the pane.
    pub quick_prompt_focus: bool,
    /// FOLLOW NEW SESSION: a QUICK PROMPT launch lands the cursor on the
    /// new session's card — the grid scrolled to it, the pane showing it,
    /// the keys still on the cards — so a run of launches can be watched
    /// going up. It selects the card and no more: entering its terminal
    /// is [`Config::quick_prompt_focus`]'s to say. On by default, a
    /// config predating the key too. Off, the cursor, the pane and FOCUS
    /// stay on the session the user was on while the new card goes up in
    /// its band, as a BACKGROUND LAUNCH already does for another project.
    /// Only a launch fired from a session card holds still: with no card
    /// under the cursor there is nothing to keep, and the cursor lands on
    /// the new session either way. [`Config::quick_prompt_focus`] on
    /// outranks it — a launch that enters the new session's pane has to
    /// go there.
    pub follow_new_session: bool,
    /// CONFIRM DRAG MOVE: a session card dragged onto another band or
    /// PROJECT TAB asks before the session moves, since a drop is easy to
    /// make by accident and the move restarts the CLI. On by default, a
    /// config predating the key too. The MOVE PICKER (`m`) never asks: a
    /// pick and Enter are already deliberate.
    pub confirm_drag_move: bool,
    /// Whether each new QUICK PROMPT starts aimed at a fresh worktree
    /// rather than the checkout under the grid's cursor (the project's
    /// ROOT BRANCH when nothing is aimed at — `launcher::target_for`).
    /// `^N` flips the one box that is up; the next box starts from this
    /// again. Off by default.
    pub quick_prompt_new_worktree: bool,
    /// SHOW TIPS: the one-line first-use tips ([`crate::learn::TIPS`]),
    /// each shown once. On by default, a config predating the key too.
    pub show_tips: bool,
    /// CRITTERS: the overlay of little pixel visitors along the bottom of
    /// either app's window. Off by default, a config predating the key too.
    pub critters: bool,
    /// How big the critters are drawn ([`CRITTER_SIZES`]). Read through
    /// [`Config::critters_size_word`], so a word off the list is `medium`.
    pub critters_size: String,
    /// How often a critter visits ([`CRITTER_VISITS`]). Read through
    /// [`Config::critters_often_word`], so a word off the list is `normal`.
    pub critters_often: String,
    /// The tips already shown, by [`crate::learn::Tip::id`]: shared by
    /// both apps, so one seen in the terminal app is not shown again in
    /// the window. Left out of the file while empty. No settings row:
    /// turning Show tips off and on again doesn't bring them back.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub tips_seen: BTreeSet<String>,
    /// The release whose WHAT'S NEW page was last shown (`0.52`, major.minor
    /// of [`crate::learn::current_version`]): an app that runs a newer one
    /// shows the page once and writes it here. Left out while empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub whats_new_seen: String,
    /// The checkouts, by path, already told that git tracks a file Kete
    /// writes for an agent's hooks (`nebula_core::tracked_settings`), so
    /// the notice shows once for each. Left out of the file while empty.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub tracked_settings_noticed: BTreeSet<String>,
    /// Hotkey overrides, keyed by `keymap::ActionSpec::id`; the value is a
    /// comma-separated chord list (`"j, down"`), and an empty string means
    /// deliberately unbound. Only rows that differ from the defaults are
    /// written, so the file stays small and new defaults reach existing
    /// installs. See the TUI's `keymap`.
    pub keybindings: BTreeMap<String, String>,
    /// Keys the files held that this build could not read; they loaded as
    /// defaults. Never written: [`Config::save`] reads it to leave those
    /// stored values alone — most likely a newer nebula's — unless the
    /// setting has been changed since. See `nebula_core::settings`.
    #[serde(skip)]
    pub skipped: BTreeSet<String>,
}

/// One project's own settings — the PROJECT TAB's rows — kept under the
/// project's repo path in [`Config::projects`]. Read through
/// [`Config::project`], which supplies the defaults for a project with no
/// entry; written through [`Config::set_project`].
/// One browser's entry under `browsers` (`crate::localhost`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BrowserSettings {
    /// The browser profile to open in: Chrome's and Edge's profile
    /// directory name (`Default`, `Profile 1`), Firefox's profile name.
    /// Empty: the browser's own pick.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub profile: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// The RUN COMMAND a menu's **Run** starts in this project's
    /// worktrees, typed on the Project tab. Empty — the default, shown as `.nebula.json` — is
    /// the checkout's PROJECT FILE `run`, where the command lived before
    /// the row existed; set, it wins over the file. The DAEMON reads it
    /// (`nebula-daemon/src/config.rs`); the TUI only edits it. Left out
    /// of the file while empty, so an entry written before the row reads
    /// the same after a save.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub run_command: String,
    /// The OPEN COMMAND `Shift+Enter` / `Shift+O` fires on this project's
    /// worktrees, typed on the Project tab — `open http://localhost:3000`,
    /// say. Empty (shown as `.nebula.json`) is the checkout's PROJECT FILE
    /// `open`; set, it wins over the file. The TUI both edits and runs it
    /// (`event_loop::open_worktree`): what it opens belongs on the machine
    /// the user sits at, never the DAEMON's. Left out of the file while
    /// empty, like `run_command`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub open_command: String,
    /// The browser `w` last opened this project's dev server in
    /// (`chrome`, `edge`, `firefox`, `default`): the row its chooser
    /// opens on next time. Left out while empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub browser: String,
    /// Keys in the entry this build doesn't know — a newer nebula's, most
    /// likely, or the retired `hide_root_worktree` an older one wrote —
    /// carried through a save untouched, as the file's top-level keys
    /// are. An entry holding one is never dropped as "all default".
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl ProjectSettings {
    /// The overlay's label for a PROJECT TAB row ([`SettingKind::is_project`]);
    /// empty for a row that is not one.
    pub fn value_label(&self, kind: SettingKind) -> String {
        match kind {
            SettingKind::RunCommand => match self.run_command.trim() {
                "" => PROJECT_FILE_CHOICE.into(),
                command => command.to_string(),
            },
            SettingKind::OpenCommand => match self.open_command.trim() {
                "" => PROJECT_FILE_CHOICE.into(),
                command => command.to_string(),
            },
            _ => String::new(),
        }
    }

    /// The stored text of a typed PROJECT TAB row as its prompt pre-fills
    /// it — `""` for an unset row, never the `.nebula.json` the overlay
    /// shows in its place. Empty for a row that is not typed.
    pub fn text_value(&self, kind: SettingKind) -> String {
        match kind {
            SettingKind::RunCommand => self.run_command.clone(),
            SettingKind::OpenCommand => self.open_command.clone(),
            _ => String::new(),
        }
    }

    /// Write a typed value into a typed PROJECT TAB row, trimmed. Empty
    /// puts the row back on its default. False for a row that is not a
    /// typed project row — nothing changes.
    pub fn set_text(&mut self, kind: SettingKind, value: &str) -> bool {
        match kind {
            SettingKind::RunCommand => {
                self.run_command = value.trim().to_string();
                true
            }
            SettingKind::OpenCommand => {
                self.open_command = value.trim().to_string();
                true
            }
            _ => false,
        }
    }

    /// The PROJECT DEFAULT ACCOUNTS: harness id to profile name, as the
    /// entry's `default_profiles` holds them
    /// (`nebula_core::profiles::PROJECT_DEFAULTS_KEY`), carried in
    /// [`ProjectSettings::other`] so a save writes them back untouched.
    /// They are written in place by `profiles::set_project_default_in_file`,
    /// never through this struct.
    pub fn default_profiles(&self) -> BTreeMap<String, String> {
        let mut obj = serde_json::Map::new();
        if let Some(v) = self.other.get(nebula_core::profiles::PROJECT_DEFAULTS_KEY) {
            obj.insert(
                nebula_core::profiles::PROJECTS_KEY.into(),
                serde_json::json!({ "_": { nebula_core::profiles::PROJECT_DEFAULTS_KEY: v } }),
            );
        }
        nebula_core::profiles::project_defaults_from(&obj)
            .remove("_")
            .unwrap_or_default()
    }

    /// The project's default account for `harness`, if it names one.
    pub fn default_profile(&self, harness: &str) -> Option<String> {
        self.default_profiles().remove(harness.trim())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            palette_enter_attaches: true,
            git_init_on_create: true,
            worktree_base_branch: String::new(),
            editor: "vim".into(),
            close_finder_on_open: true,
            ssh_sync_config: true,
            session_idle_timeout: nebula_core::settings::DEFAULT_SESSION_IDLE_TIMEOUT.into(),
            prewarm_agents: true,
            prewarm_sessions: true,
            done_sound: "Glass".into(),
            feedback_sound: "Sosumi".into(),
            preset_text: PresetText::DEFAULT.as_str().into(),
            delete_empty_worktree: false,
            auto_cleanup_merged: false,
            show_all_worktrees: true,
            theme: crate::choices::DEFAULT_THEME.into(),
            icons: crate::choices::ICON_STYLES[0].into(),
            animations: true,
            black_background: true,
            hide_card_marks: false,
            highlight_current_card: true,
            session_pane: PANE_SIDES[0].into(),
            worktree_layout: WORKTREE_LAYOUTS[0].into(),
            kanban: false,
            kanban_scope: KANBAN_SCOPES[0].into(),
            orchestrator_view: ORCHESTRATOR_VIEWS[0].into(),
            inline_graphics: crate::choices::DEFAULT_INLINE_GRAPHICS.into(),
            expand_all_worktrees: false,
            hide_card_prompt: false,
            card_issue_number: true,
            hide_draft_prs: false,
            source_control_graph: false,
            diff_mode: String::new(),
            diff_context: DIFF_CONTEXTS[0].into(),
            card_line_changes: false,
            skip_session_naming: false,
            confirm_on_archive: false,
            ask_before_archive: true,
            account_spread: nebula_core::accounts::SPREADS[0].into(),
            account_spread_threshold: nebula_core::accounts::DEFAULT_THRESHOLD.to_string(),
            usage_statusline: true,
            hide_done_subagents: true,
            subagent_retention: DEFAULT_SUBAGENT_RETENTION.into(),
            studio_idle_after: DEFAULT_STUDIO_IDLE_AFTER.into(),
            usage_ledger: true,
            focus_tint: true,
            show_workspaces: true,
            hide_projects: false,
            hide_worktrees: false,
            hide_sessions: false,
            hide_root_worktree: false,
            recent_prompts: false,
            recent_prompts_count: 3,
            projects: BTreeMap::new(),
            show_key_combos: false,
            remember_harness: false,
            pr_issue_counts: true,
            claude_model: DEFAULT_CHOICE.into(),
            claude_models: Vec::new(),
            claude_effort: DEFAULT_CHOICE.into(),
            codex_model: DEFAULT_CHOICE.into(),
            codex_effort: DEFAULT_CHOICE.into(),
            cursor_model: DEFAULT_CHOICE.into(),
            cursor_effort: DEFAULT_CHOICE.into(),
            pi_model: DEFAULT_CHOICE.into(),
            pi_effort: DEFAULT_CHOICE.into(),
            muse_model: DEFAULT_CHOICE.into(),
            muse_effort: DEFAULT_CHOICE.into(),
            opencode_model: DEFAULT_CHOICE.into(),
            claude_enabled: true,
            codex_enabled: true,
            cursor_enabled: true,
            pi_enabled: true,
            muse_enabled: true,
            opencode_enabled: true,
            hide_uninstalled_harnesses: false,
            custom_harnesses: Vec::new(),
            harnesses: BTreeMap::new(),
            profiles: Vec::new(),
            browsers: BTreeMap::new(),
            quick_prompt_kind: AgentKind::Claude.as_str().into(),
            quick_prompt_focus: false,
            follow_new_session: true,
            confirm_drag_move: true,
            quick_prompt_new_worktree: false,
            show_tips: true,
            critters: false,
            critters_size: CRITTER_SIZES[1].into(),
            critters_often: CRITTER_VISITS[1].into(),
            tips_seen: BTreeSet::new(),
            tracked_settings_noticed: BTreeSet::new(),
            whats_new_seen: String::new(),
            keybindings: BTreeMap::new(),
            skipped: BTreeSet::new(),
        }
    }
}

/// Keys a field was renamed from, `(old, new)`: the field reads the old key
/// through a `#[serde(alias)]`, and a save takes the old key out of both
/// layers. Left in beside the new one, the pair is a `duplicate field` to
/// serde — a file this build can only read key by key.
#[doc(hidden)]
pub const RENAMED_KEYS: &[(&str, &str)] = &[("hide_terminal_glyphs", "hide_card_marks")];

/// Whether a settings write without a path override panics. Always in this
/// crate's own tests. With `test-support`, only inside a cargo test harness,
/// which cargo builds under `target/<profile>/deps`. A workspace-wide
/// `cargo test` unifies the feature into the `kete` binary the Windows
/// flows drive (`target/<profile>/kete.exe`), and that binary must save
/// into its own data dir like a release build.
#[cfg(any(test, feature = "test-support"))]
fn guard_settings_writes() -> bool {
    cfg!(test)
        || std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.ends_with("deps")))
            .unwrap_or(true)
}

impl Config {
    pub fn load() -> Self {
        let cfg = load_layers(&settings_path(), &local_settings_path());
        // The Claude model rows follow `claude_models` live, as every
        // other hand edit does. Not under test: the list is process-global
        // and a test that never pinned the path would install the dev's.
        #[cfg(not(any(test, feature = "test-support")))]
        crate::claude_catalogue::sync_config(&cfg.claude_models);
        cfg
    }

    /// Patch this config's known keys into the settings files, preserving
    /// any other fields already there: a key `config.local.json` holds goes
    /// back into it, every other key into `config.json`.
    pub fn save(&self) -> std::io::Result<()> {
        // A test that reaches a save without pinning the path would write
        // the dev's own settings file (and `NEBULA_DATA_DIR` only moves it
        // to their dev instance's, which is no better). Saves hang off
        // ordinary keystrokes now — `Shift+W` is one — so make the miss
        // loud instead of leaving it to be noticed in a diff later.
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !guard_settings_writes() || CONFIG_PATH_OVERRIDE.with(|p| p.borrow().is_some()),
            "Config::save() in a test without a path override: wrap the \
             test body in config::with_config_path (or with_default_config)"
        );
        self.save_layers(&settings_path(), &local_settings_path())
    }

    /// [`Config::save`] on explicit layers: one read of both under the
    /// settings lock, this config's known keys patched in, and only a file
    /// whose contents changed written back. A layer that is there but can't
    /// be parsed is an error and nothing is written.
    fn save_layers(&self, path: &Path, local: &Path) -> std::io::Result<()> {
        let _lock = nebula_core::settings::lock(path)?;
        let mut layers = Layers::read(path, local)?;
        self.patch_layers(&mut layers, None);
        layers.commit(path, local, false)
    }

    /// Kete's own look for the terminal app: the Workshop theme, file
    /// icons picked by the terminal's font, the black background. The
    /// Appearance tab's other rows are layout, and stay.
    pub fn reset_look(&mut self) {
        let fresh = Config::default();
        self.theme = fresh.theme;
        self.icons = fresh.icons;
        self.black_background = fresh.black_background;
    }

    /// Put every setting back to its default and return the result.
    /// `config.json` is rewritten from scratch rather than patched like
    /// [`Config::save`] and `config.local.json` is removed, so keys the
    /// overlay doesn't own — anything hand-added — go too: a reset reads as
    /// if neither file had ever been edited. A layer that can't be parsed
    /// is copied to `<name>.bak` first, so the reset never destroys a hand
    /// edit with one typo in it.
    pub fn reset_to_defaults() -> std::io::Result<Self> {
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !guard_settings_writes() || CONFIG_PATH_OVERRIDE.with(|p| p.borrow().is_some()),
            "Config::reset_to_defaults() in a test without a path override: wrap \
             the test body in config::with_config_path (or with_default_config)"
        );
        Self::reset_layers(&settings_path(), &local_settings_path())
    }

    fn reset_layers(path: &Path, local: &Path) -> std::io::Result<Self> {
        let _lock = nebula_core::settings::lock(path)?;
        for layer in [path, local] {
            if strict_object(layer).is_err() {
                let mut bak = layer.as_os_str().to_owned();
                bak.push(".bak");
                std::fs::copy(layer, PathBuf::from(bak))?;
            }
        }
        match std::fs::remove_file(local) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        let cfg = Self::default();
        let mut layers = Layers::default();
        cfg.patch_layers(&mut layers, None);
        layers.commit(path, local, true)?;
        Ok(cfg)
    }

    /// Load the settings fresh, apply `change`, and write back only the
    /// top-level keys it changed — every other key, known or not, stays as
    /// the files hold it. The read and the write sit back to back, so a
    /// TUI and a desktop window saving different settings at the same time
    /// keep each other's values (a [`Config::save`] writes every known key
    /// from whatever it loaded, which is why the desktop goes through here).
    /// Returns the config as written.
    pub fn update(change: impl FnOnce(&mut Config)) -> std::io::Result<Config> {
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !guard_settings_writes() || CONFIG_PATH_OVERRIDE.with(|p| p.borrow().is_some()),
            "Config::update() in a test without a path override: wrap the              test body in config::with_config_path, or call update_in"
        );
        Self::update_in(&settings_path(), &local_settings_path(), change)
    }

    /// The two files [`Config::load`] reads, `(config.json,
    /// config.local.json)`, as this process resolves them (a test's path
    /// override included) — for a reader that watches them for changes.
    pub fn layer_paths() -> (PathBuf, PathBuf) {
        (settings_path(), local_settings_path())
    }

    /// [`Config::update`] on explicit layers: `path` with `local` over it.
    pub fn update_in(
        path: &Path,
        local: &Path,
        change: impl FnOnce(&mut Config),
    ) -> std::io::Result<Config> {
        Self::update_with_raw(path, local, change, None)
    }

    /// [`Config::update_in`], plus one top-level key `Config` has no field
    /// for (the window's `desktop` object) set from its stored value by
    /// `raw` — both in the same read and the same write, under one lock.
    pub fn update_in_with_raw(
        path: &Path,
        local: &Path,
        change: impl FnOnce(&mut Config),
        key: &str,
        raw: impl FnOnce(Option<&serde_json::Value>) -> std::io::Result<serde_json::Value>,
    ) -> std::io::Result<Config> {
        Self::update_with_raw(path, local, change, Some((key, Box::new(raw))))
    }

    #[allow(clippy::type_complexity)]
    fn update_with_raw<'a>(
        path: &Path,
        local: &Path,
        change: impl FnOnce(&mut Config),
        raw: Option<(
            &str,
            Box<dyn FnOnce(Option<&serde_json::Value>) -> std::io::Result<serde_json::Value> + 'a>,
        )>,
    ) -> std::io::Result<Config> {
        let _lock = nebula_core::settings::lock(path)?;
        let mut layers = Layers::read(path, local)?;
        let before = layers.config();
        let mut after = before.clone();
        change(&mut after);
        let old = serde_json::to_value(&before).map_err(invalid_data)?;
        let new = serde_json::to_value(&after).map_err(invalid_data)?;
        let changed: BTreeSet<String> = match (&old, &new) {
            (serde_json::Value::Object(old), serde_json::Value::Object(new)) => new
                .iter()
                .filter(|(key, value)| old.get(*key) != Some(*value))
                .map(|(key, _)| key.clone())
                .chain(old.keys().filter(|key| !new.contains_key(*key)).cloned())
                .collect(),
            _ => BTreeSet::new(),
        };
        if !changed.is_empty() {
            after.patch_layers(&mut layers, Some(&changed));
        }
        if let Some((key, raw)) = raw {
            let value = raw(layers.raw(key))?;
            layers.set_raw(key, value);
        }
        layers.commit(path, local, false)?;
        Ok(after)
    }

    /// The settings at `path` with `local` over them — [`Config::load`]
    /// on explicit layers, for a caller that is not this process's TUI.
    pub fn load_at(path: &Path, local: &Path) -> Config {
        load_layers(path, local)
    }

    /// Patch this config's known keys into `layers`: a key the local layer
    /// holds goes back into it, every other key into `config.json`'s
    /// object. `only` limits the patch to those keys ([`Config::update`]);
    /// a key in it the config no longer serializes (an emptied,
    /// skip-if-empty map) is removed from whichever layer holds it.
    fn patch_layers(&self, layers: &mut Layers, only: Option<&BTreeSet<String>>) {
        let root = &mut layers.root;
        let local_root = &mut layers.local;
        let serde_json::Value::Object(known) =
            serde_json::to_value(self).expect("Config serializes")
        else {
            unreachable!("Config serializes to a JSON object");
        };
        let defaults = serde_json::to_value(Self::default()).expect("Config serializes");
        for (old, new) in RENAMED_KEYS {
            let moved = root.remove(*old);
            // A partial write (`only`) may not rewrite the new key: carry the
            // old key's value over to it, so the rename loses nothing.
            if let (Some(value), Some(_)) = (moved, only) {
                root.entry(new.to_string()).or_insert(value);
            }
            // An old key the local layer holds stays local under its new
            // name, so the loop below writes the value back there.
            if let Some(held) = local_root.as_mut() {
                if let Some(value) = held.remove(*old) {
                    held.entry(new.to_string()).or_insert(value);
                }
            }
        }
        if let Some(only) = only {
            for key in only.iter().filter(|key| !known.contains_key(*key)) {
                root.remove(key);
                if let Some(held) = local_root.as_mut() {
                    held.remove(key);
                }
            }
        }
        for (key, value) in known {
            if only.is_some_and(|only| !only.contains(&key)) {
                continue;
            }
            // A stored value this build couldn't read loaded as its default.
            // Unless it has been changed since, leave the stored one alone.
            if self.skipped.contains(&key) && defaults.get(&key) == Some(&value) {
                continue;
            }
            match local_root.as_mut() {
                Some(held) if held.contains_key(&key) => {
                    held.insert(key, value);
                }
                _ => {
                    root.insert(key, value);
                }
            }
        }
    }

    /// `session_pane` as one of [`PANE_SIDES`]: the side the LAUNCHER
    /// VIEW lays its pane out on (the TUI's `PaneSide` by name).
    pub fn pane_side_word(&self) -> &'static str {
        crate::choices::pane_side_word(&self.session_pane)
    }

    /// `icons` as one of [`crate::choices::ICON_STYLES`]; anything else is
    /// `auto`.
    pub fn icons_word(&self) -> &'static str {
        let stored = self.icons.trim();
        crate::choices::ICON_STYLES
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(crate::choices::ICON_STYLES[0])
    }

    /// `kanban_scope` says every project rather than the one on screen.
    pub fn kanban_all(&self) -> bool {
        self.kanban_scope.trim().eq_ignore_ascii_case("all")
    }

    /// The DIFF LAYOUT `diff_mode` names; None while the file has not set
    /// one (or names one this build does not know), when the side-by-side
    /// switch an older build kept in state.json decides.
    pub fn diff_mode(&self) -> Option<crate::choices::DiffMode> {
        crate::choices::DiffMode::parse(&self.diff_mode)
    }

    /// `diff_context` as one of [`DIFF_CONTEXTS`].
    pub fn diff_context_word(&self) -> &'static str {
        let stored = self.diff_context.trim();
        DIFF_CONTEXTS
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(DIFF_CONTEXTS[0])
    }

    /// `critters_size` as one of [`CRITTER_SIZES`].
    pub fn critters_size_word(&self) -> &'static str {
        let stored = self.critters_size.trim();
        CRITTER_SIZES
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(CRITTER_SIZES[1])
    }

    /// `studio_idle_after` as one of [`STUDIO_IDLE_AFTERS`].
    pub fn studio_idle_after_word(&self) -> &'static str {
        let stored = self.studio_idle_after.trim();
        STUDIO_IDLE_AFTERS
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(DEFAULT_STUDIO_IDLE_AFTER)
    }

    /// How long the orchestrator must sit idle before an idle note.
    pub fn studio_idle_after(&self) -> std::time::Duration {
        nebula_core::settings::studio_idle_after(self.studio_idle_after_word())
    }

    /// Whether the usage ledger records anything new.
    pub fn usage_ledger(&self) -> bool {
        self.usage_ledger
    }

    /// `subagent_retention` as one of [`SUBAGENT_RETENTIONS`].
    pub fn subagent_retention_word(&self) -> &'static str {
        let stored = self.subagent_retention.trim();
        SUBAGENT_RETENTIONS
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(DEFAULT_SUBAGENT_RETENTION)
    }

    /// `critters_often` as one of [`CRITTER_VISITS`].
    pub fn critters_often_word(&self) -> &'static str {
        let stored = self.critters_often.trim();
        CRITTER_VISITS
            .iter()
            .copied()
            .find(|w| w.eq_ignore_ascii_case(stored))
            .unwrap_or(CRITTER_VISITS[1])
    }

    /// `worktree_layout` says the compact LIST rather than the cards.
    pub fn list_layout(&self) -> bool {
        self.worktree_layout_word() == "list"
    }

    /// `worktree_layout` says the NESTED layout rather than the cards.
    pub fn nested_layout(&self) -> bool {
        self.worktree_layout_word() == "nested"
    }

    /// `worktree_layout` says the three-column layout rather than the cards.
    pub fn columns_layout(&self) -> bool {
        self.worktree_layout_word() == "columns"
    }

    /// [`Self::orchestrator_view`] as one of [`ORCHESTRATOR_VIEWS`].
    pub fn orchestrator_view_word(&self) -> &'static str {
        let stored = self.orchestrator_view.trim();
        ORCHESTRATOR_VIEWS
            .iter()
            .copied()
            .find(|word| word.eq_ignore_ascii_case(stored))
            .unwrap_or(ORCHESTRATOR_VIEWS[0])
    }

    /// Whether the terminal app opens on the orchestrator's view, in a
    /// terminal `width` columns wide (None: unknown, never narrow).
    pub fn opens_on_orchestrator(&self, width: Option<u16>) -> bool {
        match self.orchestrator_view_word() {
            "always" => true,
            "never" => false,
            _ => width.is_some_and(|w| w < ORCHESTRATOR_NARROW_COLS),
        }
    }

    /// The **Worktree layout** in force, as one of [`WORKTREE_LAYOUTS`]:
    /// the stored word when it is on the list, else the cards.
    pub fn worktree_layout_word(&self) -> &'static str {
        let stored = self.worktree_layout.trim();
        WORKTREE_LAYOUTS
            .iter()
            .copied()
            .find(|word| word.eq_ignore_ascii_case(stored))
            .unwrap_or(WORKTREE_LAYOUTS[0])
    }

    pub fn inline_graphics_label(&self) -> &'static str {
        crate::choices::mode_label(&self.inline_graphics)
    }

    /// The editor the file overlays launch: `NEBULA_EDITOR` when set,
    /// otherwise the `editor` setting, otherwise vim.
    pub fn editor_command(&self) -> String {
        resolve_editor(
            nebula_core::env::non_empty(nebula_core::env::EDITOR).as_deref(),
            &self.editor,
        )
    }

    /// The effective registry this config reads: the compiled-in known
    /// harnesses with the `harnesses` map applied, then the legacy
    /// `custom_harnesses` list, then map-only new ids — plus, for
    /// built-ins, the legacy per-harness keys (`claude_model`,
    /// `codex_enabled`, …) wherever the map stays silent on that field.
    /// The map wins where both speak; the Agents tab writes the legacy
    /// keys for built-ins, so its edits apply without a migration.
    pub fn harness_registry(&self) -> Vec<HarnessDescriptor> {
        let mut all = nebula_core::harness::registry(&self.harnesses, &self.custom_harnesses);
        for entry in &mut all {
            if nebula_core::harness::builtin(&entry.id).is_none() {
                continue;
            }
            let over = self.harnesses.get(&entry.id);
            let (legacy_enabled, legacy_model, legacy_effort) =
                self.legacy_harness_fields(&entry.id);
            if over.and_then(|o| o.enabled).is_none() {
                if let Some(enabled) = legacy_enabled {
                    entry.enabled = enabled;
                }
            }
            if over.and_then(|o| o.model_default.clone()).is_none() {
                if let Some(default) = legacy_model {
                    entry.model.default = default;
                }
            }
            if over.and_then(|o| o.effort_default.clone()).is_none() {
                if let Some(default) = legacy_effort {
                    entry.effort.default = default;
                }
            }
        }
        all
    }

    /// The legacy per-harness keys for a built-in id, as
    /// `(enabled, model, effort)` — `Some` only where the file differs
    /// from the default, i.e. where the user said something. Newer than
    /// the table, older than the `harnesses` map.
    fn legacy_harness_fields(&self, id: &str) -> (Option<bool>, Option<String>, Option<String>) {
        let (enabled, model, effort) = match id {
            "claude" => (
                &self.claude_enabled,
                &self.claude_model,
                &self.claude_effort,
            ),
            "codex" => (&self.codex_enabled, &self.codex_model, &self.codex_effort),
            "cursor" => (
                &self.cursor_enabled,
                &self.cursor_model,
                &self.cursor_effort,
            ),
            "pi" => (&self.pi_enabled, &self.pi_model, &self.pi_effort),
            "muse" => (&self.muse_enabled, &self.muse_model, &self.muse_effort),
            // OpenCode has no effort key: nothing to fall back to there.
            "opencode" => {
                return (
                    (!self.opencode_enabled).then_some(false),
                    non_default(&self.opencode_model),
                    None,
                )
            }
            _ => return (None, None, None),
        };
        (
            (!enabled).then_some(false),
            non_default(model),
            non_default(effort),
        )
    }

    /// The effective built-in descriptor `kind` reads as, regardless of
    /// whether the entry is enabled or valid (the picker hides broken
    /// rows; reads degrade gracefully).
    fn builtin_descriptor(&self, kind: AgentKind) -> HarnessDescriptor {
        let id = kind.as_str();
        self.harness_registry()
            .into_iter()
            .find(|entry| entry.id == id)
            .or_else(|| nebula_core::harness::builtin(id))
            .expect("every built-in AgentKind describes")
    }

    /// The effective descriptor `(kind, custom)` reads as: the registry
    /// row, or a placeholder under its own name when the registry no
    /// longer names the id, so rows outliving their entry still render.
    pub fn effective_harness(&self, kind: AgentKind, custom: Option<&str>) -> HarnessDescriptor {
        let id = match kind {
            AgentKind::Custom => custom.unwrap_or_default().trim(),
            _ => kind.as_str(),
        };
        if let Some(descriptor) = self
            .harness_registry()
            .into_iter()
            .find(|entry| entry.id == id)
        {
            return descriptor;
        }
        CustomHarness {
            id: id.to_string(),
            label: String::new(),
            program: id.to_string(),
            enabled: true,
            model: DEFAULT_CHOICE.into(),
            model_flag: "--model".into(),
            hooks: None,
        }
        .as_descriptor()
    }

    /// The configured default model for new sessions of `kind`, as the
    /// daemon wants it: None = "default" = don't pass the flag.
    pub fn default_model(&self, kind: AgentKind) -> Option<String> {
        // Custom defaults resolve from the entry at the launch site,
        // where the id is known — never through this kind-only helper.
        if kind == AgentKind::Custom {
            return None;
        }
        self.builtin_descriptor(kind)
            .default_model()
            .map(str::to_string)
    }

    /// The configured default effort for new sessions of `kind`;
    /// None = "default" = don't pass the flag. A composing harness fits
    /// the effort against its configured family ([`fit_effort`]). A
    /// reserved effort (stored, like Muse's, but with no flag mapped yet)
    /// is None whatever the file holds — spawn would drop it anyway.
    pub fn default_effort(&self, kind: AgentKind) -> Option<String> {
        if kind == AgentKind::Custom {
            return None;
        }
        let descriptor = self.builtin_descriptor(kind);
        if descriptor.effort.flag.is_none()
            && descriptor.effort.config_key.is_none()
            && !descriptor.compose_model_effort
        {
            return None;
        }
        let model = descriptor.default_model().map(str::to_string);
        let effort = descriptor.default_effort().map(str::to_string);
        fit_effort_in(&descriptor, model.as_deref(), effort)
    }

    /// Whether the NEW SESSION PICKER offers `kind` at all.
    pub fn kind_enabled(&self, kind: AgentKind) -> bool {
        if kind == AgentKind::Custom {
            // A bare Custom kind is never enabled: entries gate themselves.
            return false;
        }
        self.builtin_descriptor(kind).enabled
    }

    /// The AGENT KINDS the picker lists, in `AgentKind::ALL` order. Empty
    /// only from a hand-edited config: the overlay refuses to switch off
    /// the last one.
    pub fn enabled_kinds(&self) -> Vec<AgentKind> {
        AgentKind::ALL
            .into_iter()
            .filter(|kind| self.kind_enabled(*kind))
            .collect()
    }

    /// Every harness the picker and presets offer, in registry order:
    /// `(kind, None)` for built-ins, `(Custom, Some(id))` for customs.
    /// Disabled and broken entries are out (broken ones surface in the
    /// Agents tab with their reason); under `hide_uninstalled_harnesses`
    /// so is anything whose program is missing from PATH. The daemon
    /// stays authoritative at launch.
    pub fn offered_harnesses(&self) -> Vec<(AgentKind, Option<String>)> {
        self.offered_harnesses_on(&std::env::var_os("PATH").unwrap_or_default())
    }

    /// [`Self::offered_harnesses`] with programs looked up on `path` in
    /// place of this process's PATH, which a test must never point
    /// elsewhere: every test thread spawning git reads it.
    pub fn offered_harnesses_on(&self, path: &std::ffi::OsStr) -> Vec<(AgentKind, Option<String>)> {
        let all = self.harness_registry();
        nebula_core::harness::usable(&all)
            .into_iter()
            .filter(|entry| {
                !self.hide_uninstalled_harnesses || program_installed_in(&entry.program, path)
            })
            .map(|entry| match AgentKind::parse(&entry.id) {
                Some(kind) => (kind, None),
                None => (AgentKind::Custom, Some(entry.id.clone())),
            })
            .collect()
    }

    /// Whether a harness may launch: its Agents tab switch, enabled and
    /// valid. The TUI asks it for a preset's harness.
    pub fn harness_usable(&self, kind: AgentKind, custom: Option<&str>) -> bool {
        let id = match kind {
            AgentKind::Custom => custom.unwrap_or_default().to_string(),
            _ => kind.as_str().to_string(),
        };
        self.harness_registry()
            .iter()
            .find(|entry| entry.id == id)
            .is_some_and(|entry| entry.enabled && entry.problem().is_none())
    }

    /// PRESET TEXT resolved: the side(s) of the task a new AGENT PRESET's
    /// text goes. `prefix` for a config that never set it or a hand edit
    /// off the list; `both` is taken for `prefix & postfix`.
    pub fn preset_text(&self) -> PresetText {
        PresetText::parse(&self.preset_text).unwrap_or(PresetText::DEFAULT)
    }

    /// The effective descriptor for a registry id, or a placeholder under
    /// its own name when the registry no longer names it, so rows
    /// outliving their entry still render.
    pub fn effective_harness_by_id(&self, id: &str) -> HarnessDescriptor {
        if let Some(descriptor) = self
            .harness_registry()
            .into_iter()
            .find(|entry| entry.id == id)
        {
            return descriptor;
        }
        CustomHarness {
            id: id.to_string(),
            label: String::new(),
            program: id.to_string(),
            enabled: true,
            model: DEFAULT_CHOICE.into(),
            model_flag: "--model".into(),
            hooks: None,
        }
        .as_descriptor()
    }

    /// `(id, field)` rows below the Agents head, in registry order: every
    /// entry, enabled or not, valid or not (broken rows show their reason
    /// so they can be fixed); Effort only while the harness offers effort.
    pub fn agent_rows(&self) -> Vec<(String, HarnessField)> {
        let mut rows = Vec::new();
        for entry in self.harness_registry() {
            rows.push((entry.id.clone(), HarnessField::Enabled));
            rows.push((entry.id.clone(), HarnessField::Model));
            if entry.effort.offered {
                rows.push((entry.id.clone(), HarnessField::Effort));
            }
        }
        rows
    }

    /// The harness row at a tab-local Agents index, or None while the
    /// index lands on the static head.
    pub fn agent_row(&self, index: usize) -> Option<(String, HarnessField)> {
        self.agent_rows()
            .into_iter()
            .nth(index.checked_sub(AGENTS_HEAD.len())?)
    }

    /// The value an Agents harness row shows.
    pub fn agent_value(&self, id: &str, field: HarnessField) -> String {
        let descriptor = self.effective_harness_by_id(id);
        match field {
            HarnessField::Enabled => on_off(descriptor.enabled).into(),
            HarnessField::Model => descriptor.model.default.clone(),
            HarnessField::Effort => {
                let choices = effort_choices_in(
                    &descriptor,
                    descriptor.default_model().map(str::to_string).as_deref(),
                );
                if choices.is_empty() {
                    "n/a".into()
                } else {
                    descriptor.effort.default.clone()
                }
            }
        }
    }

    /// The hint an Agents harness row shows: what the row edits, with the
    /// entry's problem appended while it is broken.
    pub fn agent_hint(&self, id: &str, field: HarnessField) -> String {
        let descriptor = self.effective_harness_by_id(id);
        let label = descriptor.display_label();
        let mut hint = match field {
            HarnessField::Enabled => format!(
                "Offer {label} in the New session picker (off hides it; existing sessions keep running)"
            ),
            HarnessField::Model => match descriptor.model.catalog {
                Some(nebula_core::harness::HarnessCatalog::Claude) => format!(
                    "Default model for new {label} sessions; rows follow Claude's availableModels or config.json claude_models"
                ),
                Some(nebula_core::harness::HarnessCatalog::Cursor) => format!(
                    "Default model family for new {label} sessions; rows follow cursor-agent --list-models"
                ),
                None => format!("Default model for new {label} sessions (default = CLI's pick)"),
            },
            HarnessField::Effort => {
                if descriptor.compose_model_effort {
                    format!(
                        "Effort (and -fast) variant of the chosen {label} model; n/a while it is default or auto"
                    )
                } else if let Some(flag) = descriptor.effort.flag.as_deref() {
                    format!("Default reasoning effort ({flag}) for new {label} sessions")
                } else if let (Some(flag), Some(key)) = (
                    descriptor.effort.config_flag.as_deref(),
                    descriptor.effort.config_key.as_deref(),
                ) {
                    format!("Default reasoning effort ({flag} {key}=) for new {label} sessions")
                } else {
                    format!("Reserved until the {label} CLI documents a reasoning flag (default = unset)")
                }
            }
        };
        if let Some(problem) = descriptor.problem() {
            hint.push_str(&format!(" broken: {problem}"));
        }
        hint
    }

    /// The hint for a tab-local Agents index: the head row's own hint, or
    /// the harness row's.
    pub fn agent_hint_by_index(&self, index: usize) -> String {
        if let Some(spec) = AGENTS_HEAD.get(index) {
            return spec.hint.to_string();
        }
        match self.agent_row(index) {
            Some((id, field)) => self.agent_hint(&id, field),
            None => String::new(),
        }
    }

    /// The model and effort a launch of `(kind, custom)` hands the daemon,
    /// given what a picker chose: an unpicked field (None) and the explicit
    /// "default" row both take the Agents tab's setting, whose own
    /// "default" means "no flag" and comes back None. The effort is fitted
    /// to the model (a Cursor effort only counts for its family). The TUI's
    /// `n` picker and the desktop window's launches resolve through here.
    pub fn launch_model_effort(
        &self,
        kind: AgentKind,
        custom: Option<&str>,
        model: Option<String>,
        effort: Option<String>,
    ) -> (Option<String>, Option<String>) {
        let resolve = |choice: Option<String>, configured: Option<String>| match choice {
            None => configured,
            Some(c) if c == DEFAULT_CHOICE => configured,
            some => some,
        };
        let harness = self.effective_harness(kind, custom);
        let model = resolve(model, harness.default_model().map(str::to_string));
        let effort = fit_effort_in(
            &harness,
            model.as_deref(),
            resolve(effort, harness.default_effort().map(str::to_string)),
        );
        (model, effort)
    }

    /// Set an Agents tab harness row to `value` — the desktop window's
    /// write, where [`Config::cycle_agent_row`] is the overlay's step:
    /// Enabled takes `"on"`/`"off"`, Model and Effort a choice the row
    /// offers (a composing harness refits its effort to a new model, as the
    /// cycle does). Refuses a row the registry does not name, an effort on
    /// a harness without one, and switching off the last usable harness.
    pub fn set_agent_value(
        &mut self,
        id: &str,
        field: HarnessField,
        value: &str,
    ) -> Result<(), String> {
        let Some(descriptor) = self
            .harness_registry()
            .into_iter()
            .find(|entry| entry.id == id)
        else {
            return Err(format!("no harness {id}"));
        };
        let value = value.trim();
        match field {
            HarnessField::Enabled => {
                let on = match value {
                    "on" | "true" => true,
                    "off" | "false" => false,
                    other => return Err(format!("Enabled is on or off, not {other:?}")),
                };
                // Writing what it already says changes nothing: no
                // override is created for an entry that had none.
                if on == descriptor.enabled {
                    return Ok(());
                }
                let mut next = self.clone();
                next.set_harness_enabled(id, on);
                if nebula_core::harness::usable(&next.harness_registry()).is_empty() {
                    return Err("keep at least one harness enabled".into());
                }
                *self = next;
            }
            HarnessField::Model => {
                if value.is_empty() {
                    return Err("a model is a name, or default".into());
                }
                if value == descriptor.model.default {
                    return Ok(());
                }
                self.set_harness_model(id, value.to_string());
                let descriptor = self.effective_harness_by_id(id);
                if descriptor.compose_model_effort {
                    let choices = effort_choices_in(&descriptor, Some(value));
                    if !fits(&descriptor.effort.default, &choices) {
                        let fitted = fit_effort_in(&descriptor, Some(value), None)
                            .unwrap_or_else(|| DEFAULT_CHOICE.into());
                        self.set_harness_effort(id, fitted);
                    }
                }
            }
            HarnessField::Effort => {
                let choices = effort_choices_in(
                    &descriptor,
                    descriptor.default_model().map(str::to_string).as_deref(),
                );
                if !fits(value, &choices) {
                    return Err(format!("{value:?} is not an effort {id} offers"));
                }
                let picked = choices
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(value))
                    .cloned()
                    .unwrap_or_else(|| value.to_string());
                if picked == descriptor.effort.default {
                    return Ok(());
                }
                self.set_harness_effort(id, picked);
            }
        }
        Ok(())
    }

    /// Cycle an Agents harness row: toggle Enabled, step Model / Effort
    /// through the rows the pickers offer. A composing harness refits its
    /// effort against the new model, so the row never holds an id the CLI
    /// would refuse.
    pub fn cycle_agent_row(&mut self, id: &str, field: HarnessField, delta: i32) {
        let Some(descriptor) = self
            .harness_registry()
            .into_iter()
            .find(|entry| entry.id == id)
        else {
            return;
        };
        let step = if delta == 0 { 1 } else { delta };
        match field {
            HarnessField::Enabled => self.set_harness_enabled(id, !descriptor.enabled),
            HarnessField::Model => {
                let choices = model_choices_in(&descriptor);
                let next = cycle_owned(&descriptor.model.default, &choices, step);
                self.set_harness_model(id, next.clone());
                let descriptor = self.effective_harness_by_id(id);
                if descriptor.compose_model_effort {
                    let choices = effort_choices_in(&descriptor, Some(&next));
                    if !fits(&descriptor.effort.default, &choices) {
                        let fitted = fit_effort_in(&descriptor, Some(&next), None)
                            .unwrap_or_else(|| DEFAULT_CHOICE.into());
                        self.set_harness_effort(id, fitted);
                    }
                }
            }
            HarnessField::Effort => {
                let choices = effort_choices_in(
                    &descriptor,
                    descriptor.default_model().map(str::to_string).as_deref(),
                );
                if choices.is_empty() {
                    return;
                }
                let next = cycle_owned(&descriptor.effort.default, &choices, step);
                self.set_harness_effort(id, next);
            }
        }
    }

    /// Write an Enabled toggle: the `harnesses` map where it speaks, else
    /// the legacy layer (the built-in switch, the list entry).
    #[doc(hidden)]
    pub fn set_harness_enabled(&mut self, id: &str, enabled: bool) {
        if self.harnesses.get(id).and_then(|o| o.enabled).is_some() {
            self.harness_override_mut(id).enabled = Some(enabled);
            return;
        }
        if nebula_core::harness::builtin(id).is_some() {
            self.set_legacy_enabled(id, enabled);
            return;
        }
        if let Some(entry) = self
            .custom_harnesses
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            entry.enabled = enabled;
            return;
        }
        self.harness_override_mut(id).enabled = Some(enabled);
    }

    /// Write a Model default: the map where it speaks, else the legacy
    /// layer (the built-in model key, the list entry's model).
    #[doc(hidden)]
    pub fn set_harness_model(&mut self, id: &str, model: String) {
        if self
            .harnesses
            .get(id)
            .and_then(|o| o.model_default.clone())
            .is_some()
        {
            self.harness_override_mut(id).model_default = Some(model);
            return;
        }
        if nebula_core::harness::builtin(id).is_some() {
            self.set_legacy_model(id, model);
            return;
        }
        if let Some(entry) = self
            .custom_harnesses
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            entry.model = model;
            return;
        }
        self.harness_override_mut(id).model_default = Some(model);
    }

    /// Write an Effort default: the map where it speaks, else the legacy
    /// built-in effort key (legacy list entries hold no effort; the map
    /// owns theirs).
    #[doc(hidden)]
    pub fn set_harness_effort(&mut self, id: &str, effort: String) {
        if nebula_core::harness::builtin(id).is_some()
            && self
                .harnesses
                .get(id)
                .and_then(|o| o.effort_default.clone())
                .is_none()
        {
            self.set_legacy_effort(id, effort);
            return;
        }
        self.harness_override_mut(id).effort_default = Some(effort);
    }

    /// The `harnesses` map entry for `id`, created when absent.
    fn harness_override_mut(&mut self, id: &str) -> &mut nebula_core::harness::HarnessOverride {
        self.harnesses.entry(id.to_string()).or_default()
    }

    fn set_legacy_enabled(&mut self, id: &str, enabled: bool) {
        match id {
            "claude" => self.claude_enabled = enabled,
            "codex" => self.codex_enabled = enabled,
            "cursor" => self.cursor_enabled = enabled,
            "pi" => self.pi_enabled = enabled,
            "muse" => self.muse_enabled = enabled,
            "opencode" => self.opencode_enabled = enabled,
            _ => self.harness_override_mut(id).enabled = Some(enabled),
        }
    }

    fn set_legacy_model(&mut self, id: &str, model: String) {
        match id {
            "claude" => self.claude_model = model,
            "codex" => self.codex_model = model,
            "cursor" => self.cursor_model = model,
            "pi" => self.pi_model = model,
            "muse" => self.muse_model = model,
            "opencode" => self.opencode_model = model,
            _ => self.harness_override_mut(id).model_default = Some(model),
        }
    }

    fn set_legacy_effort(&mut self, id: &str, effort: String) {
        match id {
            "claude" => self.claude_effort = effort,
            "codex" => self.codex_effort = effort,
            "cursor" => self.cursor_effort = effort,
            "pi" => self.pi_effort = effort,
            "muse" => self.muse_effort = effort,
            _ => self.harness_override_mut(id).effort_default = Some(effort),
        }
    }

    /// The AGENT KIND the QUICK PROMPT launches: the `quick_prompt_kind`
    /// setting, stepped on to the first enabled kind when that harness has
    /// been switched off on the AGENTS TAB since it was chosen (an
    /// unreadable name, or a hand-edited config with every harness off,
    /// reads as Claude — the same default the picker starts from).
    pub fn quick_prompt_kind(&self) -> AgentKind {
        let configured = AgentKind::parse(&self.quick_prompt_kind).unwrap_or_default();
        if self.kind_enabled(configured) {
            return configured;
        }
        self.enabled_kinds().first().copied().unwrap_or(configured)
    }

    /// The harness the NEW SESSION PICKER (and the PR SESSION picker)
    /// starts on: the last launch's while REMEMBER HARNESS is on — read
    /// through [`Config::quick_prompt_kind`], so one switched off since
    /// steps aside — and None, the first row, while it is off.
    pub fn remembered_kind(&self) -> Option<AgentKind> {
        self.remember_harness.then(|| self.quick_prompt_kind())
    }

    /// REMEMBER HARNESS (Settings → Experimental): make `kind` — and a
    /// model or effort a picker chose for it, `None` for one it did not —
    /// the defaults the next launch starts from, by writing the AGENTS
    /// TAB's own rows: `quick_prompt_kind`, and that harness's Model /
    /// Effort (the registry entry's, keyed by the kind name or the
    /// custom id). An explicit `"default"` pick lands as the row's own
    /// `default`. The QUICK PROMPT names only built-in kinds, so a custom
    /// entry is remembered by its Model / Effort rows alone. Returns
    /// whether anything changed, so the caller saves only then; nothing
    /// moves while the switch is off.
    pub fn remember_launch(
        &mut self,
        kind: AgentKind,
        custom: Option<&str>,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> bool {
        if !self.remember_harness {
            return false;
        }
        let id = match (kind, custom) {
            (AgentKind::Custom, Some(id)) => id.to_string(),
            (AgentKind::Custom, None) => return false,
            (kind, _) => kind.as_str().to_string(),
        };
        let mut changed = false;
        if kind != AgentKind::Custom && self.quick_prompt_kind != kind.as_str() {
            self.quick_prompt_kind = kind.as_str().into();
            changed = true;
        }
        let before = self.effective_harness_by_id(&id);
        if let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) {
            if before.model.default != model {
                self.set_harness_model(&id, model.into());
                changed = true;
            }
        }
        if let Some(effort) = effort.map(str::trim).filter(|e| !e.is_empty()) {
            if before.effort.default != effort {
                self.set_harness_effort(&id, effort.into());
                changed = true;
            }
        }
        // A composing harness (Cursor's family-suffix shape) keeps the
        // stored effort only if the family it now names ships it, as the
        // AGENTS TAB's own Model cycle does — never an id the CLI would
        // refuse.
        if changed {
            let descriptor = self.effective_harness_by_id(&id);
            if descriptor.compose_model_effort {
                let family = descriptor.default_model().map(str::to_string);
                let choices = effort_choices_in(&descriptor, family.as_deref());
                if !fits(&descriptor.effort.default, &choices) {
                    let fitted = fit_effort_in(&descriptor, family.as_deref(), None)
                        .unwrap_or_else(|| DEFAULT_CHOICE.into());
                    self.set_harness_effort(&id, fitted);
                }
            }
        }
        changed
    }

    /// The harnesses the PROJECT TAB offers a **Default account** row
    /// for: each one a usable PROFILE is for, in the order the profiles
    /// list them.
    pub fn project_account_harnesses(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.profiles.iter().filter(|p| p.problem().is_none()) {
            let h = p.harness.trim().to_string();
            if !out.contains(&h) {
                out.push(h);
            }
        }
        out
    }

    /// The harness a PROJECT TAB row past the declared ones is the
    /// **Default account** row of; None for a declared row or none at all.
    pub fn project_account_row(&self, index: usize) -> Option<String> {
        let declared = tab_settings(project_tab()).len();
        index
            .checked_sub(declared)
            .and_then(|i| self.project_account_harnesses().into_iter().nth(i))
    }

    /// The settings of the project checked out at `repo_path`: its
    /// `projects` entry, else the defaults every project without one gets.
    pub fn project(&self, repo_path: &Path) -> ProjectSettings {
        self.projects.get(repo_path).cloned().unwrap_or_default()
    }

    /// Store `settings` as the entry of the project at `repo_path`. An
    /// entry that says nothing the defaults don't is dropped rather than
    /// written, so `projects` names only the projects set up differently
    /// — and a project turned back to match the rest leaves no trace.
    pub fn set_project(&mut self, repo_path: &Path, settings: ProjectSettings) {
        if settings == ProjectSettings::default() {
            self.projects.remove(repo_path);
        } else {
            self.projects.insert(repo_path.to_path_buf(), settings);
        }
    }

    /// Move a project's stored settings to its new repo path. Returns true
    /// only when there was an entry to move.
    pub fn rekey_project(&mut self, old_path: &Path, new_path: &Path) -> bool {
        let Some(settings) = self.projects.remove(old_path) else {
            return false;
        };
        self.set_project(new_path, settings);
        true
    }

    /// The stored text of a typed PROJECT TAB row for the project at
    /// `repo_path` — what [`Config::text_value`] is for a top-level row.
    pub fn project_text_value(&self, repo_path: &Path, kind: SettingKind) -> String {
        self.project(repo_path).text_value(kind)
    }

    /// Write a typed PROJECT TAB row for the project at `repo_path` — what
    /// [`Config::set_text`] is for a top-level row. False for a row that
    /// is not a typed project row.
    pub fn set_project_text(&mut self, repo_path: &Path, kind: SettingKind, value: &str) -> bool {
        let mut settings = self.project(repo_path);
        if !settings.set_text(kind, value) {
            return false;
        }
        self.set_project(repo_path, settings);
        true
    }

    /// The overlay's label for `kind`. A PROJECT TAB row read here shows
    /// the fallback — the overlay reads the selected project's through
    /// [`Config::project`] instead.
    pub fn value_label(&self, kind: SettingKind) -> String {
        match kind {
            SettingKind::PaletteEnterAttaches => on_off(self.palette_enter_attaches).into(),
            SettingKind::WorktreeBaseBranch => match self.worktree_base_branch.trim() {
                "" => AUTO_CHOICE.into(),
                name => name.to_string(),
            },
            SettingKind::Editor => self.editor.clone(),
            SettingKind::CloseFinderOnOpen => on_off(self.close_finder_on_open).into(),
            SettingKind::SshSyncConfig => on_off(self.ssh_sync_config).into(),
            SettingKind::SessionIdleTimeout => self.session_idle_timeout.clone(),
            SettingKind::PrewarmAgents => on_off(self.prewarm_agents).into(),
            SettingKind::PrewarmSessions => on_off(self.prewarm_sessions).into(),
            SettingKind::DoneSound => self.done_sound.clone(),
            SettingKind::FeedbackSound => self.feedback_sound.clone(),
            SettingKind::PresetText => self.preset_text().as_str().into(),
            SettingKind::DeleteEmptyWorktree => on_off(self.delete_empty_worktree).into(),
            SettingKind::AutoCleanupMerged => on_off(self.auto_cleanup_merged).into(),
            SettingKind::ShowAllWorktrees => on_off(self.show_all_worktrees).into(),
            SettingKind::AskBeforeArchive => on_off(self.ask_before_archive).into(),
            SettingKind::AccountSpread => self.account_spread.clone(),
            SettingKind::AccountSpreadThreshold => self.account_spread_threshold.clone(),
            SettingKind::UsageStatusline => on_off(self.usage_statusline).into(),
            SettingKind::Theme => self.theme.clone(),
            SettingKind::Icons => self.icons_word().into(),
            SettingKind::Animations => on_off(self.animations).into(),
            SettingKind::BlackBackground => on_off(self.black_background).into(),
            SettingKind::HideCardMarks => shown_hidden(self.hide_card_marks).into(),
            SettingKind::HighlightCurrentCard => on_off(self.highlight_current_card).into(),
            SettingKind::SessionPane => self.pane_side_word().into(),
            SettingKind::WorktreeLayout => self.worktree_layout_word().into(),
            SettingKind::Kanban => on_off(self.kanban).into(),
            SettingKind::KanbanScope => {
                if self.kanban_all() {
                    "all"
                } else {
                    "project"
                }
            }
            .into(),
            SettingKind::OrchestratorView => self.orchestrator_view_word().into(),
            SettingKind::InlineGraphics => self.inline_graphics_label().into(),
            SettingKind::ExpandAllWorktrees => on_off(self.expand_all_worktrees).into(),
            SettingKind::CardIssueNumber => on_off(self.card_issue_number).into(),
            SettingKind::HideDraftPrs => shown_hidden(self.hide_draft_prs).into(),
            SettingKind::SourceControlGraph => on_off(self.source_control_graph).into(),
            SettingKind::DiffMode => self.diff_mode().unwrap_or_default().as_str().into(),
            SettingKind::DiffContext => self.diff_context_word().into(),
            // A project row with no project to speak of: what one without
            // an entry would show.
            SettingKind::RunCommand | SettingKind::OpenCommand => {
                ProjectSettings::default().value_label(kind)
            }
            SettingKind::RememberHarness => on_off(self.remember_harness).into(),
            SettingKind::HideUninstalledHarnesses => on_off(self.hide_uninstalled_harnesses).into(),
            SettingKind::QuickPromptKind => self.quick_prompt_kind.clone(),
            SettingKind::QuickPromptFocus => on_off(self.quick_prompt_focus).into(),
            SettingKind::FollowNewSession => on_off(self.follow_new_session).into(),
            SettingKind::ConfirmDragMove => on_off(self.confirm_drag_move).into(),
            SettingKind::ShowTips => on_off(self.show_tips).into(),
            SettingKind::QuickPromptNewWorktree => on_off(self.quick_prompt_new_worktree).into(),
            SettingKind::Critters => on_off(self.critters).into(),
            SettingKind::CrittersSize => self.critters_size_word().into(),
            SettingKind::CrittersOften => self.critters_often_word().into(),
            SettingKind::HideDoneSubagents => shown_hidden(self.hide_done_subagents).into(),
            SettingKind::SubagentRetention => self.subagent_retention_word().into(),
            SettingKind::StudioIdleAfter => self.studio_idle_after_word().into(),
            SettingKind::UsageLedger => on_off(self.usage_ledger).into(),
        }
    }

    /// `delta == 0` means activate (toggle a bool, cycle a choice forward).
    /// Non-zero delta cycles a choice; bools still toggle. `index` is
    /// tab-local — the Hotkeys tab has no cyclable values and no-ops here,
    /// and so does a PROJECT TAB row, which is typed for one project
    /// ([`Config::set_project_text`]). The Agents tab resolves its head rows
    /// statically and its harness rows through the registry.
    pub fn cycle(&mut self, tab: usize, index: usize, delta: i32) {
        if tab == agents_tab() {
            if let Some(spec) = AGENTS_HEAD.get(index) {
                self.cycle_kind(spec.kind, delta);
            } else if let Some((id, field)) = self.agent_row(index) {
                self.cycle_agent_row(&id, field, delta);
            }
            return;
        }
        let Some(spec) = setting_at(tab, index) else {
            return;
        };
        self.cycle_kind(spec.kind, delta);
    }

    /// Cycle one static setting row. The Agents tab's harness rows cycle
    /// through [`Config::cycle_agent_row`] instead.
    fn cycle_kind(&mut self, kind: SettingKind, delta: i32) {
        let step = if delta == 0 { 1 } else { delta };
        match kind {
            SettingKind::PaletteEnterAttaches => {
                self.palette_enter_attaches = !self.palette_enter_attaches;
            }
            // Typed, not cycled: see `SettingKind::is_text` / `set_text`.
            SettingKind::WorktreeBaseBranch => {}
            SettingKind::Editor => {
                self.editor = cycle_choice(&self.editor, EDITORS, step).into();
            }
            SettingKind::CloseFinderOnOpen => {
                self.close_finder_on_open = !self.close_finder_on_open;
            }
            SettingKind::SshSyncConfig => {
                self.ssh_sync_config = !self.ssh_sync_config;
            }
            SettingKind::SessionIdleTimeout => {
                self.session_idle_timeout =
                    cycle_choice(&self.session_idle_timeout, SESSION_IDLE_TIMEOUTS, step).into();
            }
            SettingKind::PrewarmAgents => {
                self.prewarm_agents = !self.prewarm_agents;
            }
            SettingKind::PrewarmSessions => {
                self.prewarm_sessions = !self.prewarm_sessions;
            }
            SettingKind::DoneSound => {
                self.done_sound = cycle_choice(&self.done_sound, SOUNDS, step).into();
            }
            SettingKind::FeedbackSound => {
                self.feedback_sound = cycle_choice(&self.feedback_sound, SOUNDS, step).into();
            }
            SettingKind::PresetText => {
                // Cycled from the resolved side, so a hand edit off the
                // list steps on from the default it reads as.
                self.preset_text =
                    cycle_choice(self.preset_text().as_str(), PRESET_TEXTS, step).into();
            }
            SettingKind::DeleteEmptyWorktree => {
                self.delete_empty_worktree = !self.delete_empty_worktree;
            }
            SettingKind::AutoCleanupMerged => {
                self.auto_cleanup_merged = !self.auto_cleanup_merged;
            }
            SettingKind::ShowAllWorktrees => {
                self.show_all_worktrees = !self.show_all_worktrees;
            }
            SettingKind::AskBeforeArchive => {
                self.ask_before_archive = !self.ask_before_archive;
            }
            SettingKind::Icons => {
                self.icons =
                    cycle_choice(self.icons_word(), crate::choices::ICON_STYLES, step).into();
            }
            SettingKind::Theme => {
                self.theme = cycle_choice(&self.theme, crate::choices::THEMES, step).into();
            }
            SettingKind::Animations => {
                self.animations = !self.animations;
            }
            SettingKind::BlackBackground => {
                self.black_background = !self.black_background;
            }
            SettingKind::HideCardMarks => {
                self.hide_card_marks = !self.hide_card_marks;
            }
            SettingKind::HighlightCurrentCard => {
                self.highlight_current_card = !self.highlight_current_card;
            }
            SettingKind::SessionPane => {
                // Cycled from the resolved side, so a hand edit off the
                // list steps on from the right it reads as.
                self.session_pane = cycle_choice(self.pane_side_word(), PANE_SIDES, step).into();
            }
            SettingKind::Kanban => {
                self.kanban = !self.kanban;
            }
            SettingKind::KanbanScope => {
                self.kanban_scope = if self.kanban_all() { "project" } else { "all" }.into();
            }
            SettingKind::OrchestratorView => {
                self.orchestrator_view =
                    cycle_choice(self.orchestrator_view_word(), ORCHESTRATOR_VIEWS, step).into();
            }
            SettingKind::WorktreeLayout => {
                let now = self.worktree_layout_word();
                self.worktree_layout = cycle_choice(now, WORKTREE_LAYOUTS, step).into();
            }
            SettingKind::InlineGraphics => {
                self.inline_graphics =
                    cycle_choice(self.inline_graphics_label(), INLINE_GRAPHICS, step).into();
            }
            SettingKind::ExpandAllWorktrees => {
                self.expand_all_worktrees = !self.expand_all_worktrees;
            }
            SettingKind::CardIssueNumber => {
                self.card_issue_number = !self.card_issue_number;
            }
            SettingKind::HideDraftPrs => {
                self.hide_draft_prs = !self.hide_draft_prs;
            }
            SettingKind::SourceControlGraph => {
                self.source_control_graph = !self.source_control_graph;
            }
            SettingKind::DiffMode => {
                let modes = crate::choices::DiffMode::ALL.map(|m| m.as_str());
                let now = self.diff_mode().unwrap_or_default().as_str();
                self.diff_mode = cycle_choice(now, &modes, step).into();
            }
            SettingKind::DiffContext => {
                let now = self.diff_context_word();
                self.diff_context = cycle_choice(now, DIFF_CONTEXTS, step).into();
            }
            // One project's, not the file's, and typed: see `set_project_text`.
            SettingKind::RunCommand | SettingKind::OpenCommand => {}
            SettingKind::RememberHarness => {
                self.remember_harness = !self.remember_harness;
            }
            SettingKind::HideUninstalledHarnesses => {
                self.hide_uninstalled_harnesses = !self.hide_uninstalled_harnesses;
            }
            SettingKind::QuickPromptKind => {
                self.quick_prompt_kind =
                    cycle_owned(&self.quick_prompt_kind, &agent_kind_names(), step);
            }
            SettingKind::QuickPromptFocus => {
                self.quick_prompt_focus = !self.quick_prompt_focus;
            }
            SettingKind::FollowNewSession => {
                self.follow_new_session = !self.follow_new_session;
            }
            SettingKind::ConfirmDragMove => {
                self.confirm_drag_move = !self.confirm_drag_move;
            }
            SettingKind::ShowTips => {
                self.show_tips = !self.show_tips;
            }
            SettingKind::QuickPromptNewWorktree => {
                self.quick_prompt_new_worktree = !self.quick_prompt_new_worktree;
            }
            SettingKind::AccountSpread => {
                self.account_spread =
                    cycle_choice(&self.account_spread, nebula_core::accounts::SPREADS, step).into();
            }
            SettingKind::AccountSpreadThreshold => {
                self.account_spread_threshold = cycle_choice(
                    &self.account_spread_threshold,
                    nebula_core::accounts::THRESHOLDS,
                    step,
                )
                .into();
            }
            SettingKind::UsageStatusline => {
                self.usage_statusline = !self.usage_statusline;
            }
            SettingKind::Critters => {
                self.critters = !self.critters;
            }
            SettingKind::CrittersSize => {
                let now = self.critters_size_word();
                self.critters_size = cycle_choice(now, CRITTER_SIZES, step).into();
            }
            SettingKind::CrittersOften => {
                let now = self.critters_often_word();
                self.critters_often = cycle_choice(now, CRITTER_VISITS, step).into();
            }
            SettingKind::HideDoneSubagents => {
                self.hide_done_subagents = !self.hide_done_subagents;
            }
            SettingKind::SubagentRetention => {
                let now = self.subagent_retention_word();
                self.subagent_retention = cycle_choice(now, SUBAGENT_RETENTIONS, step).into();
            }
            SettingKind::StudioIdleAfter => {
                let now = self.studio_idle_after_word();
                self.studio_idle_after = cycle_choice(now, STUDIO_IDLE_AFTERS, step).into();
            }
            SettingKind::UsageLedger => {
                self.usage_ledger = !self.usage_ledger;
            }
        }
    }

    /// The stored text of a typed row ([`SettingKind::is_text`]) as the
    /// prompt should pre-fill it — `""` for an unset row, never the `auto`
    /// the overlay shows in its place. Empty for a row that is not typed.
    pub fn text_value(&self, kind: SettingKind) -> String {
        match kind {
            SettingKind::WorktreeBaseBranch => self.worktree_base_branch.clone(),
            _ => String::new(),
        }
    }

    /// Write a typed value into a text row ([`SettingKind::is_text`]),
    /// trimmed. Empty puts the row back on its default (`auto`). False for
    /// a row that is not typed — nothing changes.
    pub fn set_text(&mut self, kind: SettingKind, value: &str) -> bool {
        match kind {
            SettingKind::WorktreeBaseBranch => {
                self.worktree_base_branch = value.trim().to_string();
                true
            }
            _ => false,
        }
    }
}

/// What the TUI plays for a status edge — the `done_sound` or
/// `feedback_sound` SETTING resolved against where the TUI is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    /// The terminal BEL (`\x07`), written through the attached terminal,
    /// which decides whether that is a sound, a flash, or a dock bounce.
    Bell,
    /// A sound file to hand to `afplay`.
    File(PathBuf),
}

impl Config {
    /// The sound to play for a finish, or `None` for silence. A named
    /// system sound only resolves to its file on macOS, on a local
    /// terminal, and when the file exists — over ssh `afplay` would ring
    /// the *remote* box, so the bell stands in there, as it does off
    /// macOS and for a name the sound folder doesn't hold.
    pub fn done_sound(&self) -> Option<Sound> {
        resolve_sound(
            &self.done_sound,
            nebula_core::host::is_remote_session(),
            cfg!(target_os = "macos"),
        )
    }

    /// The sound to play when a turn stops to ask the user, or `None` for
    /// silence — which also stands down the desktop notification, since
    /// `feedback_sound` is the one switch for both. Same fallbacks as
    /// [`Config::done_sound`].
    pub fn feedback_sound(&self) -> Option<Sound> {
        resolve_sound(
            &self.feedback_sound,
            nebula_core::host::is_remote_session(),
            cfg!(target_os = "macos"),
        )
    }
}

#[doc(hidden)]
pub fn resolve_sound(configured: &str, remote: bool, macos: bool) -> Option<Sound> {
    let name = configured.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("off") {
        return None;
    }
    if name.eq_ignore_ascii_case("bell") || remote || !macos {
        return Some(Sound::Bell);
    }
    // A sound name is a bare file stem; anything else (a path, a dot) is
    // not one, and the bell covers the typo.
    if !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(Sound::Bell);
    }
    let path = Path::new(MACOS_SOUNDS_DIR).join(format!("{name}.aiff"));
    if path.is_file() {
        Some(Sound::File(path))
    } else {
        Some(Sound::Bell)
    }
}

/// First non-blank of env override → configured value → vim.
#[doc(hidden)]
pub fn resolve_editor(env: Option<&str>, configured: &str) -> String {
    for value in [env.unwrap_or(""), configured] {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "vim".into()
}

/// Whether `kind`'s CLI resolves on this process's PATH right now. A fast
/// synchronous check for picker filtering only; the daemon re-probes
/// through the login shell at launch, which can see shims PATH misses.
/// Whether `program` resolves on this process's PATH right now — the
/// fast check behind `hide_uninstalled_harnesses`, for built-ins and
/// customs alike.
pub fn program_installed(program: &str) -> bool {
    program_installed_in(program, &std::env::var_os("PATH").unwrap_or_default())
}

/// [`program_installed`] on `path`, a PATH-style list of directories.
pub fn program_installed_in(program: &str, path: &std::ffi::OsStr) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }
    // Windows: `claude` is `claude.exe` or `claude.cmd` on disk; a bare
    // join never finds it, so look the way a spawn does (PATH + PATHEXT).
    #[cfg(windows)]
    {
        let pathext =
            nebula_core::env::non_empty("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        nebula_core::shell::resolve_program_in(program, path, &pathext).is_some()
    }
    #[cfg(not(windows))]
    {
        for dir in std::env::split_paths(path) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return true;
            }
        }
        false
    }
}

/// [`DEFAULT_CHOICE`] (or blank) → None; anything else passes through.
pub fn non_default(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case(DEFAULT_CHOICE)).then(|| value.to_string())
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn shown_hidden(hidden: bool) -> &'static str {
    if hidden {
        "hidden"
    } else {
        "shown"
    }
}

pub fn cycle_choice<'a>(current: &str, choices: &[&'a str], delta: i32) -> &'a str {
    choices[cycled_index(current, choices, delta)]
}

/// The two settings layers merged, `local` over `path`. What this build
/// can't read is logged and remembered in [`Config::skipped`], so a save
/// leaves it as stored.
fn load_layers(path: &Path, local: &Path) -> Config {
    let loaded = nebula_core::settings::load::<Config>(path, local);
    for problem in &loaded.problems {
        tracing::warn!("{problem}");
    }
    if !loaded.skipped.is_empty() {
        tracing::warn!(keys = ?loaded.skipped, "settings this build can't read keep their defaults");
    }
    Config {
        skipped: loaded.skipped,
        ..loaded.value
    }
}

/// Test shorthand: the settings at `path`, local layer beside it.
#[cfg(any(test, feature = "test-support"))]
pub fn load_from(path: &Path) -> Config {
    load_layers(path, &sibling_local_path(path))
}

/// The two settings layers as one writer read them, under the lock: what
/// each file held, and the objects being patched. A file is written back
/// only when its object changed.
#[derive(Default)]
struct Layers {
    root: nebula_core::settings::Object,
    root_read: nebula_core::settings::Object,
    /// None while there is no `config.local.json`.
    local: Option<nebula_core::settings::Object>,
    local_read: Option<nebula_core::settings::Object>,
}

impl Layers {
    /// Both layers, once. A file that is there but isn't a JSON object
    /// (malformed, a trailing comma, a list) is an error: a writer that went
    /// on would replace the user's file with defaults.
    fn read(path: &Path, local: &Path) -> std::io::Result<Layers> {
        let root = strict_object(path)?;
        let local = local.exists().then(|| strict_object(local)).transpose()?;
        Ok(Layers {
            root_read: root.clone(),
            root,
            local_read: local.clone(),
            local,
        })
    }

    /// The config the layers hold, as [`Config::load`] reads it.
    fn config(&self) -> Config {
        let mut merged = self.root.clone();
        if let Some(local) = &self.local {
            merged.extend(local.clone());
        }
        let (value, skipped) = nebula_core::settings::parse_lenient::<Config>(&merged);
        if !skipped.is_empty() {
            tracing::warn!(keys = ?skipped, "settings this build can't read keep their defaults");
        }
        Config { skipped, ..value }
    }

    /// One raw top-level key, the local layer's over `config.json`'s.
    fn raw(&self, key: &str) -> Option<&serde_json::Value> {
        self.local
            .as_ref()
            .and_then(|held| held.get(key))
            .or_else(|| self.root.get(key))
    }

    /// Set one raw key where it lives: the local layer when that holds it,
    /// else `config.json`.
    fn set_raw(&mut self, key: &str, value: serde_json::Value) {
        match self.local.as_mut() {
            Some(held) if held.contains_key(key) => {
                held.insert(key.to_string(), value);
            }
            _ => {
                self.root.insert(key.to_string(), value);
            }
        }
    }

    /// Write each layer whose object changed (`config.json` always when
    /// `force_root`), atomically.
    fn commit(self, path: &Path, local: &Path, force_root: bool) -> std::io::Result<()> {
        use nebula_core::settings;
        if force_root || self.root != self.root_read {
            settings::write_json(path, &serde_json::Value::Object(self.root))?;
        }
        if let Some(held) = self
            .local
            .filter(|held| Some(held) != self.local_read.as_ref())
        {
            settings::write_json(local, &serde_json::Value::Object(held))?;
        }
        Ok(())
    }
}

/// The object a settings file holds: empty when there is no file, an error
/// naming the file when it is there and can't be read or parsed as one.
fn strict_object(path: &Path) -> std::io::Result<nebula_core::settings::Object> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(err) => return Err(err),
    };
    let refuse = |why: String| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "{} can't be parsed ({why}); nothing was written. Fix or remove the file",
                path.display()
            ),
        )
    };
    match serde_json::from_str(nebula_core::settings::strip_bom(&raw)) {
        Ok(serde_json::Value::Object(obj)) => Ok(obj),
        Ok(_) => Err(refuse("not a JSON object".into())),
        Err(err) => Err(refuse(err.to_string())),
    }
}

/// Why a write to the layers at `path` / `local` would be refused right
/// now — a file that can't be parsed — for a window to show before the user
/// edits anything. None when both read.
pub fn write_blocker(path: &Path, local: &Path) -> Option<String> {
    Layers::read(path, local).err().map(|err| err.to_string())
}

fn invalid_data(err: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

fn settings_path() -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    {
        if let Some(path) = CONFIG_PATH_OVERRIDE.with(|p| p.borrow().clone()) {
            return path;
        }
    }
    nebula_core::paths::config_path()
}

/// The local layer. A test's path override moves it too, beside the
/// overriding file, so no test reads the dev's own.
fn local_settings_path() -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
    {
        if let Some(path) = CONFIG_PATH_OVERRIDE.with(|p| p.borrow().clone()) {
            return sibling_local_path(&path);
        }
    }
    nebula_core::paths::config_local_path()
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static CONFIG_PATH_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
pub fn with_config_path<T>(path: PathBuf, f: impl FnOnce() -> T) -> T {
    CONFIG_PATH_OVERRIDE.with(|slot| {
        let prev = slot.replace(Some(path));
        let out = f();
        slot.replace(prev);
        out
    })
}

/// Test-only accessors: nothing in the app reads these any more.
#[cfg(any(test, feature = "test-support"))]
impl Config {
    /// [`Config::save`] into `path`, with its local layer beside it.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        self.save_layers(path, &sibling_local_path(path))
    }
}

#[cfg(any(test, feature = "test-support"))]
/// `config.local.json` beside `path`: the local layer of a settings file
/// that is not this nebula's own — a test's, or [`Config::save_to`]'s.
pub fn sibling_local_path(path: &Path) -> PathBuf {
    path.with_file_name("config.local.json")
}

#[cfg(any(test, feature = "test-support"))]
/// Where a static setting lives, as `(tab, row)`. The overlay addresses
/// settings by position, so anything that wants to talk about one by name
/// — tests, and anything that ever jumps the cursor to a named setting —
/// goes through here rather than hardcoding an index. Harness rows locate
/// through [`locate_agent`].
pub fn locate(kind: SettingKind) -> Option<(usize, usize)> {
    SETTINGS_TABS.iter().enumerate().find_map(|(t, tab)| {
        match tab.body {
            TabBody::Values(settings) | TabBody::Project(settings) => {
                settings.iter().position(|s| s.kind == kind)
            }
            TabBody::Agents => AGENTS_HEAD.iter().position(|s| s.kind == kind),
            TabBody::Hotkeys => None,
        }
        .map(|i| (t, i))
    })
}

/// The PROJECT TAB's header over its **Default account** rows.
pub const PROJECT_ACCOUNTS_HEADER: &str = "Default account";

/// Index of the Project tab, whose rows are the selected project's.
pub fn project_tab() -> usize {
    SETTINGS_TABS
        .iter()
        .position(|t| matches!(t.body, TabBody::Project(_)))
        .expect("SETTINGS_TABS declares a Project tab")
}

/// The two settings layers this process reads: `config.json`, and
/// `config.local.json` over it (a test's path override moves both).
pub fn layer_paths() -> (PathBuf, PathBuf) {
    (settings_path(), local_settings_path())
}

/// Write one top-level key that [`Config`] has no field for — the desktop
/// window's own `desktop` object — leaving every other key as the files
/// hold it: into `config.local.json` when that layer already holds the key,
/// else into `config.json`. Read, patch and atomic write sit back to back.
pub fn patch_raw_key(
    path: &Path,
    local: &Path,
    key: &str,
    value: serde_json::Value,
) -> std::io::Result<()> {
    update_raw_key(path, local, key, |_| Ok(value)).map(|_| ())
}

/// Set one raw top-level key from its stored value (None when neither
/// layer holds it) in one locked read and write, as [`patch_raw_key`]
/// places it. Returns the value written.
pub fn update_raw_key(
    path: &Path,
    local: &Path,
    key: &str,
    change: impl FnOnce(Option<&serde_json::Value>) -> std::io::Result<serde_json::Value>,
) -> std::io::Result<serde_json::Value> {
    let _lock = nebula_core::settings::lock(path)?;
    let mut layers = Layers::read(path, local)?;
    let value = change(layers.raw(key))?;
    layers.set_raw(key, value.clone());
    layers.commit(path, local, false)?;
    Ok(value)
}

/// One top-level key out of the merged layers, `local` over `path`, raw.
pub fn read_raw_key(path: &Path, local: &Path, key: &str) -> Option<serde_json::Value> {
    use nebula_core::settings;
    [local, path].into_iter().find_map(|layer| {
        settings::read_object(layer)
            .ok()
            .flatten()
            .and_then(|mut obj| obj.remove(key))
    })
}

#[cfg(test)]
mod orchestrator_view_tests {
    use super::*;

    #[test]
    fn auto_is_left_out_of_a_save_and_the_other_words_are_kept() {
        let auto = serde_json::to_value(Config::default()).unwrap();
        assert!(auto.get("orchestrator_view").is_none());
        let always = serde_json::to_value(Config {
            orchestrator_view: "always".into(),
            ..Config::default()
        })
        .unwrap();
        assert_eq!(always["orchestrator_view"], "always");
        let back: Config = serde_json::from_value(always).unwrap();
        assert_eq!(back.orchestrator_view_word(), "always");
    }

    #[test]
    fn auto_means_a_narrow_terminal() {
        let cfg = |w: &str| Config {
            orchestrator_view: w.into(),
            ..Config::default()
        };
        assert!(cfg("auto").opens_on_orchestrator(Some(ORCHESTRATOR_NARROW_COLS - 1)));
        assert!(!cfg("auto").opens_on_orchestrator(Some(ORCHESTRATOR_NARROW_COLS)));
        assert!(!cfg("auto").opens_on_orchestrator(None));
        assert!(cfg("ALWAYS").opens_on_orchestrator(None));
        assert!(!cfg("never").opens_on_orchestrator(Some(40)));
        assert_eq!(cfg("weird").orchestrator_view_word(), "auto");
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use serde_json::{json, Value};

    fn layers(dir: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        (
            dir.path().join("config.json"),
            dir.path().join("config.local.json"),
        )
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn an_update_writes_only_the_keys_it_changed() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"editor": "nvim", "someone_elses": [1]}"#).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap();
        assert_eq!(
            read(&path),
            json!({"editor": "nvim", "someone_elses": [1], "kanban": true}),
            "no defaults materialised, unknown keys kept"
        );
    }

    /// A project's default accounts, as `profiles::set_project_default_in_file`
    /// writes them, read back through the loaded config and kept by an
    /// unrelated save.
    #[test]
    fn project_default_accounts_read_back_and_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"projects": {"/repo": {"run_command": "make"}}}"#).unwrap();
        nebula_core::profiles::set_project_default_in_file(&path, "/repo", "claude", Some("work"))
            .unwrap();
        nebula_core::profiles::set_project_default_in_file(&path, "/repo", "codex", Some("gpt"))
            .unwrap();
        let cfg = Config::load_at(&path, &local);
        let repo = cfg.project(Path::new("/repo"));
        assert_eq!(repo.default_profile("claude").as_deref(), Some("work"));
        assert_eq!(repo.default_profile(" codex ").as_deref(), Some("gpt"));
        assert_eq!(repo.default_profile("cursor"), None);
        assert!(cfg
            .project(Path::new("/other"))
            .default_profiles()
            .is_empty());
        Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap();
        let cfg = Config::load_at(&path, &local);
        assert_eq!(
            cfg.project(Path::new("/repo"))
                .default_profile("claude")
                .as_deref(),
            Some("work")
        );
        assert_eq!(cfg.project(Path::new("/repo")).run_command, "make");
    }

    /// The PROJECT TAB's Default account rows: one per harness a usable
    /// profile is for, after the declared rows.
    #[test]
    fn project_account_rows_follow_the_usable_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(
            &path,
            r#"{"profiles":[{"name":"gpt","harness":"codex"},{"name":"bad name","harness":"cursor"},{"name":"work","harness":"claude"},{"name":"home","harness":"codex"}]}"#,
        )
        .unwrap();
        let cfg = Config::load_at(&path, &local);
        assert_eq!(cfg.project_account_harnesses(), ["codex", "claude"]);
        let declared = tab_settings(project_tab()).len();
        assert_eq!(cfg.project_account_row(0), None, "a declared row");
        assert_eq!(cfg.project_account_row(declared).as_deref(), Some("codex"));
        assert_eq!(
            cfg.project_account_row(declared + 1).as_deref(),
            Some("claude")
        );
        assert_eq!(cfg.project_account_row(declared + 2), None);
    }

    #[test]
    fn two_writers_changing_different_keys_keep_both() {
        // The TUI and the desktop each load, change one key and save; the
        // second must not put the first one's key back to what it loaded.
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        let stale_desktop_view = Config::load_at(&path, &local);
        assert!(!stale_desktop_view.kanban);
        Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.theme = "ocean".into()).unwrap();
        let now = Config::load_at(&path, &local);
        assert!(now.kanban);
        assert_eq!(now.theme, "ocean");
    }

    #[test]
    fn an_update_keeps_a_local_key_local() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&local, r#"{"theme": "forest"}"#).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.theme = "rose".into()).unwrap();
        assert_eq!(read(&local), json!({"theme": "rose"}));
        assert!(!path.exists(), "config.json, unchanged, is not written");
    }

    #[test]
    fn an_update_carries_a_renamed_key_over() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"hide_terminal_glyphs": true}"#).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap();
        assert_eq!(
            read(&path),
            json!({"hide_card_marks": true, "kanban": true})
        );
        assert!(Config::load_at(&path, &local).hide_card_marks);
    }

    #[test]
    fn an_update_that_empties_a_map_removes_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(
            &path,
            r#"{"harnesses": {"codex": {"enabled": false}}, "x": 1}"#,
        )
        .unwrap();
        Config::update_in(&path, &local, |cfg| cfg.harnesses.clear()).unwrap();
        assert_eq!(read(&path), json!({"x": 1}));
    }

    #[test]
    fn an_update_leaves_an_unreadable_value_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"kanban": "maybe", "theme": "rose"}"#).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.theme = "sand".into()).unwrap();
        assert_eq!(read(&path), json!({"kanban": "maybe", "theme": "sand"}));
    }

    #[test]
    fn a_raw_key_patch_goes_where_the_key_lives() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"kanban": true}"#).unwrap();
        patch_raw_key(&path, &local, "desktop", json!({"theme": "dark"})).unwrap();
        assert_eq!(
            read(&path),
            json!({"kanban": true, "desktop": {"theme": "dark"}})
        );
        std::fs::write(&local, r#"{"desktop": {}}"#).unwrap();
        patch_raw_key(&path, &local, "desktop", json!({"theme": "light"})).unwrap();
        assert_eq!(read(&local), json!({"desktop": {"theme": "light"}}));
        assert_eq!(
            read_raw_key(&path, &local, "desktop"),
            Some(json!({"theme": "light"}))
        );
    }

    /// What Notepad leaves after a hand edit with one typo: a byte order
    /// mark, and a trailing comma.
    const BROKEN: &str = "\u{feff}{\n  \"theme\": \"rose\",\n  \"profiles\": [],\n}\n";

    fn assert_refused(err: impl std::fmt::Display, path: &Path) {
        let err = err.to_string();
        assert!(err.contains("can't be parsed"), "{err}");
        assert!(err.contains("nothing was written"), "{err}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), BROKEN, "untouched");
        assert!(
            !path.with_file_name("config.json.lock").exists(),
            "lock let go"
        );
    }

    #[test]
    fn a_byte_order_mark_is_read_and_written_past() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, "\u{feff}{\"theme\": \"rose\", \"x\": 1}").unwrap();
        assert_eq!(Config::load_at(&path, &local).theme, "rose");
        Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap();
        assert_eq!(read(&path)["theme"], "rose");
        assert_eq!(read(&path)["x"], 1);
        assert_eq!(read(&path)["kanban"], true);
    }

    #[test]
    fn no_writer_replaces_a_settings_file_it_cannot_parse() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, BROKEN).unwrap();
        assert_refused(
            Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap_err(),
            &path,
        );
        assert_refused(Config::default().save_to(&path).unwrap_err(), &path);
        assert_refused(
            patch_raw_key(&path, &local, "desktop", json!({})).unwrap_err(),
            &path,
        );
        assert_refused(
            crate::desktop::DesktopPrefs::update_in(&path, &local, |p| p.set("theme", "dark"))
                .unwrap_err(),
            &path,
        );
        assert_refused(crate::schema::reset_in(&path, &local).unwrap_err(), &path);
        assert!(write_blocker(&path, &local)
            .unwrap()
            .contains("config.json can't be parsed"));
    }

    #[test]
    fn a_broken_local_layer_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, r#"{"theme": "rose"}"#).unwrap();
        std::fs::write(&local, "[1,").unwrap();
        let err = Config::update_in(&path, &local, |cfg| cfg.kanban = true).unwrap_err();
        assert!(err.to_string().contains("config.local.json"), "{err}");
        assert_eq!(read(&path), json!({"theme": "rose"}));
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "[1,");
        assert!(write_blocker(&path, &local).is_some());
        std::fs::remove_file(&local).unwrap();
        assert_eq!(write_blocker(&path, &local), None);
    }

    #[test]
    fn a_terminal_reset_backs_a_broken_file_up_before_replacing_it() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(&path, BROKEN).unwrap();
        std::fs::write(&local, "{\"theme\": \"sand\"}").unwrap();
        let cfg = Config::reset_layers(&path, &local).unwrap();
        assert_eq!(cfg.theme, Config::default().theme);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.json.bak")).unwrap(),
            BROKEN
        );
        assert!(
            !dir.path().join("config.local.json.bak").exists(),
            "it parsed"
        );
        assert!(!local.exists());
        assert!(read(&path).is_object());
    }

    #[test]
    fn a_change_to_the_local_layer_leaves_config_json_unwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        // Hand formatted: a rewrite would reflow it.
        let hand = "{\"kanban\": false,   \"x\": [1,2]}";
        std::fs::write(&path, hand).unwrap();
        std::fs::write(&local, r#"{"theme": "forest"}"#).unwrap();
        Config::update_in(&path, &local, |cfg| cfg.theme = "rose".into()).unwrap();
        assert_eq!(read(&local), json!({"theme": "rose"}));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), hand);
        // A save that changes nothing writes nothing either.
        let cfg = Config::load_at(&path, &local);
        cfg.save_layers(&path, &local).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        cfg.save_layers(&path, &local).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    }

    #[test]
    fn a_reset_is_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let (path, local) = layers(&dir);
        std::fs::write(
            &path,
            r#"{"kanban": true, "desktop": {"theme": "dark", "future": 1}}"#,
        )
        .unwrap();
        let held = nebula_core::settings::lock(&path).unwrap();
        let reset = {
            let (path, local) = (path.clone(), local.clone());
            std::thread::spawn(move || crate::schema::reset_in(&path, &local))
        };
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(read(&path)["kanban"], true, "waits on the lock");
        drop(held);
        reset.join().unwrap().unwrap();
        let after = read(&path);
        assert_eq!(after["kanban"], false);
        assert_eq!(after["desktop"]["theme"], "system");
        assert_eq!(after["desktop"]["future"], 1);
    }
}
