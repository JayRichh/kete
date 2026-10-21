//! Pick a thing, see its verbs: one dispatch from a registry verb and the
//! thing it acts on to what the verb does there.
//!
//! The menus (a right-click, a card's `⋯`, the project tab's), the VERBS
//! MENU `?` opens, the footer's leading verbs and the COMMAND PALETTE's
//! top rows are all built here, from nebula-config's registry: its
//! [`MENUS`](nebula_config::actions::MENUS) for which verbs a kind's menu
//! lists and in what order, its [`MENU_GROUPS`] for the runs of rows that
//! fold into one submenu, its [`PRIMARY`](nebula_config::actions::PRIMARY)
//! for the few a footer leads with, and its menu labels for the words —
//! the desktop window reads the same tables. A verb on a [`Target`] is
//! [`menu_action`]: the row-addressed [`MenuAction`] its menu row runs
//! (Rename on a session renames that session, whichever card the cursor
//! is on), or, for a verb with no such form, its key run on the selection
//! ([`MenuAction::Run`]) — offered only when the target is the selection,
//! so the key's target and the row's are the same thing. A key bound to
//! one of the menus' own verbs (Restart, Run, Delete worktree…) goes the
//! same way ([`run_on_selection`]).
//!
//! The terminal app has no headings, rules or disabled rows (the menu
//! matrix's parity rule, docs/superpowers/specs/2026-10-16-kete-054-menu-
//! matrix.md §1.8): a row the desktop draws greyed is left out here, and
//! the sections run on without a rule. The words, the order, the
//! submenus and the keys are the desktop's.

use super::*;
use crate::keymap::{Action, KeyChord};
use nebula_config::actions::{
    group_of, menu_label, menu_of, primary_of, AppliesTo, MenuGroup, Surface, WorksIn,
};
use std::path::{Path, PathBuf};

/// A thing a verb acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Repo(ProjectId),
    Worktree(WorktreeId),
    Session(AgentId),
    Terminal(TerminalId),
    /// A file of a checkout: the checkout's folder and the file's path
    /// from it, as git spells it (`/`). A diff's file row, the tree
    /// browser's, a finder's or a search's hit.
    File {
        worktree: PathBuf,
        rel: String,
    },
    /// A folder of a checkout, the same way: the tree browser's rows.
    Folder {
        worktree: PathBuf,
        rel: String,
    },
    /// An account, by its key: a profile's name, or `@<harness>` for a
    /// harness's own login.
    Account(String),
    /// Nothing in particular: the background, a right-click on no row.
    /// What its bulk verbs take is what the board or the grid shows.
    Global,
}

impl Target {
    pub(crate) fn kind(&self) -> AppliesTo {
        match self {
            Target::Repo(_) => AppliesTo::Repo,
            Target::Worktree(_) => AppliesTo::Worktree,
            Target::Session(_) => AppliesTo::Session,
            Target::Terminal(_) => AppliesTo::Terminal,
            Target::File { .. } => AppliesTo::File,
            Target::Folder { .. } => AppliesTo::Folder,
            Target::Account(_) => AppliesTo::Account,
            Target::Global => AppliesTo::Global,
        }
    }
}

/// Where the keys are: the grid of cards, the board, or the pane.
pub(crate) fn place(app: &App) -> WorksIn {
    if app.nav.focus == Focus::Terminal || app.pane.collapsed {
        WorksIn::Pane
    } else if app.launcher.launcher_kanban && app.launcher_active() {
        WorksIn::Board
    } else {
        WorksIn::Grid
    }
}

/// What the keys act on now: the PROJECT TAB under the header's cursor,
/// the band with no card to be on, the card (a board card is a session
/// card), or in the panels the focused column's row. None with nothing
/// under the cursor.
pub(crate) fn selection(app: &App) -> Option<Target> {
    if let Some(p) = &app.launcher.launcher_tab_cursor {
        return Some(Target::Repo(p.clone()));
    }
    let row = || match app.selected_session_row()? {
        SessionRow::Agent(a) => Some(Target::Session(a.id)),
        SessionRow::Terminal(t) => Some(Target::Terminal(t.id)),
        SessionRow::Link(_) => None,
    };
    let worktree = || {
        app.selected_worktree()
            .map(|w| Target::Worktree(w.id.clone()))
    };
    if app.launcher_active() && !app.launcher.launcher_columns {
        if let Some(w) = launcher::empty_band(app).or_else(|| launcher::folded_band(app)) {
            return Some(Target::Worktree(w));
        }
        return row()
            .or_else(worktree)
            .or_else(|| app.selected_project().map(|p| Target::Repo(p.id.clone())));
    }
    match app.nav.focus {
        Focus::Projects => app.selected_project().map(|p| Target::Repo(p.id.clone())),
        Focus::Worktrees => {
            if app.selected_worktree_pr().is_some() || app.selected_worktree_issue().is_some() {
                None
            } else {
                worktree()
            }
        }
        Focus::Sessions | Focus::Terminal => row().or_else(worktree),
    }
}

/// The checkout a target runs in, and the project it is of.
pub(crate) fn worktree_of(app: &App, t: &Target) -> Option<WorktreeId> {
    match t {
        Target::Worktree(w) => Some(w.clone()),
        Target::Session(id) => app
            .tree
            .agents
            .iter()
            .find(|a| &a.id == id)
            .map(|a| a.worktree_id.clone()),
        Target::Terminal(id) => app
            .tree
            .terminals
            .iter()
            .find(|x| &x.id == id)
            .map(|x| x.worktree_id.clone()),
        _ => None,
    }
}

fn project_of(app: &App, t: &Target) -> Option<ProjectId> {
    match t {
        Target::Repo(p) => Some(p.clone()),
        _ => {
            let w = worktree_of(app, t)?;
            app.tree
                .worktrees
                .iter()
                .find(|x| x.id == w)
                .map(|x| x.project_id.clone())
        }
    }
}

/// The checkout whose dev server, editor window or folder a target means:
/// its own, or a repo's root checkout.
pub(crate) fn checkout_of(app: &App, t: &Target) -> Option<WorktreeId> {
    match t {
        Target::Repo(p) => app.root_worktree(p),
        _ => worktree_of(app, t),
    }
}

/// Whether a verb's key, run now, would act on `t`: `t` is the selection,
/// or the checkout or project the selection is in.
fn on_selection(app: &App, t: &Target) -> bool {
    let Some(sel) = selection(app) else {
        return false;
    };
    match t {
        Target::Worktree(w) => worktree_of(app, &sel).as_ref() == Some(w),
        Target::Repo(p) => project_of(app, &sel).as_ref() == Some(p),
        _ => sel == *t,
    }
}

