//! The rebindable actions as data: every key the TUI's grid reacts to,
//! with its stable config id, label, hint, Hotkeys-tab group, scope and
//! default chords. The chord parsing and dispatch (`KeyChord`, `Keymap`)
//! stay in the TUI, which needs a terminal backend for them; this table is
//! here so the desktop window can list the same bindings.

/// Which mode a binding is live in. The same chord may mean different
/// things in each, so conflicts are only conflicts within one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Help,
    Quit,
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
}

/// One positional PROJECT TAB shortcut. `⌘N` is what a browser user
/// reaches for — but it is [`Reach::Blocked`] in Terminal.app and most
/// other emulators, which never encode ⌘ into pty bytes at all. The bare
/// digit is bound alongside it and is the chord that actually fires there;
/// digits are otherwise unbound on the grid.
macro_rules! project_tab_slot {
    ($n:literal, $id:literal, $label:literal, $cmd:literal, $digit:literal) => {
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
    },
    ActionSpec {
        action: Action::FocusLeft,
        id: "focus_left",
        label: "Focus left",
        hint: "Step to the card on the left, stopping at the row's first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["h", "left"],
    },
    ActionSpec {
        action: Action::FocusRight,
        id: "focus_right",
        label: "Focus right",
        hint: "Step to the card on the right, stopping at the row's last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["l", "right"],
    },
    ActionSpec {
        action: Action::MoveDown,
        id: "move_down",
        label: "Move down",
        hint: "Move the cursor down a row of cards; twice from the project tabs, into the one under their cursor",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["j", "down"],
    },
    ActionSpec {
        action: Action::MoveUp,
        id: "move_up",
        label: "Move up",
        hint: "Move the cursor up a row of cards, stopping at the first; twice there, up to the project tabs",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["k", "up"],
    },
    ActionSpec {
        action: Action::HalfPageDown,
        id: "half_page_down",
        label: "Half page down",
        hint: "Jump the cursor down two rows of cards, stopping at the last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["ctrl+d"],
    },
    ActionSpec {
        action: Action::HalfPageUp,
        id: "half_page_up",
        label: "Half page up",
        hint: "Jump the cursor up two rows of cards, stopping at the first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["ctrl+u"],
    },
    ActionSpec {
        action: Action::Activate,
        id: "activate",
        label: "Open / attach",
        hint: "Into the pane on the card under the cursor, or lock the pane's input",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["enter"],
    },
    ActionSpec {
        action: Action::Palette,
        id: "palette",
        label: "Fuzzy jump",
        hint: "Search every project, worktree and session at once",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["/"],
    },
    ActionSpec {
        action: Action::NextAttention,
        id: "next_attention",
        label: "Next session needing you",
        hint: "Jump to the next session in the palette's attention order (needs feedback, running, unseen, then recency) in any project, wrapping",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["."],
    },
    ActionSpec {
        action: Action::PrevAttention,
        id: "prev_attention",
        label: "Prev session needing you",
        hint: "The same walk backwards: the session before this one in the palette's attention order, wrapping at the top",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &[","],
    },
    ActionSpec {
        action: Action::NextProjectTab,
        id: "next_project_tab",
        label: "Next project tab",
        hint: "Open the project on the tab to the right of this one in the header, stopping at the last",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["]"],
    },
    ActionSpec {
        action: Action::PrevProjectTab,
        id: "prev_project_tab",
        label: "Prev project tab",
        hint: "Open the project on the tab to the left, stopping at the first",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["["],
    },
    project_tab_slot!(1, "project_tab_1", "Project tab 1", "cmd+1", "1"),
    project_tab_slot!(2, "project_tab_2", "Project tab 2", "cmd+2", "2"),
    project_tab_slot!(3, "project_tab_3", "Project tab 3", "cmd+3", "3"),
    project_tab_slot!(4, "project_tab_4", "Project tab 4", "cmd+4", "4"),
    project_tab_slot!(5, "project_tab_5", "Project tab 5", "cmd+5", "5"),
    project_tab_slot!(6, "project_tab_6", "Project tab 6", "cmd+6", "6"),
    project_tab_slot!(7, "project_tab_7", "Project tab 7", "cmd+7", "7"),
    project_tab_slot!(8, "project_tab_8", "Project tab 8", "cmd+8", "8"),
    project_tab_slot!(9, "project_tab_9", "Project tab 9", "cmd+9", "9"),
    ActionSpec {
        action: Action::CloseProjectTab,
        id: "close_project_tab",
        label: "Close project tab",
        hint: "Drop this project's tab from the header and open the tab beside it. Nothing is deleted: + opens it again",
        group: "NAVIGATE",
        scope: Scope::Global,
        defaults: &["x"],
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
    },
    ActionSpec {
        action: Action::New,
        id: "new",
        label: "New session",
        hint: "Pick the harness first (→ drills into its model and effort), then the quick prompt opens set to it, in the checkout under the cursor; on the first-run splash, add a project",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["n"],
    },
    ActionSpec {
        action: Action::GitDiff,
        id: "git_diff",
        label: "Git diff",
        hint: "Open the diff viewer for the checkout under the cursor, or, with the pane reading a pull request, that pull request's diff",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["g"],
    },
    ActionSpec {
        action: Action::OpenRepo,
        id: "open_repo",
        label: "Open repo in browser",
        hint: "Send the selected repo's git remote (GitHub, GitLab, …) to your browser",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+g"],
    },
    ActionSpec {
        action: Action::OpenPullRequest,
        id: "open_pull_request",
        label: "Open pull request in browser",
        hint: "Send the pull request of the session card under the cursor (its checkout's branch) to your browser, as a click on the card's #42 line does. v lists the pull requests in kete; ⇧V goes to GitHub",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+v"],
    },
    ActionSpec {
        action: Action::OpenIssue,
        id: "open_issue",
        label: "Open issue in browser",
        hint: "Send the GitHub issue the session card under the cursor was started from to your browser. i lists the issues in kete; ⇧I goes to GitHub",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+i"],
    },
    ActionSpec {
        action: Action::RefreshPullRequests,
        id: "refresh_pull_requests",
        label: "Reload from GitHub",
        hint: "Ask GitHub again now for the project's open pull requests and issues, the selected worktree's PR and the one the pane is reading",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+r"],
    },
    ActionSpec {
        action: Action::CommentPullRequest,
        id: "comment_pull_request",
        label: "Comment on pull request",
        hint: "Open a box to type a comment and post it through gh on the card's pull request (the one ⇧V opens) or, with the pane reading a pull request (a / jump lands on one), on that one",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["y"],
    },
    ActionSpec {
        action: Action::Issues,
        id: "issues",
        label: "GitHub issues",
        hint: "List the project's open issues; Enter prompts an agent on one, ⇧Tab launches a preset on it",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["i"],
    },
    ActionSpec {
        action: Action::PullRequests,
        id: "pull_requests",
        label: "GitHub pull requests",
        hint: "List the project's open pull requests; Enter prompts a PR session on one, ⇧Tab launches a preset on it, Tab picks a harness",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["v"],
    },
    ActionSpec {
        action: Action::SwitchBranch,
        id: "switch_branch",
        label: "Switch root branch",
        hint: "Fuzzy-pick a branch or remote for the project's root checkout; uncommitted changes get stash / bring / commit / discard",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["c"],
    },
    ActionSpec {
        action: Action::OpenWorktree,
        id: "open_worktree",
        label: "Open checkout in editor",
        hint: "Open the selected checkout outside kete, usually in your editor: its project's Open command (Settings → Project), else .kete.json \"open\", e.g. open http://localhost:3000 (⇧Enter needs the kitty protocol; ⇧O arrives everywhere; ⌥Enter is the ESC CR that VS Code's Shift+Enter setup sends and tmux passes through)",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+enter", "shift+o", "alt+enter"],
    },
    ActionSpec {
        action: Action::OpenInBrowser,
        id: "open_in_browser",
        label: "Open localhost in a browser",
        hint: "Open the selected worktree's dev server (http://localhost:<port>, the port its sessions listen on) in a browser picked from Chrome, Edge, Firefox or the system default; the pick is remembered per project",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["w"],
    },
    ActionSpec {
        action: Action::Screenshot,
        id: "screenshot",
        label: "Screenshot localhost",
        hint: "Take a headless Edge or Chrome screenshot (1280×800) of the selected worktree's dev server and show it in the image view",
        group: "PROJECTS & WORKTREES",
        scope: Scope::Global,
        defaults: &["shift+w"],
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
    },
    ActionSpec {
        action: Action::Rename,
        id: "rename",
        label: "Rename",
        hint: "Rename the session under the cursor (the checkout's run command is the menu's Run / Stop)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["r"],
    },
    ActionSpec {
        action: Action::Archive,
        id: "archive",
        label: "Archive session",
        hint: "Archive the selected agent (its PTY is released)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["a"],
    },
    ActionSpec {
        action: Action::Unarchive,
        id: "unarchive",
        label: "Unarchive session",
        hint: "Bring an archived agent back into the list",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["u"],
    },
    ActionSpec {
        action: Action::ToggleArchived,
        id: "toggle_archived",
        label: "Show / hide archived",
        hint: "Swap the grid between the live sessions and the archived ones",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+a"],
    },
    ActionSpec {
        action: Action::Delete,
        id: "delete",
        label: "Delete selected",
        hint: "Remove the selected row, behind a confirmation. With the PROJECT TABS holding the keys, close the tab under their cursor, behind the same kind of confirmation: x closes it outright",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["d", "delete", "backspace"],
    },
    ActionSpec {
        action: Action::DeleteAll,
        id: "delete_all",
        label: "Delete all sessions",
        hint: "Remove every session listed for the checkout under the cursor, behind a confirmation",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+d"],
    },
    ActionSpec {
        action: Action::AgentPresets,
        id: "agent_presets",
        label: "Agent presets",
        hint: "Saved launch presets (CLI, model, effort, prefix/postfix) for the checkout under the cursor; Enter asks for an optional task, or skips it; from the pull requests list, a PR session in that branch's worktree",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["e"],
    },
    ActionSpec {
        action: Action::QuickPrompt,
        id: "quick_prompt",
        label: "Quick prompt",
        hint: "Type a prompt; Enter starts an agent on it (Settings > Agents picks which)",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["p"],
    },
    ActionSpec {
        action: Action::FollowUp,
        id: "follow_up",
        label: "Follow-up prompt",
        hint: "Open a box over the grid for the selected session's next turn, sent to the running agent on Enter; the pane stays as it is",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["space"],
    },
    ActionSpec {
        action: Action::DuplicateSession,
        id: "duplicate_session",
        label: "Duplicate session",
        hint: "Open the quick prompt set to launch what the selected card runs (the same harness, model, effort and worktree) so only the task is left to type. p opens the box on the defaults; ⇧P on the card",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["shift+p"],
    },
    ActionSpec {
        action: Action::MoveSession,
        id: "move_session",
        label: "Move session",
        hint: "Pick another worktree, of this project or another, for the selected session; it resumes there, after its current turn when it is mid-turn. Dragging the card onto another band or project tab does the same",
        group: "SESSIONS",
        scope: Scope::Global,
        defaults: &["m"],
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
    },
    ActionSpec {
        action: Action::Grep,
        id: "grep",
        label: "Find in files",
        hint: "git grep across the selected worktree",
        group: "FILES",
        scope: Scope::Global,
        defaults: &["shift+f"],
    },
    ActionSpec {
        action: Action::TreeBrowser,
        id: "tree_browser",
        label: "File tree browser",
        hint: "Browse the worktree's files with a preview pane",
        group: "FILES",
        scope: Scope::Global,
        defaults: &["b"],
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
    },
    ActionSpec {
        action: Action::ToggleKanban,
        id: "toggle_kanban",
        label: "Kanban board",
        hint: "Lay the grid out as lanes by status (Needs input, Running, Unseen, Done, PR open) or back to a band per worktree. On the board h/l walk the lanes, j/k the cards, Tab flips between this project and every project",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+k"],
    },
    ActionSpec {
        action: Action::ToggleOrchestratorView,
        id: "toggle_orchestrator_view",
        label: "Orchestrator view",
        hint: "Toggle between the chat-first orchestrator view and the grid",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["f10"],
    },
    ActionSpec {
        action: Action::Accounts,
        id: "accounts",
        label: "Accounts and usage",
        hint: "Every account (profiles, and each CLI's own login) with its 5h and weekly plan usage, resets and login; Space switches a profile on or off, d makes it its harness's default",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+u"],
    },
    ActionSpec {
        action: Action::ToggleFullScreen,
        id: "toggle_full_screen",
        label: "Full-screen session",
        hint: "Give the session in the pane the whole screen, or bring it back down beside the cards, from inside the pane too, where it is never forwarded to the agent. From the cards it full-screens the one under the cursor. ^q and ^` also bring a full-screen session back down",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["ctrl+f"],
    },
    ActionSpec {
        action: Action::PaneTabs,
        id: "pane_tabs",
        label: "Terminals in the pane",
        hint: "Walk the cursor through the terminal cards of its checkout, one after another and back round to the first, the pane reads each",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["`"],
    },
    ActionSpec {
        action: Action::Hosts,
        id: "hosts",
        label: "SSH hosts",
        hint: "Connect to a saved ssh host (restarts kete over ssh)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+h"],
    },
    ActionSpec {
        action: Action::Settings,
        id: "settings",
        label: "Settings",
        hint: "Open this settings overlay",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["s"],
    },
    ActionSpec {
        action: Action::Metrics,
        id: "metrics",
        label: "Memory usage",
        hint: "RAM used by kete and every live agent's process tree",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["shift+m"],
    },
    ActionSpec {
        action: Action::Help,
        id: "help",
        label: "Help",
        hint: "Toggle the keyboard help overlay",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["?"],
    },
    ActionSpec {
        action: Action::Quit,
        id: "quit",
        label: "Quit kete",
        hint: "Leave the TUI (sessions keep running in the daemon)",
        group: "GENERAL",
        scope: Scope::Global,
        defaults: &["q", "ctrl+c"],
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
