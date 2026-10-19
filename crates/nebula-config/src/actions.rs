//! The verb registry, as data: every key the TUI's grid reacts to, with
//! its stable config id, label, hint, Hotkeys-tab group, scope and default
//! chords, and what both apps need to say about a verb (its one word, what
//! it acts on, where it works, whether it destroys anything) and the
//! desktop window's own default keys for it. Verbs only the window has are
//! in [`DESKTOP_ONLY`]. The chord parsing and dispatch (`KeyChord`,
//! `Keymap`) stay in the TUI, which needs a terminal backend for them; the
//! window reads this table through the generated `desktop/src/lib/
//! actions.gen.ts` ([`render_ts`]).

use serde::Serialize;
use std::collections::BTreeMap;

/// Which mode a binding is live in. The same chord may mean different
/// things in each, so conflicts are only conflicts within one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The launcher grid, and a terminal pane that isn't input-locked.
    Global,
    /// Input-locked terminal pane, where every other key is forwarded to
    /// the child process.
    Terminal,
}

/// Everything a hotkey can do. Overlay-local keys (Esc to close, j/k inside
/// a picker, the line-editor bindings) are deliberately absent: they're the
/// modal grammar every overlay shares, not application hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // navigate
    FocusNext,
    FocusLeft,
    FocusRight,
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
    Activate,
    Palette,
    /// `^K`: the COMMAND PALETTE — the `/` palette's projects, worktrees,
    /// sessions and pull requests, plus every action here by its label,
    /// Enter running it.
    CommandPalette,
    /// `⌥←`: BACK to the place (project tab, card, full-screen) the grid
    /// was on before this one.
    NavBack,
    /// `⌥→`: FORWARD again, after a back.
    NavForward,
    /// `.`: the next session in the PALETTE's attention order, no modal.
    NextAttention,
    /// `,`: the same walk backwards.
    PrevAttention,
    /// `]`: the LAUNCHER VIEW's next PROJECT TAB to the right — that
    /// project's sessions. The last tab goes no further.
    NextProjectTab,
    /// `[`: the tab to the left. The first tab goes no further.
    PrevProjectTab,
    /// `x`: close the PROJECT TAB the grid is on, landing on the tab that
    /// slides into its place.
    CloseProjectTab,
    /// `+` (and `⌘P` where the terminal sends ⌘): the PROJECT DROPDOWN, the
    /// list the `+` after the PROJECT TABS drops — type to narrow it, Enter
    /// or a click opens the project.
    ProjectDropdown,
    // projects & worktrees
    AddProject,
    New,
    GitDiff,
    OpenRepo,
    /// `Shift+V`: the pull request of the session card under the cursor,
    /// in the browser — the `#42 title` line on the card. `v` lists the
    /// project's pull requests in nebula; the shifted key goes to GitHub.
    OpenPullRequest,
    /// The PULL REQUEST COMPOSER for the checkout of the card or band
    /// under the cursor: title, body, base and draft, prefilled from the
    /// session and the commits since the base; Enter pushes the branch and
    /// opens the pull request (GitHub through `gh`, Azure DevOps through
    /// its API). What dropping a Review card on Ship does.
    CreatePullRequest,
    /// Merge the open pull request of the checkout under the cursor, once
    /// its confirm (method, delete the remote branch) is answered. What
    /// dropping a Ship card on Done does.
    MergePullRequest,
    /// FOCUS HERE: narrow the bench's SCOPE (the header's scope chip) to
    /// the repo, worktree or card under the cursor: the board, the
    /// attention strip, `.`/`,` and the quick prompt follow it.
    FocusScope,
    /// The SCOPE CHIP's account: a small picker of the accounts the
    /// board's sessions run on, and Any account; the pick narrows the
    /// scope to that account's sessions, the rest of the scope kept.
    ScopeAccount,
    /// `Shift+←` on the KANBAN: the card under the cursor let go on the
    /// lane to its left, as a drop there does (core's `drop_verb`).
    MoveCardLeft,
    /// `Shift+→`: the lane to its right; off Done, the Archived zone.
    MoveCardRight,
    /// `Shift+I`: the GitHub issue the session card under the cursor was
    /// started from, in the browser. `i` lists the project's issues in
    /// nebula; the shifted key goes to GitHub.
    OpenIssue,
    /// `Shift+T`: a terminal *outside* nebula — a new Ghostty tab in the
    /// selected worktree's directory, where `t` opens one inside nebula; a
    /// silent no-op on a machine without Ghostty.
    OpenGhosttyTab,
    /// `Shift+R`: reload from GitHub now, past every timer — the project's
    /// open pull requests and issues, the worktree's PR, the one the pane
    /// is reading.
    RefreshPullRequests,
    /// `y`: reply — the COMMENT BOX on the card's pull request (the one
    /// `Shift+V` opens), or on the one the pane is reading, posted to
    /// GitHub with `gh pr comment` on Enter.
    CommentPullRequest,
    /// `i`: the ISSUES MODAL — the project's open GitHub issues, read in
    /// place, commented on, with a QUICK PROMPT or an AGENT PRESET launched
    /// on one.
    Issues,
    /// `v`: the PULL REQUESTS MODAL — the project's open pull requests,
    /// read in place, commented on, with a PR SESSION launched on one.
    PullRequests,
    /// `c`: the BRANCH SWITCHER — move the project's ROOT WORKTREE onto
    /// another branch, asking what to do with uncommitted changes.
    SwitchBranch,
    /// `⇧B`: the AZURE DEVOPS OVERLAY — the project's sprint tasks, its
    /// active pull requests and its branches, on a project whose remote
    /// is on Azure DevOps.
    Ado,
    /// `Shift+Enter` / `Shift+O` / `Alt+Enter`, on the grid: fire the
    /// selected worktree's OPEN COMMAND — the project's **Open command**
    /// setting, else its `.nebula.json` `open` (`open http://localhost:3000`,
    /// say). Three chords because only the kitty protocol carries a shifted
    /// Enter: Terminal.app sends a plain one and tmux flattens it, while
    /// `ESC CR` — what `/terminal-setup` gives Shift+Enter in VS Code, and
    /// Option+Enter on a Mac with Option as Meta — gets through both and
    /// parses as Alt+Enter. Its RUN COMMAND is the project menu's **Run**.
    OpenWorktree,
    /// `w`: the selected worktree's dev server, `http://localhost:<port>`,
    /// in a browser picked from the ones installed (`crate::localhost`).
    OpenInBrowser,
    /// `Shift+W`: a headless screenshot of that page, shown in the image
    /// modal.
    Screenshot,
    // sessions
    NewTerminal,
    Rename,
    Archive,
    Unarchive,
    ToggleArchived,
    Delete,
    DeleteAll,
    /// The AGENT PRESETS list: saved launch definitions for the checkout
    /// under the cursor.
    AgentPresets,
    /// The QUICK PROMPT: type a task, launch an agent on it.
    QuickPrompt,
    /// `Space`: the FOLLOW-UP MODAL for the selected session card — its
    /// next turn, typed in a box over the grid and sent on Enter.
    FollowUp,
    /// `Shift+P`: the QUICK PROMPT on the card under the cursor's settings
    /// — the same harness, model, effort and worktree, and the issue it
    /// was started from — so the task is all there is to type. `p` opens
    /// the box on the Agents tab defaults; the shifted key opens it on
    /// the card.
    DuplicateSession,
    /// `m`: move the card under the cursor onto another checkout, of its
    /// project or another one, picked from a list. Dragging the card onto
    /// another band or project tab does the same.
    MoveSession,
    /// `⌥↑`: the card, band or project tab under the cursor one place
    /// earlier in the user's order (`ClientRequest::Reorder`), as dragging
    /// it there does.
    ReorderUp,
    /// `⌥↓`: one place later.
    ReorderDown,
    /// `⇧S`: the SUBAGENTS OVERLAY — the project's sessions as a tree,
    /// each with the subagents running inside it.
    Subagents,
    // files
    FindFile,
    Grep,
    TreeBrowser,
    // terminal
    UnlockTerminal,
    // general
    /// `^~`: fold the LAUNCHER VIEW's PANE away and give the cards the
    /// whole body, or bring it back. Folding it also unselects the card
    /// under the cursor, so nothing is selected and nothing is read.
    ToggleLauncherPane,
    /// `F10`: toggle between the ORCHESTRATOR chat view and the grid.
    ToggleOrchestratorView,
    /// `⇧U`: the ACCOUNTS OVERLAY — each account's plan usage, login,
    /// and its on/off and default switches.
    Accounts,
    /// `⇧K`: the KANBAN — the grid as lanes by status — or back to the
    /// bands; saved as Settings → Appearance → **Kanban**.
    ToggleKanban,
    /// `^F`: FULL-SCREEN the session in the PANE — the grid and its header
    /// give way to the PTY — or bring it back down to the pane beside the
    /// cards. From the cards it full-screens the one under the cursor.
    ToggleFullScreen,
    /// `` ` ``: walk the TERMINAL cards of the cursor's checkout, one after
    /// another and back round to the first. What the pane READS, where
    /// `^~` is whether it is drawn at all.
    PaneTabs,
    /// Open the Nth PROJECT TAB (1-based) in the LAUNCHER VIEW's header.
    SelectProjectTab(u8),
    Hosts,
    Settings,
    Metrics,
    /// `?`: the VERBS MENU — what can be done to the selection (the card,
    /// band, tab or row under the cursor), each with its key, Enter
    /// running it; its last row is the full help ([`Action::Help`]).
    Verbs,
    Help,
    /// The WHAT'S NEW page (`crate::learn::WHATS_NEW`): unbound by default,
    /// in the palette and on the Help overlay.
    WhatsNew,
    Quit,
    // The menus' verbs, rebindable but unbound by default: each acts on
    // the selection, as its menu row does on the row it was opened on.
    /// Restart the session's agent, resuming its conversation.
    Restart,
    /// Cut a new worktree in the project.
    NewWorktree,
    /// Pin the session as the machine's ORCHESTRATOR, or unpin the one
    /// that is.
    PinOrchestrator,
    /// Restart the session fresh on another account, picked from a list.
    MoveToAccount,
    /// Start the checkout's RUN COMMAND, or stop it while it runs.
    ToggleRun,
    /// Delete a linked worktree (its folder; the branch stays).
    DeleteWorktree,
    /// Take the project off the list (nothing on disk is touched).
    RemoveProject,
    /// Delete the project's linked worktrees with nothing in them whose
    /// pull request merged or whose commits are all on a remote, after
    /// one confirm listing them; never forced.
    SweepWorktrees,
    /// Write this machine's settings to one file (`kete config export`).
    ExportSettings,
    /// Merge a settings file in (`kete config import`), after a confirm
    /// saying what it changes.
    ImportSettings,
    // 0.54/0.55: the editors, the copy verbs, wake-up and the critters,
    // unbound by default like the menus' verbs above.
    /// Open the folder or file in VS Code (or the VS Code family editor
    /// detected), focusing a window already on it.
    OpenInVscode,
    /// Open the checkout's solution (or the file) in Visual Studio.
    OpenInVisualStudio,
    /// Snap the checkout's editor window to the half of the screen beside
    /// Kete's.
    DockEditor,
    /// Show the folder or file in Explorer (Finder, the file manager).
    RevealInExplorer,
    /// Open the file with the app the system gives its type.
    OpenWithDefault,
    /// Open the file in Kete's own viewer.
    OpenFileInKete,
    /// Copy the thing's full path.
    CopyPath,
    /// Copy the file's or folder's path from its worktree's root.
    CopyRelativePath,
    /// Copy the checkout's branch name.
    CopyBranch,
    /// WHAKAOHO now: start the account's 5-hour window with one tiny
    /// prompt.
    WakeAccount,
    /// The CRITTERS overlay on or off.
    Critters,
    // 0.54, the menu matrix: the menus' own verbs, unbound like the rest.
    /// Mark a session's finished turn read, or every unread one in the
    /// checkout, the repo or what the board shows.
    MarkSeen,
    /// Copy the link of the checkout's pull request.
    CopyPrUrl,
    /// Archive the finished sessions already read, in the checkout, the
    /// repo or what the board shows, after one confirm listing them.
    ArchiveDone,
    /// WHAKAOHO's schedule: the wake-up view, on the account it was asked
    /// from.
    WakeSchedule,
    // 0.56: work that starts from a session as a session of its own.
    /// ASK ASIDE: a side question about the session, answered by a new
    /// session beside it in the same checkout that changes no files.
    AskAside,
}

/// What a verb acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliesTo {
    /// Nothing in particular: the app itself, a view, a list.
    Global,
    Repo,
    Worktree,
    /// An agent session.
    Session,
    /// A shell terminal (and, in the TUI, the pane's PTY).
    Terminal,
    Account,
    /// A file in a worktree: a diff's file row, the tree browser's, a
    /// finder's.
    File,
    /// A folder in a worktree.
    Folder,
}

/// Where a verb means something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorksIn {
    /// The TUI's grid of cards; the desktop's overview.
    Grid,
    /// The kanban board, in either app.
    Board,
    /// A session's pane (the TUI's) or view (the desktop's).
    Pane,
    Diff,
    /// A modal over the rest.
    Overlay,
}

/// Which app answers a verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Both,
    /// The terminal app only: the desktop has no key for it.
    Tui,
    /// The desktop window only: never in the TUI's [`ACTIONS`].
    Desktop,
}

/// Where a desktop key counts, as `desktop/src/lib/shortcuts.ts` reads it:
/// `anywhere` even in a field or terminal, `fields` in a field but not a
/// terminal, `global` with neither, `row` on a focused row or card, and
/// `overview` / `board` for the home views' own walking keys (listed, not
/// rebindable: those views handle them), and `review` for the review
/// row's ⌥ chords, which only count while a review's diff is up (the diff
/// view handles them, as the terminal app's docked review does; not
/// rebindable either).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeskScope {
    Anywhere,
    Fields,
    Global,
    Row,
    Overview,
    Board,
    Review,
}

/// One of the desktop window's key rows. A verb may have two when its
/// chords count in different places (Ctrl+K in fields, Ctrl+Shift+P even
/// in a terminal). `id` is the window's own and its config key under
/// [`DESKTOP_PREFIX`]: never rename one.
#[derive(Debug, Serialize)]
pub struct DeskKey {
    pub id: &'static str,
    /// What the window's help and its Hotkeys rows call it.
    pub label: &'static str,
    /// The window's help section.
    pub group: &'static str,
    pub scope: DeskScope,
    /// The diff editor (Monaco) binds this chord itself: the key stands
    /// down while the focus is in one.
    pub editor_owns: bool,
    /// Spelled as `chordOf` spells a key event: `ctrl+shift+b`,
    /// `alt+arrowleft`, `escape`, `?`.
    pub chords: &'static [&'static str],
}

/// A desktop key row that the diff editor doesn't own.
const fn dk(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    scope: DeskScope,
    chords: &'static [&'static str],
) -> DeskKey {
    DeskKey {
        id,
        label,
        group,
        scope,
        editor_owns: false,
        chords,
    }
}