/// `path` as this machine writes one: Windows' backslashes throughout,
/// so a checkout's folder and a file joined onto it read as one path.
pub(crate) fn native(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

/// The full path of a file or folder target, or of a checkout.
pub(crate) fn path_of(app: &App, t: &Target) -> Option<PathBuf> {
    match t {
        Target::File { worktree, rel } | Target::Folder { worktree, rel } => {
            Some(if rel.is_empty() {
                worktree.clone()
            } else {
                worktree.join(rel)
            })
        }
        Target::Repo(p) => app
            .tree
            .projects
            .iter()
            .find(|x| &x.id == p)
            .map(|x| x.repo_path.clone()),
        _ => {
            let w = worktree_of(app, t)?;
            app.tree
                .worktrees
                .iter()
                .find(|x| x.id == w)
                .map(|x| x.path.clone())
        }
    }
}

/// What an editor verb does on `t` (lane/editors' `crate::editors`): a
/// card's, a checkout's or a repo's folder, or a file or folder in one,
/// in VS Code, Visual Studio, Explorer, docked or with its default app.
/// None where its rules say it can't run now: over ssh (the far side's
/// screen), an editor not installed, Visual Studio with no solution, Dock
/// with no window open on the checkout, a program for the default app.
pub(crate) fn editor_act(app: &App, action: Action, t: &Target) -> Option<MenuAction> {
    match t {
        Target::File { worktree, .. } | Target::Folder { worktree, .. } => {
            let path = path_of(app, t)?;
            crate::editors::file_action(app, action, worktree, &path, None)
        }
        _ => crate::editors::checkout_action(app, action, &checkout_root(app, t)?),
    }
}

/// The checkout an editor row on `t` opens in: a file's or folder's own,
/// else [`checkout_root`].
fn editor_root(app: &App, t: &Target) -> Option<PathBuf> {
    match t {
        Target::File { worktree, .. } | Target::Folder { worktree, .. } => Some(worktree.clone()),
        _ => checkout_root(app, t),
    }
}

/// What verb `action` does to `t`, as a menu row runs it: None where it
/// means nothing there now — Unarchive on a live session, Delete worktree
/// on a root checkout, a key-only verb on a thing the cursor is not on.
pub(crate) fn menu_action(app: &App, action: Action, t: &Target) -> Option<MenuAction> {
    let spec = crate::keymap::spec_of(action)?;
    if !spec.applies_to.contains(&t.kind()) || spec.surface == Surface::Desktop {
        return None;
    }
    let keyed = |app: &App| on_selection(app, t).then_some(MenuAction::Run(action));
    let verb = || MenuAction::Verb {
        action,
        target: t.clone(),
    };
    let editor = matches!(
        action,
        Action::OpenInVscode
            | Action::OpenInVisualStudio
            | Action::RevealInExplorer
            | Action::DockEditor
            | Action::OpenWithDefault
    );
    match t {
        Target::Session(id) => {
            let a = app.tree.agents.iter().find(|a| &a.id == id)?;
            let id = id.clone();
            // An archived card keeps what brings it back or takes it away,
            // and its copy (matrix §5.1): it runs in no checkout now.
            if a.archived {
                return match action {
                    Action::DuplicateSession => Some(MenuAction::DuplicateAgent(id)),
                    Action::Unarchive => Some(MenuAction::UnarchiveAgent(id)),
                    Action::Delete => Some(MenuAction::DeleteAgent(id)),
                    _ => None,
                };
            }
            // A Cloud row has no terminal to attach or restart: its agent
            // runs in the cloud sandbox, so its open is the browser and
            // its follow-up the message queue; nothing on this machine is
            // its folder.
            let cloud = a.cloud_session_url();
            let local = cloud.is_none();
            let w = a.worktree_id.clone();
            match action {
                Action::Activate => Some(match cloud {
                    Some(url) => MenuAction::OpenLink(url),
                    None => MenuAction::Attach(SessionRef::Agent(id)),
                }),
                Action::FollowUp => Some(match cloud {
                    Some(_) => MenuAction::SendCloudMessage(id),
                    None => MenuAction::FollowUp,
                }),
                Action::Restart => local.then_some(MenuAction::RestartAgent(id)),
                Action::DuplicateSession => Some(MenuAction::DuplicateAgent(id)),
                Action::AskAside => local.then_some(MenuAction::AskAside(id)),
                Action::MoveSession => local.then_some(MenuAction::MoveAgentPicker(id)),
                Action::MoveToAccount => local.then_some(MenuAction::SwitchAccountPicker(id)),
                Action::Rename => Some(MenuAction::RenameAgent(id)),
                Action::PinOrchestrator if local => Some(if a.role.is_orchestrator() {
                    MenuAction::ClearOrchestrator
                } else {
                    MenuAction::MakeOrchestrator(id)
                }),
                Action::PinOrchestrator => None,
                // 0.60: only on the orchestrator itself.
                Action::RenewOrchestrator => (local && a.role.is_orchestrator()).then(verb),
                Action::Archive => Some(MenuAction::ArchiveAgent(id)),
                Action::Unarchive => None,
                Action::Delete => Some(MenuAction::DeleteAgent(id)),
                // The card's pull request and the issue it was started
                // from, what `⇧V` and `⇧I` open.
                Action::OpenPullRequest => {
                    crate::launcher::worktree_pr(app, &w).map(|pr| MenuAction::OpenLink(pr.url))
                }
                Action::OpenIssue => a.issue_url.clone().map(MenuAction::OpenLink),
                // The checkout's pull request: the composer while it has
                // none, the merge while one is open and published, a
                // comment on one open or a draft (matrix §5.1).
                Action::CreatePullRequest => {
                    ships(app, &w).then_some(MenuAction::CreatePullRequest {
                        worktree: w,
                        agent: Some(id),
                    })
                }
                Action::MergePullRequest => {
                    merges(app, &w).then_some(MenuAction::MergePullRequest {
                        worktree: w,
                        agent: Some(id),
                    })
                }
                Action::CommentPullRequest if pr_live(app, &w) => keyed(app),
                Action::CommentPullRequest => None,
                Action::CopyPath => local
                    .then(|| path_of(app, t))
                    .flatten()
                    .map(|p| MenuAction::Copy(native(&p))),
                Action::CopyBranch => local
                    .then(|| branch_of(app, &w))
                    .flatten()
                    .map(MenuAction::Copy),
                Action::CopyPrUrl => local
                    .then(|| crate::launcher::worktree_pr(app, &w))
                    .flatten()
                    .map(|pr| MenuAction::Copy(pr.url)),
                Action::MarkSeen => a.unseen.then(verb),
                Action::OpenInBrowser => (local && serves(app, &w)).then(verb),
                _ if editor => local.then(|| editor_act(app, action, t)).flatten(),
                Action::FocusScope => super::board::scope_for(app, t).map(MenuAction::FocusScope),
                Action::ReorderUp | Action::ReorderDown => Some(MenuAction::Reorder(
                    crate::order::OrderTarget::Agent(id),
                    if action == Action::ReorderUp { -1 } else { 1 },
                )),
                _ => keyed(app),
            }
        }
        Target::Terminal(id) => match action {
            Action::Activate => Some(MenuAction::Attach(SessionRef::Terminal(id.clone()))),
            Action::Rename => Some(MenuAction::RenameTerminal(id.clone())),
            Action::Delete => Some(MenuAction::CloseTerminal(id.clone())),
            // A terminal's path is its checkout's folder.
            Action::CopyPath => path_of(app, t).map(|p| MenuAction::Copy(native(&p))),
            _ => keyed(app),
        },
        Target::Worktree(id) => {
            let w = app.tree.worktrees.iter().find(|w| &w.id == id)?;
            // A stand-in checkout is not a place the DAEMON knows yet.
            if app.is_placeholder_worktree(id) {
                return None;
            }
            let id = id.clone();
            match action {
                Action::New => Some(MenuAction::NewAgent(id)),
                Action::NewTerminal => Some(MenuAction::NewTerminal(id)),
                Action::ToggleRun => Some(MenuAction::ToggleRun(id)),
                Action::OpenWorktree => Some(MenuAction::OpenWorktree(id)),
                // The ROOT WORKTREE moves between branches; a linked
                // worktree, named for its branch, is deleted instead.
                Action::SwitchBranch => w.is_main.then_some(MenuAction::SwitchBranch(id)),
                Action::DeleteWorktree => (!w.is_main).then_some(MenuAction::DeleteWorktree(id)),
                Action::CreatePullRequest => {
                    ships(app, &id).then_some(MenuAction::CreatePullRequest {
                        worktree: id,
                        agent: None,
                    })
                }
                Action::MergePullRequest => {
                    merges(app, &id).then_some(MenuAction::MergePullRequest {
                        worktree: id,
                        agent: None,
                    })
                }
                Action::FocusScope => focus_or_widen(app, t),
                // The checkout's pull request rows only with one to act on,
                // the dev server only while something listens, the bulk
                // verbs only with something to take.
                Action::OpenPullRequest => {
                    crate::launcher::worktree_pr(app, &id).map(|pr| MenuAction::OpenLink(pr.url))
                }
                Action::CopyPrUrl => {
                    crate::launcher::worktree_pr(app, &id).map(|pr| MenuAction::Copy(pr.url))
                }
                Action::CopyPath => Some(MenuAction::Copy(native(&w.path))),
                Action::CopyBranch => Some(MenuAction::Copy(w.branch.clone())),
                Action::CommentPullRequest if !pr_live(app, &id) => None,
                Action::OpenInBrowser => serves(app, &id).then(verb),
                _ if editor => editor_act(app, action, t),
                Action::MarkSeen => app
                    .tree
                    .agents
                    .iter()
                    .any(|a| a.worktree_id == id && unread(a))
                    .then(verb),
                Action::ArchiveDone => app
                    .tree
                    .agents
                    .iter()
                    .any(|a| a.worktree_id == id && done(a))
                    .then(verb),
                // The band's place among the project's; the root's stays
                // first.
                Action::ReorderUp | Action::ReorderDown => (!w.is_main).then(|| {
                    MenuAction::Reorder(
                        crate::order::OrderTarget::Worktree(id),
                        if action == Action::ReorderUp { -1 } else { 1 },
                    )
                }),
                // Only with something to delete.
                Action::DeleteAll => {
                    let any =
                        app.tree.agents.iter().any(|a| {
                            a.worktree_id == w.id && a.archived == app.launcher.show_archived
                        }) || app.tree.terminals.iter().any(|x| x.worktree_id == w.id);
                    if any {
                        keyed(app)
                    } else {
                        None
                    }
                }
                _ => keyed(app),
            }
        }
        Target::Repo(id) => {
            // The checkout the grid would launch into: what the project's
            // Run and Open act on.
            let checkout = || crate::launcher::checkout_for(app, id);
            match action {
                Action::NewWorktree => Some(MenuAction::NewWorktree(id.clone())),
                Action::SwitchBranch => app.root_worktree(id).map(MenuAction::SwitchBranch),
                Action::ToggleRun => checkout().map(MenuAction::ToggleRun),
                Action::OpenWorktree => checkout().map(MenuAction::OpenWorktree),
                // Off the repo's menu (matrix §2: an unclear target there),
                // but a key bound to it still deletes the grid's linked
                // checkout, as it always has.
                Action::DeleteWorktree => checkout()
                    .filter(|w| app.tree.worktrees.iter().any(|x| &x.id == w && !x.is_main))
                    .map(MenuAction::DeleteWorktree),
                Action::Rename => Some(MenuAction::RenameProject(id.clone())),
                Action::ReorderUp | Action::ReorderDown => Some(MenuAction::Reorder(
                    crate::order::OrderTarget::Project(id.clone()),
                    if action == Action::ReorderUp { -1 } else { 1 },
                )),
                Action::RemoveProject => Some(MenuAction::RemoveProject(id.clone())),
                // Only with a linked worktree to look at.
                Action::SweepWorktrees => app
                    .tree
                    .worktrees
                    .iter()
                    .any(|w| &w.project_id == id && !w.is_main)
                    .then(|| MenuAction::SweepWorktrees(id.clone())),
                Action::FocusScope => focus_or_widen(app, t),
                // Its remote's lists by host, the dev server of its root
                // checkout while it listens, the bulk verbs with something
                // to take.
                Action::Ado if !on_ado(app, id) => None,
                Action::PullRequests | Action::Issues if on_ado(app, id) => None,
                Action::OpenInBrowser => app
                    .root_worktree(id)
                    .is_some_and(|w| serves(app, &w))
                    .then(verb),
                Action::CopyPath => path_of(app, t).map(|p| MenuAction::Copy(native(&p))),
                _ if editor => editor_act(app, action, t),
                Action::MarkSeen => in_project(app, id, unread).then(verb),
                Action::ArchiveDone => in_project(app, id, done).then(verb),
                // 0.60: a studio repo's pages and autonomy, only on one.
                Action::StudioBoard | Action::GauntletView | Action::StudioAutonomy => {
                    is_studio_repo(app, id).then(verb)
                }
                _ => keyed(app),
            }
        }
        // A file's and a folder's rows: the editors' (lane/editors runs
        // them, `run_verb`) and the copies. What its state rules out (a
        // deleted file opens nowhere) the file's menu takes out after.
        Target::File { rel, .. } | Target::Folder { rel, .. } => match action {
            Action::OpenFileInKete => Some(verb()),
            _ if editor => editor_act(app, action, t),
            Action::CopyPath => path_of(app, t).map(|p| MenuAction::Copy(native(&p))),
            Action::CopyRelativePath => Some(MenuAction::Copy(rel.clone())),
            _ => None,
        },
        Target::Account(key) => match action {
            // Show its sessions: the scope narrowed to the account, the
            // repo and worktree kept, with no picker.
            Action::ScopeAccount => Some(MenuAction::FocusScope(nebula_core::scope::Scope {
                account: Some(key.clone()),
                ..app.launcher.scope.clone()
            })),
            Action::Accounts | Action::WakeAccount | Action::WakeSchedule => Some(verb()),
            _ => None,
        },
        Target::Global => match action {
            Action::MarkSeen => app
                .tree
                .agents
                .iter()
                .any(|a| unread(a) && in_background(app, a))
                .then(verb),
            Action::ArchiveDone => app
                .tree
                .agents
                .iter()
                .any(|a| done(a) && in_background(app, a))
                .then(verb),
            Action::ToggleArchived => Some(MenuAction::ToggleArchived),
            Action::AddProject => Some(MenuAction::AddProject),
            Action::WakeSchedule => Some(verb()),
            // 0.60: the studio pages once any repo is a studio repo, the
            // triage line while an orchestrator or a triage desk runs, the
            // usage export always.
            Action::StudioBoard | Action::GauntletView => any_studio_repo(app).then(verb),
            Action::TriageNote => triage_available(app).then(verb),
            Action::UsageCsv => Some(verb()),
            // The app's own: its key, on nothing in particular.
            _ => Some(MenuAction::Run(action)),
        },
    }
}

/// Is `id` a studio repo: a manifest in its root, or a `studio` entry
/// (`ipc_studio::StudioRepos`, read off the key path)?
fn is_studio_repo(app: &App, id: &nebula_core::ProjectId) -> bool {
    app.studio_repos.get(&app.tree.projects).contains(id)
}

/// Is any project a studio repo?
fn any_studio_repo(app: &App) -> bool {
    !app.studio_repos.get(&app.tree.projects).is_empty()
}

/// Is there someone for a triage note to go to: a live orchestrator, or a
/// session holding the `triage` role card?
pub(crate) fn triage_available(app: &App) -> bool {
    app.tree
        .agents
        .iter()
        .any(|a| !a.archived && a.role.is_orchestrator())
        || app.session_roles.values().any(|r| r.card == "triage")
}

/// FOCUS HERE on a repo or a checkout, or — on the one the scope already
/// is — the way back out: the repo a checkout is in, everything from a
/// repo (the label turns with it, [`label`]).
fn focus_or_widen(app: &App, t: &Target) -> Option<MenuAction> {
    let scope = &app.launcher.scope;
    if widens(app, t) {
        let mut wider = scope.clone();
        match t {
            Target::Worktree(_) => {
                wider.worktree = None;
                wider.session = None;
            }
            _ => {
                wider = nebula_core::scope::Scope {
                    account: scope.account.clone(),
                    ..nebula_core::scope::Scope::default()
                }
            }
        }
        return Some(MenuAction::FocusScope(wider));
    }
    super::board::scope_for(app, t).map(MenuAction::FocusScope)
}

/// Is the scope chip on `t` already, so its FOCUS HERE widens instead?
fn widens(app: &App, t: &Target) -> bool {
    let scope = &app.launcher.scope;
    match t {
        Target::Worktree(w) => scope.worktree.as_ref() == Some(w),
        Target::Repo(p) => scope.repo.as_ref() == Some(p) && scope.worktree.is_none(),
        _ => false,
    }
}

/// The folder `t`'s editor rows open: a card's or a worktree's checkout,
/// a repo's main one. None for a checkout still being cut.
fn checkout_root(app: &App, t: &Target) -> Option<std::path::PathBuf> {
    let w = match t {
        Target::Repo(p) => match app.root_worktree(p) {
            Some(w) => w,
            None => {
                return app
                    .tree
                    .projects
                    .iter()
                    .find(|x| &x.id == p)
                    .map(|x| x.repo_path.clone())
            }
        },
        _ => worktree_of(app, t)?,
    };
    if app.is_placeholder_worktree(&w) {
        return None;
    }
    app.tree
        .worktrees
        .iter()
        .find(|x| x.id == w)
        .map(|x| x.path.clone())
}

/// Is something listening on checkout `w`'s ports: its dev server up.
fn serves(app: &App, w: &WorktreeId) -> bool {
    app.tree.ports.get(w).is_some_and(|p| !p.live.is_empty())
}

fn branch_of(app: &App, w: &WorktreeId) -> Option<String> {
    app.tree
        .worktrees
        .iter()
        .find(|x| &x.id == w)
        .map(|x| x.branch.clone())
}

/// A finished turn nobody has read: what Mark seen clears.
fn unread(a: &nebula_core::Agent) -> bool {
    a.unseen && !a.archived
}

/// A finished session already read, not the orchestrator: what Archive
/// done sessions takes.
fn done(a: &nebula_core::Agent) -> bool {
    a.status == nebula_core::AgentStatus::Finished
        && !a.unseen
        && !a.archived
        && !a.role.is_orchestrator()
}

/// Has project `p` a session `pick` takes, in any of its checkouts.
fn in_project(app: &App, p: &ProjectId, pick: fn(&nebula_core::Agent) -> bool) -> bool {
    app.tree
        .agents
        .iter()
        .any(|a| pick(a) && project_has(app, p, a))
}

fn project_has(app: &App, p: &ProjectId, a: &nebula_core::Agent) -> bool {
    app.tree
        .worktrees
        .iter()
        .any(|w| w.id == a.worktree_id && &w.project_id == p)
}

/// What the background's bulk verbs take: what the board shows (the
/// scope chip, and its this-project mode), or on the grid the project
/// on screen within the chip's scope.
fn in_background(app: &App, a: &nebula_core::Agent) -> bool {
    let mut scope = crate::launcher::effective_scope(app);
    if !app.launcher.launcher_kanban && scope.repo.is_none() {
        scope.repo = app.selected_project().map(|p| p.id.clone());
    }
    crate::launcher::scope_admits(app, &scope, a)
}

/// Is project `p`'s remote on Azure DevOps, as the background probe
/// (`ado::probe`) found. Not known yet counts as no: a menu never runs
/// `git` on the loop, and the probe answers within moments of startup.
fn on_ado(app: &App, p: &ProjectId) -> bool {
    app.desk.ado_remote.get(p).copied().unwrap_or(false)
}

/// Can checkout `w` open a pull request from here: none on it at all —
/// a merged or closed one has its own row, never Create beside it — not
/// on its base branch, not a stand-in still being cut.
fn ships(app: &App, w: &WorktreeId) -> bool {
    !app.is_placeholder_worktree(w)
        && crate::launcher::worktree_pr(app, w).is_none()
        && !app
            .jobs
            .worktree_base
            .get(w)
            .is_some_and(|b| b.on_default_branch)
        && app.tree.worktrees.iter().any(|x| &x.id == w)
}

/// Has checkout `w` an open pull request to merge? Not a draft: it is
/// published first.
fn merges(app: &App, w: &WorktreeId) -> bool {
    crate::launcher::worktree_pr(app, w)
        .is_some_and(|pr| pr.standing == crate::pull_request::Standing::Open)
}

/// Has checkout `w` a pull request still open, a draft or not: what a
/// comment needs.
fn pr_live(app: &App, w: &WorktreeId) -> bool {
    crate::launcher::worktree_pr(app, w).is_some_and(|pr| {
        matches!(
            pr.standing,
            crate::pull_request::Standing::Open | crate::pull_request::Standing::Draft
        )
    })
}

/// The words a row for `action` on `t` says: the registry's menu label,
/// or what it does now where that turns (Run or Stop run, Make or Unpin
/// orchestrator, Focus here or Widen, the pull request's state).
fn label(app: &App, action: Action, t: &Target) -> String {
    let id = crate::keymap::spec_of(action).map_or("", |s| s.id);
    match (action, t) {
        (Action::ToggleRun, _) => {
            let running = match t {
                Target::Repo(p) => {
                    crate::launcher::checkout_for(app, p).is_some_and(|w| app.worktree_running(&w))
                }
                _ => worktree_of(app, t).is_some_and(|w| app.worktree_running(&w)),
            };
            if running { "Stop run" } else { "Run" }.into()
        }
        (Action::PinOrchestrator, Target::Session(id))
            if app
                .tree
                .agents
                .iter()
                .any(|a| &a.id == id && a.role.is_orchestrator()) =>
        {
            "Unpin orchestrator".into()
        }
        // A Cloud row's open is the browser and its follow-up the message
        // queue: the row says so.
        (Action::Activate | Action::FollowUp, Target::Session(id))
            if app
                .tree
                .agents
                .iter()
                .any(|a| &a.id == id && !a.archived && a.cloud_session_url().is_some()) =>
        {
            if action == Action::Activate {
                "Open in browser".into()
            } else {
                "Send to cloud session".into()
            }
        }
        // A pull request merged or closed is only there to read again.
        (Action::OpenPullRequest, _) => {
            let standing = worktree_of(app, t)
                .and_then(|w| crate::launcher::worktree_pr(app, &w))
                .map(|pr| pr.standing);
            match standing {
                Some(crate::pull_request::Standing::Merged) => "Open merged pull request".into(),
                Some(crate::pull_request::Standing::Closed) => "Open closed pull request".into(),
                _ => menu_label(id, t.kind()).to_string(),
            }
        }
        // The dev server by its port.
        (Action::OpenInBrowser, _) => match port_of(app, t) {
            Some(port) => format!("Open localhost:{port} in a browser"),
            None => menu_label(id, t.kind()).to_string(),
        },
        (Action::FocusScope, Target::Worktree(_)) if widens(app, t) => "Widen to the repo".into(),
        (Action::FocusScope, Target::Repo(_)) if widens(app, t) => "Widen to everything".into(),
        // A checkout whose pull request merged is the one to clear away.
        (Action::DeleteWorktree, Target::Worktree(w))
            if crate::launcher::worktree_pr(app, w)
                .is_some_and(|pr| pr.standing == crate::pull_request::Standing::Merged) =>
        {
            "Delete worktree (pull request merged)\u{2026}".into()
        }
        // The editor the row opens, by name where it isn't VS Code
        // (Cursor); Visual Studio's year beside another, and its pick.
        (Action::OpenInVscode, _) => crate::editors::code_label(menu_label(id, t.kind())),
        (Action::OpenInVisualStudio, _) => match editor_root(app, t) {
            Some(root) => crate::editors::vs_label(menu_label(id, t.kind()), &root),
            None => menu_label(id, t.kind()).to_string(),
        },
        _ => menu_label(id, t.kind()).to_string(),
    }
}

/// The port a target's dev server answers on, when it serves.
fn port_of(app: &App, t: &Target) -> Option<u16> {
    let w = checkout_of(app, t)?;
    serves(app, &w).then(|| app.tree.browse_port(&w)).flatten()
}

/// A menu row for verb `id` on `t`, or None where it means nothing there.
fn row(app: &App, id: &str, t: &Target) -> Option<MenuItem> {
    let spec = nebula_config::actions::spec_by_id(id)?;
    let action = menu_action(app, spec.action, t)?;
    let item = MenuItem::new(label(app, spec.action, t), action).keyed(spec.action);
    Some(if spec.destructive {
        MenuItem {
            destructive: true,
            ..item
        }
    } else {
        item
    })
}

/// The rows of `ids` that mean something on `t`, in that order; an
/// editor's other installs right after its row (Cursor after VS Code),
/// each its verb's, so it folds into `Open in ▸` beside it.
pub(crate) fn rows(app: &App, ids: &[&str], t: &Target) -> Vec<MenuItem> {
    ids.iter()
        .filter_map(|id| row(app, id, t))
        .flat_map(|item| {
            let verb = item.verb;
            let more = crate::editors::more_installs(&item.action)
                .into_iter()
                .map(move |m| MenuItem { verb, ..m });
            std::iter::once(item).chain(more)
        })
        .collect()
}

/// A row that opens a submenu of `items`.
pub(crate) fn submenu(title: impl Into<String>, items: Vec<MenuItem>) -> MenuItem {
    let title = title.into();
    MenuItem::new(title.clone(), MenuAction::Submenu { title, items })
}

/// The registry id of the verb a row is.
fn id_of(item: &MenuItem) -> Option<&'static str> {
    item.verb.and_then(crate::keymap::spec_of).map(|s| s.id)
}