/// One rebindable row: what it does, where it's live, and what it starts as.
pub struct ActionSpec {
    pub action: Action,
    /// Stable key in the config file's `keybindings` object.
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// Section header in the Hotkeys tab.
    pub group: &'static str,
    pub scope: Scope,
    pub defaults: &'static [&'static str],
    /// The verb in a word, for a footer or the verb bar.
    pub word: &'static str,
    /// Loses something no key brings back: it asks first.
    pub destructive: bool,
    pub applies_to: &'static [AppliesTo],
    pub works_in: &'static [WorksIn],
    pub surface: Surface,
    /// The desktop window's default keys for this verb, empty on a
    /// [`Surface::Tui`] row.
    pub desktop: &'static [DeskKey],
}

/// A verb only the desktop window has: an [`ActionSpec`] less the TUI's
/// action, scope and chords.
pub struct DesktopVerb {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub group: &'static str,
    pub word: &'static str,
    pub destructive: bool,
    pub applies_to: &'static [AppliesTo],
    pub works_in: &'static [WorksIn],
    pub desktop: &'static [DeskKey],
}

/// One positional PROJECT TAB shortcut. `⌘N` is what a browser user
/// reaches for — but it is [`Reach::Blocked`] in Terminal.app and most
/// other emulators, which never encode ⌘ into pty bytes at all. The bare
/// digit is bound alongside it and is the chord that actually fires there;
/// digits are otherwise unbound on the grid.
macro_rules! project_tab_slot {
    ($n:literal, $id:literal, $label:literal, $cmd:literal, $digit:literal, $desk:literal) => {
        ActionSpec {
            action: Action::SelectProjectTab($n),
            id: $id,
            label: $label,
            hint: "Open the project on that tab of the header, counting from the left (⌘N only in emulators that send ⌘)",
            group: "NAVIGATE",
            scope: Scope::Global,
            // The digit first: it arrives everywhere, and ⌘N is a silent
            // alias Help and the footer leave out (`shown_chords`).
            defaults: &[$digit, $cmd],
            word: "tab",
            destructive: false,
            applies_to: &[AppliesTo::Repo],
            works_in: &[WorksIn::Grid, WorksIn::Board],
            surface: Surface::Both,
            // The window's digit is the Nth repo on its rail, in the
            // user's order. A label each, so a rebound row in Settings,
            // Hotkeys still says which repo it opens.
            desktop: &[dk($desk, concat!("Repo ", $digit, " on the rail"), "Views", DeskScope::Global, &[$digit])],
        }
    };
}

pub const ACTIONS: &[ActionSpec] = &[
    // ---- NAVIGATE ----
    ActionSpec {
        action: Action::FocusNext,
        id: "focus_next",
        label: "Open / fold checkout",
        hint: "Open every card of the checkout under the cursor in place, or fold it back to one row: one checkout open at a time",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["tab"],
        word: "fold",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::FocusLeft,
        id: "focus_left",
        label: "Focus left",
        hint: "Step to the card on the left, stopping at the row's first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["h", "left"],
        word: "left",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::FocusRight,
        id: "focus_right",
        label: "Focus right",
        hint: "Step to the card on the right, stopping at the row's last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["l", "right"],
        word: "right",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MoveDown,
        id: "move_down",
        label: "Move down",
        hint: "Move the cursor down a row of cards; twice from the project tabs, into the one under their cursor",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["j", "down"],
        word: "down",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MoveUp,
        id: "move_up",
        label: "Move up",
        hint: "Move the cursor up a row of cards, stopping at the first; twice there, up to the project tabs",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["k", "up"],
        word: "up",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::HalfPageDown,
        id: "half_page_down",
        label: "Half page down",
        hint: "Jump the cursor down two rows of cards, stopping at the last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["ctrl+d"],
        word: "page",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::HalfPageUp,
        id: "half_page_up",
        label: "Half page up",
        hint: "Jump the cursor up two rows of cards, stopping at the first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["ctrl+u"],
        word: "page",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Activate,
        id: "activate",
        label: "Open / attach",
        hint: "Into the pane on the card under the cursor, or lock the pane's input",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["enter"],
        word: "open",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Terminal],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Palette,
        id: "palette",
        label: "Fuzzy jump",
        hint: "Search every project, worktree and session at once",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["/"],
        word: "jump",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        // The window's find: a view with a filter box (the ADO lists, the
        // diff's file list) takes f and / itself; elsewhere they open the
        // palette, the window's jump.
        desktop: &[
            dk("find", "Find: the view's filter, else the palette", "General", DeskScope::Global, &["f", "/"]),
        ],
    },
    ActionSpec {
        action: Action::CommandPalette,
        id: "command_palette",
        label: "Command palette",
        hint: "Search every project, worktree, session and pull request, and every action by its name: Enter jumps to the row or runs the action",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["ctrl+k"],
        word: "commands",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            DeskKey { id: "palette", label: "Command palette", group: "General", scope: DeskScope::Fields, editor_owns: true, chords: &["ctrl+k"] },
            dk("palette_alt", "Command palette", "General", DeskScope::Anywhere, &["ctrl+shift+p"]),
        ],
    },
    ActionSpec {
        action: Action::NavBack,
        id: "nav_back",
        label: "Back",
        hint: "Go back to the place the grid was on before (its project tab, the card under the cursor, full-screen or not), as the header's ‹ does. < is bound beside ⌥←, which needs Option as Meta on a Mac",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["alt+left", "<"],
        word: "back",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            dk("back", "Back", "Views", DeskScope::Fields, &["alt+arrowleft"]),
        ],
    },
    ActionSpec {
        action: Action::NavForward,
        id: "nav_forward",
        label: "Forward",
        hint: "Go forward again after a back, as the header's › does. > is bound beside ⌥→, which needs Option as Meta on a Mac",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["alt+right", ">"],
        word: "forward",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            dk("forward", "Forward", "Views", DeskScope::Fields, &["alt+arrowright"]),
        ],
    },
    ActionSpec {
        action: Action::NextAttention,
        id: "next_attention",
        label: "Next session needing you",
        hint: "Jump to the next session in the palette's attention order (needs feedback, running, unseen, then recency) in any project, wrapping",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["."],
        word: "next",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("next_attention", "Next session needing you", "Views", DeskScope::Global, &["."]),
        ],
    },
    ActionSpec {
        action: Action::PrevAttention,
        id: "prev_attention",
        label: "Prev session needing you",
        hint: "The same walk backwards: the session before this one in the palette's attention order, wrapping at the top",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &[","],
        word: "prev",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("prev_attention", "Previous session needing you", "Views", DeskScope::Global, &[","]),
        ],
    },
    ActionSpec {
        action: Action::NextProjectTab,
        id: "next_project_tab",
        label: "Next project tab",
        hint: "Open the project on the tab to the right of this one in the header, stopping at the last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["]"],
        word: "tab",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::PrevProjectTab,
        id: "prev_project_tab",
        label: "Prev project tab",
        hint: "Open the project on the tab to the left, stopping at the first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["["],
        word: "tab",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    project_tab_slot!(1, "project_tab_1", "Project tab 1", "cmd+1", "1", "repo_1"),
    project_tab_slot!(2, "project_tab_2", "Project tab 2", "cmd+2", "2", "repo_2"),
    project_tab_slot!(3, "project_tab_3", "Project tab 3", "cmd+3", "3", "repo_3"),
    project_tab_slot!(4, "project_tab_4", "Project tab 4", "cmd+4", "4", "repo_4"),
    project_tab_slot!(5, "project_tab_5", "Project tab 5", "cmd+5", "5", "repo_5"),
    project_tab_slot!(6, "project_tab_6", "Project tab 6", "cmd+6", "6", "repo_6"),
    project_tab_slot!(7, "project_tab_7", "Project tab 7", "cmd+7", "7", "repo_7"),
    project_tab_slot!(8, "project_tab_8", "Project tab 8", "cmd+8", "8", "repo_8"),
    project_tab_slot!(9, "project_tab_9", "Project tab 9", "cmd+9", "9", "repo_9"),
    ActionSpec {
        action: Action::CloseProjectTab,
        id: "close_project_tab",
        label: "Close project tab",
        hint: "Drop this project's tab from the header and open the tab beside it. Nothing is deleted: + opens it again",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["x"],
        word: "close",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ProjectDropdown,
        id: "project_dropdown",
        label: "Switch project",
        hint: "Drop the list of every project under the + in the header: type to narrow it, Enter or a click opens one. + is the header's own button and arrives in every terminal and in kete browser; ⌘P does the same from inside the pane, where the terminal sends ⌘ at all (Ghostty/kitty do, Terminal.app and the browser never do)",
        group: "NAVIGATE",
        scope: Scope::Global,
        // `+`, the header button's own glyph, is the key the app shows —
        // it arrives everywhere, where `⌘P` is the browser's print dialog
        // and Terminal.app's nothing. `⌘P` stays bound behind it as the
        // one chord a LOCKED PANE lets through in Ghostty and kitty.
        defaults: &["+", "cmd+p"],
        word: "projects",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    // ---- PROJECTS & WORKTREES ----
    ActionSpec {
        action: Action::AddProject,
        id: "add_project",
        label: "Open a folder as a project",
        hint: "Open a folder as a project in kete, from anywhere; ⇧O opens a checkout outside it, in your editor",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["o"],
        word: "add",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::New,
        id: "new",
        label: "New session",
        hint: "Pick the harness first (→ drills into its model and effort), then the quick prompt opens set to it, in the checkout under the cursor; on the first-run splash, add a project",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["n"],
        word: "new",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::NewWorktree,
        id: "new_worktree",
        label: "New worktree",
        hint: "Cut a new worktree in the selected or shown project",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        // No key in the terminal app: its letters belong to the card
        // under the cursor. The project's menu and the palette reach it.
        defaults: &[],
        word: "worktree",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[dk("new_worktree", "New worktree in this repo", "General", DeskScope::Global, &["shift+n"])],
    },
    ActionSpec {
        action: Action::ToggleRun,
        id: "toggle_run",
        label: "Run / stop the run command",
        hint: "Start the checkout's run command (the project's Run setting, else its .kete.json run), or stop it while it runs",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "run",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::DeleteWorktree,
        id: "delete_worktree",
        label: "Delete worktree",
        hint: "Delete the selected linked worktree's folder, behind a confirmation; its branch stays",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "delete",
        destructive: true,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        // The review row's, as the terminal app's docked review has it.
        desktop: &[dk("review_delete_worktree", "Review: delete the worktree…", "Review", DeskScope::Review, &["alt+w"])],
    },
    ActionSpec {
        action: Action::RemoveProject,
        id: "remove_project",
        label: "Remove project",
        hint: "Take the project off the list, behind a confirmation: nothing on disk is touched",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "remove",
        destructive: true,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::GitDiff,
        id: "git_diff",
        label: "Git diff",
        hint: "Open the diff viewer for the checkout under the cursor, or, with the pane reading a pull request, that pull request's diff",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        // `d` in both apps; `g`, the key it had before, stays bound.
        defaults: &["d", "g"],
        word: "diff",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            dk("open_diff", "Diff the checkout in context: the focused row, the session, the repo (again: back from the diff)", "Views", DeskScope::Anywhere, &["ctrl+shift+d"]),
            dk("diff", "Diff the checkout in context: the focused row, the session, the repo (again: back from the diff)", "Views", DeskScope::Global, &["d"]),
        ],
    },
    ActionSpec {
        action: Action::OpenRepo,
        id: "open_repo",
        label: "Open repo in browser",
        hint: "Send the selected repo's git remote (GitHub, GitLab, …) to your browser",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+g"],
        word: "remote",
        destructive: false,
        applies_to: &[AppliesTo::Repo, AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("open_remote", "Open the repo remote in the browser", "Sessions", DeskScope::Row, &["shift+g"]),
        ],
    },
    ActionSpec {
        action: Action::OpenPullRequest,
        id: "open_pull_request",
        label: "Open pull request in browser",
        hint: "Send the pull request of the session card under the cursor (its checkout's branch) to your browser, as a click on the card's #42 line does. v lists the pull requests in kete; ⇧V goes to GitHub",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+v"],
        word: "PR",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Diff],
        surface: Surface::Both,
        // ⌥P on a review: the pull request's page, or the composer while
        // the branch has none, as the terminal app's review row does.
        desktop: &[
            dk("open_pr", "Open the pull request in the browser", "Sessions", DeskScope::Row, &["shift+v"]),
            dk("review_pr", "Review: view the pull request, or open one…", "Review", DeskScope::Review, &["alt+p"]),
        ],
    },
    ActionSpec {
        action: Action::CreatePullRequest,
        id: "create_pull_request",
        label: "Create pull request\u{2026}",
        hint: "The pull request composer for the checkout under the cursor: title, body, base branch and draft, prefilled from the session and its commits; Enter pushes the branch and opens it. Dropping a Review card on Ship does the same",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "open PR",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MergePullRequest,
        id: "merge_pull_request",
        label: "Merge pull request\u{2026}",
        hint: "Merge the open pull request of the checkout under the cursor, after a confirm that picks the method and whether the remote branch goes. Dropping a Ship card on Done does the same",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "merge",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[dk("review_merge", "Review: merge the pull request…", "Review", DeskScope::Review, &["alt+m"])],
    },
    ActionSpec {
        action: Action::FocusScope,
        id: "focus_scope",
        label: "Focus here",
        hint: "Narrow the scope chip in the header to the repo, worktree or card under the cursor: the board, the attention strip, . and , and the quick prompt follow it. The chip's \u{2715} widens it back to everything, as Esc does once the cursor has let go of the card",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "focus",
        destructive: false,
        applies_to: &[AppliesTo::Repo, AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ScopeAccount,
        id: "scope_account",
        label: "Scope to an account…",
        hint: "Narrow the scope chip to the sessions of one account, picked from those the board's sessions run on (or back to any account); the chip's @ does the same by click, and an account's menu narrows to it straight away. The desktop's chip menu lists the accounts too",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "account",
        destructive: false,
        applies_to: &[AppliesTo::Global, AppliesTo::Account],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MoveCardLeft,
        id: "move_card_left",
        label: "Move card to the lane on the left",
        hint: "On the board: let the card under the cursor go on the lane to its left, as dropping it there does. A lane the agent owns refuses, saying why",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+left"],
        word: "\u{2190} lane",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MoveCardRight,
        id: "move_card_right",
        label: "Move card to the lane on the right",
        hint: "On the board: let the card under the cursor go on the lane to its right, as dropping it there does: Review to Ship opens the pull request composer, Ship to Done merges, and past Done archives (offering the worktree's safe delete)",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+right"],
        word: "lane \u{2192}",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::OpenIssue,
        id: "open_issue",
        label: "Open issue in browser",
        hint: "Send the GitHub issue the session card under the cursor was started from to your browser. i lists the issues in kete; ⇧I goes to GitHub",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+i"],
        word: "issue",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::RefreshPullRequests,
        id: "refresh_pull_requests",
        label: "Reload from GitHub",
        hint: "Ask GitHub again now for the project's open pull requests and issues, the selected worktree's PR and the one the pane is reading",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+r"],
        word: "reload",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("refresh", "Refresh: ask GitHub again for the pull requests (the Branches view fetches)", "General", DeskScope::Global, &["shift+r"]),
        ],
    },
    ActionSpec {
        action: Action::CommentPullRequest,
        id: "comment_pull_request",
        label: "Comment on pull request",
        hint: "Open a box to type a comment and post it through gh on the card's pull request (the one ⇧V opens) or, with the pane reading a pull request (a / jump lands on one), on that one",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["y"],
        word: "comment",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Issues,
        id: "issues",
        label: "GitHub issues",
        hint: "List the project's open issues; Enter prompts an agent on one, ⇧Tab launches a preset on it",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["i"],
        word: "issues",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::PullRequests,
        id: "pull_requests",
        label: "GitHub pull requests",
        hint: "List the project's open pull requests; Enter prompts a PR session on one, ⇧Tab launches a preset on it, Tab picks a harness",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["v"],
        word: "PRs",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::SwitchBranch,
        id: "switch_branch",
        label: "Switch root branch",
        hint: "Fuzzy-pick a branch or remote for the project's root checkout; uncommitted changes get stash / bring / commit / discard",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["c"],
        word: "branch",
        destructive: false,
        applies_to: &[AppliesTo::Repo, AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("switch_branch", "Switch the repo root checkout to another branch", "Sessions", DeskScope::Row, &["c"]),
        ],
    },
    ActionSpec {
        action: Action::Ado,
        id: "azure_devops",
        label: "Azure DevOps",
        hint: "On a project whose remote is on Azure DevOps (the header shows an ado button): your sprint's tasks, the active pull requests and the branches. Enter opens a task or pull request in the browser and checks a branch out in a worktree; s twice starts a task (branch, worktree, agent); on a pull request v votes and c reads its threads to reply",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+b"],
        word: "ADO",
        destructive: false,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::OpenWorktree,
        id: "open_worktree",
        label: "Open checkout in editor",
        hint: "Open the selected checkout outside kete, usually in your editor: its project's Open command (Settings → Project), else .kete.json \"open\", e.g. open http://localhost:3000 (⇧Enter needs the kitty protocol; ⇧O arrives everywhere; ⌥Enter is the ESC CR that VS Code's Shift+Enter setup sends and tmux passes through)",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+enter", "shift+o", "alt+enter"],
        word: "editor",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("open_editor", "Open its worktree in the editor", "Sessions", DeskScope::Row, &["shift+o"]),
        ],
    },
    ActionSpec {
        action: Action::OpenInBrowser,
        id: "open_in_browser",
        label: "Open localhost in a browser",
        hint: "Open the selected worktree's dev server (http://localhost:<port>, the port its sessions listen on) in a browser picked from Chrome, Edge, Firefox or the system default; the pick is remembered per project",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["w"],
        word: "browser",
        destructive: false,
        // A card's and a repo's too, as their checkout's (the menus' Open
        // in group).
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Screenshot,
        id: "screenshot",
        label: "Screenshot localhost",
        hint: "Take a headless Edge or Chrome screenshot (1280×800) of the selected worktree's dev server and show it in the image view",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+w"],
        word: "shot",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    // ---- SESSIONS ----
    ActionSpec {
        action: Action::NewTerminal,
        id: "new_terminal",
        label: "New shell terminal",
        hint: "Spawn a plain shell inside kete, in the selected worktree's directory; ⇧T opens one outside it, in a Ghostty tab",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["t"],
        word: "terminal",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("new_terminal", "New terminal in its worktree", "Sessions", DeskScope::Row, &["t"]),
        ],
    },
    // `t`'s shift pair: the same terminal, outside nebula.
    ActionSpec {
        action: Action::OpenGhosttyTab,
        id: "open_ghostty_tab",
        label: "Terminal in a Ghostty tab",
        hint: "Open a new Ghostty tab in the selected worktree's directory: t's terminal, outside kete; does nothing without Ghostty.app",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+t"],
        word: "ghostty",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Rename,
        id: "rename",
        label: "Rename",
        hint: "Rename the session under the cursor (the checkout's run command is the menu's Run / Stop)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["r"],
        word: "rename",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Terminal, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("rename", "Rename", "Sessions", DeskScope::Row, &["r"]),
        ],
    },
    ActionSpec {
        action: Action::Archive,
        id: "archive",
        label: "Archive session",
        hint: "Archive the selected agent (its PTY is released)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["a"],
        word: "archive",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("archive", "Archive", "Sessions", DeskScope::Row, &["a"]),
            dk("review_archive", "Review: archive the session", "Review", DeskScope::Review, &["alt+a"]),
        ],
    },
    ActionSpec {
        action: Action::Unarchive,
        id: "unarchive",
        label: "Unarchive session",
        hint: "Bring an archived agent back into the list",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["u"],
        word: "unarchive",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("unarchive", "Unarchive", "Sessions", DeskScope::Row, &["u"]),
        ],
    },
    ActionSpec {
        action: Action::ToggleArchived,
        id: "toggle_archived",
        label: "Show / hide archived",
        hint: "Swap the grid between the live sessions and the archived ones",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+a"],
        word: "archived",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("toggle_archived", "Show or hide archived sessions", "Views", DeskScope::Global, &["shift+a"]),
        ],
    },
    ActionSpec {
        action: Action::Delete,
        id: "delete",
        label: "Delete selected",
        hint: "Remove the selected row, behind a confirmation. With the PROJECT TABS holding the keys, close the tab under their cursor, behind the same kind of confirmation: x closes it outright. On a Mac keyboard Del is fn+⌫",
        group: "SESSIONS",
        scope: Scope::Global,
        // Del alone: `d` is the diff, and `⌫` is too easy to hit
        // on the way out of a field.
        defaults: &["delete"],
        word: "delete",
        destructive: true,
        applies_to: &[AppliesTo::Session, AppliesTo::Terminal],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("delete", "Delete (asks first)", "Sessions", DeskScope::Row, &["delete"]),
        ],
    },
    ActionSpec {
        action: Action::DeleteAll,
        id: "delete_all",
        label: "Delete all sessions",
        hint: "Remove every session listed for the checkout under the cursor, behind a confirmation",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+d"],
        word: "delete all",
        destructive: true,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Restart,
        id: "restart",
        label: "Restart the agent",
        hint: "Restart the selected session's agent, resuming its conversation",
        group: "SESSIONS",
        scope: Scope::Global,
        // ⇧R is refresh in both apps: restart is the menus' and the
        // palette's, with no default key.
        defaults: &[],
        word: "restart",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[dk("restart", "Restart the agent", "Sessions", DeskScope::Row, &[])],
    },
    ActionSpec {
        action: Action::PinOrchestrator,
        id: "pin_orchestrator",
        label: "Make or unpin the orchestrator",
        hint: "Pin the selected session as the machine's orchestrator, or unpin it when it is one",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &[],
        word: "orchestrator",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::MoveToAccount,
        id: "move_to_account",
        label: "Move to another account",
        hint: "Restart the selected session fresh on another account, picked from a list (its conversation stays behind)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &[],
        word: "account",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::AgentPresets,
        id: "agent_presets",
        label: "Agent presets",
        hint: "Saved launch presets (CLI, model, effort, prefix/postfix) for the checkout under the cursor; Enter asks for an optional task, or skips it; from the pull requests list, a PR session in that branch's worktree",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["e"],
        word: "presets",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::QuickPrompt,
        id: "quick_prompt",
        label: "Quick prompt",
        hint: "Type a prompt; Enter starts an agent on it (Settings > Agents picks which)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["p"],
        word: "prompt",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo, AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("quick_prompt", "Quick prompt", "General", DeskScope::Global, &["p", "n"]),
            dk("quick_prompt_alt", "Quick prompt", "General", DeskScope::Anywhere, &["ctrl+shift+n"]),
        ],
    },
    ActionSpec {
        action: Action::FollowUp,
        id: "follow_up",
        label: "Follow-up prompt",
        hint: "Open a box over the grid for the selected session's next turn, sent to the running agent on Enter; the pane stays as it is",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["space"],
        word: "reply",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("follow_up", "Follow-up prompt", "Sessions", DeskScope::Row, &["space"]),
        ],
    },
    ActionSpec {
        action: Action::DuplicateSession,
        id: "duplicate_session",
        label: "Duplicate session",
        hint: "Open the quick prompt set to launch what the selected card runs (the same harness, model, effort and worktree) so only the task is left to type. p opens the box on the defaults; ⇧P on the card",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+p"],
        word: "duplicate",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("duplicate", "Duplicate into the quick prompt", "Sessions", DeskScope::Row, &["shift+p"]),
        ],
    },
    ActionSpec {
        action: Action::MoveSession,
        id: "move_session",
        label: "Move session",
        hint: "Pick another worktree, of this project or another, for the selected session; it resumes there, after its current turn when it is mid-turn. Dragging the card onto another band or project tab does the same",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["m"],
        word: "move",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("move", "Move to another worktree", "Sessions", DeskScope::Row, &["m"]),
        ],
    },
    ActionSpec {
        action: Action::ReorderUp,
        id: "reorder_up",
        label: "Move item up",
        hint: "Move the card under the cursor one place earlier in its band (on the board, among its worktree's cards in its lane), or on a band with no card to be on (empty or folded) that band, or with the project tabs holding the keys the tab under their cursor one place left; the menus' Move up and dragging do the same. ⌥↑ needs Option as Meta on a Mac; ⇧↑ is bound beside it",
        group: "SESSIONS",
        scope: Scope::Global,
        // ⌥↑ is the desktop's chord; ⇧↑ beside it arrives where Option is
        // not Meta.
        defaults: &["alt+up", "shift+up"],
        word: "up",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Worktree, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("move_up", "Move up: the session, worktree or repo (or drag it)", "Sessions", DeskScope::Row, &["alt+arrowup"]),
        ],
    },
    ActionSpec {
        action: Action::ReorderDown,
        id: "reorder_down",
        label: "Move item down",
        hint: "Move the card under the cursor one place later in its band (on the board, in its lane), or the project tab under the header's cursor one place right; the menus' Move down and dragging do the same",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["alt+down", "shift+down"],
        word: "down",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Worktree, AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("move_down", "Move down: the session, worktree or repo (or drag it)", "Sessions", DeskScope::Row, &["alt+arrowdown"]),
        ],
    },
    ActionSpec {
        action: Action::Subagents,
        id: "subagents",
        label: "Subagents and session tree",
        hint: "The project's sessions as a tree, each session under the one that started it and the subagents running inside it under that; Enter opens the session, → and ← open and fold, Tab every project. A card's ⑂ chip opens it on that session",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+s"],
        word: "tree",
        destructive: false,
        applies_to: &[AppliesTo::Repo, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    // ---- FILES ----
    ActionSpec {
        action: Action::FindFile,
        id: "find_file",
        label: "Find file",
        hint: "Fuzzy file finder for the selected worktree",
        group: "FILES",
        scope: Scope::Global,
        defaults: &["f"],
        word: "find",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Grep,
        id: "grep",
        label: "Find in files",
        hint: "git grep across the selected worktree",
        group: "FILES",
        scope: Scope::Global,
        defaults: &["shift+f"],
        word: "grep",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::TreeBrowser,
        id: "tree_browser",
        label: "File tree browser",
        hint: "Browse the worktree's files as git lists them, folders folded (the terminal app previews beside the tree)",
        group: "FILES",
        scope: Scope::Global,
        defaults: &["b"],
        word: "files",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        // The window's Files view, beside Diff in the split.
        desktop: &[
            dk("open_files", "Files: the checkout in context as a tree (again: back from it)", "Views", DeskScope::Anywhere, &["ctrl+shift+e"]),
            dk("files", "Files: the checkout in context as a tree (again: back from it)", "Views", DeskScope::Global, &["b"]),
        ],
    },
    // ---- TERMINAL ----
    ActionSpec {
        action: Action::UnlockTerminal,
        id: "unlock_terminal",
        label: "Unlock terminal input",
        hint: "Leave the locked pane and go back to the card (^q always works; ^⇧H needs the kitty protocol)",
        group: "TERMINAL",
        scope: Scope::Terminal,
        defaults: &["ctrl+q", "ctrl+]", "ctrl+shift+h"],
        word: "unlock",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Terminal],
        works_in: &[WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    // ---- GENERAL ----
    ActionSpec {
        action: Action::ToggleLauncherPane,
        id: "toggle_launcher_pane",
        label: "Launcher session pane",
        hint: "Fold the pane under the cards away (which also unselects the card it was reading) or bring it back. From inside the pane the ctrl chords take two presses: the first hands the keys back to the card, the second folds the pane. ^` and ^~ need the kitty protocol; ~ is bound alongside them, the shift of the ` that walks the pane's tabs",
        group: "GENERAL",
        scope: Scope::Global,
        // `^`` is the chord to reach for — out of the pane, then the pane
        // away — and `^~` the same key for the terminals that report it
        // shifted with ctrl held. Only the kitty protocol carries either:
        // a stock terminal has no encoding for ctrl and this key, and
        // sends the same NUL it sends for ^Space. So the bare `~` is bound
        // beside them — the shift of the `` ` `` that walks the pane's
        // tabs, which is the key everything else about the pane is on.
        defaults: &["ctrl+`", "ctrl+~", "~"],
        word: "pane",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ToggleKanban,
        id: "toggle_kanban",
        label: "Kanban board",
        hint: "Lay the grid out as lanes by status (Running, Needs you, Review, Ship, Done) or back to a band per worktree; the Overview | Board toggle at the right of the grid's header does the same by click. On the board h/l walk the lanes, j/k the cards, Tab flips between this project and every project",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+k"],
        word: "board",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[
            dk("toggle_home", "Switch overview and board", "Views", DeskScope::Global, &["shift+k"]),
        ],
    },
    ActionSpec {
        action: Action::ToggleOrchestratorView,
        id: "toggle_orchestrator_view",
        label: "Orchestrator view",
        hint: "Toggle between the chat-first orchestrator view and the grid",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["f10"],
        word: "orchestrator",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        // F10 as here, but never inside a shell on the desktop (htop and mc
        // quit on it). The window's 0 is unbound now the digits are repos:
        // the row stays for a user's own key.
        desktop: &[
            dk("orchestrator", "Orchestrator (start one when there is none)", "Views", DeskScope::Global, &[]),
            dk("orchestrator_f10", "Orchestrator (start one when there is none)", "Views", DeskScope::Fields, &["f10"]),
        ],
    },
    ActionSpec {
        action: Action::Accounts,
        id: "accounts",
        label: "Accounts and usage",
        hint: "Every account (profiles, and each CLI's own login) with its 5h and weekly plan usage, resets and login; Space switches a profile on or off, d makes it its harness's default",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+u"],
        word: "accounts",
        destructive: false,
        // Global too: the background menu's View group lists it.
        applies_to: &[AppliesTo::Account, AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[
            dk("accounts", "Accounts: every login, its use, and who runs on it", "Views", DeskScope::Global, &["shift+u"]),
        ],
    },
    ActionSpec {
        action: Action::ToggleFullScreen,
        id: "toggle_full_screen",
        label: "Full-screen session",
        hint: "Give the session in the pane the whole screen, or bring it back down beside the cards. From the cards it full-screens the one under the cursor. z is typed into a locked pane like any letter; ^f works from inside it too, where it is never forwarded to the agent. ^q and ^` also bring a full-screen session back down",
        group: "GENERAL",
        scope: Scope::Global,
        // `z` in both apps; `^f` stays bound, the chord a locked pane
        // lets through.
        defaults: &["z", "ctrl+f"],
        word: "full",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        // On the desktop ^F is the diff editor's find and the shell's
        // forward-char, so the window's chord takes Shift too (its terminal
        // pane lets it through).
        desktop: &[
            dk("maximize", "Full pane: the session or diff takes the window, or comes back down", "Views", DeskScope::Anywhere, &["ctrl+shift+m"]),
            dk("maximize_z", "Full pane: the session or diff takes the window, or comes back down", "Views", DeskScope::Global, &["z"]),
        ],
    },
    ActionSpec {
        action: Action::PaneTabs,
        id: "pane_tabs",
        label: "Terminals in the pane",
        hint: "Walk the cursor through the terminal cards of its checkout, one after another and back round to the first, the pane reads each",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["`"],
        word: "terminals",
        destructive: false,
        applies_to: &[AppliesTo::Terminal],
        works_in: &[WorksIn::Grid, WorksIn::Pane],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Hosts,
        id: "hosts",
        label: "SSH hosts",
        hint: "Connect to a saved ssh host (restarts kete over ssh)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+h"],
        word: "hosts",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Settings,
        id: "settings",
        label: "Settings",
        hint: "Open this settings overlay (^, needs the kitty protocol; s arrives everywhere)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["s", "ctrl+,"],
        word: "settings",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            dk("settings", "Settings", "General", DeskScope::Anywhere, &["ctrl+,"]),
        ],
    },
    ActionSpec {
        action: Action::Metrics,
        id: "metrics",
        label: "Memory usage",
        hint: "RAM used by kete and every live agent's process tree",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+m"],
        word: "memory",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Verbs,
        id: "verbs",
        label: "Verbs for the selection",
        hint: "What can be done to the card, band, tab or row under the cursor, each with its key: type to narrow, Enter runs one; the last row is every key",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["?"],
        word: "more",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[dk("verbs", "Verbs for the selection", "General", DeskScope::Global, &["?"])],
    },
    ActionSpec {
        action: Action::Help,
        id: "help",
        label: "Help",
        hint: "Toggle the keyboard help overlay: every key (? lists the selection's verbs, with this one row down)",
        group: "GENERAL",
        scope: Scope::Global,
        // `?` is the selection's verbs now, whose last row is this.
        defaults: &["f1"],
        word: "help",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[
            // Kept, unbound, for a user's own key: F1 is `help_f1`.
            dk("help", "Keyboard shortcuts", "General", DeskScope::Global, &[]),
            dk("help_f1", "Keyboard shortcuts", "General", DeskScope::Anywhere, &["f1"]),
        ],
    },
    ActionSpec {
        action: Action::WhatsNew,
        id: "whats_new",
        label: "What's new",
        hint: "What changed in the last few releases, and the keys that moved: shown once after an upgrade, here any time after (Help has it too)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &[],
        word: "what's new",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[dk("whats_new", "What's new", "General", DeskScope::Global, &[])],
    },
    ActionSpec {
        action: Action::Quit,
        id: "quit",
        label: "Quit kete",
        hint: "Leave the TUI (sessions keep running in the daemon)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["q", "ctrl+c"],
        word: "quit",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Tui,
        desktop: &[],
    },
    // Appended: the keymap indexes this table by position.
    ActionSpec {
        action: Action::SweepWorktrees,
        id: "sweep_worktrees",
        label: "Delete merged & clean worktrees",
        hint: "Delete the project's worktrees with no session, terminal or uncommitted work whose PR merged or whose commits are all on a remote, after one confirm listing them; never forced, so git keeps any that picked up work",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &[],
        word: "sweep",
        destructive: true,
        applies_to: &[AppliesTo::Repo],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ExportSettings,
        id: "export_settings",
        label: "Export settings",
        hint: "Write config.json, the agent presets and the ssh hosts to one file, as kete config export does (never config.local.json)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &[],
        word: "export",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ImportSettings,
        id: "import_settings",
        label: "Import settings",
        hint: "Merge a settings file in, as kete config import does: what it would change is shown first and nothing is written until you say so",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &[],
        word: "import",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    // 0.54/0.55, appended: the editors, where else a path opens, the copy
    // verbs, wake-up and the critters. Unbound: the menus, `?` and the
    // palette reach them, and a user may bind one.
    ActionSpec {
        action: Action::OpenInVscode,
        id: "open_in_vscode",
        label: "Open in VS Code",
        hint: "Open the checkout, folder or file in VS Code (or the Insiders, VSCodium, Cursor or Windsurf install found), focusing a window already open on it; a file opens at its line",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "vs code",
        destructive: false,
        applies_to: &[
            AppliesTo::Worktree,
            AppliesTo::Repo,
            AppliesTo::Session,
            AppliesTo::Folder,
            AppliesTo::File,
        ],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::OpenInVisualStudio,
        id: "open_in_visual_studio",
        label: "Open in Visual Studio",
        hint: "Open the checkout's solution in Visual Studio (its folder when it has none), or the file in the instance already running; only where Visual Studio is installed",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "visual studio",
        destructive: false,
        applies_to: &[
            AppliesTo::Worktree,
            AppliesTo::Repo,
            AppliesTo::Session,
            AppliesTo::File,
        ],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::DockEditor,
        id: "dock_editor",
        label: "Dock editor beside Kete",
        hint: "Snap the checkout's open editor window to the half of the screen beside Kete's window and fit Kete to the other half; the system's own snap keys undo it",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "dock",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::RevealInExplorer,
        id: "reveal_in_explorer",
        label: "Show in Explorer",
        hint: "Show the checkout, folder or file in Explorer (Finder on macOS, the file manager elsewhere)",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "reveal",
        destructive: false,
        applies_to: &[
            AppliesTo::Worktree,
            AppliesTo::Repo,
            AppliesTo::Session,
            AppliesTo::Folder,
            AppliesTo::File,
        ],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::OpenWithDefault,
        id: "open_with_default",
        label: "Open with default app",
        hint: "Open the file with the app the system gives its type",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "open with",
        destructive: false,
        applies_to: &[AppliesTo::File],
        works_in: &[WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::OpenFileInKete,
        id: "open_file_in_kete",
        label: "Open in Kete",
        hint: "Open the file in Kete's own viewer",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "view",
        destructive: false,
        applies_to: &[AppliesTo::File],
        works_in: &[WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::CopyPath,
        id: "copy_path",
        label: "Copy path",
        hint: "Copy the full path of the checkout, folder or file to the clipboard",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "copy path",
        destructive: false,
        // A terminal's is its checkout's folder.
        applies_to: &[
            AppliesTo::Worktree,
            AppliesTo::Repo,
            AppliesTo::Session,
            AppliesTo::Terminal,
            AppliesTo::Folder,
            AppliesTo::File,
        ],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::CopyRelativePath,
        id: "copy_relative_path",
        label: "Copy relative path",
        hint: "Copy the file's or folder's path from its worktree's root to the clipboard",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "copy relative",
        destructive: false,
        applies_to: &[AppliesTo::Folder, AppliesTo::File],
        works_in: &[WorksIn::Pane, WorksIn::Diff, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::CopyBranch,
        id: "copy_branch",
        label: "Copy branch name",
        hint: "Copy the checkout's branch name to the clipboard",
        // Beside the other copies, one Hotkeys section for them all.
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "copy branch",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::WakeAccount,
        id: "wake_account",
        label: "Wake up now",
        hint: "Whakaoho: send the account one tiny prompt on its cheapest model now, so its 5-hour window starts, and resets, sooner; it runs on your own subscription",
        group: "ACCOUNTS",
        scope: Scope::Global,
        defaults: &[],
        word: "wake",
        destructive: false,
        applies_to: &[AppliesTo::Account],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::Critters,
        id: "critters",
        label: "Critters",
        hint: "Turn the critters overlay on or off: little pixel visitors along the bottom of the window (Settings, Appearance has their size and how often they come)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &[],
        word: "critters",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    // 0.54, appended by the menu matrix (docs/superpowers/specs/
    // 2026-10-16-kete-054-menu-matrix.md): verbs the menus lead with that
    // `?`, the palette and a user's own key should reach too. Unbound.
    ActionSpec {
        action: Action::MarkSeen,
        id: "mark_seen",
        label: "Mark seen",
        hint: "Mark the session's finished turn as read, or every unread session in the checkout, the repo or what the board shows, so the review lane empties without opening each one",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &[],
        word: "seen",
        destructive: false,
        applies_to: &[
            AppliesTo::Session,
            AppliesTo::Worktree,
            AppliesTo::Repo,
            AppliesTo::Global,
        ],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::CopyPrUrl,
        id: "copy_pr_url",
        label: "Copy pull request link",
        hint: "Copy the link of the checkout's pull request to the clipboard",
        group: "FILES",
        scope: Scope::Global,
        defaults: &[],
        word: "copy link",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::ArchiveDone,
        id: "archive_done",
        label: "Archive done sessions",
        hint: "Archive every finished session you have already read, in the checkout, the repo or what the board shows, after one confirm listing them; unread, running and waiting ones stay, and U brings any back",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &[],
        word: "archive done",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Repo, AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        surface: Surface::Both,
        desktop: &[],
    },
    ActionSpec {
        action: Action::WakeSchedule,
        id: "wake_schedule",
        label: "Wake-up schedule",
        hint: "Whakaoho: each account's wake-up switch, hour and days, and where its 5-hour windows fall across the day; from an account, on that account",
        group: "ACCOUNTS",
        scope: Scope::Global,
        defaults: &[],
        word: "wake-up",
        destructive: false,
        applies_to: &[AppliesTo::Account, AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Overlay],
        surface: Surface::Both,
        desktop: &[],
    },
    // 0.56, appended and unbound: a session's follow-ups that run as
    // sessions of their own.
    ActionSpec {
        action: Action::AskAside,
        id: "ask_aside",
        label: "Ask aside",
        hint: "Ask a side question about the session: a new session beside it, in the same checkout and nested under it, answers without changing files while this one keeps working",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &[],
        word: "ask aside",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane],
        surface: Surface::Both,
        desktop: &[],
    },
];

pub fn spec_at(index: usize) -> Option<&'static ActionSpec> {
    ACTIONS.get(index)
}

pub fn index_of(action: Action) -> Option<usize> {
    ACTIONS.iter().position(|s| s.action == action)
}

/// The row for an action: its label and hint, for anything that names
/// what a key did (the KEY COMBO DISPLAY, for one).
pub fn spec_of(action: Action) -> Option<&'static ActionSpec> {
    index_of(action).and_then(spec_at)
}

/// Verbs only the desktop window has: its views, its theme, its split and
/// the home views' walking keys. Never in [`ACTIONS`], which the TUI's
/// keymap indexes by position.
pub const DESKTOP_ONLY: &[DesktopVerb] = &[
    DesktopVerb {
        id: "cycle_theme",
        label: "Cycle theme",
        hint: "Step the window's theme through system, light and dark",
        group: "GENERAL",
        word: "theme",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        desktop: &[dk("cycle_theme", "Cycle theme: system, light, dark", "General", DeskScope::Anywhere, &["ctrl+shift+l"])],
    },
    DesktopVerb {
        id: "home_board",
        label: "Board",
        hint: "Show the board: the sessions as lanes by status",
        group: "VIEWS",
        word: "board",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        desktop: &[dk("home_board", "Board", "Views", DeskScope::Anywhere, &["ctrl+shift+b"])],
    },
    DesktopVerb {
        id: "home_overview",
        label: "Overview",
        hint: "Show the overview: the repos, their worktrees and sessions",
        group: "VIEWS",
        word: "overview",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        desktop: &[dk("home_overview", "Overview", "Views", DeskScope::Anywhere, &["ctrl+shift+o"])],
    },
    DesktopVerb {
        id: "open_session",
        label: "Show the session",
        hint: "Bring back the session last shown, in the split",
        group: "VIEWS",
        word: "session",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Diff],
        desktop: &[dk("open_session", "Show the session", "Views", DeskScope::Anywhere, &["ctrl+shift+t"])],
    },
    DesktopVerb {
        id: "go_home",
        label: "Home",
        hint: "The home view, overview or board, with nothing open beside it",
        group: "VIEWS",
        word: "home",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        // Unbound: the digits are repos. The header's buttons, ⇧K and the
        // palette reach the views; the row stays for a user's own key.
        desktop: &[dk("go_home", "Home", "Views", DeskScope::Global, &[])],
    },
    DesktopVerb {
        id: "go_session",
        label: "Session",
        hint: "The session last shown",
        group: "VIEWS",
        word: "session",
        destructive: false,
        applies_to: &[AppliesTo::Session],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        // Unbound: the digits are repos. The header's buttons, ⇧K and the
        // palette reach the views; the row stays for a user's own key.
        desktop: &[dk("go_session", "Session", "Views", DeskScope::Global, &[])],
    },
    DesktopVerb {
        id: "go_diff",
        label: "Diff",
        hint: "The diff last shown, or the shown session's worktree's",
        group: "VIEWS",
        word: "diff",
        destructive: false,
        applies_to: &[AppliesTo::Worktree],
        works_in: &[WorksIn::Grid, WorksIn::Board, WorksIn::Pane, WorksIn::Diff],
        // Unbound: the digits are repos. The header's buttons, ⇧K and the
        // palette reach the views; the row stays for a user's own key.
        desktop: &[dk("go_diff", "Diff", "Views", DeskScope::Global, &[])],
    },
    DesktopVerb {
        id: "close_split",
        label: "Close the split",
        hint: "Close the session or diff beside the home view, or Settings; a full pane comes back down first",
        group: "VIEWS",
        word: "close",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Pane, WorksIn::Diff],
        desktop: &[dk(
            "close_split",
            "Close the split or Settings (a full pane comes back down first)",
            "Views",
            DeskScope::Global,
            &["escape"],
        )],
    },
    DesktopVerb {
        id: "row_menu",
        label: "Actions menu",
        hint: "The focused row's or card's actions, as a right-click shows them",
        group: "SESSIONS",
        word: "actions",
        destructive: false,
        applies_to: &[AppliesTo::Repo, AppliesTo::Worktree, AppliesTo::Session, AppliesTo::Terminal],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        desktop: &[dk("row_menu", "Actions menu", "Sessions", DeskScope::Row, &["shift+f10", "contextmenu"])],
    },
    DesktopVerb {
        id: "list_move",
        label: "Move through the rows",
        hint: "Walk the overview's rows, or the board's cards in a lane",
        group: "OVERVIEW AND BOARD",
        word: "move",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        desktop: &[dk(
            "list_move",
            "Move through the rows (cards on the board)",
            "Overview and board",
            DeskScope::Overview,
            &["j", "k", "arrowdown", "arrowup"],
        )],
    },
    DesktopVerb {
        id: "list_open",
        label: "Open the session",
        hint: "Open the session on the focused row or card",
        group: "OVERVIEW AND BOARD",
        word: "open",
        destructive: false,
        applies_to: &[AppliesTo::Session, AppliesTo::Terminal],
        works_in: &[WorksIn::Grid, WorksIn::Board],
        // Enter only: Space is the follow-up, as in the terminal app.
        desktop: &[dk("list_open", "Open the session", "Overview and board", DeskScope::Overview, &["enter"])],
    },
    DesktopVerb {
        id: "overview_fold",
        label: "Unfold, fold",
        hint: "Unfold or fold the overview row's nested sessions",
        group: "OVERVIEW AND BOARD",
        word: "fold",
        destructive: false,
        applies_to: &[AppliesTo::Worktree, AppliesTo::Session],
        works_in: &[WorksIn::Grid],
        desktop: &[dk(
            "overview_fold",
            "Overview: unfold, fold nested sessions",
            "Overview and board",
            DeskScope::Overview,
            &["arrowright", "arrowleft"],
        )],
    },
    DesktopVerb {
        id: "board_lanes",
        label: "Across lanes",
        hint: "Step to the board's lane on either side",
        group: "OVERVIEW AND BOARD",
        word: "lanes",
        destructive: false,
        applies_to: &[AppliesTo::Global],
        works_in: &[WorksIn::Board],
        desktop: &[dk(
            "board_lanes",
            "Board: across lanes (also the side arrows)",
            "Overview and board",
            DeskScope::Board,
            &["h", "l"],
        )],
    },
];

/// The desktop window's key rows in the order its help lists them and its
/// key handler tries them (the first row a chord matches wins).
pub const DESKTOP_ORDER: &[&str] = &[
    "palette",
    "palette_alt",
    "verbs",
    "help",
    "help_f1",
    "whats_new",
    "settings",
    "find",
    "refresh",
    "quick_prompt",
    "quick_prompt_alt",
    "cycle_theme",
    "new_worktree",
    "back",
    "forward",
    "home_board",
    "home_overview",
    "toggle_home",
    "open_session",
    "open_diff",
    "diff",
    "open_files",
    "files",
    "repo_1",
    "repo_2",
    "repo_3",
    "repo_4",
    "repo_5",
    "repo_6",
    "repo_7",
    "repo_8",
    "repo_9",
    "go_home",
    "go_session",
    "go_diff",
    "orchestrator",
    "orchestrator_f10",
    "close_split",
    "maximize",
    "maximize_z",
    "toggle_archived",
    "next_attention",
    "prev_attention",
    "accounts",
    "rename",
    "archive",
    "unarchive",
    "delete",
    "restart",
    "move",
    "duplicate",
    "follow_up",
    "new_terminal",
    "open_editor",
    "open_remote",
    "open_pr",
    "switch_branch",
    "row_menu",
    "move_up",
    "move_down",
    "list_move",
    "list_open",
    "overview_fold",
    "board_lanes",
    "review_pr",
    "review_merge",
    "review_archive",
    "review_delete_worktree",
];

// ---- menus, the verbs menu and the verb bar ----

/// What a verb's row says in a menu, the verbs menu (`?`) and the verb
/// bar, where the Hotkeys tab's label is too long or names the wrong
/// thing: one set of words for both apps. A row with a kind says it on
/// that kind only, ahead of the row without one. A verb not here goes by
/// its label ([`menu_label`]).
pub const MENU_LABELS: &[(&str, Option<AppliesTo>, &str)] = &[
    ("activate", None, "Open"),
    ("follow_up", None, "Follow-up prompt\u{2026}"),
    ("ask_aside", None, "Ask aside\u{2026}"),
    ("restart", None, "Restart agent"),
    ("duplicate_session", None, "Duplicate"),
    ("move_session", None, "Move to\u{2026}"),
    ("move_to_account", None, "Move to account\u{2026}"),
    ("rename", None, "Rename\u{2026}"),
    ("pin_orchestrator", None, "Make orchestrator"),
    ("archive", None, "Archive"),
    ("unarchive", None, "Unarchive"),
    (
        "delete",
        Some(AppliesTo::Terminal),
        "Close terminal\u{2026}",
    ),
    ("delete", None, "Delete\u{2026}"),
    ("open_pull_request", None, "Open pull request"),
    ("open_issue", None, "Open issue"),
    (
        "comment_pull_request",
        None,
        "Comment on pull request\u{2026}",
    ),
    ("reorder_up", None, "Move up"),
    ("reorder_down", None, "Move down"),
    ("toggle_run", None, "Run"),
    ("new", None, "New agent\u{2026}"),
    ("quick_prompt", None, "New session\u{2026}"),
    ("new_terminal", None, "New terminal"),
    ("git_diff", None, "Diff"),
    ("open_worktree", None, "Open in editor"),
    ("open_repo", None, "Open remote in browser"),
    ("switch_branch", None, "Switch branch\u{2026}"),
    ("delete_all", None, "Delete all sessions\u{2026}"),
    ("delete_worktree", None, "Delete worktree\u{2026}"),
    ("new_worktree", None, "New worktree\u{2026}"),
    ("refresh_pull_requests", None, "Refresh pull requests"),
    ("remove_project", None, "Remove from Kete\u{2026}"),
    (
        "sweep_worktrees",
        None,
        "Delete merged & clean worktrees\u{2026}",
    ),
    ("export_settings", None, "Export settings\u{2026}"),
    ("import_settings", None, "Import settings\u{2026}"),
    ("toggle_full_screen", None, "Full screen"),
    ("accounts", None, "Accounts\u{2026}"),
    ("add_project", None, "Add project…"),
    // A row standing alone says the whole thing; inside its submenu
    // ([`MENU_GROUPS`]) it says the short word the group gives it.
    ("open_in_vscode", None, "Open in VS Code"),
    ("open_in_visual_studio", None, "Open in Visual Studio"),
    ("reveal_in_explorer", None, "Show in Explorer"),
    ("copy_path", None, "Copy path"),
    ("copy_relative_path", None, "Copy relative path"),
    ("copy_branch", None, "Copy branch name"),
    ("dock_editor", None, "Dock editor beside Kete"),
    ("mark_seen", Some(AppliesTo::Worktree), "Mark all seen"),
    ("mark_seen", Some(AppliesTo::Repo), "Mark all seen"),
    ("mark_seen", Some(AppliesTo::Global), "Mark all seen"),
    ("archive_done", None, "Archive done sessions\u{2026}"),
    ("wake_schedule", Some(AppliesTo::Global), "Whakaoho\u{2026}"),
    ("wake_schedule", None, "Wake-up schedule\u{2026}"),
    (
        "scope_account",
        Some(AppliesTo::Account),
        "Show its sessions",
    ),
    (
        "accounts",
        Some(AppliesTo::Account),
        "Account details\u{2026}",
    ),
];

/// The words a menu row for verb `id` on a thing of kind `on` says.
pub fn menu_label(id: &str, on: AppliesTo) -> &'static str {
    let mine = |kind: Option<AppliesTo>| {
        MENU_LABELS
            .iter()
            .find(|(i, k, _)| *i == id && *k == kind)
            .map(|(_, _, l)| *l)
    };
    mine(Some(on))
        .or_else(|| mine(None))
        .or_else(|| verbs_label(id))
        .unwrap_or("")
}