/// Every row of `items`, the rows of their submenus in their place: what
/// a menu offers, folded or not.
pub(crate) fn flat(items: &[MenuItem]) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for item in items {
        match &item.action {
            MenuAction::Submenu { items, .. } => out.extend(flat(items)),
            _ => out.push(item.clone()),
        }
    }
    out
}

/// Fold `items` the way both apps fold them ([`MENU_GROUPS`]): on a kind
/// a group lists, two or more of its rows become one `title ▸` row at the
/// place of the first, each row inside saying its group's short word; a
/// group with one row left keeps it a plain row in its whole words.
fn fold(app: &App, t: &Target, items: Vec<MenuItem>) -> Vec<MenuItem> {
    let kind = t.kind();
    let group = |item: &MenuItem| id_of(item).and_then(|id| group_of(id, kind));
    let mut out = Vec::new();
    let mut folded: Vec<&'static str> = Vec::new();
    for item in &items {
        let Some((g, _)) = group(item) else {
            out.push(item.clone());
            continue;
        };
        if folded.contains(&g.id) {
            continue;
        }
        let members: Vec<&MenuItem> = items
            .iter()
            .filter(|m| group(m).is_some_and(|(o, _)| o.id == g.id))
            .collect();
        if members.len() < 2 {
            out.push(item.clone());
            continue;
        }
        folded.push(g.id);
        let children = members
            .into_iter()
            .map(|m| {
                let word = group(m).map_or("", |(_, word)| word);
                MenuItem {
                    label: child_word(app, t, m, word),
                    ..m.clone()
                }
            })
            .collect();
        out.push(submenu(group_title(app, t, g), children));
    }
    out
}