fn verbs_label(id: &str) -> Option<&'static str> {
    ACTIONS
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.label)
        .or_else(|| DESKTOP_ONLY.iter().find(|d| d.id == id).map(|d| d.label))
}

/// Each kind's menu, in order: what a right-click (or the row's `⋯`) on
/// one lists, in both apps. Each app leaves out a row it has no verb
/// for, or one that means nothing on this thing now (Unarchive on a live
/// session, Delete worktree on a root checkout). A live session's or a
/// terminal's menu ends on its checkout's, as one `Worktree <branch> ▸`
/// row less the rows the card already has. The order runs through the
/// [`MenuSection`]s, so a menu reads do, go, open, copy, change,
/// organise, then what takes the thing away; [`MENU_GROUPS`] folds a run
/// of rows into one submenu row. The matrix behind it, with each row's
/// state rules: docs/superpowers/specs/2026-10-16-kete-054-menu-matrix.md.
pub const MENUS: &[(AppliesTo, &[&str])] = &[
    (
        AppliesTo::Session,
        &[
            "activate",
            "follow_up",
            "ask_aside",
            "git_diff",
            "open_pull_request",
            "create_pull_request",
            "merge_pull_request",
            "comment_pull_request",
            "open_issue",
            "open_in_vscode",
            "open_in_visual_studio",
            "reveal_in_explorer",
            "open_in_browser",
            "dock_editor",
            "copy_path",
            "copy_branch",
            "copy_pr_url",
            "mark_seen",
            "restart",
            "duplicate_session",
            "rename",
            "pin_orchestrator",
            "move_session",
            "move_to_account",
            "reorder_up",
            "reorder_down",
            "unarchive",
            "archive",
            "delete",
        ],
    ),
    (
        AppliesTo::Terminal,
        &["activate", "copy_path", "rename", "delete"],
    ),
    (
        AppliesTo::Worktree,
        &[
            "new",
            "quick_prompt",
            "new_terminal",
            "toggle_run",
            "git_diff",
            "open_pull_request",
            "create_pull_request",
            "merge_pull_request",
            "comment_pull_request",
            "focus_scope",
            "open_repo",
            "open_worktree",
            "open_in_vscode",
            "open_in_visual_studio",
            "reveal_in_explorer",
            "open_in_browser",
            "dock_editor",
            "copy_path",
            "copy_branch",
            "copy_pr_url",
            "mark_seen",
            "switch_branch",
            "reorder_up",
            "reorder_down",
            "archive_done",
            "delete_all",
            "delete_worktree",
        ],
    ),
    (
        AppliesTo::Repo,
        &[
            "quick_prompt",
            "new_worktree",
            "toggle_run",
            "focus_scope",
            "open_repo",
            "pull_requests",
            "issues",
            "azure_devops",
            "refresh_pull_requests",
            "open_worktree",
            "open_in_vscode",
            "open_in_visual_studio",
            "reveal_in_explorer",
            "open_in_browser",
            "dock_editor",
            "copy_path",
            "mark_seen",
            "switch_branch",
            "rename",
            "reorder_up",
            "reorder_down",
            "archive_done",
            "sweep_worktrees",
            "remove_project",
        ],
    ),
    // A file's and a folder's rows in a worktree (a diff's file list, the
    // tree, a finder); each app adds its own local rows after these (Diff
    // this file, Stage, Mark reviewed, Expand all). Short menus: flat,
    // never folded.
    (
        AppliesTo::Folder,
        &[
            "open_in_vscode",
            "reveal_in_explorer",
            "copy_path",
            "copy_relative_path",
        ],
    ),
    (
        AppliesTo::File,
        &[
            "open_file_in_kete",
            "open_in_vscode",
            "open_in_visual_studio",
            "open_with_default",
            "reveal_in_explorer",
            "copy_path",
            "copy_relative_path",
        ],
    ),
    // An account's own rows (Use as default, Switch on or off, Log in
    // again…) are each app's, placed by the matrix among these.
    (
        AppliesTo::Account,
        &["scope_account", "accounts", "wake_account", "wake_schedule"],
    ),
    // The background: a right-click on no row (the overview's, the
    // board's or the panels' empty space), each app's views in View ▸.
    (
        AppliesTo::Global,
        &[
            "quick_prompt",
            "add_project",
            "command_palette",
            "home_overview",
            "home_board",
            "toggle_kanban",
            "toggle_orchestrator_view",
            "accounts",
            "wake_schedule",
            "toggle_archived",
            "settings",
            "mark_seen",
            "critters",
            "archive_done",
        ],
    ),
];

/// Where a menu row sits, top to bottom. Every kind's [`MENUS`] runs
/// through these in order; the desktop draws a rule where the section
/// changes (the terminal app has no rules), so what takes the thing away
/// is always last, behind one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuSection {
    /// The thing's own verb: open it, reply, start something in it.
    Act,
    /// Look at what it is connected to: its diff, its pull request, the
    /// scope narrowed to it, its remote.
    Go,
    /// Open it in another app: an editor, Explorer, a browser.
    Open,
    Copy,
    /// Change it in place: restart, rename, its account, its branch.
    Change,
    /// Move it among the rest.
    Organise,
    /// Take it away: archive, delete, remove.
    Away,
}

/// Each menu verb's [`MenuSection`]; a row with a kind says it on that
/// kind only, ahead of the row without one ([`section_of`]).
pub const MENU_SECTIONS: &[(&str, Option<AppliesTo>, MenuSection)] = &[
    ("activate", None, MenuSection::Act),
    ("follow_up", None, MenuSection::Act),
    ("ask_aside", None, MenuSection::Act),
    ("new", None, MenuSection::Act),
    ("quick_prompt", None, MenuSection::Act),
    ("new_terminal", None, MenuSection::Act),
    ("toggle_run", None, MenuSection::Act),
    ("new_worktree", None, MenuSection::Act),
    ("add_project", None, MenuSection::Act),
    ("command_palette", None, MenuSection::Act),
    ("open_file_in_kete", None, MenuSection::Act),
    ("git_diff", None, MenuSection::Go),
    ("open_pull_request", None, MenuSection::Go),
    ("create_pull_request", None, MenuSection::Go),
    ("merge_pull_request", None, MenuSection::Go),
    ("comment_pull_request", None, MenuSection::Go),
    ("open_issue", None, MenuSection::Go),
    ("focus_scope", None, MenuSection::Go),
    ("open_repo", None, MenuSection::Go),
    ("pull_requests", None, MenuSection::Go),
    ("issues", None, MenuSection::Go),
    ("azure_devops", None, MenuSection::Go),
    ("refresh_pull_requests", None, MenuSection::Go),
    ("scope_account", None, MenuSection::Go),
    ("accounts", None, MenuSection::Go),
    ("home_overview", None, MenuSection::Go),
    ("home_board", None, MenuSection::Go),
    ("toggle_kanban", None, MenuSection::Go),
    ("toggle_orchestrator_view", None, MenuSection::Go),
    ("toggle_archived", None, MenuSection::Go),
    ("settings", None, MenuSection::Go),
    // The view on the background; beside Wake up now on an account.
    (
        "wake_schedule",
        Some(AppliesTo::Account),
        MenuSection::Change,
    ),
    ("wake_schedule", None, MenuSection::Go),
    ("open_worktree", None, MenuSection::Open),
    ("open_in_vscode", None, MenuSection::Open),
    ("open_in_visual_studio", None, MenuSection::Open),
    ("reveal_in_explorer", None, MenuSection::Open),
    ("open_in_browser", None, MenuSection::Open),
    ("open_with_default", None, MenuSection::Open),
    ("dock_editor", None, MenuSection::Open),
    ("copy_path", None, MenuSection::Copy),
    ("copy_relative_path", None, MenuSection::Copy),
    ("copy_branch", None, MenuSection::Copy),
    ("copy_pr_url", None, MenuSection::Copy),
    ("mark_seen", None, MenuSection::Change),
    ("restart", None, MenuSection::Change),
    ("duplicate_session", None, MenuSection::Change),
    ("rename", None, MenuSection::Change),
    ("pin_orchestrator", None, MenuSection::Change),
    ("switch_branch", None, MenuSection::Change),
    ("wake_account", None, MenuSection::Change),
    ("critters", None, MenuSection::Change),
    ("move_session", None, MenuSection::Organise),
    // A fresh restart on another login: moved, as far as the menus go.
    ("move_to_account", None, MenuSection::Organise),
    ("reorder_up", None, MenuSection::Organise),
    ("reorder_down", None, MenuSection::Organise),
    ("unarchive", None, MenuSection::Away),
    ("archive", None, MenuSection::Away),
    ("delete", None, MenuSection::Away),
    ("archive_done", None, MenuSection::Away),
    ("delete_all", None, MenuSection::Away),
    ("delete_worktree", None, MenuSection::Away),
    ("sweep_worktrees", None, MenuSection::Away),
    ("remove_project", None, MenuSection::Away),
];