/// A row's words inside its group's submenu: the group's short word, the
/// dev server with its port, a view checked while it is on.
fn child_word(app: &App, t: &Target, item: &MenuItem, word: &str) -> String {
    if let MenuAction::Editor(_) = &item.action {
        if let Some(name) = item.label.strip_prefix("Open in ") {
            return name.to_string();
        }
    }
    match item.verb {
        Some(Action::OpenInBrowser) => match port_of(app, t) {
            Some(port) => format!("{word} \u{b7} localhost:{port}"),
            None => word.into(),
        },
        Some(Action::ToggleArchived) if app.launcher.show_archived => format!("{word} \u{2713}"),
        _ => word.into(),
    }
}

/// A group's title: the registry's, the remote's by its host where the
/// app knows it.
fn group_title(app: &App, t: &Target, g: &MenuGroup) -> String {
    match (g.id, t) {
        ("remote", Target::Repo(p)) => match app.desk.ado_remote.get(p) {
            Some(true) => "Azure DevOps".into(),
            Some(false) => "GitHub".into(),
            None => g.title.into(),
        },
        _ => g.title.into(),
    }
}

/// `t`'s menu: its kind's rows ([`MENUS`](nebula_config::actions::MENUS))
/// folded into their submenus, and for a live card its checkout's menu as
/// one `Worktree ‹branch› ▸` row after them, less what the card already
/// has (matrix §5.2).
pub(crate) fn menu_for(app: &App, t: &Target) -> Vec<MenuItem> {
    let mut items = rows(app, menu_of(t.kind()), t);
    if let Target::Session(id) = t {
        // Make orchestrator only while no other session is pinned (the
        // menu matrix's trim): replacing one is rare, and `?`'s More, the
        // palette and a bound key still reach it.
        let pinned = app
            .tree
            .agents
            .iter()
            .any(|a| &a.id != id && !a.archived && a.role.is_orchestrator());
        if pinned {
            items.retain(|i| !matches!(i.action, MenuAction::MakeOrchestrator(_)));
        }
        // The session that spawned this one, beside its diff: the way
        // back up an orchestrator's tree.
        if let Some(parent) = parent_row(app, id) {
            let at = items
                .iter()
                .position(|i| i.verb == Some(Action::GitDiff))
                .or_else(|| items.iter().position(|i| i.verb == Some(Action::FollowUp)))
                .map_or(0, |i| i + 1);
            items.insert(at, parent);
        }
        // A login to renew comes first: nothing else runs until it is.
        if let Some(login) = login_row(app, id) {
            items.insert(0, login);
        }
    }
    let mut items = fold(app, t, items);
    // The connections the go-to rows end on (matrix §6): a checkout's
    // run terminal, a repo's linked worktrees. Last of the Go section,
    // ahead of what opens another app.
    let go = match t {
        Target::Worktree(w) => run_terminal_row(app, w).into_iter().collect(),
        Target::Repo(p) => worktrees_row(app, p).into_iter().collect(),
        _ => Vec::new(),
    };
    if !go.is_empty() {
        let at = items
            .iter()
            .position(|i| past_go(t, i))
            .unwrap_or(items.len());
        items.splice(at..at, go);
    }
    if let Some(checkout) = checkout_row(app, t, &items) {
        items.push(checkout);
    }
    items
}