/// The section verb `id`'s row sits in on a thing of kind `on`.
pub fn section_of(id: &str, on: AppliesTo) -> Option<MenuSection> {
    let mine = |kind: Option<AppliesTo>| {
        MENU_SECTIONS
            .iter()
            .find(|(i, k, _)| *i == id && *k == kind)
            .map(|(_, _, s)| *s)
    };
    mine(Some(on)).or_else(|| mine(None))
}

/// A submenu: the run of a menu's rows it folds into one `title ▸` row,
/// on the kinds in `on`. It folds where two or more of its rows mean
/// something on the thing now, at the place of the first; one alone
/// stays a plain row in its whole words ([`menu_label`]). Inside, each
/// row says its short word. Both apps fold the same way, and a menu's
/// type-ahead searches the folded rows too.
#[derive(Debug, Serialize)]
pub struct MenuGroup {
    pub id: &'static str,
    pub title: &'static str,
    pub on: &'static [AppliesTo],
    /// Verb id and its word inside the submenu, in [`MENUS`] order.
    pub rows: &'static [(&'static str, &'static str)],
}

pub const MENU_GROUPS: &[MenuGroup] = &[
    MenuGroup {
        id: "pull_request",
        title: "Pull request",
        on: &[AppliesTo::Session, AppliesTo::Worktree],
        rows: &[
            ("open_pull_request", "Open in browser"),
            ("create_pull_request", "Create\u{2026}"),
            ("merge_pull_request", "Merge\u{2026}"),
            ("comment_pull_request", "Comment\u{2026}"),
            ("open_issue", "Open the issue"),
        ],
    },
    // A repo's remote: its page, its pull requests and issues (GitHub)
    // or its Azure DevOps lists. An app that knows the host may title it
    // GitHub or Azure DevOps.
    MenuGroup {
        id: "remote",
        title: "Remote",
        on: &[AppliesTo::Repo],
        rows: &[
            ("open_repo", "Open in browser"),
            ("pull_requests", "Pull requests\u{2026}"),
            ("issues", "Issues\u{2026}"),
            ("azure_devops", "Azure DevOps\u{2026}"),
            ("refresh_pull_requests", "Refresh pull requests"),
        ],
    },
    MenuGroup {
        id: "open_in",
        title: "Open in",
        on: &[AppliesTo::Session, AppliesTo::Worktree, AppliesTo::Repo],
        rows: &[
            ("open_worktree", "Open command"),
            ("open_in_vscode", "VS Code"),
            ("open_in_visual_studio", "Visual Studio"),
            ("reveal_in_explorer", "Explorer"),
            ("open_in_browser", "Browser"),
            ("dock_editor", "Dock editor beside Kete"),
        ],
    },
    MenuGroup {
        id: "copy",
        title: "Copy",
        on: &[AppliesTo::Session, AppliesTo::Worktree],
        rows: &[
            ("copy_path", "Path"),
            ("copy_branch", "Branch name"),
            ("copy_pr_url", "Pull request link"),
        ],
    },
    MenuGroup {
        id: "move",
        title: "Move",
        on: &[AppliesTo::Session, AppliesTo::Worktree, AppliesTo::Repo],
        rows: &[
            ("move_session", "To another worktree\u{2026}"),
            ("move_to_account", "To another account\u{2026}"),
            ("reorder_up", "Up"),
            ("reorder_down", "Down"),
        ],
    },
    MenuGroup {
        id: "view",
        title: "View",
        on: &[AppliesTo::Global],
        rows: &[
            ("home_overview", "Overview"),
            ("home_board", "Board"),
            ("toggle_kanban", "Board or bands"),
            ("toggle_orchestrator_view", "Orchestrator"),
            ("accounts", "Accounts"),
            ("wake_schedule", "Whakaoho"),
            ("toggle_archived", "Archived sessions"),
        ],
    },
];

/// The submenu verb `id`'s row folds into on a thing of kind `on`, and
/// the row's word there.
pub fn group_of(id: &str, on: AppliesTo) -> Option<(&'static MenuGroup, &'static str)> {
    MENU_GROUPS
        .iter()
        .filter(|g| g.on.contains(&on))
        .find_map(|g| {
            g.rows
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, word)| (g, *word))
        })
}

/// The verbs a footer and the verb bar lead with, per kind, best first:
/// each app shows the first four it can run on the selection, then `?`
/// for the rest.
pub const PRIMARY: &[(AppliesTo, &[&str])] = &[
    (
        AppliesTo::Session,
        &[
            "activate",
            "follow_up",
            "git_diff",
            "archive",
            "unarchive",
            "rename",
            "restart",
            "delete",
        ],
    ),
    (AppliesTo::Terminal, &["activate", "rename", "delete"]),
    (
        AppliesTo::Worktree,
        &[
            "new",
            "quick_prompt",
            "new_terminal",
            "git_diff",
            "open_worktree",
            "toggle_run",
        ],
    ),
    (
        AppliesTo::Repo,
        &[
            "quick_prompt",
            "new_worktree",
            "switch_branch",
            "refresh_pull_requests",
            "rename",
        ],
    ),
    (AppliesTo::Account, &["accounts"]),
    (
        AppliesTo::Global,
        &[
            "quick_prompt",
            "command_palette",
            "toggle_kanban",
            "settings",
        ],
    ),
];

/// How many verbs a footer or the verb bar shows before `? more`.
pub const PRIMARY_SHOWN: usize = 4;

/// A kind's menu ([`MENUS`]), empty for a kind with none.
pub fn menu_of(kind: AppliesTo) -> &'static [&'static str] {
    MENUS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

/// A kind's leading verbs ([`PRIMARY`]).
pub fn primary_of(kind: AppliesTo) -> &'static [&'static str] {
    PRIMARY
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

/// The TUI's row for a verb id, if it has one.
pub fn spec_by_id(id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|s| s.id == id)
}

/// One registry row, either table's, as the window reads it.
#[derive(Debug, Serialize)]
pub struct Verb {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub group: &'static str,
    pub word: &'static str,
    pub destructive: bool,
    pub applies_to: &'static [AppliesTo],
    pub works_in: &'static [WorksIn],
    pub surface: Surface,
    /// The TUI's default chords, in its own spelling; empty on a desktop
    /// verb.
    pub tui: &'static [&'static str],
    pub tui_scope: Option<Scope>,
    pub desktop: &'static [DeskKey],
}

/// Every verb: the TUI's [`ACTIONS`], then [`DESKTOP_ONLY`].
pub fn verbs() -> Vec<Verb> {
    let tui = ACTIONS.iter().map(|s| Verb {
        id: s.id,
        label: s.label,
        hint: s.hint,
        group: s.group,
        word: s.word,
        destructive: s.destructive,
        applies_to: s.applies_to,
        works_in: s.works_in,
        surface: s.surface,
        tui: s.defaults,
        tui_scope: Some(s.scope),
        desktop: s.desktop,
    });
    let desk = DESKTOP_ONLY.iter().map(|d| Verb {
        id: d.id,
        label: d.label,
        hint: d.hint,
        group: d.group,
        word: d.word,
        destructive: d.destructive,
        applies_to: d.applies_to,
        works_in: d.works_in,
        surface: Surface::Desktop,
        tui: &[],
        tui_scope: None,
        desktop: d.desktop,
    });
    tui.chain(desk).collect()
}

/// The desktop's key rows in [`DESKTOP_ORDER`], each with its verb's id.
pub fn desktop_keys() -> Vec<(&'static str, &'static DeskKey)> {
    let all: Vec<(&'static str, &'static DeskKey)> = ACTIONS
        .iter()
        .flat_map(|s| s.desktop.iter().map(move |k| (s.id, k)))
        .chain(
            DESKTOP_ONLY
                .iter()
                .flat_map(|d| d.desktop.iter().map(move |k| (d.id, k))),
        )
        .collect();
    DESKTOP_ORDER
        .iter()
        .filter_map(|id| all.iter().find(|(_, k)| k.id == *id).copied())
        .collect()
}

// ---- the desktop's overrides ----

/// The desktop window's overrides share the config file's `keybindings`
/// object with the TUI's, as flat string entries under this prefix
/// (`"desktop.palette": "ctrl+k, ctrl+shift+p"`): a nested object would
/// fail to parse as the map of strings older builds read, and drop the
/// TUI's overrides with it. An empty string is a deliberate unbind, as
/// in the TUI.
pub const DESKTOP_PREFIX: &str = "desktop.";

/// The desktop's entries of a `keybindings` map: key id to its chords.
pub fn desktop_overrides(keybindings: &BTreeMap<String, String>) -> BTreeMap<String, Vec<String>> {
    keybindings
        .iter()
        .filter_map(|(k, v)| {
            let id = k.strip_prefix(DESKTOP_PREFIX)?;
            Some((id.to_string(), split_chords(v)))
        })
        .collect()
}

/// A stored chord list, `"ctrl+k, ctrl+shift+p"`, as its chords. A lone
/// `,` is a chord (prev_attention's): an empty part after one is it.
pub fn split_chords(list: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut part = String::new();
    let mut chars = list.chars().peekable();
    while let Some(c) = chars.next() {
        // `,` ends a chord unless it is the chord's key: the first thing
        // in the part, or after a `+`.
        let key = c == ',' && (part.trim().is_empty() || part.ends_with('+'));
        if c == ',' && !key {
            out.push(std::mem::take(&mut part));
            continue;
        }
        part.push(c);
        if key {
            // Its own separator, if one follows.
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            if chars.peek() == Some(&',') {
                chars.next();
                out.push(std::mem::take(&mut part));
            }
        }
    }
    out.push(part);
    out.into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

/// `tui` (the TUI keymap's overrides) as the whole of the TUI's part of
/// `keybindings`, the desktop's entries kept as they were.
pub fn merge_tui_overrides(
    keybindings: &mut BTreeMap<String, String>,
    tui: BTreeMap<String, String>,
) {
    keybindings.retain(|k, _| k.starts_with(DESKTOP_PREFIX));
    keybindings.extend(tui);
}

/// Whether `chord` may go on a key row of `scope`. A row that counts while
/// a field or terminal has the keys (`anywhere`, `fields`) takes only a
/// chord typing never sends: one with Ctrl or Alt, or an F key. Shift
/// alone is a capital letter. The window checks the same before it asks.
pub fn typing_safe(scope: DeskScope, chord: &str) -> bool {
    if !matches!(scope, DeskScope::Anywhere | DeskScope::Fields) {
        return true;
    }
    let key = chord.rsplit('+').next().unwrap_or_default();
    let fkey = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    let mods: Vec<&str> = chord.split('+').collect();
    let held = mods[..mods.len().saturating_sub(1)]
        .iter()
        .any(|m| *m == "ctrl" || *m == "alt");
    held || fkey
}

/// Set (`Some`) or clear back to its default (`None`) one desktop key's
/// override. Refuses an unknown id, a home view's walking keys (those
/// views handle them themselves) and a chord typing would send on a row
/// that counts while typing ([`typing_safe`]).
pub fn set_desktop_override(
    keybindings: &mut BTreeMap<String, String>,
    id: &str,
    chords: Option<&[String]>,
) -> Result<(), String> {
    let (_, key) = desktop_keys()
        .into_iter()
        .find(|(_, k)| k.id == id)
        .ok_or_else(|| format!("no desktop key {id:?}"))?;
    if matches!(
        key.scope,
        DeskScope::Overview | DeskScope::Board | DeskScope::Review
    ) {
        return Err(format!("{} is fixed: the view handles it", key.label));
    }
    let entry = format!("{DESKTOP_PREFIX}{id}");
    let Some(chords) = chords else {
        keybindings.remove(&entry);
        return Ok(());
    };
    let mut list: Vec<String> = Vec::new();
    for c in chords {
        let c = c.trim().to_ascii_lowercase();
        if !c.is_empty() && !typing_safe(key.scope, &c) {
            return Err(format!(
                "{c} would fire while typing: {} counts in fields, so its keys take Ctrl, Alt or an F key",
                key.label
            ));
        }
        if !c.is_empty() && !list.contains(&c) {
            list.push(c);
        }
    }
    if list
        .iter()
        .map(String::as_str)
        .eq(key.chords.iter().copied())
    {
        keybindings.remove(&entry);
    } else {
        keybindings.insert(entry, list.join(", "));
    }
    Ok(())
}

// ---- the window's copy ----

/// Where the window's copy of this table lives, from this crate.
pub const TS_PATH: &str = "../../desktop/src/lib/actions.gen.ts";

/// A string as a TS literal in plain ASCII (the window's sources must be:
/// `desktop/scripts/check-encoding.mjs`): JSON, with every other character
/// a `\u` escape.
fn ts_str(s: &str) -> String {
    let json = serde_json::to_string(s).expect("a string serializes");
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

fn ts_enum<T: Serialize>(v: &T) -> String {
    let value = serde_json::to_value(v).expect("a variant serializes");
    ts_str(value.as_str().unwrap_or_default())
}

fn ts_list<T: Serialize>(items: &[T]) -> String {
    let items: Vec<String> = items.iter().map(ts_enum).collect();
    format!("[{}]", items.join(", "))
}

fn ts_strs(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|s| ts_str(s)).collect();
    format!("[{}]", items.join(", "))
}

fn ts_union(name: &str, items: &[String]) -> String {
    let mut out = format!("export type {name} =\n");
    for (i, item) in items.iter().enumerate() {
        let end = if i + 1 == items.len() { ";" } else { "" };
        out.push_str(&format!("  | {item}{end}\n"));
    }
    out
}

const TS_HEAD: &str = "\
// Generated from crates/nebula-config/src/actions.rs: do not edit. The
// nebula-config test actions_gen_ts_is_current fails when this file and
// that table differ; KETE_BLESS=1 cargo test -p nebula-config actions_gen
// rewrites it.

";

const TS_TYPES: &str = "\
/** One verb. `tui` is the terminal app's default chords in its own
 *  spelling; `desktop` names the window's key rows for it. */
export interface ActionRow {
  id: ActionId;
  label: string;
  hint: string;
  group: string;
  word: string;
  destructive: boolean;
  appliesTo: readonly AppliesTo[];
  worksIn: readonly WorksIn[];
  surface: Surface;
  tui: readonly string[];
  desktop: readonly DeskKeyId[];
}

/** One of the window's key rows: its default chords, spelled as
 *  `chordOf` spells a key event, and where they count. */
export interface DeskKey {
  id: DeskKeyId;
  action: ActionId;
  label: string;
  group: DeskGroup;
  scope: DeskScope;
  editorOwns: boolean;
  chords: readonly string[];
}

";

/// `desktop/src/lib/actions.gen.ts`: the registry as the window imports it.
/// The `actions_gen_ts_is_current` test fails when the committed file and
/// this differ; `KETE_BLESS=1` rewrites it.
pub fn render_ts() -> String {
    let verbs = verbs();
    let keys = desktop_keys();
    let mut groups: Vec<&str> = Vec::new();
    for (_, k) in &keys {
        if !groups.contains(&k.group) {
            groups.push(k.group);
        }
    }
    let ids: Vec<String> = verbs.iter().map(|v| ts_str(v.id)).collect();
    let key_ids: Vec<String> = keys.iter().map(|(_, k)| ts_str(k.id)).collect();
    let group_names: Vec<String> = groups.iter().map(|g| ts_str(g)).collect();
    let mut out = String::from(TS_HEAD);
    out.push_str(&ts_union("ActionId", &ids));
    out.push('\n');
    out.push_str(&ts_union("DeskKeyId", &key_ids));
    out.push('\n');
    out.push_str(&ts_union("DeskGroup", &group_names));
    out.push('\n');
    let applies = [
        AppliesTo::Global,
        AppliesTo::Repo,
        AppliesTo::Worktree,
        AppliesTo::Session,
        AppliesTo::Terminal,
        AppliesTo::Account,
        AppliesTo::File,
        AppliesTo::Folder,
    ];
    let works = [
        WorksIn::Grid,
        WorksIn::Board,
        WorksIn::Pane,
        WorksIn::Diff,
        WorksIn::Overlay,
    ];
    let surfaces = [Surface::Both, Surface::Tui, Surface::Desktop];
    let scopes = [
        DeskScope::Anywhere,
        DeskScope::Fields,
        DeskScope::Global,
        DeskScope::Row,
        DeskScope::Overview,
        DeskScope::Board,
        DeskScope::Review,
    ];
    let alt = |names: Vec<String>| names.join(" | ");
    out.push_str(&format!(
        "export type AppliesTo = {};\nexport type WorksIn = {};\nexport type Surface = {};\nexport type DeskScope = {};\n\n",
        alt(applies.iter().map(ts_enum).collect()),
        alt(works.iter().map(ts_enum).collect()),
        alt(surfaces.iter().map(ts_enum).collect()),
        alt(scopes.iter().map(ts_enum).collect()),
    ));
    out.push_str(TS_TYPES);
    out.push_str("export const ACTIONS: readonly ActionRow[] = [\n");
    for v in &verbs {
        let desk: Vec<&str> = v.desktop.iter().map(|k| k.id).collect();
        out.push_str("  {\n");
        out.push_str(&format!("    id: {},\n", ts_str(v.id)));
        out.push_str(&format!("    label: {},\n", ts_str(v.label)));
        out.push_str(&format!("    hint: {},\n", ts_str(v.hint)));
        out.push_str(&format!("    group: {},\n", ts_str(v.group)));
        out.push_str(&format!("    word: {},\n", ts_str(v.word)));
        out.push_str(&format!("    destructive: {},\n", v.destructive));
        out.push_str(&format!("    appliesTo: {},\n", ts_list(v.applies_to)));
        out.push_str(&format!("    worksIn: {},\n", ts_list(v.works_in)));
        out.push_str(&format!("    surface: {},\n", ts_enum(&v.surface)));
        out.push_str(&format!("    tui: {},\n", ts_strs(v.tui)));
        out.push_str(&format!("    desktop: {},\n", ts_strs(&desk)));
        out.push_str("  },\n");
    }
    out.push_str("];\n\n/** The window's key rows, in its help's order. */\n");
    out.push_str("export const DESK_KEYS: readonly DeskKey[] = [\n");
    for (action, k) in &keys {
        out.push_str(&format!(
            "  {{ id: {}, action: {}, label: {}, group: {}, scope: {}, editorOwns: {}, chords: {} }},\n",
            ts_str(k.id),
            ts_str(action),
            ts_str(k.label),
            ts_str(k.group),
            ts_enum(&k.scope),
            k.editor_owns,
            ts_strs(k.chords),
        ));
    }
    out.push_str("];\n\n/** A verb's words in a menu, the verbs menu and the verb bar; `on`: that kind only. */\n");
    out.push_str(
        "export const MENU_LABELS: readonly { id: ActionId; on: AppliesTo | null; label: string }[] = [\n",
    );
    for (id, on, label) in MENU_LABELS {
        let on = on.map(|k| ts_enum(&k)).unwrap_or_else(|| "null".into());
        out.push_str(&format!(
            "  {{ id: {}, on: {on}, label: {} }},\n",
            ts_str(id),
            ts_str(label)
        ));
    }
    out.push_str("];\n");
    let table = |name: &str, doc: &str, rows: &[(AppliesTo, &[&str])]| {
        let mut s = format!(
            "\n/** {doc} */\nexport const {name}: Readonly<Partial<Record<AppliesTo, readonly ActionId[]>>> = {{\n"
        );
        for (kind, ids) in rows {
            let key = serde_json::to_value(kind).expect("a variant serializes");
            s.push_str(&format!(
                "  {}: {},\n",
                key.as_str().unwrap_or_default(),
                ts_strs(ids)
            ));
        }
        s.push_str("};\n");
        s
    };
    out.push_str(&table(
        "MENUS",
        "Each kind's menu, in order: a right-click's rows in both apps.",
        MENUS,
    ));
    out.push_str(&table(
        "PRIMARY",
        "The verbs a footer and the verb bar lead with, per kind, best first.",
        PRIMARY,
    ));
    out.push_str(&format!(
        "\n/** How many verbs the verb bar shows before `? more`. */\nexport const PRIMARY_SHOWN = {PRIMARY_SHOWN};\n"
    ));
    out.push_str(&render_menu_layout_ts());
    out.push_str(&render_learn_ts());
    out
}

/// The menus' sections and submenus ([`MENU_SECTIONS`], [`MENU_GROUPS`])
/// as the window imports them.
fn render_menu_layout_ts() -> String {
    let sections = [
        MenuSection::Act,
        MenuSection::Go,
        MenuSection::Open,
        MenuSection::Copy,
        MenuSection::Change,
        MenuSection::Organise,
        MenuSection::Away,
    ];
    let names: Vec<String> = sections.iter().map(ts_enum).collect();
    let mut out = format!(
        "\n/** Where a menu row sits, top to bottom; the window draws a rule where it changes. */\nexport type MenuSection = {};\n",
        names.join(" | ")
    );
    out.push_str(&format!(
        "/** The sections in their order. */\nexport const MENU_SECTION_ORDER: readonly MenuSection[] = {};\n",
        ts_list(&sections)
    ));
    out.push_str("\n/** Each menu verb's section; `on`: that kind only. */\n");
    out.push_str(
        "export const MENU_SECTIONS: readonly { id: ActionId; on: AppliesTo | null; section: MenuSection }[] = [\n",
    );
    for (id, on, section) in MENU_SECTIONS {
        let on = on.map(|k| ts_enum(&k)).unwrap_or_else(|| "null".into());
        out.push_str(&format!(
            "  {{ id: {}, on: {on}, section: {} }},\n",
            ts_str(id),
            ts_enum(section)
        ));
    }
    out.push_str("];\n");
    out.push_str(
        "\n/** A submenu: on the kinds in `on`, two or more of its rows that mean something now fold into one `title` row at the first one's place; one alone stays a plain row. `label` is a row's word inside it. */\n",
    );
    out.push_str("export interface MenuGroup {\n  id: string;\n  title: string;\n  on: readonly AppliesTo[];\n  rows: readonly { id: ActionId; label: string }[];\n}\n");
    out.push_str("export const MENU_GROUPS: readonly MenuGroup[] = [\n");
    for g in MENU_GROUPS {
        let rows: Vec<String> = g
            .rows
            .iter()
            .map(|(id, label)| format!("{{ id: {}, label: {} }}", ts_str(id), ts_str(label)))
            .collect();
        out.push_str(&format!(
            "  {{ id: {}, title: {}, on: {}, rows: [{}] }},\n",
            ts_str(g.id),
            ts_str(g.title),
            ts_list(g.on),
            rows.join(", ")
        ));
    }
    out.push_str("];\n");
    out
}

/// The LEARN IN PLACE copy ([`crate::learn`]) as the window imports it:
/// the tips, the What's new page and the empty states.
fn render_learn_ts() -> String {
    use crate::learn::{EMPTY, KEYS_CHANGED, TIPS, WHATS_NEW};
    let mut out = String::from(
        "\n/** First-use tips (nebula-config learn.rs): `{verb}` / `{verb|words}` name a key. */\n",
    );
    out.push_str("export const TIPS: readonly { id: string; text: string }[] = [\n");
    for t in TIPS {
        out.push_str(&format!(
            "  {{ id: {}, text: {} }},\n",
            ts_str(t.id),
            ts_str(t.text)
        ));
    }
    out.push_str("];\n\n/** The What's new page, newest release first. */\n");
    out.push_str(
        "export const WHATS_NEW: readonly { version: string; title: string; items: readonly string[] }[] = [\n",
    );
    for r in WHATS_NEW {
        out.push_str(&format!(
            "  {{ version: {}, title: {}, items: {} }},\n",
            ts_str(r.version),
            ts_str(r.title),
            ts_strs(r.items)
        ));
    }
    out.push_str("];\n\n/** The What's new page's keys-changed table. */\n");
    out.push_str(
        "export const KEYS_CHANGED: readonly { verb: string; tui: string; desktop: string }[] = [\n",
    );
    for k in KEYS_CHANGED {
        out.push_str(&format!(
            "  {{ verb: {}, tui: {}, desktop: {} }},\n",
            ts_str(k.verb),
            ts_str(k.tui),
            ts_str(k.desktop)
        ));
    }
    out.push_str(
        "];\n\n/** What an empty lane, list or view says, and the verb that fills it. */\n",
    );
    out.push_str(
        "export const EMPTY: readonly { id: string; text: string; verb: ActionId | null; does: string }[] = [\n",
    );
    for e in EMPTY {
        let verb = e.verb.map(ts_str).unwrap_or_else(|| "null".into());
        out.push_str(&format!(
            "  {{ id: {}, text: {}, verb: {verb}, does: {} }},\n",
            ts_str(e.id),
            ts_str(e.text),
            ts_str(e.does)
        ));
    }
    out.push_str("];\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn ids_are_unique_across_both_tables() {
        let mut seen = BTreeSet::new();
        for v in verbs() {
            assert!(seen.insert(v.id), "verb id {} twice", v.id);
        }
        let mut keys = BTreeSet::new();
        for (_, k) in desktop_keys() {
            assert!(keys.insert(k.id), "desktop key {} twice", k.id);
        }
    }

    #[test]
    fn every_desktop_key_is_in_the_order_once() {
        let mut listed: Vec<&str> = ACTIONS
            .iter()
            .flat_map(|s| s.desktop.iter().map(|k| k.id))
            .chain(
                DESKTOP_ONLY
                    .iter()
                    .flat_map(|d| d.desktop.iter().map(|k| k.id)),
            )
            .collect();
        let mut ordered: Vec<&str> = DESKTOP_ORDER.to_vec();
        listed.sort_unstable();
        ordered.sort_unstable();
        assert_eq!(ordered, listed);
        assert_eq!(desktop_keys().len(), DESKTOP_ORDER.len());
    }

    #[test]
    fn every_row_says_what_it_is() {
        for s in ACTIONS {
            match s.surface {
                Surface::Tui => assert!(s.desktop.is_empty(), "{} is TUI-only", s.id),
                Surface::Both => {}
                Surface::Desktop => panic!("{} is in the TUI's table", s.id),
            }
            assert!(!s.word.is_empty(), "{}", s.id);
            assert!(
                !s.applies_to.is_empty() && !s.works_in.is_empty(),
                "{}",
                s.id
            );
        }
        for d in DESKTOP_ONLY {
            assert!(!d.desktop.is_empty(), "{} has no key", d.id);
            assert!(!d.word.is_empty(), "{}", d.id);
            assert!(
                !d.applies_to.is_empty() && !d.works_in.is_empty(),
                "{}",
                d.id
            );
        }
    }

    #[test]
    fn the_tui_table_keeps_its_ids() {
        // Users' overrides are keyed by these: a rename strands them. Some
        // read as the desktop's other verbs (`palette` is the TUI's fuzzy
        // jump, the window's Ctrl+K is `command_palette`; `move_up` walks
        // the cursor where the window's reorders, `reorder_up`): the
        // window's key ids are their own namespace.
        assert_eq!(ACTIONS.len(), 107);
        // The 0.54/0.55 slots, appended in this order and unbound.
        let tail: Vec<&str> = ACTIONS[91..102].iter().map(|s| s.id).collect();
        assert_eq!(
            tail,
            [
                "open_in_vscode",
                "open_in_visual_studio",
                "dock_editor",
                "reveal_in_explorer",
                "open_with_default",
                "open_file_in_kete",
                "copy_path",
                "copy_relative_path",
                "copy_branch",
                "wake_account",
                "critters",
            ]
        );
        // The menu matrix's, after them.
        let matrix: Vec<&str> = ACTIONS[102..106].iter().map(|s| s.id).collect();
        assert_eq!(
            matrix,
            ["mark_seen", "copy_pr_url", "archive_done", "wake_schedule"]
        );
        // 0.56's, after those.
        let sessions: Vec<&str> = ACTIONS[106..].iter().map(|s| s.id).collect();
        assert_eq!(sessions, ["ask_aside"]);
        assert!(ACTIONS[91..]
            .iter()
            .all(|s| s.defaults.is_empty() && s.desktop.is_empty() && !s.destructive));
        for id in [
            "palette",
            "move_up",
            "move_down",
            "delete",
            "quit",
            "reorder_up",
        ] {
            assert!(ACTIONS.iter().any(|s| s.id == id), "{id}");
        }
    }

    /// The one key table both apps share: a conflict resolved toward the
    /// meaning that loses nothing, the old chords kept where harmless.
    #[test]
    fn both_apps_share_the_key_table() {
        let row = |id: &str| verbs().into_iter().find(|v| v.id == id).unwrap();
        let desk = |id: &str| -> Vec<&str> {
            row(id)
                .desktop
                .iter()
                .flat_map(|k| k.chords.iter().copied())
                .collect()
        };
        assert_eq!(row("git_diff").tui, ["d", "g"]);
        assert_eq!(desk("git_diff"), ["ctrl+shift+d", "d"]);
        assert_eq!(row("delete").tui, ["delete"]);
        assert_eq!(desk("delete"), ["delete"]);
        assert_eq!(row("follow_up").tui, ["space"]);
        assert_eq!(desk("follow_up"), ["space"]);
        assert_eq!(desk("list_open"), ["enter"]);
        assert_eq!(row("find_file").tui, ["f"]);
        assert_eq!(desk("palette"), ["f", "/"]);
        assert_eq!(row("refresh_pull_requests").tui, ["shift+r"]);
        assert_eq!(desk("refresh_pull_requests"), ["shift+r"]);
        assert!(desk("restart").is_empty());
        assert_eq!(row("toggle_full_screen").tui, ["z", "ctrl+f"]);
        assert_eq!(desk("toggle_full_screen"), ["ctrl+shift+m", "z"]);
        assert_eq!(row("settings").tui, ["s", "ctrl+,"]);
        assert_eq!(desk("settings"), ["ctrl+,"]);
        assert_eq!(desk("command_palette")[0], "ctrl+k");
        assert_eq!(row("nav_back").tui[0], "alt+left");
        assert_eq!(desk("nav_back"), ["alt+arrowleft"]);
        for n in 1..=9 {
            let id = format!("project_tab_{n}");
            assert_eq!(row(&id).tui[0], n.to_string());
            assert_eq!(desk(&id), [n.to_string()]);
        }
        for id in ["go_home", "go_session", "go_diff"] {
            assert!(desk(id).is_empty(), "{id}");
        }
        assert_eq!(desk("toggle_orchestrator_view"), ["f10"]);
        // No destructive verb keeps a bare letter in either app.
        for v in verbs() {
            if !v.destructive {
                continue;
            }
            let bare = |c: &&str| c.len() == 1 && c.chars().all(|c| c.is_ascii_alphanumeric());
            assert!(!v.tui.iter().any(bare), "{}: {:?}", v.id, v.tui);
            for k in v.desktop {
                assert!(!k.chords.iter().any(bare), "{}: {:?}", k.id, k.chords);
            }
        }
    }

    /// Every menu, footer and label row names a verb that is there, on a
    /// kind it applies to: a stale id would show an empty row.
    #[test]
    fn menus_name_real_verbs_on_their_kinds() {
        let all = verbs();
        let row = |id: &str| all.iter().find(|v| v.id == id);
        for (kind, ids) in MENUS.iter().chain(PRIMARY) {
            let mut seen = BTreeSet::new();
            for id in *ids {
                assert!(seen.insert(*id), "{id} twice on {kind:?}");
                let v = row(id).unwrap_or_else(|| panic!("{id} is no verb"));
                assert!(
                    v.applies_to.contains(kind),
                    "{id} does not apply to {kind:?}"
                );
            }
        }
        for (id, on, label) in MENU_LABELS {
            let v = row(id).unwrap_or_else(|| panic!("{id} is no verb"));
            assert!(!label.is_empty());
            if let Some(on) = on {
                assert!(v.applies_to.contains(on), "{id} on {on:?}");
            }
        }
        // One word per verb across the apps, the kind's own first.
        assert_eq!(
            menu_label("delete", AppliesTo::Terminal),
            "Close terminal\u{2026}"
        );
        assert_eq!(menu_label("delete", AppliesTo::Session), "Delete\u{2026}");
        assert_eq!(menu_label("activate", AppliesTo::Session), "Open");
        assert_eq!(
            menu_label("open_worktree", AppliesTo::Worktree),
            "Open in editor"
        );
        assert_eq!(
            menu_label("remove_project", AppliesTo::Repo),
            "Remove from Kete\u{2026}"
        );
        assert_eq!(
            menu_label("subagents", AppliesTo::Session),
            "Subagents and session tree"
        );
        // A file's, a folder's and an account's menus: the registry's rows
        // (each app adds its local ones among them).
        assert_eq!(
            menu_of(AppliesTo::Account),
            ["scope_account", "accounts", "wake_account", "wake_schedule"]
        );
        assert_eq!(
            menu_of(AppliesTo::Folder),
            [
                "open_in_vscode",
                "reveal_in_explorer",
                "copy_path",
                "copy_relative_path"
            ]
        );
        assert_eq!(
            menu_of(AppliesTo::File),
            [
                "open_file_in_kete",
                "open_in_vscode",
                "open_in_visual_studio",
                "open_with_default",
                "reveal_in_explorer",
                "copy_path",
                "copy_relative_path"
            ]
        );
        // Standing alone a row says the whole thing; in its submenu, the
        // group's short word.
        assert_eq!(
            menu_label("open_in_vscode", AppliesTo::File),
            "Open in VS Code"
        );
        assert_eq!(
            group_of("open_in_vscode", AppliesTo::Worktree).map(|(g, w)| (g.title, w)),
            Some(("Open in", "VS Code"))
        );
        assert!(group_of("open_in_vscode", AppliesTo::File).is_none());
        assert_eq!(
            menu_label("open_file_in_kete", AppliesTo::File),
            "Open in Kete"
        );
        assert_eq!(menu_label("mark_seen", AppliesTo::Repo), "Mark all seen");
        assert_eq!(menu_label("mark_seen", AppliesTo::Session), "Mark seen");
        // The editor and copy verbs sit after a checkout's diff and pull
        // request, ahead of Switch branch.
        let worktree = menu_of(AppliesTo::Worktree);
        let at = |id: &str| worktree.iter().position(|i| *i == id).unwrap();
        assert!(at("git_diff") < at("open_in_vscode"));
        assert!(at("open_repo") < at("open_in_vscode"));
        assert!(at("copy_branch") < at("switch_branch"));
        assert!(!menu_of(AppliesTo::Repo).contains(&"copy_branch"));
        // A card's Diff and a checkout's pull request page are their own
        // rows, not left to the checkout's.
        assert!(menu_of(AppliesTo::Session).contains(&"git_diff"));
        assert!(menu_of(AppliesTo::Worktree).contains(&"open_pull_request"));
        assert_eq!(
            primary_of(AppliesTo::Session)[..2],
            ["activate", "follow_up"]
        );
    }

    /// Every menu reads do, go, open, copy, change, organise, away, with
    /// what destroys last; each submenu's rows sit together in one section
    /// of every menu it folds on; and folded, no menu runs long.
    #[test]
    fn menus_follow_their_sections_and_fold_into_groups() {
        for (kind, ids) in MENUS {
            let mut last = MenuSection::Act;
            for id in *ids {
                let s = section_of(id, *kind).unwrap_or_else(|| panic!("{id} has no section"));
                assert!(
                    s >= last,
                    "{id} ({s:?}) comes after a {last:?} row on {kind:?}"
                );
                last = s;
                if verbs().iter().any(|v| v.id == *id && v.destructive) {
                    assert_eq!(s, MenuSection::Away, "{id} destroys: it goes last");
                }
            }
        }
        for (id, on, _) in MENU_SECTIONS {
            assert!(
                MENUS
                    .iter()
                    .any(|(k, ids)| ids.contains(id) && on.is_none_or(|o| o == *k)),
                "{id} has a section but is on no menu"
            );
        }
        for g in MENU_GROUPS {
            assert!(g.rows.len() >= 2 && !g.title.is_empty(), "{}", g.id);
            for kind in g.on {
                let ids = menu_of(*kind);
                let at: Vec<usize> = g
                    .rows
                    .iter()
                    .filter_map(|(id, _)| ids.iter().position(|i| i == id))
                    .collect();
                assert!(at.len() >= 2, "{} folds nothing on {kind:?}", g.id);
                // Together, in the group's order, in one section.
                assert!(
                    at.windows(2).all(|w| w[1] == w[0] + 1),
                    "{}'s rows are split on {kind:?}",
                    g.id
                );
                let sections: BTreeSet<MenuSection> = at
                    .iter()
                    .filter_map(|i| section_of(ids[*i], *kind))
                    .collect();
                assert_eq!(sections.len(), 1, "{} straddles a rule on {kind:?}", g.id);
                // No row in two submenus of one menu.
                for (id, _) in g.rows {
                    let homes = MENU_GROUPS
                        .iter()
                        .filter(|o| o.on.contains(kind) && o.rows.iter().any(|(i, _)| i == id))
                        .count();
                    assert!(homes <= 1, "{id} folds two ways on {kind:?}");
                }
            }
        }
        // Folded, at its fullest, no menu runs long. The fullest session is
        // a live card (Unarchive is an archived one's), the fullest
        // checkout a linked one (Switch branch is the root's). A live
        // card's 15 are its registry rows with Mark seen and Make
        // orchestrator both showing (0.56 adds Ask aside); read, with
        // another session pinned, it is 13, and each app adds the
        // checkout's row after (14). A
        // file's and a folder's never fold.
        let top = |kind: AppliesTo| {
            let not_with = match kind {
                AppliesTo::Session => "unarchive",
                AppliesTo::Worktree => "switch_branch",
                _ => "",
            };
            let ids: Vec<&str> = menu_of(kind)
                .iter()
                .copied()
                .filter(|id| *id != not_with)
                .collect();
            let folded: usize = MENU_GROUPS
                .iter()
                .filter(|g| g.on.contains(&kind))
                .map(|g| {
                    g.rows
                        .iter()
                        .filter(|(id, _)| ids.contains(id))
                        .count()
                        .saturating_sub(1)
                })
                .sum();
            ids.len() - folded
        };
        for (kind, most) in [
            (AppliesTo::Session, 15),
            (AppliesTo::Worktree, 15),
            (AppliesTo::Repo, 14),
            (AppliesTo::Terminal, 4),
            (AppliesTo::File, 7),
            (AppliesTo::Folder, 4),
            (AppliesTo::Account, 4),
            (AppliesTo::Global, 8),
        ] {
            assert!(top(kind) <= most, "{kind:?} runs to {} rows", top(kind));
        }
        // The account move is one of Move's rows, not a menu row of its
        // own.
        assert_eq!(
            group_of("move_to_account", AppliesTo::Session).map(|(g, w)| (g.id, w)),
            Some(("move", "To another account\u{2026}"))
        );
        assert_eq!(
            section_of("wake_schedule", AppliesTo::Account),
            Some(MenuSection::Change)
        );
        assert_eq!(
            section_of("wake_schedule", AppliesTo::Global),
            Some(MenuSection::Go)
        );
    }

    /// `?` is the selection's verbs in both apps; the full help moved to
    /// F1 (and the verbs menu's last row), its ids kept.
    #[test]
    fn question_mark_is_the_verbs_menu() {
        let row = |id: &str| verbs().into_iter().find(|v| v.id == id).unwrap();
        assert_eq!(row("verbs").tui, ["?"]);
        assert_eq!(row("help").tui, ["f1"]);
        let desk = |id: &str| {
            desktop_keys()
                .into_iter()
                .find(|(_, k)| k.id == id)
                .unwrap()
                .1
        };
        assert_eq!(desk("verbs").chords, ["?"]);
        assert!(desk("help").chords.is_empty());
        assert_eq!(desk("help_f1").chords, ["f1"]);
        // The menus' verbs the TUI took over from the window keep its ids.
        for id in ["restart", "new_worktree"] {
            assert!(ACTIONS.iter().any(|s| s.id == id), "{id}");
            assert!(!DESKTOP_ONLY.iter().any(|d| d.id == id), "{id}");
        }
    }

    #[test]
    fn chord_lists_split_on_their_commas_only() {
        assert_eq!(
            split_chords("ctrl+k, ctrl+shift+p"),
            vec!["ctrl+k", "ctrl+shift+p"]
        );
        assert_eq!(split_chords(","), vec![","]);
        assert_eq!(split_chords(",, ."), vec![",", "."]);
        assert_eq!(split_chords("ctrl+,, f2"), vec!["ctrl+,", "f2"]);
        assert_eq!(split_chords("f2, ,"), vec!["f2", ","]);
        assert!(split_chords("").is_empty());
    }

    #[test]
    fn desktop_overrides_live_beside_the_tuis() {
        let mut kb: BTreeMap<String, String> = BTreeMap::new();
        kb.insert("quit".into(), "ctrl+q".into());
        let palette = ["Ctrl+J".to_string(), "ctrl+k".to_string()];
        set_desktop_override(&mut kb, "palette", Some(&palette)).unwrap();
        set_desktop_override(&mut kb, "help_f1", Some(&[])).unwrap();
        assert_eq!(kb["desktop.palette"], "ctrl+j, ctrl+k");
        assert_eq!(kb["desktop.help_f1"], "");
        let over = desktop_overrides(&kb);
        assert_eq!(over["palette"], vec!["ctrl+j", "ctrl+k"]);
        assert!(over["help_f1"].is_empty());
        assert!(!over.contains_key("quit"));
        // Back to the defaults drops the entry.
        set_desktop_override(&mut kb, "palette", Some(&["ctrl+k".into()])).unwrap();
        assert!(!kb.contains_key("desktop.palette"));
        set_desktop_override(&mut kb, "help_f1", None).unwrap();
        assert!(!kb.contains_key("desktop.help_f1"));
        assert!(set_desktop_override(&mut kb, "nope", None).is_err());
        assert!(set_desktop_override(&mut kb, "board_lanes", Some(&["x".into()])).is_err());
        // A key that counts while typing takes no bare or shifted letter.
        assert!(set_desktop_override(&mut kb, "cycle_theme", Some(&["t".into()])).is_err());
        assert!(set_desktop_override(&mut kb, "back", Some(&["shift+b".into()])).is_err());
        assert!(set_desktop_override(&mut kb, "back", Some(&["f6".into()])).is_ok());
        assert!(set_desktop_override(&mut kb, "help", Some(&["h".into()])).is_ok());
        assert!(typing_safe(DeskScope::Anywhere, "ctrl+,"));
        assert!(!typing_safe(DeskScope::Fields, "f"));
        for (_, k) in desktop_keys() {
            for c in k.chords {
                assert!(typing_safe(k.scope, c), "{} default {c}", k.id);
            }
        }
        set_desktop_override(&mut kb, "back", None).unwrap();
        set_desktop_override(&mut kb, "help", None).unwrap();
        // A TUI save replaces its own part only.
        set_desktop_override(&mut kb, "rename", Some(&["f2".into(), ",".into()])).unwrap();
        merge_tui_overrides(
            &mut kb,
            BTreeMap::from([("help".to_string(), "f1".to_string())]),
        );
        assert_eq!(kb["desktop.rename"], "f2, ,");
        assert_eq!(desktop_overrides(&kb)["rename"], vec!["f2", ","]);
        assert_eq!(kb["help"], "f1");
        assert!(!kb.contains_key("quit"));
    }

    /// On the settings a 0.48 install left (`crates/nebula/tests/fixtures/
    /// upgrade`): the window's first rebind (Hotkeys, as `desktop_key_set`
    /// saves it) adds its `desktop.` entry and keeps every key the terminal
    /// app had, the window's 0.48 preferences and the accounts; the
    /// terminal app's next Hotkeys save keeps the window's.
    #[test]
    fn a_048_config_keeps_both_apps_keys_through_each_apps_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../nebula/tests/fixtures/upgrade/v0.48/data/config.json"
            ),
            &path,
        )
        .unwrap();
        let before = crate::Config::load_at(&path, &local);
        assert!(desktop_overrides(&before.keybindings).is_empty());
        let palette = ["ctrl+j".to_string()];
        crate::Config::update_in(&path, &local, |c| {
            set_desktop_override(&mut c.keybindings, "palette", Some(&palette)).unwrap()
        })
        .unwrap();
        let after = crate::Config::load_at(&path, &local);
        let mut expect = before.keybindings.clone();
        expect.insert("desktop.palette".into(), "ctrl+j".into());
        assert_eq!(after.keybindings, expect);
        assert_eq!(after.profiles.len(), 2);
        let prefs = crate::desktop::DesktopPrefs::load_at(&path, &local);
        assert_eq!(
            (prefs.theme.as_str(), prefs.home.as_str()),
            ("dark", "board")
        );
        assert_eq!(prefs.collapsed, vec!["alpha".to_string()]);

        let mut tui: BTreeMap<String, String> = before.keybindings.clone();
        tui.insert("help".into(), "f11".into());
        crate::Config::update_in(&path, &local, |c| {
            merge_tui_overrides(&mut c.keybindings, tui)
        })
        .unwrap();
        let kb = crate::Config::load_at(&path, &local).keybindings;
        assert_eq!(kb["desktop.palette"], "ctrl+j");
        assert_eq!(kb["help"], "f11");
        assert_eq!(kb["archive"], "");
    }

    #[test]
    fn the_generated_file_is_plain_ascii() {
        assert!(render_ts().is_ascii());
        assert_eq!(ts_str("\u{2318}P \u{1F600}"), "\"\\u2318P \\ud83d\\ude00\"");
    }

    #[test]
    fn actions_gen_ts_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_PATH);
        let want = render_ts();
        if std::env::var_os("KETE_BLESS").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run with KETE_BLESS=1)", path.display()))
            .replace('\r', "");
        assert!(
            have == want,
            "{} is stale: run KETE_BLESS=1 cargo test -p nebula-config actions_gen",
            path.display()
        );
    }
}