/// Is `item` past the Go section of `t`'s menu: a row of a later section,
/// or a submenu of them (`Open in ▸`, `Copy ▸`, `Move ▸`).
fn past_go(t: &Target, item: &MenuItem) -> bool {
    let first = match &item.action {
        MenuAction::Submenu { items, .. } => items.first(),
        _ => Some(item),
    };
    first
        .and_then(id_of)
        .and_then(|id| nebula_config::actions::section_of(id, t.kind()))
        .is_some_and(|s| s > nebula_config::actions::MenuSection::Go)
}

/// **Show run terminal** on a checkout whose run command's terminal is
/// alive: the way from the dev server to the process behind it.
fn run_terminal_row(app: &App, w: &WorktreeId) -> Option<MenuItem> {
    let term = app
        .tree
        .terminals
        .iter()
        .find(|x| &x.worktree_id == w && x.alive && x.run_command.is_some())?;
    Some(MenuItem::new(
        "Show run terminal",
        MenuAction::Attach(SessionRef::Terminal(term.id.clone())),
    ))
}

/// **Worktrees ▸** on a repo with linked worktrees: one row each, landing
/// on its band, the jump list the desktop's repo menu gives too.
fn worktrees_row(app: &App, p: &ProjectId) -> Option<MenuItem> {
    let rows: Vec<MenuItem> = app
        .tree
        .worktrees
        .iter()
        .filter(|w| &w.project_id == p && !w.is_main && !app.is_placeholder_worktree(&w.id))
        .map(|w| {
            MenuItem::new(
                w.branch.clone(),
                MenuAction::Jump(crate::palette::PaletteTarget::Worktree(w.id.clone())),
            )
        })
        .collect();
    (!rows.is_empty()).then(|| submenu("Worktrees", rows))
}

/// What a session's stop note (lane/limits) puts in its menu's banner:
/// `limited · resumes 3:45pm` while Claude waits a limit out, `needs you ·
/// log in again (work)` for a login to renew, `needs you · billing` or
/// `error · overloaded` for the rest.
fn stop_words(note: &nebula_core::stop::StopNote, a: &nebula_core::Agent) -> String {
    use nebula_core::stop::StopTone;
    let now = crate::app::now_ms() / 1000;
    match note.tone() {
        StopTone::Waiting => format!("limited \u{b7} {}", note.label(now)),
        StopTone::NeedsYou if logs_in(note) => format!(
            "needs you \u{b7} log in again ({})",
            nebula_core::accounts::session_account(a).trim_start_matches('@')
        ),
        StopTone::NeedsYou => format!("needs you \u{b7} {}", note.label(now)),
        StopTone::Error => format!("error \u{b7} {}", note.label(now)),
    }
}

/// Did the turn stop on credentials only a fresh login renews?
fn logs_in(note: &nebula_core::stop::StopNote) -> bool {
    matches!(
        note.reason.as_str(),
        nebula_core::stop::AUTHENTICATION_FAILED | "cloud_credential_error"
    )
}

/// **Log in to ‹account›…** on a session whose turn stopped on its login
/// (matrix §5.1, state-only, ahead of Open): the account's sign-in, in a
/// terminal, as the accounts overlay's `l` runs it.
fn login_row(app: &App, id: &AgentId) -> Option<MenuItem> {
    let a = app
        .tree
        .agents
        .iter()
        .find(|a| &a.id == id && !a.archived)?;
    if !app.launcher.session_stops.get(id).is_some_and(logs_in) {
        return None;
    }
    let account = nebula_core::accounts::session_account(a);
    Some(MenuItem::new(
        format!("Log in to {}\u{2026}", account.trim_start_matches('@')),
        MenuAction::LogIn(account),
    ))
}

/// **Open parent session ‹name›** on a session another one spawned, while
/// that one is live.
fn parent_row(app: &App, id: &AgentId) -> Option<MenuItem> {
    let a = app
        .tree
        .agents
        .iter()
        .find(|a| &a.id == id && !a.archived)?;
    let parent = app
        .tree
        .agents
        .iter()
        .find(|p| a.parent.as_ref() == Some(&p.id) && !p.archived)?;
    Some(MenuItem::new(
        format!("Open parent session {}", parent.name),
        MenuAction::Attach(SessionRef::Agent(parent.id.clone())),
    ))
}

/// A live card's `Worktree ‹branch› ▸` (`Checkout ‹branch› ▸` on the
/// root): its checkout's menu, flat, less every verb the card's own rows
/// already offer. Move up and down stay, as the checkout's (`Move
/// worktree up`), and so does Mark all seen, which takes more than the
/// card. None on an archived card, which runs in no checkout, or with
/// nothing left to list.
fn checkout_row(app: &App, t: &Target, card: &[MenuItem]) -> Option<MenuItem> {
    let live = match t {
        Target::Session(id) => app.tree.agents.iter().any(|a| &a.id == id && !a.archived),
        Target::Terminal(_) => true,
        _ => return None,
    };
    if !live {
        return None;
    }
    let w = worktree_of(app, t)?;
    let checkout = app.tree.worktrees.iter().find(|x| x.id == w)?;
    // A Cloud card's agent runs in the cloud sandbox: no editor for the
    // checkout it is listed under.
    let cloud = match t {
        Target::Session(id) => app
            .tree
            .agents
            .iter()
            .any(|a| &a.id == id && a.cloud_session_url().is_some()),
        _ => false,
    };
    let shown = flat(card);
    let mut items = Vec::new();
    for mut item in rows(
        app,
        menu_of(AppliesTo::Worktree),
        &Target::Worktree(w.clone()),
    ) {
        if cloud && matches!(item.action, MenuAction::Editor(_)) {
            continue;
        }
        if shown.iter().any(|i| i.verb == item.verb) {
            match item.verb {
                Some(Action::ReorderUp) => item.label = "Move worktree up".into(),
                Some(Action::ReorderDown) => item.label = "Move worktree down".into(),
                Some(Action::MarkSeen) => {}
                _ => continue,
            }
        }
        items.push(item);
    }
    if let Some(run) = run_terminal_row(app, &w) {
        let on = Target::Worktree(w.clone());
        let at = items
            .iter()
            .position(|i| past_go(&on, i))
            .unwrap_or(items.len());
        items.insert(at, run);
    }
    if items.is_empty() {
        return None;
    }
    let what = if checkout.is_main {
        "Checkout"
    } else {
        "Worktree"
    };
    Some(submenu(format!("{what} {}", checkout.branch), items))
}

/// What a menu's title says after the thing's name while the thing is in
/// a notable state (the matrix's banner, §1.7): a session waiting on you,
/// running, done and unread, disconnected, archived, the orchestrator; a
/// checkout whose pull request merged, on its base branch, serving.
pub(crate) fn banner(app: &App, t: &Target) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    match t {
        Target::Session(id) => {
            let a = app.tree.agents.iter().find(|a| &a.id == id)?;
            use nebula_core::AgentStatus as S;
            if a.archived {
                words.push("archived".into());
            } else {
                // Why its last turn stopped short says more than its dot:
                // a limit it waits out, a login to renew, an API error.
                let stop = app.launcher.session_stops.get(id);
                match (stop, a.status) {
                    (Some(note), _) => words.push(stop_words(note, a)),
                    (None, S::NeedsFeedback) => words.push("needs you".into()),
                    (None, S::Running) => words.push("running".into()),
                    (None, S::Finished) if a.unseen => words.push("done \u{b7} unread".into()),
                    (None, S::Disconnected) => {
                        words.push("disconnected \u{b7} restart to resume".into())
                    }
                    (None, S::Terminated) => words.push("stopped".into()),
                    _ => {}
                }
                if a.role.is_orchestrator() {
                    words.push("orchestrator".into());
                }
            }
        }
        Target::Worktree(w) => {
            match crate::launcher::worktree_pr(app, w).map(|pr| pr.standing) {
                Some(crate::pull_request::Standing::Merged) => words.push("merged".into()),
                Some(crate::pull_request::Standing::Closed) => words.push("closed".into()),
                _ => {}
            }
            if app
                .jobs
                .worktree_base
                .get(w)
                .is_some_and(|b| b.on_default_branch)
            {
                words.push("on the base branch".into());
            }
            if let Some(port) = port_of(app, t) {
                words.push(format!("running :{port}"));
            }
        }
        _ => {}
    }
    (!words.is_empty()).then(|| words.join(" \u{b7} "))
}

/// A right-click menu's title: the thing's name and its banner, only
/// while there is a banner to show (a plain card's menu has none).
pub(crate) fn menu_title(app: &App, t: &Target) -> Option<String> {
    let banner = banner(app, t)?;
    Some(format!("{} \u{b7} {banner}", name(app, t)))
}

/// The verbs on `t` its menu leaves out that still mean something here
/// (`place`), as their keys run them: a pull request commented on, the
/// subagents, a file found. Not the cursor's walks: those are keys, not
/// verbs. Nothing that cannot act now (matrix §8.7): no Ghostty tab off
/// macOS, no checkout verbs on an archived card.
fn more_rows(app: &App, t: &Target, shown: &[MenuItem]) -> Vec<MenuItem> {
    let place = place(app);
    let live = match t {
        Target::Session(id) => app.tree.agents.iter().any(|a| &a.id == id && !a.archived),
        _ => true,
    };
    let kinds = match t {
        Target::Session(_) | Target::Terminal(_) if live => vec![t.kind(), AppliesTo::Worktree],
        _ => vec![t.kind()],
    };
    let shown = flat(shown);
    let mut out = Vec::new();
    for kind in &kinds {
        let on = if *kind == t.kind() {
            t.clone()
        } else {
            match worktree_of(app, t) {
                Some(w) => Target::Worktree(w),
                None => continue,
            }
        };
        for spec in crate::keymap::ACTIONS {
            if spec.group == "NAVIGATE"
                || !spec.applies_to.contains(kind)
                || !spec.works_in.contains(&place)
                || matches!(spec.action, Action::Verbs | Action::Help)
                || (spec.action == Action::OpenGhosttyTab
                    && (!cfg!(target_os = "macos") || app.chrome.is_remote))
                || shown
                    .iter()
                    .chain(&out)
                    .any(|i: &MenuItem| i.verb == Some(spec.action))
            {
                continue;
            }
            if let Some(item) = row(app, spec.id, &on) {
                // The editors are the menu's to offer (an archived or Cloud
                // card offers none for the checkout it sits under).
                if !matches!(item.action, MenuAction::Editor(_)) {
                    out.push(item);
                }
            }
        }
    }
    out
}

/// The app's own verbs, where nothing is selected: the views and the
/// boxes, run as their keys run them.
fn global_rows(app: &App) -> Vec<MenuItem> {
    let place = place(app);
    crate::keymap::ACTIONS
        .iter()
        .filter(|s| {
            s.applies_to.contains(&AppliesTo::Global)
                && s.works_in.contains(&place)
                && s.group != "NAVIGATE"
                && crate::palette::is_command(s.action)
                && !matches!(s.action, Action::Verbs | Action::Help)
        })
        .map(|s| {
            MenuItem::new(
                menu_label(s.id, AppliesTo::Global),
                MenuAction::Run(s.action),
            )
            .keyed(s.action)
        })
        .collect()
}

/// What a verbs menu's title names: the thing its verbs act on.
pub(crate) fn name(app: &App, t: &Target) -> String {
    let t = match t {
        Target::Repo(id) => app
            .tree
            .projects
            .iter()
            .find(|p| &p.id == id)
            .map(|p| p.name.clone()),
        Target::Worktree(id) => app
            .tree
            .worktrees
            .iter()
            .find(|w| &w.id == id)
            .map(|w| w.branch.clone()),
        Target::Session(id) => app
            .tree
            .agents
            .iter()
            .find(|a| &a.id == id)
            .map(|a| a.name.clone()),
        Target::Terminal(id) => app
            .tree
            .terminals
            .iter()
            .find(|x| &x.id == id)
            .map(|x| x.name.clone()),
        Target::File { rel, .. } | Target::Folder { rel, .. } => Some(rel.clone()),
        Target::Account(key) => Some(key.clone()),
        Target::Global => None,
    };
    t.unwrap_or_default()
}

/// The **All keys…** row every verbs menu ends on: the full help.
fn all_keys() -> MenuItem {
    MenuItem::new("All keys\u{2026}", MenuAction::AllKeys).keyed(Action::Help)
}

/// A verbs menu over `items`: titled, type-ahead (into the submenus as
/// well), **All keys…** last.
pub(crate) fn verbs_menu(title: String, mut items: Vec<MenuItem>) -> ContextMenu {
    items.push(all_keys());
    ContextMenu {
        title: Some(title),
        filter: Some(MenuFilter {
            query: String::new(),
            all: items.clone(),
        }),
        items,
        at: None,
        hover: 0,
        area: ratatui::layout::Rect::default(),
        scroll: 0,
        parent: None,
    }
}

/// The VERBS MENU for the selection: its menu's rows, then one `More ▸`
/// with the rest of what applies to it here, then **All keys…**. On a
/// pull request's, an issue's or a link's row, that row's own menu; with
/// nothing selected, every verb of the app's own, flat, to search.
pub(crate) fn selection_menu(app: &App) -> ContextMenu {
    let Some(t) = selection(app) else {
        return match super::menus::selected_row_menu(app) {
            Some((title, items)) => verbs_menu(title, items),
            None => verbs_menu("Verbs".into(), global_rows(app)),
        };
    };
    let mut items = menu_for(app, &t);
    let more = more_rows(app, &t, &items);
    if !more.is_empty() {
        items.push(submenu("More", more));
    }
    let what = match t.kind() {
        AppliesTo::Repo => "project",
        AppliesTo::Worktree => "checkout",
        AppliesTo::Terminal => "terminal",
        _ => "session",
    };
    let title = match banner(app, &t) {
        Some(banner) => format!("{what} {} \u{b7} {banner}", name(app, &t)),
        None => format!("{what} {}", name(app, &t)),
    };
    verbs_menu(title, items)
}

/// `?`: the VERBS MENU for the selection, centered over the grid.
pub(crate) fn open(app: &mut App) {
    app.modals.verbs_under = None;
    app.modals.overlay = Some(Overlay::Menu(selection_menu(app)));
    app.chrome.dirty = true;
}

/// The verbs of the selection a footer and the palette lead with: the
/// first `n` of its kind's [`PRIMARY`](nebula_config::actions::PRIMARY)
/// list that mean something on it now — the app's own with nothing
/// selected — with the words the footer prints. `keyed`: only verbs
/// with a key bound, for a footer that names the key.
pub(crate) fn primary(app: &App, n: usize, keyed: bool) -> Vec<(Action, &'static str)> {
    let bound = |a: Action| !keyed || app.chrome.keymap.first(a).is_some();
    let Some(t) = selection(app) else {
        let place = place(app);
        return primary_of(AppliesTo::Global)
            .iter()
            .filter_map(|id| nebula_config::actions::spec_by_id(id))
            .filter(|s| s.works_in.contains(&place) && bound(s.action))
            .take(n)
            .map(|s| (s.action, s.word))
            .collect();
    };
    let mut out = Vec::new();
    let tries = |kind: AppliesTo, on: &Target, out: &mut Vec<(Action, &'static str)>| {
        for id in primary_of(kind) {
            if out.len() >= n {
                return;
            }
            let Some(spec) = nebula_config::actions::spec_by_id(id) else {
                continue;
            };
            if out.iter().any(|(a, _)| *a == spec.action) || !bound(spec.action) {
                continue;
            }
            if menu_action(app, spec.action, on).is_some() {
                out.push((spec.action, spec.word));
            }
        }
    };
    tries(t.kind(), &t, &mut out);
    // A card short of verbs (an archived one) fills from its checkout.
    if let (true, Some(w)) = (out.len() < n, worktree_of(app, &t)) {
        if matches!(t, Target::Session(_) | Target::Terminal(_)) {
            tries(AppliesTo::Worktree, &Target::Worktree(w), &mut out);
        }
    }
    out
}

/// A key bound to one of the menus' own verbs (Restart, Run, Delete
/// worktree, Mark seen, the copies…), or the palette's row of one: the
/// verb on the selection — or on the checkout or project it is in, where
/// the verb is theirs — as its menu row would run it; with nothing
/// selected, the app's own (Mark all seen on what the board shows).
pub(crate) fn run_on_selection(app: &mut App, action: Action, out: &mut Vec<ClientRequest>) {
    let mut targets = Vec::new();
    match selection(app) {
        Some(sel) => {
            targets.push(sel.clone());
            if let Some(w) = worktree_of(app, &sel).filter(|_| !matches!(sel, Target::Worktree(_)))
            {
                targets.push(Target::Worktree(w));
            }
            if let Some(p) = project_of(app, &sel).filter(|_| !matches!(sel, Target::Repo(_))) {
                targets.push(Target::Repo(p));
            }
        }
        None => targets.push(Target::Global),
    }
    let found = targets
        .iter()
        .find_map(|t| match menu_action(app, action, t) {
            // The key itself: running it again would come straight back here.
            Some(MenuAction::Run(_)) | None => None,
            Some(m) => Some(m),
        });
    match found {
        Some(m) => run_menu_action(app, m, out),
        None => flash_nothing(app, action),
    }
}

/// A studio verb on a key (0.60): on the selection, its checkout or its
/// project where it means something there, else on the app itself.
pub(crate) fn run_studio_verb(app: &mut App, action: Action, out: &mut Vec<ClientRequest>) {
    let mut targets = Vec::new();
    if let Some(sel) = selection(app) {
        targets.push(sel.clone());
        if let Some(p) = project_of(app, &sel).filter(|_| !matches!(sel, Target::Repo(_))) {
            targets.push(Target::Repo(p));
        }
    }
    targets.push(Target::Global);
    let found = targets
        .iter()
        .find_map(|t| match menu_action(app, action, t) {
            Some(MenuAction::Run(_)) | None => None,
            Some(m) => Some(m),
        });
    match found {
        Some(m) => run_menu_action(app, m, out),
        None => flash_nothing(app, action),
    }
}

fn flash_nothing(app: &mut App, action: Action) {
    let label = crate::keymap::spec_of(action).map_or("that", |s| s.label);
    app.chrome.flash = Some(format!("{label}: not for what is under the cursor"));
}

/// A row-addressed verb ([`MenuAction::Verb`]), run on its target: the
/// one dispatcher for the verbs with no older action of their own,
/// however their row was reached — a right-click, `?`, the palette or a
/// key the user bound (through [`run_on_selection`]).
pub(crate) fn run_verb(app: &mut App, action: Action, t: &Target, out: &mut Vec<ClientRequest>) {
    match action {
        Action::MarkSeen => mark_seen(app, t, out),
        Action::ArchiveDone => archive_done(app, t),
        // The browser chooser on the target's checkout, as `w` opens it
        // on the cursor's.
        Action::OpenInBrowser => match checkout_of(app, t) {
            Some(w) => crate::localhost::open_chooser_for(app, &w),
            None => flash_nothing(app, action),
        },
        // **Account details…**: the accounts overlay on that account.
        Action::Accounts => match t {
            Target::Account(key) => super::accounts::open_on(app, key),
            _ => super::accounts::open(app),
        },
        // A file in this app's own reader, the one Enter in the finder
        // opens.
        Action::OpenFileInKete => match t {
            Target::File { worktree, rel } => super::open_file_in(app, worktree, rel),
            _ => flash_nothing(app, action),
        },
        // lane/editors': on the checkout, folder or file `t` names, by its
        // rules (`editor_act`); never back through the key, which comes
        // here.
        Action::OpenInVscode
        | Action::OpenInVisualStudio
        | Action::DockEditor
        | Action::RevealInExplorer
        | Action::OpenWithDefault => match editor_act(app, action, t) {
            Some(m) => run_menu_action(app, m, out),
            None => flash_nothing(app, action),
        },
        // lane/wake's: Wake up now on the row's own account (the picked
        // one, else the Wake view to pick one), its schedule on that
        // account or, from the background, on all of them.
        Action::WakeAccount => match t {
            Target::Account(key) => super::accounts::wake_account_key(app, key),
            _ => super::accounts::wake_account_verb(app),
        },
        Action::WakeSchedule => match t {
            Target::Account(key) => super::accounts::open_wake_tab_on(app, key),
            _ => super::accounts::open_wake_tab(app),
        },
        // 0.60: each studio verb to its lane's module.
        Action::StudioBoard => crate::studio_board::open(app, repo_of(t)),
        Action::GauntletView => crate::gauntlet_view::open(app, repo_of(t)),
        Action::TriageNote => crate::triage_strip::open(app),
        Action::StudioAutonomy => crate::studio_queue_view::open_autonomy(app, repo_of(t)),
        Action::RenewOrchestrator => crate::renew_view::open(app),
        Action::UsageCsv => crate::ledger_export::run(app),
        _ => flash_nothing(app, action),
    }
}

/// The repo a studio verb's row was on; None from the background.
fn repo_of(t: &Target) -> Option<nebula_core::ProjectId> {
    match t {
        Target::Repo(id) => Some(id.clone()),
        _ => None,
    }
}

/// The sessions a bulk verb on `t` takes, those `pick` admits: the one
/// session, a checkout's, a repo's, or what the background shows.
fn sessions_in(
    app: &App,
    t: &Target,
    pick: fn(&nebula_core::Agent) -> bool,
) -> Vec<nebula_core::Agent> {
    app.tree
        .agents
        .iter()
        .filter(|a| pick(a))
        .filter(|a| match t {
            Target::Session(id) => &a.id == id,
            Target::Worktree(w) => &a.worktree_id == w,
            Target::Repo(p) => project_has(app, p, a),
            Target::Global => in_background(app, a),
            _ => false,
        })
        .cloned()
        .collect()
}

/// MARK SEEN: the session's finished turn read, or every unread one in the
/// checkout, the repo or what the board shows — the review lane emptied
/// without opening each card.
fn mark_seen(app: &mut App, t: &Target, out: &mut Vec<ClientRequest>) {
    let unread = sessions_in(app, t, unread);
    if unread.is_empty() {
        app.chrome.flash = Some("nothing unread here".into());
        return;
    }
    for a in &unread {
        super::mark_agent_seen(app, &a.id, out);
    }
    app.chrome.flash = Some(match unread.as_slice() {
        [one] => format!("marked {} seen", one.name),
        many => format!("marked {} seen", count_of(many.len(), "session")),
    });
    app.chrome.dirty = true;
}

/// ARCHIVE DONE SESSIONS: the finished sessions already read, in the
/// checkout, the repo or what the board shows, after one confirm listing
/// them — never one still running, waiting on you or unread, nor the
/// orchestrator. `u` brings any back.
fn archive_done(app: &mut App, t: &Target) {
    let done = sessions_in(app, t, done);
    if done.is_empty() {
        app.chrome.flash = Some("no finished, read session to archive here".into());
        return;
    }
    let names: Vec<String> = done.iter().map(|a| a.name.clone()).collect();
    app.modals.overlay = Some(Overlay::Confirm(ConfirmDialog {
        title: format!("Archive done \u{b7} {}", count_of(names.len(), "session")),
        message: format!(
            "Archive these finished, read sessions? u brings any back.\n{}",
            bulk_confirm_listing(&names)
        ),
        action: PendingAction::ArchiveThread {
            agents: done.into_iter().map(|a| a.id).collect(),
            terminals: Vec::new(),
        },
        area: ratatui::layout::Rect::default(),
    }));
    app.chrome.dirty = true;
}

/// The COMMAND PALETTE's first rows: the selection's own menu verbs, by
/// their menu words and the thing's name (`Archive · agent-1`), each run
/// as its key runs on the selection.
pub(crate) fn palette_verbs(app: &App) -> Vec<(Action, String)> {
    let Some(t) = selection(app) else {
        return Vec::new();
    };
    let name = name(app, &t);
    rows(app, menu_of(t.kind()), &t)
        .into_iter()
        .filter_map(|item| Some((item.verb?, format!("{} \u{b7} {name}", item.label))))
        .collect()
}

/// A row showing `key` that presses it in the overlay the menu stands on
/// ([`MenuAction::OverlayKey`]).
pub(crate) fn overlay_key_row(
    label: &str,
    code: KeyCode,
    mods: KeyModifiers,
    destructive: bool,
) -> MenuItem {
    MenuItem {
        label: label.into(),
        action: MenuAction::OverlayKey(code, mods),
        destructive,
        verb: None,
        key: Some(KeyChord { code, mods }.display()),
    }
}
